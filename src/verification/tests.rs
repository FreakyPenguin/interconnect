use super::*;
use matrix_sdk::{
    authentication::matrix::MatrixSession, config::SyncSettings, sliding_sync::VersionBuilder,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::{sync::Notify, task::JoinHandle};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

#[test]
fn confirmation_requires_current_numbers_and_only_accepts_once() {
    let mut gate = Confirmation::default();
    assert!(!gate.accept((1234, 5678, 9012)));
    gate.expected = Some((1234, 5678, 9012));
    assert!(!gate.accept((1234, 5678, 9013)));
    assert!(!gate.confirmed);
    assert!(gate.accept((1234, 5678, 9012)));
    assert!(!gate.accept((1234, 5678, 9012)));
}

struct Pair {
    clients: [Client; 2],
    states: [watch::Receiver<Model>; 2],
    stop: CancellationToken,
    workers: Vec<JoinHandle<()>>,
    servers: [MockServer; 2],
}
impl Pair {
    async fn new() -> Self {
        let servers = [MockServer::start().await, MockServer::start().await];
        let mut clients = Vec::new();
        for (index, server) in servers.iter().enumerate() {
            Mock::given(method("GET"))
                .and(path("/_matrix/client/versions"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})),
                )
                .mount(server)
                .await;
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/keys/upload"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"one_time_key_counts":{"signed_curve25519":50}})),
                )
                .mount(server)
                .await;
            let client = Client::builder()
                .homeserver_url(server.uri())
                .sliding_sync_version_builder(VersionBuilder::Native)
                .build()
                .await
                .unwrap();
            client.matrix_auth().restore_session(serde_json::from_value::<MatrixSession>(json!({
                "user_id":"@verify:example.org", "device_id": if index == 0 { "FIRST" } else { "SECOND" }, "access_token":"test-only-token",
            })).unwrap(), Default::default()).await.unwrap();
            clients.push(client);
        }
        let clients: [Client; 2] = clients.try_into().unwrap();
        let mut keys = serde_json::Map::new();
        for client in &clients {
            let device = client.encryption().get_own_device().await.unwrap().unwrap();
            keys.insert(
                device.device_id().to_string(),
                serde_json::to_value(device.as_device_keys()).unwrap(),
            );
        }
        let queues = [
            Arc::new(Mutex::new(Vec::<Value>::new())),
            Arc::new(Mutex::new(Vec::<Value>::new())),
        ];
        let notices = [Arc::new(Notify::new()), Arc::new(Notify::new())];
        let stop = CancellationToken::new();
        let mut workers = Vec::new();
        let mut states = Vec::new();
        for index in 0..2 {
            let server = &servers[index];
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/keys/query"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"device_keys":{"@verify:example.org":keys}})),
                )
                .mount(server)
                .await;
            let queue = queues[1 - index].clone();
            let notice = notices[1 - index].clone();
            Mock::given(method("PUT"))
                .and(path_regex("/_matrix/client/v3/sendToDevice/.*"))
                .respond_with(move |request: &wiremock::Request| {
                    let kind = request.url.path().split('/').nth(5).unwrap();
                    let body: Value = request.body_json().unwrap();
                    for content in body["messages"]["@verify:example.org"]
                        .as_object()
                        .unwrap()
                        .values()
                    {
                        queue.lock().unwrap().push(
                            json!({"type":kind, "sender":"@verify:example.org", "content":content}),
                        );
                    }
                    notice.notify_one();
                    ResponseTemplate::new(200).set_body_json(json!({}))
                })
                .mount(server)
                .await;
            let queue = queues[index].clone();
            // The responder is Fn, so the batch counter lives behind a mutex.
            let counter = Mutex::new(0_u64);
            Mock::given(method("GET"))
                .and(path("/_matrix/client/v3/sync"))
                .respond_with(move |_: &wiremock::Request| {
                    let mut batch = counter.lock().unwrap();
                    *batch += 1;
                    let events = std::mem::take(&mut *queue.lock().unwrap());
                    ResponseTemplate::new(200).set_body_json(json!({
                        "next_batch":batch.to_string(), "rooms":{}, "to_device":{"events":events},
                        "device_one_time_keys_count":{"signed_curve25519":50},
                        "device_unused_fallback_key_types":["signed_curve25519"],
                    }))
                })
                .mount(server)
                .await;
            let client = clients[index].clone();
            let notice = notices[index].clone();
            let stopped = stop.clone();
            workers.push(tokio::spawn(async move {
                loop {
                    tokio::select! { _ = stopped.cancelled() => break, _ = notice.notified() => {} }
                    tokio::select! {
                        _ = stopped.cancelled() => break,
                        result = client.sync_once(SyncSettings::default().timeout(Duration::ZERO)) => { result.unwrap(); }
                    }
                }
            }));
            let (sender, receiver) = watch::channel(Model::default());
            states.push(receiver);
            let service = Service::new(clients[index].clone());
            workers.push(tokio::spawn(service.run(sender, stop.clone())));
        }
        let mut pair = Self {
            clients,
            states: states.try_into().unwrap(),
            stop,
            workers,
            servers,
        };
        for index in 0..2 {
            pair.wait(index, |s| s.control.is_some()).await;
        }
        pair
    }
    async fn wait(&mut self, index: usize, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let diagnostic = self.states[index].clone();
        tokio::time::timeout(
            Duration::from_secs(10),
            self.states[index].wait_for(|s| predicate(&s.verification)),
        )
        .await
        .unwrap_or_else(|_| panic!("timed out: {:?}", diagnostic.borrow().verification.phase))
        .unwrap()
        .verification
        .clone()
    }
    fn send(&self, index: usize, action: Action) {
        let snapshot = &self.states[index].borrow().verification;
        snapshot
            .control
            .as_ref()
            .unwrap()
            .send(snapshot.revision, action)
            .unwrap();
    }
    async fn compare(&mut self) -> (Comparison, Comparison) {
        self.send(0, Action::Refresh);
        let devices = self.wait(0, |s| matches!(s.phase, Phase::Devices(_))).await;
        assert!(
            matches!(devices.phase, Phase::Devices(ref list) if list.len() == 1 && list[0].id == "SECOND")
        );
        self.send(0, Action::Start("SECOND".into()));
        let incoming = self
            .wait(1, |s| s.incoming.is_some())
            .await
            .incoming
            .unwrap();
        // Receiving a request must never accept or verify it automatically.
        assert!(matches!(
            self.states[1].borrow().verification.phase,
            Phase::Idle
        ));
        self.send(1, Action::Accept(incoming.flow));
        let first = self
            .wait(0, |s| matches!(s.phase, Phase::Compare { .. }))
            .await;
        let second = self
            .wait(1, |s| matches!(s.phase, Phase::Compare { .. }))
            .await;
        let Phase::Compare {
            comparison: first, ..
        } = first.phase
        else {
            unreachable!()
        };
        let Phase::Compare {
            comparison: second, ..
        } = second.phase
        else {
            unreachable!()
        };
        assert_eq!(first, second);
        assert_eq!(first.emojis.len(), 7);
        (first, second)
    }
    async fn close(self) {
        self.stop.cancel();
        for worker in self.workers {
            tokio::time::timeout(Duration::from_secs(7), worker)
                .await
                .unwrap()
                .unwrap();
        }
    }
}

