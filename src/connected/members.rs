//! On-demand joined-member and pending-invitation snapshots. The session drops this future on navigation.
use super::Model;
use matrix_sdk::{Client, RoomMemberships, room::RoomMemberRole, sync::RoomUpdate};
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast::error::RecvError, watch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub user_id: String,
    pub name: String,
    pub avatar_url: Option<matrix_sdk::ruma::OwnedMxcUri>,
    pub role: String,
    /// Pending invitation, rather than joined membership.
    pub invited: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Members {
    pub generation: u64,
    /// Joined people followed by pending invitees, each group sorted by name and ID.
    pub rows: Arc<Vec<Member>>,
    pub loading: bool,
    pub error: Option<String>,
}

pub(super) async fn view(client: Client, id: String, generation: u64, state: watch::Sender<Model>) {
    let Ok(id) = id.parse::<matrix_sdk::ruma::OwnedRoomId>() else {
        return;
    };
    let Some(room) = client.get_room(&id) else {
        return;
    };
    let mut changes = room.subscribe_to_updates();
    let mut snapshot = Members {
        generation,
        rows: Arc::default(),
        loading: true,
        error: None,
    };
    state.send_modify(|s| s.members = Some(snapshot.clone()));
    loop {
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            room.members(RoomMemberships::JOIN | RoomMemberships::INVITE),
        )
        .await;
        snapshot.loading = false;
        match result {
            Ok(Ok(members)) => {
                let mut rows: Vec<_> = members
                    .into_iter()
                    .map(|member| Member {
                        invited: member.membership()
                            == &matrix_sdk::ruma::events::room::member::MembershipState::Invite,
                        user_id: member.user_id().to_string(),
                        name: member.name().to_owned(),
                        avatar_url: member.avatar_url().map(ToOwned::to_owned),
                        role: match member.suggested_role_for_power_level() {
                            RoomMemberRole::Creator => "Creator",
                            RoomMemberRole::Administrator => "Admin",
                            RoomMemberRole::Moderator => "Moderator",
                            RoomMemberRole::User => "",
                        }
                        .into(),
                    })
                    .collect();
                rows.sort_by_cached_key(|m| (m.invited, m.name.to_lowercase(), m.user_id.clone()));
                snapshot.rows = Arc::new(rows);
                snapshot.error = None;
            }
            Ok(Err(_)) => snapshot.error = Some("Could not load room members.".into()),
            Err(_) => snapshot.error = Some("Loading room members timed out.".into()),
        }
        state.send_if_modified(|s| {
            if s.members.as_ref() == Some(&snapshot) {
                return false;
            }
            s.members = Some(snapshot.clone());
            true
        });
        loop {
            match changes.recv().await {
                Ok(update) if affects_members(&update) => break,
                Err(RecvError::Lagged(_)) => break,
                Err(RecvError::Closed) => return,
                _ => {}
            }
        }
        // A fresh store read covers the whole burst of queued room updates.
        while changes.try_recv().is_ok() {}
    }
}
fn relevant(event_type: Option<String>) -> bool {
    matches!(
        event_type.as_deref(),
        Some("m.room.member" | "m.room.power_levels" | "m.room.create" | "m.room.redaction")
    )
}
fn affects_members(update: &RoomUpdate) -> bool {
    let RoomUpdate::Joined { updates, .. } = update else {
        return true;
    };
    let state = match &updates.state {
        matrix_sdk::sync::State::Before(events) | matrix_sdk::sync::State::After(events) => events,
    };
    state
        .iter()
        .any(|e| relevant(e.get_field("type").ok().flatten()))
        || updates
            .timeline
            .events
            .iter()
            .any(|e| relevant(e.raw().get_field("type").ok().flatten()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    #[tokio::test]
    async fn lazy_members_load_follow_state_updates_and_cancel_without_late_snapshots() {
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
        let event = |kind: &str, key: &str, content: serde_json::Value| json!({"room_id":"!room:example.org","type":kind,"state_key":key,"event_id":format!("${kind}-{key}"),"sender":"@test:example.org","origin_server_ts":1,"content":content});
        let own = event(
            "m.room.member",
            "@test:example.org",
            json!({"membership":"join","displayname":"Zoe"}),
        );
        let other = event(
            "m.room.member",
            "@other:example.org",
            json!({"membership":"join","displayname":"Alice","avatar_url":"mxc://example.org/avatar"}),
        );
        let invited = event(
            "m.room.member",
            "@invite:example.org",
            json!({"membership":"invite","displayname":"Invite","avatar_url":"mxc://example.org/invite"}),
        );
        let departed = event(
            "m.room.member",
            "@departed:example.org",
            json!({"membership":"leave"}),
        );
        let banned = event(
            "m.room.member",
            "@banned:example.org",
            json!({"membership":"ban"}),
        );
        Mock::given(method("GET"))
            .and(path_regex(".*/members"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"chunk":[own.clone(), other, invited, departed, banned]}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        let response = Arc::new(std::sync::Mutex::new(
            json!({"next_batch":"seed","rooms":{"join":{"!room:example.org":{"state":{"events":[
            event("m.room.create","",json!({"creator":"@test:example.org","room_version":"10"})), own,
            event("m.room.power_levels","",json!({"users":{"@test:example.org":100,"@other:example.org":50}}))
        ]},"timeline":{"events":[],"limited":false}}}}}),
        ));
        let body = response.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(body.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        // Ordinary /sync assumes full membership; reproduce sliding sync's lazy state.
        client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap()
            .mark_members_missing();
        let (state, mut snapshots) = watch::channel(Model::default());
        let worker = tokio::spawn(view(
            client.clone(),
            "!room:example.org".into(),
            7,
            state.clone(),
        ));
        let loaded = tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|s| s.members.as_ref().is_some_and(|m| !m.loading)),
        )
        .await
        .unwrap()
        .unwrap()
        .members
        .clone()
        .unwrap();
        assert_eq!(loaded.rows.len(), 3, "snapshot: {loaded:?}");
        assert_eq!(loaded.rows[0].name, "Alice");
        assert_eq!(loaded.rows[0].role, "Moderator");
        assert!(loaded.rows[0].avatar_url.is_some());
        assert_eq!(loaded.rows[1].role, "Admin");
        assert!(!loaded.rows[0].invited && !loaded.rows[1].invited);
        assert_eq!(loaded.rows[2].user_id, "@invite:example.org");
        assert!(loaded.rows[2].invited && loaded.rows[2].avatar_url.is_some());
        *response.lock().unwrap() = json!({"next_batch":"leave","rooms":{"join":{"!room:example.org":{"state":{"events":[event("m.room.member","@other:example.org",json!({"membership":"leave"}))]},"timeline":{"events":[],"limited":false}}}}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|s| s.members.as_ref().is_some_and(|m| m.rows.len() == 2)),
        )
        .await
        .unwrap()
        .unwrap();
        // Accepting, renewing, then declining an invitation updates the same roster.
        for (batch, membership) in [
            ("accept", "join"),
            ("renew", "invite"),
            ("decline", "leave"),
        ] {
            *response.lock().unwrap() = json!({"next_batch":batch,"rooms":{"join":{"!room:example.org":{"state":{"events":[event("m.room.member","@invite:example.org",json!({"membership":membership,"displayname":"Invite"}))]},"timeline":{"events":[],"limited":false}}}}});
            client
                .sync_once(SyncSettings::default().timeout(Duration::ZERO))
                .await
                .unwrap();
            tokio::time::timeout(
                Duration::from_secs(5),
                snapshots.wait_for(|s| {
                    s.members.as_ref().is_some_and(|m| {
                        let person = m.rows.iter().find(|m| m.user_id == "@invite:example.org");
                        match membership {
                            "join" => person.is_some_and(|m| !m.invited),
                            "invite" => person.is_some_and(|m| m.invited),
                            _ => person.is_none(),
                        }
                    })
                }),
            )
            .await
            .unwrap()
            .unwrap();
        }
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        state.send_modify(|s| s.members = None);
        *response.lock().unwrap() = json!({"next_batch":"rename","rooms":{"join":{"!room:example.org":{"state":{"events":[event("m.room.member","@test:example.org",json!({"membership":"join","displayname":"Renamed"}))]},"timeline":{"events":[],"limited":false}}}}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        assert!(snapshots.borrow().members.is_none());

        server.verify().await;
        server.reset().await;
        Mock::given(method("GET"))
            .and(path_regex(".*/members"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Fixture refusal"})),
            )
            .mount(&server)
            .await;
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap();
        room.mark_members_missing();
        let worker = tokio::spawn(view(
            client.clone(),
            room.room_id().to_string(),
            8,
            state.clone(),
        ));
        let failed = tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|s| {
                s.members
                    .as_ref()
                    .is_some_and(|m| !m.loading && m.error.is_some())
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .members
        .clone()
        .unwrap();
        assert_eq!(failed.generation, 8);
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());

        server.reset().await;
        let started = Arc::new(tokio::sync::Notify::new());
        let notify = started.clone();
        Mock::given(method("GET"))
            .and(path_regex(".*/members"))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(60))
                    .set_body_json(json!({"chunk":[]}))
            })
            .mount(&server)
            .await;
        let worker = tokio::spawn(view(client, room.room_id().to_string(), 9, state.clone()));
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        assert!(snapshots.borrow().members.as_ref().unwrap().loading);
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        state.send_modify(|s| s.members = None);
        assert!(snapshots.borrow().members.is_none());
    }
}
