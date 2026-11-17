use super::*;
use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

const STORAGE: &str = "!saved:example.org";
const SOURCE: &str = "!source:example.org";
fn pointer(event: &str) -> Pointer {
    Pointer {
        room_id: SOURCE.into(),
        event_id: event.into(),
        via: Vec::new(),
    }
}
fn bookmark(id: &str, event: &str, timestamp: u64) -> Value {
    json!({"type":EVENT_TYPE,"event_id":id,"sender":"@test:example.org","origin_server_ts":timestamp,"content":{POINTER:pointer(event),"m.text":[{"body":"Saved link"}]}})
}
fn edit(id: &str, target: &str, event: &str, timestamp: u64) -> Value {
    json!({"type":EVENT_TYPE,"event_id":id,"sender":"@test:example.org","origin_server_ts":timestamp,"content":{"m.relates_to":{"rel_type":"m.replace","event_id":target},"m.new_content":{POINTER:pointer(event)}}})
}
fn state(kind: &str, key: &str, content: Value) -> Value {
    json!({"type":kind,"state_key":key,"sender":"@test:example.org","event_id":format!("${kind}-{key}"),"origin_server_ts":1,"content":content})
}
fn room(kind: Option<&str>, events: Vec<Value>) -> Value {
    let mut create = json!({"creator":"@test:example.org","room_version":"11"});
    if let Some(kind) = kind {
        create["type"] = json!(kind);
    }
    json!({"state":{"events":[state("m.room.create","",create),state("m.room.join_rules","",json!({"join_rule":"invite"})),state("m.room.member","@test:example.org",json!({"membership":"join"})),state("m.room.encryption","",json!({"algorithm":"m.megolm.v1.aes-sha2"}))]},"timeline":{"events":events,"prev_batch":"older","limited":false}})
}
struct Fixture {
    server: MockServer,
    client: Client,
    response: Arc<std::sync::Mutex<Value>>,
}
impl Fixture {
    async fn new(events: Vec<Value>) -> Self {
        Self::create(events, true).await
    }
    async fn create(events: Vec<Value>, include_storage: bool) -> Self {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"],"unstable_features":{"org.matrix.simplified_msc3575":true}}))).mount(&server).await;
        Mock::given(method("GET")).and(path_regex(".*/members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[state("m.room.member","@test:example.org",json!({"membership":"join"}))]}))).mount(&server).await;
        for suffix in ["keys/query", "keys/claim", "keys/upload"] {
            Mock::given(method("POST")).and(path(format!("/_matrix/client/v3/{suffix}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"device_keys":{},"one_time_keys":{},"one_time_key_counts":{"signed_curve25519":50}}))).mount(&server).await;
        }
        Mock::given(method("PUT"))
            .and(path_regex(".*/sendToDevice/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let mut rooms = json!({SOURCE:room(None, Vec::new())});
        if include_storage {
            rooms[STORAGE] = room(Some(ROOM_TYPE), events);
        }
        let response = Arc::new(std::sync::Mutex::new(
            json!({"next_batch":"seed","rooms":{"join":rooms}}),
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
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
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
        client.event_cache().subscribe().unwrap();
        let fixture = Self {
            server,
            client,
            response,
        };
        fixture.sync().await;
        fixture
    }
    async fn sync(&self) {
        self.client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
    }
    async fn no_account_data(&self) {
        Mock::given(method("GET"))
            .and(path_regex(".*/account_data/.*bookmarks_room"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"Not set"})),
            )
            .mount(&self.server)
            .await;
    }
    async fn account_data(&self) {
        Mock::given(method("GET"))
            .and(path_regex(".*/account_data/.*bookmarks_room"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"active_bookmarks_room_id":STORAGE})),
            )
            .mount(&self.server)
            .await;
    }
}

#[test]
fn replacement_validation_duplicate_grouping_and_redaction() {
    let root = bookmark("$one", "$original", 1);
    let latest = edit("$edit", "$one", "$latest", 3);
    let duplicate = bookmark("$two", "$latest", 2);
    let mut foreign = edit("$foreign", "$one", "$wrong", 10);
    foreign["sender"] = json!("@other:example.org");
    let mut redacted = edit("$removed-edit", "$one", "$wrong", 20);
    redacted["unsigned"] = json!({"redacted_because":{}});
    let events = vec![root.clone(), latest, duplicate, foreign, redacted];
    let entries = project(&events, &HashSet::new());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].pointer.event_id, "$latest");
    assert_eq!(entries[0].bookmark_ids, ["$one", "$two"]);
    assert!(project(&events, &HashSet::from(["$one".into(), "$two".into()])).is_empty());
    let mut root = root;
    root["unsigned"] = json!({"redacted_because":{}});
    assert!(project(&[root], &HashSet::new()).is_empty());
}

#[tokio::test]
async fn first_use_is_read_only_and_settles_without_a_room_or_idle_requests() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.no_account_data().await;
    let service = Service::new(fixture.client.clone());
    let sync = matrix_sdk_ui::sync_service::SyncService::builder(fixture.client.clone())
        .build()
        .await
        .unwrap();
    let (state, mut snapshots) = watch::channel(Model::default());
    let worker = tokio::spawn(service.run_owned(sync, state));
    tokio::time::timeout(
        Duration::from_secs(5),
        snapshots.wait_for(|model| model.bookmarks.complete),
    )
    .await
    .unwrap()
    .unwrap();
    let before = fixture.server.received_requests().await.unwrap().len();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        fixture.server.received_requests().await.unwrap().len(),
        before
    );
    assert!(
        !fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.method == "POST" && request.url.path().ends_with("/createRoom"))
    );
    worker.abort();
    let _ = worker.await;
}

