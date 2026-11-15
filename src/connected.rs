//! Connected UI experiment: one session, cancellable view futures, latest-state delivery.
use std::{future::pending, pin::Pin, sync::Arc, time::Duration};

use anyhow::{Context, ensure};
use iced::futures::{StreamExt, stream::BoxStream};
use matrix_sdk::Client;
use matrix_sdk_ui::{
    room_list_service::State as RoomState,
    sync_service::{State, SyncService},
    timeline::{
        EventItemOrigin, EventSendState, Timeline, TimelineBuilder, TimelineEventItemId,
        TimelineFocus, TimelineItemContent,
    },
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub room: Option<(u64, String)>,
    pub thread: Option<(u64, String)>,
    pub shutdown: bool,
    pub preferences: Option<crate::navigation_preferences::Preferences>,
}

pub use crate::navigation::{RoomEntry, SpaceEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Message,
    Activity,
}
impl RowKind {
    pub(crate) fn from_content(content: &TimelineItemContent) -> Self {
        match content {
            TimelineItemContent::MembershipChange(_)
            | TimelineItemContent::ProfileChange(_)
            | TimelineItemContent::OtherState(_)
            | TimelineItemContent::FailedToParseState { .. }
            | TimelineItemContent::CallInvite
            | TimelineItemContent::RtcNotification { .. } => Self::Activity,
            _ => Self::Message,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MessageRow {
    pub key: String,
    pub kind: RowKind,
    pub event_id: Option<String>,
    pub sender: String,
    pub display_name: String,
    pub timestamp_ms: Option<i64>,
    pub thread_replies: u32,
    pub thread_root: Option<String>,
    pub is_own: bool,
    pub is_live: bool,
    pub body: String,
    pub send_state: String,
    pub retry: Option<RetrySend>,
}

/// UI snapshots and messages can be destroyed on Iced's main thread, outside
/// Executor::enter. SDK/SQLite destructors may call Tokio's spawn_blocking.
/// Keep their final drop inside the originating runtime context, including when
/// a queued snapshot or retry message is discarded rather than handled.
#[derive(Debug)]
struct RuntimeOwned<T> {
    value: Option<T>,
    runtime: tokio::runtime::Handle,
}
impl<T> RuntimeOwned<T> {
    fn new(value: T) -> Arc<Self> {
        Arc::new(Self {
            value: Some(value),
            runtime: tokio::runtime::Handle::current(),
        })
    }
}
impl<T> std::ops::Deref for RuntimeOwned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
            .as_ref()
            .expect("runtime-owned value present until drop")
    }
}
impl<T> Drop for RuntimeOwned<T> {
    fn drop(&mut self) {
        let _entered = self.runtime.enter();
        drop(self.value.take());
    }
}

/// Captured send target; survives navigation until queue admission completes.
#[derive(Clone)]
pub struct Composer(Arc<RuntimeOwned<Arc<Timeline>>>);
impl std::fmt::Debug for Composer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Composer")
    }
}
impl Composer {
    pub async fn send(self, body: String) -> Result<(), String> {
        if body.trim().is_empty() {
            return Err("Message is empty".into());
        }
        Box::pin(
            self.0.send(
                matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(body)
                    .into(),
            ),
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct RetrySend {
    pub transaction: String,
    state: Arc<RuntimeOwned<RetryState>>,
}
#[derive(Debug)]
struct RetryState {
    handle: matrix_sdk::send_queue::SendHandle,
    queue: matrix_sdk::send_queue::RoomSendQueue,
}
impl RetrySend {
    pub async fn retry(self) -> Result<(), String> {
        self.state
            .handle
            .unwedge()
            .await
            .map_err(|e| e.to_string())?;
        self.state.queue.set_enabled(true);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum History {
    Loading,
    Ready,
    End,
    Failed(String),
}

const HISTORY_PAGE_SIZE: u16 = 50;
const INITIAL_THREAD_PAGES: usize = 10;
const INITIAL_THREAD_EVENTS: usize = 500;

#[derive(Clone, Debug)]
pub struct Pane {
    pub generation: u64,
    pub rows: Arc<Vec<MessageRow>>,
    pub error: Option<String>,
    pub history: History,
    pub history_revision: u64,
    pub load_older: Option<mpsc::Sender<u64>>,
    pub composer: Option<Composer>,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub diagnostics: Arc<crate::diagnostics::Diagnostics>,
    pub status: String,
    pub rooms: Arc<Vec<RoomEntry>>,
    pub spaces: Arc<Vec<SpaceEntry>>,
    pub navigation_error: Option<String>,
    pub saved_navigation: Option<crate::navigation_preferences::Preferences>,
    pub preferences_error: Option<String>,
    pub room: Option<Pane>,
    pub thread: Option<Pane>,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            diagnostics: Arc::default(),
            status: "Restoring saved session…".into(),
            rooms: Arc::default(),
            spaces: Arc::default(),
            navigation_error: None,
            saved_navigation: None,
            preferences_error: None,
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
    let directory = std::path::PathBuf::from(&profile);
    let load_directory = directory.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        crate::navigation_preferences::Preferences::load(&load_directory)
    })
    .await;
    let preferences = match loaded {
        Ok(Ok(preferences)) => preferences,
        _ => {
            state.send_modify(|s| {
                s.preferences_error =
                    Some("Could not restore navigation. Using the default view.".into())
            });
            Default::default()
        }
    };
    state.send_modify(|s| s.saved_navigation = Some(preferences.clone()));
    let sync = build_sync(restored.client.clone()).await?;
    let stop_preferences = tokio_util::sync::CancellationToken::new();
    let persistence = crate::navigation_preferences::persist(
        directory,
        selection.clone(),
        state.clone(),
        preferences,
        stop_preferences.clone(),
    );
    let connection = connected(&restored.client, &sync, &mut selection, &state);
    let outcome = {
        tokio::pin!(persistence, connection);
        let mut writer_finished = false;
        let result = tokio::select! {
            result = &mut connection => result,
            _ = &mut persistence => { writer_finished = true; connection.await },
        };
        stop_preferences.cancel();
        if !writer_finished {
            persistence.await;
        }
        result
    };
    state.send_modify(|s| s.status = "Stopping synchronization…".into());
    let stopped = crate::sso::stop_with_deadline(sync.stop(), Duration::from_secs(15)).await;
    // Keep the profile guard alive until SDK stop has completed or failed.
    drop(sync);
    drop(restored);
    outcome.and(stopped)
}

async fn build_sync(client: Client) -> anyhow::Result<SyncService> {
    Ok(SyncService::builder(client)
        .with_offline_mode()
        .build()
        .await?)
}

fn connection_status(sync: &State, rooms: &RoomState) -> &'static str {
    match sync {
        State::Offline => {
            "Offline — reconnecting automatically. You can still compose and queue messages."
        }
        State::Running
            if matches!(
                rooms,
                RoomState::SettingUp | RoomState::Running | RoomState::Recovering
            ) =>
        {
            "Connected"
        }
        State::Running | State::Idle => "Connecting… Messages and drafts are retained.",
        State::Error(_) | State::Terminated => "Synchronization interrupted. Session retained.",
    }
}

async fn connected(
    client: &Client,
    sync: &SyncService,
    selection: &mut watch::Receiver<Selection>,
    state: &watch::Sender<Model>,
) -> anyhow::Result<()> {
    let navigation = navigation_updates(client, state);
    tokio::pin!(navigation);
    let mut sync_states = sync.state();
    let mut room_states = sync.room_list_service().state();
    let mut session_changes = client.subscribe_to_session_changes();
    if !selection.borrow().shutdown {
        sync.start().await;
    }
    let mut active = Selection::default();
    let mut room_view = inactive();
    let mut thread_view = inactive();
    let result = async {
        loop {
            let next = selection.borrow_and_update().clone();
            if next.shutdown { break; }
            let status = connection_status(&sync_states.get(), &room_states.get());
            state.send_if_modified(|s| {
                if s.status == status { false } else { s.status = status.into(); true }
            });
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
                _ = &mut navigation => anyhow::bail!("Navigation update stream ended"),
                _ = &mut room_view => room_view = inactive(),
                _ = &mut thread_view => thread_view = inactive(),
                event = room_states.next() => {
                    if event.is_none() { anyhow::bail!("room-list stream ended"); }

                }
                event = session_changes.recv() => match event {
                    Ok(matrix_sdk::SessionChange::UnknownToken(_)) => anyhow::bail!("The server rejected this session. Sign-in is required; saved credentials retained."),
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {},
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => anyhow::bail!("Session notification stream ended"),
                },
                event = sync_states.next() => match event {
                    Some(State::Error(error)) => anyhow::bail!("Synchronization service stopped: {error}. Session retained."),
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

async fn navigation_updates(client: &Client, state: &watch::Sender<Model>) {
    // Subscribe before reading the cache so startup changes cannot be missed.
    let mut rooms = client.subscribe_to_all_room_updates();
    let mut metadata = client.room_info_notable_update_receiver();
    loop {
        match crate::navigation::snapshot(client).await {
            Ok((rooms, spaces)) => {
                state.send_if_modified(|s| {
                    if *s.rooms == rooms && *s.spaces == spaces && s.navigation_error.is_none() {
                        return false;
                    }
                    s.rooms = Arc::new(rooms);
                    s.spaces = Arc::new(spaces);
                    s.navigation_error = None;
                    true
                });
            }
            Err(_) => {
                state.send_if_modified(|s| {
                    let error =
                        "Could not refresh room navigation. Retrying on the next account update.";
                    if s.navigation_error.as_deref() == Some(error) {
                        return false;
                    }
                    s.navigation_error = Some(error.into());
                    true
                });
            }
        }
        tokio::select! {
            event = rooms.recv() => if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { return; },
            event = metadata.recv() => if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { return; },
        }
        // Superseded notifications need one fresh snapshot, not one DB scan each.
        while rooms.try_recv().is_ok() {}
        while metadata.try_recv().is_ok() {}
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
    let diagnostics = state.borrow().diagnostics.clone();
    let _lifetime = diagnostics.view(root.is_some());
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
                    // The UI coalesces replies at their latest position. Keep
                    // them even when their root is outside the loaded history.
                    hide_threaded_events: false,
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
        let timeline = Arc::new(timeline);
        diagnostics.timeline(&timeline);
        let (_, updates) = timeline.subscribe().await;
        let mut updates: BoxStream<'_, _> = Box::pin(updates);
        let (load_older, mut requests) = mpsc::channel(1);
        let mut history = History::Ready;
        let mut revision = 0;
        // Typical threads load completely; long threads switch to viewport demand.
        for _ in 0..if is_thread { INITIAL_THREAD_PAGES } else { 1 } {
            revision += 1;
            history = load_history(
                &timeline, generation, is_thread, revision, &mut updates, &state, &load_older,
            ).await?;
            let loaded_events = timeline.items().await.iter()
                .filter(|item| item.as_event().is_some()).count();
            if !matches!(history, History::Ready)
                || loaded_events >= INITIAL_THREAD_EVENTS
            {
                break;
            }
        }
        publish(&timeline, generation, is_thread, revision, &history, &state, &load_older).await;
        loop {
            tokio::select! {
                update = updates.next() => {
                    ensure!(update.is_some(), "Timeline subscription ended");
                }
                Some(requested_revision) = requests.recv() => {
                    if requested_revision == revision && !matches!(history, History::End) {
                        revision += 1;
                        history = load_history(&timeline, generation, is_thread, revision, &mut updates, &state, &load_older).await?;
                    }
                }
            }
            publish(&timeline, generation, is_thread, revision, &history, &state, &load_older).await;
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
                history_revision: 0,
                load_older: None,
                composer: None,
            });
        });
    }
}

// Owned by the view: room switches and shutdown cancel an in-flight page.
// Live snapshots continue during each fetch, including retries.
async fn load_history(
    timeline: &Arc<Timeline>,
    generation: u64,
    thread: bool,
    revision: u64,
    updates: &mut (impl iced::futures::Stream + Unpin),
    state: &watch::Sender<Model>,
    load_older: &mpsc::Sender<u64>,
) -> anyhow::Result<History> {
    publish(
        timeline,
        generation,
        thread,
        revision,
        &History::Loading,
        state,
        load_older,
    )
    .await;
    let page = tokio::time::timeout(
        Duration::from_secs(30),
        timeline.paginate_backwards(HISTORY_PAGE_SIZE),
    );
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
                publish(timeline, generation, thread, revision, &History::Loading, state, load_older).await;
            }
        }
    }
}

