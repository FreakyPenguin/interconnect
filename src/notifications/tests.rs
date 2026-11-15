use super::*;
use serde_json::json;

fn event() -> serde_json::Value {
    json!({"type":"m.room.message", "event_id":"$message", "sender":"@other:example.org",
        "origin_server_ts":200_000, "content":{"msgtype":"m.text", "body":"Sample message"}})
}
fn parse(value: serde_json::Value) -> Option<Candidate> {
    Candidate::parse(
        serde_json::from_value(value).unwrap(),
        "!room:example.org",
        "Sample room",
        "@me:example.org",
    )
}

#[test]
fn rejects_own_messages_edits_redactions_and_non_messages() {
    assert!(parse(event()).is_some());
    for kind in ["m.room.member", "m.reaction", "m.room.redaction"] {
        let mut value = event();
        value["type"] = kind.into();
        assert!(parse(value).is_none());
    }
    let mut value = event();
    value["sender"] = "@me:example.org".into();
    assert!(parse(value).is_none());
    let mut value = event();
    value["content"]["m.relates_to"] = json!({"rel_type":"m.replace", "event_id":"$original"});
    assert!(parse(value).is_none());
    let mut value = event();
    value["unsigned"] = json!({"redacted_because":{}});
    assert!(parse(value).is_none());
    let mut value = event();
    value["type"] = "m.room.encrypted".into();
    value["content"] = json!({"ciphertext":"not preview text"});
    let candidate = parse(value).unwrap();
    assert_eq!(candidate.body, "Encrypted message");
    assert_eq!(
        candidate.preview(false),
        ("Chatclient".into(), "New Matrix message".into())
    );
}

#[test]
fn freshness_focus_thread_and_settings_policy() {
    let candidate = parse(event()).unwrap();
    let mut selection = Selection::default();
    assert!(candidate.eligible(&selection, 100_000, 200_000));
    assert!(!candidate.eligible(&selection, 200_001, 200_000));
    assert!(!candidate.eligible(&selection, 100_000, 320_001));
    assert!(!candidate.eligible(&selection, 0, 79_999));
    selection.room = Some((1, candidate.room.clone()));
    selection.conversation_visible = true;
    assert!(!candidate.eligible(&selection, 100_000, 200_000));
    let mut reply = parse(event()).unwrap();
    reply.thread = Some("$root".into());
    assert!(reply.eligible(&selection, 100_000, 200_000));
    selection.thread = Some((2, "$different".into()));
    assert!(reply.eligible(&selection, 100_000, 200_000));
    selection.thread = Some((3, "$root".into()));
    assert!(!reply.eligible(&selection, 100_000, 200_000));
    selection.conversation_visible = false;
    assert!(reply.eligible(&selection, 100_000, 200_000));
    selection.notifications.enabled = false;
    assert!(!reply.eligible(&selection, 100_000, 200_000));
    selection.notifications.enabled = true;
    selection.shutdown = true;
    assert!(!reply.eligible(&selection, 100_000, 200_000));
}

#[test]
fn deduplication_and_preview_memory_are_bounded() {
    let mut candidate = parse(event()).unwrap();
    let mut seen = Seen::default();
    assert!(seen.insert(&candidate));
    assert!(!seen.insert(&candidate));
    for index in 0..SEEN_LIMIT {
        candidate.id = format!("${index}");
        assert!(seen.insert(&candidate));
    }
    assert_eq!(seen.ids.len(), SEEN_LIMIT);
    assert_eq!(seen.order.len(), SEEN_LIMIT);
    assert!(seen.insert(&parse(event()).unwrap()));
    let mut value = event();
    value["content"]["body"] = "😀\n".repeat(1000).into();
    let candidate = parse(value).unwrap();
    assert_eq!(candidate.body.chars().count(), 300);
    assert!(!candidate.body.contains('\n'));
    assert_eq!(candidate.preview(true).1.chars().count(), 300);
}

