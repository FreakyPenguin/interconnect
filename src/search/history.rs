//! On-demand backwards pagination, separate from the selected UI timelines.
use super::{Filters, Hit, Service};
use matrix_sdk::{
    RoomState,
    room::MessagesOptions,
    ruma::{
        OwnedRoomId,
        events::{
            AnySyncMessageLikeEvent, AnySyncTimelineEvent,
            room::message::{Relation, SyncRoomMessageEvent},
        },
        uint,
    },
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};

pub const HISTORY_PAGE_SIZE: usize = 50;
#[derive(Clone, Debug)]
struct RoomCursor {
    room: String,
    from: Option<String>,
    seen: HashSet<String>,
    edits: HashMap<String, (String, String)>,
}
#[derive(Clone, Debug, Default)]
pub struct HistoryCursor {
    pending: VecDeque<RoomCursor>,
    pub rooms: usize,
    pub finished: usize,
    pub scanned: usize,
    pub missing_keys: usize,
    pub failed: usize,
}
impl HistoryCursor {
    pub fn complete(&self) -> bool {
        self.pending.is_empty()
    }
}
#[derive(Clone, Debug)]
pub struct HistoryPage {
    pub hits: Vec<Hit>,
    pub cursor: HistoryCursor,
}
impl Service {
    /// Fetch a single page. Dropping this future cancels the current SDK request.
    /// The caller controls continuation and retains only plain pagination state.
    pub async fn history_page(
        self,
        query: String,
        cursor: Option<HistoryCursor>,
    ) -> Result<HistoryPage, String> {
        self.history_page_filtered(query, cursor, Filters::default())
            .await
    }