#[tokio::test]
async fn two_clients_complete_sas_only_after_explicit_matching_confirmations() {
    let mut pair = Pair::new().await;
    let (comparison, _) = pair.compare().await;
    let current = pair.states[0].borrow().verification.clone();
    pair.send(
        0,
        Action::Finish {
            prefetch_keys: true,
        },
    );
    current
        .control
        .as_ref()
        .unwrap()
        .send(current.revision - 1, Action::Confirm(comparison.decimals))
        .unwrap();
    pair.send(0, Action::Confirm((0, 0, 0)));
    // A subsequent refresh acts as a command barrier; it is rejected while busy.
    pair.send(0, Action::Refresh);
    pair.send(0, Action::Confirm(comparison.decimals));
    pair.wait(0, |s| matches!(s.phase, Phase::Confirmed { .. }))
        .await;
    assert_eq!(pair.states[0].borrow().verification.prefetch_keys, None);
    pair.send(0, Action::Confirm(comparison.decimals));
    pair.send(1, Action::Confirm(comparison.decimals));
    pair.wait(0, |s| s.phase == Phase::Complete).await;
    pair.wait(1, |s| s.phase == Phase::Complete).await;
    for state in &pair.states {
        assert_eq!(
            state.borrow().verification.prefetch_keys,
            None,
            "comparison completion must wait for the final-dialog choice"
        );
    }
    let current = pair.states[0].borrow().verification.clone();
    current
        .control
        .as_ref()
        .unwrap()
        .send(
            current.revision - 1,
            Action::Finish {
                prefetch_keys: true,
            },
        )
        .unwrap();
    for (index, choice) in [(0, false), (1, true)] {
        pair.send(
            index,
            Action::Finish {
                prefetch_keys: choice,
            },
        );
        pair.wait(index, |s| s.prefetch_keys == Some(choice)).await;
        // A duplicate close or stale choice cannot reverse the decision.
        pair.send(
            index,
            Action::Finish {
                prefetch_keys: !choice,
            },
        );
        pair.send(index, Action::Refresh);
        let refreshed = pair
            .wait(index, |s| matches!(s.phase, Phase::Devices(_)))
            .await;
        assert_eq!(refreshed.prefetch_keys, Some(choice));
    }
    for (index, peer) in [(0, "SECOND"), (1, "FIRST")] {
        assert!(
            pair.clients[index]
                .encryption()
                .get_device(pair.clients[index].user_id().unwrap(), peer.into())
                .await
                .unwrap()
                .unwrap()
                .is_verified()
        );
        let requests = pair.servers[index].received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r
                    .url
                    .path()
                    .contains("sendToDevice/m.key.verification.mac/"))
                .count(),
            1
        );
    }
    pair.close().await;
}