impl Service {
    async fn run_owned(
        self,
        sync: matrix_sdk_ui::sync_service::SyncService,
        state: watch::Sender<Model>,
    ) {
        self.run(&sync, state).await;
    }
}

#[tokio::test]
async fn paginated_index_follows_replacements_redactions_and_cancellation() {
    let fixture = Fixture::new(vec![
        bookmark("$new", "$second", 5),
        edit("$edit", "$old", "$edited", 4),
    ])
    .await;
    Mock::given(method("GET")).and(path_regex(".*/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"start":"older","chunk":[bookmark("$old","$first",1),bookmark("$duplicate","$edited",2)],"state":[]}))).mount(&fixture.server).await;
    let service = Service::new(fixture.client.clone());
    let (state, mut snapshots) = watch::channel(Model::default());
    let room = fixture
        .client
        .get_room(&STORAGE.parse::<OwnedRoomId>().unwrap())
        .unwrap();
    let worker = tokio::spawn(async move {
        service.watch_room(room, &state).await.unwrap();
    });
    let loaded = tokio::time::timeout(
        Duration::from_secs(5),
        snapshots.wait_for(|model| model.bookmarks.complete && model.bookmarks.entries.len() == 2),
    )
    .await
    .unwrap()
    .unwrap()
    .clone();
    assert!(loaded.bookmarks.contains(SOURCE, "$edited"));
    assert_eq!(
        loaded
            .bookmarks
            .entries
            .iter()
            .find(|entry| entry.pointer.event_id == "$edited")
            .unwrap()
            .bookmark_ids
            .len(),
        2
    );
    let rooms = crate::navigation::snapshot(&fixture.client)
        .await
        .unwrap()
        .rooms;
    assert_eq!(
        rooms.len(),
        1,
        "the storage room is hidden from regular navigation"
    );
    *fixture.response.lock().unwrap() = json!({"next_batch":"edit","rooms":{"join":{STORAGE:{"timeline":{"events":[edit("$edit-new","$new","$changed",8)],"limited":false}}}}});
    fixture.sync().await;
    tokio::time::timeout(
        Duration::from_secs(5),
        snapshots.wait_for(|model| model.bookmarks.contains(SOURCE, "$changed")),
    )
    .await
    .unwrap()
    .unwrap();
    *fixture.response.lock().unwrap() = json!({"next_batch":"redact","rooms":{"join":{STORAGE:{"timeline":{"events":[{"type":"m.room.redaction","event_id":"$redact","sender":"@test:example.org","origin_server_ts":10,"redacts":"$new","content":{"redacts":"$new"}}],"limited":false}}}}});
    fixture.sync().await;
    tokio::time::timeout(
        Duration::from_secs(5),
        snapshots.wait_for(|model| model.bookmarks.entries.len() == 1),
    )
    .await
    .unwrap()
    .unwrap();
    worker.abort();
    let _ = worker.await;
    snapshots.borrow_and_update();
    *fixture.response.lock().unwrap() = json!({"next_batch":"after-close","rooms":{"join":{STORAGE:{"timeline":{"events":[bookmark("$late","$late-target",20)],"limited":false}}}}});
    fixture.sync().await;
    assert!(!matches!(snapshots.has_changed(), Ok(true)));
}

#[tokio::test]
async fn encrypted_save_reuses_transaction_on_retry_and_removes_all_duplicates() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.account_data().await;
    Mock::given(method("GET")).and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"type":"m.room.message","room_id":SOURCE,"event_id":"$original","sender":"@test:example.org","origin_server_ts":1,"content":{"msgtype":"m.text","body":"Private source text"}}))).mount(&fixture.server).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/m.room.encrypted/.*"))
        .respond_with(move |_: &wiremock::Request| {
            if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"fixture failure"}))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({"event_id":"$saved"}))
            }
        })
        .mount(&fixture.server)
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(".*/redact/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$redaction"})))
        .mount(&fixture.server)
        .await;
    let service = Service::new(fixture.client.clone());
    service.0.writes.lock().await.indexed_room = Some(STORAGE.parse().unwrap());
    service.0.snapshots.send_modify(|snapshot| {
        snapshot.complete = true;
        snapshot.loading = false;
    });
    assert!(
        service
            .clone()
            .set(pointer("$original"), true)
            .await
            .is_err()
    );
    service
        .clone()
        .set(pointer("$original"), true)
        .await
        .unwrap();
    let requests = fixture.server.received_requests().await.unwrap();
    let sends: Vec<_> = requests
        .iter()
        .filter(|request| request.url.path().contains("/send/m.room.encrypted/"))
        .collect();
    assert_eq!(sends.len(), 2);
    assert_eq!(
        sends[0].url.path(),
        sends[1].url.path(),
        "retry must preserve transaction identity"
    );
    let content: Value = serde_json::from_slice(&sends[1].body).unwrap();
    assert_eq!(content["algorithm"], "m.megolm.v1.aes-sha2");
    assert!(content.get(POINTER).is_none());
    let storage = fixture
        .client
        .get_room(&STORAGE.parse::<OwnedRoomId>().unwrap())
        .unwrap();
    let encrypted = Raw::from_json_string(json!({"type":"m.room.encrypted","sender":"@test:example.org","event_id":"$saved","origin_server_ts":5,"content":content}).to_string()).unwrap();
    let decrypted = storage.decrypt_event(&encrypted, None).await.unwrap();
    let event: Value = serde_json::from_str(decrypted.raw().json().get()).unwrap();
    assert_eq!(event["type"], EVENT_TYPE);
    assert_eq!(event["content"][POINTER]["event_id"], "$original");
    let fallback = event["content"]["m.text"][0]["body"].as_str().unwrap();
    assert!(fallback.contains("matrix:roomid/") && !fallback.contains("Private source text"));
    // Binary bookmark state is maintained even before the server sync echo.
    service
        .clone()
        .set(pointer("$original"), true)
        .await
        .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    service
        .0
        .snapshots
        .send_modify(|snapshot| snapshot.entries[0].bookmark_ids.push("$duplicate".into()));
    service
        .clone()
        .set(pointer("$original"), false)
        .await
        .unwrap();
    assert!(service.0.snapshots.borrow().entries.is_empty());
    assert_eq!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path().contains("/redact/"))
            .count(),
        2
    );
}

