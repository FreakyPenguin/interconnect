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

#[tokio::test]
async fn navigator_demand_is_independent_of_selection_and_does_not_truncate_contacts() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let mut selected = selection(&[]);
    selected.room = None;
    selected.presence.poll_interval = Some(Duration::from_secs(60));
    selected.presence.navigator_users = (0..100)
        .map(|n| format!("@peer{n}:example.org"))
        .chain([
            "bad".into(),
            "@test:example.org".into(),
            "@peer0:example.org".into(),
        ])
        .collect();
    let navigator = requested(&client, &selected);
    assert_eq!(navigator.demand.users.len(), 100);
    assert!(!navigator.demand.users.contains(&"bad".into()));
    assert!(!navigator.demand.users.contains(&"@test:example.org".into()));
    selected.room = Some((2, "!group:example.org".into()));
    selected.members = Some(3);
    assert_eq!(requested(&client, &selected), navigator);
    selected.presence.users = (0..100)
        .map(|n| format!("@member{n}:example.org"))
        .collect();
    assert_eq!(requested(&client, &selected).demand.users.len(), 150);
    selected.presence.poll_interval = None;
    assert_eq!(requested(&client, &selected).demand.users.len(), 50);
    selected.shutdown = true;
    assert!(requested(&client, &selected).demand.users.is_empty());
}

#[test]
fn a_large_navigator_cache_contracts_without_evicting_requested_contacts() {
    let now = tokio::time::Instant::now();
    let mut cache: BTreeMap<_, _> = (0..MAX_USERS + 10)
        .map(|n| (format!("@peer{n}:example.org"), (Status::Online, now)))
        .collect();
    let all = cache.keys().cloned().collect::<Vec<_>>();
    trim_cache(&mut cache, &all);
    assert_eq!(cache.len(), MAX_USERS + 10);
    let requested = vec!["@peer0:example.org".into()];
    trim_cache(&mut cache, &requested);
    assert_eq!(cache.len(), MAX_USERS);
    assert!(cache.contains_key(&requested[0]));
}

#[tokio::test]
async fn viewport_demand_combines_with_people_and_navigator_without_the_page_cap() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let mut selected = selection(&[]);
    selected.presence.users = (0..100)
        .map(|n| format!("@member{n}:example.org"))
        .collect();
    selected.presence.timeline_users = (0..75)
        .map(|n| format!("@sender{n}:example.org"))
        .chain([
            "bad".into(),
            "@test:example.org".into(),
            "@sender0:example.org".into(),
        ])
        .collect();
    selected.presence.navigator_users =
        vec!["@sender0:example.org".into(), "@dm:example.org".into()];
    selected.presence.poll_interval = Some(Duration::from_secs(60));
    let request = requested(&client, &selected);
    assert_eq!(request.demand.users.len(), 126);
    assert_eq!(request.demand.timeline_users.len(), 74);
    assert!(!request.demand.users.contains(&"bad".into()));
    assert!(!request.demand.users.contains(&"@test:example.org".into()));
    selected.room = None;
    assert_eq!(
        requested(&client, &selected).demand.users,
        ["@dm:example.org", "@sender0:example.org"]
    );
    selected.shutdown = true;
    assert!(requested(&client, &selected).demand.users.is_empty());
}

#[tokio::test]
async fn viewport_senders_reuse_cache_and_superseded_threads_cannot_publish_late_results() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/presence/[^/]+/status"))
        .respond_with(move |_: &wiremock::Request| {
            count.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"presence":"online"}))
        })
        .mount(&server)
        .await;
    let started = Arc::new(tokio::sync::Notify::new());
    let notify = started.clone();
    let carol_calls = Arc::new(AtomicUsize::new(0));
    let carol_count = carol_calls.clone();
    Mock::given(method("GET"))
        .and(path(
            "/_matrix/client/v3/presence/@carol:example.org/status",
        ))
        .respond_with(move |_: &wiremock::Request| {
            if carol_count.fetch_add(1, Ordering::SeqCst) == 0 {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(200))
                    .set_body_json(json!({"presence":"offline"}))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({"presence":"online"}))
            }
        })
        .with_priority(1)
        .mount(&server)
        .await;
    let mut selected = selection(&[]);
    selected.presence.timeline_users = vec![
        "@alice:example.org".into(),
        "@alice:example.org".into(),
        "@bob:example.org".into(),
    ];
    let (control, desired) = watch::channel(selected);
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, desired, &state).await });
    ready(&mut updates, "@bob:example.org", Status::Online).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    control.send_modify(|s| s.presence.timeline_users = vec!["@bob:example.org".into()]);
    tokio::time::sleep(Duration::from_millis(40)).await;
    control.send_modify(|s| s.presence.timeline_users = vec!["@alice:example.org".into()]);
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "scrolling reuses fresh snapshots"
    );
    control.send_modify(|s| {
        s.thread = Some((2, "$thread:example.org".into()));
        s.presence.timeline_users = vec!["@carol:example.org".into()];
    });
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .unwrap();
    control.send_modify(|s| s.thread.as_mut().unwrap().0 += 1);
    ready(&mut updates, "@carol:example.org", Status::Online).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(carol_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        updates.borrow().presence.get("@carol:example.org"),
        Some(&Status::Online)
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn viewport_cancellation_preserves_completed_navigator_reads_from_a_slow_batch() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    Mock::given(method("GET"))
        .and(path(
            "/_matrix/client/v3/presence/@alice:example.org/status",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(500))
                .set_body_json(json!({"presence":"offline"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/presence/@bob:example.org/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"presence":"online"})))
        .expect(1)
        .mount(&server)
        .await;
    let mut selected = selection(&[]);
    selected.presence.timeline_users = vec!["@alice:example.org".into()];
    selected.presence.navigator_users = vec!["@bob:example.org".into()];
    selected.presence.poll_interval = Some(Duration::from_secs(60));
    let (control, desired) = watch::channel(selected);
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, desired, &state).await });
    tokio::time::timeout(
        Duration::from_secs(3),
        updates.wait_for(|model| {
            model.presence_loading
                && model.presence.get("@bob:example.org") == Some(&Status::Online)
        }),
    )
    .await
    .unwrap()
    .unwrap();
    control.send_modify(|s| s.presence.timeline_users.clear());
    ready(&mut updates, "@bob:example.org", Status::Online).await;
    tokio::time::sleep(Duration::from_millis(550)).await;
    assert!(!updates.borrow().presence.contains_key("@alice:example.org"));
    assert_eq!(
        updates.borrow().presence.get("@bob:example.org"),
        Some(&Status::Online)
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    server.verify().await;
}

