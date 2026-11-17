use super::super::{PresenceDemand, Selection};
use super::*;
use matrix_sdk::authentication::matrix::MatrixSession;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

async fn client(server: &MockServer) -> Client {
    Mock::given(method("GET"))
        .and(path("/_matrix/client/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
        .mount(server)
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
fn selection(users: &[&str]) -> Selection {
    Selection {
        room: Some((1, "!room:example.org".into())),
        presence: PresenceDemand {
            users: users.iter().map(|user| (*user).into()).collect(),
            ..Default::default()
        },
        ..Default::default()
    }
}
async fn ready(updates: &mut watch::Receiver<Model>, user: &str, status: Status) {
    tokio::time::timeout(
        Duration::from_secs(3),
        updates.wait_for(|state| {
            !state.presence_loading
                && state.presence.get(user).copied().unwrap_or_default() == status
        }),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn opening_views_uses_cache_and_explicit_refresh_rechecks_without_sync_or_idle_requests() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/presence/[^/]+/status"))
        .respond_with(move |_: &wiremock::Request| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(json!({"presence":if n == 0 {"online"} else {"unavailable"}}))
        })
        .mount(&server)
        .await;
    let (control, desired) = watch::channel(selection(&["@alice:example.org"]));
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, desired, &state).await });
    ready(&mut updates, "@alice:example.org", Status::Online).await;
    control.send_modify(|s| s.room.as_mut().unwrap().0 += 1);
    control.send_modify(|s| s.notifications.enabled = false);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "cached navigation and settled views make no requests"
    );
    control.send_modify(|s| s.presence.refresh += 1);
    ready(&mut updates, "@alice:example.org", Status::Away).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| !request.url.path().ends_with("/sync"))
    );
}

#[tokio::test]
async fn superseded_room_results_and_shutdown_cannot_publish_late_presence() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let started = Arc::new(tokio::sync::Notify::new());
    let notify = started.clone();
    Mock::given(method("GET"))
        .and(path(
            "/_matrix/client/v3/presence/@alice:example.org/status",
        ))
        .respond_with(move |_: &wiremock::Request| {
            notify.notify_one();
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_json(json!({"presence":"online"}))
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/presence/@bob:example.org/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"presence":"offline"})))
        .mount(&server)
        .await;
    let (control, desired) = watch::channel(selection(&["@alice:example.org"]));
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, desired, &state).await });
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .unwrap();
    control.send_modify(|s| {
        s.room = Some((2, "!other:example.org".into()));
        s.presence.users = vec!["@bob:example.org".into()];
    });
    ready(&mut updates, "@bob:example.org", Status::Offline).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!updates.borrow().presence.contains_key("@alice:example.org"));
    control.send_modify(|s| {
        s.presence.users = vec!["@alice:example.org".into()];
        s.presence.refresh += 1;
    });
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .unwrap();
    control.send_modify(|s| s.shutdown = true);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!updates.borrow().presence.contains_key("@alice:example.org"));
    assert!(!updates.borrow().presence_loading);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn lookup_failures_hide_the_requested_cached_status_and_refresh_can_retry() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/presence/[^/]+/status"))
        .respond_with(
            move |_: &wiremock::Request| match count.fetch_add(1, Ordering::SeqCst) {
                0 => ResponseTemplate::new(200).set_body_json(json!({"presence":"online"})),
                1 => ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"presence disabled"})),
                _ => ResponseTemplate::new(200).set_body_json(json!({"presence":"offline"})),
            },
        )
        .mount(&server)
        .await;
    let (control, desired) = watch::channel(selection(&["@alice:example.org"]));
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, desired, &state).await });
    ready(&mut updates, "@alice:example.org", Status::Online).await;
    control.send_modify(|s| s.presence.refresh += 1);
    ready(&mut updates, "@alice:example.org", Status::Unknown).await;
    control.send_modify(|s| s.room.as_mut().unwrap().0 += 1);
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "failure is cached instead of automatically retried"
    );
    control.send_modify(|s| s.presence.refresh += 1);
    ready(&mut updates, "@alice:example.org", Status::Offline).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn demand_is_bounded_and_rejects_self_invalid_and_duplicate_users() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let mut selected = selection(&[
        "bad",
        "@test:example.org",
        "@alice:example.org",
        "@alice:example.org",
    ]);
    assert_eq!(
        requested(&client, &selected).demand.users,
        ["@alice:example.org"]
    );
    selected.presence.users = (0..100).map(|n| format!("@user{n}:example.org")).collect();
    assert_eq!(
        requested(&client, &selected).demand.users.len(),
        MAX_REQUESTED
    );
    selected.shutdown = true;
    assert!(requested(&client, &selected).demand.users.is_empty());
    for (input, output) in [
        ("online", Status::Online),
        ("unavailable", Status::Away),
        ("offline", Status::Offline),
        ("future", Status::Unknown),
    ] {
        assert_eq!(Status::from_presence(&PresenceState::from(input)), output);
    }
}
