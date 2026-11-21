use super::*;
use serde_json::json;
use std::time::Duration;

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
        ("Interconnect".into(), "New Matrix message".into())
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
                Ok(None)
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
        ("Interconnect".into(), "New Matrix message".into())
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
        Vec<String>,
    );
    struct Desktop(Arc<Mutex<Vec<Call>>>);
    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl Desktop {
        fn get_server_information(&self) -> (&str, &str, &str, &str) {
            ("Fixture", "Interconnect", "1", "1.2")
        }

        #[allow(clippy::too_many_arguments)] // freedesktop protocol signature
        fn notify(
            &self,
            app_name: String,
            _replaces_id: u32,
            _app_icon: String,
            summary: String,
            body: String,
            actions: Vec<String>,
            hints: HashMap<String, zbus::zvariant::OwnedVariant>,
            _expire_timeout: i32,
        ) -> zbus::fdo::Result<u32> {
            let reject = summary == "Rejected sample";
            self.0
                .lock()
                .unwrap()
                .push((app_name, summary, body, hints, actions));
            if reject {
                Err(zbus::fdo::Error::Failed(
                    "Simulated desktop rejection".into(),
                ))
            } else {
                Ok(42)
            }
        }

        async fn activate(
            &self,
            id: u32,
            action: &str,
            #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
        ) -> zbus::fdo::Result<()> {
            Self::action_invoked(&emitter, id, action)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
        }

        async fn dismiss(
            &self,
            id: u32,
            #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
        ) -> zbus::fdo::Result<()> {
            Self::notification_closed(&emitter, id, 2)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
        }

        #[zbus(signal)]
        async fn action_invoked(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            id: u32,
            action: &str,
        ) -> zbus::Result<()>;

        #[zbus(signal)]
        async fn notification_closed(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            id: u32,
            reason: u32,
        ) -> zbus::Result<()>;
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
        const CHILD: &str = "INTERCONNECT_NOTIFICATION_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let response = show("Sample room", "<b>hello & goodbye</b>", true)
                .await
                .unwrap()
                .unwrap();
            let clicked = tokio::spawn(response);
            let connection = zbus::Connection::session().await.unwrap();
            let proxy = zbus::Proxy::new(
                &connection,
                "org.freedesktop.Notifications",
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
            )
            .await
            .unwrap();
            proxy
                .call::<_, _, ()>("Activate", &(99u32, "default"))
                .await
                .unwrap();
            proxy
                .call::<_, _, ()>("Activate", &(42u32, "default"))
                .await
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_secs(2), clicked)
                    .await
                    .unwrap()
                    .unwrap()
            );
            test_notification().await.unwrap();
            assert!(
                show("Rejected sample", "Account-free error check", true)
                    .await
                    .is_err()
            );
            let response = show("Dismissed sample", "No navigation", true)
                .await
                .unwrap()
                .unwrap();
            let dismissed = tokio::spawn(response);
            proxy.call::<_, _, ()>("Dismiss", &42u32).await.unwrap();
            assert!(
                !tokio::time::timeout(Duration::from_secs(2), dismissed)
                    .await
                    .unwrap()
                    .unwrap()
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
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].0, "Interconnect");
        assert_eq!(calls[0].4, ["default", "Open notification"]);
        assert_eq!(calls[0].1, "Sample room");
        assert_eq!(calls[0].2, "&lt;b&gt;hello &amp; goodbye&lt;/b&gt;");
        assert_eq!(
            <&str>::try_from(calls[0].3.get("category").unwrap()).unwrap(),
            "im.received"
        );
        assert_eq!(calls[1].2, "Desktop notifications are working.");
        assert!(calls[1].4.is_empty(), "sample has no conversation action");
        assert_eq!(calls[2].1, "Rejected sample");
    }
}

