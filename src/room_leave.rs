//! Fresh, on-demand room-leave warnings. Leaving never forgets a room or deletes keys.
use matrix_sdk::{
    Client, RoomState,
    ruma::{
        api::client::state::get_state_events,
        events::{
            StateEventType,
            room::power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent, UserPowerLevel},
        },
    },
};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    room: String,
    pub encrypted: bool,
    pub privileged: bool,
    pub last_admin: bool,
    pub no_remaining_inviter: bool,
    pub last_member: bool,
    other_members: usize,
}
impl Plan {
    /// Local sample data only; real writes always verify the server's current state.
    pub fn demo(room: String, privileged: bool) -> Self {
        Self {
            room,
            encrypted: true,
            privileged,
            last_admin: privileged,
            no_remaining_inviter: false,
            last_member: false,
            other_members: 4,
        }
    }
    pub fn warnings(&self) -> Vec<&'static str> {
        let mut warnings = vec![
            "You will stop receiving messages and this room will disappear from your joined rooms. Leaving does not delete the room or other members' history.",
            "You may need another invitation to rejoin. Rejoining does not guarantee access to earlier messages.",
        ];
        if self.encrypted {
            warnings.push("This room is encrypted. History may become permanently unreadable if the required decryption keys are lost or unavailable; rejoining does not restore those keys. Before leaving, verify your key backup or access from another trusted device. Leaving does not itself delete your stored keys.");
        }
        if self.privileged {
            warnings.push("You have elevated permissions. Transfer responsibility to another trusted member before leaving. You may not regain your permissions if you rejoin.");
        }
        if self.last_admin {
            warnings.push("No other joined member can currently manage permissions. Leaving may leave the room without anyone able to appoint an administrator or repair access rules.");
        }
        if self.no_remaining_inviter {
            warnings.push("This room does not allow anyone to join freely, and no remaining joined member can currently invite people. You and others may be unable to join again unless an existing invitation or another permitted join route works.");
        }
        if self.last_member {
            warnings.push("You are the last joined member. The room may become permanently inaccessible, and its history may be lost to you. Do not rely on being able to reopen it later.");
        }
        warnings
    }
}
async fn read(client: &Client, id: &str) -> Result<Plan, String> {
    let room_id = id
        .parse::<matrix_sdk::ruma::OwnedRoomId>()
        .map_err(|_| "Invalid room ID.")?;
    let room = client
        .get_room(&room_id)
        .filter(|room| room.state() == RoomState::Joined && !room.is_space())
        .ok_or("You are no longer joined to this room.")?;
    let response = client
        .send(get_state_events::v3::Request::new(room_id))
        .await
        .map_err(|_| "Could not check the consequences of leaving. Reload to try again.")?;
    let mut encrypted = false;
    let mut public = false;
    let mut space = false;
    let mut joined = vec![];
    let mut content: Option<RoomPowerLevelsEventContent> = None;
    for raw in response.room_state {
        let event: serde_json::Value =
            serde_json::from_str(raw.json().get()).map_err(|_| "Could not read room state.")?;
        let value = &event["content"];
        match event["type"].as_str() {
            Some("m.room.encryption") => encrypted = true,
            Some("m.room.create") => space = value["type"] == "m.space",
            Some("m.room.join_rules") => public = value["join_rule"] == "public",
            Some("m.room.power_levels") => {
                content = Some(
                    serde_json::from_value(value.clone())
                        .map_err(|_| "Could not check room permissions.")?,
                )
            }
            Some("m.room.member") if value["membership"] == "join" => {
                let user = event["state_key"]
                    .as_str()
                    .ok_or("Invalid room membership.")?
                    .parse::<matrix_sdk::ruma::OwnedUserId>()
                    .map_err(|_| "Invalid room member ID.")?;
                joined.push(user);
            }
            _ => {}
        }
    }
    let own = room.own_user_id();
    if space || !joined.iter().any(|user| user == own) {
        return Err("You are no longer joined to this room.".into());
    }
    let rules = room.clone_info().room_version_rules_or_default();
    let levels = RoomPowerLevels::new(
        content.into(),
        &rules.authorization,
        room.creators().ok_or("Could not verify room creators.")?,
    );
    let own_admin = levels.user_can_send_state(own, StateEventType::RoomPowerLevels);
    let others: Vec<_> = joined
        .iter()
        .filter(|user| user.as_str() != own.as_str())
        .collect();
    Ok(Plan {
        room: id.to_owned(),
        encrypted,
        privileged: own_admin || levels.for_user(own) > UserPowerLevel::Int(levels.users_default),
        last_admin: own_admin
            && !others
                .iter()
                .any(|user| levels.user_can_send_state(user, StateEventType::RoomPowerLevels)),
        no_remaining_inviter: !public && !others.iter().any(|user| levels.user_can_invite(user)),
        last_member: others.is_empty(),
        other_members: others.len(),
    })
}
pub(crate) async fn prepare(client: &Client, id: &str) -> Result<Plan, String> {
    tokio::time::timeout(Duration::from_secs(30), read(client, id))
        .await
        .map_err(|_| "Checking leave consequences timed out. Reload to try again.".to_owned())?
}
pub(crate) async fn execute(client: &Client, id: &str, plan: Plan) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(45), async {
        if plan.room != id || read(client, id).await? != plan {
            return Err("Room membership, permissions, or encryption changed. Reload and review the warnings before leaving.".into());
        }
        crate::room_membership::leave(client, id).await
    }).await.map_err(|_| "Leaving timed out and may have succeeded. Check your room list before retrying.".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{
        authentication::matrix::MatrixSession,
        config::{RequestConfig, SyncSettings},
    };
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    fn events(
        own_power: u32,
        other_power: Option<u32>,
        public: bool,
        encrypted: bool,
    ) -> Vec<Value> {
        let event = |kind: &str, key: &str, content: Value| json!({"type":kind,"state_key":key,"content":content,"sender":"@me:example.org","event_id":format!("${kind}-{key}"),"origin_server_ts":1});
        let mut events = vec![
            event(
                "m.room.create",
                "",
                json!({"creator":"@me:example.org", "room_version":"10"}),
            ),
            event(
                "m.room.member",
                "@me:example.org",
                json!({"membership":"join"}),
            ),
            event(
                "m.room.join_rules",
                "",
                json!({"join_rule":if public {"public"} else {"invite"}}),
            ),
            event(
                "m.room.power_levels",
                "",
                json!({"users":{"@me:example.org":own_power,"@other:example.org":other_power.unwrap_or(0)},"invite":50}),
            ),
        ];
        if other_power.is_some() {
            events.push(event(
                "m.room.member",
                "@other:example.org",
                json!({"membership":"join"}),
            ));
        }
        if encrypted {
            events.push(event(
                "m.room.encryption",
                "",
                json!({"algorithm":"m.megolm.v1.aes-sha2"}),
            ));
        }
        events
    }
    async fn fixture(events: Vec<Value>) -> (MockServer, Client, Arc<Mutex<Vec<Value>>>) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .request_config(RequestConfig::default().retry_limit(0))
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!room:example.org":{"state":{"events":events},"timeline":{"events":[],"limited":false},"account_data":{"events":[]}}}}}))).mount(&server).await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let state = Arc::new(Mutex::new(events));
        let response = state.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/rooms/!room:example.org/state"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(response.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        (server, client, state)
    }
    #[tokio::test]
    async fn encrypted_privileged_and_last_member_risks_are_distinct() {
        let (_server, client, state) = fixture(events(100, Some(0), false, true)).await;
        let plan = prepare(&client, "!room:example.org").await.unwrap();
        assert!(plan.encrypted && plan.privileged && plan.last_admin && plan.no_remaining_inviter);
        assert!(!plan.last_member);
        let warnings = plan.warnings().join(" ");
        assert!(warnings.contains("permanently unreadable"));
        assert!(warnings.contains("without anyone able to appoint an administrator"));
        assert!(warnings.contains("unable to join again"));
        *state.lock().unwrap() = events(100, None, false, true);
        assert!(
            prepare(&client, "!room:example.org")
                .await
                .unwrap()
                .last_member
        );
        *state.lock().unwrap() = events(100, Some(100), false, true);
        let plan = prepare(&client, "!room:example.org").await.unwrap();
        assert!(!plan.last_admin && !plan.no_remaining_inviter && !plan.last_member);
        *state.lock().unwrap() = events(0, Some(100), true, false);
        let plan = prepare(&client, "!room:example.org").await.unwrap();
        assert!(
            !plan.encrypted && !plan.privileged && !plan.last_admin && !plan.no_remaining_inviter
        );
        assert_eq!(plan.warnings().len(), 2);
    }
    #[tokio::test]
    async fn changed_consequences_and_wrong_targets_prevent_leave_writes() {
        let (server, client, state) = fixture(events(100, Some(100), true, false)).await;
        let plan = prepare(&client, "!room:example.org").await.unwrap();
        *state.lock().unwrap() = events(100, None, false, true);
        assert!(
            execute(&client, "!room:example.org", plan.clone())
                .await
                .unwrap_err()
                .contains("changed")
        );
        assert!(execute(&client, "!other:example.org", plan).await.is_err());
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| !r.url.path().ends_with("/leave"))
        );
    }
    #[tokio::test]
    async fn confirmed_leave_rechecks_state_and_never_forgets_or_deletes_keys() {
        let (server, client, _) = fixture(events(100, None, false, true)).await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/rooms/!room:example.org/leave"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let plan = prepare(&client, "!room:example.org").await.unwrap();
        execute(&client, "!room:example.org", plan).await.unwrap();
        assert!(client.joined_rooms().is_empty());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.url.path().ends_with("/state"))
                .count(),
            2
        );
        assert!(
            requests
                .iter()
                .all(|r| !r.url.path().ends_with("/forget") && r.method != "DELETE")
        );
    }
    #[tokio::test]
    async fn unavailable_state_or_changed_membership_requires_reloading() {
        let (server, client, state) = fixture(events(100, Some(100), false, false)).await;
        state
            .lock()
            .unwrap()
            .retain(|event| event["state_key"] != "@me:example.org");
        assert!(prepare(&client, "!room:example.org").await.is_err());
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/rooms/!room:example.org/state"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"no access"})),
            )
            .with_priority(1)
            .mount(&server)
            .await;
        assert!(
            prepare(&client, "!room:example.org")
                .await
                .unwrap_err()
                .contains("Reload")
        );
    }
}
