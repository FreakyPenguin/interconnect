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

mod receipts;
pub use receipts::ReadTarget;
mod typing;
pub use typing::{Activity as TypingActivity, Typing};
mod members;
pub use members::{Member, Members};
mod pinned;
pub use pinned::Pinned;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub typing: Option<TypingActivity>,
    pub read: [Option<ReadTarget>; 2],
    pub room: Option<(u64, String)>,
    pub thread: Option<(u64, String)>,
    /// Unique request generation; only load the selected room while People is open.
    pub members: Option<u64>,
    pub shutdown: bool,
    pub preferences: Option<crate::navigation_preferences::Preferences>,
    pub avatars: Vec<matrix_sdk::ruma::OwnedMxcUri>,
    pub images: Vec<crate::image_media::Key>,
    pub image_retry: u64,
    pub notifications: crate::settings::Notifications,
    pub conversation_visible: bool,
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
    pub avatar_url: Option<matrix_sdk::ruma::OwnedMxcUri>,
    pub image: Option<crate::image_media::Attachment>,
    pub download: Option<crate::download::Attachment>,
    pub reactions: Vec<Reaction>,
    pub can_react: bool,
    pub read_by: Vec<String>,
    pub timestamp_ms: Option<i64>,
    pub thread_replies: u32,
    pub thread_root: Option<String>,
    pub is_own: bool,
    pub is_live: bool,
    pub body: String,
    pub formatted: Option<Arc<crate::message_document::Document>>,
    pub send_state: String,
    pub retry: Option<RetrySend>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaction {
    pub key: String,
    pub count: usize,
    pub is_own: bool,
}

fn reactions(content: &TimelineItemContent, own_user: &matrix_sdk::ruma::UserId) -> Vec<Reaction> {
    let mut reactions: Vec<_> = content
        .reactions()
        .into_iter()
        .flat_map(|reactions| reactions.iter())
        .filter(|(_, senders)| !senders.is_empty())
        .map(|(key, senders)| Reaction {
            key: key.clone(),
            count: senders.len(),
            is_own: senders.contains_key(own_user),
        })
        .collect();
    // Redaction can reorder SDK entries. Keep remaining badges in place.
    reactions.sort_unstable_by(|a, b| a.key.cmp(&b.key));
    reactions
}

