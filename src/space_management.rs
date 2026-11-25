//! Explicit space child linking and unlinking. Parent-side permission is sufficient.
use crate::navigation::SpaceEntry;
use matrix_sdk::{
    Client, RoomState,
    ruma::{
        OwnedRoomId,
        events::{StateEventType, space::child::SpaceChildEventContent},
    },
};

/// Parent-side eligibility using the known joined-space graph. Shared parents
/// are allowed; a path from child back to parent would introduce a cycle.
pub fn linkable_parents(spaces: &[SpaceEntry], child: &str) -> std::collections::HashSet<String> {
    let descendants =
        crate::navigation::members(&crate::navigation::Scope::Space(child.into()), spaces)
            .unwrap_or_default();
    spaces
        .iter()
        .filter(|space| {
            space.id != child
                && space.can_manage_rooms
                && !space.children.iter().any(|id| id == child)
                && !descendants.contains(&space.id)
        })
        .map(|space| space.id.clone())
        .collect()
}

/// Read cached joined-space edges only. Unknown children remain terminal nodes;
/// there is no hierarchy crawl or network request for unjoined spaces.
async fn joined_graph(client: &Client) -> Result<Vec<SpaceEntry>, String> {
    let mut spaces = Vec::new();
    for room in client
        .joined_rooms()
        .into_iter()
        .filter(|room| room.is_space())
    {
        let mut children = Vec::new();
        for raw in room
            .get_state_events_static::<SpaceChildEventContent>()
            .await
            .map_err(|_| "Could not check the space hierarchy.")?
        {
            let event = raw
                .deserialize()
                .map_err(|_| "Could not read the space hierarchy.")?;
            if let matrix_sdk::deserialized_responses::SyncOrStrippedState::Sync(
                matrix_sdk::ruma::events::SyncStateEvent::Original(event),
            ) = event
                && !event.content.via.is_empty()
            {
                children.push(event.state_key.to_string());
            }
        }
        spaces.push(SpaceEntry {
            id: room.room_id().to_string(),
            children,
            // The selected parent is permission-checked separately below.
            can_manage_rooms: true,
            ..Default::default()
        });
    }
    Ok(spaces)
}

/// Empty `via` removes the active child link. Do not redact history, leave the
/// room, or require permission to write `m.space.parent` in the child room.
pub async fn remove_room(client: &Client, space: &str, child: &str) -> Result<(), String> {
    let space: OwnedRoomId = space.parse().map_err(|_| "Choose a valid space.")?;
    let child: OwnedRoomId = child.parse().map_err(|_| "Choose a valid room.")?;
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let parent = client
            .get_room(&space)
            .filter(|r| r.state() == RoomState::Joined && r.is_space())
            .ok_or("The selected space is no longer available.")?;
        let levels = parent
            .power_levels()
            .await
            .map_err(|_| "Could not check space permissions.")?;
        if !levels.user_can_send_state(parent.own_user_id(), StateEventType::SpaceChild) {
            return Err("You no longer have permission to remove rooms from this space.".into());
        }
        // Recheck the direct link in the SDK cache rather than trusting UI state.
        let linked = parent
            .get_state_event_static_for_key::<SpaceChildEventContent, _>(&child)
            .await
            .map_err(|_| "Could not check the room’s space link.")?
            .and_then(|raw| raw.deserialize().ok())
            .is_some_and(|event| {
                matches!(event,
                matrix_sdk::deserialized_responses::SyncOrStrippedState::Sync(
                    matrix_sdk::ruma::events::SyncStateEvent::Original(event)
                ) if !event.content.via.is_empty())
            });
        if !linked {
            return Err("This room is no longer directly linked to this space.".into());
        }
        parent
            .send_state_event_for_key(&child, SpaceChildEventContent::new(vec![]))
            .await
            .map_err(|_| {
                "Could not remove the room from the space. Check the room list before retrying."
                    .to_owned()
            })?;
        Ok(())
    })
    .await
    .map_err(|_| "Removing the room timed out. Check the room list before retrying.".to_owned())?
}

/// Link an existing joined room or space using a routing server known to be in it.
/// Parent permission is sufficient; child-side metadata and membership stay intact.
pub async fn add_room(client: &Client, space: &str, child: &str) -> Result<(), String> {
    let id: OwnedRoomId = child.parse().map_err(|_| "Choose a valid room.")?;
    let room = client
        .get_room(&id)
        .filter(|r| r.state() == RoomState::Joined)
        .ok_or("The room is no longer available.")?;
    link_room(
        client,
        space,
        child,
        vec![room.own_user_id().server_name().to_owned()],
    )
    .await
}

