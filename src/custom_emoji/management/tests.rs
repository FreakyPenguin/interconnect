use super::*;
use matrix_sdk::{
    authentication::matrix::MatrixSession,
    config::{RequestConfig, SyncSettings},
};
use std::{collections::HashMap, sync::Mutex};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

fn content() -> Value {
    json!({"extra":{"keep":true},"pack":{"display_name":"Cats","attribution":"Artist","future":1},"images":{"cat":{"url":"mxc://example.org/cat","body":"Cat","info":{"w":64,"h":64},"usage":["emoticon"],"future":{"keep":true}},"sticker":{"url":"mxc://example.org/sticker","usage":["sticker"]}}})
}
#[test]
fn edits_preserve_metadata_and_validate_collisions_limits_and_deletions() {
    let baseline = content();
    let renamed = edited(Some(&baseline), &Edit::Rename("New cats".into())).unwrap();
    assert_eq!(renamed["extra"], baseline["extra"]);
    assert_eq!(renamed["pack"]["attribution"], "Artist");
    let edit = Edit::Image {
        previous: Some("cat".into()),
        shortcode: "wave".into(),
        description: "Waving".into(),
        uri: "mxc://example.org/cat".into(),
        upload: None,
    };
    let next = edited(Some(&renamed), &edit).unwrap();
    assert!(next["images"].get("cat").is_none());
    assert_eq!(
        next["images"]["wave"]["future"],
        baseline["images"]["cat"]["future"]
    );
    assert_eq!(
        next["images"]["wave"]["info"],
        baseline["images"]["cat"]["info"]
    );
    assert_eq!(next["images"]["sticker"], baseline["images"]["sticker"]);
    let removed = edited(Some(&next), &Edit::RemoveImage("wave".into())).unwrap();
    assert_eq!(removed["images"].as_object().unwrap().len(), 1);
    assert_eq!(edited(Some(&baseline), &Edit::Delete).unwrap(), json!({}));
    for (shortcode, uri) in [
        ("bad:name", "mxc://example.org/x"),
        ("cat", "mxc://example.org/x"),
        ("new", "https://example.org/x"),
    ] {
        assert!(
            edited(
                Some(&baseline),
                &Edit::Image {
                    previous: None,
                    shortcode: shortcode.into(),
                    description: String::new(),
                    uri: uri.into(),
                    upload: None
                }
            )
            .is_err()
        );
    }
    assert!(edited(Some(&baseline), &Edit::Rename(String::new())).is_err());
    let oversized = json!({"extra":"x".repeat(LIMIT),"images":{}});
    assert!(edited(Some(&oversized), &Edit::Rename("Cats".into())).is_err());
}

