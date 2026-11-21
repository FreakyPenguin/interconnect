//! Explicit, cancellable homeserver message search. No background indexing.
use crate::connected::RuntimeOwned;
use matrix_sdk::{
    Client,
    ruma::{
        api::client::search::search_events::v3 as search,
        events::{
            AnyMessageLikeEvent, AnyTimelineEvent,
            room::message::{Relation, RoomMessageEvent},
        },
        uint,
    },
};
use std::{sync::Arc, time::Duration};

mod history;
pub use history::{HistoryCursor, HistoryPage};

pub const PAGE_SIZE: usize = 30;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub room: String,
    pub event: String,
    pub sender: String,
    pub sender_id: String,
    pub body: String,
    pub thread_root: Option<String>,
    pub timestamp_ms: i64,
}
/// Exact identity filters and UTC timestamp bounds (inclusive start, exclusive end).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    pub room: Option<String>,
    pub sender: Option<String>,
    pub after_ms: Option<i64>,
    pub before_ms: Option<i64>,
}
impl Filters {
    pub fn matches(&self, hit: &Hit) -> bool {
        self.room.as_ref().is_none_or(|room| room == &hit.room)
            && self
                .sender
                .as_ref()
                .is_none_or(|sender| sender == &hit.sender_id)
            && self.after_ms.is_none_or(|start| hit.timestamp_ms >= start)
            && self.before_ms.is_none_or(|end| hit.timestamp_ms < end)
    }
}
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub hits: Vec<Hit>,
    pub next: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Service(Arc<RuntimeOwned<Client>>);
impl Service {
    pub(crate) fn new(client: Client) -> Self {
        Self(RuntimeOwned::new(client))
    }

    pub async fn search(self, query: String, next: Option<String>) -> Result<Page, String> {
        self.search_filtered(query, next, Filters::default()).await
    }

