use super::*;
use iced::futures::channel::{mpsc, oneshot};
use matrix_sdk::{
    authentication::matrix::MatrixSession,
    config::{RequestConfig, SyncSettings},
    ruma::{device_id, room_id, user_id},
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Notify;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path, path_regex, query_param},
};

async fn client(server: &MockServer, device: &str) -> Client {
    let client = Client::builder()
        .homeserver_url(server.uri())
        .request_config(RequestConfig::default().retry_limit(0))
        .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
        .build()
        .await
        .unwrap();
    client
        .matrix_auth()
        .restore_session(
            serde_json::from_value::<MatrixSession>(json!({
                "user_id":"@me:example.org", "device_id":device, "access_token":"test-only"
            }))
            .unwrap(),
            Default::default(),
        )
        .await
        .unwrap();
    client
}

async fn fixture() -> (MockServer, Client) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_matrix/client/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(".*/keys/(upload|query|claim)"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_keys":{}, "one_time_keys":{}, "one_time_key_counts":{"signed_curve25519":50}
        })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(".*/sendToDevice/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    let state = |kind: &str, content: Value| {
        json!({
            "type":kind,"state_key":"","sender":"@me:example.org",
            "event_id":format!("${kind}"),"origin_server_ts":1,"content":content
        })
    };
    let room = |encrypted: bool, room_type: Option<&str>| {
        let mut events = vec![
            state(
                "m.room.create",
                json!({"creator":"@me:example.org","room_version":"11","type":room_type}),
            ),
            json!({"type":"m.room.member","state_key":"@me:example.org","sender":"@me:example.org",
                "event_id":"$member","origin_server_ts":1,"content":{"membership":"join"}}),
        ];
        if encrypted {
            events.push(state(
                "m.room.encryption",
                json!({"algorithm":"m.megolm.v1.aes-sha2"}),
            ));
        }
        json!({"state":{"events":events},"timeline":{"events":[],"limited":false}})
    };
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/sync"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "next_batch":"fixture", "rooms":{
                "join":{
                    "!a:example.org":room(true, None), "!b:example.org":room(true, None),
                    "!public:example.org":room(false, None), "!unknown:example.org":room(false, None),
                    "!space:example.org":room(true, Some("m.space")),
                    "!saved:example.org":room(true, Some(crate::bookmarks::ROOM_TYPE))
                },
                "leave":{"!left:example.org":room(true, None)},
                "invite":{"!invite:example.org":{"invite_state":{"events":[
                    {"type":"m.room.member","state_key":"@me:example.org","sender":"@other:example.org","content":{"membership":"invite"}}
                ]}}}
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/state/m.room.encryption/?"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "errcode":"M_NOT_FOUND","error":"No encryption event"
        })))
        .mount(&server)
        .await;
    let client = client(&server, "TEST").await;
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    client
        .get_room(room_id!("!public:example.org"))
        .unwrap()
        .latest_encryption_state()
        .await
        .unwrap();
    for (id, stamp) in [("!a:example.org", 100), ("!b:example.org", 10)] {
        let room = client
            .get_room(&id.parse::<matrix_sdk::ruma::OwnedRoomId>().unwrap())
            .unwrap();
        room.update_and_save_room_info(|mut info| {
            info.update_recency_stamp(stamp.into());
            (info, Default::default())
        })
        .await
        .unwrap();
    }
    (server, client)
}

fn verified() -> impl Stream<Item = VerificationState> {
    stream::iter([VerificationState::Verified]).chain(stream::pending())
}

async fn history_requests(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path().ends_with("/messages"))
        .collect()
}