#[tokio::test]
async fn mismatch_cancels_both_clients_without_verifying_devices() {
    let mut pair = Pair::new().await;
    pair.compare().await;
    pair.send(0, Action::Mismatch);
    pair.wait(0, |s| matches!(s.phase, Phase::Failed(_))).await;
    pair.wait(1, |s| matches!(s.phase, Phase::Failed(_))).await;
    assert!(
        !pair.clients[0]
            .encryption()
            .get_device(pair.clients[0].user_id().unwrap(), "SECOND".into())
            .await
            .unwrap()
            .unwrap()
            .is_verified()
    );
    let requests = pair.servers[0].received_requests().await.unwrap();
    assert!(
        !requests
            .iter()
            .any(|r| r.url.path().contains("verification.mac/"))
    );
    pair.close().await;
}

#[tokio::test]
async fn cancellation_rejects_late_confirmation_and_closes_the_peer_flow() {
    let mut pair = Pair::new().await;
    let (comparison, _) = pair.compare().await;
    let snapshot = pair.states[0].borrow().verification.clone();
    pair.send(0, Action::Cancel);
    pair.send(0, Action::Confirm(comparison.decimals));
    pair.wait(0, |s| s.phase == Phase::Cancelled).await;
    snapshot
        .control
        .as_ref()
        .unwrap()
        .send(snapshot.revision, Action::Confirm(comparison.decimals))
        .unwrap();
    pair.wait(1, |s| matches!(s.phase, Phase::Failed(_))).await;
    assert_eq!(pair.states[0].borrow().verification.phase, Phase::Cancelled);
    let requests = pair.servers[0].received_requests().await.unwrap();
    assert!(
        !requests
            .iter()
            .any(|r| r.url.path().contains("verification.mac/"))
    );
    pair.close().await;
}

#[tokio::test]
async fn immediate_close_cancels_a_start_from_the_previous_snapshot() {
    let mut pair = Pair::new().await;
    pair.send(0, Action::Refresh);
    let snapshot = pair.wait(0, |s| matches!(s.phase, Phase::Devices(_))).await;
    let control = snapshot.control.unwrap();
    control
        .send(snapshot.revision, Action::Start("SECOND".into()))
        .unwrap();
    control.send(snapshot.revision, Action::Cancel).unwrap();
    pair.wait(0, |s| s.phase == Phase::Cancelled).await;
    pair.close().await;
    assert!(
        control
            .send(snapshot.revision, Action::Confirm((1234, 5678, 9012)))
            .is_err()
    );
}

#[tokio::test]
async fn idle_service_and_shutdown_make_no_network_requests() {
    let pair = Pair::new().await;
    for server in &pair.servers {
        assert!(server.received_requests().await.unwrap().is_empty());
    }
    pair.close().await;
}