/// UI snapshots and messages can be destroyed on Iced's main thread, outside
/// Executor::enter. SDK/SQLite destructors may call Tokio's spawn_blocking.
/// Keep their final drop inside the originating runtime context, including when
/// a queued snapshot or retry message is discarded rather than handled.
#[derive(Debug)]
pub(crate) struct RuntimeOwned<T> {
    value: Option<T>,
    runtime: tokio::runtime::Handle,
}
impl<T> RuntimeOwned<T> {
    pub(crate) fn new(value: T) -> Arc<Self> {
        Self::with_runtime(value, tokio::runtime::Handle::current())
    }
    pub(crate) fn with_runtime(value: T, runtime: tokio::runtime::Handle) -> Arc<Self> {
        Arc::new(Self {
            value: Some(value),
            runtime,
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
    /// Keep the runtime-owned timeline alive for the duration of the media request.
    pub async fn download_attachment(
        self,
        attachment: crate::download::Attachment,
    ) -> Result<Vec<u8>, String> {
        attachment.fetch(&self.0.room().client()).await
    }

    pub fn people_directory(&self) -> crate::people::Directory {
        crate::people::Directory::from_runtime(
            self.0.room().client().clone(),
            self.0.runtime.clone(),
        )
    }

    /// Explicit writes finish against the captured room, even after navigation.
    pub async fn invite_member(self, input: String) -> Result<(), String> {
        let user = crate::people::parse_user_id(&input)?;
        let invite = async {
            use matrix_sdk::ruma::events::room::member::MembershipState;
            let room = self.0.room();
            let levels = room
                .power_levels()
                .await
                .map_err(|error| error.to_string())?;
            if room.state() != matrix_sdk::RoomState::Joined
                || !levels.user_can_invite(room.own_user_id())
            {
                return Err("You no longer have permission to invite people to this room.".into());
            }
            if let Some(member) = room
                .get_member_no_sync(&user)
                .await
                .map_err(|error| error.to_string())?
            {
                match member.membership() {
                    MembershipState::Join => {
                        return Err("This person is already a room member.".into());
                    }
                    MembershipState::Invite => {
                        return Err("This person already has an invitation.".into());
                    }
                    MembershipState::Ban => {
                        return Err("This person is banned from the room.".into());
                    }
                    _ => {}
                }
            }
            room.invite_user_by_id(&user)
                .await
                .map_err(|error| error.to_string())
        };
        tokio::time::timeout(Duration::from_secs(30), invite)
            .await
            .map_err(|_| {
                "The invitation timed out and may have been sent. Check membership before retrying."
                    .to_owned()
            })?
    }

    /// Read current server state so successive local pin changes preserve one another
    /// while the sync stream is still catching up. Callers serialize writes per room.
    pub async fn set_message_pinned(self, event: String, pinned: bool) -> Result<(), String> {
        let update = async {
            use matrix_sdk::ruma::events::{
                StateEventType, room::pinned_events::RoomPinnedEventsEventContent,
            };
            let event: matrix_sdk::ruma::OwnedEventId = event
                .parse()
                .map_err(|error: matrix_sdk::ruma::IdParseError| error.to_string())?;
            let room = self.0.room();
            let mut pins = room
                .load_pinned_events()
                .await
                .map_err(|error| error.to_string())?
                .unwrap_or_default();
            let levels = room
                .power_levels()
                .await
                .map_err(|error| error.to_string())?;
            if room.state() != matrix_sdk::RoomState::Joined
                || !levels.user_can_send_state(room.own_user_id(), StateEventType::RoomPinnedEvents)
            {
                return Err(
                    "You no longer have permission to change pinned messages in this room.".into(),
                );
            }
            let changed = if pinned {
                if pins.contains(&event) {
                    false
                } else {
                    pins.push(event);
                    true
                }
            } else {
                let previous = pins.len();
                pins.retain(|id| id != &event);
                pins.len() != previous
            };
            if changed {
                room.send_state_event(RoomPinnedEventsEventContent::new(pins))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        };
        tokio::time::timeout(Duration::from_secs(30), update)
            .await
            .map_err(|_| {
                "Updating pinned messages timed out. Check pinned messages before retrying."
                    .to_owned()
            })?
    }

    /// Use the captured room, rechecking current state permissions before sending.
    pub async fn set_topic(self, topic: String) -> Result<(), String> {
        let update = async {
            let room = self.0.room();
            let levels = room
                .power_levels()
                .await
                .map_err(|error| error.to_string())?;
            if room.state() != matrix_sdk::RoomState::Joined
                || !levels.user_can_send_state(
                    room.own_user_id(),
                    matrix_sdk::ruma::events::StateEventType::RoomTopic,
                )
            {
                return Err("You no longer have permission to edit this room topic.".into());
            }
            room.set_room_topic(&topic)
                .await
                .map_err(|error| error.to_string())?;
            Ok(())
        };
        tokio::time::timeout(Duration::from_secs(30), update)
            .await
            .map_err(|_| {
                "Saving topic timed out. Check the room topic before retrying.".to_owned()
            })?
    }

    /// Set the state shown by a chip, so repeated/stale clicks cannot invert it.
    pub async fn set_reaction(
        self,
        event_id: String,
        key: String,
        selected: bool,
    ) -> Result<(), String> {
        if selected {
            return self.add_reaction(event_id, key).await;
        }
        use matrix_sdk::ruma::events::{
            AnySyncMessageLikeEvent, AnySyncTimelineEvent, relation::RelationType,
        };
        let event_id: matrix_sdk::ruma::OwnedEventId = event_id
            .parse()
            .map_err(|error: matrix_sdk::ruma::IdParseError| error.to_string())?;
        let remove = async {
            {
                let items = self.0.items().await;
                let event = items
                    .iter()
                    .filter_map(|item| item.as_event())
                    .find(|event| event.event_id() == Some(event_id.as_ref()))
                    .ok_or("Message is no longer available")?;
                if !can_react(event.content()) {
                    return Err("This message cannot receive reactions".to_owned());
                }
                if !event
                    .content()
                    .reactions()
                    .and_then(|reactions| reactions.get(&key))
                    .is_some_and(|senders| senders.contains_key(self.0.room().own_user_id()))
                {
                    return Ok(());
                }
            }
            let (cache, _lifetime) = self
                .0
                .room()
                .event_cache()
                .await
                .map_err(|error| error.to_string())?;
            let related = cache
                .find_event_relations(&event_id, Some(vec![RelationType::Annotation]))
                .await
                .map_err(|error| error.to_string())?;
            let mut ids = std::collections::HashSet::new();
            for event in related {
                let Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::Reaction(event))) =
                    event.raw().deserialize()
                else {
                    continue;
                };
                let Some(event) = event.as_original() else {
                    continue;
                };
                if event.sender == self.0.room().own_user_id()
                    && event.content.relates_to.event_id == event_id
                    && event.content.relates_to.key == key
                {
                    ids.insert(event.event_id.clone());
                }
            }
            if ids.is_empty() {
                return Err(
                    "Your reaction is not available yet. Wait for sync and try again.".into(),
                );
            }
            // A sender can have duplicate annotations. Remove all our matching
            // events, keeping every other sender's reaction intact.
            for id in ids {
                self.0
                    .room()
                    .redact(&id, None, None)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        };
        tokio::time::timeout(Duration::from_secs(30), remove)
            .await
            .map_err(|_| "Removing reaction timed out".to_owned())?
    }

    /// Add-only: picking an existing own reaction must never toggle it off.
    /// Keep the originating timeline alive until this asynchronous request ends.
    pub async fn add_reaction(self, event_id: String, key: String) -> Result<(), String> {
        use matrix_sdk::ruma::events::{reaction::ReactionEventContent, relation::Annotation};
        let event_id: matrix_sdk::ruma::OwnedEventId = event_id
            .parse()
            .map_err(|error: matrix_sdk::ruma::IdParseError| error.to_string())?;
        if key.trim().is_empty() {
            return Err("Reaction is empty".into());
        }
        {
            let items = self.0.items().await;
            let event = items
                .iter()
                .filter_map(|item| item.as_event())
                .find(|event| event.event_id() == Some(event_id.as_ref()))
                .ok_or("Message is no longer available")?;
            if !can_react(event.content()) {
                return Err("This message cannot receive reactions".into());
            }
            if event
                .content()
                .reactions()
                .and_then(|reactions| reactions.get(&key))
                .is_some_and(|senders| senders.contains_key(self.0.room().own_user_id()))
            {
                return Ok(());
            }
        }
        // Await the request so delivery errors can be shown and retried by the
        // picker. The SDK sync stream supplies the resulting reaction badge.
        tokio::time::timeout(
            Duration::from_secs(30),
            self.0
                .room()
                .send(ReactionEventContent::from(Annotation::new(event_id, key))),
        )
        .await
        .map_err(|_| "Sending reaction timed out".to_owned())?
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    /// Read locally on a blocking worker, then let the SDK own upload, encryption,
    /// local echoes and retries. The captured timeline supplies the thread relation.
    pub async fn send_attachment(
        self,
        attachment: crate::attachment::Attachment,
        caption: String,
    ) -> Result<(), String> {
        use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
        use matrix_sdk_ui::timeline::{AttachmentConfig, AttachmentSource};
        let prepared = attachment.read().await?;
        let config = AttachmentConfig {
            info: Some(prepared.info),
            thumbnail: prepared.thumbnail,
            caption: (!caption.trim().is_empty()).then(|| TextMessageEventContent::plain(caption)),
            ..Default::default()
        };
        self.0
            .send_attachment(
                AttachmentSource::Data {
                    bytes: prepared.bytes,
                    filename: prepared.filename,
                },
                prepared.mime,
                config,
            )
            .use_send_queue()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn send(self, body: String) -> Result<(), String> {
        self.send_as(body, crate::composition::Format::Markdown)
            .await
    }

    pub async fn send_as(
        self,
        body: String,
        format: crate::composition::Format,
    ) -> Result<(), String> {
        let prepared = crate::composition::prepare(body, format).await?;
        Box::pin(self.0.send(prepared.message_content().into()))
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

fn can_react(content: &TimelineItemContent) -> bool {
    matches!(content, TimelineItemContent::MsgLike(content) if !matches!(content.kind,
        matrix_sdk_ui::timeline::MsgLikeKind::Redacted | matrix_sdk_ui::timeline::MsgLikeKind::UnableToDecrypt(_)))
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Authentication {
    Checking,
    SignIn { error: Option<String> },
    Waiting { url: Option<String> },
    Failed(String),
}

#[derive(Clone, Debug)]
pub enum LoginRequest {
    SignIn {
        homeserver: String,
        method: crate::session::LoginMethod,
    },
}

#[derive(Debug)]
struct SignInRequired;
impl std::fmt::Display for SignInRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "The server rejected this session. Sign-in is required; saved credentials retained.",
        )
    }
}
impl std::error::Error for SignInRequired {}

#[derive(Clone, Debug)]
pub struct Model {
    pub people_directory: Option<crate::people::Directory>,
    pub typing: Option<Typing>,
    pub authentication: Option<Authentication>,
    pub account_id: Option<String>,
    pub verification: crate::verification::Snapshot,
    pub diagnostics: Arc<crate::diagnostics::Diagnostics>,
    pub status: String,
    pub rooms: Arc<Vec<RoomEntry>>,
    pub spaces: Arc<Vec<SpaceEntry>>,
    pub navigation_error: Option<String>,
    pub saved_navigation: Option<crate::navigation_preferences::Preferences>,
    pub preferences_error: Option<String>,
    pub account_name: String,
    pub account_avatar_url: Option<matrix_sdk::ruma::OwnedMxcUri>,
    pub avatars: Arc<crate::media::Images>,
    pub images: crate::media::Snapshot<crate::image_media::Key>,
    pub media_error: Option<String>,
    pub notification_error: Option<String>,
    pub receipt_errors: [Option<(u64, String)>; 2],
    pub room: Option<Pane>,
    pub thread: Option<Pane>,
    pub pinned: Option<Pinned>,
    pub members: Option<Members>,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            people_directory: None,
            typing: None,
            authentication: Some(Authentication::Checking),
            account_id: None,
            verification: Default::default(),
            diagnostics: Arc::default(),
            status: "Restoring saved session…".into(),
            rooms: Arc::default(),
            spaces: Arc::default(),
            navigation_error: None,
            saved_navigation: None,
            preferences_error: None,
            account_name: "Me".into(),
            account_avatar_url: None,
            avatars: Arc::default(),
            images: Default::default(),
            media_error: None,
            notification_error: None,
            receipt_errors: Default::default(),
            room: None,
            thread: None,
            pinned: None,
            members: None,
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
    mut login: mpsc::Receiver<LoginRequest>,
) -> anyhow::Result<()> {
    let opened = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(90), crate::session::DesktopSession::open(&profile)) => result.context("session restoration timed out").and_then(|r| r),
        _ = selection.wait_for(|s| s.shutdown) => return Ok(()),
    };
    let mut desktop = match opened {
        Ok(desktop) => desktop,
        Err(error) => {
            state.send_modify(|s| {
                s.authentication = Some(Authentication::Failed(format!("{error:#}")))
            });
            return Err(error);
        }
    };
    loop {
        if desktop.client.is_none() {
            if !matches!(
                state.borrow().authentication,
                Some(Authentication::SignIn { .. })
            ) {
                state.send_modify(|s| {
                    s.authentication = Some(Authentication::SignIn { error: None })
                });
            }
            loop {
                let (homeserver, method) = tokio::select! {
                    request = login.recv() => match request { Some(LoginRequest::SignIn { homeserver, method }) => (homeserver, method), None => return Ok(()) },
                    _ = selection.wait_for(|s| s.shutdown) => return Ok(()),
                };
                state.send_modify(|s| {
                    s.authentication = Some(Authentication::Waiting { url: None })
                });
                let result = tokio::select! {
                    result = tokio::time::timeout(Duration::from_secs(180), desktop.sign_in(&homeserver, method, |url| {
                        state.send_modify(|s| s.authentication = Some(Authentication::Waiting { url: Some(url) }));
                    })) => result.context("Sign-in timed out after three minutes").and_then(|r| r),
                    _ = selection.wait_for(|s| s.shutdown) => return Ok(()),

                };
                // Discard duplicate submissions from an older UI snapshot.
                while login.try_recv().is_ok() {}
                match result {
                    Ok(()) => break,
                    Err(_) => state.send_modify(|s| s.authentication = Some(Authentication::SignIn {
                        error: Some("Sign-in could not finish. Check your credentials, homeserver, and system keyring, then try again. For browser sign-in, check the callback connection. If you approved a device, remove the abandoned Interconnect device in another client.".into()),
                    })),
                }
            }
        }
        let client = desktop.client.as_ref().expect("authenticated");
        state.send_modify(|s| {
            s.authentication = None;
            s.people_directory = Some(crate::people::Directory::new(client.clone()));
            s.account_id = client.user_id().map(ToString::to_string);
            s.status = "Loading conversations…".into();
        });
        let result = run_authenticated(&profile, client, selection.clone(), state.clone()).await;
        if result.as_ref().is_err_and(|e| e.is::<SignInRequired>()) && !selection.borrow().shutdown
        {
            desktop.client = None;
            state.send_replace(Model { authentication: Some(Authentication::SignIn {
                error: Some("Your session has expired. Sign in to continue. This creates a new device; previous encrypted stores are retained.".into()),
            }), ..Model::default() });
            // Wait for an explicit sign-in; do not replace a device automatically.
            continue;
        }
        return result;
    }
}

async fn run_authenticated(
    profile: &str,
    client: &Client,
    mut selection: watch::Receiver<Selection>,
    state: watch::Sender<Model>,
) -> anyhow::Result<()> {
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
    let sync = build_sync(client.clone()).await?;
    let stop_preferences = tokio_util::sync::CancellationToken::new();
    let persistence = crate::navigation_preferences::persist(
        directory,
        selection.clone(),
        state.clone(),
        preferences,
        stop_preferences.clone(),
    );
    // Register before starting sync. Delivery runs independently of UI snapshots
    // and never holds up SDK sync processing on a desktop service.
    let notifications =
        crate::notifications::Service::install(client, selection.clone(), state.clone()).await;
    let stop_verification = tokio_util::sync::CancellationToken::new();
    let verification = crate::verification::Service::new(client.clone())
        .run(state.clone(), stop_verification.clone());
    tokio::pin!(verification);
    let connection = async {
        tokio::select! {
            result = connected(client, &sync, &mut selection, &state) => result,
            _ = notifications.run() => anyhow::bail!("Notification worker ended"),
        }
    };
    let mut verification_finished = false;
    let outcome = {
        tokio::pin!(persistence, connection);
        let mut writer_finished = false;
        let result = tokio::select! {
            result = &mut connection => result,
            _ = &mut verification => { verification_finished = true; Err(anyhow::anyhow!("Verification worker ended")) },
            _ = &mut persistence => {
                writer_finished = true;
                tokio::select! {
                    result = &mut connection => result,
                    _ = &mut verification => { verification_finished = true; Err(anyhow::anyhow!("Verification worker ended")) },
                }
            },
        };
        stop_preferences.cancel();
        if !writer_finished {
            persistence.await;
        }
        result
    };
    stop_verification.cancel();
    if !verification_finished {
        verification.await;
    }
    state.send_modify(|s| s.status = "Stopping synchronization…".into());
    let stopped = crate::sso::stop_with_deadline(sync.stop(), Duration::from_secs(15)).await;
    // Keep the profile guard alive until SDK stop has completed or failed.
    drop(sync);
    outcome.and(stopped)
}

async fn build_sync(client: Client) -> anyhow::Result<SyncService> {
    Ok(SyncService::builder(client)
        .with_offline_mode()
        // The SDK default of one event can hide a mention behind a later
        // message in an unselected room. Retain a bounded live burst for alerts.
        .with_room_list_timeline_limit(20)
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
    let (avatar_requests, requests) = watch::channel(crate::media::Demand::default());
    let (images, mut avatar_updates) = watch::channel(crate::media::Snapshot::default());
    let slots = Arc::new(tokio::sync::Semaphore::new(4));
    let media = crate::media::run_requests(client.clone(), requests, images, slots.clone());
    let (image_requests, requests) = watch::channel(crate::media::Demand::default());
    let (images, mut image_updates) = watch::channel(crate::media::Snapshot::default());
    let attachments = crate::media::run_requests(client.clone(), requests, images, slots);
    tokio::pin!(media, attachments);
    let mut media_finished = false;
    let mut images_finished = false;
    let mut retry_epoch = 0;
    let profile = account_profile_updates(client, state);
    tokio::pin!(profile);
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
    let mut typing_view = inactive();
    let typing_stop = tokio_util::sync::CancellationToken::new();
    let outgoing = typing::outgoing(client.clone(), selection.clone(), typing_stop.clone());
    tokio::pin!(outgoing);
    let mut outgoing_finished = false;
    let mut pinned_view = inactive();
    let mut members_view = inactive();
    let mut thread_view = inactive();
    let result = async {
        loop {
            let next = selection.borrow_and_update().clone();
            if next.shutdown { break; }
            let demand = crate::media::Demand { uris: next.avatars.clone(), retry_epoch };
            avatar_requests.send_if_modified(|old| {
                if *old == demand { false } else { *old = demand; true }
            });
            let demand = crate::media::Demand { uris: next.images.clone(), retry_epoch: retry_epoch.wrapping_add(next.image_retry) };
            image_requests.send_if_modified(|old| {
                if *old == demand { false } else { *old = demand; true }
            });
            let status = connection_status(&sync_states.get(), &room_states.get());
            state.send_if_modified(|s| {
                if s.status == status { false } else { s.status = status.into(); true }
            });
            if active.room != next.room {
                // Drop the old subscriptions before constructing replacements.
                members_view = inactive();
                typing_view = inactive();
                room_view = inactive();
                pinned_view = inactive();
                thread_view = inactive();
                if let Some((_, id)) = &active.room {
                    let id: matrix_sdk::ruma::OwnedRoomId = id.parse()?;
                    sync.room_list_service().remove_room_subscriptions(&[&id]);
                }
                state.send_modify(|s| { s.room = None; s.thread = None; s.pinned = None; s.typing = None; });
                if let Some((generation, id)) = &next.room {
                    let id: matrix_sdk::ruma::OwnedRoomId = id.parse()?;
                    typing_view = typing::incoming(client.clone(), id.to_string(), *generation, state.clone());
                    // Sync subscription setup is cancelled by a newer navigation request.
                    let room_list = sync.room_list_service();
                    let ids = [&*id];
                    tokio::select! {
                        _ = room_list.set_room_subscriptions(&ids) => {},
                        changed = selection.changed() => { changed.context("UI closed")?; typing_view = inactive(); sync.room_list_service().remove_room_subscriptions(&[&id]); active = Selection::default(); continue; }
                    }
                    room_view = Box::pin(view(client.clone(), id.to_string(), None, *generation, state.clone(), selection.clone()));
                    pinned_view = Box::pin(pinned::view(client.clone(), id.to_string(), *generation, state.clone()));
                }
            }
            if active.thread != next.thread || active.room != next.room {
                thread_view = inactive();
                state.send_modify(|s| s.thread = None);
                if let (Some((_, room)), Some((generation, root))) = (&next.room, &next.thread) {
                    thread_view = Box::pin(view(client.clone(), room.clone(), Some(root.clone()), *generation, state.clone(), selection.clone()));
                }
            }
            if active.members != next.members || active.room != next.room {
                members_view = inactive();
                state.send_modify(|s| s.members = None);
                if let (Some((_, room)), Some(generation)) = (&next.room, next.members) {
                    members_view = Box::pin(members::view(client.clone(), room.clone(), generation, state.clone()));
                }
            }
            active = next;
            tokio::select! {
                result = &mut media, if !media_finished => {
                    media_finished = true;
                    if result.is_err() {
                        state.send_modify(|s| s.media_error = Some("Could not initialize the avatar cache.".into()));
                    }
                }
                result = &mut attachments, if !images_finished => {
                    images_finished = true;
                    if result.is_err() {
                        state.send_modify(|s| s.media_error = Some("Could not initialize the image cache.".into()));
                    }
                }
                changed = image_updates.changed(), if !images_finished => {
                    if changed.is_ok() {
                        let images = image_updates.borrow_and_update().clone();
                        state.send_modify(|s| s.images = images);
                    }
                }
                changed = avatar_updates.changed(), if !media_finished => {
                    if changed.is_ok() {
                        let images = avatar_updates.borrow_and_update().clone();
                        state.send_modify(|s| s.avatars = images.ready);
                    }
                }
                changed = selection.changed() => { if changed.is_err() { break; } }
                _ = &mut profile => {},
                _ = &mut navigation => anyhow::bail!("Navigation update stream ended"),
                _ = &mut typing_view => typing_view = inactive(),
                _ = &mut outgoing, if !outgoing_finished => outgoing_finished = true,
                _ = &mut room_view => room_view = inactive(),
                _ = &mut pinned_view => pinned_view = inactive(),
                _ = &mut members_view => members_view = inactive(),
                _ = &mut thread_view => thread_view = inactive(),
                event = room_states.next() => {
                    if event.is_none() { anyhow::bail!("room-list stream ended"); }

                }
                event = session_changes.recv() => match event {
                    Ok(matrix_sdk::SessionChange::UnknownToken(_)) => return Err(SignInRequired.into()),
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {},
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => anyhow::bail!("Session notification stream ended"),
                },
                event = sync_states.next() => match event {
                    Some(State::Error(error)) => anyhow::bail!("Synchronization service stopped: {error}. Session retained."),
                    None | Some(State::Terminated) => anyhow::bail!("Synchronization ended. Session retained."),
                    Some(State::Running) => retry_epoch += 1,
                    _ => {},
                }
            }
        }
        Ok(())
    }.await;
    typing_stop.cancel();
    if !outgoing_finished {
        outgoing.await;
    }
    drop(typing_view);
    drop(room_view);
    drop(pinned_view);
    drop(members_view);
    drop(thread_view);
    if let Some((_, id)) = active.room {
        let id: matrix_sdk::ruma::OwnedRoomId = id.parse()?;
        sync.room_list_service().remove_room_subscriptions(&[&id]);
    }
    result
}

// Scoped to the connection future: shutdown drops profile requests and subscriptions.
async fn account_profile_updates(client: &Client, state: &watch::Sender<Model>) {
    use matrix_sdk::ruma::profile::{AvatarUrl, DisplayName, UserProfile};

    let fallback = client.user_id().map_or("Me", |id| id.as_str());
    let publish = |profile: UserProfile| {
        let name = profile
            .get_static::<DisplayName>()
            .ok()
            .flatten()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| fallback.to_owned());
        let avatar = profile.get_static::<AvatarUrl>().ok().flatten();
        state.send_if_modified(|s| {
            if s.account_name == name && s.account_avatar_url == avatar {
                return false;
            }
            s.account_name = name;
            s.account_avatar_url = avatar;
            true
        });
    };
    publish(UserProfile::default());
    if let Ok(updates) = client.subscribe_to_own_profile() {
        tokio::pin!(updates);
        if let Some(cached) = updates.next().await {
            publish(cached);
        }
        // A one-time fetch also supports servers without the profiles extension.
        // Sync profile updates supersede an in-flight fetch.
        let account = client.account();
        tokio::select! {
            result = account.fetch_user_profile() => {
                if let Ok(profile) = result { publish(profile.data); }
            }
            Some(profile) = updates.next() => publish(profile),
        }
        while let Some(profile) = updates.next().await {
            publish(profile);
        }
    }
    pending::<()>().await;
}

async fn navigation_updates(client: &Client, state: &watch::Sender<Model>) {
    // Explicit creation also refreshes navigation before its first sync arrives.
    let mut refresh = state
        .borrow()
        .people_directory
        .as_ref()
        .map(|d| d.navigation_changes());
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
            _ = async { match &mut refresh { Some(receiver) => { if matches!(receiver.recv().await, Err(tokio::sync::broadcast::error::RecvError::Closed)) { pending::<()>().await; } }, None => pending::<()>().await } } => {},
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
    selection: watch::Receiver<Selection>,
) {
    tracing::debug!(target: "interconnect", generation, thread = root.is_some(), "connected presentation view started");
    let diagnostics = state.borrow().diagnostics.clone();
    let _lifetime = diagnostics.view(root.is_some());
    let is_thread = root.is_some();
    let result: anyhow::Result<()> = async {
        let setup = async {
            let room_id: matrix_sdk::ruma::OwnedRoomId = room_id.parse()?;
            let room = client.get_room(&room_id).context("Room is not loaded")?;
            // The SDK handles both room types and chooses encryption when sending.
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
                    .track_read_marker_and_receipts(matrix_sdk_ui::timeline::TimelineReadReceiptTracking::AllEvents)
                    .build()
                    .await?,
            )
        };
        let timeline = tokio::time::timeout(Duration::from_secs(60), setup)
            .await
            .context("Timeline loading timed out")??;
        let timeline = Arc::new(timeline);
        diagnostics.timeline(&timeline);
        let receipts = receipts::run(timeline.clone(), selection, generation, is_thread, state.clone());
        tokio::pin!(receipts);
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
                _ = &mut receipts => break,
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
        Ok(())
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
    let previous = {
        let model = state.borrow();
        let pane = if thread { &model.thread } else { &model.room };
        pane.as_ref()
            .filter(|p| p.generation == generation)
            .map(|p| p.rows.clone())
            .unwrap_or_default()
    };
    let rows = timeline_rows_cached(timeline, previous).await;
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

async fn timeline_rows(timeline: &Timeline) -> Vec<MessageRow> {
    timeline_rows_cached(timeline, Arc::default()).await
}

async fn timeline_rows_cached(
    timeline: &Timeline,
    previous: Arc<Vec<MessageRow>>,
) -> Vec<MessageRow> {
    let items = timeline.items().await;
    let own_user = timeline.room().own_user_id().to_owned();
    let queue = timeline.room().send_queue();
    let has_text = items.iter().filter_map(|item| item.as_event()).any(|event| {
        matches!(event.content(), TimelineItemContent::MsgLike(content) if matches!(&content.kind,
            matrix_sdk_ui::timeline::MsgLikeKind::Message(message) if matches!(message.msgtype(), matrix_sdk::ruma::events::room::message::MessageType::Text(_) | matrix_sdk::ruma::events::room::message::MessageType::Notice(_) | matrix_sdk::ruma::events::room::message::MessageType::Emote(_))))
    });
    // Dropping a view cannot interrupt a running blocking call. Stop between
    // bounded messages so an obsolete projection does not finish an entire history.
    let stopped = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = stopped.clone().drop_guard();
    let project = move || {
        let previous: std::collections::HashMap<_, _> =
            previous.iter().map(|row| (row.key.as_str(), row)).collect();
        items
            .iter()
            .take_while(|_| !stopped.is_cancelled())
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
                    avatar_url: match event.sender_profile() {
                        matrix_sdk_ui::timeline::TimelineDetails::Ready(profile) => {
                            profile.avatar_url.clone()
                        }
                        _ => None,
                    },
                    image: crate::image_media::Attachment::from_content(content),
                    download: crate::download::Attachment::from_content(content),
                    reactions: reactions(content, &own_user),
                    can_react: event.event_id().is_some() && can_react(content),
                    read_by: {
                        let mut users: Vec<_> = event
                            .read_receipts()
                            .keys()
                            .filter(|user| **user != own_user)
                            .map(ToString::to_string)
                            .collect();
                        users.sort();
                        users
                    },
                    timestamp_ms: i64::try_from(u64::from(event.timestamp().get())).ok(),
                    thread_replies: content
                        .thread_summary()
                        .map_or(0, |summary| summary.num_replies),
                    thread_root: content.thread_root().map(|id| id.to_string()),
                    is_own: event.is_own(),
                    is_live: matches!(event.origin(), Some(EventItemOrigin::Sync)),
                    formatted: match content {
                        TimelineItemContent::MsgLike(content) => match &content.kind {
                            matrix_sdk_ui::timeline::MsgLikeKind::Message(message) => {
                                let key = match event.identifier() {
                                    TimelineEventItemId::EventId(id) => id.to_string(),
                                    TimelineEventItemId::TransactionId(id) => format!("local:{id}"),
                                };
                                crate::message_document::Document::from_message(
                                    message.msgtype(),
                                    previous
                                        .get(key.as_str())
                                        .and_then(|row| row.formatted.as_ref()),
                                )
                            }
                            _ => None,
                        },
                        _ => None,
                    },
                    body: crate::event_text::body(content, event.sender()),
                    send_state: match event.send_state() {
                        Some(EventSendState::NotSentYet { .. }) => "Pending".into(),
                        Some(EventSendState::SendingFailed { error, .. }) => {
                            format!("Failed: {error}")
                        }
                        Some(EventSendState::Sent { .. }) => "Sent".into(),
                        None if event.sender() == own_user => "Sent".into(),
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
                                    queue: queue.clone(),
                                }),
                            })
                    } else {
                        None
                    },
                }
            })
            .collect()
    };
    if has_text {
        tokio::task::spawn_blocking(project)
            .await
            .expect("message projection worker panicked")
    } else {
        project()
    }
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
    async fn live_thread_reply_and_reactions_update_room_and_thread_snapshots() {
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
                    {"type":"m.room.member","state_key":"@other:example.org","sender":"@other:example.org","event_id":"$other-member","origin_server_ts":1,"content":{"membership":"join","displayname":"Other","avatar_url":"mxc://example.org/avatar"}},
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
            .respond_with(ResponseTemplate::new(200).set_body_json(root.clone()))
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
            "content":{"msgtype":"m.image","body":"Fresh thread reply","url":"mxc://example.org/photo","info":{"thumbnail_url":"mxc://example.org/preview"},"m.relates_to":{"rel_type":"m.thread","event_id":"$outside","is_falling_back":false}}});
        Mock::given(method("GET"))
            .and(path_regex(".*/relations/.*"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"chunk":[reply.clone()]})),
            )
            .mount(&server)
            .await;
        let initial_reaction = json!({"type":"m.reaction", "event_id":"$reaction-cached", "sender":"@other:example.org", "origin_server_ts":2,
            "content":{"m.relates_to":{"rel_type":"m.annotation", "event_id":"$reply", "key":"📌"}}});
        *response.lock().unwrap() = json!({"next_batch":"two", "rooms":{"join":{"!room:example.org":{"timeline":{"events":[reply, initial_reaction],"limited":false}}}}});
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
        assert_eq!(
            reply.reactions,
            vec![Reaction {
                key: "📌".into(),
                count: 1,
                is_own: false
            }]
        );
        assert!(
            reply
                .image
                .as_ref()
                .is_some_and(|image| image.preview.is_some())
        );
        assert_eq!(
            reply.avatar_url.as_deref().map(|uri| uri.as_str()),
            Some("mxc://example.org/avatar")
        );
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
        let thread = Arc::new(thread);
        let (_, thread_updates) = thread.subscribe().await;
        tokio::pin!(thread_updates);

        async fn expect_reactions(
            timeline: &Arc<Timeline>,
            updates: &mut (impl iced::futures::Stream + Unpin),
            thread: bool,
            expected: &[(&str, Vec<Reaction>)],
        ) {
            let (state, snapshots) = watch::channel(Model::default());
            let (older, _) = mpsc::channel(1);
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    publish(timeline, 1, thread, 1, &History::Ready, &state, &older).await;
                    let model = snapshots.borrow();
                    let pane = if thread { &model.thread } else { &model.room };
                    let rows = &pane.as_ref().unwrap().rows;
                    // Reactions never become standalone message rows.
                    assert!(rows.iter().all(|row| !row.key.starts_with("$reaction")));
                    if expected.iter().all(|(id, reactions)| {
                        rows.iter()
                            .any(|row| row.key == *id && row.reactions == *reactions)
                    }) {
                        break;
                    }
                    drop(model);
                    updates.next().await.unwrap();
                }
            })
            .await
            .unwrap_or_else(|_| {
                let model = snapshots.borrow();
                let pane = if thread { &model.thread } else { &model.room };
                panic!("reaction update timed out (thread={thread}), expected {expected:?}, actual {:?}", pane.as_ref().unwrap().rows.iter().map(|row| (&row.key, &row.reactions)).collect::<Vec<_>>());
            });
        }

        // Opening a thread retains reactions already known to the room cache.
        expect_reactions(
            &thread,
            &mut thread_updates,
            true,
            &[(
                "$reply",
                vec![Reaction {
                    key: "📌".into(),
                    count: 1,
                    is_own: false,
                }],
            )],
        )
        .await;

        let reaction = |id: &str, target: &str, sender: &str, key: &str| {
            json!({
                "type":"m.reaction", "event_id":id, "sender":sender, "origin_server_ts":3,
                "content":{"m.relates_to":{"rel_type":"m.annotation", "event_id":target, "key":key}}
            })
        };
        let redaction = |id: &str, target: &str| {
            json!({
                "type":"m.room.redaction", "event_id":id, "sender":"@other:example.org", "origin_server_ts":4,
                "redacts":target, "content":{"redacts":target}
            })
        };
        // Both Unicode sequences and ordinary text annotation keys are valid.
        // Load the original into the room too: the SDK routes its live reactions
        // there, while reactions on replies also reach the thread timeline.
        let batches = [
            (
                "three",
                vec![
                    root,
                    reaction("$reaction-root", "$outside", "@other:example.org", "❤️"),
                    reaction("$reaction-own", "$reply", "@test:example.org", "👍🏽"),
                    reaction("$reaction-other", "$reply", "@other:example.org", "👍🏽"),
                    reaction("$reaction-text", "$reply", "@other:example.org", "approved"),
                ],
                vec![
                    Reaction {
                        key: "approved".into(),
                        count: 1,
                        is_own: false,
                    },
                    Reaction {
                        key: "👍🏽".into(),
                        count: 2,
                        is_own: true,
                    },
                    Reaction {
                        key: "📌".into(),
                        count: 1,
                        is_own: false,
                    },
                ],
                vec![Reaction {
                    key: "❤️".into(),
                    count: 1,
                    is_own: false,
                }],
            ),
            (
                "four",
                vec![
                    redaction("$remove-root", "$reaction-root"),
                    redaction("$remove-other", "$reaction-other"),
                    redaction("$remove-text", "$reaction-text"),
                    redaction("$remove-cached", "$reaction-cached"),
                ],
                vec![Reaction {
                    key: "👍🏽".into(),
                    count: 1,
                    is_own: true,
                }],
                vec![],
            ),
            (
                "five",
                vec![redaction("$remove-own", "$reaction-own")],
                vec![],
                vec![],
            ),
        ];
        for (batch, events, reply_reactions, root_reactions) in batches {
            *response.lock().unwrap() = json!({"next_batch":batch, "rooms":{"join":{"!room:example.org":{"timeline":{"events":events,"limited":false}}}}});
            client
                .sync_once(SyncSettings::default().timeout(Duration::ZERO))
                .await
                .unwrap();
            expect_reactions(
                &timeline,
                &mut updates,
                false,
                &[
                    ("$reply", reply_reactions.clone()),
                    ("$outside", root_reactions),
                ],
            )
            .await;
            expect_reactions(
                &thread,
                &mut thread_updates,
                true,
                &[("$reply", reply_reactions)],
            )
            .await;
            if batch == "three" {
                // Add-only selection of an own reaction does not send or redact.
                Composer(RuntimeOwned::new(timeline.clone()))
                    .add_reaction("$reply".into(), "👍🏽".into())
                    .await
                    .unwrap();
            }
        }
        let requests = server.received_requests().await.unwrap();
        assert!(!requests.iter().any(|request| request.method == "PUT"));

        Mock::given(method("PUT"))
            .and(path_regex(".*/send/m.reaction/.*"))
            .respond_with(|request: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                if body["m.relates_to"]["key"] == "🛑" {
                    ResponseTemplate::new(403).set_body_json(
                        json!({"errcode":"M_FORBIDDEN","error":"fixture reaction denied"}),
                    )
                } else {
                    ResponseTemplate::new(200).set_body_json(json!({"event_id":"$sent-reaction"}))
                }
            })
            .mount(&server)
            .await;
        let room_composer = Composer(RuntimeOwned::new(timeline.clone()));
        let thread_composer = Composer(RuntimeOwned::new(thread.clone()));
        room_composer
            .clone()
            .add_reaction("$reply".into(), "🚀".into())
            .await
            .unwrap();
        thread_composer
            .clone()
            .add_reaction("$outside".into(), "🦀".into())
            .await
            .unwrap();
        assert!(
            room_composer
                .clone()
                .add_reaction("$reply".into(), "🛑".into())
                .await
                .unwrap_err()
                .contains("fixture reaction denied")
        );
        assert!(
            room_composer
                .clone()
                .add_reaction("$missing".into(), "👍".into())
                .await
                .is_err()
        );
        assert!(
            room_composer
                .clone()
                .add_reaction("$reply".into(), " ".into())
                .await
                .is_err()
        );
        let requests = server.received_requests().await.unwrap();
        let sent: Vec<serde_json::Value> = requests
            .iter()
            .filter(|request| request.method == "PUT")
            .map(|request| {
                assert!(
                    request.url.path().contains("/send/m.reaction/"),
                    "SDK sends reactions as plaintext even in an encrypted room"
                );
                serde_json::from_slice(&request.body).unwrap()
            })
            .collect();
        assert_eq!(sent.len(), 3);
        assert_eq!(
            sent[0],
            json!({"m.relates_to":{"rel_type":"m.annotation","event_id":"$reply","key":"🚀"}})
        );
        assert_eq!(
            sent[1],
            json!({"m.relates_to":{"rel_type":"m.annotation","event_id":"$outside","key":"🦀"}})
        );

        Mock::given(method("PUT"))
            .and(path_regex(".*/redact/.*"))
            .respond_with(|request: &wiremock::Request| {
                if request.url.path().contains("toggle-denied") {
                    ResponseTemplate::new(403).set_body_json(
                        json!({"errcode":"M_FORBIDDEN","error":"fixture redaction denied"}),
                    )
                } else {
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"event_id":"$removed-reaction"}))
                }
            })
            .mount(&server)
            .await;
        let events = vec![
            reaction("$toggle-own-a", "$reply", "@test:example.org", "👍🏽"),
            reaction("$toggle-own-b", "$reply", "@test:example.org", "👍🏽"),
            reaction("$toggle-other", "$reply", "@other:example.org", "👍🏽"),
            reaction("$toggle-root", "$outside", "@test:example.org", "❤️"),
            reaction("$toggle-text", "$reply", "@test:example.org", "approved"),
            reaction("$toggle-denied", "$reply", "@test:example.org", "🛑"),
        ];
        *response.lock().unwrap() = json!({"next_batch":"six", "rooms":{"join":{"!room:example.org":{"timeline":{"events":events,"limited":false}}}}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let expected = vec![
            Reaction {
                key: "approved".into(),
                count: 1,
                is_own: true,
            },
            Reaction {
                key: "👍🏽".into(),
                count: 2,
                is_own: true,
            },
            Reaction {
                key: "🛑".into(),
                count: 1,
                is_own: true,
            },
        ];
        expect_reactions(
            &timeline,
            &mut updates,
            false,
            &[
                ("$reply", expected.clone()),
                (
                    "$outside",
                    vec![Reaction {
                        key: "❤️".into(),
                        count: 1,
                        is_own: true,
                    }],
                ),
            ],
        )
        .await;
        expect_reactions(
            &thread,
            &mut thread_updates,
            true,
            &[("$reply", expected.clone())],
        )
        .await;
        // Remove both duplicate own events, never another sender's annotation.
        thread_composer
            .clone()
            .set_reaction("$reply".into(), "👍🏽".into(), false)
            .await
            .unwrap();
        room_composer
            .clone()
            .set_reaction("$outside".into(), "❤️".into(), false)
            .await
            .unwrap();
        room_composer
            .clone()
            .set_reaction("$reply".into(), "approved".into(), false)
            .await
            .unwrap();
        assert!(
            room_composer
                .clone()
                .set_reaction("$reply".into(), "🛑".into(), false)
                .await
                .unwrap_err()
                .contains("fixture redaction denied")
        );
        // Failure and HTTP success both leave badge updates to SDK sync.
        expect_reactions(&timeline, &mut updates, false, &[("$reply", expected)]).await;
        let requests = server.received_requests().await.unwrap();
        let redactions: Vec<_> = requests
            .iter()
            .filter(|request| request.url.path().contains("/redact/"))
            .collect();
        assert_eq!(redactions.len(), 5);
        for id in [
            "toggle-own-a",
            "toggle-own-b",
            "toggle-root",
            "toggle-text",
            "toggle-denied",
        ] {
            assert!(
                redactions
                    .iter()
                    .any(|request| request.url.path().contains(id))
            );
        }
        assert!(
            !redactions
                .iter()
                .any(|request| request.url.path().contains("toggle-other"))
        );

        let events: Vec<_> = [
            "$toggle-own-a",
            "$toggle-own-b",
            "$toggle-root",
            "$toggle-text",
        ]
        .iter()
        .enumerate()
        .map(|(index, id)| redaction(&format!("$toggle-removal-{index}"), id))
        .collect();
        *response.lock().unwrap() = json!({"next_batch":"seven", "rooms":{"join":{"!room:example.org":{"timeline":{"events":events,"limited":false}}}}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let expected = vec![
            Reaction {
                key: "👍🏽".into(),
                count: 1,
                is_own: false,
            },
            Reaction {
                key: "🛑".into(),
                count: 1,
                is_own: true,
            },
        ];
        expect_reactions(
            &timeline,
            &mut updates,
            false,
            &[("$reply", expected.clone()), ("$outside", vec![])],
        )
        .await;
        expect_reactions(&thread, &mut thread_updates, true, &[("$reply", expected)]).await;
        // Replaying an old remove intent is a no-op rather than adding it back.
        room_composer
            .set_reaction("$reply".into(), "👍🏽".into(), false)
            .await
            .unwrap();
        assert_eq!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|request| request.url.path().contains("/redact/"))
                .count(),
            5
        );
    }

    #[tokio::test]
    async fn account_profile_loads_without_an_open_room_or_profiles_extension() {
        for (body, name, avatar) in [
            (
                json!({"displayname":"Test Person", "avatar_url":"mxc://example.org/account"}),
                "Test Person",
                Some("mxc://example.org/account"),
            ),
            (json!({"displayname":"Test Person"}), "Test Person", None),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/_matrix/client/versions"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/_matrix/client/v3/profile/@test:example.org"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
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
                    serde_json::from_value::<MatrixSession>(json!({
                        "user_id":"@test:example.org", "device_id":"TEST", "access_token":"fake"
                    }))
                    .unwrap(),
                    Default::default(),
                )
                .await
                .unwrap();
            let (state, mut updates) = watch::channel(Model::default());
            let profile = account_profile_updates(&client, &state);
            tokio::pin!(profile);
            tokio::time::timeout(Duration::from_secs(5), async {
                tokio::select! {
                    _ = &mut profile => panic!("profile subscription must remain pending"),
                    result = updates.wait_for(|snapshot| {
                        snapshot.account_name == name
                            && snapshot.account_avatar_url.as_deref().map(|uri| uri.as_str()) == avatar
                    }) => { result.unwrap(); },
                }
            }).await.unwrap();
        }
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
        let mut conversation = room(
            "Conversation",
            false,
            vec![
                event("m.room.topic", "", json!({"topic":"Initial topic"})),
                event(
                    "m.room.pinned_events",
                    "",
                    json!({"pinned":["$cached-pin"]}),
                ),
                event(
                    "m.room.power_levels",
                    "",
                    json!({"users":{"@test:example.org":50},"state_default":100,"events":{"m.room.topic":50,"m.room.pinned_events":50}}),
                ),
            ],
        );
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
                    && s.rooms[0].topic == "Initial topic"
                    && s.rooms[0].can_edit_topic
                    && s.rooms[0].can_pin
                    && s.rooms[0].can_invite
                    && s.rooms[0].pinned_events == ["$cached-pin"]
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
        client.event_cache().subscribe().unwrap();
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap();
        let composer = Composer(RuntimeOwned::new(Arc::new(
            TimelineBuilder::new(&room).build().await.unwrap(),
        )));
        Mock::given(method("PUT"))
            .and(path_regex(".*/state/m.room.topic/?"))
            .and(wiremock::matchers::body_partial_json(
                json!({"topic":"Saved topic"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"event_id":"$topic"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(".*/rooms/[^/]+/invite"))
            .and(wiremock::matchers::body_json(
                json!({"user_id":"@invite:remote.example"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            composer
                .clone()
                .invite_member("not a Matrix ID".into())
                .await
                .is_err()
        );
        assert!(
            composer
                .clone()
                .invite_member("@test:example.org".into())
                .await
                .unwrap_err()
                .contains("already")
        );
        composer
            .clone()
            .invite_member(" @invite:remote.example ".into())
            .await
            .unwrap();
        composer
            .clone()
            .set_topic("Saved topic".into())
            .await
            .unwrap();
        let pins = Arc::new(std::sync::Mutex::new(vec!["$existing".to_owned()]));
        let current = pins.clone();
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.pinned_events/?"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"pinned":current.lock().unwrap().clone()}))
            })
            .mount(&server)
            .await;
        let current = pins.clone();
        Mock::given(method("PUT"))
            .and(path_regex(".*/state/m.room.pinned_events/?"))
            .respond_with(move |request: &wiremock::Request| {
                let content: serde_json::Value = request.body_json().unwrap();
                *current.lock().unwrap() =
                    serde_json::from_value(content["pinned"].clone()).unwrap();
                ResponseTemplate::new(200).set_body_json(json!({"event_id":"$pins"}))
            })
            .expect(3)
            .mount(&server)
            .await;
        composer
            .clone()
            .set_message_pinned("$pin-one".into(), true)
            .await
            .unwrap();
        composer
            .clone()
            .set_message_pinned("$pin-two".into(), true)
            .await
            .unwrap();
        composer
            .clone()
            .set_message_pinned("$pin-two".into(), true)
            .await
            .unwrap();
        assert_eq!(
            *pins.lock().unwrap(),
            vec!["$existing", "$pin-one", "$pin-two"]
        );
        composer
            .clone()
            .set_message_pinned("$pin-one".into(), false)
            .await
            .unwrap();
        composer
            .clone()
            .set_message_pinned("$pin-one".into(), false)
            .await
            .unwrap();
        assert_eq!(*pins.lock().unwrap(), vec!["$existing", "$pin-two"]);
        // Global and room account-data updates must work without a room-list lifecycle transition.
        *response.lock().unwrap() = json!({"next_batch":"two","account_data":{"events":[{"type":"m.direct","content":{}}]},"rooms":{"join":{
            "!room:example.org":{"state":{"events":[event("m.room.name","",json!({"name":"Renamed"})), event("m.room.topic","",json!({"topic":""})), event("m.room.pinned_events","",json!({"pinned":["$existing","$pin-one","$pin-two"]})), event("m.room.power_levels","",json!({"users":{"@test:example.org":50},"invite":100,"state_default":50,"events":{"m.room.topic":100,"m.room.pinned_events":100}}))]},"account_data":{"events":[{"type":"m.tag","content":{"tags":{}}},{"type":"m.marked_unread","content":{"unread":false}}]}},
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
                    && s.rooms[0].topic.is_empty()
                    && !s.rooms[0].can_edit_topic
                    && !s.rooms[0].can_pin
                    && !s.rooms[0].can_invite
                    && s.rooms[0].pinned_events.contains(&"$pin-two".to_owned())
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
            composer
                .clone()
                .invite_member("@forbidden:remote.example".into())
                .await
                .unwrap_err()
                .contains("permission")
        );
        assert!(
            composer
                .clone()
                .set_topic("Must not be sent".into())
                .await
                .unwrap_err()
                .contains("permission")
        );
        assert!(
            composer
                .clone()
                .set_message_pinned("$forbidden".into(), true)
                .await
                .unwrap_err()
                .contains("permission")
        );
        assert!(
            composer
                .clone()
                .set_message_pinned("$existing".into(), false)
                .await
                .unwrap_err()
                .contains("permission")
        );
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
        history_fixture(false, false, 3, 3, true).await;
    }

    #[tokio::test]
    async fn unencrypted_rooms_support_history_threads_sending_and_retry() {
        history_fixture(false, false, 3, 3, false).await;
    }

    #[tokio::test]
    async fn long_threads_stop_eager_loading_and_continue_on_demand() {
        history_fixture(false, false, 1, 12, true).await;
    }

    #[tokio::test]
    async fn shutdown_cancels_pending_initial_history() {
        history_fixture(true, false, 3, 3, true).await;
    }

    #[tokio::test]
    async fn offline_views_recover_and_can_send_without_restarting_ui() {
        history_fixture(false, true, 3, 3, true).await;
    }

    #[tokio::test]
    async fn sqlite_restart_reads_encrypted_history_offline_and_reconciles() {
        cached_restart_fixture(true).await;
    }

    #[tokio::test]
    async fn sqlite_restart_reads_unencrypted_history_offline_and_reconciles() {
        cached_restart_fixture(false).await;
    }

    async fn cached_restart_fixture(encrypted: bool) {
        use matrix_sdk::{config::SyncSettings, ruma::room_id};
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        async fn open(server: &MockServer, directory: &std::path::Path) -> Client {
            let client = Client::builder()
                .cross_process_store_config(
                    matrix_sdk_common::cross_process_lock::CrossProcessLockConfig::SingleProcess,
                )
                .homeserver_url(server.uri())
                .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
                .with_threading_support(matrix_sdk::ThreadingSupport::Enabled {
                    with_subscriptions: false,
                })
                .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
                .sqlite_store(directory, Some("test-only-cache-passphrase"))
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
            client
        }

        let directory = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let online = Arc::new(AtomicBool::new(true));
        let available = online.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(move |_: &wiremock::Request| {
                if available.load(Ordering::SeqCst) {
                    ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]}))
                } else {
                    ResponseTemplate::new(503)
                }
            })
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(".*/keys/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "device_keys":{}, "one_time_key_counts":{"signed_curve25519":50}
            })))
            .mount(&server)
            .await;
        let member = json!({"type":"m.room.member", "state_key":"@test:example.org",
            "sender":"@test:example.org", "event_id":"$member", "origin_server_ts":1,
            "content":{"membership":"join"}});
        let mut required_state = vec![
            member.clone(),
            json!({"type":"m.room.name", "state_key":"", "sender":"@test:example.org",
                "event_id":"$name", "origin_server_ts":1, "content":{"name":"Cached room"}}),
        ];
        if encrypted {
            required_state.push(json!({"type":"m.room.encryption", "state_key":"",
                "sender":"@test:example.org", "event_id":"$encryption", "origin_server_ts":1,
                "content":{"algorithm":"m.megolm.v1.aes-sha2"}}));
        }
        Mock::given(method("GET"))
            .and(path_regex(".*/state/m.room.encryption/?"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "errcode":"M_NOT_FOUND", "error":"No encryption event"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(".*/members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"chunk":[member]})))
            .mount(&server)
            .await;
        let initial = Arc::new(std::sync::Mutex::new(json!({"next_batch":"seed",
            "rooms":{"join":{"!room:example.org":{
                "state":{"events":required_state},
                "timeline":{"events":[], "limited":false, "prev_batch":"older"}
            }}}
        })));
        let seed = initial.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(seed.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        let send_count = AtomicUsize::new(0);
        Mock::given(method("PUT"))
            .and(path_regex(".*/send/.*"))
            .respond_with(move |_: &wiremock::Request| {
                let id = if send_count.fetch_add(1, Ordering::SeqCst) == 0 {
                    "$cached"
                } else {
                    "$fresh"
                };
                ResponseTemplate::new(200).set_body_json(json!({"event_id":id}))
            })
            .mount(&server)
            .await;

        let first = open(&server, directory.path()).await;
        first.event_cache().subscribe().unwrap();
        first
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let room = first.get_room(room_id!("!room:example.org")).unwrap();
        let timeline = TimelineBuilder::new(&room).build().await.unwrap();
        let (_, updates) = timeline.subscribe().await;
        let mut updates = Box::pin(updates);
        for body in ["Cached message", "Arrived after restart"] {
            room.send(
                matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(body),
            )
            .await
            .unwrap();
        }
        let requests = server.received_requests().await.unwrap();
        let events: Vec<_> = requests
            .iter()
            .filter(|r| r.url.path().contains("/send/"))
            .enumerate()
            .map(|(index, request)| {
                let content: serde_json::Value = request.body_json().unwrap();
                if encrypted {
                    assert!(content.get("body").is_none());
                }
                json!({"type":if encrypted {"m.room.encrypted"} else {"m.room.message"},
                    "event_id":if index == 0 {"$cached"} else {"$fresh"},
                    "sender":"@test:example.org", "origin_server_ts":index + 2,
                    "content":content})
            })
            .collect();
        assert_eq!(events.len(), 2);
        *initial.lock().unwrap() = json!({"next_batch":"cached", "rooms":{"join":{
            "!room:example.org":{"timeline":{"events":[events[0]], "limited":false}}
        }}});
        first
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if timeline.items().await.iter().any(|item| {
                    item.as_event().is_some_and(|event| {
                        event.event_id().is_some_and(|id| id.as_str() == "$cached")
                    })
                }) {
                    break;
                }
                updates.next().await.unwrap();
            }
        })
        .await
        .unwrap();
        // Release all first-client handles; the next client must read SQLite.
        drop(updates);
        drop(timeline);
        drop(room);
        drop(first);
        online.store(false, Ordering::SeqCst);
        let cached_event = events[0].clone();
        Mock::given(method("GET"))
            .and(path_regex(".*/messages"))
            .respond_with(ResponseTemplate::new(503).set_delay(Duration::from_secs(60)))
            .mount(&server)
            .await;
        let available = online.clone();
        Mock::given(method("POST"))
            .and(path("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"))
            .respond_with(move |_: &wiremock::Request| {
                if !available.load(Ordering::SeqCst) {
                    return ResponseTemplate::new(503);
                }
                ResponseTemplate::new(200).set_delay(Duration::from_millis(50)).set_body_json(json!({
                    "pos":"reconnected", "lists":{"all_rooms":{"count":1,"ops":[
                        {"op":"SYNC","range":[0,0],"room_ids":["!room:example.org"]}
                    ]}}, "rooms":{"!room:example.org":{"name":"Cached room", "initial":true,
                        "required_state":required_state, "timeline":events, "prev_batch":"older", "num_live":1}},
                    "extensions":{"e2ee":{"device_one_time_keys_count":{"signed_curve25519":50}}}
                }))
            })
            .mount(&server)
            .await;

        let restored = open(&server, directory.path()).await;
        if encrypted {
            // Decrypt the original ciphertext with the reopened store, rather
            // than relying only on already-decrypted cached timeline content.
            let event =
                matrix_sdk::ruma::serde::Raw::from_json_string(cached_event.to_string()).unwrap();
            let decrypted = restored
                .get_room(room_id!("!room:example.org"))
                .unwrap()
                .decrypt_event(&event, None)
                .await
                .unwrap();
            let content: serde_json::Value =
                serde_json::from_str(decrypted.raw().json().get()).unwrap();
            assert_eq!(content["content"]["body"], "Cached message");
        }
        let sync = build_sync(restored.clone()).await.unwrap();
        let (control, mut selection) = watch::channel(Selection {
            room: Some((1, "!room:example.org".into())),
            ..Default::default()
        });
        let (state, mut snapshots) = watch::channel(Model::default());
        let worker = tokio::spawn(async move {
            let result = connected(&restored, &sync, &mut selection, &state).await;
            crate::sso::stop_with_deadline(sync.stop(), Duration::from_secs(5))
                .await
                .unwrap();
            result.unwrap();
        });
        tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|model| {
                model.rooms.iter().any(|r| r.name == "Cached room")
                    && model.room.as_ref().is_some_and(|pane| {
                        pane.rows.iter().any(|r| {
                            r.event_id.as_deref() == Some("$cached") && r.body == "Cached message"
                        })
                    })
            }),
        )
        .await
        .unwrap()
        .unwrap();
        tokio::time::timeout(
            Duration::from_secs(15),
            snapshots.wait_for(|s| s.status.starts_with("Offline")),
        )
        .await
        .unwrap()
        .unwrap();
        online.store(true, Ordering::SeqCst);
        let model = tokio::time::timeout(
            Duration::from_secs(15),
            snapshots.wait_for(|model| {
                model.status == "Connected"
                    && model.room.as_ref().is_some_and(|pane| {
                        pane.rows.iter().any(|r| {
                            r.event_id.as_deref() == Some("$fresh")
                                && r.body == "Arrived after restart"
                        })
                    })
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        let pane = model.room.unwrap();
        assert_eq!(
            pane.rows
                .iter()
                .filter(|r| r.event_id.as_deref() == Some("$cached"))
                .count(),
            1
        );
        assert!(pane.error.is_none());
        control.send_modify(|s| s.shutdown = true);
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn missing_profile_offers_login_without_network_and_can_close() {
        let parent = tempfile::tempdir().unwrap();
        let directory = parent.path().join("new-profile");
        let (control, selection) = watch::channel(Selection::default());
        let (state, mut snapshots) = watch::channel(Model::default());
        let (_requests, login) = mpsc::channel(1);
        let path = directory.to_str().unwrap().to_owned();
        let worker = tokio::spawn(run(path, selection, state, login));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                snapshots.changed().await.unwrap();
                if matches!(
                    snapshots.borrow().authentication,
                    Some(Authentication::SignIn { .. })
                ) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(!directory.join("store").exists());
        control.send_modify(|s| s.shutdown = true);
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(snapshots.borrow().rooms.is_empty());
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
        assert!(error.is::<SignInRequired>());
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

    // SDK pagination completion and timeline-content publication are separate
    // events. Await the actual content, including when parsing yields to a worker.
    async fn wait_for_row(
        snapshots: &mut watch::Receiver<Model>,
        thread: bool,
        body: &str,
    ) -> Pane {
        let model = tokio::time::timeout(
            Duration::from_secs(5),
            snapshots.wait_for(|model| {
                let pane = if thread { &model.thread } else { &model.room };
                pane.as_ref()
                    .is_some_and(|pane| pane.rows.iter().any(|row| row.body == body))
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        if thread {
            model.thread.unwrap()
        } else {
            model.room.unwrap()
        }
    }

    async fn history_fixture(
        cancel_history: bool,
        reconnect: bool,
        cycles: u64,
        thread_pages: usize,
        encrypted: bool,
    ) {
        let server = MockServer::start().await;
        let online = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let versions_online = online.clone();
        let sync_online = online.clone();
        let root = json!({"type":"m.room.message", "event_id":"$root", "sender":"@test:example.org", "origin_server_ts":1, "content":{"msgtype":"m.text", "body":"fixture root", "format":"org.matrix.custom.html", "formatted_body":"<p><b>fixture root</b></p>"}});
        let mut required_state = vec![
            json!({"type":"m.room.member", "state_key":"@test:example.org", "sender":"@test:example.org", "event_id":"$member", "origin_server_ts":0,"content":{"membership":"join"}}),
        ];
        if encrypted {
            required_state.push(json!({"type":"m.room.encryption","state_key":"","sender":"@test:example.org","event_id":"$encryption","origin_server_ts":0,"content":{"algorithm":"m.megolm.v1.aes-sha2"}}));
        }
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
                    "required_state":required_state, "timeline":[],"prev_batch":"older","num_live":0}},
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
        let send_type = if encrypted {
            "m.room.encrypted"
        } else {
            "m.room.message"
        };
        Mock::given(method("PUT"))
            .and(path_regex(format!(".*/send/{send_type}/.*")))
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
                        p.generation == generation
                            && !matches!(p.history, History::Loading)
                            && p.rows.iter().any(|row| row.body == "fixture root")
                    })
                }),
            )
            .await
            .unwrap()
            .unwrap()
            .clone();
            let pane = model.room.unwrap();
            assert!(pane.error.is_none(), "{:?}", pane.error);
            let root_document = pane
                .rows
                .iter()
                .find(|r| r.body == "fixture root")
                .unwrap()
                .formatted
                .clone()
                .expect("formatted SDK content reaches the UI snapshot");
            assert!(root_document.blocks[0].runs[0].style.bold);
            if generation == 1 {
                request_page(&mut snapshots, false).await;
                let page = wait_for_row(&mut snapshots, false, "hidden reply").await;
                assert!(matches!(page.history, History::Ready));
                let cached = page
                    .rows
                    .iter()
                    .find(|r| r.body == "fixture root")
                    .unwrap()
                    .formatted
                    .as_ref()
                    .unwrap();
                assert!(
                    Arc::ptr_eq(&root_document, cached),
                    "unchanged HTML reuses its parsed document"
                );
                let reply = page.rows.iter().find(|r| r.body == "hidden reply").unwrap();
                assert_eq!(reply.thread_root.as_deref(), Some("$root"));
                assert!(!reply.is_live, "paginated replies must not count as new");
                assert!(reply.is_own);
                assert!(
                    page.load_older.is_some(),
                    "a reply-only page must allow further pagination"
                );
                request_page(&mut snapshots, false).await;
                let page = wait_for_row(&mut snapshots, false, "older visible message").await;
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
                    .send("room **send** fixture".into())
                    .await
                    .unwrap();
                let failed = tokio::time::timeout(
                    Duration::from_secs(10),
                    snapshots.wait_for(|s| {
                        s.room.as_ref().is_some_and(|p| {
                            p.rows
                                .iter()
                                .any(|r| r.body == "room **send** fixture" && r.retry.is_some())
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
                            p.rows.iter().any(|r| {
                                r.body == "room **send** fixture" && r.send_state == "Sent"
                            })
                        })
                    }),
                )
                .await
                .unwrap()
                .unwrap();
                let thread_composer = thread_pane.composer.clone().unwrap();
                let decrypt_room = thread_composer.0.room().clone();
                thread_composer
                    .send("thread **send** fixture".into())
                    .await
                    .unwrap();
                tokio::time::timeout(
                    Duration::from_secs(10),
                    snapshots.wait_for(|s| {
                        s.thread.as_ref().is_some_and(|p| {
                            p.rows.iter().any(|r| {
                                r.body == "thread **send** fixture" && r.send_state == "Sent"
                            })
                        })
                    }),
                )
                .await
                .unwrap()
                .unwrap();
                let requests = server.received_requests().await.unwrap();
                let sends: Vec<_> = requests
                    .iter()
                    .filter(|r| r.url.path().contains("/send/"))
                    .collect();
                assert_eq!(
                    sends.len(),
                    3,
                    "failed send, same-transaction retry, then thread send"
                );
                assert_eq!(sends[0].url.path(), sends[1].url.path());
                for (request, threaded) in [(sends[1], false), (sends[2], true)] {
                    assert!(request.url.path().contains(&format!("/send/{send_type}/")));
                    let content: serde_json::Value = request.body_json().unwrap();
                    let content = if encrypted {
                        assert!(
                            content.get("body").is_none(),
                            "message must be encrypted on the wire"
                        );
                        let event = matrix_sdk::ruma::serde::Raw::from_json_string(json!({"type":"m.room.encrypted", "event_id":if threaded {"$sent2"} else {"$sent1"}, "sender":"@test:example.org", "origin_server_ts":2, "content":content}).to_string()).unwrap();
                        let decrypted = decrypt_room.decrypt_event(&event, None).await.unwrap();
                        let json: serde_json::Value =
                            serde_json::from_str(decrypted.raw().json().get()).unwrap();
                        json["content"].clone()
                    } else {
                        content
                    };
                    assert_eq!(
                        content["body"],
                        if threaded {
                            "thread **send** fixture"
                        } else {
                            "room **send** fixture"
                        }
                    );
                    assert_eq!(content["format"], "org.matrix.custom.html");
                    assert!(
                        content["formatted_body"]
                            .as_str()
                            .unwrap()
                            .contains("<strong>send</strong>")
                    );
                    if threaded {
                        assert_eq!(content["m.relates_to"]["rel_type"], "m.thread");
                        assert_eq!(content["m.relates_to"]["event_id"], "$root");
                    } else {
                        assert!(content.get("m.relates_to").is_none());
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
        history_fixture(false, false, 100, 1, true).await;
    }
}

#[cfg(test)]
#[path = "connected/attachments_tests.rs"]
mod attachments_tests;
