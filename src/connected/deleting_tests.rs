//! Offline server-response and permission tests; no real account is opened.
use super::*;
use matrix_sdk::{
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    ruma::{TransactionId, event_id, room_id},
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

struct Fixture {
    server: MockServer,
    client: Client,
    timeline: Arc<Timeline>,
    response: Arc<std::sync::Mutex<Value>>,
    batch: u64,
}
fn event(id: &str, sender: &str, content: Value) -> Value {
    json!({"type":"m.room.message","event_id":id,"room_id":"!room:example.org","sender":sender,"origin_server_ts":2,"content":content})
}
fn levels(id: &str, content: Value) -> Value {
    json!({"type":"m.room.power_levels","state_key":"","event_id":id,"sender":"@test:example.org","origin_server_ts":1,"content":content})
}
fn redaction(id: &str, target: &str) -> Value {
    json!({"type":"m.room.redaction","event_id":id,"sender":"@test:example.org","origin_server_ts":3,"redacts":target,"content":{"redacts":target}})
}
async fn wait_rows(
    timeline: &Timeline,
    predicate: impl Fn(&[MessageRow]) -> bool,
) -> Vec<MessageRow> {
    let (_, updates) = timeline.subscribe().await;
    tokio::pin!(updates);
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let rows = timeline_rows(timeline).await;
            if predicate(&rows) {
                break rows;
            }
            updates.next().await.unwrap();
        }
    })
    .await;
    match result {
        Ok(rows) => rows,
        Err(error) => panic!(
            "{error}: {:?}",
            timeline_rows(timeline)
                .await
                .iter()
                .map(|row| (&row.key, &row.body, row.redactable))
                .collect::<Vec<_>>()
        ),
    }
}
impl Fixture {
    async fn new() -> Self {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let root = event(
            "$root",
            "@test:example.org",
            json!({"msgtype":"m.text","body":"Original message"}),
        );
        let reply = event(
            "$reply",
            "@test:example.org",
            json!({"msgtype":"m.text","body":"Thread reply","m.relates_to":{"rel_type":"m.thread","event_id":"$root","is_falling_back":false}}),
        );
        Mock::given(method("GET"))
            .and(path_regex(".*/event/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(root.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(".*/relations/.*"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"chunk":[reply.clone()]})),
            )
            .mount(&server)
            .await;
        let response = Arc::new(std::sync::Mutex::new(
            json!({"next_batch":"one","rooms":{"join":{"!room:example.org":{
                "state":{"events":[
                    {"type":"m.room.create","state_key":"","sender":"@creator:example.org","event_id":"$create","origin_server_ts":0,"content":{"creator":"@creator:example.org","room_version":"10"}},
                    {"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":1,"content":{"membership":"join"}},
                    {"type":"m.room.encryption","state_key":"","sender":"@test:example.org","event_id":"$encryption","origin_server_ts":1,"content":{"algorithm":"m.megolm.v1.aes-sha2"}},
                    levels("$levels", json!({"users":{"@test:example.org":0},"redact":50}))
                ]},"timeline":{"events":[],"limited":false}
            }}}}),
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
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
            .with_threading_support(matrix_sdk::ThreadingSupport::Enabled {
                with_subscriptions: false,
            })
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(
                    json!({"user_id":"@test:example.org","device_id":"TEST","access_token":"fake"}),
                )
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let room = client.get_room(room_id!("!room:example.org")).unwrap();
        let timeline = Arc::new(
            TimelineBuilder::new(&room)
                .with_focus(TimelineFocus::Live {
                    hide_threaded_events: false,
                })
                .build()
                .await
                .unwrap(),
        );
        let mut fixture = Self {
            server,
            client,
            timeline,
            response,
            batch: 1,
        };
        fixture
            .sync(vec![
                root,
                reply,
                event(
                    "$media",
                    "@test:example.org",
                    json!({"msgtype":"m.image","body":"photo","url":"mxc://example.org/image"}),
                ),
                event(
                    "$other",
                    "@other:example.org",
                    json!({"msgtype":"m.text","body":"Another person's message"}),
                ),
            ])
            .await;
        wait_rows(&fixture.timeline, |rows| rows.len() == 4).await;
        fixture
    }
    fn composer(&self) -> Composer {
        Composer(RuntimeOwned::new(self.timeline.clone()))
    }
    async fn sync(&mut self, events: Vec<Value>) {
        self.batch += 1;
        *self.response.lock().unwrap() = json!({"next_batch":format!("batch-{}",self.batch),"rooms":{"join":{"!room:example.org":{"timeline":{"events":events,"limited":false}}}}});
        self.client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn deleting_awaits_server_errors_retries_the_same_transaction_and_updates_room_and_thread() {
    let mut fixture = Fixture::new().await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(method("PUT"))
        .and(path_regex(".*/redact/.*"))
        .respond_with(move |_: &wiremock::Request| {
            if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"fixture refusal"}))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({"event_id":"$deleted"}))
            }
        })
        .mount(&fixture.server)
        .await;
    let composer = fixture.composer();
    let error = composer
        .clone()
        .delete_message("$other".into(), TransactionId::new())
        .await
        .unwrap_err();
    assert!(error.contains("permission"), "{error}");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let transaction = TransactionId::new();
    assert!(
        composer
            .clone()
            .delete_message("$media".into(), transaction.clone())
            .await
            .unwrap_err()
            .contains("fixture refusal")
    );
    composer
        .clone()
        .delete_message("$media".into(), transaction)
        .await
        .unwrap();
    let room = fixture
        .client
        .get_room(room_id!("!room:example.org"))
        .unwrap();
    let thread = Arc::new(
        TimelineBuilder::new(&room)
            .with_focus(TimelineFocus::Thread {
                root_event_id: event_id!("$root").to_owned(),
            })
            .build()
            .await
            .unwrap(),
    );
    thread.paginate_backwards(20).await.unwrap();
    wait_rows(&thread, |rows| rows.iter().any(|row| row.key == "$reply")).await;
    Composer(RuntimeOwned::new(thread.clone()))
        .delete_message("$reply".into(), TransactionId::new())
        .await
        .unwrap();
    composer
        .clone()
        .delete_message("$root".into(), TransactionId::new())
        .await
        .unwrap();
    let requests: Vec<_> = fixture
        .server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path().contains("/redact/"))
        .collect();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[0].url, requests[1].url,
        "explicit retry retains the transaction"
    );
    for request in &requests {
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).unwrap(),
            json!({})
        );
    }
    let before = timeline_rows(&fixture.timeline).await;
    fixture
        .sync(vec![
            redaction("$remove-media", "$media"),
            redaction("$remove-reply", "$reply"),
            redaction("$remove-root", "$root"),
        ])
        .await;
    let rows = wait_rows(&fixture.timeline, |rows| {
        rows.iter().filter(|row| !row.redactable).count() == 3
    })
    .await;
    assert_eq!(rows.len(), 4, "redactions preserve timeline positions");
    for id in ["$media", "$reply", "$root"] {
        let row = rows.iter().find(|row| row.key == id).unwrap();
        let original = before.iter().find(|row| row.key == id).unwrap();
        assert_eq!(row.timestamp_ms, original.timestamp_ms);
        assert_eq!(row.body, "Message removed");
        assert!(row.image.is_none() && row.download.is_none() && row.formatted.is_none());
        assert!(!row.can_react && row.edit_source.is_none());
    }
    let replies = wait_rows(&thread, |rows| {
        rows.iter().any(|row| row.key == "$root" && !row.redactable)
            && rows
                .iter()
                .all(|row| row.key != "$reply" || !row.redactable)
    })
    .await;
    // SDK 0.19.1 may remove a redacted reply from its thread projection because
    // the redaction strips the thread relation. Its room placeholder remains.
    assert!(replies.iter().all(|row| row.body != "Thread reply"));
    assert!(
        composer
            .clone()
            .delete_message("$root".into(), TransactionId::new())
            .await
            .is_err()
    );
    assert!(
        composer
            .delete_message("$missing".into(), TransactionId::new())
            .await
            .is_err()
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 4);
}

