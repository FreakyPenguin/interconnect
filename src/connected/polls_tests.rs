//! Mock-homeserver poll workflows; no saved account or remote server is used.
use super::*;
use crate::polls::Creation;
use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings, ruma::room_id};
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
fn event(kind: &str, id: &str, sender: &str, timestamp: u64, content: Value) -> Value {
    json!({"type":kind,"event_id":id,"sender":sender,"origin_server_ts":timestamp,"content":content})
}
fn creation(hidden_results: bool, max_selections: u64) -> Creation {
    Creation {
        question: "Lunch?".into(),
        answers: vec!["Soup".into(), "Salad".into()],
        hidden_results,
        max_selections,
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
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.encryption/?"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"Not encrypted"})),
            )
            .mount(&server)
            .await;
        let response = Arc::new(std::sync::Mutex::new(json!({
            "next_batch":"initial", "rooms":{"join":{"!room:example.org":{
                "state":{"events":[
                    {"type":"m.room.create","state_key":"","sender":"@test:example.org","event_id":"$create","origin_server_ts":0,"content":{"creator":"@test:example.org","room_version":"10"}},
                    {"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":1,"content":{"membership":"join"}},
                    {"type":"m.room.power_levels","state_key":"","sender":"@test:example.org","event_id":"$levels","origin_server_ts":1,"content":{"users":{"@test:example.org":0}}}
                ]}, "timeline":{"events":[],"limited":false}
            }}}
        })));
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
            .request_config(matrix_sdk::config::RequestConfig::default().disable_retry())
            .with_threading_support(matrix_sdk::ThreadingSupport::Enabled {
                with_subscriptions: false,
            })
            .build()
            .await
            .unwrap();
        client.matrix_auth().restore_session(serde_json::from_value::<MatrixSession>(json!({"user_id":"@test:example.org","device_id":"TEST","access_token":"fixture"})).unwrap(), Default::default()).await.unwrap();
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
            batch: 0,
        };
        let content = serde_json::to_value(creation(true, 2).to_content().unwrap()).unwrap();
        fixture
            .sync(vec![
                event(
                    "org.matrix.msc3381.poll.start",
                    "$own",
                    "@test:example.org",
                    10,
                    content.clone(),
                ),
                event(
                    "org.matrix.msc3381.poll.start",
                    "$other",
                    "@other:example.org",
                    10,
                    content,
                ),
                event(
                    "m.room.message",
                    "$text",
                    "@test:example.org",
                    10,
                    json!({"msgtype":"m.text","body":"ordinary message"}),
                ),
            ])
            .await;
        fixture
            .wait(|rows| rows.iter().filter(|row| row.poll.is_some()).count() == 2)
            .await;
        fixture
    }
    fn composer(&self) -> Composer {
        Composer(RuntimeOwned::new(self.timeline.clone()))
    }
    async fn sync(&mut self, events: Vec<Value>) {
        self.batch += 1;
        *self.response.lock().unwrap() = json!({"next_batch":format!("batch-{}", self.batch),"rooms":{"join":{"!room:example.org":{"timeline":{"events":events,"limited":false}}}}});
        self.client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
    }
    async fn wait(&self, predicate: impl Fn(&[MessageRow]) -> bool) -> Vec<MessageRow> {
        let (_, updates) = self.timeline.subscribe().await;
        tokio::pin!(updates);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let rows = timeline_rows(&self.timeline).await;
                if predicate(&rows) {
                    return rows;
                }
                updates.next().await.unwrap();
            }
        })
        .await
        .unwrap()
    }
}
#[tokio::test]
async fn creation_sends_all_modes_and_rejects_invalid_content_before_network_writes() {
    let fixture = Fixture::new().await;
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/org.matrix.msc3381.poll.start/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$sent"})))
        .expect(4)
        .mount(&fixture.server)
        .await;
    for hidden in [false, true] {
        for limit in [1, 2] {
            fixture
                .composer()
                .create_poll(creation(hidden, limit))
                .await
                .unwrap();
        }
    }
    let mut invalid = creation(false, 1);
    invalid.answers[1] = "soup".into();
    assert!(fixture.composer().create_poll(invalid).await.is_err());
    wait_sends(&fixture.server, 4).await;
    let requests = fixture.server.received_requests().await.unwrap();
    let bodies: Vec<Value> = requests
        .iter()
        .filter(|request| request.method == "PUT")
        .map(|request| request.body_json().unwrap())
        .collect();
    assert_eq!(bodies.len(), 4);
    for (body, (hidden, limit)) in bodies
        .iter()
        .zip([(false, 1), (false, 2), (true, 1), (true, 2)])
    {
        let start = &body["org.matrix.msc3381.poll.start"];
        assert_eq!(start["max_selections"], limit);
        assert_eq!(
            start["kind"],
            if hidden {
                "org.matrix.msc3381.poll.undisclosed"
            } else {
                "org.matrix.msc3381.poll.disclosed"
            }
        );
        assert_eq!(start["answers"][0]["org.matrix.msc1767.text"], "Soup");
        assert!(body.get("m.relates_to").is_none());
    }
}
#[tokio::test]
async fn ending_requires_an_own_poll_retries_the_same_transaction_and_reveals_hidden_votes() {
    let mut fixture = Fixture::new().await;
    let transaction = matrix_sdk::ruma::TransactionId::new();
    for id in ["$other", "$text", "$missing"] {
        assert!(
            fixture
                .composer()
                .end_poll(id.into(), transaction.clone())
                .await
                .is_err()
        );
    }
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/org.matrix.msc3381.poll.end/.*"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Denied"})),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&fixture.server)
        .await;
    assert!(
        fixture
            .composer()
            .end_poll("$own".into(), transaction.clone())
            .await
            .is_err()
    );
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/org.matrix.msc3381.poll.end/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$end"})))
        .expect(1)
        .mount(&fixture.server)
        .await;
    fixture
        .composer()
        .end_poll("$own".into(), transaction)
        .await
        .unwrap();
    let requests = fixture.server.received_requests().await.unwrap();
    let sends: Vec<_> = requests
        .iter()
        .filter(|request| request.method == "PUT")
        .collect();
    assert_eq!(sends.len(), 2);
    assert_eq!(sends[0].url.path(), sends[1].url.path());
    let content: Value = sends[1].body_json().unwrap();
    assert_eq!(
        content["m.relates_to"],
        json!({"rel_type":"m.reference","event_id":"$own"})
    );
    fixture.sync(vec![event("org.matrix.msc3381.poll.response", "$vote", "@test:example.org", 20,
        json!({"org.matrix.msc3381.poll.response":{"answers":["0","1"]},"m.relates_to":{"rel_type":"m.reference","event_id":"$own"}}))]).await;
    let rows = fixture
        .wait(|rows| {
            rows.iter().any(|row| {
                row.event_id.as_deref() == Some("$own")
                    && row
                        .poll
                        .as_ref()
                        .is_some_and(|poll| poll.answers.iter().all(|answer| answer.selected))
            })
        })
        .await;
    let poll = rows
        .iter()
        .find(|row| row.event_id.as_deref() == Some("$own"))
        .unwrap()
        .poll
        .as_ref()
        .unwrap();
    assert!(!poll.disclosed);
    assert!(
        poll.answers
            .iter()
            .all(|answer| answer.votes == 0 && answer.voters.is_empty())
    );
    fixture
        .sync(vec![event(
            "org.matrix.msc3381.poll.end",
            "$end",
            "@test:example.org",
            30,
            content,
        )])
        .await;
    let rows = fixture
        .wait(|rows| {
            rows.iter().any(|row| {
                row.event_id.as_deref() == Some("$own")
                    && row.poll.as_ref().is_some_and(|poll| poll.ended)
            })
        })
        .await;
    let poll = rows
        .iter()
        .find(|row| row.event_id.as_deref() == Some("$own"))
        .unwrap()
        .poll
        .as_ref()
        .unwrap();
    assert!(poll.disclosed);
    assert!(
        poll.answers
            .iter()
            .all(|answer| answer.votes == 1 && answer.voters.len() == 1)
    );
    assert!(poll.validate_vote(&["0".into()]).is_err());
    // Already-ended polls are idempotent and never send another end event.
    fixture
        .composer()
        .end_poll("$own".into(), matrix_sdk::ruma::TransactionId::new())
        .await
        .unwrap();
}
#[tokio::test]
async fn ending_rechecks_permissions_and_redactions_without_sending() {
    let mut fixture = Fixture::new().await;
    fixture.sync(vec![json!({"type":"m.room.power_levels","state_key":"","event_id":"$denied","sender":"@test:example.org","origin_server_ts":15,"content":{"users":{"@test:example.org":0},"events":{"org.matrix.msc3381.poll.end":50}}})]).await;
    assert!(
        fixture
            .composer()
            .end_poll("$own".into(), matrix_sdk::ruma::TransactionId::new())
            .await
            .unwrap_err()
            .contains("permission")
    );
    fixture.sync(vec![json!({"type":"m.room.power_levels","state_key":"","event_id":"$allowed","sender":"@test:example.org","origin_server_ts":16,"content":{"users":{"@test:example.org":0}}}),
        json!({"type":"m.room.redaction","event_id":"$redaction","sender":"@test:example.org","origin_server_ts":20,"redacts":"$own","content":{"redacts":"$own"}})]).await;
    fixture
        .wait(|rows| {
            rows.iter()
                .any(|row| row.event_id.as_deref() == Some("$own") && row.poll.is_none())
        })
        .await;
    assert!(
        fixture
            .composer()
            .end_poll("$own".into(), matrix_sdk::ruma::TransactionId::new())
            .await
            .is_err()
    );
    assert!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method != "PUT")
    );
}