    pub async fn history_page_filtered(
        self,
        query: String,
        cursor: Option<HistoryCursor>,
        filters: Filters,
    ) -> Result<HistoryPage, String> {
        let mut cursor = cursor.unwrap_or_else(|| {
            let mut rooms: Vec<_> = self
                .0
                .joined_rooms()
                .into_iter()
                .filter(|room| {
                    filters
                        .room
                        .as_deref()
                        .is_none_or(|id| id == room.room_id().as_str())
                })
                .filter(|room| !room.is_space() && !crate::bookmarks::is_bookmarks_room(room))
                .filter(|room| {
                    room.encryption_state().is_encrypted() || room.encryption_state().is_unknown()
                })
                .map(|room| room.room_id().to_string())
                .collect();
            rooms.sort();
            HistoryCursor {
                rooms: rooms.len(),
                pending: rooms
                    .into_iter()
                    .map(|room| RoomCursor {
                        room,
                        from: None,
                        seen: HashSet::new(),
                        edits: HashMap::new(),
                    })
                    .collect(),
                ..Default::default()
            }
        });
        let Some(mut current) = cursor.pending.pop_front() else {
            return Ok(HistoryPage {
                hits: Vec::new(),
                cursor,
            });
        };
        let room = current
            .room
            .parse::<OwnedRoomId>()
            .ok()
            .and_then(|id| self.0.get_room(&id))
            .filter(|room| room.state() == RoomState::Joined);
        let Some(room) = room else {
            cursor.finished += 1;
            return Ok(HistoryPage {
                hits: Vec::new(),
                cursor,
            });
        };
        let mut options = MessagesOptions::backward();
        options.from = current.from.clone();
        options.limit = uint!(50);
        // Filter encrypted envelopes too: filtering only m.room.message would
        // exclude the ciphertext before the SDK has a chance to decrypt it.
        options.filter.types = Some(vec!["m.room.encrypted".into(), "m.room.message".into()]);
        let response = tokio::time::timeout(Duration::from_secs(30), room.messages(options)).await;
        let Ok(Ok(response)) = response else {
            cursor.failed += 1;
            cursor.finished += 1;
            return Ok(HistoryPage {
                hits: Vec::new(),
                cursor,
            });
        };
        if room.state() != RoomState::Joined {
            cursor.finished += 1;
            return Ok(HistoryPage {
                hits: Vec::new(),
                cursor,
            });
        }
        let query = query.trim().to_lowercase();
        let mut hits = Vec::new();
        for event in response.chunk.into_iter().take(HISTORY_PAGE_SIZE) {
            cursor.scanned += 1;
            if event
                .raw()
                .get_field::<String>("type")
                .ok()
                .flatten()
                .as_deref()
                == Some("m.room.encrypted")
            {
                cursor.missing_keys += 1;
            }
            let Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
                SyncRoomMessageEvent::Original(event),
            ))) = event.raw().deserialize()
            else {
                continue;
            };
            let thread_root = match &event.content.relates_to {
                Some(Relation::Thread(thread)) => Some(thread.event_id.to_string()),
                Some(Relation::Replacement(edit)) => {
                    current
                        .edits
                        .entry(edit.event_id.to_string())
                        .or_insert_with(|| {
                            (
                                event.sender.to_string(),
                                edit.new_content.msgtype.body().to_owned(),
                            )
                        });
                    continue;
                }
                _ => None,
            };
            let body = current
                .edits
                .remove(event.event_id.as_str())
                .filter(|(sender, _)| sender == event.sender.as_str())
                .map_or_else(|| event.content.body().to_owned(), |(_, body)| body);
            if query.is_empty() || !body.to_lowercase().contains(&query) {
                continue;
            }
            hits.push(Hit {
                room: current.room.clone(),
                event: event.event_id.to_string(),
                sender: event.sender.to_string(),
                sender_id: event.sender.to_string(),
                body,
                thread_root,
                timestamp_ms: u64::from(event.origin_server_ts.get()).min(i64::MAX as u64) as i64,
            });
        }
        hits.retain(|hit| filters.matches(hit));
        // Tokens may advance through empty pages. Track cycles so a broken
        // homeserver cannot make an explicit scan loop forever.
        if let Some(next) = response.end.filter(|token| {
            Some(token) != current.from.as_ref()
                && token != &response.start
                && !current.seen.contains(token)
        }) {
            current.seen.insert(next.clone());
            current.from = Some(next);
            cursor.pending.push_back(current);
        } else {
            cursor.finished += 1;
        }
        Ok(HistoryPage { hits, cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{
        authentication::matrix::MatrixSession,
        config::{RequestConfig, SyncSettings},
        ruma::{events::room::message::RoomMessageEventContent, room_id},
    };
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex, query_param},
    };

    async fn fixture() -> (MockServer, matrix_sdk::Client) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        for suffix in ["query", "upload", "claim"] {
            Mock::given(method("POST")).and(path(format!("/_matrix/client/v3/keys/{suffix}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"device_keys":{},"one_time_keys":{},"one_time_key_counts":{"signed_curve25519":50}}))).mount(&server).await;
        }
        Mock::given(method("PUT"))
            .and(path_regex(".*/sendToDevice/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let state = |kind: &str, key: &str, content: Value| json!({"type":kind,"state_key":key,"sender":"@me:example.org","event_id":format!("${kind}"),"origin_server_ts":1,"content":content});
        let member = state(
            "m.room.member",
            "@me:example.org",
            json!({"membership":"join"}),
        );
        Mock::given(method("GET"))
            .and(path_regex(".*/members"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"chunk":[member.clone()]})),
            )
            .mount(&server)
            .await;
        let room = |encrypted: bool, saved: bool| {
            let mut events = vec![
                member.clone(),
                state(
                    "m.room.create",
                    "",
                    if saved {
                        json!({"creator":"@me:example.org","room_version":"11","type":crate::bookmarks::ROOM_TYPE})
                    } else {
                        json!({"creator":"@me:example.org","room_version":"11"})
                    },
                ),
            ];
            if encrypted {
                events.push(state(
                    "m.room.encryption",
                    "",
                    json!({"algorithm":"m.megolm.v1.aes-sha2"}),
                ));
            }
            json!({"state":{"events":events},"timeline":{"events":[],"limited":false}})
        };
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"sync","rooms":{"join":{
                "!encrypted:example.org":room(true, false), "!public:example.org":room(false, false), "!saved:example.org":room(true, true)
            }}}))).mount(&server).await;
        let client = matrix_sdk::Client::builder()
            .homeserver_url(server.uri())
            .request_config(RequestConfig::default().retry_limit(0))
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(
                    json!({"user_id":"@me:example.org","device_id":"TEST","access_token":"fake"}),
                )
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.encryption/?"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"No encryption event"})),
            )
            .mount(&server)
            .await;
        // A partial sync without an encryption event leaves the SDK state
        // unknown. Resolve this room to exercise known-public exclusion.
        client
            .get_room(room_id!("!public:example.org"))
            .unwrap()
            .latest_encryption_state()
            .await
            .unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn historical_edits_are_applied_across_pages_and_thread_targets_are_preserved() {
        let (server, client) = fixture().await;
        let event = |id: &str, sender: &str, content: Value| json!({"type":"m.room.message","event_id":id,"room_id":"!encrypted:example.org","sender":sender,"origin_server_ts":1234,"content":content});
        let plain = |body: &str| json!({"msgtype":"m.text","body":body});
        let edit = |target: &str| json!({"msgtype":"m.text","body":"* needle","m.new_content":plain("needle edited"),"m.relates_to":{"rel_type":"m.replace","event_id":target}});
        Mock::given(method("GET")).and(path_regex(".*/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"start":"latest","end":"older","chunk":[
                event("$edit", "@me:example.org", edit("$original")),
                event("$forged", "@other:example.org", edit("$untouched")),
                event("$reply", "@me:example.org", json!({"msgtype":"m.text","body":"needle reply","m.relates_to":{"rel_type":"m.thread","event_id":"$root"}}))
            ],"state":[]}))).with_priority(10).expect(1).mount(&server).await;
        Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .and(query_param("from", "older"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"start":"older","chunk":[
                event("$original", "@me:example.org", plain("outdated")),
                event("$untouched", "@me:example.org", plain("not a match")),
                event("$redacted", "@me:example.org", json!({}))
            ],"state":[]}),
            ))
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        let service = Service::new(client);
        let first = service
            .clone()
            .history_page("needle".into(), None)
            .await
            .unwrap();
        assert_eq!(first.hits.len(), 1);
        assert_eq!(first.hits[0].thread_root.as_deref(), Some("$root"));
        let second = service
            .history_page("needle".into(), Some(first.cursor))
            .await
            .unwrap();
        assert_eq!(second.hits.len(), 1);
        assert_eq!(second.hits[0].event, "$original");
        assert_eq!(second.hits[0].body, "needle edited");
        assert!(second.cursor.complete());
    }

    #[tokio::test]
    async fn loads_unopened_encrypted_history_decrypts_and_resumes_after_empty_pages() {
        let (server, client) = fixture().await;
        // The only send is against the isolated mock homeserver, to generate real ciphertext.
        Mock::given(method("PUT"))
            .and(path_regex(".*/send/m.room.encrypted/.*"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"event_id":"$historical"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        client
            .get_room(room_id!("!encrypted:example.org"))
            .unwrap()
            .send(RoomMessageEventContent::text_plain("Older CAFÉ needle"))
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let ciphertext: Value = requests
            .iter()
            .find(|request| request.url.path().contains("/send/m.room.encrypted/"))
            .unwrap()
            .body_json()
            .unwrap();
        assert!(ciphertext.get("body").is_none());
        assert!(
            !requests
                .iter()
                .any(|request| request.url.path().ends_with("/messages")),
            "no history loading before the user asks"
        );
        let mut missing = ciphertext.clone();
        missing["session_id"] = json!("unknown-session");
        let event = |id: &str, content: Value| json!({"type":"m.room.encrypted","event_id":id,"room_id":"!encrypted:example.org","sender":"@me:example.org","origin_server_ts":1234,"content":content});
        Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"start":"latest","end":"older","chunk":[],"state":[]})),
            )
            .with_priority(10)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path_regex(".*/messages")).and(query_param("from", "older"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"start":"older","chunk":[event("$historical", ciphertext),event("$locked",missing)],"state":[]})))
            .with_priority(1).expect(1).mount(&server).await;
        let service = Service::new(client);
        let first = service
            .clone()
            .history_page("café".into(), None)
            .await
            .unwrap();
        assert_eq!(
            first.cursor.rooms, 1,
            "public rooms and bookmark storage are excluded"
        );
        assert!(
            !first.cursor.complete(),
            "empty pages with a new token remain resumable"
        );
        let second = service
            .history_page("café".into(), Some(first.cursor))
            .await
            .unwrap();
        assert_eq!(second.hits.len(), 1, "{:?}", second.cursor);
        assert_eq!(second.hits[0].event, "$historical");
        assert_eq!(second.hits[0].body, "Older CAFÉ needle");
        assert!(second.cursor.complete());
        assert_eq!(second.cursor.finished, 1);
        assert_eq!(second.cursor.missing_keys, 1);
        assert_eq!(second.cursor.scanned, 2);
    }

    #[tokio::test]
    async fn filters_limit_rooms_and_check_original_sender_and_timestamp() {
        let (server, client) = fixture().await;
        let event = |id: &str, sender: &str, timestamp: u64| json!({"type":"m.room.message","event_id":id,"sender":sender,"origin_server_ts":timestamp,"content":{"msgtype":"m.text","body":"needle"}});
        Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"start":"latest","chunk":[
                event("$included", "@alice:example.org", 1000),
                event("$wrong-sender", "@other:example.org", 1000),
                event("$before", "@alice:example.org", 999),
                event("$end", "@alice:example.org", 2000)
            ],"state":[]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let service = Service::new(client);
        let filters = Filters {
            room: Some("!encrypted:example.org".into()),
            sender: Some("@alice:example.org".into()),
            after_ms: Some(1000),
            before_ms: Some(2000),
        };
        let page = service
            .clone()
            .history_page_filtered("needle".into(), None, filters)
            .await
            .unwrap();
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].event, "$included");
        assert!(page.cursor.complete());
        let empty = service
            .history_page_filtered(
                "needle".into(),
                None,
                Filters {
                    room: Some("!not-joined:example.org".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(empty.cursor.rooms, 0);
        assert!(empty.hits.is_empty());
    }

    #[tokio::test]
    async fn history_failures_and_cyclic_tokens_stop_without_continuous_retries() {
        let (server, client) = fixture().await;
        let mock = Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"start":"latest","end":"cycle","chunk":[],"state":[]})),
            )
            .expect(2)
            .mount_as_scoped(&server)
            .await;
        let service = Service::new(client);
        let first = service
            .clone()
            .history_page("needle".into(), None)
            .await
            .unwrap();
        let second = service
            .clone()
            .history_page("needle".into(), Some(first.cursor))
            .await
            .unwrap();
        assert!(second.cursor.complete());
        drop(mock);
        Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Not allowed"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let failed = service.history_page("needle".into(), None).await.unwrap();
        assert!(failed.cursor.complete());
        assert_eq!(failed.cursor.failed, 1);
        assert_eq!(failed.cursor.scanned, 0);
    }
}
