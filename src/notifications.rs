//! Bounded, session-owned inbox collection and desktop delivery are independent.
//! SDK push rules supply messages; SDK snapshots supply invitations and verification.
use std::{
    collections::{HashSet, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};

use iced::futures::future::select_all;
use matrix_sdk::{
    Client, Room, RoomState, deserialized_responses::RawAnySyncOrStrippedTimelineEvent,
    sync::Notification,
};
use serde::{Deserialize, Serialize};
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

/// Destination retained locally, never included in the OS payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Conversation {
        room: String,
        thread: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<String>,
    },
    Invitation {
        room: String,
    },
    Verification {
        flow: String,
        device: String,
    },
}

pub const HISTORY_LIMIT: usize = 256;

/// A bounded inbox entry. Timestamp is milliseconds since the Unix epoch.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub target: Target,
    pub timestamp: u64,
    pub title: String,
    pub body: String,
}
impl Entry {
    /// Older records encoded the original event only in the stable message ID.
    /// Recover that exact destination without inferring from display text.
    pub fn destination(&self) -> Target {
        let Target::Conversation {
            room,
            thread,
            event: None,
        } = &self.target
        else {
            return self.target.clone();
        };
        let recovered = self
            .id
            .strip_prefix("message:")
            .filter(|identity| identity.len() <= 2048)
            .and_then(|identity| serde_json::from_str::<(String, String)>(identity).ok());
        let Some((source, event)) = recovered else {
            return self.target.clone();
        };
        if &source != room
            || event.len() > 1024
            || event.chars().any(char::is_control)
            || event.parse::<matrix_sdk::ruma::OwnedEventId>().is_err()
        {
            return self.target.clone();
        }
        Target::Conversation {
            room: room.clone(),
            thread: thread.clone(),
            event: Some(event),
        }
    }
}
impl std::fmt::Debug for Entry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Entry")
            .field("id", &self.id)
            .field("target", &self.target)
            .field("timestamp", &self.timestamp)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub entry: Entry,
    pub read: bool,
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
#[derive(Clone)]
struct Candidate {
    category: Category,
    room: String,
    room_name: String,
    id: String,
    sender: String,
    timestamp: u64,
    thread: Option<String>,
    body: String,
}