async fn publish(
    timeline: &Arc<Timeline>,
    generation: u64,
    thread: bool,
    revision: u64,
    history: &History,
    state: &watch::Sender<Model>,
    load_older: &mpsc::Sender<u64>,
) {
    let items = timeline.items().await;
    let rows = items
        .iter()
        .filter_map(|item| item.as_event())
        .map(|event| {
            let content = event.content();
            MessageRow {
                key: match event.identifier() {
                    TimelineEventItemId::EventId(id) => id.to_string(),
                    TimelineEventItemId::TransactionId(id) => format!("local:{id}"),
                },
                kind: RowKind::from_content(content),
                event_id: event.event_id().map(ToString::to_string),
                sender: event.sender().to_string(),
                display_name: match event.sender_profile() {
                    matrix_sdk_ui::timeline::TimelineDetails::Ready(profile) => profile
                        .display_name
                        .as_ref()
                        .filter(|name| !name.trim().is_empty())
                        .map(|name| {
                            if profile.display_name_ambiguous {
                                format!("{name} ({})", event.sender())
                            } else {
                                name.clone()
                            }
                        })
                        .unwrap_or_else(|| event.sender().to_string()),
                    _ => event.sender().to_string(),
                },
                timestamp_ms: i64::try_from(u64::from(event.timestamp().get())).ok(),
                thread_replies: content
                    .thread_summary()
                    .map_or(0, |summary| summary.num_replies),
                thread_root: content.thread_root().map(|id| id.to_string()),
                is_own: event.is_own(),
                is_live: matches!(event.origin(), Some(EventItemOrigin::Sync)),
                body: crate::event_text::body(content, event.sender()),
                send_state: match event.send_state() {
                    Some(EventSendState::NotSentYet { .. }) => "Pending".into(),
                    Some(EventSendState::SendingFailed { error, .. }) => format!("Failed: {error}"),
                    Some(EventSendState::Sent { .. }) => "Sent".into(),
                    None if event.sender() == timeline.room().own_user_id() => "Sent".into(),
                    None => String::new(),
                },
                retry: if matches!(
                    event.send_state(),
                    Some(EventSendState::SendingFailed { .. })
                ) {
                    event
                        .local_echo_send_handle()
                        .zip(event.transaction_id())
                        .map(|(handle, id)| RetrySend {
                            transaction: id.to_string(),
                            state: RuntimeOwned::new(RetryState {
                                handle,
                                queue: timeline.room().send_queue(),
                            }),
                        })
                } else {
                    None
                },
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
            history_revision: revision,
            composer: Some(Composer(RuntimeOwned::new(timeline.clone()))),
            load_older: matches!(history, History::Ready | History::Failed(_))
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

    #[test]
    fn final_ui_handle_drop_enters_runtime_and_drains_blocking_cleanup() {
        struct NeedsRuntime(std::sync::mpsc::Sender<()>);
        impl Drop for NeedsRuntime {
            fn drop(&mut self) {
                let done = self.0.clone();
                drop(tokio::task::spawn_blocking(move || done.send(()).unwrap()));
            }
        }
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let first = runtime.block_on(async { RuntimeOwned::new(NeedsRuntime(done)) });
        let queued_message = first.clone();
        assert!(tokio::runtime::Handle::try_current().is_err());
        drop(first);
        assert!(finished.try_recv().is_err());
        // Models, queued messages, and widget trees may release their last clone
        // on arbitrary non-runtime threads.
        std::thread::spawn(move || drop(queued_message))
            .join()
            .unwrap();
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(tokio::runtime::Handle::try_current().is_err());
        crate::shutdown_runtime(runtime, Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn sqlite_client_final_drop_outside_executor_is_safe() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let client = runtime.block_on(async {
            let client = Client::builder()
                .cross_process_store_config(
                    matrix_sdk_common::cross_process_lock::CrossProcessLockConfig::SingleProcess,
                )
                .homeserver_url("http://127.0.0.1:1")
                .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
                .sqlite_store(directory.path(), Some("test-only-passphrase"))
                .build()
                .await
                .unwrap();
            RuntimeOwned::new(client)
        });
        assert!(tokio::runtime::Handle::try_current().is_err());
        drop(client);
        crate::shutdown_runtime(runtime, Duration::from_secs(5)).unwrap();
    }

    #[tokio::test]
    async fn live_thread_reply_is_projected_without_a_loaded_root_and_can_open_its_thread() {
        use matrix_sdk::{config::SyncSettings, ruma::event_id};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let response = Arc::new(std::sync::Mutex::new(
            json!({"next_batch":"one", "rooms":{"join":{"!room:example.org":{
                "state":{"events":[
                    {"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":1,"content":{"membership":"join"}},
                    {"type":"m.room.encryption","state_key":"","sender":"@test:example.org","event_id":"$encryption","origin_server_ts":1,"content":{"algorithm":"m.megolm.v1.aes-sha2"}}
                ]}, "timeline":{"events":[],"limited":false}
            }}}}),
        ));
        let body = response.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(body.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        let root = json!({"type":"m.room.message","event_id":"$outside","room_id":"!room:example.org","sender":"@other:example.org","origin_server_ts":1,"content":{"msgtype":"m.text","body":"Older thread original"}});
        Mock::given(method("GET"))
            .and(path_regex(".*/event/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(root))
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
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap();
        let timeline = Arc::new(
            TimelineBuilder::new(&room)
                .with_focus(TimelineFocus::Live {
                    hide_threaded_events: false,
                })
                .build()
                .await
                .unwrap(),
        );
        let (_, updates) = timeline.subscribe().await;
        tokio::pin!(updates);
        let reply = json!({"type":"m.room.message","event_id":"$reply","sender":"@other:example.org","origin_server_ts":2,
            "content":{"msgtype":"m.text","body":"Fresh thread reply","m.relates_to":{"rel_type":"m.thread","event_id":"$outside","is_falling_back":false}}});
        Mock::given(method("GET"))
            .and(path_regex(".*/relations/.*"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"chunk":[reply.clone()]})),
            )
            .mount(&server)
            .await;
        *response.lock().unwrap() = json!({"next_batch":"two", "rooms":{"join":{"!room:example.org":{"timeline":{"events":[reply],"limited":false}}}}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if timeline.items().await.iter().any(|item| {
                    item.as_event()
                        .is_some_and(|event| event.event_id() == Some(event_id!("$reply")))
                }) {
                    break;
                }
                updates.next().await.unwrap();
            }
        })
        .await
        .unwrap();
        let (state, snapshots) = watch::channel(Model::default());
        let (older, _) = mpsc::channel(1);
        publish(&timeline, 1, false, 1, &History::Ready, &state, &older).await;
        let pane = snapshots.borrow().room.clone().unwrap();
        assert!(
            !pane
                .rows
                .iter()
                .any(|row| row.event_id.as_deref() == Some("$outside"))
        );
        let reply = pane.rows.iter().find(|row| row.key == "$reply").unwrap();
        assert_eq!(reply.thread_root.as_deref(), Some("$outside"));
        assert!(reply.is_live && !reply.is_own);
        assert_eq!(reply.body, "Fresh thread reply");
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path().contains("/event/")),
            "projecting activity must not fetch the old original"
        );
        let thread = TimelineBuilder::new(&room)
            .with_focus(TimelineFocus::Thread {
                root_event_id: event_id!("$outside").to_owned(),
            })
            .build()
            .await
            .unwrap();
        thread.paginate_backwards(20).await.unwrap();
        assert!(thread.items().await.iter().any(|item| {
            item.as_event()
                .is_some_and(|event| event.event_id() == Some(event_id!("$outside")))
        }));
    }

    #[tokio::test]
    async fn navigation_tracks_sdk_state_and_account_data_without_timelines() {
        use matrix_sdk::config::SyncSettings;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
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
        let event = |kind: &str, key: &str, content: serde_json::Value| json!({"type":kind,"state_key":key,"sender":"@test:example.org","event_id":format!("${kind}-{key}"),"origin_server_ts":1,"content":content});
        let room = |name: &str, space: bool, mut extra: Vec<serde_json::Value>| {
            extra.extend([
                event(
                    "m.room.create",
                    "",
                    if space {
                        json!({"creator":"@test:example.org","type":"m.space"})
                    } else {
                        json!({"creator":"@test:example.org"})
                    },
                ),
                event(
                    "m.room.member",
                    "@test:example.org",
                    json!({"membership":"join"}),
                ),
                event("m.room.name", "", json!({"name":name})),
            ]);
            json!({"state":{"events":extra},"timeline":{"events":[],"limited":false},"account_data":{"events":[]}})
        };
        let mut conversation = room("Conversation", false, vec![]);
        conversation["account_data"] = json!({"events":[{"type":"m.tag","content":{"tags":{"m.favourite":{}}}},{"type":"m.marked_unread","content":{"unread":true}}]});
        let first = json!({"next_batch":"one","rooms":{"join":{
            "!space:example.org": room("Space",true,vec![event("m.space.child","!nested:example.org",json!({"via":["example.org"]}))]),
            "!nested:example.org": room("Nested",true,vec![event("m.space.child","!room:example.org",json!({"via":["example.org"]}))]),
            "!other:example.org": room("Other",true,vec![event("m.space.child","!room:example.org",json!({"via":["example.org"]}))]),
            "!room:example.org": conversation
        }},"account_data":{"events":[{"type":"m.direct","content":{"@peer:example.org":["!room:example.org"]}}]}});
        let response = Arc::new(std::sync::Mutex::new(first));
        let body = response.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(body.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        let (state, mut snapshots) = watch::channel(Model::default());
        let watched = client.clone();
        let task = tokio::spawn(async move { navigation_updates(&watched, &state).await });
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|s| {
                s.spaces.len() == 3
                    && s.rooms.len() == 1
                    && s.rooms[0].favorite
                    && s.rooms[0].direct
                    && s.rooms[0].unread
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        let members = crate::navigation::members(
            &crate::navigation::Scope::Space("!space:example.org".into()),
            &first.spaces,
        )
        .unwrap();
        assert!(members.contains("!room:example.org"));
        assert!(first.room.is_none() && first.thread.is_none());
        // Global and room account-data updates must work without a room-list lifecycle transition.
        *response.lock().unwrap() = json!({"next_batch":"two","account_data":{"events":[{"type":"m.direct","content":{}}]},"rooms":{"join":{
            "!room:example.org":{"state":{"events":[event("m.room.name","",json!({"name":"Renamed"}))]},"account_data":{"events":[{"type":"m.tag","content":{"tags":{}}},{"type":"m.marked_unread","content":{"unread":false}}]}},
            "!nested:example.org":{"state":{"events":[event("m.space.child","!room:example.org",json!({}))]}}
        }}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let updated = tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|s| {
                s.rooms.len() == 1
                    && s.rooms[0].name == "Renamed"
                    && !s.rooms[0].favorite
                    && !s.rooms[0].direct
                    && !s.rooms[0].unread
                    && s.spaces
                        .iter()
                        .any(|s| s.id == "!nested:example.org" && s.children.is_empty())
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        assert!(
            !crate::navigation::members(
                &crate::navigation::Scope::Space("!space:example.org".into()),
                &updated.spaces
            )
            .unwrap()
            .contains("!room:example.org")
        );
        assert!(
            crate::navigation::members(
                &crate::navigation::Scope::Space("!other:example.org".into()),
                &updated.spaces
            )
            .unwrap()
            .contains("!room:example.org")
        );
        *response.lock().unwrap() = json!({"next_batch":"three","rooms":{"leave":{"!room:example.org":{"timeline":{"events":[event("m.room.member","@test:example.org",json!({"membership":"leave"}))]}}}}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|s| s.rooms.is_empty()),
        )
        .await
        .unwrap()
        .unwrap();
        task.abort();
        let _ = task.await;
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|r| r.url.path().contains("/messages"))
        );
    }

    #[tokio::test]
    async fn sdk_views_switch_and_shutdown_without_ui_consumption() {
        history_fixture(false, false, 3, 3).await;
    }

    #[tokio::test]
    async fn long_threads_stop_eager_loading_and_continue_on_demand() {
        history_fixture(false, false, 1, 12).await;
    }

    #[tokio::test]
    async fn shutdown_cancels_pending_initial_history() {
        history_fixture(true, false, 3, 3).await;
    }

    #[tokio::test]
    async fn offline_views_recover_and_can_send_without_restarting_ui() {
        history_fixture(false, true, 3, 3).await;
    }

    #[tokio::test]
    async fn revoked_session_is_not_treated_as_a_network_outage() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"],"unstable_features":{"org.matrix.simplified_msc3575":true}})))
            .mount(&server).await;
        Mock::given(method("POST"))
            .and(path_regex(".*/keys/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"device_keys":{},"one_time_key_counts":{"signed_curve25519":50}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({"errcode":"M_UNKNOWN_TOKEN","error":"revoked fixture session","soft_logout":false})))
            .mount(&server).await;
        let client = Client::builder()
            .cross_process_store_config(
                matrix_sdk_common::cross_process_lock::CrossProcessLockConfig::SingleProcess,
            )
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
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
        let sync = build_sync(client.clone()).await.unwrap();
        let (_control, mut selection) = watch::channel(Selection::default());
        let (state, _snapshots) = watch::channel(Model::default());
        let error = tokio::time::timeout(
            Duration::from_secs(10),
            connected(&client, &sync, &mut selection, &state),
        )
        .await
        .unwrap()
        .unwrap_err();
        crate::sso::stop_with_deadline(sync.stop(), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(error.to_string().contains("Sign-in is required"));
    }

    async fn request_page(snapshots: &mut watch::Receiver<Model>, thread: bool) -> Pane {
        let pane = if thread {
            snapshots.borrow().thread.clone()
        } else {
            snapshots.borrow().room.clone()
        }
        .unwrap();
        let revision = pane.history_revision;
        pane.load_older.unwrap().try_send(revision).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                snapshots.changed().await.unwrap();
                let pane = if thread {
                    snapshots.borrow_and_update().thread.clone()
                } else {
                    snapshots.borrow_and_update().room.clone()
                }
                .unwrap();
                if pane.history_revision > revision && !matches!(pane.history, History::Loading) {
                    return pane;
                }
            }
        })
        .await
        .unwrap()
    }

    async fn history_fixture(
        cancel_history: bool,
        reconnect: bool,
        cycles: u64,
        thread_pages: usize,
    ) {
        let server = MockServer::start().await;
        let online = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let versions_online = online.clone();
        let sync_online = online.clone();
        let root = json!({"type":"m.room.message", "event_id":"$root", "sender":"@test:example.org", "origin_server_ts":1, "content":{"msgtype":"m.text", "body":"fixture root"}});
        Mock::given(method("GET")).and(path("/_matrix/client/versions"))
            .respond_with(move |_: &wiremock::Request| {
                if versions_online.load(std::sync::atomic::Ordering::SeqCst) {
                    ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"], "unstable_features":{"org.matrix.simplified_msc3575":true}}))
                } else { ResponseTemplate::new(503).set_body_json(json!({"errcode":"M_UNKNOWN", "error":"offline fixture"})) }
            }).mount(&server).await;
        Mock::given(method("POST")).and(path("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"))
            .respond_with(move |_: &wiremock::Request| {
                if !sync_online.load(std::sync::atomic::Ordering::SeqCst) {
                    return ResponseTemplate::new(503).set_body_json(json!({"errcode":"M_UNKNOWN", "error":"offline fixture"}));
                }
                ResponseTemplate::new(200).set_delay(Duration::from_millis(50)).set_body_json(json!({
                "pos":"test-pos", "lists":{"all_rooms":{"count":1,"ops":[{"op":"SYNC","range":[0,0],"room_ids":["!room:example.org"]}]}},
                "rooms":{"!room:example.org":{"name":"Fixture", "initial":true, "is_dm":false, "joined_count":1, "invited_count":0,
                    "required_state":[
                        {"type":"m.room.member", "state_key":"@test:example.org", "sender":"@test:example.org", "event_id":"$member", "origin_server_ts":0,"content":{"membership":"join"}},
                        {"type":"m.room.encryption","state_key":"","sender":"@test:example.org","event_id":"$encryption","origin_server_ts":0,"content":{"algorithm":"m.megolm.v1.aes-sha2"}}
                    ], "timeline":[],"prev_batch":"older","num_live":0}},
                "extensions":{"e2ee":{"device_one_time_keys_count":{"signed_curve25519":50},"device_unused_fallback_key_types":["signed_curve25519"]}}
            })) }).mount(&server).await;
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
        let relation_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let relation_count = relation_calls.clone();
        let replies_per_page = if thread_pages > 10 { 50 } else { 5 };
        Mock::given(method("GET"))
            .and(path_regex(".*/relations/.*"))
            .respond_with(move |request: &wiremock::Request| {
                relation_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let page = request.url.query_pairs().find(|(key, _)| key == "from")
                    .map_or(0, |(_, value)| value.parse::<usize>().unwrap());
                let chunk: Vec<_> = (0..replies_per_page).map(|i| {
                    let index = page * replies_per_page + i;
                    json!({"type":"m.room.message", "event_id":format!("$thread-{index}"), "sender":"@test:example.org", "origin_server_ts":(thread_pages * replies_per_page - index + 10), "content":{"msgtype":"m.text", "body":format!("thread history {index}"), "m.relates_to":{"rel_type":"m.thread","event_id":"$root","is_falling_back":true,"m.in_reply_to":{"event_id":"$root"}}}})
                }).collect();
                let mut response = json!({"chunk":chunk});
                if page + 1 < thread_pages {
                    response["next_batch"] = json!((page + 1).to_string());
                }
                ResponseTemplate::new(200).set_body_json(response)
            })
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path_regex(".*/members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[{"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":0,"content":{"membership":"join"}}]})))
            .mount(&server).await;
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let send_attempts = attempts.clone();
        let sends_online = online.clone();
        Mock::given(method("PUT"))
            .and(path_regex(".*/send/m.room.encrypted/.*"))
            .respond_with(move |_: &wiremock::Request| {
                if !sends_online.load(std::sync::atomic::Ordering::SeqCst) {
                    return ResponseTemplate::new(503)
                        .set_body_json(json!({"errcode":"M_UNKNOWN", "error":"offline fixture"}));
                }
                let attempt = send_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if attempt == 0 {
                    ResponseTemplate::new(403)
                        .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"test send failure"}))
                } else {
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"event_id":format!("$sent{attempt}")}))
                }
            })
            .mount(&server)
            .await;
        let client = Client::builder()
            .cross_process_store_config(
                matrix_sdk_common::cross_process_lock::CrossProcessLockConfig::SingleProcess,
            )
            .homeserver_url(server.uri())
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
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
        let sync = build_sync(client.clone()).await.unwrap();
        crate::sso::start_and_wait(&sync).await.unwrap();
        let (navigation, mut selections) = watch::channel(Selection::default());
        let diagnostics = Arc::new(crate::diagnostics::Diagnostics::new(true));
        let (state, mut snapshots) = watch::channel(Model {
            diagnostics: diagnostics.clone(),
            ..Model::default()
        });
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
        for generation in 1..=cycles {
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
                let page = request_page(&mut snapshots, false).await;
                assert!(matches!(page.history, History::Ready));
                let reply = page.rows.iter().find(|r| r.body == "hidden reply").unwrap();
                assert_eq!(reply.thread_root.as_deref(), Some("$root"));
                assert!(!reply.is_live, "paginated replies must not count as new");
                assert!(reply.is_own);
                assert!(
                    page.load_older.is_some(),
                    "a reply-only page must allow further pagination"
                );
                let page = request_page(&mut snapshots, false).await;
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
            let thread_pane = model.thread.unwrap();
            assert!(thread_pane.error.is_none());
            if generation == 1 {
                assert_eq!(
                    relation_calls.load(std::sync::atomic::Ordering::SeqCst),
                    thread_pages.min(10)
                );
                if thread_pages > 10 {
                    assert!(matches!(thread_pane.history, History::Ready));
                    let page = request_page(&mut snapshots, true).await;
                    assert!(matches!(page.history, History::Ready));
                    let page = request_page(&mut snapshots, true).await;
                    assert!(matches!(page.history, History::End));
                } else {
                    assert!(matches!(thread_pane.history, History::End));
                }
                tokio::time::timeout(
                    Duration::from_secs(5),
                    snapshots.wait_for(|s| {
                        s.thread.as_ref().is_some_and(|p| {
                            p.rows
                                .iter()
                                .filter(|r| r.body.starts_with("thread history "))
                                .count()
                                == thread_pages * replies_per_page
                        })
                    }),
                )
                .await
                .unwrap()
                .unwrap();
                let room_composer = snapshots
                    .borrow()
                    .room
                    .as_ref()
                    .unwrap()
                    .composer
                    .clone()
                    .unwrap();
                room_composer
                    .send("room send fixture".into())
                    .await
                    .unwrap();
                let failed = tokio::time::timeout(
                    Duration::from_secs(10),
                    snapshots.wait_for(|s| {
                        s.room.as_ref().is_some_and(|p| {
                            p.rows
                                .iter()
                                .any(|r| r.body == "room send fixture" && r.retry.is_some())
                        })
                    }),
                )
                .await
                .unwrap()
                .unwrap()
                .clone();
                let retry = failed
                    .room
                    .unwrap()
                    .rows
                    .iter()
                    .find_map(|r| r.retry.clone())
                    .unwrap();
                retry.retry().await.unwrap();
                tokio::time::timeout(
                    Duration::from_secs(10),
                    snapshots.wait_for(|s| {
                        s.room.as_ref().is_some_and(|p| {
                            p.rows
                                .iter()
                                .any(|r| r.body == "room send fixture" && r.send_state == "Sent")
                        })
                    }),
                )
                .await
                .unwrap()
                .unwrap();
                let thread_composer = thread_pane.composer.clone().unwrap();
                let decrypt_room = thread_composer.0.room().clone();
                thread_composer
                    .send("thread send fixture".into())
                    .await
                    .unwrap();
                tokio::time::timeout(
                    Duration::from_secs(10),
                    snapshots.wait_for(|s| {
                        s.thread.as_ref().is_some_and(|p| {
                            p.rows
                                .iter()
                                .any(|r| r.body == "thread send fixture" && r.send_state == "Sent")
                        })
                    }),
                )
                .await
                .unwrap()
                .unwrap();
                let requests = server.received_requests().await.unwrap();
                let sends: Vec<_> = requests
                    .iter()
                    .filter(|r| r.url.path().contains("/send/m.room.encrypted/"))
                    .collect();
                assert_eq!(
                    sends.len(),
                    3,
                    "failed send, same-transaction retry, then thread send"
                );
                assert_eq!(sends[0].url.path(), sends[1].url.path());
                for (request, threaded) in [(sends[1], false), (sends[2], true)] {
                    let content: serde_json::Value = request.body_json().unwrap();
                    assert!(
                        content.get("body").is_none(),
                        "message must be encrypted on the wire"
                    );
                    let event = matrix_sdk::ruma::serde::Raw::from_json_string(json!({"type":"m.room.encrypted", "event_id":if threaded {"$sent2"} else {"$sent1"}, "sender":"@test:example.org", "origin_server_ts":2, "content":content}).to_string()).unwrap();
                    let decrypted = decrypt_room.decrypt_event(&event, None).await.unwrap();
                    let json: serde_json::Value =
                        serde_json::from_str(decrypted.raw().json().get()).unwrap();
                    assert_eq!(
                        json["content"]["body"],
                        if threaded {
                            "thread send fixture"
                        } else {
                            "room send fixture"
                        }
                    );
                    if threaded {
                        assert_eq!(json["content"]["m.relates_to"]["rel_type"], "m.thread");
                        assert_eq!(json["content"]["m.relates_to"]["event_id"], "$root");
                    } else {
                        assert!(json["content"].get("m.relates_to").is_none());
                    }
                }
            }
            drop(thread_pane);
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
            assert_eq!(diagnostics.snapshot()["active_views"], json!([1, 0]));
            assert_eq!(diagnostics.snapshot()["peak_views"], json!([1, 1]));
            // Old GUI snapshots can retain timelines; release our snapshots before checking.
            drop(pane);
            assert_eq!(diagnostics.snapshot()["live_timelines"], 1);
        }
        if reconnect {
            online.store(false, std::sync::atomic::Ordering::SeqCst);
            let offline = tokio::time::timeout(
                Duration::from_secs(15),
                snapshots.wait_for(|s| s.status.starts_with("Offline")),
            )
            .await
            .unwrap()
            .unwrap()
            .clone();
            assert!(
                !worker.is_finished(),
                "offline must not terminate the UI backend"
            );
            let pane = offline.room.unwrap();
            assert_eq!(pane.generation, 3);
            pane.composer
                .unwrap()
                .send("queued during outage".into())
                .await
                .unwrap();
            let failed = tokio::time::timeout(
                Duration::from_secs(20),
                snapshots.wait_for(|s| {
                    s.room.as_ref().is_some_and(|p| {
                        p.rows
                            .iter()
                            .any(|r| r.body == "queued during outage" && r.retry.is_some())
                    })
                }),
            )
            .await
            .unwrap()
            .unwrap()
            .clone();
            let retry = failed
                .room
                .unwrap()
                .rows
                .iter()
                .find(|r| r.body == "queued during outage")
                .unwrap()
                .retry
                .clone()
                .unwrap();
            online.store(true, std::sync::atomic::Ordering::SeqCst);
            tokio::time::timeout(
                Duration::from_secs(20),
                snapshots.wait_for(|s| s.status == "Connected"),
            )
            .await
            .unwrap()
            .unwrap();
            retry.retry().await.unwrap();
            tokio::time::timeout(
                Duration::from_secs(15),
                snapshots.wait_for(|s| {
                    s.room.as_ref().is_some_and(|p| {
                        p.rows
                            .iter()
                            .any(|r| r.body == "queued during outage" && r.send_state == "Sent")
                    })
                }),
            )
            .await
            .unwrap()
            .unwrap();
            let composer = snapshots
                .borrow()
                .room
                .as_ref()
                .unwrap()
                .composer
                .clone()
                .unwrap();
            composer.send("after reconnect".into()).await.unwrap();
            tokio::time::timeout(
                Duration::from_secs(15),
                snapshots.wait_for(|s| {
                    s.room.as_ref().is_some_and(|p| {
                        p.rows
                            .iter()
                            .any(|r| r.body == "after reconnect" && r.send_state == "Sent")
                    })
                }),
            )
            .await
            .unwrap()
            .unwrap();
            // A second outage verifies shutdown interrupts SDK offline probes.
            online.store(false, std::sync::atomic::Ordering::SeqCst);
            tokio::time::timeout(
                Duration::from_secs(15),
                snapshots.wait_for(|s| s.status.starts_with("Offline")),
            )
            .await
            .unwrap()
            .unwrap();
        }
        // A UI that stops draining snapshots cannot block navigation or shutdown.
        navigation.send_modify(|s| s.shutdown = true);
        tokio::time::timeout(Duration::from_secs(6), worker)
            .await
            .unwrap()
            .unwrap();
        drop(snapshots);
        assert_eq!(diagnostics.snapshot()["active_views"], json!([0, 0]));
        assert_eq!(diagnostics.snapshot()["live_timelines"], 0);
        assert_eq!(
            diagnostics.snapshot()["view_starts"],
            json!([cycles, cycles])
        );
    }

    #[tokio::test]
    #[ignore = "100-cycle SDK lifecycle stress check; run explicitly"]
    async fn repeated_navigation_releases_timelines() {
        history_fixture(false, false, 100, 1).await;
    }
}
