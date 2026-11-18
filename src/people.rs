//! Homeserver people search and explicit direct-chat creation.
use crate::connected::RuntimeOwned;
use matrix_sdk::{
    Client,
    ruma::{OwnedMxcUri, OwnedUserId},
};
use std::{sync::Arc, time::Duration};

pub const SEARCH_LIMIT: usize = 20;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub user_id: String,
    pub name: String,
    pub avatar_url: Option<OwnedMxcUri>,
}
#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub people: Vec<Person>,
    pub limited: bool,
}
/// Accept full Matrix IDs even when a homeserver's directory omits the person.
pub fn parse_user_id(input: &str) -> Result<OwnedUserId, String> {
    input.trim().parse().map_err(|_| {
        "Choose a search result or enter a full Matrix ID, such as @name:example.org.".into()
    })
}
#[derive(Clone, Debug)]
pub struct Directory(
    Arc<RuntimeOwned<Client>>,
    tokio::sync::broadcast::Sender<()>,
    Arc<tokio::sync::Mutex<std::collections::HashMap<String, matrix_sdk::ruma::OwnedRoomId>>>,
    Arc<tokio::sync::Mutex<()>>,
);
impl Directory {
    /// Construct inside the client's Tokio runtime; final drop is runtime-safe.
    pub fn new(client: Client) -> Self {
        Self(
            RuntimeOwned::new(client),
            tokio::sync::broadcast::channel(1).0,
            Arc::default(),
            Arc::default(),
        )
    }
    pub(crate) fn from_runtime(client: Client, runtime: tokio::runtime::Handle) -> Self {
        Self(
            RuntimeOwned::with_runtime(client, runtime),
            tokio::sync::broadcast::channel(1).0,
            Arc::default(),
            Arc::default(),
        )
    }
    pub(crate) fn navigation_changes(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.1.subscribe()
    }
    /// Reuse a joined one-to-one DM; otherwise explicitly create an encrypted DM.
    /// Creation is a single attempt; dropping this future cancels pending network work.
    pub async fn open_direct(self, input: String) -> Result<crate::navigation::RoomEntry, String> {
        let user = parse_user_id(&input)?;
        if self.0.user_id() == Some(user.as_ref()) {
            return Err("Choose someone other than yourself.".into());
        }
        // Keep newly created rooms reusable before m.direct returns through sync,
        // and serialize creation for a person across clones of this account handle.
        let mut opened = self.2.lock().await;
        let cached = opened
            .get(user.as_str())
            .filter(|id| {
                self.0.get_room(id).is_none_or(|room| {
                    room.state() == matrix_sdk::RoomState::Joined
                        && room.active_members_count() <= 2
                })
            })
            .cloned();
        let existing = self
            .0
            .get_dm_rooms(&user)
            .find(|room| room.active_members_count() <= 2);
        let id = match cached.or_else(|| existing.as_ref().map(|room| room.room_id().to_owned())) {
            Some(id) => id,
            None => {
                use matrix_sdk::ruma::{
                    api::client::room::create_room::v3::{Request, RoomPreset},
                    events::{InitialStateEvent, room::encryption::RoomEncryptionEventContent},
                };
                let mut request = Request::new();
                request.invite = vec![user.clone()];
                request.is_direct = true;
                request.preset = Some(RoomPreset::TrustedPrivateChat);
                request.initial_state = vec![
                    InitialStateEvent::with_empty_state_key(
                        RoomEncryptionEventContent::with_recommended_defaults(),
                    )
                    .to_raw_any(),
                ];
                let id = crate::room_creation::send_create(&self.0, request).await?;
                // Remember the confirmed ID before ancillary account-data work, so
                // cancellation or a failed m.direct update cannot recreate this room.
                opened.insert(user.to_string(), id.clone());
                let _ = self.1.send(());
                let _ = tokio::time::timeout(
                    Duration::from_secs(10),
                    self.0
                        .account()
                        .mark_as_dm(&id, std::slice::from_ref(&user)),
                )
                .await;
                id
            }
        };
        opened.insert(user.to_string(), id.clone());
        let room = self.0.get_room(&id);
        Ok(crate::navigation::RoomEntry {
            id: id.to_string(),
            name: room
                .as_ref()
                .and_then(|room| {
                    room.name()
                        .or_else(|| room.cached_display_name().map(|name| name.to_string()))
                })
                .unwrap_or_else(|| user.to_string()),
            direct: true,
            direct_user: Some(user.to_string()),
            member_count: room.as_ref().map_or(1, |room| room.joined_members_count()),
            ..Default::default()
        })
    }
    /// Fetch only when mention completion opens; cancellation drops the request.
    pub async fn mention_members(self, room: String) -> Result<Vec<Person>, String> {
        let room: matrix_sdk::ruma::OwnedRoomId =
            room.parse().map_err(|_| "Invalid room".to_owned())?;
        let room = self
            .0
            .get_room(&room)
            .ok_or_else(|| "Room is unavailable".to_owned())?;
        let members = tokio::time::timeout(
            Duration::from_secs(30),
            room.members(matrix_sdk::RoomMemberships::JOIN),
        )
        .await
        .map_err(|_| "Loading room members timed out".to_owned())?
        .map_err(|_| "Could not load room members".to_owned())?;
        Ok(members
            .into_iter()
            .take(4096)
            .map(|member| Person {
                user_id: member.user_id().to_string(),
                name: member.name().to_owned(),
                avatar_url: None,
            })
            .collect())
    }
    pub async fn join_room(self, input: String) -> Result<crate::room_membership::Joined, String> {
        let result = crate::room_membership::join(&self.0, &input).await;
        let _ = self.1.send(());
        result
    }