#[tokio::test]
async fn pending_permission_rechecks_focus_and_worker_drop_cancels_delivery() {
    let (events, receiver) = mpsc::channel(QUEUE_LIMIT);
    let (control, selection) = watch::channel(Selection::default());
    let (state, _updates) = watch::channel(Model::default());
    let service = Service {
        events: receiver,
        selection,
        state,
        started: 0,
    };
    let (auth_started, mut started) = mpsc::channel(2);
    let permission = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = permission.clone();
    let (delivered, mut previews) = mpsc::channel(2);
    let task = tokio::spawn(service.run_with(
        move || {
            let started = auth_started.clone();
            let permission = permission.clone();
            async move {
                started.send(()).await.unwrap();
                permission.notified().await;
                Ok(())
            }
        },
        move |title, body| {
            let delivered = delivered.clone();
            async move {
                delivered.send((title, body)).await.unwrap();
                Ok(())
            }
        },
    ));
    let mut candidate = parse(event()).unwrap();
    candidate.timestamp = now_ms();
    events.send(candidate).await.ok().unwrap();
    started.recv().await.unwrap();
    control.send_modify(|s| {
        s.conversation_visible = true;
        s.room = Some((1, "!room:example.org".into()));
        s.notifications.show_preview = false;
    });
    release.notify_one();
    let mut candidate = parse(event()).unwrap();
    candidate.timestamp = now_ms();
    candidate.room = "!other:example.org".into();
    events.send(candidate).await.ok().unwrap();
    started.recv().await.unwrap();
    release.notify_one();
    let delivered = tokio::time::timeout(Duration::from_secs(2), previews.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        delivered,
        ("Chatclient".into(), "New Matrix message".into())
    );
    assert!(
        previews.try_recv().is_err(),
        "focused first event must be suppressed after permission"
    );
    let mut candidate = parse(event()).unwrap();
    candidate.timestamp = now_ms();
    candidate.room = "!third:example.org".into();
    events.send(candidate).await.ok().unwrap();
    started.recv().await.unwrap();
    // Teardown drops a pending authorization and the entire bounded queue.
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(events.is_closed());
    assert!(previews.recv().await.is_none());
}

#[tokio::test]
async fn sdk_push_rules_filter_muted_rooms_without_open_timelines() {
    use matrix_sdk::{
        authentication::matrix::MatrixSession,
        config::SyncSettings,
        ruma::{push::Ruleset, user_id},
    };
    use std::sync::{Arc, Mutex};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_matrix/client/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
        .mount(&server)
        .await;
    let mut rules =
        serde_json::to_value(Ruleset::server_default(user_id!("@me:example.org"))).unwrap();
    rules["override"].as_array_mut().unwrap().insert(0, json!({"rule_id":"mute-test", "enabled":true, "default":false,
        "conditions":[{"kind":"event_match", "key":"room_id", "pattern":"!muted:example.org"}], "actions":[]}));
    let joined = json!({"state":{"events":[
        {"type":"m.room.member", "state_key":"@me:example.org", "sender":"@me:example.org", "event_id":"$me", "origin_server_ts":1, "content":{"membership":"join"}},
        {"type":"m.room.member", "state_key":"@other:example.org", "sender":"@other:example.org", "event_id":"$other", "origin_server_ts":1, "content":{"membership":"join"}}
    ]}, "timeline":{"limited":false, "events":[]}});
    let response = Arc::new(Mutex::new(
        json!({"next_batch":"initial", "account_data":{"events":[{"type":"m.push_rules", "content":{"global":rules}}]},
        "rooms":{"join":{"!room:example.org":joined.clone(), "!muted:example.org":joined}}}),
    ));
    let body = response.clone();
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/sync"))
        .respond_with(move |_: &wiremock::Request| {
            ResponseTemplate::new(200).set_body_json(body.lock().unwrap().clone())
        })
        .mount(&server)
        .await;
    let client = Client::builder()
        .homeserver_url(server.uri())
        .build()
        .await
        .unwrap();
    client
        .matrix_auth()
        .restore_session(
            serde_json::from_value::<MatrixSession>(
                json!({"user_id":"@me:example.org", "device_id":"TEST", "access_token":"fake"}),
            )
            .unwrap(),
            Default::default(),
        )
        .await
        .unwrap();
    let (_control, selection) = watch::channel(Selection::default());
    let (state, _updates) = watch::channel(Model::default());
    let mut service = Service::install(&client, selection, state).await;
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    let mut live = event();
    live["origin_server_ts"] = now_ms().into();
    // Mentioning the localpart ensures the default Matrix mention rule matches.
    live["content"]["body"] = "Hello me".into();
    *response.lock().unwrap() = json!({"next_batch":"live", "rooms":{"join":{
        "!room:example.org":{"timeline":{"events":[live.clone()], "limited":false}},
        "!muted:example.org":{"timeline":{"events":[live], "limited":false}}
    }}});
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    let candidate = service
        .events
        .try_recv()
        .expect("SDK notification without a timeline view");
    assert_eq!(candidate.room, "!room:example.org");
    assert_eq!(candidate.body, "Hello me");
    assert!(candidate.eligible(&service.selection.borrow(), service.started, now_ms()));
    assert!(
        service.events.try_recv().is_err(),
        "muted room must not notify"
    );
    // Dropping the receiver leaves an inert callback, with no SDK or runtime
    // owned by it. Subsequent sync must still complete without a consumer.
    drop(service);
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
}

