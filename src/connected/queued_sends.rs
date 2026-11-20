//! Queue cancellation is separate from redacting an already sent event.
use super::RuntimeOwned;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct CancelSend {
    pub room: String,
    pub transaction: String,
    handle: Arc<RuntimeOwned<matrix_sdk::send_queue::SendHandle>>,
}
impl CancelSend {
    pub(super) fn new(
        room: String,
        transaction: String,
        handle: matrix_sdk::send_queue::SendHandle,
    ) -> Self {
        Self {
            room,
            transaction,
            handle: RuntimeOwned::new(handle),
        }
    }

    /// The SDK aborts pending uploads and removes unsent queue entries. If a send
    /// is already in flight, it may finish and then be redacted by the SDK.
    /// `true` acknowledges the local cancellation, not server-side removal.
    pub async fn cancel(self) -> Result<bool, String> {
        // SDK abort mutates queue dependencies in several awaited storage steps.
        // Keep the admitted operation alive until completion; dropping it on a
        // presentation timeout could leave only part of that graph removed.
        self.handle.abort().await.map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connected::{Composer, MessageRow, timeline_rows};
    use iced::futures::StreamExt;
    use matrix_sdk::{Client, authentication::matrix::MatrixSession, config::SyncSettings};
    use matrix_sdk_ui::timeline::{Timeline, TimelineBuilder};
    use serde_json::json;
    use std::time::Duration;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    async fn fixture() -> (MockServer, Client, Arc<Timeline>) {
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
                    .set_body_json(json!({"start":"one","chunk":[],"state":[]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!room:example.org":{"state":{"events":[{"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":0,"content":{"membership":"join"}}]},"timeline":{"events":[],"limited":false}}}}}))).mount(&server).await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
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
        client.send_queue().set_enabled(false).await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap();
        let timeline = Arc::new(TimelineBuilder::new(&room).build().await.unwrap());
        (server, client, timeline)
    }
    async fn wait_row(
        timeline: &Timeline,
        body: &str,
        matches: impl Fn(&MessageRow) -> bool,
    ) -> MessageRow {
        let (_, mut updates) = timeline.subscribe().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(row) = timeline_rows(timeline)
                    .await
                    .into_iter()
                    .find(|row| row.body == body && matches(row))
                {
                    return row;
                }
                updates.next().await.expect("timeline remains active");
            }
        })
        .await
        .expect("expected queue state reaches the timeline")
    }
    async fn wait_removed(timeline: &Timeline, transaction: &str) {
        let (_, mut updates) = timeline.subscribe().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if timeline_rows(timeline).await.iter().all(|row| {
                    row.cancel_send
                        .as_ref()
                        .is_none_or(|handle| handle.transaction != transaction)
                }) {
                    return;
                }
                updates.next().await.expect("timeline remains active");
            }
        })
        .await
        .expect("cancelled local echo disappears");
    }
    async fn send(timeline: &Timeline, body: &str) {
        timeline
            .send(
                matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(body)
                    .into(),
            )
            .await
            .unwrap();
    }
    async fn successful_sends(server: &MockServer) {
        Mock::given(method("PUT"))
            .and(path_regex(".*/send/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$sent"})))
            .mount(server)
            .await;
    }
    #[tokio::test]
    async fn pending_and_failed_messages_cancel_without_sending_or_redacting() {
        for failed in [false, true] {
            let (server, client, timeline) = fixture().await;
            Mock::given(method("PUT"))
                .and(path_regex(".*/send/.*"))
                .respond_with(
                    ResponseTemplate::new(403)
                        .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"fixture denial"})),
                )
                .mount(&server)
                .await;
            send(&timeline, "cancel fixture").await;
            if failed {
                client.send_queue().set_enabled(true).await;
            }
            let row = wait_row(&timeline, "cancel fixture", |row| {
                if failed {
                    row.retry.is_some()
                } else {
                    row.cancel_send.is_some()
                }
            })
            .await;
            assert!(row.event_id.is_none());
            let handle = row.cancel_send.unwrap();
            let transaction = handle.transaction.clone();
            assert!(handle.cancel().await.unwrap());
            wait_removed(&timeline, &transaction).await;
            let (echoes, _) = timeline.room().send_queue().subscribe().await.unwrap();
            assert!(echoes.is_empty());
            let requests = server.received_requests().await.unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.url.path().contains("/send/"))
                    .count(),
                usize::from(failed)
            );
            assert!(
                requests
                    .iter()
                    .all(|request| !request.url.path().contains("/redact/"))
            );
        }
    }
    #[tokio::test]
    async fn stale_handle_does_not_cancel_or_redact_a_sent_message() {
        let (server, client, timeline) = fixture().await;
        successful_sends(&server).await;
        send(&timeline, "already sent fixture").await;
        let handle = wait_row(&timeline, "already sent fixture", |row| {
            row.cancel_send.is_some()
        })
        .await
        .cancel_send
        .unwrap();
        client.send_queue().set_enabled(true).await;
        let sent = wait_row(&timeline, "already sent fixture", |row| {
            row.send_state == "Sent"
        })
        .await;
        assert!(sent.cancel_send.is_none());
        assert!(!handle.cancel().await.unwrap());
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| !request.url.path().contains("/redact/"))
        );
    }
    #[tokio::test]
    async fn send_in_flight_uses_the_sdks_followup_redaction() {
        let (server, client, timeline) = fixture().await;
        let started = Arc::new(tokio::sync::Notify::new());
        let notify = started.clone();
        Mock::given(method("PUT"))
            .and(path_regex(".*/send/.*"))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(1))
                    .set_body_json(json!({"event_id":"$racing"}))
            })
            .mount(&server)
            .await;
        let redacted = Arc::new(tokio::sync::Notify::new());
        let notify = redacted.clone();
        Mock::given(method("PUT"))
            .and(path_regex(".*/redact/.*"))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(200).set_body_json(json!({"event_id":"$redacted"}))
            })
            .mount(&server)
            .await;
        send(&timeline, "send race fixture").await;
        let handle = wait_row(&timeline, "send race fixture", |row| {
            row.cancel_send.is_some()
        })
        .await
        .cancel_send
        .unwrap();
        client.send_queue().set_enabled(true).await;
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        assert!(handle.cancel().await.unwrap());
        tokio::time::timeout(Duration::from_secs(5), redacted.notified())
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn active_upload_cancels_its_dependent_message_and_allows_the_next_send() {
        let (server, client, timeline) = fixture().await;
        successful_sends(&server).await;
        Mock::given(method("GET"))
            .and(path_regex(".*/config"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"m.upload.size":52428800})),
            )
            .mount(&server)
            .await;
        let started = Arc::new(tokio::sync::Notify::new());
        let notify = started.clone();
        Mock::given(method("POST"))
            .and(path_regex(".*/upload"))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(1))
                    .set_body_json(json!({"content_uri":"mxc://example.org/cancelled"}))
            })
            .mount(&server)
            .await;
        let file = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
        std::fs::write(file.path(), "queued upload fixture").unwrap();
        let attachment = crate::attachment::Attachment::from_path(file.path().to_owned())
            .await
            .unwrap();
        let body = attachment.filename.clone();
        Composer(RuntimeOwned::new(timeline.clone()))
            .send_attachment_reply(attachment, String::new(), None)
            .await
            .unwrap();
        let handle = wait_row(&timeline, &body, |row| row.cancel_send.is_some())
            .await
            .cancel_send
            .unwrap();
        client.send_queue().set_enabled(true).await;
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        let transaction = handle.transaction.clone();
        assert!(handle.cancel().await.unwrap());
        wait_removed(&timeline, &transaction).await;
        send(&timeline, "after cancellation").await;
        wait_row(&timeline, "after cancellation", |row| {
            row.send_state == "Sent"
        })
        .await;
        let requests = server.received_requests().await.unwrap();
        let sends: Vec<_> = requests
            .iter()
            .filter(|request| request.url.path().contains("/send/"))
            .collect();
        assert_eq!(sends.len(), 1);
        let content: serde_json::Value = serde_json::from_slice(&sends[0].body).unwrap();
        assert_eq!(content["body"], "after cancellation");
    }
}