/// Resolve an address without joining, then write only the parent-side child link.
/// Unknown children are terminal in the cached graph; remote cycles cannot be proven absent.
pub async fn link_address(client: &Client, space: &str, input: &str) -> Result<String, String> {
    let operation = async {
        let address = crate::room_membership::Address::parse(input)?;
        let (child, via) = match <OwnedRoomId>::try_from(address.room.clone()) {
            Ok(id) => (id, address.via),
            Err(_) => {
                let alias = address
                    .room
                    .as_str()
                    .parse::<matrix_sdk::ruma::OwnedRoomAliasId>()
                    .map_err(|_| "Enter a valid room address.")?;
                let resolved = client.resolve_room_alias(&alias).await.map_err(
                    |_| "Could not resolve this address. Check the address and your connection.",
                )?;
                // Alias resolution returns servers known to participate in the room.
                (resolved.room_id, resolved.servers)
            }
        };
        let mut routes = Vec::new();
        for server in via.into_iter().take(10) {
            if !routes.contains(&server) {
                routes.push(server);
            }
        }
        if routes.is_empty() {
            return Err(
                "No routing servers are available. Use a room link containing via servers.".into(),
            );
        }
        link_room(client, space, child.as_str(), routes).await?;
        Ok(child.to_string())
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), operation)
        .await
        .map_err(|_| "Linking timed out. Reload the space before retrying.".to_owned())?
}

