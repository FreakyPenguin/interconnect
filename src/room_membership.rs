//! Explicit room membership writes and local invitation snapshots.
use crate::navigation::{RoomEntry, SpaceEntry};
use matrix_sdk::{Client, RoomState};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invitation {
    pub id: String,
    pub name: String,
    pub inviter: Option<String>,
    pub space: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Joined {
    pub id: String,
    pub name: String,
    pub space: bool,
}
impl Joined {
    pub fn room(&self) -> RoomEntry {
        RoomEntry {
            id: self.id.clone(),
            name: self.name.clone(),
            ..Default::default()
        }
    }
    pub fn space(&self) -> SpaceEntry {
        SpaceEntry {
            id: self.id.clone(),
            name: self.name.clone(),
            ..Default::default()
        }
    }
}

/// Reads stripped state from the SDK store; never fetches room members or polls.
pub async fn invitations(client: &Client) -> Vec<Invitation> {
    let mut invitations = Vec::new();
    for room in client.invited_rooms() {
        invitations.push(Invitation {
            id: room.room_id().to_string(),
            name: room
                .name()
                .or_else(|| room.cached_display_name().map(|n| n.to_string()))
                .unwrap_or_else(|| room.room_id().to_string()),
            inviter: room
                .invite_details()
                .await
                .ok()
                .map(|details| details.inviter_id.to_string()),
            space: room.is_space(),
        });
    }
    invitations.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    invitations
}

pub(crate) async fn respond(
    client: &Client,
    id: &str,
    accept: bool,
) -> Result<Option<Joined>, String> {
    let id: matrix_sdk::ruma::OwnedRoomId = id.parse().map_err(|_| "Invalid room ID.")?;
    let room = client
        .get_room(&id)
        .filter(|room| room.state() == RoomState::Invited)
        .ok_or("This invitation is no longer available. Check your room list.")?;
    let name = room
        .name()
        .or_else(|| room.cached_display_name().map(|n| n.to_string()))
        .unwrap_or_else(|| id.to_string());
    let space = room.is_space();
    tokio::time::timeout(Duration::from_secs(30), async {
        if accept { room.join().await } else { room.leave().await }
    }).await.map_err(|_| "The invitation request timed out and may have completed. Check your room list before retrying.".to_owned())?
        .map_err(|_| "Could not update this invitation. Check your connection and try again.".to_owned())?;
    Ok(accept.then(|| Joined {
        id: id.to_string(),
        name,
        space,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{
        authentication::matrix::MatrixSession,
        config::{RequestConfig, SyncSettings},
    };
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    async fn fixture(server: &MockServer) -> Client {
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(server)
            .await;
        for endpoint in ["upload", "query", "claim"] {
            Mock::given(method("POST")).and(path(format!("/_matrix/client/v3/keys/{endpoint}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"device_keys":{},"one_time_keys":{},"one_time_key_counts":{"signed_curve25519":50}})))
                .mount(server).await;
        }
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
                serde_json::from_value::<MatrixSession>(json!({
                    "user_id":"@me:example.org","device_id":"FIXTURE","access_token":"fixture-token"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        let invite = |space: bool, name: &str| {
            json!({"invite_state":{"events":[
                {"type":"m.room.create","state_key":"","sender":"@other:example.org","content":if space { json!({"type":"m.space"}) } else { json!({}) }},
                {"type":"m.room.name","state_key":"","sender":"@other:example.org","content":{"name":name}},
                {"type":"m.room.member","state_key":"@me:example.org","sender":"@other:example.org","content":{"membership":"invite"}}
            ]}})
        };
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"invite":{
                "!room:example.org":invite(false,"A room"), "!space:example.org":invite(true,"Z space")
            }}}))).mount(server).await;
        client.sync_once(SyncSettings::default()).await.unwrap();
        client
    }

    #[tokio::test]
    async fn invitation_snapshot_uses_local_stripped_state_without_network_requests() {
        let server = MockServer::start().await;
        let client = fixture(&server).await;
        let before = server.received_requests().await.unwrap().len();
        let invites = invitations(&client).await;
        assert_eq!(invites.len(), 2);
        assert_eq!(invites[0].name, "A room");
        assert_eq!(invites[0].inviter.as_deref(), Some("@other:example.org"));
        assert!(!invites[0].space);
        assert!(invites[1].space);
        assert_eq!(server.received_requests().await.unwrap().len(), before);
    }

    #[tokio::test]
    async fn invitation_writes_update_membership_and_reject_stale_actions() {
        let server = MockServer::start().await;
        let client = fixture(&server).await;
        Mock::given(method("POST"))
            .and(path_regex("/_matrix/client/v3/(rooms/[^/]+/join|join/.*)"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"room_id":"!room:example.org"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex("/_matrix/client/v3/rooms/.*/leave"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex("/_matrix/client/v3/rooms/.*/forget"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let joined = respond(&client, "!room:example.org", true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(joined.name, "A room");
        assert_eq!(client.joined_rooms().len(), 1);
        assert!(respond(&client, "!room:example.org", false).await.is_err());
        assert!(
            respond(&client, "!space:example.org", false)
                .await
                .unwrap()
                .is_none()
        );
        assert!(invitations(&client).await.is_empty());
        assert!(respond(&client, "invalid", true).await.is_err());
    }

    #[tokio::test]
    async fn failed_invitation_keeps_it_available_for_retry() {
        let server = MockServer::start().await;
        let client = fixture(&server).await;
        Mock::given(method("POST"))
            .and(path_regex("/_matrix/client/v3/(rooms/[^/]+/join|join/.*)"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"refused"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(respond(&client, "!room:example.org", true).await.is_err());
        assert_eq!(invitations(&client).await.len(), 2);
    }
}