    pub async fn leave_room(self, room: String) -> Result<(), String> {
        let result = crate::room_membership::leave(&self.0, &room).await;
        let _ = self.1.send(());
        result
    }

    pub async fn respond_to_invitation(
        self,
        room: String,
        accept: bool,
    ) -> Result<Option<crate::room_membership::Joined>, String> {
        let result = crate::room_membership::respond(&self.0, &room, accept).await;
        // Also refresh after an ambiguous timeout: sync may confirm the write.
        let _ = self.1.send(());
        result
    }

    /// Resolve a joined space without opening a timeline or changing selection.
    pub async fn space_management(
        self,
        id: String,
    ) -> Result<crate::room_management::Management, String> {
        let id = id
            .parse::<matrix_sdk::ruma::OwnedRoomId>()
            .map_err(|_| "Choose a valid space.")?;
        let room = self
            .0
            .get_room(&id)
            .filter(|r| r.is_space() && r.state() == matrix_sdk::RoomState::Joined)
            .ok_or("The selected space is no longer available.")?;
        Ok(crate::room_management::Management::new(room))
    }

    pub async fn create_room(
        self,
        options: crate::room_creation::Options,
    ) -> Result<crate::room_creation::Created, String> {
        let result = crate::room_creation::create(&self.0, options).await;
        if result.is_ok() {
            let _ = self.1.send(());
        }
        result
    }
    pub async fn link_created_room(
        self,
        created: crate::room_creation::Created,
    ) -> crate::room_creation::Created {
        let result = crate::room_creation::link(&self.0, created).await;
        let _ = self.1.send(());
        result
    }
    /// Link a joined room to a writable space without changing room membership.
    pub async fn add_room_to_space(self, space: String, room: String) -> Result<(), String> {
        let result = crate::space_management::add_room(&self.0, &space, &room).await;
        let _ = self.1.send(());
        result
    }
    /// Unlink a direct child without leaving either room or editing child-side state.
    pub async fn remove_room_from_space(self, space: String, room: String) -> Result<(), String> {
        let result = crate::space_management::remove_room(&self.0, &space, &room).await;
        let _ = self.1.send(());
        result
    }