#[tokio::test]
async fn clicks_are_bounded_session_owned_and_independent_of_delivery() {
    let (events, receiver) = mpsc::channel(QUEUE_LIMIT);
    let (_control, selection) = watch::channel(Selection::default());
    let (state, mut updates) = watch::channel(Model::default());
    let service = Service {
        events: receiver,
        selection,
        state,
        started: 0,
    };
    let (delivered, mut handles) = mpsc::channel(QUEUE_LIMIT);
    let task = tokio::spawn(service.run_with(
        || async { Ok(()) },
        move |_, _| {
            let delivered = delivered.clone();
            async move {
                let (click, response) = tokio::sync::oneshot::channel();
                delivered.send(click).await.unwrap();
                Ok(Some(
                    Box::pin(async move { response.await.unwrap_or(false) }) as Response,
                ))
            }
        },
    ));
    let mut observers = Vec::new();
    for index in 0..=QUEUE_LIMIT {
        let mut candidate = parse(event()).unwrap();
        candidate.id = format!("${index}");
        candidate.timestamp = now_ms();
        candidate.thread = Some("$thread".into());
        events.send(candidate).await.ok().unwrap();
        observers.push(handles.recv().await.unwrap());
    }
    // Evict the oldest observer, keeping delivery moving while nobody clicks.
    observers[0].closed().await;
    assert!(observers[0].is_closed());
    observers.pop().unwrap().send(true).unwrap();
    let activation = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            updates.changed().await.unwrap();
            if let Some(activation) = updates.borrow().notification_activation.clone() {
                break activation;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        activation.target,
        Target::Conversation {
            room: "!room:example.org".into(),
            thread: Some("$thread".into()),
            event: Some(format!("${QUEUE_LIMIT}")),
        }
    );
    assert!(activation.is_active());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(
        !activation.is_active(),
        "queued UI clicks expire with the session"
    );
    assert!(observers.iter().all(|observer| observer.is_closed()));
}

