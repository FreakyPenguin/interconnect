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
    /// Once submitted, this write is allowed to finish even if its picker closes.
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
            .and_then(|id| self.0.get_room(id))
            .filter(|room| {
                room.state() == matrix_sdk::RoomState::Joined && room.active_members_count() <= 2
            });
        let room = match cached.or_else(|| {
            self.0
                .get_dm_rooms(&user)
                .find(|room| room.active_members_count() <= 2)
        }) {
            Some(room) => room,
            None => self.0.create_dm(&user).await.map_err(|_| {
                "Could not open the chat. Check your room list before trying again.".to_owned()
            })?,
        };
        opened.insert(user.to_string(), room.room_id().to_owned());
        let _ = self.1.send(());
        Ok(crate::navigation::RoomEntry {
            id: room.room_id().to_string(),
            name: room
                .name()
                .or_else(|| room.cached_display_name().map(|name| name.to_string()))
                .unwrap_or_else(|| user.to_string()),
            direct: true,
            direct_user: Some(user.to_string()),
            member_count: room.joined_members_count(),
            ..Default::default()
        })
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
