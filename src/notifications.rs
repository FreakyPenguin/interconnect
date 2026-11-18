//! Desktop delivery is independent of replaceable UI snapshots and timeline views.
//! Only SDK push-rule notifications enter this bounded, session-owned worker.
use std::{
    collections::{HashSet, VecDeque},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use iced::futures::future::select_all;
use matrix_sdk::{
    Client, Room, RoomState, deserialized_responses::RawAnySyncOrStrippedTimelineEvent,
    sync::Notification,
};
use serde::Deserialize;
use std::{
    pin::Pin,
    sync::{Arc, Weak},
};
use tokio::sync::{mpsc, watch};

use crate::connected::{Model, Selection};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "macos", test))]
mod responses;

const QUEUE_LIMIT: usize = 64;
const SEEN_LIMIT: usize = 2048;
const MAX_AGE_MS: u64 = 120_000;

/// Conversation identity retained locally, never included in the OS payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub room: String,
    pub thread: Option<String>,
}

/// A click is valid only while its authenticated worker is alive.
#[derive(Debug)]
pub struct Activation {
    pub target: Target,
    session: Weak<()>,
}
impl Activation {
    /// The caller retains the session lease; dropping it invalidates this click.
    pub fn new(target: Target, session: &Arc<()>) -> Self {
        Self {
            target,
            session: Arc::downgrade(session),
        }
    }

    pub fn is_active(&self) -> bool {
        self.session.strong_count() > 0
    }
}

type PendingResponse = Pin<Box<dyn Future<Output = (bool, Target)> + Send>>;
type Response = Pin<Box<dyn Future<Output = bool> + Send>>;

// Never derive Debug: event previews must not appear in diagnostics.
struct Candidate {
    room: String,
    room_name: String,
    id: String,
    sender: String,
    timestamp: u64,
    thread: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    event_id: String,
    sender: String,
    origin_server_ts: u64,
    content: serde_json::Value,
    #[serde(default)]
    unsigned: serde_json::Value,
}

impl Candidate {
    fn from_event(notification: Notification, room: &Room) -> Option<Self> {
        if room.state() != RoomState::Joined
            || !notification
                .actions
                .iter()
                .any(|action| action.should_notify())
        {
            return None;
        }
        let RawAnySyncOrStrippedTimelineEvent::Sync(raw) = notification.event else {
            return None;
        };
        let event: Event = serde_json::from_str(raw.json().get()).ok()?;
        Self::parse(
            event,
            room.room_id().as_str(),
            room.name().as_deref().unwrap_or("Matrix room"),
            room.own_user_id().as_str(),
        )
    }