async fn wait_sends(server: &MockServer, count: usize) -> Vec<wiremock::Request> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let sends: Vec<_> = server
                .received_requests()
                .await
                .unwrap()
                .into_iter()
                .filter(|request| {
                    request
                        .url
                        .path()
                        .contains("/send/org.matrix.msc3381.poll.start/")
                })
                .collect();
            if sends.len() >= count {
                return sends;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn thread_creation_form_and_template_use_the_captured_sdk_thread_relation() {
    let fixture = Fixture::new().await;
    let mut root = event(
        "m.room.message",
        "$text",
        "@test:example.org",
        10,
        json!({"msgtype":"m.text","body":"ordinary message"}),
    );
    root["room_id"] = json!("!room:example.org");
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root))
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/relations/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[]})))
        .mount(&fixture.server)
        .await;
    let count = std::sync::atomic::AtomicUsize::new(0);
    Mock::given(method("PUT")).and(path_regex(".*/send/org.matrix.msc3381.poll.start/.*"))
        .respond_with(move |_: &wiremock::Request| ResponseTemplate::new(200).set_body_json(json!({"event_id":format!("$thread-poll-{}",count.fetch_add(1,std::sync::atomic::Ordering::SeqCst))})))
        .expect(5).mount(&fixture.server).await;
    let timeline = Arc::new(
        TimelineBuilder::new(fixture.timeline.room())
            .with_focus(TimelineFocus::Thread {
                root_event_id: "$text".parse().unwrap(),
            })
            .build()
            .await
            .unwrap(),
    );
    for hidden in [false, true] {
        for limit in [1, 2] {
            Composer(RuntimeOwned::new(timeline.clone()))
                .create_poll(creation(hidden, limit))
                .await
                .unwrap();
        }
    }
    Composer(RuntimeOwned::new(timeline.clone()))
        .send("/poll Dessert?\nCake\nFruit".into())
        .await
        .unwrap();
    let requests = wait_sends(&fixture.server, 5).await;
    for (request, (hidden, limit)) in
        requests
            .iter()
            .zip([(false, 1), (false, 2), (true, 1), (true, 2), (false, 1)])
    {
        let content: Value = request.body_json().unwrap();
        let relation = &content["m.relates_to"];
        assert_eq!(relation["rel_type"], "m.thread");
        assert_eq!(relation["event_id"], "$text");
        assert_eq!(relation["is_falling_back"], true);
        assert_eq!(
            content["org.matrix.msc3381.poll.start"]["max_selections"],
            limit
        );
        assert_eq!(
            content["org.matrix.msc3381.poll.start"]["kind"],
            if hidden {
                "org.matrix.msc3381.poll.undisclosed"
            } else {
                "org.matrix.msc3381.poll.disclosed"
            }
        );
    }
    let rows = timeline_rows(&timeline).await;
    assert_eq!(
        rows.iter()
            .filter(|row| row.poll.is_some() && row.thread_root.as_deref() == Some("$text"))
            .count(),
        5
    );
}

