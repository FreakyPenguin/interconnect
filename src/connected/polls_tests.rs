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
            .build()
            .await
            .unwrap();
        client.matrix_auth().restore_session(serde_json::from_value::<MatrixSession>(json!({"user_id":"@test:example.org","device_id":"TEST","access_token":"fixture"})).unwrap(), Default::default()).await.unwrap();
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let room = client.get_room(room_id!("!room:example.org")).unwrap();
        let timeline = Arc::new(TimelineBuilder::new(&room).build().await.unwrap());
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
