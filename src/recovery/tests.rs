use super::*;
use matrix_sdk::sliding_sync::VersionBuilder;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path_regex},
};

async fn fixture(server: &MockServer) -> Client {
    fixture_device(server, "RECOVERY").await
}

async fn fixture_device(server: &MockServer, device: &str) -> Client {
    Mock::given(method("GET"))
        .and(path_regex(".*/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(".*/keys/upload"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"one_time_key_counts":{"signed_curve25519":50}})),
        )
        .mount(server)
        .await;
    let client = Client::builder()
        .homeserver_url(server.uri())
        .sliding_sync_version_builder(VersionBuilder::Native)
        .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
        .build()
        .await
        .unwrap();
    client.matrix_auth().restore_session(serde_json::from_value(json!({
                "user_id":"@recovery:example.org", "device_id":device, "access_token":"test-only-token"
    })).unwrap(), Default::default()).await.unwrap();
    client
        .encryption()
        .wait_for_e2ee_initialization_tasks()
        .await;
    client
}

#[test]
fn debug_redacts_all_secret_containers() {
    let key = Secret::new("sensitive-fixture".into());
    let snapshot = Snapshot {
        key: Some(key.clone()),
        ..Snapshot::default()
    };
    for debug in [
        format!("{key:?}"),
        format!("{snapshot:?}"),
        format!("{:?}", Action::Recover(key)),
    ] {
        assert!(!debug.contains("sensitive-fixture"));
        assert!(debug.contains("redacted"));
    }
}

#[tokio::test]
async fn existing_secret_storage_is_never_replaced_by_create() {
    let server = MockServer::start().await;
    let client = fixture(&server).await;
    Mock::given(method("GET"))
        .and(path_regex(".*/account_data/m.secret_storage.default_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"key":"existing"})))
        .mount(&server)
        .await;
    let (state, _) = watch::channel(Model::default());
    let error = perform(&client, Action::Create, &state).await.unwrap_err();
    assert!(error.contains("already exists"));
    assert!(
        !server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.method == "PUT")
    );
}

#[tokio::test]
async fn replacement_requires_all_secrets_and_upload_requires_active_backup() {
    let server = MockServer::start().await;
    let client = fixture(&server).await;
    let (state, _) = watch::channel(Model::default());
    assert!(
        perform(&client, Action::Replace, &state)
            .await
            .unwrap_err()
            .contains("Recover your existing secrets")
    );
    assert!(
        perform(&client, Action::Upload, &state)
            .await
            .unwrap_err()
            .contains("first")
    );
}

#[tokio::test]
async fn worker_rejects_stale_commands_and_stops_during_network_work() {
    let server = MockServer::start().await;
    let client = fixture(&server).await;
    Mock::given(method("GET"))
        .and(path_regex(".*/account_data/m.secret_storage.default_key"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"missing"}))
                .set_delay(Duration::from_secs(60)),
        )
        .mount(&server)
        .await;
    let (state, mut snapshots) = watch::channel(Model::default());
    let stop = CancellationToken::new();
    let worker = tokio::spawn(run(client, state.clone(), stop.clone()));
    let snapshot = snapshots
        .wait_for(|s| s.recovery.control.is_some())
        .await
        .unwrap()
        .recovery
        .clone();
    let control = snapshot.control.unwrap();
    control
        .send(snapshot.revision + 17, Action::Create)
        .unwrap();
    tokio::task::yield_now().await;
    assert!(!snapshots.borrow().recovery.busy);
    control.send(snapshot.revision, Action::Create).unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        snapshots.wait_for(|s| s.recovery.busy),
    )
    .await
    .unwrap()
    .unwrap();
    // A stale acknowledgement must not discard a key from a newer operation.
    state.send_modify(|s| s.recovery.key = Some(Secret::new("fixture-key".into())));
    control
        .send(snapshot.revision, Action::Acknowledge)
        .unwrap();
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
    assert!(snapshots.borrow().recovery.key.is_none());
    assert!(snapshots.borrow().recovery.control.is_none());
    assert!(control.send(snapshot.revision, Action::Create).is_err());
}