fn poll_start(id: &str, root: Option<&str>) -> Value {
    let mut content = serde_json::to_value(creation(true, 2).to_content().unwrap()).unwrap();
    if let Some(root) = root {
        content["m.relates_to"] = json!({"rel_type":"m.thread","event_id":root,"is_falling_back":true,"m.in_reply_to":{"event_id":root}});
    }
    let mut original = event(
        "org.matrix.msc3381.poll.start",
        id,
        "@test:example.org",
        10,
        content,
    );
    original["room_id"] = json!("!room:example.org");
    original
}

#[tokio::test]
async fn editing_an_unanswered_poll_replaces_all_fields_and_preserves_event_identity() {
    let mut fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(poll_start("$own", None)))
        .mount(&fixture.server)
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/org.matrix.msc3381.poll.start/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$edit"})))
        .expect(1)
        .mount(&fixture.server)
        .await;
    let replacement = Creation {
        question: "Updated lunch?".into(),
        answers: vec!["Bread".into(), "Salad".into(), "Soup".into()],
        max_selections: 3,
        hidden_results: false,
    };
    fixture
        .composer()
        .edit_poll("$own".into(), creation(true, 2), replacement.clone())
        .await
        .unwrap();
    let sends = wait_sends(&fixture.server, 1).await;
    let content: Value = sends[0].body_json().unwrap();
    assert_eq!(
        content["m.relates_to"],
        json!({"rel_type":"m.replace","event_id":"$own"})
    );
    let new = &content["m.new_content"]["org.matrix.msc3381.poll.start"];
    assert_eq!(new["question"]["org.matrix.msc1767.text"], "Updated lunch?");
    assert_eq!(new["max_selections"], 3);
    assert_eq!(new["kind"], "org.matrix.msc3381.poll.disclosed");
    assert_eq!(new["answers"][0]["org.matrix.msc1767.text"], "Bread");
    fixture
        .sync(vec![event(
            "org.matrix.msc3381.poll.start",
            "$edit",
            "@test:example.org",
            20,
            content,
        )])
        .await;
    let rows = fixture
        .wait(|rows| {
            rows.iter()
                .any(|row| row.event_id.as_deref() == Some("$own") && row.edited)
        })
        .await;
    let row = rows
        .iter()
        .find(|row| row.event_id.as_deref() == Some("$own"))
        .unwrap();
    assert_eq!(Creation::from(row.poll.as_ref().unwrap()), replacement);
    assert!(row.poll.as_ref().unwrap().editable);
    assert!(row.thread_root.is_none());
    assert_eq!(rows.iter().filter(|row| row.poll.is_some()).count(), 2);
}

