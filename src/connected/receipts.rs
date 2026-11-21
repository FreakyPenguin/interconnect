//! View-owned receipts: bounded, serialized requests, with no idle timers.
use super::{Model, Selection};
use matrix_sdk::ruma::api::client::receipt::create_receipt::v3::ReceiptType;
use matrix_sdk_ui::timeline::Timeline;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadTarget {
    pub generation: u64,
    /// Last visible server event, or None for an empty room.
    pub event: Option<String>,
}

fn desired(selection: &Selection, generation: u64, thread: bool) -> Option<ReadTarget> {
    let selected = if thread {
        &selection.thread
    } else {
        &selection.room
    };
    if selection.shutdown
        || !selection.conversation_visible
        || selected
            .as_ref()
            .is_none_or(|(current, _)| *current != generation)
    {
        return None;
    }
    selection.read[usize::from(thread)]
        .clone()
        .filter(|target| target.generation == generation && (!thread || target.event.is_some()))
}

pub(super) async fn run(
    timeline: Arc<Timeline>,
    mut selection: watch::Receiver<Selection>,
    generation: u64,
    thread: bool,
    state: watch::Sender<Model>,
) {
    let mut attempted = None;
    loop {
        let next = selection.borrow_and_update().clone();
        if next.shutdown {
            return;
        }
        let target = desired(&next, generation, thread)
            .map(|target| (target, next.privacy.public_read_receipts));
        if target != attempted {
            attempted = target.clone();
            if let Some((target, public)) = target {
                let request = tokio::time::timeout(Duration::from_secs(15), async {
                    let Some(event) = &target.event else {
                        // An arrival may have superseded the empty viewport before this task ran.
                        if timeline
                            .items()
                            .await
                            .iter()
                            .any(|item| item.as_event().is_some())
                        {
                            return Ok(());
                        }
                        return timeline
                            .room()
                            .set_unread_flag(false)
                            .await
                            .map_err(|e| e.to_string());
                    };
                    let event: matrix_sdk::ruma::OwnedEventId = event
                        .parse()
                        .map_err(|_| "Invalid read receipt event".to_owned())?;
                    timeline
                        .send_single_receipt(
                            if public {
                                ReceiptType::Read
                            } else {
                                ReceiptType::ReadPrivate
                            },
                            event.clone(),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    // A thread never moves the room's fully-read marker.
                    if !thread {
                        timeline
                            .send_single_receipt(ReceiptType::FullyRead, event)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    Ok::<_, String>(())
                });
                tokio::pin!(request);
                let result = loop {
                    tokio::select! {
                        biased;
                        changed = selection.changed() => {
                            if changed.is_err() { return; }
                            let current = selection.borrow_and_update();
                            if desired(&current, generation, thread).as_ref() != Some(&target)
                                || current.privacy.public_read_receipts != public {
                                // Policy, navigation, focus, or viewport changes cancel this request.
                                attempted = None;
                                break None;
                            }
                        }
                        result = &mut request => break Some(result.unwrap_or_else(|_| Err("Read receipt timed out".into()))),
                    }
                };
                if let Some(result) = result {
                    let error = result.err().map(|error| (generation, error));
                    state.send_if_modified(|model| {
                        let slot = &mut model.receipt_errors[usize::from(thread)];
                        if *slot == error {
                            false
                        } else {
                            *slot = error;
                            true
                        }
                    });
                }
                continue;
            }
        }
        if selection.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{Client, authentication::matrix::MatrixSession, config::SyncSettings};
    use matrix_sdk_ui::timeline::{TimelineBuilder, TimelineFocus, TimelineReadReceiptTracking};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    async fn fixture(thread: bool) -> (MockServer, Arc<Timeline>) {
        fixture_events(thread, true).await
    }

    async fn fixture_events(thread: bool, populated: bool) -> (MockServer, Arc<Timeline>) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let root = json!({"type":"m.room.message","event_id":"$root","room_id":"!room:example.org","sender":"@other:example.org","origin_server_ts":1,"content":{"msgtype":"m.text","body":"Root"}});
        let reply = json!({"type":"m.room.message","event_id":"$reply","sender":"@other:example.org","origin_server_ts":2,"content":{"msgtype":"m.text","body":"Reply","m.relates_to":{"rel_type":"m.thread","event_id":"$root","is_falling_back":false}}});
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!room:example.org":{
                "state":{"events":[{"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":0,"content":{"membership":"join"}}]},
                "timeline":{"events": if populated { vec![root.clone(),reply] } else { vec![] },"limited":false},
                "account_data":{"events":[{"type":"m.marked_unread","content":{"unread":true}}]},
                "ephemeral":{"events":[{"type":"m.receipt","content":{"$root":{"m.read":{"@reader:example.org":{"ts":3},"@test:example.org":{"ts":3}}}}}]}
            }}}}))).mount(&server).await;
        Mock::given(method("GET"))
            .and(path_regex(".*/event/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(root))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(".*/relations/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[]})))
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
        client.event_cache().subscribe().unwrap();
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap();
        let timeline = TimelineBuilder::new(&room)
            .with_focus(if thread {
                TimelineFocus::Thread {
                    root_event_id: "$root".parse().unwrap(),
                }
            } else {
                TimelineFocus::Live {
                    hide_threaded_events: false,
                }
            })
            .track_read_marker_and_receipts(TimelineReadReceiptTracking::AllEvents)
            .build()
            .await
            .unwrap();
        (server, Arc::new(timeline))
    }

    fn selection(thread: bool) -> Selection {
        let mut selection = Selection {
            room: Some((1, "!room:example.org".into())),
            thread: thread.then(|| (2, "$root".into())),
            conversation_visible: true,
            ..Default::default()
        };
        selection.read[usize::from(thread)] = Some(ReadTarget {
            generation: if thread { 2 } else { 1 },
            event: Some("$reply".into()),
        });
        selection
    }

    async fn wait_requests(server: &MockServer, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let requests = server.received_requests().await.unwrap();
                if requests
                    .iter()
                    .filter(|request| {
                        request.url.path().contains("/receipt/")
                            || request.url.path().contains("/account_data/")
                                && request.method == "PUT"
                    })
                    .count()
                    >= count
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn enabling_sharing_after_synced_private_receipt_can_publish_same_event() {
        let (server, timeline) = fixture(false).await;
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"private","rooms":{"join":{"!room:example.org":{
                "timeline":{"events":[],"limited":false},
                "ephemeral":{"events":[{"type":"m.receipt","content":{"$reply":{"m.read.private":{"@test:example.org":{"ts":4}}}}}]}
            }}}}))).with_priority(1).expect(1).mount(&server).await;
        timeline
            .room()
            .client()
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if timeline
                    .latest_user_read_receipt(timeline.room().own_user_id())
                    .await
                    .is_some_and(|(event, _)| event.as_str() == "$reply")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        Mock::given(method("POST"))
            .and(path_regex(".*/receipt/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(".*/account_data/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let (control, requests) = watch::channel(selection(false));
        let (state, _) = watch::channel(Model::default());
        let worker = tokio::spawn(run(timeline, requests, 1, false, state));
        wait_requests(&server, 4).await;
        control.send_modify(|s| s.shutdown = true);
        worker.await.unwrap();
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|r| r.url.path().ends_with("/receipt/m.read/$reply"))
        );
    }

    #[tokio::test]
    async fn privacy_switch_at_same_event_revisits_receipts_and_preserves_markers() {
        let (server, timeline) = fixture(false).await;
        Mock::given(method("POST"))
            .and(path_regex(".*/receipt/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(6)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(".*/account_data/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let (control, requests) = watch::channel(selection(false));
        let (state, _) = watch::channel(Model::default());
        let worker = tokio::spawn(run(timeline, requests, 1, false, state));
        wait_requests(&server, 4).await;
        control.send_modify(|s| s.privacy.public_read_receipts = false);
        wait_requests(&server, 8).await;
        control.send_modify(|s| s.privacy.public_read_receipts = true);
        wait_requests(&server, 12).await;
        control.send_modify(|s| s.shutdown = true);
        worker.await.unwrap();
        let paths: Vec<_> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().contains("/receipt/"))
            .map(|r| r.url.path().to_owned())
            .collect();
        for (path, kind) in paths.iter().zip([
            "m.read",
            "m.fully_read",
            "m.read.private",
            "m.fully_read",
            "m.read",
            "m.fully_read",
        ]) {
            assert!(path.ends_with(&format!("/receipt/{kind}/$reply")), "{path}");
        }
    }

    #[tokio::test]
    async fn private_thread_receipts_keep_thread_scope_and_do_not_clear_room() {
        let (server, timeline) = fixture(true).await;
        Mock::given(method("POST"))
            .and(path_regex(".*/receipt/m.read.private/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let mut initial = selection(true);
        initial.privacy.public_read_receipts = false;
        let (control, requests) = watch::channel(initial);
        let (state, _) = watch::channel(Model::default());
        let worker = tokio::spawn(run(timeline, requests, 2, true, state));
        wait_requests(&server, 1).await;
        control.send_modify(|s| s.shutdown = true);
        worker.await.unwrap();
        let requests = server.received_requests().await.unwrap();
        let receipt = requests
            .iter()
            .find(|r| r.url.path().contains("/receipt/"))
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&receipt.body).unwrap(),
            json!({"thread_id":"$root"})
        );
        assert!(
            !requests
                .iter()
                .any(|r| r.method == "PUT" || r.url.path().contains("m.fully_read"))
        );
    }

    #[tokio::test]
    async fn catch_up_boundary_is_frozen_per_room_visit() {
        use super::super::{History, publish};
        use iced::futures::StreamExt;
        let (server, timeline) = fixture(false).await;
        let (state, _) = watch::channel(Model::default());
        let (older, _) = tokio::sync::mpsc::channel(1);
        let boundary = || state.borrow().room.as_ref().unwrap().read_boundary.clone();
        publish(&timeline, 1, false, 0, &History::Ready, &state, &older).await;
        assert_eq!(boundary().as_deref(), Some("$root"));
        let (_, updates) = timeline.subscribe().await;
        tokio::pin!(updates);
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"two","rooms":{"join":{"!room:example.org":{
                "timeline":{"events":[],"limited":false},
                "ephemeral":{"events":[{"type":"m.receipt","content":{"$reply":{"m.read":{"@test:example.org":{"ts":4}}}}}]}
            }}}}))).with_priority(1).expect(1).mount(&server).await;
        timeline
            .room()
            .client()
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let read = timeline
                    .latest_user_read_receipt(timeline.room().own_user_id())
                    .await;
                if read.is_some_and(|(id, _)| id.as_str() == "$reply") {
                    break;
                }
                assert!(updates.next().await.is_some());
            }
        })
        .await
        .unwrap();
        publish(&timeline, 1, false, 1, &History::End, &state, &older).await;
        assert_eq!(boundary().as_deref(), Some("$root"));
        publish(&timeline, 2, false, 0, &History::Ready, &state, &older).await;
        assert_eq!(boundary().as_deref(), Some("$reply"));
        // An absent opening receipt also stays absent for that visit.
        state.send_modify(|s| s.room.as_mut().unwrap().read_boundary = None);
        publish(&timeline, 2, false, 1, &History::Ready, &state, &older).await;
        assert!(boundary().is_none());
        publish(&timeline, 3, true, 0, &History::Ready, &state, &older).await;
        assert!(
            state
                .borrow()
                .thread
                .as_ref()
                .unwrap()
                .read_boundary
                .is_none()
        );
    }

    #[tokio::test]
    async fn room_receipt_and_fully_read_are_bounded_to_visible_event_and_deduplicated() {
        let (server, timeline) = fixture(false).await;
        Mock::given(method("POST"))
            .and(path_regex(".*/receipt/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(2)
            .mount(&server)
            .await;
        // Each successful unthreaded receipt asks to unset the manual flag until sync confirms it.
        Mock::given(method("PUT"))
            .and(path_regex(".*/account_data/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let initial = selection(false);
        let (control, requests) = watch::channel(initial.clone());
        let (state, _) = watch::channel(Model::default());
        let worker = tokio::spawn(run(timeline.clone(), requests, 1, false, state));
        wait_requests(&server, 4).await;
        for _ in 0..3 {
            control.send_replace(initial.clone());
            tokio::task::yield_now().await;
        }
        let requests = server.received_requests().await.unwrap();
        let receipts: Vec<_> = requests
            .iter()
            .filter(|request| request.url.path().contains("/receipt/"))
            .collect();
        assert_eq!(receipts.len(), 2);
        assert!(receipts.iter().all(|request| {
            request.url.path().ends_with("/$reply")
                && !serde_json::from_slice::<serde_json::Value>(&request.body)
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .contains_key("thread_id")
        }));
        assert!(
            requests
                .iter()
                .filter(|request| request.method == "PUT")
                .all(
                    |request| serde_json::from_slice::<serde_json::Value>(&request.body).unwrap()
                        == json!({"unread":false})
                )
        );
        let rows = super::super::timeline_rows(&timeline).await;
        let root = rows
            .iter()
            .find(|row| row.event_id.as_deref() == Some("$root"))
            .unwrap();
        assert!(root.read_by.contains(&"@reader:example.org".to_owned()));
        assert!(!root.read_by.contains(&"@test:example.org".to_owned()));
        control.send_modify(|selection| selection.shutdown = true);
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn thread_receipt_does_not_clear_room_marker() {
        let (server, timeline) = fixture(true).await;
        Mock::given(method("POST"))
            .and(path_regex(".*/receipt/m.read/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let (control, requests) = watch::channel(selection(true));
        let (state, _) = watch::channel(Model::default());
        let worker = tokio::spawn(run(timeline, requests, 2, true, state));
        wait_requests(&server, 1).await;
        control.send_modify(|selection| selection.shutdown = true);
        worker.await.unwrap();
        let requests = server.received_requests().await.unwrap();
        let receipt = requests
            .iter()
            .find(|request| request.url.path().contains("/receipt/"))
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&receipt.body).unwrap(),
            json!({"thread_id":"$root"})
        );
        assert!(
            !requests
                .iter()
                .any(|request| request.method == "PUT"
                    || request.url.path().contains("m.fully_read"))
        );
    }

    #[tokio::test]
    async fn failures_surface_once_and_refocusing_retries() {
        let (server, timeline) = fixture(false).await;
        Mock::given(method("POST"))
            .and(path_regex(".*/receipt/.*"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(json!({"errcode":"M_UNKNOWN","error":"fixture failure"})),
            )
            .expect(2)
            .mount(&server)
            .await;
        let (control, requests) = watch::channel(selection(false));
        let (state, mut updates) = watch::channel(Model::default());
        let worker = tokio::spawn(run(timeline, requests, 1, false, state));
        tokio::time::timeout(
            Duration::from_secs(5),
            updates.wait_for(|model| model.receipt_errors[0].is_some()),
        )
        .await
        .unwrap()
        .unwrap();
        control.send_modify(|selection| selection.avatars.clear());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), updates.changed())
                .await
                .is_err()
        );
        control.send_modify(|selection| selection.conversation_visible = false);
        tokio::time::sleep(Duration::from_millis(20)).await;
        control.send_modify(|selection| selection.conversation_visible = true);
        wait_requests(&server, 2).await;
        control.send_modify(|selection| selection.shutdown = true);
        worker.await.unwrap();
    }
    #[tokio::test]
    async fn empty_view_clears_only_manual_flag_and_rejects_a_superseded_empty_target() {
        for populated in [false, true] {
            let (server, timeline) = fixture_events(false, populated).await;
            Mock::given(method("PUT"))
                .and(path_regex(".*/account_data/.*"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .expect(if populated { 0 } else { 1 })
                .mount(&server)
                .await;
            let mut selection = selection(false);
            selection.read[0].as_mut().unwrap().event = None;
            let (control, requests) = watch::channel(selection);
            let (state, mut updates) = watch::channel(Model::default());
            let worker = tokio::spawn(run(timeline, requests, 1, false, state));
            if populated {
                assert!(
                    tokio::time::timeout(Duration::from_millis(50), updates.changed())
                        .await
                        .is_err()
                );
            } else {
                wait_requests(&server, 1).await;
            }
            control.send_modify(|selection| selection.shutdown = true);
            worker.await.unwrap();
            assert!(
                !server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .any(|request| request.url.path().contains("/receipt/"))
            );
        }
    }

    #[tokio::test]
    async fn receipt_only_sync_moves_reader_and_clears_navigation_unread_flag() {
        use iced::futures::StreamExt;
        let (server, timeline) = fixture(false).await;
        assert!(timeline.room().is_marked_unread());
        let (_, updates) = timeline.subscribe().await;
        tokio::pin!(updates);
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"two","rooms":{"join":{"!room:example.org":{
                "timeline":{"events":[],"limited":false},
                "account_data":{"events":[{"type":"m.marked_unread","content":{"unread":false}}]},
                "ephemeral":{"events":[{"type":"m.receipt","content":{"$reply":{"m.read":{"@reader:example.org":{"ts":4}}}}}]}
            }}}}))).with_priority(1).expect(1).mount(&server).await;
        timeline
            .room()
            .client()
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let rows = super::super::timeline_rows(&timeline).await;
                if rows.iter().any(|row| {
                    row.event_id.as_deref() == Some("$reply")
                        && row.read_by.contains(&"@reader:example.org".into())
                }) {
                    assert!(
                        rows.iter()
                            .filter(|row| row.event_id.as_deref() == Some("$root"))
                            .all(|row| !row.read_by.contains(&"@reader:example.org".into()))
                    );
                    break;
                }
                assert!(updates.next().await.is_some());
            }
        })
        .await
        .unwrap();
        assert!(!timeline.room().is_marked_unread());
    }

    #[tokio::test]
    async fn focus_loss_cancels_pending_receipt_and_shutdown_finishes_promptly() {
        let (server, timeline) = fixture(false).await;
        Mock::given(method("POST"))
            .and(path_regex(".*/receipt/.*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({}))
                    .set_delay(Duration::from_secs(60)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (control, requests) = watch::channel(selection(false));
        let (state, mut updates) = watch::channel(Model::default());
        let worker = tokio::spawn(run(timeline, requests, 1, false, state));
        wait_requests(&server, 1).await;
        control.send_modify(|selection| selection.conversation_visible = false);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), updates.changed())
                .await
                .is_err()
        );
        control.send_modify(|selection| selection.shutdown = true);
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.method == "PUT"
                    || request.url.path().contains("m.fully_read"))
        );
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

    #[test]
    fn desired_rejects_stale_generations_hidden_views_and_shutdown() {
        let target = ReadTarget {
            generation: 3,
            event: Some("$seen".into()),
        };
        let mut selection = Selection {
            room: Some((3, "!room:example.org".into())),
            conversation_visible: true,
            read: [Some(target.clone()), None],
            ..Default::default()
        };
        assert_eq!(desired(&selection, 3, false), Some(target));
        assert_eq!(desired(&selection, 2, false), None);
        assert_eq!(desired(&selection, 3, true), None);
        selection.conversation_visible = false;
        assert_eq!(desired(&selection, 3, false), None);
        selection.conversation_visible = true;
        selection.shutdown = true;
        assert_eq!(desired(&selection, 3, false), None);
    }
}
