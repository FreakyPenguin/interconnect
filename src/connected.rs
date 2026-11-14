//! Connected UI experiment: one session, cancellable view futures, latest-state delivery.
use std::{future::pending, pin::Pin, sync::Arc, time::Duration};

use anyhow::{Context, ensure};
use iced::futures::{StreamExt, stream::BoxStream};
use matrix_sdk::Client;
use matrix_sdk_ui::{
    sync_service::{State, SyncService},
    timeline::{Timeline, TimelineBuilder, TimelineFocus},
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub room: Option<(u64, String)>,
    pub thread: Option<(u64, String)>,
    pub shutdown: bool,
}

#[derive(Clone, Debug)]
pub struct RoomEntry {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct MessageRow {
    pub event_id: Option<String>,
    pub sender: String,
    pub body: String,
    pub send_state: String,
}

#[derive(Clone, Debug)]
pub enum History {
    Loading,
    Ready,
    End,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Pane {
    pub generation: u64,
    pub rows: Arc<Vec<MessageRow>>,
    pub error: Option<String>,
    pub history: History,
    pub load_older: Option<mpsc::Sender<()>>,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub status: String,
    pub rooms: Arc<Vec<RoomEntry>>,
    pub room: Option<Pane>,
    pub thread: Option<Pane>,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            status: "Restoring saved session…".into(),
            rooms: Arc::default(),
            room: None,
            thread: None,
        }
    }
}

type ViewFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
fn inactive() -> ViewFuture {
    Box::pin(pending())
}

/// UI state is replaceable, so a watch channel retains only the latest snapshot.
/// SDK diffs are consumed in order by the SDK; we never coalesce raw diffs.
pub async fn run(
    profile: String,
    mut selection: watch::Receiver<Selection>,
    state: watch::Sender<Model>,
) -> anyhow::Result<()> {
    let restored = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(90), crate::session::restore(&profile)) => result.context("session restoration timed out")??,
        _ = selection.wait_for(|s| s.shutdown) => return Ok(()),
    };
    let sync = SyncService::builder(restored.client.clone())
        .build()
        .await?;
    let outcome = tokio::select! {
        result = crate::sso::start_and_wait(&sync) => result,
        _ = selection.wait_for(|s| s.shutdown) => Ok(()),
    };
    let outcome = if outcome.is_ok() && !selection.borrow().shutdown {
        connected(&restored.client, &sync, &mut selection, &state).await
    } else {
        outcome
    };
    state.send_modify(|s| s.status = "Stopping synchronization…".into());
    let stopped = crate::sso::stop_with_deadline(sync.stop(), Duration::from_secs(15)).await;
    // Keep the profile guard alive until SDK stop has completed or failed.
    drop(sync);
    drop(restored);
    outcome.and(stopped)
}

async fn connected(
    client: &Client,
    sync: &SyncService,
    selection: &mut watch::Receiver<Selection>,
    state: &watch::Sender<Model>,
) -> anyhow::Result<()> {
    refresh_rooms(client, state);
    let mut sync_states = sync.state();
    let mut room_states = sync.room_list_service().state();
    let mut active = Selection::default();
    let mut room_view = inactive();
    let mut thread_view = inactive();
    let result = async {
        loop {
            let next = selection.borrow_and_update().clone();
            if next.shutdown { break; }
            if active.room != next.room {
                // Drop the old subscriptions before constructing replacements.
                room_view = inactive();
                thread_view = inactive();
                if let Some((_, id)) = &active.room {
                    let id: matrix_sdk::ruma::OwnedRoomId = id.parse()?;
                    sync.room_list_service().remove_room_subscriptions(&[&id]);
                }
                state.send_modify(|s| { s.room = None; s.thread = None; });
                if let Some((generation, id)) = &next.room {
                    let id: matrix_sdk::ruma::OwnedRoomId = id.parse()?;
                    // Sync subscription setup is cancelled by a newer navigation request.
                    let room_list = sync.room_list_service();
                    let ids = [&*id];
                    tokio::select! {
                        _ = room_list.set_room_subscriptions(&ids) => {},
                        changed = selection.changed() => { changed.context("UI closed")?; sync.room_list_service().remove_room_subscriptions(&[&id]); active = Selection::default(); continue; }
                    }
                    room_view = Box::pin(view(client.clone(), id.to_string(), None, *generation, state.clone()));
                }
            }
            if active.thread != next.thread || active.room != next.room {
                thread_view = inactive();
                state.send_modify(|s| s.thread = None);
                if let (Some((_, room)), Some((generation, root))) = (&next.room, &next.thread) {
                    thread_view = Box::pin(view(client.clone(), room.clone(), Some(root.clone()), *generation, state.clone()));
                }
            }
            active = next;
            tokio::select! {
                changed = selection.changed() => { if changed.is_err() { break; } }
                _ = &mut room_view => room_view = inactive(),
                _ = &mut thread_view => thread_view = inactive(),
                event = room_states.next() => {
                    if event.is_none() { anyhow::bail!("room-list stream ended"); }
                    refresh_rooms(client, state);
                }
                event = sync_states.next() => match event {
                    Some(State::Error(_)) => anyhow::bail!("Synchronization failed; close and reopen to retry. Session retained."),
                    None | Some(State::Terminated) => anyhow::bail!("Synchronization ended. Session retained."),
                    _ => {},
                }
            }
        }
        Ok(())
    }.await;
    drop(room_view);
    drop(thread_view);
    if let Some((_, id)) = active.room {
        let id: matrix_sdk::ruma::OwnedRoomId = id.parse()?;
        sync.room_list_service().remove_room_subscriptions(&[&id]);
    }
    result
}