fn invitation(room: &str) -> crate::room_membership::Invitation {
    crate::room_membership::Invitation {
        id: room.into(),
        name: "Private invitation room".into(),
        inviter: Some("@inviter:example.org".into()),
        space: false,
    }
}
async fn wait_model(
    updates: &mut watch::Receiver<Model>,
    predicate: impl Fn(&Model) -> bool,
) -> Model {
    tokio::time::timeout(Duration::from_secs(3), updates.wait_for(predicate))
        .await
        .expect("notification worker publishes the expected state")
        .unwrap()
        .clone()
}
#[test]
fn attention_baseline_metadata_and_reinvitation_are_event_driven() {
    let mut attention = Attention::default();
    let mut model = Model::default();
    assert!(attention.observe(&model, 100).is_empty());
    model.invitations_ready = true;
    model.invitations = Arc::new(vec![invitation("!invite:example.org")]);
    let baseline = attention.observe(&model, 100);
    assert_eq!(baseline.len(), 1);
    assert!(!baseline[0].1, "initial pending invitations are silent");
    assert!(attention.observe(&model, 100).is_empty());
    Arc::make_mut(&mut model.invitations)[0].name = "Renamed room".into();
    assert!(
        attention.observe(&model, 100).is_empty(),
        "metadata does not re-alert"
    );
    model.invitations = Arc::default();
    assert!(attention.observe(&model, 100).is_empty());
    model.invitations = Arc::new(vec![invitation("!invite:example.org")]);
    let fresh = attention.observe(&model, 100);
    assert!(fresh[0].1);
    assert_eq!(fresh[0].0.history_id(), baseline[0].0.history_id());
    assert!(
        fresh[0].0.timestamp > baseline[0].0.timestamp,
        "live episodes differ even within one clock tick"
    );
    model.verification.incoming = Some(crate::verification::Incoming {
        flow: "flow".into(),
        device: "OTHER".into(),
    });
    let verification = attention.observe(&model, 100);
    assert_eq!(verification.len(), 1);
    assert!(verification[0].1);
    assert!(attention.observe(&model, 100).is_empty());
}
#[test]
fn category_targets_previews_ids_and_debug_do_not_leak_hidden_previews() {
    let invite = Candidate::invitation(&invitation("!invite:example.org"), 100);
    assert_eq!(
        invite.target(),
        Target::Invitation {
            room: "!invite:example.org".into()
        }
    );
    assert_eq!(
        invite.preview(false),
        ("Interconnect".into(), "New room invitation".into())
    );
    assert!(!invite.entry(false).body.contains("inviter"));
    let verification = Candidate::verification(
        &crate::verification::Incoming {
            flow: "flow".into(),
            device: "DEVICE".into(),
        },
        100,
    );
    assert_eq!(
        verification.target(),
        Target::Verification {
            flow: "flow".into(),
            device: "DEVICE".into()
        }
    );
    assert_eq!(
        verification.preview(false),
        (
            "Interconnect".into(),
            "Device verification requested".into()
        )
    );
    let mut selection = Selection::default();
    assert!(verification.eligible(&selection, 0, 100));
    selection.verification_visible = true;
    assert!(!verification.eligible(&selection, 0, 100));
    assert!(invite.eligible(&selection, 0, 100));
    let mut message = parse(event()).unwrap();
    assert_eq!(
        message.target(),
        Target::Conversation {
            room: "!room:example.org".into(),
            thread: None,
            event: Some("$message".into()),
        }
    );
    assert_eq!(message.entry(false).target, message.target());
    assert_eq!(message.entry(false).body, "New Matrix message");
    message.room = "!a:b:$c".into();
    message.id = "$d".into();
    let mut other = parse(event()).unwrap();
    other.room = "!a:b".into();
    other.id = "$c:$d".into();
    assert_ne!(message.history_id(), other.history_id());
    assert_ne!(invite.history_id(), verification.history_id());
    let mut entry = invite.entry(true);
    entry.title = "preview-title-canary".into();
    entry.body = "preview-body-canary".into();
    let debug = format!("{:?}", Record { entry, read: false });
    assert!(!debug.contains("preview-title-canary"));
    assert!(!debug.contains("preview-body-canary"));
}
#[tokio::test]
async fn inbox_records_when_native_alerts_are_disabled_or_focused_and_is_bounded() {
    let (events, receiver) = mpsc::channel(QUEUE_LIMIT);
    let mut selected = Selection::default();
    selected.notifications.enabled = false;
    selected.notifications.show_preview = false;
    selected.room = Some((1, "!room:example.org".into()));
    selected.conversation_visible = true;
    let (_control, selection) = watch::channel(selected);
    let (state, mut updates) = watch::channel(Model::default());
    let service = Service {
        events: receiver,
        selection,
        state,
        started: 0,
    };
    let task = tokio::spawn(service.run_with(
        || async {
            panic!("disabled native alerts never ask desktop permission");
            #[allow(unreachable_code)]
            Ok(())
        },
        |_, _| async {
            panic!("disabled native alerts never deliver");
            #[allow(unreachable_code)]
            Ok(None)
        },
    ));
    let timestamp = now_ms();
    for index in 0..HISTORY_LIMIT + 20 {
        let mut candidate = parse(event()).unwrap();
        candidate.id = format!("$inbox-{index}");
        candidate.timestamp = timestamp + index as u64;
        events.send(candidate).await.ok().unwrap();
    }
    let model = wait_model(&mut updates, |model| {
        model
            .notification_history
            .first()
            .is_some_and(|entry| entry.timestamp == timestamp + HISTORY_LIMIT as u64 + 19)
    })
    .await;
    assert_eq!(model.notification_history.len(), HISTORY_LIMIT);
    assert!(
        model
            .notification_history
            .iter()
            .all(|entry| entry.title == "Interconnect" && entry.body == "New Matrix message")
    );
    assert!(
        model
            .notification_history
            .iter()
            .all(|entry| entry.timestamp >= timestamp + 20)
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
#[tokio::test]
async fn permission_wait_does_not_delay_history_and_initial_invites_stay_silent() {
    let (events, receiver) = mpsc::channel(QUEUE_LIMIT);
    let (_control, selection) = watch::channel(Selection::default());
    let initial = Model {
        invitations_ready: true,
        invitations: Arc::new(vec![invitation("!initial:example.org")]),
        ..Default::default()
    };
    let (state, mut updates) = watch::channel(initial);
    let publisher = state.clone();
    let service = Service {
        events: receiver,
        selection,
        state,
        started: 0,
    };
    let (authorized, mut started) = mpsc::channel(QUEUE_LIMIT);
    let permission = Arc::new(tokio::sync::Semaphore::new(0));
    let permit = permission.clone();
    let (delivered, mut previews) = mpsc::channel(QUEUE_LIMIT);
    let task = tokio::spawn(service.run_with(
        move || {
            let authorized = authorized.clone();
            let permission = permission.clone();
            async move {
                authorized.send(()).await.unwrap();
                permission.acquire().await.unwrap().forget();
                Ok(())
            }
        },
        move |title, body| {
            let delivered = delivered.clone();
            async move {
                delivered.send((title, body)).await.unwrap();
                Ok(None)
            }
        },
    ));
    wait_model(&mut updates, |model| model.notification_history.len() == 1).await;
    assert!(
        started.try_recv().is_err(),
        "baseline invitations never ask native permission"
    );
    let mut first = parse(event()).unwrap();
    first.timestamp = now_ms();
    events.send(first).await.ok().unwrap();
    started.recv().await.unwrap();
    let mut second = parse(event()).unwrap();
    second.id = "$second".into();
    second.timestamp = now_ms();
    events.send(second).await.ok().unwrap();
    publisher.send_modify(|model| {
        model.invitations = Arc::new(vec![
            invitation("!initial:example.org"),
            invitation("!new:example.org"),
        ]);
        model.verification.incoming = Some(crate::verification::Incoming {
            flow: "flow".into(),
            device: "OTHER".into(),
        });
    });
    let model = wait_model(&mut updates, |model| model.notification_history.len() == 5).await;
    assert!(model.notification_history.iter().any(|entry| matches!(entry.target, Target::Invitation { ref room } if room == "!new:example.org")));
    assert!(model.notification_history.iter().any(
        |entry| matches!(entry.target, Target::Verification { ref flow, .. } if flow == "flow")
    ));
    assert!(
        previews.try_recv().is_err(),
        "permission is still pending while all history is collected"
    );
    permit.add_permits(1);
    previews.recv().await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
#[tokio::test]
async fn invitation_and_verification_clicks_review_only_current_targets() {
    let (events, receiver) = mpsc::channel(QUEUE_LIMIT);
    let (_control, selection) = watch::channel(Selection::default());
    let initial = Model {
        invitations_ready: true,
        ..Default::default()
    };
    let (state, mut updates) = watch::channel(initial);
    let publisher = state.clone();
    let service = Service {
        events: receiver,
        selection,
        state,
        started: 0,
    };
    let (delivered, mut handles) = mpsc::channel(QUEUE_LIMIT);
    let task = tokio::spawn(service.run_with(
        || async { Ok(()) },
        move |title, body| {
            let delivered = delivered.clone();
            async move {
                let (click, response) = tokio::sync::oneshot::channel();
                delivered.send((title, body, click)).await.unwrap();
                Ok(Some(
                    Box::pin(async move { response.await.unwrap_or(false) }) as Response,
                ))
            }
        },
    ));
    // Let the empty initial snapshot establish the silent baseline first.
    let mut barrier = parse(event()).unwrap();
    barrier.timestamp = now_ms();
    events.send(barrier).await.ok().unwrap();
    let (_, _, click) = handles.recv().await.unwrap();
    drop(click);
    publisher.send_modify(|model| {
        model.invitations = Arc::new(vec![invitation("!live:example.org")]);
        model.verification.incoming = Some(crate::verification::Incoming {
            flow: "flow".into(),
            device: "OTHER".into(),
        });
    });
    let (title, _, invitation_click) = handles.recv().await.unwrap();
    assert!(title.contains("Invitation"));
    let (_, _, expired_verification_click) = handles.recv().await.unwrap();
    invitation_click.send(true).unwrap();
    wait_model(&mut updates, |model| {
        model
            .notification_activation
            .as_ref()
            .is_some_and(|activation| {
                activation.target
                    == Target::Invitation {
                        room: "!live:example.org".into(),
                    }
            })
    })
    .await;
    publisher.send_modify(|model| {
        model.notification_activation = None;
        model.verification.incoming = None;
    });
    expired_verification_click.send(true).unwrap();
    let mut barrier = parse(event()).unwrap();
    barrier.id = "$barrier".into();
    barrier.timestamp = now_ms();
    events.send(barrier).await.ok().unwrap();
    let (_, _, click) = handles.recv().await.unwrap();
    drop(click);
    assert!(
        updates.borrow().notification_activation.is_none(),
        "expired flow cannot reactivate verification"
    );
    publisher.send_modify(|model| {
        model.verification.incoming = Some(crate::verification::Incoming {
            flow: "new-flow".into(),
            device: "OTHER".into(),
        })
    });
    let (_, _, verification_click) = handles.recv().await.unwrap();
    verification_click.send(true).unwrap();
    let model = wait_model(&mut updates, |model| {
        model
            .notification_activation
            .as_ref()
            .is_some_and(|activation| {
                activation.target
                    == Target::Verification {
                        flow: "new-flow".into(),
                        device: "OTHER".into(),
                    }
            })
    })
    .await;
    assert_eq!(
        model.verification.incoming.as_ref().unwrap().flow,
        "new-flow"
    );
    assert!(
        matches!(model.verification.phase, crate::verification::Phase::Idle),
        "clicking never accepts the request"
    );
    let activation = model.notification_activation.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!activation.is_active());
}
#[tokio::test]
async fn native_failure_preserves_history_and_verification_suppression_is_independent() {
    let (events, receiver) = mpsc::channel(QUEUE_LIMIT);
    let selected = Selection {
        verification_visible: true,
        ..Default::default()
    };
    let (_control, selection) = watch::channel(selected);
    let (state, mut updates) = watch::channel(Model::default());
    let publisher = state.clone();
    let service = Service {
        events: receiver,
        selection,
        state,
        started: 0,
    };
    let (delivered, mut previews) = mpsc::channel(QUEUE_LIMIT);
    let task = tokio::spawn(service.run_with(
        || async { Ok(()) },
        move |title, body| {
            let delivered = delivered.clone();
            async move {
                delivered.send((title, body)).await.unwrap();
                Err("fixture desktop denial".into())
            }
        },
    ));
    publisher.send_modify(|model| {
        model.verification.incoming = Some(crate::verification::Incoming {
            flow: "reviewing".into(),
            device: "OTHER".into(),
        })
    });
    wait_model(&mut updates, |model| model.notification_history.len() == 1).await;
    assert!(
        previews.try_recv().is_err(),
        "active verification review suppresses native duplicate only"
    );
    let mut candidate = parse(event()).unwrap();
    candidate.timestamp = now_ms();
    events.send(candidate).await.ok().unwrap();
    previews.recv().await.unwrap();
    let model = wait_model(&mut updates, |model| {
        model.notification_error.as_deref() == Some("fixture desktop denial")
    })
    .await;
    assert_eq!(model.notification_history.len(), 2);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[test]
fn cached_and_first_server_invitations_are_silent_before_live_transitions() {
    let mut attention = Attention::default();
    let mut model = Model {
        invitations: Arc::new(vec![invitation("!cached:example.org")]),
        ..Default::default()
    };
    let cached = attention.observe(&model, 100);
    assert_eq!(cached.len(), 1);
    assert!(
        !cached[0].1,
        "offline cached invite enters history silently"
    );
    model.invitations_ready = true;
    Arc::make_mut(&mut model.invitations).push(invitation("!server-baseline:example.org"));
    let server = attention.observe(&model, 100);
    assert_eq!(server.len(), 1);
    assert!(
        !server[0].1,
        "first successful sync does not replay existing invitations"
    );
    Arc::make_mut(&mut model.invitations).push(invitation("!live:example.org"));
    let live = attention.observe(&model, 100);
    assert_eq!(live.len(), 1);
    assert!(live[0].1, "later invitations get native alerts");
}
#[tokio::test]
async fn focused_messages_enter_history_without_native_authorization() {
    let (events, receiver) = mpsc::channel(QUEUE_LIMIT);
    let selected = Selection {
        room: Some((1, "!room:example.org".into())),
        conversation_visible: true,
        ..Default::default()
    };
    assert!(selected.notifications.enabled);
    let (_control, selection) = watch::channel(selected);
    let (state, mut updates) = watch::channel(Model::default());
    let service = Service {
        events: receiver,
        selection,
        state,
        started: 0,
    };
    let task = tokio::spawn(service.run_with(
        || async {
            panic!("focused messages never ask desktop permission");
            #[allow(unreachable_code)]
            Ok(())
        },
        |_, _| async {
            panic!("focused messages never deliver native alerts");
            #[allow(unreachable_code)]
            Ok(None)
        },
    ));
    let mut candidate = parse(event()).unwrap();
    candidate.timestamp = now_ms();
    events.send(candidate).await.ok().unwrap();
    let model = wait_model(&mut updates, |model| model.notification_history.len() == 1).await;
    assert_eq!(model.notification_history[0].title, "Sample room");
    assert!(
        model.notification_history[0]
            .body
            .contains("Sample message")
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[test]
fn legacy_message_identity_recovers_the_original_event_without_mutating_saved_records() {
    let entry = Entry {
        id: format!(
            "message:{}",
            serde_json::to_string(&("!room:example.org", "$original/with+symbols")).unwrap()
        ),
        target: Target::Conversation {
            room: "!room:example.org".into(),
            thread: Some("$thread".into()),
            event: None,
        },
        timestamp: 1,
        title: "A generic preview".into(),
        body: "New Matrix message".into(),
    };
    assert_eq!(
        entry.destination(),
        Target::Conversation {
            room: "!room:example.org".into(),
            thread: Some("$thread".into()),
            event: Some("$original/with+symbols".into()),
        }
    );
    assert!(matches!(
        entry.target,
        Target::Conversation { event: None, .. }
    ));
    let mut explicit = entry;
    explicit.target = Target::Conversation {
        room: "!room:example.org".into(),
        thread: None,
        event: Some("$explicit".into()),
    };
    assert_eq!(
        explicit.destination(),
        explicit.target,
        "explicit event wins over legacy identity"
    );
}
#[test]
fn malformed_or_mismatched_legacy_identities_keep_conversation_navigation() {
    let mut entry = Entry {
        id: String::new(),
        target: Target::Conversation {
            room: "!room:example.org".into(),
            thread: None,
            event: None,
        },
        timestamp: 1,
        title: String::new(),
        body: String::new(),
    };
    for identity in [
        "other:[\"!room:example.org\",\"$event\"]".to_owned(),
        "message:invalid-json".into(),
        "message:[\"!other:example.org\",\"$event\"]".into(),
        "message:[\"!room:example.org\",\"invalid\"]".into(),
        "message:[\"!room:example.org\",\"$event\",\"extra\"]".into(),
        "message:{\"room\":\"!room:example.org\",\"event\":\"$event\"}".into(),
        format!(
            "message:{}",
            serde_json::to_string(&("!room:example.org", "$event\n")).unwrap()
        ),
        format!(
            "message:{}",
            serde_json::to_string(&("!room:example.org", format!("${}", "x".repeat(1024))))
                .unwrap()
        ),
    ] {
        entry.id = identity;
        assert_eq!(
            entry.destination(),
            entry.target,
            "invalid identity {:?}",
            entry.id
        );
    }
    entry.id = "message:[\"!room:example.org\",\"$event\"]".into();
    entry.target = Target::Invitation {
        room: "!room:example.org".into(),
    };
    assert_eq!(
        entry.destination(),
        entry.target,
        "message IDs cannot reinterpret another category"
    );
}