#[tokio::test]
async fn permission_changes_and_departure_are_rechecked_before_a_redaction() {
    let mut fixture = Fixture::new().await;
    Mock::given(method("PUT"))
        .and(path_regex(".*/redact/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$deleted"})))
        .mount(&fixture.server)
        .await;
    let composer = fixture.composer();
    fixture
        .sync(vec![levels(
            "$no-redact",
            json!({"events":{"m.room.redaction":50},"users":{"@test:example.org":0},"redact":50}),
        )])
        .await;
    let error = composer
        .clone()
        .delete_message("$root".into(), TransactionId::new())
        .await
        .unwrap_err();
    assert!(error.contains("permission"), "{error}");
    assert!(
        !fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.url.path().contains("/redact/"))
    );
    fixture
        .sync(vec![levels(
            "$moderator",
            json!({"users":{"@test:example.org":100},"redact":50}),
        )])
        .await;
    composer
        .clone()
        .delete_message("$other".into(), TransactionId::new())
        .await
        .unwrap();
    fixture.batch += 1;
    *fixture.response.lock().unwrap() = json!({"next_batch":format!("batch-{}",fixture.batch),"rooms":{"leave":{"!room:example.org":{"timeline":{"events":[{"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$leave","origin_server_ts":4,"content":{"membership":"leave"}}],"limited":false}}}}});
    fixture
        .client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    assert!(
        composer
            .delete_message("$root".into(), TransactionId::new())
            .await
            .unwrap_err()
            .contains("member")
    );
    assert_eq!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path().contains("/redact/"))
            .count(),
        1
    );
}

#[tokio::test]
async fn deletion_reasons_are_trimmed_bounded_and_attached_to_redactions() {
    let fixture = Fixture::new().await;
    Mock::given(method("PUT"))
        .and(path_regex(".*/redact/.*"))
        .and(wiremock::matchers::body_json(json!({"reason":"Spam"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$deleted"})))
        .expect(1)
        .mount(&fixture.server)
        .await;
    let composer = fixture.composer();
    assert!(
        composer
            .clone()
            .delete_message_with_reason(
                "$media".into(),
                TransactionId::new(),
                "x".repeat(crate::moderation::MAX_REASON_BYTES + 1)
            )
            .await
            .is_err()
    );
    composer
        .delete_message_with_reason("$media".into(), TransactionId::new(), "  Spam  ".into())
        .await
        .unwrap();
}