async fn history_mock(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path_regex(".*/messages"))
        .and(query_param("dir", "b"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "start":"latest","end":"older","chunk":[],"state":[]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn waits_for_verification_and_discovery_in_either_order() {
    for discovery_first in [false, true] {
        let (server, client) = fixture().await;
        history_mock(&server).await;
        let (trust, updates) = mpsc::unbounded();
        let (discovered, discovery) = oneshot::channel();
        let worker = tokio::spawn(async move {
            when_ready(
                &client,
                updates,
                async { discovery.await.unwrap() },
                std::future::ready(true),
            )
            .await;
        });
        trust.unbounded_send(VerificationState::Unverified).unwrap();
        if discovery_first {
            discovered.send(true).unwrap();
            tokio::time::sleep(Duration::from_millis(25)).await;
            assert!(history_requests(&server).await.is_empty());
            trust.unbounded_send(VerificationState::Verified).unwrap();
        } else {
            trust.unbounded_send(VerificationState::Verified).unwrap();
            tokio::time::sleep(Duration::from_millis(25)).await;
            assert!(history_requests(&server).await.is_empty());
            discovered.send(true).unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(history_requests(&server).await.len(), 2);
    }
}

#[tokio::test]
async fn verified_rooms_wait_for_the_final_dialog_and_unchecked_skips_the_pass() {
    for choice in [false, true] {
        let (server, client) = fixture().await;
        history_mock(&server).await;
        let (decision, permission) = oneshot::channel();
        let worker = tokio::spawn(async move {
            when_ready(&client, verified(), std::future::ready(true), async {
                permission.await.unwrap()
            })
            .await;
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            history_requests(&server).await.is_empty(),
            "trust alone must not start room reads"
        );
        decision.send(choice).unwrap();
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            history_requests(&server).await.len(),
            if choice { 2 } else { 0 }
        );
    }
}

#[tokio::test]
async fn verified_startup_reads_one_page_per_conversation_recent_first_and_skips_failures() {
    let (server, client) = fixture().await;
    history_mock(&server).await;
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/rooms/!a:example.org/messages"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"gone"})),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    when_ready(
        &client,
        verified(),
        std::future::ready(true),
        std::future::ready(true),
    )
    .await;
    let requests = history_requests(&server).await;
    assert_eq!(requests.len(), 2);
    assert!(requests[0].url.path().contains("/!a:example.org/"));
    assert!(requests[1].url.path().contains("/!b:example.org/"));
    for request in requests {
        let query: std::collections::HashMap<_, _> = request.url.query_pairs().collect();
        assert_eq!(query["limit"], "50");
        assert_eq!(query["dir"], "b");
        assert!(
            !query.contains_key("from"),
            "must not paginate through history"
        );
        let filter: Value = serde_json::from_str(&query["filter"]).unwrap();
        assert_eq!(filter["types"], json!(["m.room.encrypted"]));
    }
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| !r.url.path().contains("/receipt/") && !r.url.path().contains("/read_markers")));
    assert!(
        !client
            .get_room(room_id!("!unknown:example.org"))
            .unwrap()
            .encryption_state()
            .is_unknown()
    );
}

#[tokio::test]
async fn trust_revocation_and_dropping_the_worker_cancel_the_queue() {
    for revoke in [true, false] {
        let (server, client) = fixture().await;
        let started = Arc::new(Notify::new());
        let notify = started.clone();
        Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .respond_with(move |_: &Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(60))
                    .set_body_json(json!({"start":"latest","chunk":[],"state":[]}))
            })
            .mount(&server)
            .await;
        let (trust, updates) = mpsc::unbounded();
        trust.unbounded_send(VerificationState::Verified).unwrap();
        let worker = tokio::spawn(async move {
            when_ready(
                &client,
                updates,
                std::future::ready(true),
                std::future::ready(true),
            )
            .await;
        });
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        if revoke {
            trust.unbounded_send(VerificationState::Unverified).unwrap();
        } else {
            worker.abort();
        }
        let result = tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .unwrap();
        if revoke {
            result.unwrap();
        } else {
            assert!(result.unwrap_err().is_cancelled());
        }
        assert_eq!(
            history_requests(&server).await.len(),
            1,
            "remaining rooms must not start"
        );
    }
}