#[tokio::test]
async fn displayed_preview_tracks_source_edits_and_redactions() {
    let fixture = Fixture::new(Vec::new()).await;
    let original = json!({"type":"m.room.message","event_id":"$original","room_id":SOURCE,"sender":"@test:example.org","origin_server_ts":1,"content":{"msgtype":"m.text","body":"Original text"}});
    Mock::given(method("GET")).and(path_regex(".*/context/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event":original.clone(),"events_before":[],"events_after":[],"state":[],"start":"start","end":"end"}))).mount(&fixture.server).await;
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(original))
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/relations/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[]})))
        .mount(&fixture.server)
        .await;
    let service = Service::new(fixture.client.clone());
    let mut previews = service.previews(vec![pointer("$original")]);
    let first = tokio::time::timeout(Duration::from_secs(5), previews.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.body, "Original text");
    *fixture.response.lock().unwrap() = json!({"next_batch":"source-edit","rooms":{"join":{SOURCE:{"timeline":{"events":[{"type":"m.room.message","event_id":"$source-edit","sender":"@test:example.org","origin_server_ts":3,"content":{"msgtype":"m.text","body":"* Updated text","m.relates_to":{"rel_type":"m.replace","event_id":"$original"},"m.new_content":{"msgtype":"m.text","body":"Updated text"}}}],"limited":false}}}}});
    fixture.sync().await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if previews.next().await.unwrap().body == "Updated text" {
                break;
            }
        }
    })
    .await
    .unwrap();
    *fixture.response.lock().unwrap() = json!({"next_batch":"source-redact","rooms":{"join":{SOURCE:{"timeline":{"events":[{"type":"m.room.redaction","event_id":"$source-redaction","sender":"@test:example.org","origin_server_ts":4,"redacts":"$original","content":{"redacts":"$original"}}],"limited":false}}}}});
    fixture.sync().await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if previews.next().await.unwrap().body == "Message removed" {
                break;
            }
        }
    })
    .await
    .unwrap();
    drop(previews);
    let before = fixture.server.received_requests().await.unwrap().len();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        fixture.server.received_requests().await.unwrap().len(),
        before
    );
}

