use super::*;
use crate::media::ImageRequest as _;
use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

async fn sends_attachments(encrypted: bool) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_matrix/client/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/state/m.room.encryption/.*"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"errcode":"M_NOT_FOUND","error":"Not encrypted"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"start":"one", "chunk":[], "state":[]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/relations/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[]})))
        .mount(&server)
        .await;
    let root = json!({"type":"m.room.message","event_id":"$root","room_id":"!room:example.org","sender":"@test:example.org","origin_server_ts":1,"content":{"msgtype":"m.text","body":"attachment fixture root"}});
    let member = json!({"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":0,"content":{"membership":"join"}});
    let mut state = vec![member.clone()];
    if encrypted {
        state.push(json!({"type":"m.room.encryption","state_key":"","sender":"@test:example.org","event_id":"$encryption","origin_server_ts":0,"content":{"algorithm":"m.megolm.v1.aes-sha2"}}));
    }
    Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!room:example.org":{"state":{"events":state},"timeline":{"events":[root.clone()],"limited":false}}}}})))
        .mount(&server).await;
    Mock::given(method("GET"))
        .and(path_regex(".*/event/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[member]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/keys/upload"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"one_time_key_counts":{"signed_curve25519":50}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/keys/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"device_keys":{}})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/config"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"m.upload.size":52428800})))
        .mount(&server)
        .await;
    let uploads = std::sync::atomic::AtomicUsize::new(0);
    Mock::given(method("POST"))
        .and(path_regex(".*/upload"))
        .respond_with(move |_: &wiremock::Request| {
            let index = uploads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(
                json!({"content_uri":format!("mxc://example.org/attachment-{index}")}),
            )
        })
        .mount(&server)
        .await;
    let events = std::sync::atomic::AtomicUsize::new(0);
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/.*"))
        .respond_with(move |_: &wiremock::Request| {
            let index = events.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"event_id":format!("$sent-{index}")}))
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
    let (state, mut snapshots) = watch::channel(Model::default());
    let (_selection_control, selection) = watch::channel(Selection::default());
    let room_worker = tokio::spawn(view(
        client.clone(),
        "!room:example.org".into(),
        None,
        1,
        state.clone(),
        selection.clone(),
    ));
    let thread_worker = tokio::spawn(view(
        client.clone(),
        "!room:example.org".into(),
        Some("$root".into()),
        2,
        state.clone(),
        selection.clone(),
    ));
    let opened = tokio::time::timeout(
        Duration::from_secs(5),
        snapshots.wait_for(|model| {
            model.room.as_ref().is_some_and(|pane| {
                pane.composer.is_some() && !matches!(pane.history, History::Loading)
            }) && model.thread.as_ref().is_some_and(|pane| {
                pane.composer.is_some() && !matches!(pane.history, History::Loading)
            })
        }),
    )
    .await
    .unwrap()
    .unwrap()
    .clone();
    let room_timeline = (**opened.room.unwrap().composer.unwrap().0).clone();
    let thread_timeline = (**opened.thread.unwrap().composer.unwrap().0).clone();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("fixture.png");
    image::RgbaImage::new(800, 400).save(&file).unwrap();
    let attachment = crate::attachment::Attachment::from_path(file)
        .await
        .unwrap();
    for (index, timeline) in [room_timeline, thread_timeline].into_iter().enumerate() {
        let (_, updates) = timeline.subscribe().await;
        tokio::pin!(updates);
        let caption = if index == 0 { "" } else { "fixture caption" };
        let expected_body = if index == 0 { "fixture.png" } else { caption };
        Composer(RuntimeOwned::new(timeline.clone()))
            .send_attachment(attachment.clone(), caption.into())
            .await
            .unwrap();
        // Watch the actual queued request, rather than treating admission as delivery.
        let sent = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let requests = server.received_requests().await.unwrap();
                let sends: Vec<_> = requests
                    .iter()
                    .filter(|request| request.url.path().contains("/send/"))
                    .collect();
                if sends.len() > index {
                    let send = sends[index];
                    let content: serde_json::Value = send.body_json().unwrap();
                    if encrypted {
                        assert!(send.url.path().contains("/send/m.room.encrypted/"));
                        assert!(content.get("ciphertext").is_some());
                        assert!(content.get("body").is_none());
                    } else {
                        assert!(send.url.path().contains("/send/m.room.message/"));
                        assert_eq!(content["msgtype"], "m.image");
                        assert_eq!(content["body"], expected_body);
                        if index == 0 {
                            assert!(content.get("filename").is_none());
                        } else {
                            assert_eq!(content["filename"], "fixture.png");
                        }
                        assert!(content["info"]["size"].as_u64().unwrap() > 0);
                        assert_eq!(content["info"]["w"], 800);
                        assert_eq!(content["info"]["h"], 400);
                        assert!(
                            content["url"]
                                .as_str()
                                .unwrap()
                                .starts_with("mxc://example.org/attachment-")
                        );
                        assert!(
                            content["info"]["thumbnail_url"]
                                .as_str()
                                .unwrap()
                                .starts_with("mxc://example.org/attachment-")
                        );
                        assert_ne!(content["url"], content["info"]["thumbnail_url"]);
                        assert_eq!(content["info"]["thumbnail_info"]["w"], 512);
                        assert_eq!(content["info"]["thumbnail_info"]["h"], 256);
                        if index == 0 {
                            assert!(content.get("m.relates_to").is_none());
                        } else {
                            assert_eq!(content["m.relates_to"]["rel_type"], "m.thread");
                            assert_eq!(content["m.relates_to"]["event_id"], "$root");
                        }
                    }
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(
            sent.is_ok(),
            "queued attachment did not reach fixture server; requests: {:?}; items: {:?}",
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .map(|r| r.url.path())
                .collect::<Vec<_>>(),
            timeline.items().await
        );
        let visible = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let rows = timeline_rows(&timeline).await;
                if let Some(row) = rows.iter().find(|row| {
                    row.body == expected_body && row.image.is_some() && row.send_state == "Sent"
                }) {
                    assert_eq!(row.thread_root.as_deref(), (index == 1).then_some("$root"));
                    let preview = row
                        .image
                        .as_ref()
                        .unwrap()
                        .preview
                        .as_ref()
                        .expect("sent image must have an inline preview");
                    assert!(matches!(
                        preview.parameters().format,
                        matrix_sdk::media::MediaFormat::File
                    ));
                    if encrypted {
                        assert!(matches!(
                            preview.parameters().source,
                            matrix_sdk::ruma::events::room::MediaSource::Encrypted(_)
                        ));
                    }
                    let bytes = client
                        .media()
                        .get_media_content(&preview.parameters(), true)
                        .await
                        .unwrap();
                    let image = image::load_from_memory(&bytes).unwrap();
                    assert_eq!((image.width(), image.height()), (512, 256));
                    break;
                }
                updates.next().await.expect("timeline updates ended");
            }
        })
        .await;
        assert!(
            visible.is_ok(),
            "sent image missing from local timeline: {:?}",
            timeline_rows(&timeline).await
        );
    }
    let published = tokio::time::timeout(
        Duration::from_secs(5),
        snapshots.wait_for(|model| {
            [&model.room, &model.thread]
                .into_iter()
                .enumerate()
                .all(|(index, pane)| {
                    pane.as_ref().is_some_and(|pane| {
                        pane.rows.iter().any(|row| {
                            row.body
                                == if index == 0 {
                                    "fixture.png"
                                } else {
                                    "fixture caption"
                                }
                                && row.image.is_some()
                                && row.send_state == "Sent"
                        })
                    })
                })
        }),
    )
    .await
    .is_ok_and(|result| result.is_ok());
    assert!(
        published,
        "local image was not published to the UI: {:?}",
        snapshots.borrow()
    );
    room_worker.abort();
    thread_worker.abort();
    let _ = room_worker.await;
    let _ = thread_worker.await;
    let requests = server.received_requests().await.unwrap();
    let uploads: Vec<_> = requests
        .iter()
        .filter(|request| {
            request.url.path().contains("/media/") && request.url.path().ends_with("/upload")
        })
        .collect();
    assert_eq!(uploads.len(), 4); // Each original has its own thumbnail.
    assert!(
        !requests.iter().any(|request| request.method == "GET"
            && (request.url.path().contains("/download/")
                || request.url.path().contains("/thumbnail/"))),
        "own sent previews must load from the SDK cache"
    );
    let mut originals = 0;
    let mut thumbnails = 0;
    let original = std::fs::read(dir.path().join("fixture.png")).unwrap();
    for upload in uploads {
        if encrypted {
            assert!(
                image::guess_format(&upload.body).is_err(),
                "encrypted originals and thumbnails must not be uploaded as plaintext images"
            );
            assert_ne!(
                upload.body, original,
                "encrypted rooms must not upload plaintext media"
            );
        } else if upload.body == original {
            originals += 1;
        } else {
            let image = image::load_from_memory(&upload.body).unwrap();
            assert_eq!((image.width(), image.height()), (512, 256));
            thumbnails += 1;
        }
    }
    if !encrypted {
        assert_eq!((originals, thumbnails), (2, 2));
    }
}

#[tokio::test]
async fn room_and_thread_attachment_payloads_have_captions_metadata_and_correct_relations() {
    sends_attachments(false).await;
}

#[tokio::test]
async fn encrypted_room_and_thread_attachments_encrypt_media_and_events() {
    sends_attachments(true).await;
}
