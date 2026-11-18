//! On-demand management for joined rooms and spaces without opening a timeline.
use crate::connected::RuntimeOwned;
use matrix_sdk::{
    Room, RoomState,
    ruma::events::{
        StateEventType,
        room::join_rules::{JoinRule, RoomJoinRulesEventContent},
    },
};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Debug)]
pub struct Management(Arc<RuntimeOwned<Room>>);
#[derive(Clone, Debug)]
pub struct Settings {
    pub name: String,
    pub topic: String,
    pub access: Option<JoinRule>,
    pub editable: [bool; 3],
}
#[derive(Clone, Debug)]
pub enum Change {
    Name(String, String),
    Topic(String, String),
    Access(Option<JoinRule>, JoinRule),
}
impl Settings {
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Name(_, value) => self.name = value.clone(),
            Change::Topic(_, value) => self.topic = value.clone(),
            Change::Access(_, value) => self.access = Some(value.clone()),
        }
    }
}
impl Management {
    /// Construct on the account's runtime; ownership keeps SDK cleanup runtime-safe.
    pub(crate) fn new(room: Room) -> Self {
        Self(RuntimeOwned::new(room))
    }
    fn joined(&self) -> Result<(), String> {
        if self.0.state() != RoomState::Joined {
            return Err("You are no longer joined to this room or space.".into());
        }
        Ok(())
    }
    async fn read_settings(&self) -> Result<Settings, String> {
        self.joined()?;
        let levels = self
            .0
            .power_levels()
            .await
            .map_err(|_| "Could not check permissions.")?;
        Ok(Settings {
            name: self.0.name().unwrap_or_default(),
            topic: self.0.topic().unwrap_or_default(),
            access: self.0.join_rule(),
            editable: [
                StateEventType::RoomName,
                StateEventType::RoomTopic,
                StateEventType::RoomJoinRules,
            ]
            .map(|event| levels.user_can_send_state(self.0.own_user_id(), event)),
        })
    }
    pub async fn settings(self) -> Result<Settings, String> {
        tokio::time::timeout(Duration::from_secs(30), self.read_settings())
            .await
            .map_err(|_| "Loading settings timed out. Try Reload.".to_owned())?
    }
    /// Compare against synced state and recheck permissions before each explicit write.
    /// Matrix has no state compare-and-swap; a remote race remains possible.
    pub async fn save(self, change: Change) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(30), async {
            let settings = self.read_settings().await?;
            let (unchanged, allowed) = match &change {
                Change::Name(before, _) => (settings.name == *before, settings.editable[0]),
                Change::Topic(before, _) => (settings.topic == *before, settings.editable[1]),
                Change::Access(before, _) => (settings.access == *before, settings.editable[2]),
            };
            if !allowed {
                return Err("You no longer have permission to change this setting.".into());
            }
            if !unchanged {
                return Err("This setting changed. Reload before saving again.".into());
            }
            match change {
                Change::Name(_, value) => {
                    if value.trim().is_empty() || value.len() > 255 {
                        return Err("Enter a name within 255 bytes.".into());
                    }
                    self.0
                        .set_name(value)
                        .await
                        .map_err(|_| "Could not save the name.")?;
                }
                Change::Topic(_, value) => {
                    self.0
                        .set_room_topic(&value)
                        .await
                        .map_err(|_| "Could not save the topic.")?;
                }
                Change::Access(_, value) => {
                    if !matches!(settings.access, Some(JoinRule::Invite | JoinRule::Public))
                        || !matches!(value, JoinRule::Invite | JoinRule::Public)
                    {
                        return Err("This access rule cannot be changed with this editor.".into());
                    }
                    self.0
                        .send_state_event(RoomJoinRulesEventContent::new(value))
                        .await
                        .map_err(|_| "Could not save access rules.")?;
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| "Saving timed out. Reload to check the result before retrying.".to_owned())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{Client, authentication::matrix::MatrixSession, config::SyncSettings};
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path, path_regex},
    };

    async fn fixture(power: u32, access: &str) -> (MockServer, Management) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(
                    json!({"user_id":"@test:example.org","device_id":"TEST","access_token":"fake"}),
                )
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        let event = |kind: &str, key: &str, content: Value| json!({"type":kind,"state_key":key,"content":content,"sender":"@test:example.org","event_id":format!("${kind}-{key}"),"origin_server_ts":1});
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!space:example.org":{
                "state":{"events":[event("m.room.create", "", json!({"creator":"@test:example.org","type":"m.space"})),
                    event("m.room.member", "@test:example.org", json!({"membership":"join"})),
                    event("m.room.name", "", json!({"name":"Original"})),
                    event("m.room.topic", "", json!({"topic":"Topic"})),
                    event("m.room.join_rules", "", json!({"join_rule":access})),
                    event("m.room.power_levels", "", json!({"users":{"@test:example.org":power},"state_default":50}))]},
                "timeline":{"events":[],"limited":false},"account_data":{"events":[]}
            }}}}))).mount(&server).await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let control = crate::people::Directory::new(client)
            .space_management("!space:example.org".into())
            .await
            .unwrap();
        (server, control)
    }
    #[tokio::test]
    async fn space_metadata_rechecks_permissions_conflicts_and_special_access_rules() {
        let (server, control) = fixture(100, "invite").await;
        let settings = control.clone().settings().await.unwrap();
        assert_eq!(settings.editable, [true; 3]);
        assert_eq!(settings.name, "Original");
        assert!(
            control
                .clone()
                .save(Change::Name("Stale".into(), "New".into()))
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
        control
            .clone()
            .save(Change::Name("Original".into(), "New".into()))
            .await
            .unwrap();
        Mock::given(method("PUT"))
            .and(path_regex(".*/state/m.room.topic/?"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"refused"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            control
                .save(Change::Topic("Topic".into(), "New".into()))
                .await
                .is_err()
        );
        for (power, access) in [(0, "invite"), (100, "restricted")] {
            let (server, control) = fixture(power, access).await;
            let settings = control.clone().settings().await.unwrap();
            let before = server.received_requests().await.unwrap().len();
            assert!(
                control
                    .save(Change::Access(settings.access, JoinRule::Public))
                    .await
                    .is_err()
            );
            assert_eq!(server.received_requests().await.unwrap().len(), before);
        }
    }
}