fn refresh_rooms(client: &Client, state: &watch::Sender<Model>) {
    let mut rooms: Vec<_> = client
        .joined_rooms()
        .into_iter()
        .map(|r| RoomEntry {
            id: r.room_id().to_string(),
            name: r.name().unwrap_or_else(|| r.room_id().to_string()),
        })
        .collect();
    rooms.sort_by_cached_key(|r| (r.name.to_lowercase(), r.id.clone()));
    state.send_if_modified(|s| {
        let changed = s.rooms.len() != rooms.len()
            || s.rooms
                .iter()
                .zip(&rooms)
                .any(|(a, b)| a.id != b.id || a.name != b.name);
        if changed {
            s.rooms = Arc::new(rooms);
        }
        let status = "Connected — select an encrypted room";
        let status_changed = s.status != status;
        if status_changed {
            s.status = status.into();
        }
        changed || status_changed
    });
}

struct ViewLifetime;
impl Drop for ViewLifetime {
    fn drop(&mut self) {
        tracing::debug!(target: "chatclient", "connected presentation view stopped");
    }
}

async fn view(
    client: Client,
    room_id: String,
    root: Option<String>,
    generation: u64,
    state: watch::Sender<Model>,
) {
    tracing::debug!(target: "chatclient", generation, thread = root.is_some(), "connected presentation view started");
    let _lifetime = ViewLifetime;
    let is_thread = root.is_some();
    let result: anyhow::Result<()> = async {
        let setup = async {
            let room_id: matrix_sdk::ruma::OwnedRoomId = room_id.parse()?;
            let room = client.get_room(&room_id).context("Room is not loaded")?;
            ensure!(
                room.latest_encryption_state().await?.is_encrypted(),
                "Select an encrypted room for this prototype"
            );
            let focus = match root {
                Some(root) => TimelineFocus::Thread {
                    root_event_id: root.parse()?,
                },
                None => TimelineFocus::Live {
                    hide_threaded_events: true,
                },
            };
            Ok::<_, anyhow::Error>(
                TimelineBuilder::new(&room)
                    .with_focus(focus)
                    .build()
                    .await?,
            )
        };
        let timeline = tokio::time::timeout(Duration::from_secs(60), setup)
            .await
            .context("Timeline loading timed out")??;
        let (_, updates) = timeline.subscribe().await;
        let mut updates: BoxStream<'_, _> = Box::pin(updates);
        let (load_older, mut requests) = mpsc::channel(1);
        let mut history = if is_thread { History::Ready } else {
            load_room_history(&timeline, generation, &mut updates, &state, &load_older).await?
        };
        publish(&timeline, generation, is_thread, &history, &state, &load_older).await;
        loop {
            tokio::select! {
                update = updates.next() => {
                    ensure!(update.is_some(), "Timeline subscription ended");
                }
                Some(()) = requests.recv() => {
                    if !is_thread && !matches!(history, History::End) {
                        history = load_room_history(&timeline, generation, &mut updates, &state, &load_older).await?;
                    }
                }
            }
            publish(&timeline, generation, is_thread, &history, &state, &load_older).await;
        }
    }
    .await;
    if let Err(error) = result {
        state.send_modify(|s| {
            let pane = if is_thread {
                &mut s.thread
            } else {
                &mut s.room
            };
            *pane = Some(Pane {
                generation,
                rows: Arc::default(),
                error: Some(format!("{error:#}")),
                history: History::Ready,
                load_older: None,
            });
        });
    }
}