#[derive(Clone)]
enum Category {
    Message,
    Invitation,
    Verification { flow: String, device: String },
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
            category: Category::Message,
            room: room.into(),
            room_name: plain(room_name, 100),
            id: event.event_id,
            sender: event.sender,
            timestamp: event.origin_server_ts,
            thread,
            body,
        })
    }

    fn invitation(invitation: &crate::room_membership::Invitation, timestamp: u64) -> Self {
        Self {
            category: Category::Invitation,
            room: invitation.id.clone(),
            room_name: plain(&invitation.name, 100),
            id: invitation.id.clone(),
            sender: invitation.inviter.clone().unwrap_or_default(),
            timestamp,
            thread: None,
            body: if invitation.space { "space" } else { "room" }.into(),
        }
    }
    fn verification(incoming: &crate::verification::Incoming, timestamp: u64) -> Self {
        Self {
            category: Category::Verification {
                flow: incoming.flow.clone(),
                device: incoming.device.clone(),
            },
            room: String::new(),
            room_name: String::new(),
            id: incoming.flow.clone(),
            sender: String::new(),
            timestamp,
            thread: None,
            body: String::new(),
        }
    }
    fn target(&self) -> Target {
        match &self.category {
            Category::Message => Target::Conversation {
                room: self.room.clone(),
                thread: self.thread.clone(),
                event: Some(self.id.clone()),
            },
            Category::Invitation => Target::Invitation {
                room: self.room.clone(),
            },
            Category::Verification { flow, device } => Target::Verification {
                flow: flow.clone(),
                device: device.clone(),
            },
        }
    }
    fn history_id(&self) -> String {
        // Encode identity tuples rather than concatenate unescaped Matrix IDs.
        // This stays stable across restarts and cannot collide across categories.
        match &self.category {
            Category::Message => format!(
                "message:{}",
                serde_json::to_string(&(&self.room, &self.id)).expect("string tuple")
            ),
            Category::Invitation => format!("invitation:{}", self.room),
            Category::Verification { flow, device } => format!(
                "verification:{}",
                serde_json::to_string(&(flow, device)).expect("string tuple")
            ),
        }
    }
    fn fresh(&self, started: u64, now: u64) -> bool {
        self.timestamp >= started
            && now.saturating_sub(self.timestamp) <= MAX_AGE_MS
            && self.timestamp <= now.saturating_add(MAX_AGE_MS)
    }
    fn eligible(&self, selection: &Selection, started: u64, now: u64) -> bool {
        if selection.shutdown
            || selection.sign_out != 0
            || !selection.notifications.enabled
            || !self.fresh(started, now)
        {
            return false;
        }
        match &self.category {
            Category::Invitation => return true,
            Category::Verification { .. } => return !selection.verification_visible,
            Category::Message => {}
        }
        if !selection.conversation_visible
            || selection
                .room
                .as_ref()
                .is_none_or(|(_, room)| room != &self.room)
        {
            return true;
        }
        self.thread.as_ref().is_some_and(|root| {
            selection
                .thread
                .as_ref()
                .is_none_or(|(_, open)| open != root)
        })
    }
    fn current(&self, model: &Model) -> bool {
        target_current(&self.target(), model)
    }
    fn preview(&self, show: bool) -> (String, String) {
        if !show {
            return (
                "Interconnect".into(),
                match self.category {
                    Category::Message => "New Matrix message",
                    Category::Invitation => "New room invitation",
                    Category::Verification { .. } => "Device verification requested",
                }
                .into(),
            );
        }
        match &self.category {
            Category::Message => (
                plain(&self.room_name, 100),
                plain(&format!("{}: {}", self.sender, self.body), 300),
            ),
            Category::Invitation => (
                plain(&format!("Invitation to {}", self.room_name), 100),
                if self.sender.is_empty() {
                    format!("You were invited to this {}", self.body)
                } else {
                    plain(
                        &format!("{} invited you to this {}", self.sender, self.body),
                        300,
                    )
                },
            ),
            Category::Verification { device, .. } => (
                "Device verification requested".into(),
                plain(&format!("Review the request from device {device}"), 300),
            ),
        }
    }
    fn entry(&self, show_preview: bool) -> Entry {
        let (title, body) = self.preview(show_preview);
        Entry {
            id: self.history_id(),
            target: self.target(),
            timestamp: self.timestamp,
            title,
            body,
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
        let key = (candidate.room.clone(), candidate.history_id());
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

fn target_current(target: &Target, model: &Model) -> bool {
    match target {
        Target::Conversation { .. } => true,
        Target::Invitation { room } => model.invitations.iter().any(|invite| &invite.id == room),
        Target::Verification { flow, device } => model
            .verification
            .incoming
            .as_ref()
            .is_some_and(|incoming| &incoming.flow == flow && &incoming.device == device),
    }
}
#[derive(Default)]
struct Attention {
    invitations_ready: bool,
    invitations: HashSet<String>,
    invitations_source: Option<Arc<Vec<crate::room_membership::Invitation>>>,
    incoming: Option<(String, String)>,
    last_timestamp: u64,
}
impl Attention {
    fn timestamp(&mut self, now: u64) -> u64 {
        self.last_timestamp = now.max(self.last_timestamp.saturating_add(1));
        self.last_timestamp
    }

    /// The first completed invitation snapshot is history-only. Later membership
    /// transitions notify, while metadata changes and our own publications do not.
    fn observe(&mut self, model: &Model, timestamp: u64) -> Vec<(Candidate, bool)> {
        let mut notices = Vec::new();
        if (!self.invitations_ready && model.invitations_ready)
            || self
                .invitations_source
                .as_ref()
                .is_none_or(|source| !Arc::ptr_eq(source, &model.invitations))
        {
            for invitation in model.invitations.iter() {
                if !self.invitations.contains(&invitation.id) {
                    let timestamp = self.timestamp(timestamp);
                    notices.push((
                        Candidate::invitation(invitation, timestamp),
                        self.invitations_ready && model.invitations_ready,
                    ));
                }
            }
            self.invitations = model
                .invitations
                .iter()
                .map(|invite| invite.id.clone())
                .collect();
            self.invitations_source = Some(model.invitations.clone());
            self.invitations_ready |= model.invitations_ready;
        }
        let incoming = model
            .verification
            .incoming
            .as_ref()
            .map(|incoming| (incoming.flow.clone(), incoming.device.clone()));
        if incoming != self.incoming {
            if let Some(incoming) = &model.verification.incoming {
                let timestamp = self.timestamp(timestamp);
                notices.push((Candidate::verification(incoming, timestamp), true));
            }
            self.incoming = incoming;
        }
        notices
    }
}
fn record(state: &watch::Sender<Model>, entry: Entry) {
    state.send_if_modified(|model| {
        if model.notification_history.iter().any(|old| old == &entry) {
            return false;
        }
        let history = Arc::make_mut(&mut model.notification_history);
        history.retain(|old| old.id != entry.id);
        history.push(entry);
        history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
        history.truncate(HISTORY_LIMIT);
        true
    });
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

    async fn run_with<A, AF, S, SF>(mut self, mut authorize: A, show: S)
    where
        A: FnMut() -> AF,
        AF: Future<Output = Result<(), String>> + Send + 'static,
        S: FnMut(String, String) -> SF + Send + 'static,
        SF: Future<Output = Result<Option<Response>, String>> + Send + 'static,
    {
        type Delivery<S> =
            Pin<Box<dyn Future<Output = (Target, S, Result<Option<Response>, String>)> + Send>>;
        let mut seen = Seen::default();
        let mut attention = Attention::default();
        let mut models = self.state.subscribe();
        let session = Arc::new(());
        let mut responses: Vec<PendingResponse> = Vec::new();
        let mut pending = VecDeque::<Candidate>::new();
        let mut show = Some(show);
        let mut delivery: Option<Delivery<S>> = None;
        let mut shutdown = self.selection.clone();
        let mut notices = attention.observe(&models.borrow_and_update(), now_ms());
        loop {
            for (candidate, native) in notices.drain(..) {
                // Invitation transitions are deduplicated by the snapshot observer,
                // allowing a later reinvitation to replace the same stable entry.
                let fresh = candidate.fresh(self.started, now_ms());
                let unseen =
                    matches!(candidate.category, Category::Invitation) || seen.insert(&candidate);
                if !fresh || !unseen {
                    continue;
                }
                record(
                    &self.state,
                    candidate.entry(self.selection.borrow().notifications.show_preview),
                );
                if native && candidate.eligible(&self.selection.borrow(), self.started, now_ms()) {
                    if pending.len() >= QUEUE_LIMIT {
                        pending.pop_front();
                    }
                    pending.push_back(candidate);
                }
            }
            if delivery.is_none() {
                while let Some(candidate) = pending.pop_front() {
                    if !candidate.eligible(&self.selection.borrow(), self.started, now_ms())
                        || !candidate.current(&models.borrow())
                    {
                        continue;
                    }
                    let authorization = authorize();
                    let mut show_callback = show.take().expect("one desktop delivery at a time");
                    let selection = self.selection.clone();
                    let model = models.clone();
                    let started = self.started;
                    delivery = Some(Box::pin(async move {
                        let target = candidate.target();
                        let result = async {
                            authorization.await?;
                            let preview = {
                                let selection = selection.borrow();
                                if !candidate.eligible(&selection, started, now_ms())
                                    || !candidate.current(&model.borrow())
                                {
                                    return Ok(None);
                                }
                                candidate.preview(selection.notifications.show_preview)
                            };
                            show_callback(preview.0, preview.1).await
                        }
                        .await;
                        (target, show_callback, result)
                    }));
                    break;
                }
            }
            tokio::select! {
                biased;
                _ = shutdown.wait_for(|s| s.shutdown || s.sign_out != 0) => break,
                ((clicked, target), index, _) = async { select_all(responses.iter_mut()).await }, if !responses.is_empty() => {
                    drop(responses.remove(index));
                    if clicked && target_current(&target, &models.borrow()) {
                        self.state.send_modify(|state| state.notification_activation = Some(Arc::new(Activation::new(target, &session))));
                    }
                }
                (target, callback, result) = async { delivery.as_mut().expect("active delivery").await }, if delivery.is_some() => {
                    delivery = None;
                    show = Some(callback);
                    match result {
                        Ok(Some(response)) => {
                            if responses.len() >= QUEUE_LIMIT { drop(responses.remove(0)); }
                            responses.push(Box::pin(async move { (response.await, target) }));
                            self.state.send_if_modified(|state| state.notification_error.take().is_some());
                        }
                        Ok(None) => {}
                        Err(error) => { self.state.send_if_modified(|state| {
                            if state.notification_error.as_ref() == Some(&error) { false } else { state.notification_error = Some(error); true }
                        }); }
                    }
                }
                changed = models.changed() => {
                    if changed.is_err() { break; }
                    notices = attention.observe(&models.borrow_and_update(), now_ms());
                }
                candidate = self.events.recv() => {
                    let Some(candidate) = candidate else { break; };
                    notices.push((candidate, true));
                }
            }
        }
        drop(delivery);
        drop(responses);
        drop(session);
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
        use std::{collections::HashMap, time::Duration};
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
                            vec!["default", "Open notification"]
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
