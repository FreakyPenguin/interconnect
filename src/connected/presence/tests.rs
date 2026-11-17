use super::*;
use matrix_sdk::authentication::matrix::MatrixSession;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn event(user: &str, status: &str) -> Raw<PresenceEvent> {
    Raw::from_json_string(
        json!({
            "type":"m.presence", "sender":user, "content":{"presence":status}
        })
        .to_string(),
    )
    .unwrap()
}

fn project(events: &[Raw<PresenceEvent>], state: &watch::Sender<Model>) {
    let mut users = (*state.borrow().presence).clone();
    apply(events, &mut users);
    publish(&users, state);
}

#[test]
fn projection_deduplicates_and_keeps_unknown_distinct_from_offline() {
    let (state, mut updates) = watch::channel(Model::default());
    project(
        &[
            event("@alice:example.org", "online"),
            event("@bob:example.org", "unavailable"),
        ],
        &state,
    );
    assert!(updates.has_changed().unwrap());
    assert_eq!(
        updates.borrow_and_update().presence["@alice:example.org"],
        Status::Online
    );
    project(&[event("@alice:example.org", "online")], &state);
    assert!(!updates.has_changed().unwrap());
    project(
        &[
            event("@alice:example.org", "offline"),
            event("@bob:example.org", "future-status"),
        ],
        &state,
    );
    assert_eq!(
        state.borrow().presence["@alice:example.org"],
        Status::Offline
    );
    assert_eq!(state.borrow().presence["@bob:example.org"], Status::Unknown);
    clear(&state);
    assert!(state.borrow().presence.is_empty());
    updates.borrow_and_update();
    clear(&state);
    assert!(!updates.has_changed().unwrap());
}

#[test]
fn projection_is_bounded_and_ignores_malformed_events() {
    let (state, _updates) = watch::channel(Model::default());
    let events: Vec<_> = (0..MAX_USERS + 10)
        .map(|n| event(&format!("@user{n}:example.org"), "online"))
        .collect();
    project(&events, &state);
    assert_eq!(state.borrow().presence.len(), MAX_USERS);
    project(&[Raw::from_json_string("{}".into()).unwrap()], &state);
    assert_eq!(state.borrow().presence.len(), MAX_USERS);
}

async fn client(server: &MockServer) -> Client {
    Mock::given(method("GET"))
        .and(path("/_matrix/client/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
        .mount(server)
        .await;
    let client = Client::builder()
        .homeserver_url(server.uri())
        .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
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
    client
}

#[tokio::test]
async fn filtered_sync_delivers_presence_and_to_device_before_advancing_and_cancels() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let (seen, mut received) = tokio::sync::mpsc::unbounded_channel();
    let handler = client.add_event_handler(move |_event: matrix_sdk::ruma::events::key::verification::request::ToDeviceKeyVerificationRequestEvent| {
        let seen = seen.clone();
        async move { let _ = seen.send(()); }
    });
    let _guard = client.event_handler_drop_guard(handler);
    Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
        .and(wiremock::matchers::query_param_is_missing("since"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "next_batch":"presence-1",
            "presence":{"events":[{"type":"m.presence","sender":"@alice:example.org","content":{"presence":"online"}}]},
            "to_device":{"events":[{"type":"m.key.verification.request","sender":"@alice:example.org","content":{
                "from_device":"OTHER", "methods":["m.sas.v1"], "timestamp":1, "transaction_id":"presence-test"
            }}]}
        }))).expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/sync"))
        .and(wiremock::matchers::query_param("since", "presence-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(60))
                .set_body_json(json!({"next_batch":"presence-2"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (state, mut updates) = watch::channel(Model::default());
    let owned = client.clone();
    let task = tokio::spawn(async move { run(&owned, &state).await });
    tokio::time::timeout(
        Duration::from_secs(10),
        updates.wait_for(|s| s.presence.get("@alice:example.org") == Some(&Status::Online)),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), received.recv())
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if server.received_requests().await.unwrap().iter().any(|r| {
                r.url
                    .query_pairs()
                    .any(|(k, v)| k == "since" && v == "presence-1")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let requests = server.received_requests().await.unwrap();
    let sync = requests
        .iter()
        .filter(|r| r.url.path().ends_with("/sync"))
        .collect::<Vec<_>>();
    assert_eq!(sync.len(), 2);
    for request in sync {
        let query: BTreeMap<_, _> = request.url.query_pairs().into_owned().collect();
        assert_eq!(query["timeout"], "30000");
        let filter: serde_json::Value = serde_json::from_str(&query["filter"]).unwrap();
        assert_eq!(filter["room"]["rooms"], json!([]));
        assert_eq!(filter["account_data"]["types"], json!([]));
        assert!(filter.get("presence").is_none());
    }
}

#[tokio::test]
async fn failed_sync_hides_stale_status_and_recovery_restores_unchanged_users() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let server = MockServer::start().await;
    let client = client(&server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
        .respond_with(move |request: &wiremock::Request| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            if n > 0 {
                assert!(request.url.query_pairs().any(|(key, value)| key == "since" && value == "one"));
            }
            match n {
                0 => ResponseTemplate::new(200).set_body_json(json!({
                    "next_batch":"one", "presence":{"events":[{
                        "type":"m.presence", "sender":"@alice:example.org", "content":{"presence":"online"}
                    }]}
                })),
                1 => ResponseTemplate::new(500).set_body_json(json!({"errcode":"M_UNKNOWN", "error":"test"})),
                2 => ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one"})),
                _ => ResponseTemplate::new(200).set_delay(Duration::from_secs(60)).set_body_json(json!({"next_batch":"two"})),
            }
        }).mount(&server).await;
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, &state).await });
    for online in [true, false, true] {
        tokio::time::timeout(
            Duration::from_secs(10),
            updates.wait_for(|s| !s.presence.is_empty() == online),
        )
        .await
        .unwrap()
        .unwrap();
    }
    assert_eq!(
        updates.borrow().presence["@alice:example.org"],
        Status::Online
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(calls.load(Ordering::SeqCst) >= 3);
}
