//! On-demand room settings. Explicit writes retain their runtime-owned room.
use super::{Composer, RuntimeOwned};
use matrix_sdk::{
    RoomState,
    notification_settings::RoomNotificationMode,
    ruma::events::{
        StateEventType,
        room::{
            history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent},
            join_rules::{JoinRule, RoomJoinRulesEventContent},
        },
    },
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Notifications {
    #[default]
    Default,
    All,
    Mentions,
    Mute,
}
impl Notifications {
    fn from_mode(mode: RoomNotificationMode) -> Self {
        match mode {
            RoomNotificationMode::AllMessages => Self::All,
            RoomNotificationMode::MentionsAndKeywordsOnly => Self::Mentions,
            RoomNotificationMode::Mute => Self::Mute,
        }
    }
}
impl std::fmt::Display for Notifications {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Default => "Use account default",
            Self::All => "All messages",
            Self::Mentions => "Mentions & keywords",
            Self::Mute => "Mute room",
        })
    }
}
/// One account-scoped push-rule view, retained across local edits until sync catches up.
#[derive(Clone)]
pub struct Control(Arc<RuntimeOwned<matrix_sdk::notification_settings::NotificationSettings>>);
impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RoomSettingsControl")
    }
}
#[derive(Clone, Debug)]
pub struct Settings {
    pub control: Option<Control>,
    pub name: String,
    pub topic: String,
    pub notifications: Notifications,
    pub default_notifications: Notifications,
    pub access: Option<JoinRule>,
    pub history: HistoryVisibility,
    pub encryption: String,
    pub editable: [bool; 4],
}
#[derive(Clone, Debug)]
pub enum Change {
    Name(String, String),
    Topic(String, String),
    Notifications(Notifications, Notifications),
    Access(Option<JoinRule>, JoinRule),
    History(HistoryVisibility, HistoryVisibility),
}
impl Composer {
    pub async fn room_settings(self, control: Option<Control>) -> Result<Settings, String> {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.read_room_settings(control),
        )
        .await
        .map_err(|_| "Loading room settings timed out. Try Reload.".to_owned())?
    }
    async fn read_room_settings(&self, control: Option<Control>) -> Result<Settings, String> {
        let room = self.0.room();
        if room.state() != RoomState::Joined {
            return Err("You are no longer joined to this room.".into());
        }
        let levels = room.power_levels().await.map_err(|e| e.to_string())?;
        let control = match control {
            Some(control) => control,
            None => Control(RuntimeOwned::with_runtime(
                room.client().notification_settings().await,
                self.0.runtime.clone(),
            )),
        };
        let notifications = &control.0;
        let encryption = room.encryption_state();
        let default = notifications
            .get_default_room_notification_mode(
                encryption.is_encrypted().into(),
                (room.active_members_count() == 2).into(),
            )
            .await;
        Ok(Settings {
            control: Some(control.clone()),
            name: room.name().unwrap_or_default(),
            topic: room.topic().unwrap_or_default(),
            notifications: notifications
                .get_user_defined_room_notification_mode(room.room_id())
                .await
                .map(Notifications::from_mode)
                .unwrap_or_default(),
            default_notifications: Notifications::from_mode(default),
            access: room.join_rule(),
            history: room.history_visibility_or_default(),
            encryption: if encryption.is_unknown() {
                "Unknown"
            } else if encryption.is_encrypted() {
                "End-to-end encrypted"
            } else {
                "Not encrypted"
            }
            .into(),
            editable: [
                StateEventType::RoomName,
                StateEventType::RoomTopic,
                StateEventType::RoomJoinRules,
                StateEventType::RoomHistoryVisibility,
            ]
            .map(|event| levels.user_can_send_state(room.own_user_id(), event)),
        })
    }
    /// Compare the edited field with current synced state and recheck permissions.
    /// Matrix has no compare-and-swap for state events; remote races remain possible.
    pub async fn change_room_setting(
        self,
        change: Change,
        control: Option<Control>,
    ) -> Result<(), String> {
        let update = async {
            let current = self.read_room_settings(control).await?;
            let room = self.0.room();
            let (unchanged, allowed) = match &change {
                Change::Name(before, _) => (current.name == *before, current.editable[0]),
                Change::Topic(before, _) => (current.topic == *before, current.editable[1]),
                Change::Notifications(before, _) => (current.notifications == *before, true),
                Change::Access(before, _) => (current.access == *before, current.editable[2]),
                Change::History(before, _) => (current.history == *before, current.editable[3]),
            };
            if !allowed {
                return Err("You no longer have permission to change this setting.".into());
            }
            if !unchanged {
                return Err("This setting changed. Reload before saving again.".into());
            }
            match change {
                Change::Name(_, value) => {
                    room.set_name(value).await.map_err(|e| e.to_string())?;
                }
                Change::Topic(_, value) => {
                    room.set_room_topic(&value)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                Change::Access(_, value) => {
                    if !matches!(current.access, Some(JoinRule::Invite | JoinRule::Public))
                        || !matches!(value, JoinRule::Invite | JoinRule::Public)
                    {
                        return Err(
                            "This room uses an access rule that this editor cannot change.".into(),
                        );
                    }
                    room.send_state_event(RoomJoinRulesEventContent::new(value))
                        .await
                        .map_err(|e| e.to_string())?;
                }
                Change::History(_, value) => {
                    room.send_state_event(RoomHistoryVisibilityEventContent::new(value))
                        .await
                        .map_err(|e| e.to_string())?;
                }
                Change::Notifications(_, value) => {
                    let control = current.control.as_ref().expect("loaded control");
                    let settings = &control.0;
                    if value == Notifications::Default {
                        settings
                            .delete_user_defined_room_rules(room.room_id())
                            .await
                            .map_err(|e| e.to_string())?;
                    } else {
                        let mode = match value {
                            Notifications::All => RoomNotificationMode::AllMessages,
                            Notifications::Mentions => {
                                RoomNotificationMode::MentionsAndKeywordsOnly
                            }
                            Notifications::Mute => RoomNotificationMode::Mute,
                            Notifications::Default => unreachable!(),
                        };
                        settings
                            .set_room_notification_mode(room.room_id(), mode)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            Ok(())
        };
        tokio::time::timeout(std::time::Duration::from_secs(30), update)
            .await
            .map_err(|_| {
                "Saving timed out and may have succeeded. Reload to check before retrying."
                    .to_owned()
            })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{Client, authentication::matrix::MatrixSession, config::SyncSettings};
    use matrix_sdk_ui::timeline::TimelineBuilder;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path, path_regex},
    };

    async fn fixture(power: u32) -> (MockServer, Composer) {
        fixture_with_access(power, "invite").await
    }
    async fn fixture_with_access(power: u32, access: &str) -> (MockServer, Composer) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(json!({
                    "user_id":"@test:example.org", "device_id":"TEST", "access_token":"fake"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        let event = |kind: &str, key: &str, content: serde_json::Value| {
            json!({
                "type":kind, "state_key":key, "content":content, "sender":"@test:example.org",
                "event_id":format!("${kind}"), "origin_server_ts":1
            })
        };
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "next_batch":"one", "rooms":{"join":{"!room:example.org":{
                    "state":{"events":[
                        event("m.room.create", "", json!({"creator":"@test:example.org"})),
                        event("m.room.member", "@test:example.org", json!({"membership":"join"})),
                        event("m.room.name", "", json!({"name":"Original"})),
                        event("m.room.topic", "", json!({"topic":"Topic"})),
                        event("m.room.join_rules", "", json!({"join_rule":access,"allow":[]})),
                        event("m.room.power_levels", "", json!({"users":{"@test:example.org":power}, "state_default":50}))
                    ]}, "timeline":{"events":[], "limited":false}, "account_data":{"events":[]}
                }}}
            }))).mount(&server).await;
        client
            .sync_once(SyncSettings::default().timeout(std::time::Duration::ZERO))
            .await
            .unwrap();
        client.event_cache().subscribe().unwrap();
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap();
        let composer = Composer(RuntimeOwned::new(Arc::new(
            TimelineBuilder::new(&room).build().await.unwrap(),
        )));
        (server, composer)
    }

    #[tokio::test]
    async fn metadata_permissions_conflicts_and_server_errors() {
        let (server, composer) = fixture(100).await;
        let snapshot = composer.clone().room_settings(None).await.unwrap();
        assert_eq!(snapshot.name, "Original");
        assert_eq!(snapshot.topic, "Topic");
        assert_eq!(snapshot.editable, [true; 4]);
        assert_eq!(snapshot.notifications, Notifications::Default);
        assert!(
            composer
                .clone()
                .change_room_setting(
                    Change::Name("Stale".into(), "New".into()),
                    snapshot.control.clone()
                )
                .await
                .unwrap_err()
                .contains("Reload")
        );
        Mock::given(method("PUT"))
            .and(path_regex(".*/state/m.room.name/?"))
            .and(body_json(json!({"name":"New"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$name"})))
            .expect(1)
            .mount(&server)
            .await;
        composer
            .clone()
            .change_room_setting(
                Change::Name("Original".into(), "New".into()),
                snapshot.control.clone(),
            )
            .await
            .unwrap();
        Mock::given(method("PUT"))
            .and(path_regex(".*/state/m.room.history_visibility/?"))
            .and(body_json(json!({"history_visibility":"joined"})))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN", "error":"refused"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            composer
                .change_room_setting(
                    Change::History(snapshot.history, HistoryVisibility::Joined),
                    snapshot.control
                )
                .await
                .is_err()
        );

        let (server, composer) = fixture(0).await;
        let before = server.received_requests().await.unwrap().len();
        let snapshot = composer.clone().room_settings(None).await.unwrap();
        assert_eq!(snapshot.editable, [false; 4]);
        assert!(
            composer
                .change_room_setting(
                    Change::Topic("Topic".into(), "Forbidden".into()),
                    snapshot.control
                )
                .await
                .unwrap_err()
                .contains("permission")
        );
        assert_eq!(server.received_requests().await.unwrap().len(), before);
    }

    #[tokio::test]
    async fn notifications_can_be_muted_then_reset_before_sync_even_without_state_permissions() {
        let (server, composer) = fixture(0).await;
        let snapshot = composer.clone().room_settings(None).await.unwrap();
        Mock::given(method("PUT"))
            .and(path_regex(".*/pushrules/global/override/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path_regex(".*/pushrules/global/override/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        composer
            .clone()
            .change_room_setting(
                Change::Notifications(Notifications::Default, Notifications::Mute),
                snapshot.control.clone(),
            )
            .await
            .unwrap();
        let muted = composer
            .clone()
            .room_settings(snapshot.control)
            .await
            .unwrap();
        assert_eq!(muted.notifications, Notifications::Mute);
        composer
            .clone()
            .change_room_setting(
                Change::Notifications(Notifications::Mute, Notifications::Default),
                muted.control.clone(),
            )
            .await
            .unwrap();
        let restored = composer.clone().room_settings(muted.control).await.unwrap();
        assert_eq!(restored.notifications, Notifications::Default);
        Mock::given(method("PUT"))
            .and(path_regex(".*/pushrules/global/room/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path_regex(".*/pushrules/global/room/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let mut snapshot = restored;
        for mode in [
            Notifications::All,
            Notifications::Mentions,
            Notifications::Default,
        ] {
            composer
                .clone()
                .change_room_setting(
                    Change::Notifications(snapshot.notifications, mode),
                    snapshot.control.clone(),
                )
                .await
                .unwrap();
            snapshot = composer
                .clone()
                .room_settings(snapshot.control)
                .await
                .unwrap();
            assert_eq!(snapshot.notifications, mode);
        }
    }
    #[tokio::test]
    async fn access_writes_preserve_special_rules_and_send_public_only_when_supported() {
        let (server, composer) = fixture(100).await;
        let snapshot = composer.clone().room_settings(None).await.unwrap();
        Mock::given(method("PUT"))
            .and(path_regex(".*/state/m.room.join_rules/?"))
            .and(body_json(json!({"join_rule":"public"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$access"})))
            .expect(1)
            .mount(&server)
            .await;
        composer
            .change_room_setting(
                Change::Access(snapshot.access, JoinRule::Public),
                snapshot.control,
            )
            .await
            .unwrap();
        let (server, composer) = fixture_with_access(100, "restricted").await;
        let snapshot = composer.clone().room_settings(None).await.unwrap();
        let before = server.received_requests().await.unwrap().len();
        assert!(
            composer
                .change_room_setting(
                    Change::Access(snapshot.access, JoinRule::Public),
                    snapshot.control
                )
                .await
                .unwrap_err()
                .contains("cannot change")
        );
        assert_eq!(server.received_requests().await.unwrap().len(), before);
    }
}