#[tokio::test]
async fn polling_refreshes_every_navigator_contact_without_an_open_room() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/presence/[^/]+/status"))
        .respond_with(move |_: &wiremock::Request| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({
                "presence": if n < 75 { "online" } else { "unavailable" }
            }))
        })
        .mount(&server)
        .await;
    let mut selected = selection(&[]);
    selected.room = None;
    selected.presence.navigator_users = (0..75).map(|n| format!("@peer{n}:example.org")).collect();
    selected.presence.poll_interval = Some(Duration::from_millis(150));
    let (control, desired) = watch::channel(selected);
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, desired, &state).await });
    ready(&mut updates, "@peer74:example.org", Status::Online).await;
    assert_eq!(calls.load(Ordering::SeqCst), 75);
    assert_eq!(updates.borrow().presence.len(), 75);
    control.send_modify(|s| s.room = Some((2, "!group:example.org".into())));
    ready(&mut updates, "@peer74:example.org", Status::Away).await;
    assert_eq!(calls.load(Ordering::SeqCst), 150);
    assert!(
        updates
            .borrow()
            .presence
            .values()
            .all(|s| *s == Status::Away)
    );
    control.send_modify(|s| s.shutdown = true);
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 150);
    assert!(!updates.borrow().presence_loading);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn periodic_refresh_bypasses_cache_reschedules_on_change_and_stops_when_disabled() {
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
    let mut selected = selection(&["@alice:example.org"]);
    selected.presence.poll_interval = Some(Duration::from_secs(5));
    let (control, desired) = watch::channel(selected);
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, desired, &state).await });
    ready(&mut updates, "@alice:example.org", Status::Online).await;
    control.send_modify(|s| s.presence.poll_interval = Some(Duration::from_millis(80)));
    ready(&mut updates, "@alice:example.org", Status::Away).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the periodic refresh bypasses the fresh 60-second cache"
    );
    control.send_modify(|s| s.presence.poll_interval = None);
    tokio::time::sleep(Duration::from_millis(180)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "disabling removes the pending deadline"
    );
    control.send_modify(|s| {
        s.presence.users.clear();
        s.presence.poll_interval = Some(Duration::from_millis(20));
    });
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2, "empty views never poll");
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
async fn slow_periodic_reads_never_overlap_and_navigation_cancels_their_results_and_timer() {
    let server = MockServer::start().await;
    let client = client(&server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let started = Arc::new(tokio::sync::Notify::new());
    let notified = started.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/presence/[^/]+/status"))
        .respond_with(move |_: &wiremock::Request| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return ResponseTemplate::new(200).set_body_json(json!({"presence":"online"}));
            }
            notified.notify_one();
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(250))
                .set_body_json(json!({"presence":"offline"}))
        })
        .mount(&server)
        .await;
    let mut selected = selection(&["@alice:example.org"]);
    selected.presence.poll_interval = Some(Duration::from_millis(40));
    let (control, desired) = watch::channel(selected);
    let (state, mut updates) = watch::channel(Model::default());
    let task = tokio::spawn(async move { run(&client, desired, &state).await });
    ready(&mut updates, "@alice:example.org", Status::Online).await;
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "one slow batch spans multiple intervals without overlap"
    );
    control.send_modify(|s| {
        s.room = None;
        s.presence.users.clear();
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        updates.borrow().presence.get("@alice:example.org"),
        Some(&Status::Online),
        "cancelled periodic results never enter the cache"
    );
    assert!(!updates.borrow().presence_loading);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