#[tokio::test]
async fn editing_rejects_foreign_stale_ended_redacted_and_hidden_responded_polls() {
    let mut fixture = Fixture::new().await;
    let original = creation(true, 2);
    let replacement = creation(false, 1);
    for id in ["$other", "$text", "$missing"] {
        assert!(
            fixture
                .composer()
                .edit_poll(id.into(), original.clone(), replacement.clone())
                .await
                .is_err()
        );
    }
    let mut stale = original.clone();
    stale.question = "Old question".into();
    assert!(
        fixture
            .composer()
            .edit_poll("$own".into(), stale, replacement.clone())
            .await
            .unwrap_err()
            .contains("changed")
    );
    // Even responses with invalid/empty selections make the SDK poll non-editable.
    fixture.sync(vec![event("org.matrix.msc3381.poll.response", "$response", "@other:example.org", 20,
        json!({"org.matrix.msc3381.poll.response":{"answers":["invalid"]},"m.relates_to":{"rel_type":"m.reference","event_id":"$own"}}))]).await;
    let rows = fixture
        .wait(|rows| {
            rows.iter().any(|row| {
                row.event_id.as_deref() == Some("$own")
                    && row.poll.as_ref().is_some_and(|poll| !poll.editable)
            })
        })
        .await;
    let poll = rows
        .iter()
        .find(|row| row.event_id.as_deref() == Some("$own"))
        .unwrap()
        .poll
        .as_ref()
        .unwrap();
    assert!(!poll.disclosed);
    assert!(poll.answers.iter().all(|answer| answer.votes == 0));
    assert!(
        fixture
            .composer()
            .edit_poll("$own".into(), original.clone(), replacement.clone())
            .await
            .is_err()
    );
    fixture.sync(vec![event("org.matrix.msc3381.poll.end", "$ended", "@test:example.org", 30,
        json!({"org.matrix.msc3381.poll.end":{},"org.matrix.msc1767.text":"Ended","m.relates_to":{"rel_type":"m.reference","event_id":"$own"}}))]).await;
    fixture
        .wait(|rows| {
            rows.iter().any(|row| {
                row.event_id.as_deref() == Some("$own")
                    && row.poll.as_ref().is_some_and(|poll| poll.ended)
            })
        })
        .await;
    assert!(
        fixture
            .composer()
            .edit_poll("$own".into(), original.clone(), replacement.clone())
            .await
            .is_err()
    );
    fixture.sync(vec![json!({"type":"m.room.redaction","event_id":"$redacted","sender":"@test:example.org","origin_server_ts":40,"redacts":"$own","content":{"redacts":"$own"}})]).await;
    fixture
        .wait(|rows| {
            rows.iter()
                .any(|row| row.event_id.as_deref() == Some("$own") && row.poll.is_none())
        })
        .await;
    assert!(
        fixture
            .composer()
            .edit_poll("$own".into(), original, replacement)
            .await
            .is_err()
    );
    assert!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method != "PUT")
    );
}