#[cfg(target_os = "linux")]
mod dbus {
    use super::*;
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
        sync::{Arc, Mutex},
    };

    type Call = (
        String,
        String,
        String,
        HashMap<String, zbus::zvariant::OwnedVariant>,
    );
    struct Desktop(Arc<Mutex<Vec<Call>>>);
    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl Desktop {
        #[allow(clippy::too_many_arguments)] // freedesktop protocol signature
        fn notify(
            &self,
            app_name: String,
            _replaces_id: u32,
            _app_icon: String,
            summary: String,
            body: String,
            _actions: Vec<String>,
            hints: HashMap<String, zbus::zvariant::OwnedVariant>,
            _expire_timeout: i32,
        ) -> zbus::fdo::Result<u32> {
            let reject = summary == "Rejected sample";
            self.0
                .lock()
                .unwrap()
                .push((app_name, summary, body, hints));
            if reject {
                Err(zbus::fdo::Error::Failed(
                    "Simulated desktop rejection".into(),
                ))
            } else {
                Ok(42)
            }
        }
    }
    struct Bus(Child);
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[tokio::test]
    async fn delivery_on_private_session_bus() {
        const CHILD: &str = "CHATCLIENT_NOTIFICATION_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            show("Sample room", "<b>hello & goodbye</b>").await.unwrap();
            test_notification().await.unwrap();
            assert!(
                show("Rejected sample", "Account-free error check")
                    .await
                    .is_err()
            );
            return;
        }
        let mut bus = Bus(Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon required for isolated D-Bus validation"));
        let mut address = String::new();
        BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let _desktop = zbus::connection::Builder::address(address.trim())
            .name("org.freedesktop.Notifications")
            .serve_at("/org/freedesktop/Notifications", Desktop(calls.clone()))
            .build()
            .await
            .unwrap();
        // Configure only a fresh subprocess. The runner's environment and host
        // desktop bus remain untouched, including in parallel test execution.
        let executable = std::env::current_exe().unwrap();
        let status = tokio::task::spawn_blocking(move || {
            Command::new(executable)
                .args([
                    "--exact",
                    "notifications::tests::dbus::delivery_on_private_session_bus",
                ])
                .env(CHILD, "1")
                .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
                .status()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(status.success());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "Chatclient");
        assert_eq!(calls[0].1, "Sample room");
        assert_eq!(calls[0].2, "&lt;b&gt;hello &amp; goodbye&lt;/b&gt;");
        assert_eq!(
            <&str>::try_from(calls[0].3.get("category").unwrap()).unwrap(),
            "im.received"
        );
        assert_eq!(calls[1].2, "Desktop notifications are working.");
        assert_eq!(calls[2].1, "Rejected sample");
    }
}
