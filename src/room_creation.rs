//! Explicit room creation and retryable space linking, independent of the UI.
use crate::navigation::RoomEntry;
use matrix_sdk::{
    Client, RoomState,
    ruma::{
        OwnedRoomId,
        api::client::room::{
            Visibility as DirectoryVisibility,
            create_room::v3::{Request, RoomPreset},
        },
        events::{
            InitialStateEvent, StateEventType,
            room::{
                encryption::RoomEncryptionEventContent,
                history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent},
            },
            space::{child::SpaceChildEventContent, parent::SpaceParentEventContent},
        },
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Private,
    Public,
}
impl std::fmt::Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Private => "Private · invitation only",
            Self::Public => "Public · anyone can join",
        })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum History {
    #[default]
    Joined,
    Invited,
    Shared,
    WorldReadable,
}
impl std::fmt::Display for History {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Joined => "From when members join",
            Self::Invited => "From when members are invited",
            Self::Shared => "All history for members",
            Self::WorldReadable => "Anyone, including non-members",
        })
    }
}
#[derive(Clone, Debug)]
pub struct Options {
    pub name: String,
    pub topic: String,
    pub space: Option<String>,
    pub visibility: Visibility,
    pub encrypted: bool,
    pub history: History,
    pub listed: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            name: String::new(),
            topic: String::new(),
            space: None,
            visibility: Visibility::Private,
            encrypted: true,
            history: History::Joined,
            listed: false,
        }
    }
}
impl Options {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Enter a room name.".into());
        }
        if self.name.len() > 255 {
            return Err("Keep the room name within 255 bytes.".into());
        }
        if self.listed && self.visibility != Visibility::Public {
            return Err("Only public rooms can be listed in the directory.".into());
        }
        if self.history == History::WorldReadable
            && (self.encrypted || self.visibility != Visibility::Public)
        {
            return Err("History for non-members requires a public, unencrypted room.".into());
        }
        if self
            .space
            .as_ref()
            .is_some_and(|id| id.parse::<OwnedRoomId>().is_err())
        {
            return Err("Choose a valid space.".into());
        }
        Ok(())
    }
    fn request(&self, via: matrix_sdk::ruma::OwnedServerName) -> Request {
        let mut request = Request::new();
        request.name = Some(self.name.trim().to_owned());
        request.topic = (!self.topic.trim().is_empty()).then(|| self.topic.trim().to_owned());
        request.preset = Some(match self.visibility {
            Visibility::Private => RoomPreset::PrivateChat,
            Visibility::Public => RoomPreset::PublicChat,
        });
        request.visibility = if self.listed {
            DirectoryVisibility::Public
        } else {
            DirectoryVisibility::Private
        };
        let history = match self.history {
            History::Joined => HistoryVisibility::Joined,
            History::Invited => HistoryVisibility::Invited,
            History::Shared => HistoryVisibility::Shared,
            History::WorldReadable => HistoryVisibility::WorldReadable,
        };
        request.initial_state.push(
            InitialStateEvent::with_empty_state_key(RoomHistoryVisibilityEventContent::new(
                history,
            ))
            .to_raw_any(),
        );
        if self.encrypted {
            request.initial_state.push(
                InitialStateEvent::with_empty_state_key(
                    RoomEncryptionEventContent::with_recommended_defaults(),
                )
                .to_raw_any(),
            );
        }
        if let Some(space) = &self.space {
            let mut parent = SpaceParentEventContent::new(vec![via]);
            parent.canonical = true;
            request.initial_state.push(
                InitialStateEvent::new(
                    space.parse::<OwnedRoomId>().expect("validated space"),
                    parent,
                )
                .to_raw_any(),
            );
        }
        request
    }
}
#[derive(Clone, Debug)]
pub struct Created {
    pub room: RoomEntry,
    pub space: Option<String>,
    pub link_error: Option<String>,
}
async fn writable_space(client: &Client, id: &str) -> Result<matrix_sdk::Room, String> {
    let id = id
        .parse::<OwnedRoomId>()
        .map_err(|_| "Choose a valid space.")?;
    let room = client
        .get_room(&id)
        .filter(|room| room.state() == RoomState::Joined && room.is_space())
        .ok_or("The selected space is no longer available.")?;
    let levels = room
        .power_levels()
        .await
        .map_err(|_| "Could not check permissions for the selected space.")?;
    if !levels.user_can_send_state(room.own_user_id(), StateEventType::SpaceChild) {
        return Err("You do not have permission to add rooms to this space. Choose another space or No space.".into());
    }
    Ok(room)
}
pub(crate) async fn create(client: &Client, options: Options) -> Result<Created, String> {
    options.validate()?;
    let user = client.user_id().ok_or("Sign in before creating a room.")?;
    if let Some(id) = &options.space {
        writable_space(client, id).await?;
    }
    let room = client
        .create_room(options.request(user.server_name().to_owned()))
        .await
        .map_err(|_| {
            "Could not confirm room creation. Check your room list before trying again.".to_owned()
        })?;
    let created = Created {
        room: RoomEntry {
            id: room.room_id().to_string(),
            name: options.name.trim().to_owned(),
            topic: options.topic.trim().to_owned(),
            member_count: 1,
            ..Default::default()
        },
        space: options.space,
        link_error: None,
    };
    Ok(link(client, created).await)
}
pub(crate) async fn link(client: &Client, mut created: Created) -> Created {
    if let Some(space) = &created.space {
        let result = async {
            let parent = writable_space(client, space).await?;
            let user = client.user_id().ok_or("Sign in before adding the room to a space.")?;
            let id = created.room.id.parse::<OwnedRoomId>().map_err(|_| "The created room ID is invalid.")?;
            parent.send_state_event_for_key(&id, SpaceChildEventContent::new(vec![user.server_name().to_owned()])).await
                .map_err(|_| "Could not add the room to the space. You can retry adding it or open the room now.".to_owned())?;
            Ok::<_, String>(())
        }.await;
        created.link_error = result.err();
    }
    created
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, body_partial_json, method, path},
    };

    fn named() -> Options {
        Options {
            name: "  Project room  ".into(),
            topic: "  Planning  ".into(),
            ..Default::default()
        }
    }
    fn state<'a>(body: &'a Value, kind: &str) -> Option<&'a Value> {
        body["initial_state"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["type"] == kind)
            .map(|event| &event["content"])
    }
    fn request_body(options: &Options) -> Value {
        let request = options.request("example.org".parse().unwrap());
        json!({"name":request.name,"topic":request.topic,"preset":request.preset,"visibility":request.visibility,
            "initial_state":request.initial_state,"is_direct":request.is_direct,"invite":request.invite})
    }
    #[test]
    fn defaults_and_every_history_policy_have_explicit_request_semantics() {
        let mut options = named();
        let body = request_body(&options);
        assert_eq!(body["name"], "Project room");
        assert_eq!(body["topic"], "Planning");
        assert_eq!(body["preset"], "private_chat");
        assert_eq!(
            state(&body, "m.room.history_visibility").unwrap()["history_visibility"],
            "joined"
        );
        assert_eq!(
            state(&body, "m.room.encryption").unwrap()["algorithm"],
            "m.megolm.v1.aes-sha2"
        );
        assert_eq!(body["is_direct"], false);
        assert_eq!(body["invite"], json!([]));
        options.visibility = Visibility::Public;
        options.listed = true;
        options.encrypted = false;
        for (history, expected) in [
            (History::Joined, "joined"),
            (History::Invited, "invited"),
            (History::Shared, "shared"),
            (History::WorldReadable, "world_readable"),
        ] {
            options.history = history;
            options.validate().unwrap();
            let body = request_body(&options);
            assert_eq!(body["preset"], "public_chat");
            assert_eq!(body["visibility"], "public");
            assert_eq!(
                state(&body, "m.room.history_visibility").unwrap()["history_visibility"],
                expected
            );
            assert!(state(&body, "m.room.encryption").is_none());
        }
        for bad in [
            Options::default(),
            Options {
                name: "x".repeat(256),
                ..named()
            },
            Options {
                listed: true,
                ..named()
            },
            Options {
                history: History::WorldReadable,
                ..named()
            },
            Options {
                space: Some("invalid".into()),
                ..named()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }
    async fn fixture(server: &MockServer, space_power: Option<u8>) -> Client {
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
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
        if let Some(power) = space_power {
            let event = |kind: &str, key: &str, content: Value| json!({"type":kind,"state_key":key,"sender":"@test:example.org","event_id":format!("${kind}-{key}"),"origin_server_ts":1,"content":content});
            Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!space:example.org":{
                    "state":{"events":[event("m.room.create", "", json!({"creator":"@test:example.org","type":"m.space"})),
                    event("m.room.member", "@test:example.org", json!({"membership":"join"})),
                    event("m.room.power_levels", "", json!({"users":{"@test:example.org":power},"state_default":50}))]},
                    "timeline":{"events":[],"limited":false}, "account_data":{"events":[]}
                }}}}))).mount(server).await;
            client
                .sync_once(SyncSettings::default().timeout(std::time::Duration::ZERO))
                .await
                .unwrap();
        }
        client
    }
    #[tokio::test]
    async fn creation_links_both_sides_and_failed_link_retries_without_recreating() {
        let server = MockServer::start().await;
        let client = fixture(&server, Some(100)).await;
        Mock::given(method("POST")).and(path("/_matrix/client/v3/createRoom"))
            .and(body_partial_json(json!({"name":"Project room","preset":"private_chat","initial_state":[
                {"type":"m.room.history_visibility","state_key":"","content":{"history_visibility":"joined"}},
                {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}},
                {"type":"m.space.parent","state_key":"!space:example.org","content":{"via":["example.org"],"canonical":true}}
            ]}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"room_id":"!new:example.org"})))
            .expect(1).mount(&server).await;
        let failed = Mock::given(method("PUT"))
            .and(path(
                "/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/!new:example.org",
            ))
            .and(body_json(json!({"via":["example.org"]})))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Fixture refusal"})),
            )
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        let options = Options {
            space: Some("!space:example.org".into()),
            ..named()
        };
        let created = create(&client, options).await.unwrap();
        assert_eq!(created.room.id, "!new:example.org");
        assert!(created.link_error.is_some());
        drop(failed);
        Mock::given(method("PUT"))
            .and(path(
                "/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/!new:example.org",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$linked"})))
            .expect(1)
            .mount(&server)
            .await;
        let linked = link(&client, created).await;
        assert!(linked.link_error.is_none());
        assert_eq!(linked.room.id, "!new:example.org");
    }
    #[tokio::test]
    async fn permission_denial_and_missing_space_prevent_creation_and_server_errors_surface() {
        let server = MockServer::start().await;
        let client = fixture(&server, Some(0)).await;
        assert!(
            create(
                &client,
                Options {
                    space: Some("!space:example.org".into()),
                    ..named()
                }
            )
            .await
            .unwrap_err()
            .contains("permission")
        );
        assert!(
            create(
                &client,
                Options {
                    space: Some("!gone:example.org".into()),
                    ..named()
                }
            )
            .await
            .is_err()
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| !r.url.path().ends_with("/createRoom"))
        );
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/createRoom"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Fixture refusal"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            create(&client, named())
                .await
                .unwrap_err()
                .contains("Check your room list")
        );
    }
}