    pub async fn search_filtered(
        self,
        query: String,
        next: Option<String>,
        filters: Filters,
    ) -> Result<Page, String> {
        let rooms: Vec<_> = self
            .0
            .joined_rooms()
            .iter()
            .filter(|room| {
                filters
                    .room
                    .as_deref()
                    .is_none_or(|id| id == room.room_id().as_str())
            })
            .map(|room| room.room_id().to_owned())
            .collect();
        if rooms.is_empty() {
            return Ok(Page::default());
        }
        let mut criteria = search::Criteria::new(query.trim().to_owned());
        criteria.keys = Some(vec![search::SearchKeys::ContentBody]);
        criteria.order_by = Some(search::OrderBy::Recent);
        criteria.filter.rooms = Some(rooms.clone());
        criteria.filter.senders = filters
            .sender
            .as_ref()
            .map(|sender| {
                sender
                    .parse::<matrix_sdk::ruma::OwnedUserId>()
                    .map(|id| vec![id])
            })
            .transpose()
            .map_err(|_| "Enter a full Matrix user ID, such as @alice:example.org.".to_owned())?;
        criteria.filter.types = Some(vec!["m.room.message".into()]);
        criteria.filter.limit = Some(uint!(30));
        criteria.event_context.before_limit = uint!(0);
        criteria.event_context.after_limit = uint!(0);
        criteria.event_context.include_profile = true;
        let mut categories = search::Categories::new();
        categories.room_events = Some(criteria);
        let mut request = search::Request::new(categories);
        request.next_batch = next.clone();
        let response = tokio::time::timeout(Duration::from_secs(30), self.0.send(request)).await
            .map_err(|_| "Server search timed out. Try again.".to_owned())?
            .map_err(|_| "Server search is unavailable. Your homeserver may not support message search. Local results are still shown.".to_owned())?;
        let results = response.search_categories.room_events;
        let mut hits = Vec::new();
        for result in results.results.into_iter().take(PAGE_SIZE) {
            let Some(raw) = result.result else {
                continue;
            };
            let Ok(AnyTimelineEvent::MessageLike(AnyMessageLikeEvent::RoomMessage(
                RoomMessageEvent::Original(event),
            ))) = raw.deserialize()
            else {
                continue;
            };
            if !rooms.contains(&event.room_id)
                || self
                    .0
                    .get_room(&event.room_id)
                    .is_none_or(|room| room.state() != matrix_sdk::RoomState::Joined)
            {
                continue;
            }
            let sender = result
                .context
                .profile_info
                .get(&event.sender)
                .and_then(|profile| profile.displayname.clone())
                .unwrap_or_else(|| event.sender.to_string());
            let thread_root = match &event.content.relates_to {
                Some(Relation::Thread(thread)) => Some(thread.event_id.to_string()),
                Some(Relation::Replacement(_)) => continue,
                _ => None,
            };
            hits.push(Hit {
                room: event.room_id.to_string(),
                event: event.event_id.to_string(),
                sender,
                sender_id: event.sender.to_string(),
                body: event.content.body().to_owned(),
                thread_root,
                timestamp_ms: u64::from(event.origin_server_ts.get()).min(i64::MAX as u64) as i64,
            });
        }
        hits.retain(|hit| filters.matches(hit));
        Ok(Page {
            hits,
            next: results
                .next_batch
                .filter(|token| Some(token) != next.as_ref()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::authentication::matrix::MatrixSession;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, method, path, query_param},
    };

    #[tokio::test]
    async fn searches_joined_rooms_and_paginates_without_accepting_unjoined_or_redacted_events() {
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
                    json!({"user_id":"@me:example.org","device_id":"TEST","access_token":"fake"}),
                )
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"sync", "rooms":{"join":{"!room:example.org":{"state":{"events":[]},"timeline":{"events":[],"limited":false,"prev_batch":"p"}}}}}))).mount(&server).await;
        client.sync_once(Default::default()).await.unwrap();
        let event = |room: &str, id: &str| json!({"type":"m.room.message","event_id":id,"room_id":room,"sender":"@alice:example.org","origin_server_ts":1234,"content":{"msgtype":"m.text","body":"hello world", "m.relates_to":{"rel_type":"m.thread","event_id":"$root"}}});
        Mock::given(method("POST")).and(path("/_matrix/client/v3/search")).and(query_param("next_batch", "page2"))
            .and(body_partial_json(json!({"search_categories":{"room_events":{"search_term":"hello","keys":["content.body"],"filter":{"rooms":["!room:example.org"],"types":["m.room.message"],"limit":30}}}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"search_categories":{"room_events":{"next_batch":"page2","results":[
                {"result":event("!room:example.org", "$one"),"context":{"profile_info":{"@alice:example.org":{"displayname":"Alice"}}}},
                {"result":event("!other:example.org", "$denied")},
                {"result":{"type":"m.room.message","event_id":"$redacted","room_id":"!room:example.org","sender":"@alice:example.org","origin_server_ts":1234,"content":{}}}
            ]}}}))).expect(1).mount(&server).await;
        let mut impostor = event("!room:example.org", "$wrong");
        impostor["sender"] = json!("@impostor:example.org");
        Mock::given(method("POST")).and(path("/_matrix/client/v3/search"))
            .and(body_partial_json(json!({"search_categories":{"room_events":{"filter":{"rooms":["!room:example.org"],"senders":["@alice:example.org"]}}}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"search_categories":{"room_events":{"next_batch":"filtered-next","results":[
                {"result":event("!room:example.org", "$included")},
                {"result":impostor}
            ]}}}))).expect(1).with_priority(1).mount(&server).await;
        let filtered = Service::new(client.clone())
            .search_filtered(
                "hello".into(),
                None,
                Filters {
                    room: Some("!room:example.org".into()),
                    sender: Some("@alice:example.org".into()),
                    after_ms: Some(1234),
                    before_ms: Some(1235),
                },
            )
            .await
            .unwrap();
        assert_eq!(filtered.hits.len(), 1);
        assert_eq!(filtered.hits[0].event, "$included");
        assert_eq!(filtered.next.as_deref(), Some("filtered-next"));
        let page = Service::new(client)
            .search(" hello ".into(), Some("page2".into()))
            .await
            .unwrap();
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].sender, "Alice");
        assert_eq!(page.hits[0].thread_root.as_deref(), Some("$root"));
        assert!(
            page.next.is_none(),
            "a repeating pagination token cannot loop forever"
        );
    }
}
