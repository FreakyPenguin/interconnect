//! Explicit space child unlinking. Parent-side permission is sufficient.
use matrix_sdk::{
    Client, RoomState,
    ruma::{
        OwnedRoomId,
        events::{StateEventType, space::child::SpaceChildEventContent},
    },
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

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
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "next_batch":"one", "rooms":{"join":{"!space:example.org":{
                    "state":{"events":[
                        event("m.room.create", "", json!({"creator":"@test:example.org", "type":"m.space"})),
                        event("m.room.member", "@test:example.org", json!({"membership":"join"})),
                        event("m.room.power_levels", "", json!({"users":{"@test:example.org":power}, "state_default":100, "events":{"m.space.child":50}})),
                        event("m.space.child", "!room:example.org", json!({"via":["example.org"]})),
                        event("m.space.child", "!removed:example.org", json!({"via":[]}))
                    ]}, "timeline":{"events":[], "limited":false}, "account_data":{"events":[]}
                }}}
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
        assert!(crate::navigation::snapshot(&client).await.unwrap().spaces[0].can_manage_rooms);
        Mock::given(method("PUT"))
            .and(path(
                "/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/!room:example.org",
            ))
            .and(body_json(json!({"via":[]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$removed"})))
            .expect(1)
            .mount(&server)
            .await;
        remove_room(&client, "!space:example.org", "!room:example.org")
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let writes: Vec<_> = requests
            .iter()
            .filter(|r| r.method.as_str() != "GET" && r.url.path().contains("/rooms/"))
            .collect();
        assert_eq!(writes.len(), 1); // No leave, redaction, or child-side parent edit.
    }
    #[tokio::test]
    async fn denied_permission_and_missing_or_inactive_links_never_write() {
        for power in [0, 50] {
            let server = MockServer::start().await;
            let client = fixture(&server, power).await;
            assert_eq!(
                crate::navigation::snapshot(&client).await.unwrap().spaces[0].can_manage_rooms,
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
            assert!(
                server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .all(|r| r.method.as_str() == "GET" || !r.url.path().contains("/rooms/"))
            );
        }
    }
    #[tokio::test]
    async fn server_rejection_is_reported() {
        let server = MockServer::start().await;
        let client = fixture(&server, 50).await;
        Mock::given(method("PUT"))
            .and(path(
                "/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/!room:example.org",
            ))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN", "error":"Permission changed"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            remove_room(&client, "!space:example.org", "!room:example.org")
                .await
                .unwrap_err()
                .contains("Could not remove")
        );
    }
}