struct Fixture {
    server: MockServer,
    client: Client,
    service: Service,
    data: Arc<Mutex<HashMap<String, Value>>>,
}
fn key(request: &wiremock::Request) -> String {
    let path = request.url.path();
    if let Some((_, state)) = path.split_once("/state/") {
        state.into()
    } else {
        path.rsplit('/').next().unwrap().into()
    }
}
async fn fixture() -> Fixture {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/_matrix/client/versions")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"],"unstable_features":{"org.matrix.simplified_msc3575":true}}))).mount(&server).await;
    let event = |kind: &str, state_key: &str, content: Value| json!({"type":kind,"state_key":state_key,"content":content,"event_id":format!("${kind}-{state_key}"),"sender":"@test:example.org","origin_server_ts":1});
    Mock::given(method("GET")).and(path("/_matrix/client/v3/sync")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!room:example.org":{"state":{"events":[event("m.room.create","",json!({"creator":"@test:example.org","room_version":"11"})),event("m.room.member","@test:example.org",json!({"membership":"join"})),event("m.room.power_levels","",json!({"users":{"@test:example.org":100}})),event("m.room.image_pack","cats",content())]},"timeline":{"events":[],"limited":false}}}}}))).mount(&server).await;
    let data = Arc::new(Mutex::new(HashMap::from([
        ("m.room.image_pack/cats".into(), content()),
        (PERSONAL.into(), content()),
        (
            GLOBAL_TYPES[0].into(),
            json!({"extra":1,"rooms":{"!room:example.org":{"cats":{"future":7},"other":{}},"!other:example.org":{"else":{}}}}),
        ),
    ])));
    let get = data.clone();
    Mock::given(method("GET"))
        .and(path_regex(".*/(state|account_data)/.*"))
        .respond_with(move |request: &wiremock::Request| {
            match get.lock().unwrap().get(&key(request)) {
                Some(content) => ResponseTemplate::new(200).set_body_json(content),
                None => ResponseTemplate::new(404)
                    .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"Missing fixture"})),
            }
        })
        .mount(&server)
        .await;
    let put = data.clone();
    Mock::given(method("PUT"))
        .and(path_regex(".*/(state|account_data)/.*"))
        .respond_with(move |request: &wiremock::Request| {
            put.lock()
                .unwrap()
                .insert(key(request), request.body_json().unwrap());
            ResponseTemplate::new(200).set_body_json(if request.url.path().contains("/state/") {
                json!({"event_id":"$saved"})
            } else {
                json!({})
            })
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/media/.*config"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"m.upload.size":2097152})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(".*/media/.*upload"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"content_uri":"mxc://example.org/uploaded"})),
        )
        .mount(&server)
        .await;
    let client = Client::builder()
        .homeserver_url(server.uri())
        .request_config(RequestConfig::default().retry_limit(0))
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
    let service = Service::new(client.clone());
    Fixture {
        server,
        client,
        service,
        data,
    }
}
#[tokio::test]
async fn personal_room_create_edit_delete_and_global_membership_preserve_other_fields() {
    let fixture = fixture().await;
    let listing = fixture
        .service
        .clone()
        .load("!room:example.org".into())
        .await
        .unwrap();
    assert!(listing.can_create);
    assert_eq!(listing.packs.len(), 2);
    assert!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() != "PUT")
    );
    let personal = fixture
        .service
        .clone()
        .save(
            listing.packs[0].clone(),
            Edit::Rename("Personal cats".into()),
        )
        .await
        .unwrap();
    assert_eq!(personal.name, "Personal cats");
    let room = listing.packs[1].clone();
    let enabled = fixture
        .service
        .clone()
        .enable(room.clone(), true)
        .await
        .unwrap();
    assert!(enabled.global);
    assert_eq!(
        fixture.data.lock().unwrap()[GLOBAL_TYPES[0]]["rooms"]["!room:example.org"]["cats"]["future"],
        7
    );
    fixture
        .service
        .clone()
        .enable(room.clone(), false)
        .await
        .unwrap();
    let data = fixture.data.lock().unwrap()[GLOBAL_TYPES[0]].clone();
    assert!(data["rooms"]["!room:example.org"].get("cats").is_none());
    assert!(data["rooms"]["!room:example.org"].get("other").is_some());
    assert!(data["rooms"].get("!other:example.org").is_some());
    assert_eq!(data["extra"], 1);
    let renamed = fixture
        .service
        .clone()
        .save(room, Edit::Rename("Room cats".into()))
        .await
        .unwrap();
    assert_eq!(
        renamed.content.as_ref().unwrap()["pack"]["attribution"],
        "Artist"
    );
    fixture
        .service
        .clone()
        .save(renamed, Edit::Delete)
        .await
        .unwrap();
    assert_eq!(
        fixture.data.lock().unwrap()["m.room.image_pack/cats"],
        json!({})
    );
    let new = pack(
        Source::Room {
            room: "!room:example.org".parse().unwrap(),
            key: "new".into(),
            kind: ROOM_TYPES[0].into(),
        },
        None,
        "New",
        true,
        false,
    );
    let saved = fixture
        .service
        .clone()
        .save(new, Edit::Rename("Created".into()))
        .await
        .unwrap();
    assert_eq!(saved.content.as_ref().unwrap()["images"], json!({}));
}
#[tokio::test]
async fn fresh_state_conflicts_and_permission_loss_prevent_writes_and_uploads() {
    let fixture = fixture().await;
    let baseline = fixture
        .service
        .clone()
        .load("!room:example.org".into())
        .await
        .unwrap()
        .packs[1]
        .clone();
    fixture
        .data
        .lock()
        .unwrap()
        .get_mut("m.room.image_pack/cats")
        .unwrap()["extra"] = json!("changed");
    assert_eq!(
        fixture
            .service
            .clone()
            .save(baseline.clone(), Edit::Rename("Lost edit".into()))
            .await
            .unwrap_err(),
        CONFLICT
    );
    assert!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() != "PUT")
    );
    fixture
        .data
        .lock()
        .unwrap()
        .insert("m.room.image_pack/cats".into(), content());
    fixture
        .data
        .lock()
        .unwrap()
        .insert(format!("{}/cats", ROOM_TYPES[1]), content());
    let legacy = Pack {
        source: Source::Room {
            room: "!room:example.org".parse().unwrap(),
            key: "cats".into(),
            kind: ROOM_TYPES[1].into(),
        },
        ..baseline.clone()
    };
    assert_eq!(
        fixture
            .service
            .clone()
            .save(legacy, Edit::Delete)
            .await
            .unwrap_err(),
        CONFLICT
    );
    Mock::given(method("GET")).and(path("/_matrix/client/v3/sync")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"two","rooms":{"join":{"!room:example.org":{"timeline":{"events":[{"type":"m.room.power_levels","state_key":"","event_id":"$demoted","sender":"@test:example.org","origin_server_ts":2,"content":{"users":{"@test:example.org":0},"state_default":50}}],"limited":false}}}}}))).with_priority(1).mount(&fixture.server).await;
    fixture
        .client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    assert!(
        fixture
            .service
            .clone()
            .save(baseline, Edit::Delete)
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
            .all(|r| r.method.as_str() != "PUT")
    );
}
#[tokio::test]
async fn image_preparation_is_bounded_and_upload_is_only_performed_by_save() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cat.png");
    image::RgbaImage::new(16, 16).save(&path).unwrap();
    let upload = read_image(path).await.unwrap();
    assert!(matches!(
        upload.preview(),
        iced::widget::image::Handle::Rgba {
            width: 16,
            height: 16,
            ..
        }
    ));
    let fixture = fixture().await;
    let baseline = fixture
        .service
        .clone()
        .load("!room:example.org".into())
        .await
        .unwrap()
        .packs[1]
        .clone();
    assert!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| !r.url.path().contains("/media/"))
    );
    let saved = fixture
        .service
        .clone()
        .save(
            baseline,
            Edit::Image {
                previous: None,
                shortcode: "new_cat".into(),
                description: "New cat".into(),
                uri: String::new(),
                upload: Some(upload),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let content = saved.content.unwrap();
    assert_eq!(
        content["images"]["new_cat"]["url"],
        "mxc://example.org/uploaded"
    );
    assert_eq!(content["images"]["new_cat"]["info"]["w"], 16);
    assert_eq!(
        content["images"]["cat"],
        super::tests::content()["images"]["cat"]
    );
    let oversized = directory.path().join("large.png");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(2 * 1024 * 1024 + 1)
        .unwrap();
    assert!(read_image(oversized).await.unwrap_err().contains("2 MiB"));
    assert!(read_image(directory.path().to_owned()).await.is_err());
    let invalid = directory.path().join("not-an-image.png");
    std::fs::write(&invalid, b"This is not an image").unwrap();
    assert!(read_image(invalid).await.is_err());
    let wide = directory.path().join("wide.png");
    image::RgbaImage::new(4097, 1).save(&wide).unwrap();
    assert!(read_image(wide).await.is_err());
}

#[tokio::test]
async fn malformed_global_data_is_rejected_without_overwriting_it() {
    let fixture = fixture().await;
    let pack = fixture
        .service
        .clone()
        .load("!room:example.org".into())
        .await
        .unwrap()
        .packs[1]
        .clone();
    fixture
        .data
        .lock()
        .unwrap()
        .insert(GLOBAL_TYPES[0].into(), json!("malformed"));
    assert!(
        fixture
            .service
            .clone()
            .enable(pack, true)
            .await
            .unwrap_err()
            .contains("Invalid")
    );
    assert_eq!(
        fixture.data.lock().unwrap()[GLOBAL_TYPES[0]],
        json!("malformed")
    );
    assert!(
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() != "PUT")
    );
}
