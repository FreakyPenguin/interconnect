//! Explicit leave/forget and guarded disbanding of a single Matrix space.
//! Disbanding is a sequence of acknowledged writes, not server-side history erasure.
use matrix_sdk::{
    Room, RoomState,
    ruma::{
        OwnedRoomId, OwnedUserId,
        api::client::{
            directory::{get_room_visibility, set_room_visibility},
            membership::{kick_user, leave_room},
            room::Visibility,
            state::{get_state_events, send_state_event},
        },
        events::{
            StateEventType,
            room::{
                join_rules::{JoinRule, RoomJoinRulesEventContent},
                power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent},
            },
            space::child::SpaceChildEventContent,
        },
    },
};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Leave,
    Disband,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub action: Action,
    pub name: String,
    pub members: usize,
    pub invitations: usize,
    pub knocks: usize,
    pub children: usize,
    pub blocked: Option<String>,
    room: OwnedRoomId,
    signature: BTreeMap<(String, String), String>,
}
impl Plan {
    /// Local preview for the application's account-independent demo mode.
    pub fn demo(
        action: Action,
        name: String,
        members: usize,
        invitations: usize,
        children: usize,
    ) -> Self {
        Self {
            action,
            name,
            members,
            invitations,
            knocks: 0,
            children,
            blocked: None,
            room: matrix_sdk::ruma::room_id!("!demo:example.org").to_owned(),
            signature: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub left: bool,
    pub forgotten: bool,
    pub warning: Option<String>,
}
struct Snapshot {
    name: String,
    signature: BTreeMap<(String, String), String>,
    members: Vec<OwnedUserId>,
    invitations: usize,
    knocks: usize,
    joined: usize,
    children: Vec<OwnedRoomId>,
    blocked: Option<String>,
    closed: bool,
}
fn write_config() -> matrix_sdk::config::RequestConfig {
    matrix_sdk::config::RequestConfig::default()
        .disable_retry()
        .timeout(Duration::from_secs(20))
}
async fn snapshot(room: &Room) -> Result<Snapshot, String> {
    if !room.is_space() || room.state() != RoomState::Joined {
        return Err("This space is no longer joined.".into());
    }
    let response = room
        .client()
        .send(get_state_events::v3::Request::new(
            room.room_id().to_owned(),
        ))
        .await
        .map_err(|_| "Could not read current space state. Try again.")?;
    let mut signature = BTreeMap::new();
    let mut name = room.name().unwrap_or_else(|| room.room_id().to_string());
    let mut members = vec![];
    let mut children = vec![];
    let mut invitations = 0;
    let mut knocks = 0;
    let mut joined = 0;
    let mut own_joined = false;
    let mut levels = None;
    let mut space = false;
    let mut closed = false;
    for raw in response.room_state {
        let event: Value = serde_json::from_str(raw.json().get())
            .map_err(|_| "Could not read a space state event.")?;
        let kind = event["type"]
            .as_str()
            .ok_or("Invalid space state event type.")?;
        if !matches!(
            kind,
            "m.room.create"
                | "m.room.name"
                | "m.room.member"
                | "m.room.power_levels"
                | "m.room.join_rules"
                | "m.space.child"
        ) {
            continue;
        }
        let key = event["state_key"]
            .as_str()
            .ok_or("Invalid space state key.")?;
        let id = event["event_id"]
            .as_str()
            .ok_or("Missing space state event ID.")?;
        if signature
            .insert((kind.into(), key.into()), id.into())
            .is_some()
        {
            return Err("Duplicate state events prevent a safe space operation.".into());
        }
        let content = &event["content"];
        match kind {
            "m.room.create" => space = content["type"] == "m.space",
            "m.room.name" => {
                if let Some(value) = content["name"].as_str() {
                    name = value.to_owned();
                }
            }
            "m.room.join_rules" => closed = content["join_rule"] == "invite",
            "m.room.power_levels" => {
                if event["unsigned"]["redacted_because"].is_null() {
                    let content: RoomPowerLevelsEventContent =
                        serde_json::from_value(content.clone())
                            .map_err(|_| "Could not read space permissions.")?;
                    let creators = room.creators().ok_or("Unknown space creators.")?;
                    let rules = room.clone_info().room_version_rules_or_default();
                    levels = Some(RoomPowerLevels::new(
                        content.into(),
                        &rules.authorization,
                        creators,
                    ));
                }
            }
            "m.room.member" => {
                let user = key
                    .parse::<OwnedUserId>()
                    .map_err(|_| "Invalid space member ID.")?;
                let membership = content["membership"]
                    .as_str()
                    .ok_or("Unknown space membership state.")?;
                if user == room.own_user_id() {
                    own_joined = membership == "join";
                    continue;
                }
                match membership {
                    "join" => joined += 1,
                    "invite" => invitations += 1,
                    "knock" => knocks += 1,
                    "leave" | "ban" => continue,
                    _ => {
                        return Err(
                            "Unknown membership state prevents a safe space operation.".into()
                        );
                    }
                }
                members.push(user);
            }
            "m.space.child" => {
                let child: SpaceChildEventContent = serde_json::from_value(content.clone())
                    .map_err(|_| "Could not read a space child link.")?;
                if !child.via.is_empty() {
                    children.push(
                        key.parse::<OwnedRoomId>()
                            .map_err(|_| "Invalid space child ID.")?,
                    );
                }
            }
            _ => {}
        }
    }
    if !space || !own_joined {
        return Err(
            "You are no longer joined to this space, or its type cannot be verified.".into(),
        );
    }
    if name.trim().is_empty() {
        name = room.room_id().to_string();
    }
    members.sort();
    children.sort();
    let blocked = match levels {
        None => Some("Current space permissions could not be verified.".into()),
        Some(levels) => {
            if !levels.user_can_send_state(room.own_user_id(), StateEventType::RoomJoinRules)
                || !levels.user_can_send_state(room.own_user_id(), StateEventType::SpaceChild)
            {
                Some(
                    "Disbanding requires permission to close access and unlink all child rooms."
                        .into(),
                )
            } else if members
                .iter()
                .any(|u| !levels.user_can_kick_user(room.own_user_id(), u))
            {
                Some("You cannot remove every other member or pending invitation. Another administrator or protected creator may prevent disbanding.".into())
            } else {
                None
            }
        }
    };
    Ok(Snapshot {
        name,
        signature,
        members,
        invitations,
        knocks,
        joined,
        children,
        blocked,
        closed,
    })
}
pub(crate) async fn prepare(room: &Room, action: Action) -> Result<Plan, String> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let state = snapshot(room).await?;
        Ok(Plan {
            action,
            name: state.name,
            members: state.joined,
            invitations: state.invitations,
            knocks: state.knocks,
            children: state.children.len(),
            blocked: if action == Action::Disband {
                state.blocked
            } else {
                None
            },
            room: room.room_id().to_owned(),
            signature: state.signature,
        })
    })
    .await
    .map_err(|_| "Checking the space timed out. Try again.".to_owned())?
}
/// Update only this space after a successful single-room leave. SDK `leave()`
/// also leaves predecessors; these actions must never target another room.
async fn mark_left(room: &Room) -> Result<(), String> {
    room.update_and_save_room_info(|mut info| {
        info.mark_as_left();
        info.mark_state_partially_synced();
        info.mark_members_missing();
        (info, Default::default())
    })
    .await
    .map_err(|_| {
        "The server accepted leaving, but local state could not be updated. Allow sync to catch up."
            .into()
    })
}
pub(crate) async fn retry_forget(room: &Room) -> Outcome {
    if !room.is_space() || !matches!(room.state(), RoomState::Left | RoomState::Banned) {
        return Outcome { left: false, forgotten: false, warning: Some("The space's left state is not available yet. Allow sync to catch up before retrying forgetting.".into()) };
    }
    let result = tokio::time::timeout(Duration::from_secs(30), room.forget()).await;
    let warning = match result {
        Ok(Ok(())) => None,
        _ => Some("You have left the space, but forgetting it did not complete. Allow sync to catch up and retry forgetting.".into()),
    };
    Outcome {
        left: true,
        forgotten: warning.is_none(),
        warning,
    }
}
pub(crate) async fn execute(room: &Room, plan: Plan) -> Result<Outcome, String> {
    if room.room_id() != plan.room {
        return Err("The space target changed. Reload before continuing.".into());
    }
    let mut left = false;
    let action = plan.action;
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        let current = snapshot(room).await?;
        if action == Action::Disband && current.signature != plan.signature { return Err("The space changed. Reload this confirmation and review it again.".into()); }
        if action == Action::Disband {
            if let Some(reason) = current.blocked { return Err(reason); }
            let client = room.client();
            // Stop public/restricted joining before removing the confirmed members.
            let request = send_state_event::v3::Request::new(room.room_id().to_owned(), &matrix_sdk::ruma::events::EmptyStateKey, &RoomJoinRulesEventContent::new(JoinRule::Invite)).map_err(|_| "Could not prepare closed access.")?;
            client.send(request).with_request_config(write_config()).await.map_err(|_| "Could not close space access.")?;
            let visibility = client.send(get_room_visibility::v3::Request::new(room.room_id().to_owned())).await.map_err(|_| "Could not check directory visibility.")?.visibility;
            if visibility == Visibility::Public {
                client.send(set_room_visibility::v3::Request::new(room.room_id().to_owned(), Visibility::Private)).with_request_config(write_config()).await.map_err(|_| "Could not unlist the space from this homeserver's directory.")?;
            }
            for user in current.members {
                let mut request = kick_user::v3::Request::new(room.room_id().to_owned(), user);
                request.reason = Some("Space disbanded".into());
                client.send(request).with_request_config(write_config()).await.map_err(|_| "Could not remove a member or pending invitation.")?;
            }
            for child in current.children {
                let request = send_state_event::v3::Request::new(room.room_id().to_owned(), child.as_str(), &SpaceChildEventContent::new(vec![])).map_err(|_| "Could not prepare a child unlink.")?;
                client.send(request).with_request_config(write_config()).await.map_err(|_| "Could not unlink a child room or subspace.")?;
            }
            // Do not abandon the space if another member/link appeared during the writes.
            let final_state = snapshot(room).await?;
            if !final_state.members.is_empty() || !final_state.children.is_empty() || !final_state.closed {
                return Err("Members, links, or access changed during disbanding. You remain a member; reload to review what remains.".into());
            }
            let visibility = client.send(get_room_visibility::v3::Request::new(room.room_id().to_owned())).await.map_err(|_| "Could not verify directory visibility.")?.visibility;
            if visibility != Visibility::Private { return Err("The space was republished while disbanding. You remain a member; reload before retrying.".into()); }
        }
        room.client().send(leave_room::v3::Request::new(room.room_id().to_owned())).with_request_config(write_config()).await.map_err(|_| "Could not confirm leaving this space. Check membership before retrying.")?;
        left = true;
        mark_left(room).await?;
        let outcome = retry_forget(room).await;
        Ok(outcome)
    }).await;
    if matches!(room.state(), RoomState::Left | RoomState::Banned) {
        left = true;
    }
    match result {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(message)) if left => Ok(Outcome { left: true, forgotten: false, warning: Some(message) }),
        Err(_) if left => Ok(Outcome { left: true, forgotten: false, warning: Some("You left the space, but cleanup timed out. Retry forgetting after sync catches up.".into()) }),
        Ok(Err(message)) => Err(if action == Action::Disband { format!("{message} Disbanding may have partially completed. Reload to review access, members, and links before retrying.") } else { message }),
        Err(_) => Err("The operation timed out and may have partially completed. Check membership, access, members, and links before retrying.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{
        Client,
        authentication::matrix::MatrixSession,
        config::{RequestConfig, SyncSettings},
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    struct Fixture {
        server: MockServer,
        room: Room,
        state: Arc<Mutex<Vec<Value>>>,
    }
    fn state_event(kind: &str, key: &str, content: Value) -> Value {
        json!({"type":kind,"state_key":key,"content":content,"event_id":format!("${kind}-{key}"),"sender":"@owner:example.org","origin_server_ts":1})
    }
    async fn fixture(power: u32, peer_creator: bool, race: bool) -> Fixture {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .request_config(RequestConfig::default().retry_limit(0))
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .build()
            .await
            .unwrap();
        client.matrix_auth().restore_session(serde_json::from_value::<MatrixSession>(json!({"user_id":"@owner:example.org","device_id":"TEST","access_token":"fake"})).unwrap(), Default::default()).await.unwrap();
        let mut create = json!({"creator":"@owner:example.org","type":"m.space","room_version":if peer_creator {"12"} else {"10"},"predecessor":{"room_id":"!older:example.org","event_id":"$old"}});
        if peer_creator {
            create["additional_creators"] = json!(["@peer:example.org"]);
        }
        let mut events = vec![
            state_event("m.room.create", "", create),
            state_event("m.room.name", "", json!({"name":"Test space"})),
            state_event(
                "m.room.power_levels",
                "",
                json!({"users":{"@owner:example.org":power},"state_default":50}),
            ),
            state_event("m.room.join_rules", "", json!({"join_rule":"public"})),
            state_event(
                "m.room.member",
                "@owner:example.org",
                json!({"membership":"join"}),
            ),
            state_event(
                "m.room.member",
                "@other:example.org",
                json!({"membership":"join"}),
            ),
            state_event(
                "m.room.member",
                "@invited:example.org",
                json!({"membership":"invite"}),
            ),
            state_event(
                "m.room.member",
                "@knock:example.org",
                json!({"membership":"knock"}),
            ),
            state_event(
                "m.space.child",
                "!child:example.org",
                json!({"via":["example.org"]}),
            ),
        ];
        if peer_creator {
            events.push(state_event(
                "m.room.member",
                "@peer:example.org",
                json!({"membership":"join"}),
            ));
        }
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!space:example.org":{
                "state":{"events":events},"timeline":{"events":[],"limited":false},"account_data":{"events":[]}
            }}}}))).mount(&server).await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!space:example.org"))
            .unwrap();
        let state = Arc::new(Mutex::new(events));
        let listed = Arc::new(Mutex::new(true));
        let current = state.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/rooms/!space:example.org/state"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(json!(*current.lock().unwrap()))
            })
            .mount(&server)
            .await;
        let visibility = listed.clone();
        Mock::given(method("GET")).and(path("/_matrix/client/v3/directory/list/room/!space:example.org"))
            .respond_with(move |_: &wiremock::Request| ResponseTemplate::new(200).set_body_json(json!({"visibility":if *visibility.lock().unwrap() {"public"} else {"private"}}))).mount(&server).await;
        let visibility = listed.clone();
        Mock::given(method("PUT"))
            .and(path(
                "/_matrix/client/v3/directory/list/room/!space:example.org",
            ))
            .and(wiremock::matchers::body_json(
                json!({"visibility":"private"}),
            ))
            .respond_with(move |_: &wiremock::Request| {
                *visibility.lock().unwrap() = false;
                ResponseTemplate::new(200).set_body_json(json!({}))
            })
            .mount(&server)
            .await;
        let current = state.clone();
        Mock::given(method("PUT"))
            .and(path(
                "/_matrix/client/v3/rooms/!space:example.org/state/m.room.join_rules/",
            ))
            .and(wiremock::matchers::body_json(json!({"join_rule":"invite"})))
            .respond_with(move |_: &wiremock::Request| {
                let mut state = current.lock().unwrap();
                let event = state
                    .iter_mut()
                    .find(|e| e["type"] == "m.room.join_rules")
                    .unwrap();
                event["event_id"] = json!("$closed");
                event["content"] = json!({"join_rule":"invite"});
                ResponseTemplate::new(200).set_body_json(json!({"event_id":"$closed"}))
            })
            .mount(&server)
            .await;
        let current = state.clone();
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/rooms/!space:example.org/kick"))
            .respond_with(move |request: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let mut state = current.lock().unwrap();
                let event = state
                    .iter_mut()
                    .find(|e| e["type"] == "m.room.member" && e["state_key"] == body["user_id"])
                    .unwrap();
                event["content"]["membership"] = json!("leave");
                event["event_id"] = json!(format!("$removed-{}", body["user_id"]));
                ResponseTemplate::new(200).set_body_json(json!({}))
            })
            .mount(&server)
            .await;
        let current = state.clone();
        Mock::given(method("PUT")).and(path("/_matrix/client/v3/rooms/!space:example.org/state/m.space.child/!child:example.org"))
            .and(wiremock::matchers::body_json(json!({"via":[]})))
            .respond_with(move |_: &wiremock::Request| {
                let mut state = current.lock().unwrap(); let event = state.iter_mut().find(|e| e["type"] == "m.space.child").unwrap();
                event["content"] = json!({"via":[]}); event["event_id"] = json!("$unlinked");
                if race { state.push(state_event("m.room.member", "@late:example.org", json!({"membership":"join"}))); }
                ResponseTemplate::new(200).set_body_json(json!({"event_id":"$unlinked"}))
            }).mount(&server).await;
        for endpoint in ["leave", "forget"] {
            Mock::given(method("POST"))
                .and(path(format!(
                    "/_matrix/client/v3/rooms/!space:example.org/{endpoint}"
                )))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .mount(&server)
                .await;
        }
        Fixture {
            server,
            room,
            state,
        }
    }
    #[tokio::test]
    async fn leave_forgets_only_this_space_and_preserves_members_and_children() {
        let fixture = fixture(0, false, false).await;
        let plan = prepare(&fixture.room, Action::Leave).await.unwrap();
        assert!(plan.blocked.is_none());
        assert_eq!(
            (plan.members, plan.invitations, plan.knocks, plan.children),
            (1, 1, 1, 1)
        );
        // Other people's activity must not prevent an account-only leave.
        fixture.state.lock().unwrap().push(state_event(
            "m.room.member",
            "@late:example.org",
            json!({"membership":"join"}),
        ));
        let before = fixture.state.lock().unwrap().clone();
        let outcome = execute(&fixture.room, plan).await.unwrap();
        assert!(outcome.left && outcome.forgotten);
        assert_eq!(*fixture.state.lock().unwrap(), before);
        let requests = fixture.server.received_requests().await.unwrap();
        let writes = requests
            .iter()
            .filter(|r| r.method != "GET" && !r.url.path().contains("/keys/"))
            .map(|r| r.url.path())
            .collect::<Vec<_>>();
        assert_eq!(
            writes,
            vec![
                "/_matrix/client/v3/rooms/!space:example.org/leave",
                "/_matrix/client/v3/rooms/!space:example.org/forget"
            ]
        );
        assert!(
            fixture
                .room
                .client()
                .get_room(fixture.room.room_id())
                .is_none()
        );
    }
    #[tokio::test]
    async fn disband_closes_unlists_removes_members_unlinks_children_then_leaves_and_forgets() {
        let fixture = fixture(100, false, false).await;
        let plan = prepare(&fixture.room, Action::Disband).await.unwrap();
        assert!(plan.blocked.is_none());
        let outcome = execute(&fixture.room, plan).await.unwrap();
        assert!(outcome.left && outcome.forgotten);
        let requests = fixture.server.received_requests().await.unwrap();
        let writes = requests
            .iter()
            .filter(|r| r.method != "GET" && !r.url.path().contains("/keys/"))
            .collect::<Vec<_>>();
        assert!(writes[0].url.path().contains("m.room.join_rules"));
        assert!(writes[1].url.path().contains("directory/list/room"));
        assert_eq!(
            writes
                .iter()
                .filter(|r| r.url.path().ends_with("/kick"))
                .count(),
            3
        );
        assert!(
            writes[writes.len() - 3]
                .url
                .path()
                .contains("m.space.child")
        );
        assert!(writes[writes.len() - 2].url.path().ends_with("/leave"));
        assert!(writes.last().unwrap().url.path().ends_with("/forget"));
        assert!(
            !requests
                .iter()
                .any(|r| r.url.path().contains("/rooms/!child:")
                    || r.url.path().contains("/rooms/!older:"))
        );
    }
    #[tokio::test]
    async fn permission_or_creator_protection_and_stale_confirmation_prevent_writes() {
        for (power, creator) in [(0, false), (100, true)] {
            let fixture = fixture(power, creator, false).await;
            let plan = prepare(&fixture.room, Action::Disband).await.unwrap();
            assert!(plan.blocked.is_some());
            assert!(execute(&fixture.room, plan).await.is_err());
            assert!(
                fixture
                    .server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .all(|r| r.method == "GET" || r.url.path().contains("/keys/"))
            );
        }
        let fixture = fixture(100, false, false).await;
        let plan = prepare(&fixture.room, Action::Disband).await.unwrap();
        fixture.state.lock().unwrap().push(state_event(
            "m.room.member",
            "@new:example.org",
            json!({"membership":"join"}),
        ));
        assert!(
            execute(&fixture.room, plan)
                .await
                .unwrap_err()
                .contains("space changed")
        );
        assert!(
            fixture
                .server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| r.method == "GET" || r.url.path().contains("/keys/"))
        );
    }
    #[tokio::test]
    async fn partial_disband_and_concurrent_new_members_never_abandon_the_space() {
        let fixture = fixture(100, false, false).await;
        let plan = prepare(&fixture.room, Action::Disband).await.unwrap();
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/rooms/!space:example.org/kick"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Revoked"})),
            )
            .with_priority(1)
            .expect(1)
            .mount(&fixture.server)
            .await;
        let error = execute(&fixture.room, plan).await.unwrap_err();
        assert!(error.contains("partially completed"));
        assert!(
            fixture
                .server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| !r.url.path().ends_with("/leave") && !r.url.path().ends_with("/forget"))
        );
        let raced = self::fixture(100, false, true).await;
        let plan = prepare(&raced.room, Action::Disband).await.unwrap();
        assert!(
            execute(&raced.room, plan)
                .await
                .unwrap_err()
                .contains("remain a member")
        );
        assert_eq!(raced.room.state(), RoomState::Joined);
        assert!(
            raced
                .server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| !r.url.path().ends_with("/leave"))
        );
    }
    #[tokio::test]
    async fn failed_forget_can_retry_without_repeating_the_leave_or_disband() {
        let fixture = fixture(100, false, false).await;
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let responses = attempts.clone();
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/rooms/!space:example.org/forget"))
            .respond_with(move |_: &wiremock::Request| {
                if responses.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    ResponseTemplate::new(403)
                        .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Temporary denial"}))
                } else {
                    ResponseTemplate::new(200).set_body_json(json!({}))
                }
            })
            .with_priority(1)
            .expect(2)
            .mount(&fixture.server)
            .await;
        let plan = prepare(&fixture.room, Action::Leave).await.unwrap();
        let outcome = execute(&fixture.room, plan).await.unwrap();
        assert!(outcome.left && !outcome.forgotten && outcome.warning.is_some());
        assert_eq!(fixture.room.state(), RoomState::Left);
        assert!(retry_forget(&fixture.room).await.forgotten);
        assert_eq!(
            fixture
                .server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|r| r.url.path().ends_with("/leave"))
                .count(),
            1
        );
    }
}
