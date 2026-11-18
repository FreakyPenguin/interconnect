//! Explicit, paginated space discovery. No timelines, account writes, or polling.
use matrix_sdk::{
    Client, RoomState,
    ruma::{
        OwnedRoomId, OwnedServerName,
        api::client::space::get_hierarchy::v1::Request,
        room::{JoinRuleSummary, RoomType},
    },
};
use std::{collections::HashMap, time::Duration};

pub const PAGE_SIZE: usize = 50;
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub topic: String,
    pub alias: Option<String>,
    pub space: bool,
    pub members: u64,
    pub access: JoinRuleSummary,
}
impl Entry {
    pub fn can_join(&self) -> bool {
        matches!(
            self.access,
            JoinRuleSummary::Public
                | JoinRuleSummary::Restricted(_)
                | JoinRuleSummary::KnockRestricted(_)
        )
    }
    pub fn access_label(&self) -> &'static str {
        match self.access {
            JoinRuleSummary::Public => "Public",
            JoinRuleSummary::Restricted(_) | JoinRuleSummary::KnockRestricted(_) => {
                "Restricted access"
            }
            JoinRuleSummary::Invite => "Invitation required",
            JoinRuleSummary::Knock => "Invitation or knock required",
            _ => "Private or custom access",
        }
    }
    /// Keep the room ID fixed; route using child-link hints, then known server names.
    pub fn address(
        &self,
        routes: &HashMap<String, Vec<OwnedServerName>>,
    ) -> Result<String, String> {
        let id = self
            .id
            .parse::<OwnedRoomId>()
            .map_err(|_| "Invalid room ID.")?;
        let mut via = routes.get(&self.id).cloned().unwrap_or_default();
        if via.is_empty() {
            if let Some(server) = id.server_name() {
                via.push(server.to_owned());
            } else if let Some(alias) = self
                .alias
                .as_ref()
                .and_then(|a| a.parse::<matrix_sdk::ruma::OwnedRoomAliasId>().ok())
            {
                via.push(alias.server_name().to_owned());
            }
        }
        via.truncate(10);
        if via.is_empty() {
            return Err(
                "No routing servers are available for this room. Reload the space and try again."
                    .into(),
            );
        }
        crate::composition::MentionTarget::Room {
            room_id: id.into(),
            via,
        }
        .uri()
        .ok_or_else(|| "Could not prepare the room address.".into())
    }
}
#[derive(Clone, Debug)]
pub struct Link {
    pub parent: String,
    pub child: String,
    pub via: Vec<OwnedServerName>,
}
#[derive(Clone, Debug)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub links: Vec<Link>,
    pub next: Option<String>,
}
pub(crate) async fn load(
    client: &Client,
    space: String,
    from: Option<String>,
) -> Result<Page, String> {
    let id = space
        .parse::<OwnedRoomId>()
        .map_err(|_| "Choose a valid space.")?;
    if !client
        .get_room(&id)
        .is_some_and(|r| r.state() == RoomState::Joined && r.is_space())
    {
        return Err("The selected space is no longer available.".into());
    }
    if from.as_ref().is_some_and(|t| t.len() > 4096) {
        return Err("The space pagination token is invalid. Reload the space.".into());
    }
    let mut request = Request::new(id);
    request.from = from;
    request.limit = Some(matrix_sdk::ruma::uint!(50));
    // Include visible descendants and non-suggested rooms; fetch another page only on demand.
    let response = tokio::time::timeout(Duration::from_secs(30), client.send(request))
        .await
        .map_err(|_| "Loading space rooms timed out. Try again.".to_owned())?
        .map_err(|_| {
            "Could not load space rooms. Check your connection and space access, then retry."
                .to_owned()
        })?;
    if response.rooms.len() > 1000 {
        return Err("The server returned too many rooms in one page.".into());
    }
    let mut entries = Vec::new();
    let mut links = Vec::new();
    for chunk in response.rooms {
        let summary = chunk.summary;
        for event in chunk
            .children_state
            .into_iter()
            .filter_map(|raw| raw.deserialize().ok())
        {
            if event.content.via.is_empty() {
                continue;
            }
            links.push(Link {
                parent: summary.room_id.to_string(),
                child: event.state_key.to_string(),
                via: event.content.via.into_iter().take(10).collect(),
            });
        }
        if summary.room_id.as_str() == space {
            continue;
        }
        entries.push(Entry {
            id: summary.room_id.to_string(),
            name: summary
                .name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| summary.room_id.to_string()),
            topic: summary.topic.unwrap_or_default(),
            alias: summary.canonical_alias.map(|a| a.to_string()),
            space: summary.room_type == Some(RoomType::Space),
            members: summary.num_joined_members.into(),
            access: summary.join_rule,
        });
    }
    Ok(Page {
        entries,
        links,
        next: response.next_batch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{
        authentication::matrix::MatrixSession,
        config::{RequestConfig, SyncSettings},
    };
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param, query_param_is_missing},
    };

    async fn fixture(server: &MockServer) -> Client {
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(server)
            .await;
        let client = Client::builder()
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
                    json!({"user_id":"@test:example.org","device_id":"TEST","access_token":"fake"}),
                )
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        let event = |kind: &str, key: &str, content: Value| json!({"type":kind,"state_key":key,"content":content,"sender":"@test:example.org","event_id":format!("${kind}"),"origin_server_ts":1});
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!space:example.org":{
                "state":{"events":[event("m.room.create", "", json!({"creator":"@test:example.org","type":"m.space"})),event("m.room.member", "@test:example.org", json!({"membership":"join"}))]},
                "timeline":{"events":[],"limited":false},"account_data":{"events":[]}
            }}}}))).mount(server).await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        client
    }
    fn summary(id: &str, space: bool) -> Value {
        let mut value = json!({"room_id":id,"name":"A visible room","topic":"Topic","num_joined_members":42,"world_readable":false,"guest_can_join":false,"join_rule":"public","children_state":[]});
        if space {
            value["room_type"] = json!("m.space");
        }
        value
    }
    #[tokio::test]
    async fn hierarchy_discovers_unjoined_rooms_routes_and_explicit_pages() {
        let server = MockServer::start().await;
        let client = fixture(&server).await;
        let mut root = summary("!space:example.org", true);
        root["children_state"] = json!([
            {"type":"m.space.child","state_key":"!opaque","sender":"@test:example.org","origin_server_ts":1,"content":{"via":["route.example","backup.example"]}},
            {"type":"m.space.child","state_key":"!removed:example.org","sender":"@test:example.org","origin_server_ts":1,"content":{"via":[]}}
        ]);
        Mock::given(method("GET"))
            .and(path(
                "/_matrix/client/v1/rooms/!space:example.org/hierarchy",
            ))
            .and(query_param("limit", "50"))
            .and(query_param_is_missing("from"))
            .and(query_param_is_missing("max_depth"))
            .and(query_param_is_missing("suggested_only"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"rooms":[root,summary("!opaque", false)],"next_batch":"next-page"}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let first = load(&client, "!space:example.org".into(), None)
            .await
            .unwrap();
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.entries[0].id, "!opaque");
        assert_eq!(first.entries[0].members, 42);
        assert_eq!(first.entries[0].topic, "Topic");
        assert!(first.entries[0].can_join());
        assert_eq!(first.links.len(), 1);
        let routes = first.links.into_iter().map(|l| (l.child, l.via)).collect();
        let address =
            crate::room_membership::Address::parse(&first.entries[0].address(&routes).unwrap())
                .unwrap();
        assert_eq!(address.room.as_str(), "!opaque");
        assert_eq!(
            address.via.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            vec!["route.example", "backup.example"]
        );
        // No task fetches the continuation until the caller explicitly requests it.
        Mock::given(method("GET"))
            .and(path(
                "/_matrix/client/v1/rooms/!space:example.org/hierarchy",
            ))
            .and(query_param("from", "next-page"))
            .and(query_param("limit", "50"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"rooms":[summary("!sub:example.org",true)]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let second = load(&client, "!space:example.org".into(), first.next)
            .await
            .unwrap();
        assert!(second.entries[0].space);
        assert!(second.next.is_none());
    }
    #[tokio::test]
    async fn errors_and_unavailable_spaces_do_not_become_empty_successes() {
        let server = MockServer::start().await;
        let client = fixture(&server).await;
        let before = server.received_requests().await.unwrap().len();
        assert!(
            load(&client, "!missing:example.org".into(), None)
                .await
                .is_err()
        );
        assert!(load(&client, "invalid".into(), None).await.is_err());
        assert!(
            load(&client, "!space:example.org".into(), Some("x".repeat(4097)))
                .await
                .is_err()
        );
        assert_eq!(server.received_requests().await.unwrap().len(), before);
        Mock::given(method("GET"))
            .and(path(
                "/_matrix/client/v1/rooms/!space:example.org/hierarchy",
            ))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Private space"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            load(&client, "!space:example.org".into(), None)
                .await
                .unwrap_err()
                .contains("Could not load")
        );
    }
    #[test]
    fn join_rules_and_routing_fallbacks_preserve_room_identity() {
        let mut entry = Entry {
            id: "!opaque".into(),
            name: "Room".into(),
            topic: String::new(),
            alias: None,
            members: 0,
            space: false,
            access: JoinRuleSummary::Public,
        };
        assert!(entry.address(&HashMap::new()).is_err());
        entry.alias = Some("#room:alias.example".into());
        let address =
            crate::room_membership::Address::parse(&entry.address(&HashMap::new()).unwrap())
                .unwrap();
        assert_eq!(address.room.as_str(), "!opaque");
        assert_eq!(address.via[0].as_str(), "alias.example");
        entry.id = "!room:id.example".into();
        let address =
            crate::room_membership::Address::parse(&entry.address(&HashMap::new()).unwrap())
                .unwrap();
        assert_eq!(address.via[0].as_str(), "id.example");
        entry.access = JoinRuleSummary::Invite;
        assert!(!entry.can_join());
        entry.access = JoinRuleSummary::Knock;
        assert!(!entry.can_join());
        entry.access = JoinRuleSummary::Restricted(Default::default());
        assert!(entry.can_join());
    }
}