// Owned by the view: room switches and shutdown cancel an in-flight page.
// Live snapshots continue during each fetch, including retries.
async fn load_room_history(
    timeline: &Timeline,
    generation: u64,
    updates: &mut (impl iced::futures::Stream + Unpin),
    state: &watch::Sender<Model>,
    load_older: &mpsc::Sender<()>,
) -> anyhow::Result<History> {
    publish(
        timeline,
        generation,
        false,
        &History::Loading,
        state,
        load_older,
    )
    .await;
    let page = tokio::time::timeout(Duration::from_secs(30), timeline.paginate_backwards(20));
    tokio::pin!(page);
    loop {
        tokio::select! {
            result = &mut page => return Ok(match result {
                Ok(Ok(true)) => History::End,
                Ok(Ok(false)) => History::Ready,
                Ok(Err(error)) => History::Failed(error.to_string()),
                Err(_) => History::Failed("History loading timed out".into()),
            }),
            update = updates.next() => {
                ensure!(update.is_some(), "Timeline subscription ended");
                publish(timeline, generation, false, &History::Loading, state, load_older).await;
            }
        }
    }
}

async fn publish(
    timeline: &Timeline,
    generation: u64,
    thread: bool,
    history: &History,
    state: &watch::Sender<Model>,
    load_older: &mpsc::Sender<()>,
) {
    let items = timeline.items().await;
    let rows = items
        .iter()
        .filter_map(|item| item.as_event())
        .map(|event| {
            let content = event.content();
            MessageRow {
                event_id: event.event_id().map(ToString::to_string),
                sender: event.sender().to_string(),
                body: if content.is_unable_to_decrypt() {
                    "Unable to decrypt — keys unavailable".into()
                } else {
                    content
                        .as_message()
                        .map(|m| m.body().to_owned())
                        .unwrap_or_else(|| "[non-text event]".into())
                },
                send_state: event
                    .send_state()
                    .map(|s| format!("{s:?}"))
                    .unwrap_or_default(),
            }
        })
        .collect();
    state.send_modify(|s| {
        let pane = if thread { &mut s.thread } else { &mut s.room };
        *pane = Some(Pane {
            generation,
            rows: Arc::new(rows),
            error: None,
            history: history.clone(),
            load_older: (!thread && matches!(history, History::Ready | History::Failed(_)))
                .then(|| load_older.clone()),
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::authentication::matrix::MatrixSession;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex, query_param},
    };

    #[tokio::test]
    async fn sdk_views_switch_and_shutdown_without_ui_consumption() {
        history_fixture(false).await;
    }

    #[tokio::test]
    async fn shutdown_cancels_pending_initial_history() {
        history_fixture(true).await;
    }

    async fn request_page(snapshots: &mut watch::Receiver<Model>) -> Pane {
        let sender = snapshots
            .borrow_and_update()
            .room
            .as_ref()
            .unwrap()
            .load_older
            .clone()
            .unwrap();
        sender.try_send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                snapshots.changed().await.unwrap();
                let pane = snapshots.borrow_and_update().room.clone().unwrap();
                if !matches!(pane.history, History::Loading) {
                    return pane;
                }
            }
        })
        .await
        .unwrap()
    }

    async fn history_fixture(cancel_history: bool) {
        let server = MockServer::start().await;
        let root = json!({"type":"m.room.message", "event_id":"$root", "sender":"@test:example.org", "origin_server_ts":1, "content":{"msgtype":"m.text", "body":"fixture root"}});
        Mock::given(method("GET")).and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"], "unstable_features":{"org.matrix.simplified_msc3575":true}}))).mount(&server).await;
        Mock::given(method("POST")).and(path("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(50)).set_body_json(json!({
                "pos":"test-pos", "lists":{"all_rooms":{"count":1,"ops":[{"op":"SYNC","range":[0,0],"room_ids":["!room:example.org"]}]}},
                "rooms":{"!room:example.org":{"name":"Fixture", "initial":true, "is_dm":false, "joined_count":1, "invited_count":0,
                    "required_state":[
                        {"type":"m.room.member", "state_key":"@test:example.org", "sender":"@test:example.org", "event_id":"$member", "origin_server_ts":0,"content":{"membership":"join"}},
                        {"type":"m.room.encryption","state_key":"","sender":"@test:example.org","event_id":"$encryption","origin_server_ts":0,"content":{"algorithm":"m.megolm.v1.aes-sha2"}}
                    ], "timeline":[],"prev_batch":"older","num_live":0}},
                "extensions":{"e2ee":{"device_one_time_keys_count":{"signed_curve25519":50},"device_unused_fallback_key_types":["signed_curve25519"]}}
            }))).mount(&server).await;
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
            .and(path_regex(".*/messages"))
            .and(query_param("from", "older"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(if cancel_history {
                        Duration::from_secs(60)
                    } else {
                        Duration::ZERO
                    })
                    .set_body_json(json!({"start":"older","end":"before-root","chunk":[root.clone()],"state":[]})),
            )
            .mount(&server)
            .await;
        let reply = json!({"type":"m.room.message", "event_id":"$hidden", "sender":"@test:example.org", "origin_server_ts":0, "content":{"msgtype":"m.text", "body":"hidden reply", "m.relates_to":{"rel_type":"m.thread","event_id":"$root","is_falling_back":true,"m.in_reply_to":{"event_id":"$root"}}}});
        Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .and(query_param("from", "before-root"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"start":"before-root","end":"before-reply","chunk":[reply],"state":[]}),
            ))
            .mount(&server)
            .await;
        let older = json!({"type":"m.room.message", "event_id":"$old", "sender":"@test:example.org", "origin_server_ts":0, "content":{"msgtype":"m.text", "body":"older visible message"}});
        Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .and(query_param("from", "before-reply"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"start":"before-reply","chunk":[older],"state":[]})),
            )
            .mount(&server)
            .await;
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
            .with_threading_support(matrix_sdk::ThreadingSupport::Enabled {
                with_subscriptions: false,
            })
            .build()
            .await
            .unwrap();
        let session: MatrixSession = serde_json::from_value(
            json!({"user_id":"@test:example.org","device_id":"TEST","access_token":"fake"}),
        )
        .unwrap();
        client
            .matrix_auth()
            .restore_session(session, Default::default())
            .await
            .unwrap();
        let sync = SyncService::builder(client.clone()).build().await.unwrap();
        crate::sso::start_and_wait(&sync).await.unwrap();
        let (navigation, mut selections) = watch::channel(Selection::default());
        let (state, mut snapshots) = watch::channel(Model::default());
        let worker = tokio::spawn(async move {
            let result = connected(&client, &sync, &mut selections, &state).await;
            crate::sso::stop_with_deadline(sync.stop(), Duration::from_secs(5))
                .await
                .unwrap();
            result.unwrap();
        });
        if cancel_history {
            navigation.send_modify(|s| s.room = Some((1, "!room:example.org".into())));
            tokio::time::timeout(
                Duration::from_secs(5),
                snapshots.wait_for(|s| {
                    s.room
                        .as_ref()
                        .is_some_and(|p| matches!(p.history, History::Loading))
                }),
            )
            .await
            .unwrap()
            .unwrap();
            navigation.send_modify(|s| s.shutdown = true);
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap();
            return;
        }
        for generation in 1..=3 {
            navigation.send_replace(Selection {
                room: Some((generation, "!room:example.org".into())),
                ..Default::default()
            });
            let model = tokio::time::timeout(
                Duration::from_secs(5),
                snapshots.wait_for(|s| {
                    s.room.as_ref().is_some_and(|p| {
                        p.generation == generation && !matches!(p.history, History::Loading)
                    })
                }),
            )
            .await
            .unwrap()
            .unwrap()
            .clone();
            let pane = model.room.unwrap();
            assert!(pane.error.is_none(), "{:?}", pane.error);
            assert!(pane.rows.iter().any(|r| r.body == "fixture root"));
            if generation == 1 {
                let page = request_page(&mut snapshots).await;
                assert!(matches!(page.history, History::Ready));
                assert!(!page.rows.iter().any(|r| r.body == "hidden reply"));
                assert!(
                    page.load_older.is_some(),
                    "a hidden-only page must allow further pagination"
                );
                let page = request_page(&mut snapshots).await;
                assert!(page.rows.iter().any(|r| r.body == "older visible message"));
                assert!(matches!(page.history, History::End));
                assert!(page.load_older.is_none());
            }
            navigation.send_modify(|s| s.thread = Some((generation, "$root".into())));
            let model = tokio::time::timeout(
                Duration::from_secs(5),
                snapshots.wait_for(|s| {
                    s.thread.as_ref().is_some_and(|p| {
                        p.generation == generation && !matches!(p.history, History::Loading)
                    })
                }),
            )
            .await
            .unwrap()
            .unwrap()
            .clone();
            assert!(model.thread.unwrap().error.is_none());
            navigation.send_modify(|s| s.thread = None);
            let model = tokio::time::timeout(
                Duration::from_secs(5),
                snapshots.wait_for(|s| s.thread.is_none()),
            )
            .await
            .unwrap()
            .unwrap()
            .clone();
            assert_eq!(model.room.unwrap().generation, generation);
        }
        // A UI that stops draining snapshots cannot block navigation or shutdown.
        navigation.send_modify(|s| s.shutdown = true);
        tokio::time::timeout(Duration::from_secs(6), worker)
            .await
            .unwrap()
            .unwrap();
    }
}