async fn link_room(
    client: &Client,
    space: &str,
    child: &str,
    via: Vec<matrix_sdk::ruma::OwnedServerName>,
) -> Result<(), String> {
    let space: OwnedRoomId = space.parse().map_err(|_| "Choose a valid space.")?;
    let child: OwnedRoomId = child.parse().map_err(|_| "Choose a valid room.")?;
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let parent = client
            .get_room(&space)
            .filter(|r| r.state() == RoomState::Joined && r.is_space())
            .ok_or("The selected space is no longer available.")?;
        if space == child {
            return Err("A space cannot be added to itself.".into());
        }
        let levels = parent
            .power_levels()
            .await
            .map_err(|_| "Could not check space permissions.")?;
        if !levels.user_can_send_state(parent.own_user_id(), StateEventType::SpaceChild) {
            return Err("You no longer have permission to add rooms to this space.".into());
        }
        if let Some(raw) = parent
            .get_state_event_static_for_key::<SpaceChildEventContent, _>(&child)
            .await
            .map_err(|_| "Could not check the room’s space link.")?
        {
            let event = raw
                .deserialize()
                .map_err(|_| "Could not read the room’s space link.")?;
            if matches!(event, matrix_sdk::deserialized_responses::SyncOrStrippedState::Sync(
                matrix_sdk::ruma::events::SyncStateEvent::Original(event)
            ) if !event.content.via.is_empty())
            {
                return Err("This room is already directly linked to this space.".into());
            }
        }
        if !linkable_parents(&joined_graph(client).await?, child.as_str()).contains(space.as_str()) {
            return Err("This link would create a cycle in the known space hierarchy, or is already linked. Refresh the space list before retrying.".into());
        }
        let request = matrix_sdk::ruma::api::client::state::send_state_event::v3::Request::new(
            space, &child, &SpaceChildEventContent::new(via))
            .map_err(|_| "Could not prepare the space link.")?;
        client.send(request)
            .with_request_config(matrix_sdk::config::RequestConfig::default().disable_retry())
            .await
            .map_err(|_| {
                "Could not add the room to the space. Check the room list before retrying."
                    .to_owned()
            })?;
        Ok(())
    })
    .await
    .map_err(|_| "Adding the room timed out. Check the room list before retrying.".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    async fn room_writes(server: &MockServer) -> Vec<wiremock::Request> {
        server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.method.as_str() != "GET" && r.url.path().contains("/rooms/"))
            .collect()
    }

    #[test]
    fn parents_allow_shared_children_but_reject_self_duplicates_and_cycles() {
        let space = |id: &str, children: &[&str], writable| SpaceEntry {
            id: id.into(),
            children: children.iter().map(|id| (*id).into()).collect(),
            can_manage_rooms: writable,
            ..Default::default()
        };
        let spaces = vec![
            space("child", &["descendant"], false),
            space("descendant", &["ancestor", "unknown"], true),
            space("ancestor", &[], true),
            space("shared", &["child"], true),
            space("fresh", &[], true),
            space("denied", &[], false),
            space("cycle-a", &["cycle-b"], true),
            space("cycle-b", &["cycle-a"], true),
        ];
        let parents = linkable_parents(&spaces, "child");
        assert_eq!(
            parents,
            ["fresh", "cycle-a", "cycle-b"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
        assert!(linkable_parents(&spaces, "ordinary-room").contains("fresh"));
    }

    #[tokio::test]
    async fn joined_subspace_links_only_parent_state_and_retains_other_parent() {
        let server = MockServer::start().await;
        let client = fixture(&server, 50).await;
        Mock::given(method("PUT"))
            .and(path("/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/!fresh-space:example.org"))
            .and(body_json(json!({"via":["example.org"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$linked"})))
            .expect(1).mount(&server).await;
        add_room(&client, "!space:example.org", "!fresh-space:example.org")
            .await
            .unwrap();
        let graph = joined_graph(&client).await.unwrap();
        assert!(
            graph
                .iter()
                .find(|s| s.id == "!shared:example.org")
                .unwrap()
                .children
                .contains(&"!fresh-space:example.org".into())
        );
        assert_eq!(room_writes(&server).await.len(), 1);
    }

    #[tokio::test]
    async fn subspace_self_cycle_duplicates_and_denied_permission_never_write() {
        for power in [0, 50] {
            let server = MockServer::start().await;
            let client = fixture(&server, power).await;
            for child in [
                "!space:example.org",
                "!nested:example.org",
                "!descendant:example.org",
            ] {
                assert!(
                    add_room(&client, "!space:example.org", child)
                        .await
                        .is_err()
                );
            }
            assert!(
                add_room(&client, "!space:example.org", "!linked-space:example.org")
                    .await
                    .is_err()
            );
            assert!(
                add_room(&client, "!shared:example.org", "!fresh-space:example.org")
                    .await
                    .unwrap_err()
                    .contains("already")
            );
            if power == 0 {
                assert!(
                    add_room(&client, "!space:example.org", "!fresh-space:example.org")
                        .await
                        .unwrap_err()
                        .contains("permission")
                );
            }
            assert!(room_writes(&server).await.is_empty());
        }
    }

    #[tokio::test]
    async fn address_links_resolve_aliases_and_keep_unjoined_children_unjoined() {
        let server = MockServer::start().await;
        let client = fixture(&server, 50).await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/directory/room/%23remote:elsewhere.org"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "room_id":"!remote-space", "servers":["route.example", "backup.example", "route.example"]
            }))).expect(1).mount(&server).await;
        for (child, via) in [
            ("!remote-space", json!(["route.example", "backup.example"])),
            ("!remote-room", json!(["hint.example"])),
            ("!legacy:origin.example", json!(["origin.example"])),
        ] {
            Mock::given(method("PUT"))
                .and(path(format!(
                    "/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/{child}"
                )))
                .and(body_json(json!({"via":via})))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"event_id":"$linked"})),
                )
                .expect(1)
                .mount(&server)
                .await;
        }
        assert_eq!(
            link_address(&client, "!space:example.org", "#remote:elsewhere.org")
                .await
                .unwrap(),
            "!remote-space"
        );
        assert_eq!(
            link_address(
                &client,
                "!space:example.org",
                "https://matrix.to/#/!remote-room?via=hint.example"
            )
            .await
            .unwrap(),
            "!remote-room"
        );
        link_address(&client, "!space:example.org", "!legacy:origin.example")
            .await
            .unwrap();
        for child in ["!remote-space", "!remote-room", "!legacy:origin.example"] {
            assert!(
                client
                    .get_room(&child.parse::<OwnedRoomId>().unwrap())
                    .is_none()
            );
        }
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| matches!(r.method.as_str(), "PUT" | "DELETE"))
                .count(),
            3
        );
        assert!(
            requests
                .iter()
                .all(|r| !r.url.path().contains("/join")
                    && !r.url.path().contains("/m.space.parent/"))
        );
    }

    #[tokio::test]
    async fn address_link_failures_do_not_join_or_write_child_state_and_writes_are_not_retried() {
        for power in [0, 50] {
            let server = MockServer::start().await;
            let client = fixture(&server, power).await;
            for input in [
                "invalid",
                "!opaque",
                "!space:example.org",
                "!room:example.org",
                "!nested:example.org",
            ] {
                assert!(
                    link_address(&client, "!space:example.org", input)
                        .await
                        .is_err(),
                    "{input}"
                );
            }
            Mock::given(method("GET"))
                .and(path(
                    "/_matrix/client/v3/directory/room/%23empty:example.org",
                ))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"room_id":"!empty", "servers":[]})),
                )
                .expect(1)
                .mount(&server)
                .await;
            assert!(
                link_address(&client, "!space:example.org", "#empty:example.org")
                    .await
                    .unwrap_err()
                    .contains("routing")
            );
            if power == 0 {
                assert!(
                    link_address(&client, "!space:example.org", "!remote:example.org")
                        .await
                        .unwrap_err()
                        .contains("permission")
                );
            } else {
                Mock::given(method("PUT"))
                    .and(path("/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/!remote:example.org"))
                    .respond_with(ResponseTemplate::new(503).set_body_json(json!({"errcode":"M_UNKNOWN", "error":"Unavailable"})))
                    .expect(1).mount(&server).await;
                assert!(
                    link_address(&client, "!space:example.org", "!remote:example.org")
                        .await
                        .is_err()
                );
            }
            assert_eq!(
                server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .filter(|r| matches!(r.method.as_str(), "PUT" | "DELETE"))
                    .count(),
                usize::from(power == 50)
            );
        }
    }

    async fn fixture(server: &MockServer, power: u8) -> Client {
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
                    "user_id":"@test:example.org", "device_id":"TEST", "access_token":"fake"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        let event = |kind: &str, key: &str, content: Value| {
            json!({
                "type":kind, "state_key":key, "content":content, "sender":"@test:example.org",
                "event_id":format!("${kind}-{key}"), "origin_server_ts":1
            })
        };
        let joined_room = json!({
            "state":{"events":[
                event("m.room.create", "", json!({"creator":"@admin:example.org"})),
                event("m.room.member", "@test:example.org", json!({"membership":"join"})),
                event("m.room.power_levels", "", json!({"users":{"@test:example.org":0}, "state_default":100}))
            ]}, "timeline":{"events":[],"limited":false}, "account_data":{"events":[]}
        });
        let joined_space = |children: &[&str], power: u8| {
            let mut state = vec![
                event(
                    "m.room.create",
                    "",
                    json!({"creator":"@admin:example.org", "type":"m.space"}),
                ),
                event(
                    "m.room.member",
                    "@test:example.org",
                    json!({"membership":"join"}),
                ),
                event(
                    "m.room.power_levels",
                    "",
                    json!({"users":{"@test:example.org":power}, "state_default":100, "events":{"m.space.child":50}}),
                ),
            ];
            state.extend(
                children
                    .iter()
                    .map(|child| event("m.space.child", child, json!({"via":["example.org"]}))),
            );
            json!({"state":{"events":state}, "timeline":{"events":[],"limited":false}, "account_data":{"events":[]}})
        };
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "next_batch":"one", "rooms":{"join":{"!space:example.org":{
                    "state":{"events":[
                        event("m.room.create", "", json!({"creator":"@test:example.org", "type":"m.space"})),
                        event("m.room.member", "@test:example.org", json!({"membership":"join"})),
                        event("m.room.power_levels", "", json!({"users":{"@test:example.org":power}, "state_default":100, "events":{"m.space.child":50}})),
                        event("m.space.child", "!room:example.org", json!({"via":["example.org"]})),
                        event("m.space.child", "!linked-space:example.org", json!({"via":["example.org"]})),
                        event("m.space.child", "!removed:example.org", json!({"via":[]}))
                    ]}, "timeline":{"events":[], "limited":false}, "account_data":{"events":[]}
                }, "!fresh:example.org":joined_room, "!removed:example.org":joined_room,
                   "!room:example.org":joined_room,
                   "!fresh-space:example.org":joined_space(&[], 0),
                   "!linked-space:example.org":joined_space(&[], 0),
                   "!shared:example.org":joined_space(&["!fresh-space:example.org"], 50),
                   "!nested:example.org":joined_space(&["!descendant:example.org"], 0),
                   "!descendant:example.org":joined_space(&["!space:example.org"], 0)}}
            }))).mount(server).await;
        client
            .sync_once(SyncSettings::default().timeout(std::time::Duration::ZERO))
            .await
            .unwrap();
        client
    }
    #[tokio::test]
    async fn unlink_sends_only_empty_child_state_and_projects_event_specific_permission() {
        let server = MockServer::start().await;
        let client = fixture(&server, 50).await;
        assert!(
            crate::navigation::snapshot(&client)
                .await
                .unwrap()
                .spaces
                .iter()
                .find(|s| s.id == "!space:example.org")
                .unwrap()
                .can_manage_rooms
        );
        for child in ["!room:example.org", "!linked-space:example.org"] {
            Mock::given(method("PUT"))
                .and(path(format!(
                    "/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/{child}"
                )))
                .and(body_json(json!({"via":[]})))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"event_id":"$removed"})),
                )
                .expect(1)
                .mount(&server)
                .await;
            remove_room(&client, "!space:example.org", child)
                .await
                .unwrap();
        }
        assert_eq!(room_writes(&server).await.len(), 2);
    }
    #[tokio::test]
    async fn denied_permission_and_missing_or_inactive_links_never_write() {
        for power in [0, 50] {
            let server = MockServer::start().await;
            let client = fixture(&server, power).await;
            assert_eq!(
                crate::navigation::snapshot(&client)
                    .await
                    .unwrap()
                    .spaces
                    .iter()
                    .find(|s| s.id == "!space:example.org")
                    .unwrap()
                    .can_manage_rooms,
                power == 50
            );
            for (space, child) in [
                ("!space:example.org", "!missing:example.org"),
                ("!space:example.org", "!removed:example.org"),
                ("!gone:example.org", "!room:example.org"),
                ("invalid", "!room:example.org"),
                ("!space:example.org", "invalid"),
            ] {
                assert!(remove_room(&client, space, child).await.is_err());
            }
            if power == 0 {
                assert!(
                    remove_room(&client, "!space:example.org", "!room:example.org")
                        .await
                        .unwrap_err()
                        .contains("permission")
                );
            }
            assert!(room_writes(&server).await.is_empty());
        }
    }
    #[tokio::test]
    async fn add_links_joined_rooms_and_inactive_links_without_child_permission_or_parent_edits() {
        let server = MockServer::start().await;
        let client = fixture(&server, 50).await;
        for child in ["!fresh:example.org", "!removed:example.org"] {
            Mock::given(method("PUT"))
                .and(path(format!(
                    "/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/{child}"
                )))
                .and(body_json(json!({"via":["example.org"]})))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"event_id":"$added"})),
                )
                .expect(1)
                .mount(&server)
                .await;
            add_room(&client, "!space:example.org", child)
                .await
                .unwrap();
        }
        assert_eq!(room_writes(&server).await.len(), 2);
    }
    #[tokio::test]
    async fn add_rejects_unwritable_spaces_existing_links_and_unavailable_targets_before_writing() {
        for power in [0, 50] {
            let server = MockServer::start().await;
            let client = fixture(&server, power).await;
            for (space, child) in [
                ("!space:example.org", "!room:example.org"),
                ("!space:example.org", "!missing:example.org"),
                ("!gone:example.org", "!fresh:example.org"),
                ("!fresh:example.org", "!removed:example.org"),
                ("!space:example.org", "!space:example.org"),
                ("invalid", "!fresh:example.org"),
                ("!space:example.org", "invalid"),
            ] {
                assert!(add_room(&client, space, child).await.is_err());
            }
            if power == 0 {
                assert!(
                    add_room(&client, "!space:example.org", "!fresh:example.org")
                        .await
                        .unwrap_err()
                        .contains("permission")
                );
            }
            assert!(room_writes(&server).await.is_empty());
        }
    }
    #[tokio::test]
    async fn server_rejections_are_reported_for_linking_and_unlinking() {
        for (adding, child, error) in [
            (false, "!room:example.org", "Could not remove"),
            (true, "!fresh:example.org", "Could not add"),
        ] {
            let server = MockServer::start().await;
            let client = fixture(&server, 50).await;
            Mock::given(method("PUT"))
                .and(path(format!(
                    "/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/{child}"
                )))
                .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                    "errcode":"M_FORBIDDEN", "error":"Permission changed"
                })))
                .expect(1)
                .mount(&server)
                .await;
            let result = if adding {
                add_room(&client, "!space:example.org", child).await
            } else {
                remove_room(&client, "!space:example.org", child).await
            };
            assert!(result.unwrap_err().contains(error));
        }
    }
}