#[tokio::test]
async fn first_save_creates_encrypted_private_room_waits_for_sync_and_publishes_pointer() {
    let fixture = Fixture::create(Vec::new(), false).await;
    let published = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let get = published.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/account_data/.*bookmarks_room"))
        .respond_with(move |_: &wiremock::Request| {
            if get.load(std::sync::atomic::Ordering::SeqCst) {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"active_bookmarks_room_id":STORAGE}))
            } else {
                ResponseTemplate::new(404)
                    .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"Not set"}))
            }
        })
        .mount(&fixture.server)
        .await;
    let put = published.clone();
    Mock::given(method("PUT"))
        .and(path_regex(".*/account_data/.*bookmarks_room"))
        .respond_with(move |_: &wiremock::Request| {
            put.store(true, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({}))
        })
        .mount(&fixture.server)
        .await;
    let created = Arc::new(Notify::new());
    let signal = created.clone();
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/createRoom"))
        .respond_with(move |_: &wiremock::Request| {
            signal.notify_one();
            ResponseTemplate::new(200).set_body_json(json!({"room_id":STORAGE}))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET")).and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"type":"m.room.message","event_id":"$original","room_id":SOURCE,"sender":"@test:example.org","origin_server_ts":1,"content":{"msgtype":"m.text","body":"Private text"}}))).mount(&fixture.server).await;
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/m.room.encrypted/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$saved"})))
        .mount(&fixture.server)
        .await;
    let service = Service::new(fixture.client.clone());
    service.0.snapshots.send_modify(|snapshot| {
        snapshot.complete = true;
        snapshot.loading = false;
    });
    let saving = service.clone().set(pointer("$original"), true);
    let sync_room = async {
        created.notified().await;
        *fixture.response.lock().unwrap() = json!({"next_batch":"created","rooms":{"join":{STORAGE:room(Some(ROOM_TYPE),Vec::new())}}});
        fixture.sync().await;
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(saving, sync_room)
    })
    .await
    .unwrap();
    result.unwrap();
    assert!(published.load(std::sync::atomic::Ordering::SeqCst));
    let requests = fixture.server.received_requests().await.unwrap();
    let create = requests
        .iter()
        .find(|request| request.url.path().ends_with("/createRoom"))
        .unwrap();
    let body: Value = serde_json::from_slice(&create.body).unwrap();
    assert_eq!(body["creation_content"]["type"], ROOM_TYPE);
    assert_eq!(body["preset"], "private_chat");
    assert!(
        body.get("invite")
            .is_none_or(|invite| invite.as_array().unwrap().is_empty())
    );
    assert!(
        body["initial_state"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["type"] == "m.room.encryption"
                && event["content"]["algorithm"] == "m.megolm.v1.aes-sha2")
    );
    let pointer_write = requests
        .iter()
        .find(|request| request.method == "PUT" && request.url.path().contains("/account_data/"))
        .unwrap();
    let body: Value = serde_json::from_slice(&pointer_write.body).unwrap();
    assert_eq!(body["active_bookmarks_room_id"], STORAGE);
    let reopened = Service::new(fixture.client.clone());
    let storage = reopened
        .storage_room(&mut Writes::default(), false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(storage.room_id().as_str(), STORAGE);
}

#[tokio::test]
async fn inaccessible_active_room_is_never_replaced_and_incomplete_history_cannot_be_written() {
    let fixture = Fixture::new(Vec::new()).await;
    Mock::given(method("GET"))
        .and(path_regex(".*/account_data/.*bookmarks_room"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"active_bookmarks_room_id":"!unavailable:example.org"})),
        )
        .mount(&fixture.server)
        .await;
    let service = Service::new(fixture.client.clone());
    assert!(
        service
            .clone()
            .set(pointer("$original"), true)
            .await
            .unwrap_err()
            .contains("still loading")
    );
    service
        .0
        .snapshots
        .send_modify(|snapshot| snapshot.complete = true);
    assert!(
        service
            .clone()
            .set(pointer("$original"), true)
            .await
            .unwrap_err()
            .contains("unavailable")
    );
    assert!(
        !fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.url.path().ends_with("/createRoom")
                || request.url.path().contains("/send/"))
    );
}