    /// Account-owned handle supports unopened rooms; serialize explicit moves until server ACK.
    pub async fn move_room_to_section(
        self,
        room: String,
        destination: String,
    ) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(30), async {
            let _guard = self.3.lock().await;
            crate::room_sections::move_room(&self.0, &room, &destination).await
        })
        .await
        .map_err(|_| "Moving the room timed out. Check its section before retrying.".to_owned())?
    }

    /// Mark an unopened or selected joined room unread without moving read receipts.
    pub async fn mark_room_unread(self, room: String) -> Result<(), String> {
        let id: matrix_sdk::ruma::OwnedRoomId =
            room.parse().map_err(|_| "Invalid room ID".to_owned())?;
        let room = self
            .0
            .get_room(&id)
            .filter(|room| room.state() == matrix_sdk::RoomState::Joined)
            .ok_or_else(|| "This room is no longer joined.".to_owned())?;
        tokio::time::timeout(Duration::from_secs(15), room.set_unread_flag(true))
            .await
            .map_err(|_| "Marking the room unread timed out. Try again.".to_owned())?
            .map_err(|_| "Could not mark the room as unread. Try again.".to_owned())
    }

    /// Serialize settings edits with room moves, against fresh server state.
    pub async fn change_section(self, change: crate::room_sections::Change) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(30), async {
            let _guard = self.3.lock().await;
            crate::room_sections::change_section(&self.0, change).await
        })
        .await
        .map_err(|_| {
            "Saving the section timed out. Check section management before retrying.".to_owned()
        })?
    }

    pub async fn search(self, query: String) -> Result<SearchResults, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(SearchResults::default());
        }
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            self.0.search_users(query, SEARCH_LIMIT as u64),
        )
        .await
        .map_err(|_| "Searching the server directory timed out.".to_owned())?
        .map_err(|_| "Could not search the server's user directory.".to_owned())?;
        let mut seen = std::collections::HashSet::new();
        let limited = result.limited || result.results.len() > SEARCH_LIMIT;
        let people = result
            .results
            .into_iter()
            .filter(|p| seen.insert(p.user_id.clone()))
            .take(SEARCH_LIMIT)
            .map(|p| Person {
                name: p
                    .display_name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| p.user_id.to_string()),
                user_id: p.user_id.to_string(),
                avatar_url: p.avatar_url,
            })
            .collect();
        Ok(SearchResults { people, limited })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::authentication::matrix::MatrixSession;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    async fn direct_fixture(server: &MockServer) -> Client {
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
                serde_json::from_value::<MatrixSession>(json!({
                    "user_id":"@test:example.org","device_id":"TEST","access_token":"fake"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        client
    }

    #[tokio::test]
    async fn mention_members_loads_on_demand_and_returns_only_joined_users() {
        let server = MockServer::start().await;
        let client = direct_fixture(&server).await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "next_batch":"one", "rooms":{"join":{"!room:example.org":{
                    "state":{"events":[]}, "timeline":{"events":[],"limited":false}
                }}}
            })))
            .mount(&server)
            .await;
        client
            .sync_once(matrix_sdk::config::SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        // Classic sync assumes a full roster; reproduce native sliding sync's lazy state.
        client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap()
            .mark_members_missing();
        let member = |user: &str, membership: &str| {
            json!({
                "room_id":"!room:example.org", "type":"m.room.member", "state_key":user, "sender":user, "event_id":format!("${user}"), "origin_server_ts":1,
                "content":{"membership":membership,"displayname":user,"avatar_url":"mxc://example.org/avatar"}
            })
        };
        Mock::given(method("GET")).and(path("/_matrix/client/v3/rooms/!room:example.org/members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[
                member("@test:example.org", "join"), member("@maya:example.org", "join"), member("@invite:example.org", "invite"), member("@left:example.org", "leave")
            ]}))).expect(1).mount(&server).await;
        let directory = Directory::new(client);
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path().ends_with("/members"))
        );
        let people = directory
            .clone()
            .mention_members("!room:example.org".into())
            .await
            .unwrap();
        assert_eq!(people.len(), 2);
        assert!(
            people
                .iter()
                .any(|person| person.user_id == "@maya:example.org")
        );
        assert!(people.iter().all(|person| person.avatar_url.is_none()));
        assert!(
            directory
                .mention_members("!missing:example.org".into())
                .await
                .is_err()
        );
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path().contains("/send/")
                    || request.url.path().ends_with("/createRoom"))
        );
    }
    #[tokio::test]
    async fn marking_unopened_room_unread_writes_only_flag_and_reports_errors() {
        let server = MockServer::start().await;
        let client = direct_fixture(&server).await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "next_batch":"one", "rooms":{"join":{"!room:example.org":{
                    "state":{"events":[]}, "timeline":{"events":[],"limited":false}
                }}}
            })))
            .mount(&server)
            .await;
        client
            .sync_once(matrix_sdk::config::SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let endpoint = "/_matrix/client/v3/user/@test:example.org/rooms/!room:example.org/account_data/m.marked_unread";
        let success = Mock::given(method("PUT"))
            .and(path(endpoint))
            .and(body_json(json!({"unread":true})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        let directory = Directory::new(client);
        directory
            .clone()
            .mark_room_unread("!room:example.org".into())
            .await
            .unwrap();
        drop(success);
        Mock::given(method("PUT"))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "errcode":"M_FORBIDDEN", "error":"fixture"
            })))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            directory
                .clone()
                .mark_room_unread("!room:example.org".into())
                .await
                .is_err()
        );
        assert!(
            directory
                .clone()
                .mark_room_unread("!missing:example.org".into())
                .await
                .is_err()
        );
        assert!(directory.mark_room_unread("invalid".into()).await.is_err());
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path().contains("/receipt/")
                    || request.url.path().contains("/read_markers"))
        );
    }

    #[tokio::test]
    async fn cancelled_dm_metadata_update_keeps_confirmed_room_reusable() {
        let server = MockServer::start().await;
        let client = direct_fixture(&server).await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/createRoom"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"room_id":"!new:example.org"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let metadata_started = Arc::new(tokio::sync::Notify::new());
        let notify = metadata_started.clone();
        Mock::given(method("GET"))
            .and(path(
                "/_matrix/client/v3/user/@test:example.org/account_data/m.direct",
            ))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_body_json(json!({}))
                    .set_delay(Duration::from_secs(30))
            })
            .expect(1)
            .mount(&server)
            .await;
        let directory = Directory::new(client);
        let task = tokio::spawn(directory.clone().open_direct("@peer:example.org".into()));
        tokio::time::timeout(Duration::from_secs(2), metadata_started.notified())
            .await
            .expect("creation should reach metadata update");
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let reused = tokio::time::timeout(
            Duration::from_secs(2),
            directory.open_direct("@peer:example.org".into()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(reused.id, "!new:example.org");
    }

    #[tokio::test]
    async fn direct_creation_server_failure_returns_after_one_attempt() {
        let server = MockServer::start().await;
        let client = direct_fixture(&server).await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/createRoom"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(json!({"errcode":"M_UNKNOWN","error":"ambiguous"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let directory = Directory::new(client);
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            directory.open_direct("@peer:example.org".into()),
        )
        .await
        .expect("DM creation must not retry indefinitely");
        assert!(result.unwrap_err().contains("Check your room list"));
    }

    #[tokio::test]
    async fn direct_chat_is_encrypted_private_and_reused_before_sync() {
        use wiremock::matchers::body_partial_json;
        let server = MockServer::start().await;
        let client = direct_fixture(&server).await;
        Mock::given(method("POST")).and(path("/_matrix/client/v3/createRoom"))
            .and(body_partial_json(json!({"invite":["@peer:example.org"],"is_direct":true,
                "preset":"trusted_private_chat", "initial_state":[{"type":"m.room.encryption", "state_key":"",
                "content":{"algorithm":"m.megolm.v1.aes-sha2"}}]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"room_id":"!new:example.org"})))
            .expect(1).mount(&server).await;
        let account_path = "/_matrix/client/v3/user/@test:example.org/account_data/m.direct";
        Mock::given(method("GET"))
            .and(path(account_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path(account_path))
            .and(body_json(json!({"@peer:example.org":["!new:example.org"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let directory = Directory::new(client);
        let mut navigation = directory.navigation_changes();
        assert!(
            directory
                .clone()
                .open_direct("@test:example.org".into())
                .await
                .is_err()
        );
        assert!(
            directory
                .clone()
                .open_direct("invalid".into())
                .await
                .is_err()
        );
        let room = directory
            .clone()
            .open_direct("@peer:example.org".into())
            .await
            .unwrap();
        assert!(room.direct);
        assert_eq!(room.id, "!new:example.org");
        navigation.recv().await.unwrap();
        let reused = directory
            .open_direct("@peer:example.org".into())
            .await
            .unwrap();
        assert_eq!(reused.id, room.id);
    }

    #[tokio::test]
    async fn direct_chat_reuses_joined_one_to_one_rooms_and_surfaces_refusal() {
        use matrix_sdk::config::SyncSettings;
        let server = MockServer::start().await;
        let client = direct_fixture(&server).await;
        let event = |kind: &str, key: &str, content: serde_json::Value| {
            json!({
                "type":kind,"state_key":key,"sender":"@test:example.org","event_id":format!("${kind}-{key}"),"origin_server_ts":1,"content":content
            })
        };
        let room = |group: bool| {
            let mut events = vec![
                event("m.room.create", "", json!({"creator":"@test:example.org"})),
                event(
                    "m.room.member",
                    "@test:example.org",
                    json!({"membership":"join"}),
                ),
                event(
                    "m.room.member",
                    "@peer:example.org",
                    json!({"membership":"invite"}),
                ),
            ];
            if group {
                events.push(event(
                    "m.room.member",
                    "@third:example.org",
                    json!({"membership":"join"}),
                ));
            }
            json!({"state":{"events":events},"timeline":{"events":[],"limited":false},"account_data":{"events":[]}})
        };
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"next_batch":"one", "rooms":{"join":{
                    "!group:example.org":room(true), "!existing:example.org":room(false)
                }}, "account_data":{"events":[{"type":"m.direct","content":{
                    "@peer:example.org":["!group:example.org","!existing:example.org"]
                }}]}}),
            ))
            .mount(&server)
            .await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let directory = Directory::new(client);
        let room = directory
            .clone()
            .open_direct("@peer:example.org".into())
            .await
            .unwrap();
        assert_eq!(room.id, "!existing:example.org");
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
            directory
                .open_direct("@blocked:example.org".into())
                .await
                .is_err()
        );
    }

    #[test]
    fn raw_ids_work_without_directory_results_and_names_or_emails_are_not_ids() {
        assert_eq!(
            parse_user_id(" @unknown:elsewhere.org ").unwrap().as_str(),
            "@unknown:elsewhere.org"
        );
        for invalid in [
            "",
            "Alice",
            "alice@example.org",
            "@alice",
            "!room:example.org",
        ] {
            assert!(parse_user_id(invalid).is_err(), "{invalid}");
        }
    }
    #[tokio::test]
    async fn directory_sends_bounded_search_preserves_rank_and_profiles_and_reports_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
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
        Mock::given(method("POST")).and(path("/_matrix/client/v3/user_directory/search")).and(body_json(json!({"search_term":"Alice","limit":20}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":[
            {"user_id":"@alice:example.org","display_name":"Alice","avatar_url":"mxc://example.org/alice"},
            {"user_id":"@alice:example.org","display_name":"Duplicate"},
            {"user_id":"@another:example.org","display_name":""}
        ],"limited":true}))).expect(1).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/user_directory/search"))
            .and(body_json(json!({"search_term":"error","limit":20})))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Fixture refusal"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let directory = Directory::new(client);
        assert!(
            directory
                .clone()
                .search("  ".into())
                .await
                .unwrap()
                .people
                .is_empty()
        );
        let result = directory.clone().search(" Alice ".into()).await.unwrap();
        assert!(result.limited);
        assert_eq!(result.people.len(), 2);
        assert_eq!(result.people[0].name, "Alice");
        assert_eq!(
            result.people[0].avatar_url.as_ref().unwrap().as_str(),
            "mxc://example.org/alice"
        );
        assert_eq!(result.people[1].name, "@another:example.org");
        assert!(directory.clone().search("error".into()).await.is_err());
        let started = Arc::new(tokio::sync::Notify::new());
        let notify = started.clone();
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/user_directory/search"))
            .and(body_json(json!({"search_term":"slow","limit":20})))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(60))
                    .set_body_json(json!({"results":[],"limited":false}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let task = tokio::spawn(directory.search("slow".into()));
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}