    fn parse(event: Event, room: &str, room_name: &str, own_user: &str) -> Option<Self> {
        if event.sender == own_user
            || event.unsigned.get("redacted_because").is_some()
            || event
                .content
                .pointer("/m.relates_to/rel_type")
                .and_then(|v| v.as_str())
                == Some("m.replace")
        {
            return None;
        }
        let body = match event.kind.as_str() {
            "m.room.message" => plain(event.content.get("body")?.as_str()?, 300),
            "m.room.encrypted" => "Encrypted message".into(),
            "m.sticker" => "Sticker".into(),
            _ => return None,
        };
        let thread = (event
            .content
            .pointer("/m.relates_to/rel_type")
            .and_then(|v| v.as_str())
            == Some("m.thread"))
        .then(|| {
            event
                .content
                .pointer("/m.relates_to/event_id")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .flatten();
        Some(Self {
            room: room.into(),
            room_name: plain(room_name, 100),
            id: event.event_id,
            sender: event.sender,
            timestamp: event.origin_server_ts,
            thread,
            body,
        })
    }

    fn eligible(&self, selection: &Selection, started: u64, now: u64) -> bool {
        if selection.shutdown
            || !selection.notifications.enabled
            || self.timestamp < started
            || now.saturating_sub(self.timestamp) > MAX_AGE_MS
            || self.timestamp > now.saturating_add(MAX_AGE_MS)
        {
            return false;
        }
        if !selection.conversation_visible
            || selection
                .room
                .as_ref()
                .is_none_or(|(_, room)| room != &self.room)
        {
            return true;
        }
        // Room timelines hide thread replies; only an open matching thread
        // suppresses those. Both the room and thread panes can be visible.
        self.thread.as_ref().is_some_and(|root| {
            selection
                .thread
                .as_ref()
                .is_none_or(|(_, open)| open != root)
        })
    }

    fn preview(&self, show: bool) -> (String, String) {
        if show {
            (
                plain(&self.room_name, 100),
                plain(&format!("{}: {}", self.sender, self.body), 300),
            )
        } else {
            ("Interconnect".into(), "New Matrix message".into())
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn plain(value: &str, limit: usize) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(limit)
        .collect()
}

#[derive(Default)]
struct Seen {
    ids: HashSet<(String, String)>,
    order: VecDeque<(String, String)>,
}
impl Seen {
    fn insert(&mut self, candidate: &Candidate) -> bool {
        let key = (candidate.room.clone(), candidate.id.clone());
        if !self.ids.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        if self.order.len() > SEEN_LIMIT {
            self.ids
                .remove(&self.order.pop_front().expect("bounded queue"));
        }
        true
    }
}

pub(crate) struct Service {
    events: mpsc::Receiver<Candidate>,
    selection: watch::Receiver<Selection>,
    state: watch::Sender<Model>,
    started: u64,
}
impl Service {
    pub(crate) async fn install(
        client: &Client,
        selection: watch::Receiver<Selection>,
        state: watch::Sender<Model>,
    ) -> Self {
        let started = now_ms();
        let (sender, events) = mpsc::channel(QUEUE_LIMIT);
        client
            .register_notification_handler(move |notification, room, _| {
                let sender = sender.clone();
                async move {
                    if let Some(candidate) = Candidate::from_event(notification, &room) {
                        // A slow or absent desktop must never hold up Matrix sync.
                        // Burst overflow drops alerts rather than accumulating work.
                        let _ = sender.try_send(candidate);
                    }
                }
            })
            .await;
        Self {
            events,
            selection,
            state,
            started,
        }
    }

    pub(crate) async fn run(self) {
        self.run_with(authorize, |title, body| async move {
            show(&title, &body, true).await
        })
        .await;
    }

    async fn run_with<A, AF, S, SF>(mut self, mut authorize: A, mut show: S)
    where
        A: FnMut() -> AF,
        AF: Future<Output = Result<(), String>>,
        S: FnMut(String, String) -> SF,
        SF: Future<Output = Result<Option<Response>, String>>,
    {
        let mut seen = Seen::default();
        let session = Arc::new(());
        let mut responses: Vec<PendingResponse> = Vec::new();
        let mut shutdown = self.selection.clone();
        'worker: loop {
            let candidate = tokio::select! {
                biased;
                _ = shutdown.wait_for(|s| s.shutdown || s.sign_out != 0) => break,
                ((clicked, target), index, _) = async { select_all(responses.iter_mut()).await }, if !responses.is_empty() => {
                    drop(responses.remove(index));
                    if clicked {
                        self.state.send_modify(|state| state.notification_activation = Some(Arc::new(Activation::new(target, &session))));
                    }
                    continue;
                }
                candidate = self.events.recv() => {
                    let Some(candidate) = candidate else { break };
                    candidate
                }
            };
            // Record even suppressed events to avoid re-alerting after focus changes.
            if !seen.insert(&candidate)
                || !candidate.eligible(&self.selection.borrow(), self.started, now_ms())
            {
                continue;
            }
            let mut shutdown = self.selection.clone();
            let delivery = async {
                authorize().await?;
                // Permission can take time. Re-check current focus, settings
                // and age before sharing content with the desktop service.
                let preview = {
                    let selection = self.selection.borrow();
                    if !candidate.eligible(&selection, self.started, now_ms()) {
                        return Ok(None);
                    }
                    candidate.preview(selection.notifications.show_preview)
                };
                show(preview.0, preview.1).await
            };
            let result = {
                tokio::pin!(delivery);
                loop {
                    tokio::select! {
                        biased;
                        _ = shutdown.wait_for(|s| s.shutdown || s.sign_out != 0) => break 'worker,
                        ((clicked, target), index, _) = async { select_all(responses.iter_mut()).await }, if !responses.is_empty() => {
                            drop(responses.remove(index));
                            if clicked {
                                self.state.send_modify(|state| state.notification_activation = Some(Arc::new(Activation::new(target, &session))));
                            }
                        }
                        result = &mut delivery => break result,
                    }
                }
            };
            if matches!(result, Ok(None)) {
                continue;
            }
            let result = result.map(|response| {
                if let Some(response) = response {
                    // Bound retained OS handles; keep the newest alerts clickable.
                    if responses.len() >= QUEUE_LIMIT {
                        drop(responses.remove(0));
                    }
                    let target = Target {
                        room: candidate.room.clone(),
                        thread: candidate.thread.clone(),
                    };
                    responses.push(Box::pin(async move { (response.await, target) }));
                }
            });
            self.state.send_if_modified(|state| {
                if state.notification_error == result.as_ref().err().cloned() {
                    false
                } else {
                    state.notification_error = result.err();
                    true
                }
            });
        }
        drop(responses);
        drop(session);
        // The connection owns shutdown; remain inert until it drops this worker.
        std::future::pending::<()>().await;
    }
}

async fn authorize() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::authorize().await?;
    }
    Ok(())
}

async fn show(title: &str, body: &str, observe: bool) -> Result<Option<Response>, String> {
    #[cfg(target_os = "linux")]
    {
        use iced::futures::StreamExt;
        use std::collections::HashMap;
        let deliver = async {
            let connection = zbus::Connection::session().await?;
            let proxy = zbus::Proxy::new_owned(
                connection,
                "org.freedesktop.Notifications",
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
            )
            .await?;
            // Activate a lazily started notification daemon before subscribing.
            proxy.call_method("GetServerInformation", &()).await?;
            // Subscribe before Notify: a fast click or dismissal cannot race
            // registration. Proxy streams authenticate the service's sender.
            let signals = if observe {
                Some((
                    proxy.receive_signal("ActionInvoked").await?,
                    proxy.receive_signal("NotificationClosed").await?,
                ))
            } else {
                None
            };
            let hints: HashMap<&str, zbus::Variant<'_>> = HashMap::from([
                ("category", "im.received".into()),
                ("desktop-entry", "interconnect".into()),
            ]);
            let body = body
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let id: u32 = proxy
                .call(
                    "Notify",
                    &(
                        "Interconnect",
                        0u32,
                        "",
                        title,
                        body,
                        if observe {
                            vec!["default", "Open conversation"]
                        } else {
                            Vec::new()
                        },
                        hints,
                        -1i32,
                    ),
                )
                .await?;
            let Some((mut actions, mut closed)) = signals else {
                return Ok(None);
            };
            Ok::<_, zbus::Error>(Some(Box::pin(async move {
                // Retain the connection and signal subscriptions until response
                // or cancellation. No blocking waiter or timer is needed.
                let _proxy = proxy;
                loop {
                    tokio::select! {
                        biased;
                        action = actions.next() => {
                            let Some(action) = action else { return false };
                            if let Ok((notification, action)) = action.body().deserialize::<(u32, String)>()
                                && notification == id
                            {
                                return action == "default";
                            }
                        }
                        close = closed.next() => {
                            let Some(close) = close else { return false };
                            if let Ok((notification, _)) = close.body().deserialize::<(u32, u32)>()
                                && notification == id
                            {
                                return false;
                            }
                        }
                    }
                }
            }) as Response))
        };
        tokio::time::timeout(Duration::from_secs(10), deliver).await
            .map_err(|_| "Desktop notification service timed out.".to_owned())?
            .map_err(|_| "Could not deliver a desktop notification. Check desktop notification permissions and the session D-Bus service on Linux.".to_owned())
    }
    #[cfg(target_os = "macos")]
    {
        macos::show(title, body, observe).await
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (title, body, observe);
        Err("Desktop notifications are supported on Linux and macOS.".into())
    }
}

/// Explicit, account-independent native test, also available in demo mode.
pub async fn test_notification() -> Result<(), String> {
    authorize().await?;
    show("Interconnect", "Desktop notifications are working.", false)
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests;
