//! Explicit reports to the account's homeserver, independent of moderation power.
use matrix_sdk::{Room, RoomState, config::RequestConfig, ruma::OwnedEventId};
use std::time::Duration;

pub const MAX_REASON_BYTES: usize = 4000;

pub fn validate_reason(reason: &str) -> Result<&str, String> {
    let reason = reason.trim();
    if reason.is_empty() || reason.len() > MAX_REASON_BYTES {
        return Err(format!("Enter a reason within {MAX_REASON_BYTES} bytes."));
    }
    Ok(reason)
}

/// Dropping this future cancels network work; callers retain admitted writes
/// across navigation. A lost response is ambiguous and must not auto-retry.
pub async fn report(room: &Room, event: String, reason: String) -> Result<(), String> {
    let reason = validate_reason(&reason)?.to_owned();
    let event: OwnedEventId = event
        .parse()
        .map_err(|_| "This message has no valid event ID.")?;
    if room.state() != RoomState::Joined {
        return Err("You are no longer joined to this room or space.".into());
    }
    let mut request = matrix_sdk::ruma::api::client::room::report_content::v3::Request::new(
        room.room_id().to_owned(),
        event,
    );
    request.reason = Some(reason);
    tokio::time::timeout(Duration::from_secs(30), async {
        room.client().send(request)
            .with_request_config(RequestConfig::short_retry().retry_limit(0)).await
            .map_err(|_| "Could not confirm the report. It may have reached your homeserver; retry only if needed.".to_owned())?;
        Ok(())
    }).await.map_err(|_| "Reporting timed out. The report may have reached your homeserver; retry only if needed.".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{Client, authentication::matrix::MatrixSession, config::SyncSettings};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    async fn fixture(space: bool) -> (MockServer, Room) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
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
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one","rooms":{"join":{"!target:example.org":{
                "state":{"events":[{"type":"m.room.create","state_key":"","sender":"@test:example.org","event_id":"$create","origin_server_ts":1,"content":if space { json!({"creator":"@test:example.org","type":"m.space"}) } else { json!({"creator":"@test:example.org"}) }}]},
                "timeline":{"events":[],"limited":false}
            }}}}))).mount(&server).await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!target:example.org"))
            .unwrap();
        (server, room)
    }
    #[test]
    fn reasons_must_be_nonempty_and_bounded() {
        assert_eq!(validate_reason("  Spam  ").unwrap(), "Spam");
        for reason in [" ".to_owned(), "é".repeat(MAX_REASON_BYTES)] {
            assert!(validate_reason(&reason).is_err());
        }
    }
    #[tokio::test]
    async fn reports_capture_room_event_and_reason_without_requiring_moderator_power() {
        for space in [false, true] {
            let (server, room) = fixture(space).await;
            assert_eq!(room.is_space(), space);
            Mock::given(method("POST"))
                .and(path(
                    "/_matrix/client/v3/rooms/!target:example.org/report/$message",
                ))
                .and(body_json(json!({"reason":"Spam"})))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .expect(1)
                .mount(&server)
                .await;
            report(&room, "$message".into(), "  Spam  ".into())
                .await
                .unwrap();
            for (event, reason) in [("invalid", "Spam"), ("$message", " ")] {
                assert!(report(&room, event.into(), reason.into()).await.is_err());
            }
            let requests = server.received_requests().await.unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|r| r.url.path().contains("/report/"))
                    .count(),
                1
            );
            assert!(!requests.iter().any(|r| {
                [
                    "/send/",
                    "/redact/",
                    "/ban",
                    "/unban",
                    "/invite",
                    "/createRoom",
                ]
                .iter()
                .any(|endpoint| r.url.path().contains(endpoint))
            }));
        }
    }
    #[tokio::test]
    async fn failure_is_single_attempt_and_dropping_a_report_cancels_it() {
        let (server, room) = fixture(false).await;
        let failed = Mock::given(method("POST"))
            .and(path(
                "/_matrix/client/v3/rooms/!target:example.org/report/$failure",
            ))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(json!({"errcode":"M_UNKNOWN","error":"fixture"})),
            )
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        assert!(
            report(&room, "$failure".into(), "Spam".into())
                .await
                .unwrap_err()
                .contains("may have reached")
        );
        drop(failed);
        let started = std::sync::Arc::new(tokio::sync::Notify::new());
        let notify = started.clone();
        Mock::given(method("POST"))
            .and(path(
                "/_matrix/client/v3/rooms/!target:example.org/report/$slow",
            ))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(60))
                    .set_body_json(json!({}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let task = tokio::spawn(async move { report(&room, "$slow".into(), "Spam".into()).await });
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}