#[tokio::test]
async fn missing_session_keys_are_requested_from_own_devices_once_via_sdk_sync() {
    let (server, client) = fixture().await;
    let peer = self::client(&server, "OTHER").await;
    let mut devices = serde_json::Map::new();
    for device_client in [&client, &peer] {
        let device = device_client
            .encryption()
            .get_own_device()
            .await
            .unwrap()
            .unwrap();
        devices.insert(
            device.device_id().to_string(),
            serde_json::to_value(device.as_device_keys()).unwrap(),
        );
    }
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/keys/query"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"device_keys":{"@me:example.org":devices}})),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    client
        .encryption()
        .request_user_identity(user_id!("@me:example.org"))
        .await
        .unwrap();
    client
        .encryption()
        .get_device(user_id!("@me:example.org"), device_id!("OTHER"))
        .await
        .unwrap()
        .unwrap()
        .set_local_trust(matrix_sdk_crypto::LocalTrust::Verified)
        .await
        .unwrap();

    let mut session =
        matrix_sdk::encryption::vodozemac::megolm::GroupSession::new(Default::default());
    let ciphertext = session.encrypt("test-only plaintext").to_base64();
    let content = json!({
        "algorithm":"m.megolm.v1.aes-sha2", "session_id":session.session_id(),
        "sender_key":peer.encryption().get_own_device().await.unwrap().unwrap().curve25519_key().unwrap().to_base64(),
        "device_id":"OTHER", "ciphertext":ciphertext
    });
    let event = |id: &str| {
        json!({"type":"m.room.encrypted","event_id":id,"sender":"@me:example.org",
        "origin_server_ts":1,"room_id":"!a:example.org","content":content})
    };
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/rooms/!a:example.org/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "start":"latest","end":"older","chunk":[event("$one"),event("$two")],"state":[]
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    history_mock(&server).await;
    when_ready(
        &client,
        verified(),
        std::future::ready(true),
        std::future::ready(true),
    )
    .await;
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    let requests: Vec<_> = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path().contains("/sendToDevice/m.room_key_request/"))
        .collect();
    assert_eq!(
        requests.len(),
        1,
        "one request per missing session, not per message"
    );
    let body: Value = requests[0].body_json().unwrap();
    let request = &body["messages"]["@me:example.org"]["*"];
    assert_eq!(request["body"]["room_id"], "!a:example.org");
    assert_eq!(request["body"]["session_id"], session.session_id());
    assert_eq!(request["action"], "request");
    assert_eq!(history_requests(&server).await.len(), 2);
}

#[tokio::test]
async fn discovery_waits_for_later_batches_and_excludes_retained_left_rooms_from_count() {
    let (server, client) = fixture().await;
    let iteration = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = iteration.clone();
    Mock::given(method("POST"))
        .and(path("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"))
        .respond_with(move |_: &Request| {
            let batch = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let rooms = if batch < 2 {
                json!({})
            } else {
                json!({"!late:example.org":{
                    "initial":true,"name":"Late conversation","joined_count":1,"invited_count":0,
                    "required_state":[
                        {"type":"m.room.member","state_key":"@me:example.org","sender":"@me:example.org","event_id":"$late-member","origin_server_ts":1,"content":{"membership":"join"}},
                        {"type":"m.room.encryption","state_key":"","sender":"@me:example.org","event_id":"$late-encryption","origin_server_ts":1,"content":{"algorithm":"m.megolm.v1.aes-sha2"}}
                    ],"timeline":[]
                }})
            };
            ResponseTemplate::new(200).set_body_json(json!({
                "pos":batch.to_string(), "lists":{"all_rooms":{"count":8,"ops":[]}},"rooms":rooms
            }))
        })
        .mount(&server).await;
    let sync = super::super::build_sync(client.clone()).await.unwrap();
    let service = sync.room_list_service();
    let updates = service.sync();
    let discovery = discover_rooms(&sync);
    tokio::pin!(updates, discovery);
    assert!(iced::futures::poll!(&mut discovery).is_pending());
    updates.next().await.unwrap().unwrap();
    assert!(iced::futures::poll!(&mut discovery).is_pending());
    updates.next().await.unwrap().unwrap();
    assert_eq!(service.state().get(), State::Running);
    assert!(
        iced::futures::poll!(&mut discovery).is_pending(),
        "Running is not full discovery; retained left rooms must not satisfy the count"
    );
    updates.next().await.unwrap().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), &mut discovery)
            .await
            .unwrap()
    );
    assert!(client.get_room(room_id!("!late:example.org")).is_some());
    service.stop_sync().unwrap();
    assert!(updates.next().await.is_none());
}
