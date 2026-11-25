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
    pub avatar: crate::room_avatar::Details,
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
    pub async fn save_avatar(
        self,
        baseline: Option<matrix_sdk::ruma::OwnedMxcUri>,
        upload: Option<crate::room_avatar::Upload>,
    ) -> Result<Option<matrix_sdk::ruma::OwnedMxcUri>, String> {
        crate::room_avatar::save(&self.0, baseline, upload).await
    }

    pub async fn prepare_space_exit(
        self,
        action: crate::space_exit::Action,
    ) -> Result<crate::space_exit::Plan, String> {
        crate::space_exit::prepare(&self.0, action).await
    }
    pub async fn exit_space(
        self,
        plan: crate::space_exit::Plan,
    ) -> Result<crate::space_exit::Outcome, String> {
        crate::space_exit::execute(&self.0, plan).await
    }
    pub async fn retry_forgetting_space(self) -> crate::space_exit::Outcome {
        crate::space_exit::retry_forget(&self.0).await
    }
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
            avatar: crate::room_avatar::Details {
                url: self.0.avatar_url(),
                editable: levels
                    .user_can_send_state(self.0.own_user_id(), StateEventType::RoomAvatar),
            },
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
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut settings = self.read_settings().await?;
            settings.avatar.url = crate::room_avatar::current(&self.0).await?;
            Ok(settings)
        })
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
        fixture_kind(power, access, true).await
    }
    async fn fixture_kind(power: u32, access: &str, space: bool) -> (MockServer, Management) {
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
                "state":{"events":[event("m.room.create", "", if space { json!({"creator":"@test:example.org","type":"m.space"}) } else { json!({"creator":"@test:example.org"}) }),
                    event("m.room.member", "@test:example.org", json!({"membership":"join"})),
                    event("m.room.member", "@other:example.org", json!({"membership":"join","displayname":"Other"})),
                    event("m.room.member", "@peer:example.org", json!({"membership":"join"})),
                    event("m.room.member", "@pending:example.org", json!({"membership":"invite"})),
                    event("m.room.member", "@banned:example.org", json!({"membership":"ban","reason":"Repeated spam"})),
                    event("m.room.member", "@departed:example.org", json!({"membership":"leave"})),
                    event("m.room.name", "", json!({"name":"Original"})),
                    event("m.room.topic", "", json!({"topic":"Topic"})),
                    event("m.room.join_rules", "", json!({"join_rule":access})),
                    event("m.room.power_levels", "", json!({"users":{"@test:example.org":power,"@peer:example.org":100},"state_default":50,"events":{"m.room.topic":60},"notifications":{"room":75}}))]},
                "timeline":{"events":[],"limited":false},"account_data":{"events":[]}
            }}}}))).mount(&server).await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let chunk: Vec<_> = [
            ("test", "join"),
            ("other", "join"),
            ("peer", "join"),
            ("pending", "invite"),
            ("banned", "ban"),
            ("departed", "leave"),
        ]
        .into_iter()
        .map(|(user, membership)| {
            event(
                "m.room.member",
                &format!("@{user}:example.org"),
                json!({"membership":membership}),
            )
        })
        .collect();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/rooms/!space:example.org/members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":chunk})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.avatar/?"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"No avatar"})),
            )
            .mount(&server)
            .await;
        let directory = crate::people::Directory::new(client);
        let control = if space {
            directory
                .space_management("!space:example.org".into())
                .await
        } else {
            directory.room_management("!space:example.org".into()).await
        }
        .unwrap();
        (server, control)
    }
    async fn avatar_upload() -> crate::room_avatar::Upload {
        let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        image::DynamicImage::new_rgba8(2, 3)
            .save(file.path())
            .unwrap();
        crate::room_avatar::read_image(file.path().to_owned())
            .await
            .unwrap()
    }
    async fn avatar_permissions(server: &MockServer, power: u32) {
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.power_levels/?"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"users":{"@test:example.org":power}})),
            )
            .mount(server)
            .await;
    }
    async fn avatar_writer(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path_regex(".*/media/.*config"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"m.upload.size": 10485760})),
            )
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(".*/media/.*upload"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"content_uri":"mxc://example.org/new"})),
            )
            .mount(server)
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(".*/state/m.room.avatar/?"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$avatar"})))
            .mount(server)
            .await;
    }
    #[tokio::test]
    async fn avatar_upload_and_removal_publish_only_confirmed_image_metadata() {
        let (server, control) = fixture(100, "invite").await;
        avatar_permissions(&server, 100).await;
        avatar_writer(&server).await;
        let settings = control.clone().settings().await.unwrap();
        assert!(settings.avatar.editable);
        assert!(settings.avatar.url.is_none());
        let upload = avatar_upload().await;
        let result = control.clone().save_avatar(None, Some(upload)).await;
        assert!(
            result.is_ok(),
            "{result:?}; requests: {:?}",
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .map(|r| (r.method.to_string(), r.url.path().to_owned()))
                .collect::<Vec<_>>()
        );
        let url = result.unwrap().unwrap();
        assert_eq!(url.as_str(), "mxc://example.org/new");
        let requests = server.received_requests().await.unwrap();
        let writes: Vec<_> = requests.iter().filter(|r| r.method == "PUT").collect();
        assert_eq!(writes.len(), 1);
        let content: Value = serde_json::from_slice(&writes[0].body).unwrap();
        assert_eq!(content["url"], "mxc://example.org/new");
        assert_eq!(content["info"]["mimetype"], "image/png");
        assert_eq!(content["info"]["w"], 2);
        assert_eq!(content["info"]["h"], 3);
        let media = requests
            .iter()
            .find(|r| r.method == "POST" && r.url.path().contains("/media/"))
            .unwrap();
        assert_eq!(
            image::guess_format(&media.body).unwrap(),
            image::ImageFormat::Png
        );
        assert_eq!(content["info"]["size"], media.body.len());
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.avatar/?"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"url":url})))
            .with_priority(1)
            .mount(&server)
            .await;
        assert_eq!(
            control.clone().settings().await.unwrap().avatar.url,
            Some(url.clone())
        );
        assert!(
            control
                .save_avatar(Some(url), None)
                .await
                .unwrap()
                .is_none()
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.method == "POST" && r.url.path().contains("/media/"))
                .count(),
            1
        );
        let last = requests.iter().rfind(|r| r.method == "PUT").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&last.body).unwrap(),
            json!({"url":null})
        );
    }
    #[tokio::test]
    async fn avatar_permission_and_baseline_checks_prevent_any_writes() {
        let (server, control) = fixture(0, "invite").await;
        avatar_permissions(&server, 0).await;
        assert!(!control.clone().settings().await.unwrap().avatar.editable);
        assert!(
            control
                .save_avatar(None, Some(avatar_upload().await))
                .await
                .unwrap_err()
                .contains("permission")
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| r.method == "GET"
                    || (!r.url.path().contains("/state/") && !r.url.path().contains("/media/")))
        );
        let (server, control) = fixture(100, "invite").await;
        avatar_permissions(&server, 100).await;
        assert!(
            control
                .save_avatar(Some("mxc://example.org/stale".into()), None)
                .await
                .unwrap_err()
                .contains("changed")
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| r.method == "GET"
                    || (!r.url.path().contains("/state/") && !r.url.path().contains("/media/")))
        );
    }
    #[tokio::test]
    async fn avatar_remote_edit_during_upload_prevents_publication() {
        let (server, control) = fixture(100, "invite").await;
        avatar_permissions(&server, 100).await;
        avatar_writer(&server).await;
        let reads = std::sync::atomic::AtomicUsize::new(0);
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.avatar/?"))
            .respond_with(move |_: &wiremock::Request| {
                let content = if reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    json!({})
                } else {
                    json!({"url":"mxc://example.org/remote"})
                };
                ResponseTemplate::new(200).set_body_json(content)
            })
            .with_priority(1)
            .mount(&server)
            .await;
        assert!(
            control
                .save_avatar(None, Some(avatar_upload().await))
                .await
                .unwrap_err()
                .contains("changed")
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.method == "POST" && r.url.path().contains("/media/"))
                .count(),
            1
        );
        assert!(requests.iter().all(|r| r.method != "PUT"));
    }
    #[tokio::test]
    async fn avatar_permission_revocation_during_upload_prevents_publication() {
        let (server, control) = fixture(100, "invite").await;
        avatar_writer(&server).await;
        let reads = std::sync::atomic::AtomicUsize::new(0);
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.power_levels/?"))
            .respond_with(move |_: &wiremock::Request| {
                let power = if reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    100
                } else {
                    0
                };
                ResponseTemplate::new(200)
                    .set_body_json(json!({"users":{"@test:example.org":power}}))
            })
            .mount(&server)
            .await;
        assert!(
            control
                .save_avatar(None, Some(avatar_upload().await))
                .await
                .unwrap_err()
                .contains("permission")
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.method == "POST" && r.url.path().contains("/media/"))
                .count(),
            1
        );
        assert!(requests.iter().all(|r| r.method != "PUT"));
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
    #[tokio::test]
    async fn shared_member_editor_lists_members_checks_boundaries_and_preserves_power_events() {
        let (server, control) = fixture(100, "invite").await;
        let roster = control.clone().members().await.unwrap();
        assert!(roster.can_invite);
        assert_eq!(roster.rows.len(), 4);
        let other = roster
            .rows
            .iter()
            .find(|m| m.member.user_id == "@other:example.org")
            .unwrap();
        assert!(other.can_remove);
        assert!(other.roles.contains(&Role::Admin));
        let peer = roster
            .rows
            .iter()
            .find(|m| m.member.user_id == "@peer:example.org")
            .unwrap();
        assert!(!peer.can_remove);
        assert!(peer.roles.is_empty());
        let before = server.received_requests().await.unwrap().len();
        for change in [
            MemberChange::Remove {
                user: "@test:example.org".into(),
                invited: false,
            },
            MemberChange::Remove {
                user: "@peer:example.org".into(),
                invited: false,
            },
            MemberChange::Remove {
                user: "@pending:example.org".into(),
                invited: false,
            },
            MemberChange::Role {
                user: "@other:example.org".into(),
                before: UserPowerLevel::Int(matrix_sdk::ruma::int!(50)),
                role: Role::Admin,
            },
            MemberChange::Role {
                user: "@peer:example.org".into(),
                before: UserPowerLevel::Int(matrix_sdk::ruma::int!(100)),
                role: Role::Member,
            },
            MemberChange::Invite("@pending:example.org".into()),
            MemberChange::Invite("@banned:example.org".into()),
        ] {
            assert!(control.clone().change_member(change).await.is_err());
        }
        assert_eq!(server.received_requests().await.unwrap().len(), before);
        Mock::given(method("PUT")).and(path_regex(".*/state/m.room.power_levels/?"))
            .and(wiremock::matchers::body_partial_json(json!({"users":{"@other:example.org":50,"@test:example.org":100,"@peer:example.org":100},"events":{"m.room.topic":60},"notifications":{"room":75}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$roles"}))).expect(1).mount(&server).await;
        control
            .clone()
            .change_member(MemberChange::Role {
                user: "@other:example.org".into(),
                before: other.power,
                role: Role::Moderator,
            })
            .await
            .unwrap();
        Mock::given(method("POST"))
            .and(path_regex(".*/kick"))
            .and(body_json(json!({"user_id":"@pending:example.org"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        control
            .clone()
            .change_member(MemberChange::Remove {
                user: "@pending:example.org".into(),
                invited: true,
            })
            .await
            .unwrap();
        Mock::given(method("POST"))
            .and(path_regex(".*/invite"))
            .and(body_json(json!({"user_id":"@new:example.org"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        control
            .clone()
            .change_member(MemberChange::Invite("@new:example.org".into()))
            .await
            .unwrap();
        Mock::given(method("POST"))
            .and(path_regex(".*/kick"))
            .and(body_json(json!({"user_id":"@other:example.org"})))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"refused"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            control
                .change_member(MemberChange::Remove {
                    user: "@other:example.org".into(),
                    invited: false
                })
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn moderators_cannot_assign_above_their_power_and_read_only_users_cannot_remove() {
        for actor in [0, 50] {
            let (server, control) = fixture(actor, "invite").await;
            let roster = control.clone().members().await.unwrap();
            let other = roster
                .rows
                .iter()
                .find(|m| m.member.user_id == "@other:example.org")
                .unwrap();
            assert!(!other.roles.contains(&Role::Admin));
            let before = server.received_requests().await.unwrap().len();
            assert!(
                control
                    .clone()
                    .change_member(MemberChange::Role {
                        user: "@other:example.org".into(),
                        before: other.power,
                        role: Role::Admin
                    })
                    .await
                    .is_err()
            );
            if actor == 0 {
                assert!(
                    control
                        .change_member(MemberChange::Remove {
                            user: "@other:example.org".into(),
                            invited: false
                        })
                        .await
                        .is_err()
                );
            }
            assert_eq!(server.received_requests().await.unwrap().len(), before);
        }
    }
    #[tokio::test]
    async fn moderation_reasons_and_authoritative_ban_details_cover_rooms_and_spaces() {
        for space in [false, true] {
            let (server, control) = fixture_kind(100, "invite", space).await;
            let roster = control.clone().members().await.unwrap();
            let details = roster.banned[0].details.as_ref().unwrap();
            assert_eq!(details.actor.as_deref(), Some("@test:example.org"));
            assert_eq!(details.timestamp_ms, Some(1));
            assert_eq!(details.reason.as_deref(), Some("Repeated spam"));
            for (action, user, change) in [
                (
                    "kick",
                    "@other:example.org",
                    MemberChange::Remove {
                        user: "@other:example.org".into(),
                        invited: false,
                    },
                ),
                (
                    "ban",
                    "@other:example.org",
                    MemberChange::Ban {
                        user: "@other:example.org".into(),
                        invited: false,
                    },
                ),
                (
                    "ban",
                    "@unknown:example.org",
                    MemberChange::BanId("@unknown:example.org".into()),
                ),
                (
                    "unban",
                    "@banned:example.org",
                    MemberChange::Unban("@banned:example.org".into()),
                ),
            ] {
                let request = Mock::given(method("POST"))
                    .and(path(format!(
                        "/_matrix/client/v3/rooms/!space:example.org/{action}"
                    )))
                    .and(body_json(json!({"user_id":user,"reason":"Spam"})))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                    .expect(1)
                    .mount_as_scoped(&server)
                    .await;
                assert!(
                    control
                        .clone()
                        .change_member_with_reason(
                            change.clone(),
                            "x".repeat(crate::moderation::MAX_REASON_BYTES + 1)
                        )
                        .await
                        .is_err()
                );
                control
                    .clone()
                    .change_member_with_reason(change, "  Spam  ".into())
                    .await
                    .unwrap();
                drop(request);
            }
        }
    }
    #[tokio::test]
    async fn redacted_ban_reasons_and_times_remain_unavailable() {
        for space in [false, true] {
            let (server, control) = fixture_kind(100, "invite", space).await;
            Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"redacted","rooms":{"join":{"!space:example.org":{
                    "state":{"events":[{"type":"m.room.member","state_key":"@banned:example.org","sender":"@moderator:example.org","event_id":"$ban-redacted","origin_server_ts":2,"content":{"membership":"ban"},"unsigned":{"redacted_because":{"type":"m.room.redaction","event_id":"$redact","sender":"@test:example.org","origin_server_ts":3,"redacts":"$ban-redacted","content":{}}}}]},
                    "timeline":{"events":[],"limited":false}
                }}}}))).with_priority(1).mount(&server).await;
            control
                .0
                .client()
                .sync_once(SyncSettings::default().timeout(Duration::ZERO))
                .await
                .unwrap();
            let roster = control.members().await.unwrap();
            let details = roster.banned[0].details.as_ref().unwrap();
            assert_eq!(details.actor.as_deref(), Some("@moderator:example.org"));
            assert!(details.reason.is_none());
            assert!(details.timestamp_ms.is_none());
        }
    }
    #[tokio::test]
    async fn ban_by_id_covers_departed_and_unknown_people_in_rooms_and_spaces() {
        for space in [false, true] {
            for power in [0, 50, 100] {
                let (server, control) = fixture_kind(power, "invite", space).await;
                let roster = control.clone().members().await.unwrap();
                assert_eq!(roster.can_ban, power >= 50);
                for user in [
                    "invalid",
                    "@test:example.org",
                    "@peer:example.org",
                    "@banned:example.org",
                ] {
                    assert!(
                        control
                            .clone()
                            .change_member(MemberChange::BanId(user.into()))
                            .await
                            .is_err()
                    );
                }
                for user in [
                    "@departed:example.org",
                    "@unknown:elsewhere.org",
                    "@other:example.org",
                ] {
                    let request = Mock::given(method("POST"))
                        .and(path_regex(".*/ban"))
                        .and(body_json(json!({"user_id":user})))
                        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                        .expect(if power >= 50 { 1 } else { 0 })
                        .mount_as_scoped(&server)
                        .await;
                    assert_eq!(
                        control
                            .clone()
                            .change_member(MemberChange::BanId(user.into()))
                            .await
                            .is_ok(),
                        power >= 50
                    );
                    drop(request);
                }
                assert!(
                    !server.received_requests().await.unwrap().iter().any(|r| r
                        .url
                        .path()
                        .ends_with("/invite")
                        || r.url.path().contains("/send/"))
                );
            }
        }
    }
    #[tokio::test]
    async fn bans_and_unbans_check_membership_and_power_in_rooms_and_spaces() {
        for space in [false, true] {
            for actor in [0, 50, 100] {
                let (server, control) = fixture_kind(actor, "invite", space).await;
                let roster = control.clone().members().await.unwrap();
                let other = roster
                    .rows
                    .iter()
                    .find(|m| m.member.user_id == "@other:example.org")
                    .unwrap();
                assert_eq!(other.can_ban, actor >= 50);
                assert_eq!(roster.banned.len(), 1);
                assert_eq!(roster.banned[0].user_id, "@banned:example.org");
                assert_eq!(roster.banned[0].can_unban, actor >= 50);
                for change in [
                    MemberChange::Ban {
                        user: "@test:example.org".into(),
                        invited: false,
                    },
                    MemberChange::Ban {
                        user: "@peer:example.org".into(),
                        invited: false,
                    },
                    MemberChange::Ban {
                        user: "@pending:example.org".into(),
                        invited: false,
                    },
                    MemberChange::Unban("@other:example.org".into()),
                    MemberChange::Unban("invalid".into()),
                ] {
                    assert!(control.clone().change_member(change).await.is_err());
                }
                for (action, user, change) in [
                    (
                        "ban",
                        "@other:example.org",
                        MemberChange::Ban {
                            user: "@other:example.org".into(),
                            invited: false,
                        },
                    ),
                    (
                        "ban",
                        "@pending:example.org",
                        MemberChange::Ban {
                            user: "@pending:example.org".into(),
                            invited: true,
                        },
                    ),
                    (
                        "unban",
                        "@banned:example.org",
                        MemberChange::Unban("@banned:example.org".into()),
                    ),
                ] {
                    let response = Mock::given(method("POST"))
                        .and(path(format!(
                            "/_matrix/client/v3/rooms/!space:example.org/{action}"
                        )))
                        .and(body_json(json!({"user_id":user})))
                        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                        .expect(if actor >= 50 { 1 } else { 0 })
                        .mount_as_scoped(&server)
                        .await;
                    assert_eq!(
                        control.clone().change_member(change).await.is_ok(),
                        actor >= 50
                    );
                    drop(response);
                }
                Mock::given(method("POST"))
                    .and(path_regex(".*/ban"))
                    .respond_with(
                        ResponseTemplate::new(500)
                            .set_body_json(json!({"errcode":"M_UNKNOWN","error":"fixture"})),
                    )
                    .expect(if actor >= 50 { 1 } else { 0 })
                    .mount(&server)
                    .await;
                assert!(
                    control
                        .change_member(MemberChange::Ban {
                            user: "@other:example.org".into(),
                            invited: false
                        })
                        .await
                        .is_err()
                );
            }
        }
    }
}

/// Shared invitation checks for conversation invitations and space management.
pub(crate) async fn invite(room: &Room, input: &str) -> Result<(), String> {
    use matrix_sdk::ruma::events::room::member::MembershipState;
    let user = crate::people::parse_user_id(input)?;
    tokio::time::timeout(Duration::from_secs(30), async {
        let levels = room.power_levels().await.map_err(|e| e.to_string())?;
        if room.state() != RoomState::Joined || !levels.user_can_invite(room.own_user_id()) {
            return Err(
                "You no longer have permission to invite people to this room or space.".into(),
            );
        }
        if let Some(member) = room
            .get_member_no_sync(&user)
            .await
            .map_err(|e| e.to_string())?
        {
            match member.membership() {
                MembershipState::Join => return Err("This person is already a room member.".into()),
                MembershipState::Invite => {
                    return Err("This person already has an invitation.".into());
                }
                MembershipState::Ban => return Err("This person is banned from the room.".into()),
                _ => {}
            }
        }
        room.invite_user_by_id(&user)
            .await
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| {
        "The invitation timed out and may have been sent. Check membership before retrying."
            .to_owned()
    })?
}

use matrix_sdk::ruma::{OwnedUserId, events::room::power_levels::UserPowerLevel};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Member,
    Moderator,
    Admin,
}
impl Role {
    fn power(self) -> matrix_sdk::ruma::Int {
        match self {
            Self::Member => matrix_sdk::ruma::int!(0),
            Self::Moderator => matrix_sdk::ruma::int!(50),
            Self::Admin => matrix_sdk::ruma::int!(100),
        }
    }
}
impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Member => "Member",
            Self::Moderator => "Moderator",
            Self::Admin => "Admin",
        })
    }
}
#[derive(Clone, Debug)]
pub struct ManagedMember {
    pub member: crate::connected::Member,
    pub power: UserPowerLevel,
    pub can_remove: bool,
    pub can_ban: bool,
    pub roles: Vec<Role>,
}
#[derive(Clone, Debug)]
pub struct BannedMember {
    pub user_id: String,
    pub name: String,
    pub can_unban: bool,
    pub details: Option<BanDetails>,
}
#[derive(Clone, Debug)]
pub struct BanDetails {
    pub actor: Option<String>,
    pub timestamp_ms: Option<i64>,
    pub reason: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Roster {
    pub rows: Vec<ManagedMember>,
    pub banned: Vec<BannedMember>,
    pub can_invite: bool,
    pub can_ban: bool,
    pub can_unban: bool,
}
#[derive(Clone, Debug)]
pub enum MemberChange {
    Invite(String),
    Remove {
        user: String,
        invited: bool,
    },
    Ban {
        user: String,
        invited: bool,
    },
    BanId(String),
    Unban(String),
    Role {
        user: String,
        before: UserPowerLevel,
        role: Role,
    },
}
impl Management {
    pub async fn members(self) -> Result<Roster, String> {
        tokio::time::timeout(Duration::from_secs(30), async {
            self.joined()?;
            let rows = crate::connected::members::load(&self.0).await?;
            let levels = self
                .0
                .power_levels()
                .await
                .map_err(|_| "Could not check member permissions.")?;
            let own = self.0.own_user_id();
            let mut managed = Vec::with_capacity(rows.len());
            for member in rows {
                let user = member
                    .user_id
                    .parse::<OwnedUserId>()
                    .map_err(|_| "Invalid member ID.")?;
                let power = levels.for_user(&user);
                let other = user != own;
                let roles = [Role::Member, Role::Moderator, Role::Admin]
                    .into_iter()
                    .filter(|role| {
                        other
                            && !member.invited
                            && levels.user_can_change_user_power_level(own, &user)
                            && levels.for_user(own) >= role.power()
                            && power != role.power()
                    })
                    .collect();
                managed.push(ManagedMember {
                    can_remove: other && levels.user_can_kick_user(own, &user),
                    can_ban: other && levels.user_can_ban_user(own, &user),
                    member,
                    power,
                    roles,
                });
            }
            let mut banned: Vec<_> = self
                .0
                .members(matrix_sdk::RoomMemberships::BAN)
                .await
                .map_err(|_| "Could not load banned members.")?
                .into_iter()
                .map(|m| BannedMember {
                    user_id: m.user_id().to_string(),
                    name: m.name().to_owned(),
                    can_unban: m.user_id() != own && levels.user_can_unban_user(own, m.user_id()),
                    details: Some(BanDetails {
                        actor: Some(m.event().sender().to_string()),
                        timestamp_ms: m
                            .event()
                            .timestamp()
                            .and_then(|ts| i64::try_from(u64::from(ts)).ok()),
                        reason: m.event().reason().map(ToOwned::to_owned),
                    }),
                })
                .collect();
            banned.sort_by_cached_key(|m| (m.name.to_lowercase(), m.user_id.clone()));
            Ok(Roster {
                banned,
                rows: managed,
                can_invite: levels.user_can_invite(own),
                can_ban: levels.user_can_ban(own),
                can_unban: levels.user_can_unban(own),
            })
        })
        .await
        .map_err(|_| "Loading members timed out. Try Reload.".to_owned())?
    }
    pub async fn change_member(self, change: MemberChange) -> Result<(), String> {
        self.change_member_with_reason(change, String::new()).await
    }
    pub async fn change_member_with_reason(
        self,
        change: MemberChange,
        input: String,
    ) -> Result<(), String> {
        let reason = crate::moderation::reason(&input)?;
        if reason.is_some() && matches!(change, MemberChange::Invite(_) | MemberChange::Role { .. })
        {
            return Err("Reasons apply to removals, bans, and unbans.".into());
        }
        if let MemberChange::Invite(input) = change {
            return invite(&self.0, &input).await;
        }
        tokio::time::timeout(Duration::from_secs(30), async {
            use matrix_sdk::ruma::events::room::{
                member::MembershipState, power_levels::RoomPowerLevelsEventContent,
            };
            self.joined()?;
            let input = match &change {
                MemberChange::Remove { user, .. }
                | MemberChange::Ban { user, .. }
                | MemberChange::BanId(user)
                | MemberChange::Unban(user)
                | MemberChange::Role { user, .. } => user,
                _ => unreachable!(),
            };
            let user = crate::people::parse_user_id(input)?;
            let own = self.0.own_user_id();
            if user == own {
                return Err("Use room or space settings to manage your own membership.".into());
            }
            if matches!(change, MemberChange::BanId(_)) {
                self.0
                    .sync_members()
                    .await
                    .map_err(|_| "Could not check membership before banning. Try Reload.")?;
            }
            let member = self
                .0
                .get_member_no_sync(&user)
                .await
                .map_err(|_| "Could not check membership.")?;
            if member.is_none() && !matches!(change, MemberChange::BanId(_)) {
                return Err("This person is no longer a member. Reload.".into());
            }
            let mut levels = self
                .0
                .power_levels()
                .await
                .map_err(|_| "Could not check permissions.")?;
            match change {
                MemberChange::Remove { invited, .. } => {
                    let expected = if invited {
                        MembershipState::Invite
                    } else {
                        MembershipState::Join
                    };
                    if member
                        .as_ref()
                        .is_none_or(|member| member.membership() != &expected)
                    {
                        return Err(
                            "Membership changed. Reload before removing this person.".into()
                        );
                    }
                    if !levels.user_can_kick_user(own, &user) {
                        return Err("You no longer have permission to remove this person.".into());
                    }
                    let mut request =
                        matrix_sdk::ruma::api::client::membership::kick_user::v3::Request::new(
                            self.0.room_id().to_owned(),
                            user,
                        );
                    request.reason = reason;
                    self.0
                        .client()
                        .send(request)
                        .with_request_config(
                            matrix_sdk::config::RequestConfig::short_retry().retry_limit(0),
                        )
                        .await
                        .map_err(
                            |_| "Could not confirm the removal. Reload membership before retrying.",
                        )?;
                }
                MemberChange::Ban { invited, .. } => {
                    let expected = if invited {
                        MembershipState::Invite
                    } else {
                        MembershipState::Join
                    };
                    if member
                        .as_ref()
                        .is_none_or(|member| member.membership() != &expected)
                    {
                        return Err("Membership changed. Reload before banning this person.".into());
                    }
                    if !levels.user_can_ban_user(own, &user) {
                        return Err("You no longer have permission to ban this person.".into());
                    }
                    let mut request =
                        matrix_sdk::ruma::api::client::membership::ban_user::v3::Request::new(
                            self.0.room_id().to_owned(),
                            user,
                        );
                    request.reason = reason;
                    self.0
                        .client()
                        .send(request)
                        .with_request_config(
                            matrix_sdk::config::RequestConfig::short_retry().retry_limit(0),
                        )
                        .await
                        .map_err(
                            |_| "Could not confirm the ban. Reload membership before retrying.",
                        )?;
                }
                MemberChange::BanId(_) => {
                    if member
                        .as_ref()
                        .is_some_and(|m| m.membership() == &MembershipState::Ban)
                    {
                        return Err(
                            "This person is already banned. Reload to review their ban.".into()
                        );
                    }
                    if !levels.user_can_ban_user(own, &user) {
                        return Err("You no longer have permission to ban this person.".into());
                    }
                    let mut request =
                        matrix_sdk::ruma::api::client::membership::ban_user::v3::Request::new(
                            self.0.room_id().to_owned(),
                            user,
                        );
                    request.reason = reason;
                    self.0
                        .client()
                        .send(request)
                        .with_request_config(
                            matrix_sdk::config::RequestConfig::short_retry().retry_limit(0),
                        )
                        .await
                        .map_err(
                            |_| "Could not confirm the ban. Reload membership before retrying.",
                        )?;
                }
                MemberChange::Unban(_) => {
                    if member
                        .as_ref()
                        .is_none_or(|member| member.membership() != &MembershipState::Ban)
                    {
                        return Err("This person is no longer banned. Reload.".into());
                    }
                    if !levels.user_can_unban_user(own, &user) {
                        return Err("You no longer have permission to unban this person.".into());
                    }
                    let mut request =
                        matrix_sdk::ruma::api::client::membership::unban_user::v3::Request::new(
                            self.0.room_id().to_owned(),
                            user,
                        );
                    request.reason = reason;
                    self.0
                        .client()
                        .send(request)
                        .with_request_config(
                            matrix_sdk::config::RequestConfig::short_retry().retry_limit(0),
                        )
                        .await
                        .map_err(
                            |_| "Could not confirm the unban. Reload membership before retrying.",
                        )?;
                }
                MemberChange::Role { before, role, .. } => {
                    if member
                        .as_ref()
                        .is_none_or(|member| member.membership() != &MembershipState::Join)
                        || levels.for_user(&user) != before
                    {
                        return Err("Membership or role changed. Reload before saving.".into());
                    }
                    if !levels.user_can_change_user_power_level(own, &user)
                        || levels.for_user(own) < role.power()
                    {
                        return Err("You no longer have permission to assign this role.".into());
                    }
                    levels.users.insert(user, role.power());
                    self.0
                        .send_state_event(
                            RoomPowerLevelsEventContent::try_from(levels)
                                .map_err(|_| "Could not preserve power levels.")?,
                        )
                        .await
                        .map_err(|_| "Could not change this role.")?;
                }
                MemberChange::Invite(_) => unreachable!(),
            }
            Ok(())
        })
        .await
        .map_err(|_| "The member change timed out. Reload membership before retrying.".to_owned())?
    }
}