#[tokio::test]
async fn editing_rechecks_permissions_and_validates_replacement_without_writes() {
    let mut fixture = Fixture::new().await;
    let mut invalid = creation(true, 2);
    invalid.answers[1] = "soup".into();
    assert!(
        fixture
            .composer()
            .edit_poll("$own".into(), creation(true, 2), invalid)
            .await
            .is_err()
    );
    fixture.sync(vec![json!({"type":"m.room.power_levels","state_key":"","event_id":"$denied-edit","sender":"@test:example.org","origin_server_ts":15,"content":{"users":{"@test:example.org":0},"events":{"org.matrix.msc3381.poll.start":50}}})]).await;
    assert!(
        fixture
            .composer()
            .edit_poll("$own".into(), creation(true, 2), creation(false, 1))
            .await
            .unwrap_err()
            .contains("permission")
    );
    assert!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method != "PUT")
    );
}

#[tokio::test]
async fn editing_thread_polls_keeps_the_original_thread_and_event() {
    let mut fixture = Fixture::new().await;
    let mut root = event(
        "m.room.message",
        "$text",
        "@test:example.org",
        10,
        json!({"msgtype":"m.text","body":"ordinary message"}),
    );
    root["room_id"] = json!("!room:example.org");
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root))
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/relations/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[]})))
        .mount(&fixture.server)
        .await;
    let timeline = Arc::new(
        TimelineBuilder::new(fixture.timeline.room())
            .with_focus(TimelineFocus::Thread {
                root_event_id: "$text".parse().unwrap(),
            })
            .build()
            .await
            .unwrap(),
    );
    let original = poll_start("$thread-poll", Some("$text"));
    fixture.sync(vec![original.clone()]).await;
    let (_, updates) = timeline.subscribe().await;
    tokio::pin!(updates);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if timeline_rows(&timeline)
                .await
                .iter()
                .any(|row| row.event_id.as_deref() == Some("$thread-poll"))
            {
                break;
            }
            updates.next().await.unwrap();
        }
    })
    .await
    .unwrap();
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*thread-poll"))
        .respond_with(ResponseTemplate::new(200).set_body_json(original))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/org.matrix.msc3381.poll.start/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$thread-edit"})))
        .expect(1)
        .mount(&fixture.server)
        .await;
    let mut replacement = creation(false, 1);
    replacement.question = "Updated thread question".into();
    Composer(RuntimeOwned::new(timeline.clone()))
        .edit_poll(
            "$thread-poll".into(),
            creation(true, 2),
            replacement.clone(),
        )
        .await
        .unwrap();
    let sends = wait_sends(&fixture.server, 1).await;
    let content: Value = sends[0].body_json().unwrap();
    assert_eq!(
        content["m.relates_to"],
        json!({"rel_type":"m.replace","event_id":"$thread-poll"})
    );
    fixture
        .sync(vec![event(
            "org.matrix.msc3381.poll.start",
            "$thread-edit",
            "@test:example.org",
            30,
            content,
        )])
        .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let rows = timeline_rows(&timeline).await;
            if let Some(row) = rows
                .iter()
                .find(|row| row.event_id.as_deref() == Some("$thread-poll") && row.edited)
            {
                assert_eq!(row.thread_root.as_deref(), Some("$text"));
                assert_eq!(Creation::from(row.poll.as_ref().unwrap()), replacement);
                break;
            }
            updates.next().await.unwrap();
        }
    })
    .await
    .unwrap();
    let rows = fixture
        .wait(|rows| {
            rows.iter()
                .any(|row| row.event_id.as_deref() == Some("$thread-poll") && row.edited)
        })
        .await;
    assert_eq!(
        rows.iter()
            .find(|row| row.event_id.as_deref() == Some("$thread-poll"))
            .unwrap()
            .thread_root
            .as_deref(),
        Some("$text")
    );
}

#[tokio::test]
async fn rejected_poll_edits_report_errors_and_explicit_retries_reuse_the_transaction() {
    let fixture = Fixture::new().await;
    let transaction = matrix_sdk::ruma::TransactionId::new();
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/org.matrix.msc3381.poll.start/.*"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Denied"})),
        )
        .expect(1)
        .up_to_n_times(1)
        .mount(&fixture.server)
        .await;
    assert!(
        fixture
            .composer()
            .edit_poll_with_transaction(
                "$own".into(),
                creation(true, 2),
                creation(false, 1),
                transaction.clone()
            )
            .await
            .is_err()
    );
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/org.matrix.msc3381.poll.start/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$retried-edit"})))
        .expect(1)
        .mount(&fixture.server)
        .await;
    fixture
        .composer()
        .edit_poll_with_transaction(
            "$own".into(),
            creation(true, 2),
            creation(false, 1),
            transaction,
        )
        .await
        .unwrap();
    let sends = wait_sends(&fixture.server, 2).await;
    assert_eq!(sends[0].url.path(), sends[1].url.path());
}
