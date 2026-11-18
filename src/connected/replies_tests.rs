//! Offline Matrix protocol checks for ordinary and in-thread replies.
use super::*;
use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings, ruma::room_id};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

#[tokio::test]
async fn reply_payloads_preserve_format_mentions_and_thread_scope() {
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
    Mock::given(method("GET"))
        .and(path_regex(".*/relations/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[]})))
        .mount(&server)
        .await;
    let original = json!({"type":"m.room.message","event_id":"$original","room_id":"!room:example.org","sender":"@other:example.org","origin_server_ts":2,"content":{"msgtype":"m.text","body":"Original message"}});
    let mut threaded = original.clone();
    threaded["event_id"] = json!("$thread-target");
    threaded["content"]["m.relates_to"] = json!({"rel_type":"m.thread", "event_id":"$original", "is_falling_back":true, "m.in_reply_to":{"event_id":"$original"}});
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*thread-target"))
        .respond_with(ResponseTemplate::new(200).set_body_json(threaded.clone()))
        .with_priority(1)
        .mount(&server)
        .await;
    let received = json!({"type":"m.room.message","event_id":"$received","room_id":"!room:example.org","sender":"@test:example.org","origin_server_ts":3,"content":{"msgtype":"m.text","body":"> <@other:example.org> Original message\n\nAnswer","m.relates_to":{"m.in_reply_to":{"event_id":"$original"}}}});
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(original.clone()))
        .mount(&server)
        .await;
    let sync = Arc::new(std::sync::Mutex::new(
        json!({"next_batch":"one","rooms":{"join":{"!room:example.org":{
            "state":{"events":[
                {"type":"m.room.create","state_key":"","sender":"@test:example.org","event_id":"$create","origin_server_ts":0,"content":{"creator":"@test:example.org","room_version":"10"}},
                {"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":1,"content":{"membership":"join"}},
                {"type":"m.room.member","state_key":"@other:example.org","sender":"@other:example.org","event_id":"$other-member","origin_server_ts":1,"content":{"membership":"join","displayname":"Other"}}
            ]},"timeline":{"events":[],"limited":false}
        }}}}),
    ));
    let response = sync.clone();
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/sync"))
        .respond_with(move |_: &wiremock::Request| {
            ResponseTemplate::new(200).set_body_json(response.lock().unwrap().clone())
        })
        .mount(&server)
        .await;
    let sent = std::sync::atomic::AtomicUsize::new(0);
    Mock::given(method("PUT")).and(path_regex(".*/send/m.room.message/.*"))
        .respond_with(move |_: &wiremock::Request| ResponseTemplate::new(200).set_body_json(json!({"event_id":format!("$sent-{}",sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst))}))).mount(&server).await;
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
    let timeline = Arc::new(TimelineBuilder::new(&room).build().await.unwrap());
    let (_, updates) = timeline.subscribe().await;
    tokio::pin!(updates);
    *sync.lock().unwrap() = json!({"next_batch":"two","rooms":{"join":{"!room:example.org":{"timeline":{"events":[original, received, threaded],"limited":false}}}}});
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if timeline_rows(&timeline)
                .await
                .iter()
                .any(|row| row.key == "$received")
            {
                break;
            }
            updates.next().await.unwrap();
        }
    })
    .await
    .unwrap();
    let rows = timeline_rows(&timeline).await;
    let received = rows.iter().find(|row| row.key == "$received").unwrap();
    assert_eq!(received.body, "Answer");
    assert!(received.thread_root.is_none());
    let quote = received.reply.as_ref().unwrap();
    assert_eq!(quote.event, "$original");
    assert_eq!(quote.body, "Original message");
    assert_eq!(quote.sender.as_deref(), Some("Other"));

    let thread = Arc::new(
        TimelineBuilder::new(&room)
            .with_focus(TimelineFocus::Thread {
                root_event_id: "$original".parse().unwrap(),
            })
            .build()
            .await
            .unwrap(),
    );
    for (index, timeline) in [timeline.clone(), thread, timeline].into_iter().enumerate() {
        let target = if index == 2 {
            "$thread-target"
        } else {
            "$original"
        };
        Composer(RuntimeOwned::new(timeline))
            .send_reply_with_bindings(
                "**Answer** @third:example.org".into(),
                crate::composition::Format::Markdown,
                crate::composition::Bindings {
                    mentions: vec!["@third:example.org".parse().unwrap()],
                    ..Default::default()
                },
                Some(target.into()),
            )
            .await
            .unwrap();
        let content: Value = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let sends: Vec<_> = server
                    .received_requests()
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|request| request.url.path().contains("/send/m.room.message/"))
                    .collect();
                if let Some(request) = sends.get(index) {
                    break request.body_json().unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(content["m.relates_to"]["m.in_reply_to"]["event_id"], target);
        let users = content["m.mentions"]["user_ids"].as_array().unwrap();
        assert!(users.contains(&json!("@other:example.org")));
        assert!(users.contains(&json!("@third:example.org")));
        assert!(
            content["formatted_body"]
                .as_str()
                .unwrap()
                .contains("<strong>Answer</strong>")
        );
        if index != 1 {
            assert!(content["m.relates_to"].get("rel_type").is_none());
        } else {
            assert_eq!(content["m.relates_to"]["rel_type"], "m.thread");
            assert_eq!(content["m.relates_to"]["event_id"], "$original");
            assert!(
                !content["m.relates_to"]["is_falling_back"]
                    .as_bool()
                    .unwrap_or(false)
            );
        }
    }
}
