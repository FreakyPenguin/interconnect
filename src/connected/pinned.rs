//! Selected-room pins. Dropping the view cancels setup and releases SDK subscriptions.
use super::{MessageRow, Model, ViewFuture, inactive, timeline_rows};
use iced::futures::StreamExt;
use matrix_sdk::{Client, Room, ruma::OwnedEventId};
use matrix_sdk_ui::timeline::{TimelineBuilder, TimelineFocus};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct Pinned {
    pub generation: u64,
    pub total: usize,
    pub rows: Arc<Vec<MessageRow>>,
    pub loading: bool,
    pub error: Option<String>,
}

pub(super) async fn view(
    client: Client,
    room: String,
    generation: u64,
    state: watch::Sender<Model>,
) {
    let Ok(id) = room.parse::<matrix_sdk::ruma::OwnedRoomId>() else {
        return;
    };
    let Some(room) = client.get_room(&id) else {
        return;
    };
    // Subscribe before reading state, including rooms whose first pins arrive later.
    let changes = room.pinned_event_ids_stream();
    tokio::pin!(changes);
    let mut ids = room.pinned_event_ids().unwrap_or_default();
    ids.sort();
    ids.dedup();
    let mut display = snapshot(&room, &ids, generation, &state);
    loop {
        tokio::select! {
            _ = &mut display => display = inactive(),
            next = changes.next() => {
                let Some(mut next) = next else { return; };
                next.sort();
                next.dedup();
                if next != ids {
                    ids = next;
                    // Cancel in-flight loading before building a new pinned timeline.
                    drop(display);
                    display = snapshot(&room, &ids, generation, &state);
                }
            }
        }
    }
}
fn snapshot(
    room: &Room,
    ids: &[OwnedEventId],
    generation: u64,
    state: &watch::Sender<Model>,
) -> ViewFuture {
    if ids.is_empty() {
        state.send_modify(|s| s.pinned = None);
        return inactive();
    }
    let room = room.clone();
    let state = state.clone();
    let ids = ids.to_vec();
    let total = ids.len();
    let expected = total.min(
        room.client()
            .event_cache()
            .config()
            .max_pinned_events_to_load,
    );
    state.send_modify(|s| {
        s.pinned = Some(Pinned {
            generation,
            total,
            rows: Arc::default(),
            loading: true,
            error: None,
        })
    });
    Box::pin(async move {
        let setup = TimelineBuilder::new(&room)
            .with_focus(TimelineFocus::PinnedEvents)
            .build();
        let timeline = match tokio::time::timeout(Duration::from_secs(30), setup).await {
            Ok(Ok(timeline)) => timeline,
            result => {
                let error = match result {
                    Ok(Err(_)) => "Could not load pinned messages.",
                    Err(_) => "Loading pinned messages timed out.",
                    Ok(Ok(_)) => unreachable!(),
                };
                state.send_modify(|s| {
                    s.pinned = Some(Pinned {
                        generation,
                        total,
                        rows: Arc::default(),
                        loading: false,
                        error: Some(error.into()),
                    })
                });
                return;
            }
        };
        let timeline = Arc::new(timeline);
        state.borrow().diagnostics.timeline(&timeline);
        let (_, updates) = timeline.subscribe().await;
        tokio::pin!(updates);
        let deadline = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(deadline);
        let mut expired = false;
        loop {
            let mut rows = timeline_rows(&timeline).await;
            let mut seen = std::collections::HashSet::new();
            rows.retain(|row| {
                row.event_id.as_ref().is_some_and(|id| {
                    ids.iter().any(|pin| pin.as_str() == id) && seen.insert(id.clone())
                })
            });
            let loading = !expired && rows.len() < expected;
            state.send_modify(|s| {
                s.pinned = Some(Pinned {
                    generation,
                    total,
                    rows: Arc::new(rows),
                    loading,
                    error: None,
                })
            });
            tokio::select! {
                update = updates.next() => if update.is_none() { return; },
                _ = &mut deadline, if loading => expired = true,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    #[tokio::test]
    async fn pins_outside_history_load_follow_updates_and_cancel_without_late_snapshots() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(".*/relations/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[]})))
            .mount(&server)
            .await;
        for (id, body) in [
            ("$one", "An older pinned message"),
            ("$two", "Another pinned message"),
        ] {
            Mock::given(method("GET"))
                .and(path(format!(
                    "/_matrix/client/v3/rooms/!room:example.org/event/{id}"
                )))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "type":"m.room.message", "event_id":id, "room_id":"!room:example.org",
                    "sender":"@test:example.org", "origin_server_ts":10,
                    "content":{"msgtype":"m.text","body":body}
                })))
                .mount(&server)
                .await;
        }
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(json!({
                    "user_id":"@test:example.org", "device_id":"TEST", "access_token":"fake"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        client.event_cache().subscribe().unwrap();
        let event = |kind: &str, key: &str, content: serde_json::Value| {
            json!({
                "type":kind,"state_key":key,"event_id":format!("${kind}"),
                "sender":"@test:example.org","origin_server_ts":1,"content":content
            })
        };
        let response = Arc::new(std::sync::Mutex::new(
            json!({"next_batch":"seed","rooms":{"join":{
                "!room:example.org":{"state":{"events":[
                    event("m.room.create","",json!({"creator":"@test:example.org"})),
                    event("m.room.member","@test:example.org",json!({"membership":"join"})),
                    event("m.room.pinned_events","",json!({"pinned":[]}))
                ]},"timeline":{"events":[],"limited":false}}
            }}}),
        ));
        let body = response.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(body.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let (state, mut snapshots) = watch::channel(Model::default());
        let worker = tokio::spawn(view(
            client.clone(),
            "!room:example.org".into(),
            7,
            state.clone(),
        ));
        *response.lock().unwrap() = json!({"next_batch":"pins","rooms":{"join":{
            "!room:example.org":{"state":{"events":[event("m.room.pinned_events","",json!({"pinned":["$one","$two"]}))]},"timeline":{"events":[],"limited":false}}
        }}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let loaded = tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|model| {
                model
                    .pinned
                    .as_ref()
                    .is_some_and(|pins| pins.rows.len() == 2 && !pins.loading)
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        let pins = loaded.pinned.unwrap();
        assert_eq!(pins.generation, 7);
        assert_eq!(pins.total, 2);
        assert!(
            pins.rows
                .iter()
                .any(|row| row.body == "An older pinned message")
        );
        assert!(
            loaded.room.is_none(),
            "pins do not require room history or pagination"
        );
        *response.lock().unwrap() = json!({"next_batch":"unpin","rooms":{"join":{
            "!room:example.org":{"state":{"events":[event("m.room.pinned_events","",json!({"pinned":["$two"]}))]},"timeline":{"events":[],"limited":false}}
        }}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|model| {
                model.pinned.as_ref().is_some_and(|pins| {
                    pins.total == 1
                        && pins.rows.len() == 1
                        && pins.rows[0].event_id.as_deref() == Some("$two")
                })
            }),
        )
        .await
        .unwrap()
        .unwrap();
        *response.lock().unwrap() = json!({"next_batch":"redacted","rooms":{"join":{
            "!room:example.org":{"timeline":{"events":[{
                "type":"m.room.redaction","event_id":"$redaction","sender":"@test:example.org",
                "origin_server_ts":20,"redacts":"$two","content":{"redacts":"$two"}
            }],"limited":false}}
        }}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|model| {
                model.pinned.as_ref().is_some_and(|pins| {
                    pins.rows.len() == 1 && pins.rows[0].body == "Message removed"
                })
            }),
        )
        .await
        .unwrap()
        .unwrap();
        *response.lock().unwrap() = json!({"next_batch":"empty","rooms":{"join":{
            "!room:example.org":{"state":{"events":[event("m.room.pinned_events","",json!({"pinned":[]}))]},"timeline":{"events":[],"limited":false}}
        }}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|model| model.pinned.is_none()),
        )
        .await
        .unwrap()
        .unwrap();
        worker.abort();
        let _ = worker.await;
        snapshots.borrow_and_update();
        *response.lock().unwrap() = json!({"next_batch":"after-close","rooms":{"join":{
            "!room:example.org":{"state":{"events":[event("m.room.pinned_events","",json!({"pinned":["$one"]}))]}}
        }}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        assert!(
            !snapshots.has_changed().unwrap(),
            "a cancelled view cannot publish late snapshots"
        );
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.method == "PUT" || request.url.path().contains("/messages"))
        );
    }
}