#[tokio::test]
async fn unsaved_key_survives_commands_until_current_acknowledgement() {
    let server = MockServer::start().await;
    let client = fixture(&server).await;
    let (state, mut snapshots) = watch::channel(Model::default());
    let stop = CancellationToken::new();
    let worker = tokio::spawn(run(client, state.clone(), stop.clone()));
    let snapshot = snapshots
        .wait_for(|s| s.recovery.control.is_some())
        .await
        .unwrap()
        .recovery
        .clone();
    let control = snapshot.control.unwrap();
    state.send_modify(|s| s.recovery.key = Some(Secret::new("fixture-key".into())));
    control.send(snapshot.revision, Action::Replace).unwrap();
    tokio::task::yield_now().await;
    assert!(snapshots.borrow().recovery.key.is_some());
    control
        .send(snapshot.revision + 1, Action::Acknowledge)
        .unwrap();
    tokio::task::yield_now().await;
    assert!(snapshots.borrow().recovery.key.is_some());
    control
        .send(snapshot.revision, Action::Acknowledge)
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        snapshots.wait_for(|s| s.recovery.key.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    stop.cancel();
    worker.await.unwrap();
}

/// Exercise actual SDK encryption and secret storage, with only HTTP mocked.
#[tokio::test]
async fn create_recover_and_replace_round_trip_preserves_the_backup() {
    use serde_json::Value;
    use std::{collections::BTreeMap, sync::Mutex};
    let server = MockServer::start().await;
    let account_data = Arc::new(Mutex::new(BTreeMap::<String, Value>::new()));
    let fail_refresh = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let fail_after_write = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let data = account_data.clone();
    let failed = fail_refresh.clone();
    let armed = fail_after_write.clone();
    Mock::given(method("PUT"))
        .and(path_regex(".*/account_data/.*"))
        .respond_with(move |request: &wiremock::Request| {
            data.lock()
                .unwrap()
                .insert(request.url.path().to_owned(), request.body_json().unwrap());
            if request
                .url
                .path()
                .ends_with("/m.secret_storage.default_key")
                && armed.load(Ordering::Relaxed)
            {
                failed.store(true, Ordering::Relaxed);
            }
            ResponseTemplate::new(200).set_body_json(json!({}))
        })
        .mount(&server)
        .await;
    let data = account_data.clone();
    let failed = fail_refresh.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/account_data/.*"))
        .respond_with(move |request: &wiremock::Request| {
            if failed.load(Ordering::Relaxed) {
                return ResponseTemplate::new(503)
                    .set_body_json(json!({"errcode":"M_UNKNOWN","error":"fixture unavailable"}));
            }
            if let Some(value) = data.lock().unwrap().get(request.url.path()) {
                ResponseTemplate::new(200).set_body_json(value)
            } else {
                ResponseTemplate::new(404)
                    .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"missing"}))
            }
        })
        .mount(&server)
        .await;
    let signing = Arc::new(Mutex::new(json!({})));
    let keys = signing.clone();
    Mock::given(method("POST"))
        .and(path_regex(".*/keys/device_signing/upload"))
        .respond_with(move |request: &wiremock::Request| {
            *keys.lock().unwrap() = request.body_json().unwrap();
            ResponseTemplate::new(200).set_body_json(json!({}))
        })
        .mount(&server)
        .await;
    let keys = signing.clone();
    Mock::given(method("POST"))
        .and(path_regex(".*/keys/query"))
        .respond_with(move |_: &wiremock::Request| {
            let keys = keys.lock().unwrap();
            let mut response = json!({"device_keys":{}, "failures":{}});
            for (one, many) in [
                ("master_key", "master_keys"),
                ("self_signing_key", "self_signing_keys"),
                ("user_signing_key", "user_signing_keys"),
            ] {
                if let Some(key) = keys.get(one) {
                    response[many] = json!({"@recovery:example.org":key});
                }
            }
            ResponseTemplate::new(200).set_body_json(response)
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(".*/keys/signatures/upload"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"failures":{}})))
        .mount(&server)
        .await;
    let backup = Arc::new(Mutex::new(None::<Value>));
    let version = backup.clone();
    Mock::given(method("POST"))
        .and(path_regex(".*/room_keys/version"))
        .respond_with(move |request: &wiremock::Request| {
            let mut body: Value = request.body_json().unwrap();
            body["version"] = json!("1");
            body["count"] = json!(0);
            body["etag"] = json!("fixture");
            *version.lock().unwrap() = Some(body);
            ResponseTemplate::new(200).set_body_json(json!({"version":"1"}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let version = backup.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/room_keys/version(/1)?"))
        .respond_with(move |_: &wiremock::Request| {
            if let Some(body) = &*version.lock().unwrap() {
                ResponseTemplate::new(200).set_body_json(body)
            } else {
                ResponseTemplate::new(404)
                    .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"missing"}))
            }
        })
        .mount(&server)
        .await;
    let first = fixture(&server).await;
    first
        .encryption()
        .bootstrap_cross_signing(None)
        .await
        .unwrap();
    let (state, _) = watch::channel(Model::default());
    let (key, _) = perform(&first, Action::Create, &state).await.unwrap();
    let key = key.unwrap();
    assert_eq!(
        first.encryption().recovery().state(),
        RecoveryState::Enabled
    );
    assert_eq!(first.encryption().backups().state(), BackupState::Enabled);
    let original_backup = backup.lock().unwrap().clone();

    let second = fixture_device(&server, "RESTORED").await;
    assert!(
        !second
            .encryption()
            .cross_signing_status()
            .await
            .unwrap()
            .is_complete()
    );
    // A wrong key must preserve the account's existing recovery data.
    assert!(
        perform(
            &second,
            Action::Recover(Secret::new("wrong-key".into())),
            &state
        )
        .await
        .is_err()
    );
    perform(&second, Action::Recover(key.clone()), &state)
        .await
        .unwrap();
    assert!(
        second
            .encryption()
            .cross_signing_status()
            .await
            .unwrap()
            .is_complete()
    );
    assert_eq!(
        second.encryption().recovery().state(),
        RecoveryState::Enabled
    );
    assert_eq!(second.encryption().backups().state(), BackupState::Enabled);
    let (replacement, _) = perform(&second, Action::Replace, &state).await.unwrap();
    let replacement = replacement.unwrap();
    assert_ne!(key.expose(), replacement.expose());
    assert!(perform(&first, Action::Recover(key), &state).await.is_err());
    perform(&first, Action::Recover(replacement), &state)
        .await
        .unwrap();
    assert_eq!(*backup.lock().unwrap(), original_backup);

    // Failure after committing the new default key must still deliver that key.
    state.send_modify(|s| s.recovery.key = None);
    fail_after_write.store(true, Ordering::Relaxed);
    assert!(perform(&second, Action::Replace, &state).await.is_err());
    let retained = state
        .borrow()
        .recovery
        .key
        .clone()
        .expect("committed key must survive a failed state refresh");
    fail_refresh.store(false, Ordering::Relaxed);
    fail_after_write.store(false, Ordering::Relaxed);
    perform(&first, Action::Recover(retained), &state)
        .await
        .unwrap();
    assert_eq!(*backup.lock().unwrap(), original_backup);
}