#[tokio::test]
async fn active_room_outside_the_initial_list_waits_for_subscription_metadata() {
    let fixture = Fixture::create(Vec::new(), false).await;
    let requested = Arc::new(Notify::new());
    let signal = requested.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/account_data/.*bookmarks_room"))
        .respond_with(move |_: &wiremock::Request| {
            signal.notify_one();
            ResponseTemplate::new(200).set_body_json(json!({"active_bookmarks_room_id":STORAGE}))
        })
        .mount(&fixture.server)
        .await;
    let service = Service::new(fixture.client.clone());
    let sync = matrix_sdk_ui::sync_service::SyncService::builder(fixture.client.clone())
        .build()
        .await
        .unwrap();
    service
        .0
        .room_list
        .set(sync.room_list_service())
        .ok()
        .unwrap();
    let lookup = async {
        service
            .storage_room(&mut Writes::default(), false)
            .await
            .unwrap()
            .unwrap()
    };
    let deliver = async {
        requested.notified().await;
        *fixture.response.lock().unwrap() = json!({"next_batch":"outside-range","rooms":{"join":{STORAGE:room(Some(ROOM_TYPE),Vec::new())}}});
        fixture.sync().await;
    };
    let (room, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(lookup, deliver)
    })
    .await
    .unwrap();
    assert_eq!(room.room_id().as_str(), STORAGE);
    service.unsubscribe_storage();
}

#[tokio::test]
async fn an_account_data_room_switch_cannot_apply_old_bookmark_ids_to_the_new_room() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.account_data().await;
    let service = Service::new(fixture.client.clone());
    service.0.writes.lock().await.indexed_room = Some("!previous:example.org".parse().unwrap());
    service.0.snapshots.send_modify(|snapshot| {
        snapshot.complete = true;
        snapshot.entries = vec![Entry {
            pointer: pointer("$original"),
            bookmark_ids: vec!["$old-bookmark".into()],
            saved_at: 1,
            fallback: "Saved".into(),
        }];
    });
    let error = service.set(pointer("$original"), false).await.unwrap_err();
    assert!(error.contains("active bookmarks room changed"), "{error}");
    assert!(
        !fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.url.path().contains("/redact/")
                || request.url.path().contains("/send/"))
    );
}

#[tokio::test]
async fn source_preview_waits_for_a_room_outside_the_initial_metadata_range() {
    let fixture = Fixture::new(Vec::new()).await;
    let outside = "!outside:example.org";
    let original = json!({"type":"m.room.message","event_id":"$outside","room_id":outside,"sender":"@test:example.org","origin_server_ts":1,"content":{"msgtype":"m.text","body":"Saved outside the initial range"}});
    Mock::given(method("GET")).and(path_regex(".*/context/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event":original.clone(),"events_before":[],"events_after":[],"state":[],"start":"start","end":"end"}))).mount(&fixture.server).await;
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(original))
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/relations/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[]})))
        .mount(&fixture.server)
        .await;
    let service = Service::new(fixture.client.clone());
    let mut previews = service.previews(vec![Pointer {
        room_id: outside.into(),
        event_id: "$outside".into(),
        via: Vec::new(),
    }]);
    assert!(
        iced::futures::poll!(previews.next()).is_pending(),
        "metadata absence must wait instead of reporting a failed preview"
    );
    *fixture.response.lock().unwrap() =
        json!({"next_batch":"outside-source","rooms":{"join":{outside:room(None,Vec::new())}}});
    fixture.sync().await;
    let preview = tokio::time::timeout(Duration::from_secs(5), previews.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preview.body, "Saved outside the initial range");
}
