#![recursion_limit = "256"]
#[path = "interconnect/attachments.rs"]
mod attachments;
#[path = "interconnect/bookmarks.rs"]
mod bookmarks;
#[path = "interconnect/composer_picker.rs"]
mod composer_picker;
#[path = "interconnect/context_menu.rs"]
mod context_menu;
#[path = "interconnect/create_room.rs"]
mod create_room;
#[path = "interconnect/demo.rs"]
mod demo;
#[path = "interconnect/downloads.rs"]
mod downloads;
#[path = "interconnect/emoji.rs"]
mod emoji;
#[path = "interconnect/emoji_completion.rs"]
mod emoji_completion;
#[path = "interconnect/emoji_picker.rs"]
mod emoji_picker;
#[path = "interconnect/file_drop.rs"]
mod file_drop;
#[path = "interconnect/formatted.rs"]
mod formatted;
#[path = "interconnect/images.rs"]
mod images;
#[path = "interconnect/inline_image.rs"]
mod inline_image;
#[path = "interconnect/invite.rs"]
mod invite;
#[path = "interconnect/login.rs"]
mod login;
#[path = "interconnect/markdown_composer.rs"]
mod markdown_composer;
#[path = "interconnect/members.rs"]
mod members;
#[path = "interconnect/new_chat.rs"]
mod new_chat;
#[path = "interconnect/people_picker.rs"]
mod people_picker;
#[path = "interconnect/pin_action.rs"]
mod pin_action;
#[path = "interconnect/pinned.rs"]
mod pinned;
#[path = "interconnect/popover.rs"]
mod popover;
#[path = "interconnect/presentation.rs"]
mod presentation;
#[path = "interconnect/reaction_picker.rs"]
mod reaction_picker;
#[path = "interconnect/reactions.rs"]
mod reactions;
#[path = "interconnect/receipts.rs"]
mod receipts;
#[path = "interconnect/room_picker.rs"]
mod room_picker;
#[path = "interconnect/section_reorder.rs"]
mod section_reorder;
#[path = "interconnect/selectable.rs"]
mod selectable;
#[path = "interconnect/selection.rs"]
mod selection;
#[path = "interconnect/settings.rs"]
mod settings;
#[path = "interconnect/shell.rs"]
mod shell;
#[path = "interconnect/thread_activity.rs"]
mod thread_activity;
#[path = "interconnect/timeline.rs"]
mod timeline;
#[path = "interconnect/topic.rs"]
mod topic;
#[path = "interconnect/typing.rs"]
mod typing;
#[path = "interconnect/verification.rs"]
mod verification;

use iced::{
    Element, Length, Subscription, Task,
    futures::SinkExt,
    widget::{button, column, container, row, scrollable, text, text_editor},
};
use interconnect::connected::{History, MessageRow, Model, Pane, RetrySend, RowKind, Selection};
use interconnect::navigation::Scope;
use interconnect::navigation_preferences::Preferences;
use std::collections::{HashMap, HashSet};
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DraftKey {
    room: String,
    root: Option<String>,
}
#[derive(Default)]
struct Draft {
    markdown: markdown_composer::State,
    content: text_editor::Content,
    attachment: Option<interconnect::attachment::Attachment>,
    pending: bool,
    error: Option<String>,
}

const MAX_PENDING_OPERATIONS: usize = 8;

struct App {
    context_menu: Option<context_menu::State>,
    manual_unread_generation: Option<u64>,
    section_reorder: section_reorder::State,
    bookmarks: bookmarks::State,
    settings: settings::State,
    login: login::State,
    verification: verification::State,
    profile: String,
    account_id: Option<String>,
    model: Model,
    selection: Selection,
    control: Option<watch::Sender<Selection>>,
    generation: u64,
    closing: bool,
    stopped: bool,
    drafts: HashMap<DraftKey, Draft>,
    retrying: HashSet<String>,
    retry_errors: HashMap<String, String>,
    pending_ops: usize,
    composing: bool,
    shell: shell::State,
    demo: Option<demo::Demo>,
    preferences_ready: bool,
    pending_navigation: Option<Preferences>,
    scroll_positions: HashMap<DraftKey, f32>,
    pending_scroll: [Option<f32>; 2],
    timeline_metrics: [Option<(u64, u64, timeline::Metrics)>; 2],
    thread_activity: thread_activity::State,
    window_focused: bool,
    window_width: Option<f32>,
    thread_scroll_target: Option<(u64, f32)>,
    visible_rows: [Vec<String>; 2],
    inline_demand: inline_image::Demand,
    local_inline: inline_image::LocalImages,
    image_view: Option<images::Viewing>,
    downloads: downloads::State,
    reaction_picker: Option<reaction_picker::State>,
    reaction_sends: HashSet<reaction_picker::Sending>,
    reaction_error: Option<(reaction_picker::Target, String)>,
    composer_picker: Option<composer_picker::State>,
    attachment_picker: Option<attachments::Target>,
    clipboard_paste: Option<attachments::Paste>,
    emoji_completion: emoji_completion::State,
    topic_editor: Option<topic::State>,
    members: members::State,
    invite: Option<invite::State>,
    inviting: HashSet<String>,
    pinning: HashSet<reaction_picker::Target>,
    pin_error: Option<(reaction_picker::Target, String)>,
    room_picker: Option<room_picker::State>,
    new_chat: Option<new_chat::State>,
    create_room: Option<create_room::State>,
    create_room_request: Option<u64>,
    pending_created_space: Option<(String, String)>,
    direct_request: Option<u64>,
    pending_created_room: Option<interconnect::navigation::RoomEntry>,
}
#[derive(Clone, Debug)]
enum Message {
    ContextMenu(context_menu::Event),
    RoomSectionMoved(Option<String>, u64, Result<(), String>),
    RoomMarkedUnread(Option<String>, u64, Result<(), String>),
    SectionChanged(Option<String>, u64, Result<(), String>),
    Reorder(section_reorder::Event),
    SectionsReordered(Option<String>, u64, Result<(), String>),
    OpenMessageLink(String),
    LinkOpened,
    Bookmark(bookmarks::Event),
    QuickRoom(room_picker::Event),
    NewChat(new_chat::Event),
    CreateRoom(create_room::Event),
    Pin(pin_action::Event),
    Topic(topic::Event),
    Members(members::Event),
    Invite(invite::Event),
    Login(login::Event),
    Verification(verification::Event),
    Settings(settings::Event),
    Image(images::Event),
    Download(downloads::Event),
    Reaction(reaction_picker::Event),
    ComposerEmoji(composer_picker::Event),
    Attachment(attachments::Event),
    EmojiCompletion(emoji_completion::Event),
    Space(Scope),
    Section(String),
    Search(String),
    SearchFocus,
    SearchOpen,
    AdjacentRoom(bool),
    OpenLastThread,
    ToggleNavigation,
    Focus(bool),
    Escape,
    Control(watch::Sender<Selection>),
    State(Box<Model>),
    InlineImagesDemand {
        context: inline_image::Context,
        row: String,
        document: std::sync::Arc<interconnect::message_document::Document>,
        keys: Vec<interconnect::image_media::Key>,
    },
    RowsVisible {
        thread: bool,
        generation: u64,
        keys: Vec<String>,
    },
    Stopped(Result<(), String>),
    Room(String),
    Thread(String),
    OpenThreadLatest(String),
    DemoThreadReply,
    WindowFocused(bool),
    WindowResized(f32),
    TimelineVisible {
        thread: bool,
        generation: u64,
        keys: Vec<String>,
        metrics: timeline::Metrics,
    },
    CloseThread,
    JumpToNewest {
        thread: bool,
        generation: u64,
    },
    Scrolled {
        thread: bool,
        generation: u64,
        offset: f32,
    },
    TimelineMeasured {
        thread: bool,
        generation: u64,
        revision: u64,
        metrics: timeline::Metrics,
    },
    LoadOlder {
        thread: bool,
        generation: u64,
        revision: u64,
    },
    Close,
    Exit,
    Markdown(markdown_composer::Event),
    Edit(DraftKey, text_editor::Action),
    Send(DraftKey),
    Sent(DraftKey, Option<String>, Result<(), String>),
    Retry(RetrySend),
    Retried(String, Result<(), String>),
    Composing(bool),
}
impl App {
    fn new() -> Self {
        let mut app = Self {
            bookmarks: bookmarks::State::default(),
            settings: settings::State::default(),
            login: login::State::default(),
            verification: verification::State::default(),
            profile: String::new(),
            account_id: None,
            model: Model::default(),
            selection: Selection::default(),
            control: None,
            generation: 0,
            closing: false,
            stopped: false,
            drafts: HashMap::new(),
            retrying: HashSet::new(),
            retry_errors: HashMap::new(),
            pending_ops: 0,
            composing: false,
            shell: shell::State::default(),
            context_menu: None,
            manual_unread_generation: None,
            section_reorder: Default::default(),
            demo: None,
            preferences_ready: false,
            pending_navigation: None,
            scroll_positions: HashMap::new(),
            pending_scroll: [None, None],
            timeline_metrics: [None, None],
            thread_activity: thread_activity::State::default(),
            window_focused: false,
            window_width: None,
            thread_scroll_target: None,
            visible_rows: Default::default(),
            inline_demand: HashMap::new(),
            local_inline: Default::default(),
            image_view: None,
            downloads: downloads::State::default(),
            reaction_picker: None,
            reaction_sends: HashSet::new(),
            reaction_error: None,
            composer_picker: None,
            attachment_picker: None,
            clipboard_paste: None,
            emoji_completion: emoji_completion::State::default(),
            topic_editor: None,
            members: members::State::default(),
            invite: None,
            inviting: HashSet::new(),
            pinning: HashSet::new(),
            pin_error: None,
            room_picker: None,
            new_chat: None,
            create_room: None,
            create_room_request: None,
            pending_created_space: None,
            direct_request: None,
            pending_created_room: None,
        };
        if std::env::args().any(|arg| arg == "--demo") {
            app.model.authentication = None;
            app.model.bookmarks.complete = true;
            app.demo = Some(demo::Demo::default());
            app.model.rooms = demo::rooms().into();
            app.model.sections = demo::sections().into();
            app.model.spaces = demo::spaces().into();
            app.shell.space = Scope::Space("studio-space".into());
            app.model.status = "Design preview · local sample data".into();
            app.model.images = demo::images();
            app.local_inline = demo::inline_assets();
            let _ = app.update(Message::Room("studio".into()));
        }
        app
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        interconnect::diagnostics::update();
        if let Message::ContextMenu(event) = message {
            let _ = self.reorder_event(section_reorder::Event::Cancel);
            return self.context_menu_event(event);
        }
        if matches!(
            message,
            Message::Bookmark(bookmarks::Event::Open)
                | Message::Space(_)
                | Message::Search(_)
                | Message::Section(_)
                | Message::Room(_)
                | Message::Escape
                | Message::WindowFocused(false)
                | Message::WindowResized(_)
                | Message::Close
                | Message::Exit
        ) {
            let _ = self.reorder_event(section_reorder::Event::Cancel);
        }
        if self.context_menu.is_some() && matches!(message, Message::Escape) {
            self.context_menu = None;
            return Task::none();
        }
        if matches!(
            message,
            Message::Room(_)
                | Message::Thread(_)
                | Message::CloseThread
                | Message::Space(_)
                | Message::Section(_)
                | Message::Search(_)
                | Message::SearchFocus
                | Message::AdjacentRoom(_)
                | Message::OpenLastThread
                | Message::OpenThreadLatest(_)
                | Message::ToggleNavigation
                | Message::WindowFocused(false)
                | Message::WindowResized(_)
                | Message::Close
                | Message::Exit
                | Message::Stopped(_)
                | Message::QuickRoom(_)
                | Message::NewChat(_)
                | Message::CreateRoom(_)
                | Message::Settings(_)
                | Message::Verification(_)
                | Message::Image(_)
                | Message::Reaction(_)
                | Message::ComposerEmoji(_)
                | Message::Invite(_)
                | Message::Topic(_)
                | Message::Members(_)
                | Message::Bookmark(bookmarks::Event::Open)
        ) {
            self.context_menu = None;
        }
        if let Message::QuickRoom(event) = message {
            let task = self.quick_room_event(event);
            self.publish_notification_context();
            return task;
        }
        if self.create_room.is_some() && matches!(message, Message::Escape) {
            return self.update(Message::CreateRoom(create_room::Event::Close));
        }
        if self.new_chat.is_some() && matches!(message, Message::Escape) {
            return self.update(Message::NewChat(new_chat::Event::Close));
        }
        if self.room_picker.is_some() && matches!(message, Message::Escape) {
            return self.update(Message::QuickRoom(room_picker::Event::Close));
        }
        if (self.room_picker.is_some() || self.new_chat.is_some() || self.create_room.is_some())
            && matches!(
                message,
                Message::Settings(settings::Event::Open)
                    | Message::Verification(verification::Event::Open)
            )
        {
            return Task::none();
        }
        if matches!(
            &message,
            Message::Room(_)
                | Message::Thread(_)
                | Message::CloseThread
                | Message::Close
                | Message::Exit
                | Message::Send(_)
                | Message::WindowFocused(false)
                | Message::SearchFocus
                | Message::Focus(_)
                | Message::Settings(settings::Event::Open)
                | Message::Verification(verification::Event::Open)
        ) {
            self.stop_typing();
        }
        if let Message::Verification(event) = message {
            let task = self.verification_update(event);
            self.publish_notification_context();
            return task;
        }
        if let Message::Login(event) = message {
            return self.login_update(event);
        }
        if let Message::Settings(event) = message {
            if (self.image_view.is_some() || self.verification.open)
                && matches!(event, settings::Event::Open)
            {
                return Task::none();
            }
            if matches!(event, settings::Event::Open) {
                self.reaction_picker = None;
                self.composer_picker = None;
                self.emoji_completion.clear();
            }
            let task = self.settings_update(event);
            self.publish_notification_context();
            return task;
        }
        if matches!(message, Message::Close | Message::Exit) && self.defer_close_for_settings() {
            return Task::none();
        }
        if matches!(
            message,
            Message::Room(_)
                | Message::Thread(_)
                | Message::CloseThread
                | Message::Members(_)
                | Message::AdjacentRoom(_)
                | Message::OpenLastThread
                | Message::OpenThreadLatest(_)
                | Message::SearchFocus
                | Message::Focus(_)
                | Message::Close
                | Message::Exit
                | Message::Send(_)
                | Message::WindowFocused(false)
        ) {
            self.emoji_completion.clear();
        }
        if matches!(message, Message::Escape)
            && let Some(state) = &self.composer_picker
        {
            return self.update(state.dismiss());
        }
        if matches!(message, Message::Escape)
            && let Some(state) = &self.reaction_picker
        {
            return self.update(state.dismiss());
        }
        if matches!(message, Message::Escape)
            && let Some(dismiss) = self.emoji_completion.dismiss()
        {
            return self.update(dismiss);
        }
        if self.create_room.is_some()
            || self.new_chat.is_some()
            || self.room_picker.is_some()
            || self.settings.draft.is_some()
            || self.verification.open
            || self.image_view.is_some()
            || self.reaction_picker.is_some()
            || self.composer_picker.is_some()
        {
            if matches!(message, Message::Escape) {
                if self.verification.open {
                    return self.update(Message::Verification(verification::Event::Close));
                }
                if self.image_view.is_some() {
                    return self.update(Message::Image(images::Event::Close));
                }
                return self.update(Message::Settings(settings::Event::Cancel));
            }
            if matches!(
                message,
                Message::Space(_)
                    | Message::Section(_)
                    | Message::Search(_)
                    | Message::SearchFocus
                    | Message::SearchOpen
                    | Message::AdjacentRoom(_)
                    | Message::OpenLastThread
                    | Message::ToggleNavigation
                    | Message::Focus(_)
                    | Message::Bookmark(
                        bookmarks::Event::Open
                            | bookmarks::Event::Remove(_)
                            | bookmarks::Event::Conversation(_)
                            | bookmarks::Event::More
                            | bookmarks::Event::Previous
                            | bookmarks::Event::Refresh
                    )
                    | Message::Room(_)
                    | Message::Thread(_)
                    | Message::OpenThreadLatest(_)
                    | Message::CloseThread
                    | Message::Members(_)
                    | Message::Invite(
                        invite::Event::Open
                            | invite::Event::Picker(
                                _,
                                people_picker::Event::Edit(_)
                                    | people_picker::Event::Select(_)
                                    | people_picker::Event::Retry
                                    | people_picker::Event::Submit
                            )
                            | invite::Event::Submit
                            | invite::Event::Cancel
                    )
                    | Message::Edit(..)
                    | Message::Markdown(
                        markdown_composer::Event::Mode(_) | markdown_composer::Event::Preview(_)
                    )
                    | Message::Send(_)
                    | Message::Retry(_)
                    | Message::JumpToNewest { .. }
                    | Message::Scrolled { .. }
                    | Message::TimelineVisible { .. }
                    | Message::RowsVisible { .. }
            ) {
                return Task::none();
            }
        }
        if matches!(message, Message::Escape) && self.invite.is_some() {
            return self.update(Message::Invite(invite::Event::Cancel));
        }
        if matches!(message, Message::Escape) && self.topic_editor.is_some() {
            return self.update(Message::Topic(topic::Event::Cancel));
        }
        if matches!(message, Message::Escape) && self.bookmarks.open {
            return self.update(Message::Bookmark(bookmarks::Event::Close));
        }
        if matches!(
            &message,
            Message::Space(_) | Message::Room(_) | Message::Section(_)
        ) {
            self.pending_navigation = None;
        }

        if self.bookmarks.open
            && matches!(
                message,
                Message::Edit(..)
                    | Message::Send(_)
                    | Message::LoadOlder { .. }
                    | Message::TimelineMeasured { .. }
                    | Message::RowsVisible { .. }
                    | Message::Scrolled { .. }
                    | Message::Pin(pin_action::Event::Set(..))
                    | Message::Reaction(
                        reaction_picker::Event::Open(_) | reaction_picker::Event::Chip { .. }
                    )
            )
        {
            return Task::none();
        }
        let navigation = matches!(
            &message,
            Message::Room(_)
                | Message::Thread(_)
                | Message::CloseThread
                | Message::Members(
                    members::Event::Open | members::Event::Close | members::Event::Retry
                )
        );
        let observe_threads =
            navigation || matches!(&message, Message::State(_) | Message::DemoThreadReply);
        let opening_image = matches!(&message, Message::Image(images::Event::Open { .. }));
        let closing_image = matches!(&message, Message::Image(images::Event::Close));
        let refresh_media = matches!(
            &message,
            Message::Members(_) | Message::Invite(_) | Message::NewChat(_) | Message::Bookmark(_)
        ) || navigation
            || matches!(
                &message,
                Message::State(_)
                    | Message::InlineImagesDemand { .. }
                    | Message::RowsVisible { .. }
                    | Message::Close
                    | Message::Stopped(_)
                    | Message::Image(
                        images::Event::Open { .. } | images::Event::Close | images::Event::Retry
                    )
            );
        let mut action_task = Task::none();
        match message {
            Message::OpenMessageLink(link) => {
                if self.closing || self.stopped {
                    return Task::none();
                }
                if let Some(link) = interconnect::message_document::safe_link(&link) {
                    return Task::perform(
                        async move {
                            #[cfg(target_os = "macos")]
                            let program = "open";
                            #[cfg(not(target_os = "macos"))]
                            let program = "xdg-open";
                            let _ = tokio::process::Command::new(program)
                                .arg(link)
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .kill_on_drop(true)
                                .status()
                                .await;
                        },
                        |_| Message::LinkOpened,
                    );
                }
            }
            Message::LinkOpened => {}

            Message::CreateRoom(event) => action_task = self.create_room_event(event),
            Message::Bookmark(event) => action_task = self.bookmark_event(event),
            Message::NewChat(event) => action_task = self.new_chat_event(event),
            Message::Pin(event) => action_task = self.pin_event(event),
            Message::Topic(event) => action_task = self.topic_event(event),
            Message::Members(event) => action_task = self.members_event(event),
            Message::Invite(event) => action_task = self.invite_event(event),
            Message::Reaction(event) => action_task = self.reaction_event(event),
            Message::Attachment(event) => action_task = self.attachment_event(event),
            Message::ComposerEmoji(event) => action_task = self.composer_picker_event(event),
            Message::EmojiCompletion(event) => action_task = self.emoji_completion_event(event),
            Message::Image(event) => self.image_event(event),
            Message::Download(event) => action_task = self.download_event(event),
            Message::InlineImagesDemand {
                context,
                row,
                document,
                mut keys,
            } => {
                let selected = if context.thread {
                    &self.selection.thread
                } else {
                    &self.selection.room
                };
                if !selected
                    .as_ref()
                    .is_some_and(|(generation, _)| *generation == context.generation)
                {
                    return Task::none();
                }
                let pane = if context.thread {
                    &self.model.thread
                } else {
                    &self.model.room
                };
                let current = current_pane(pane, context.generation)
                    .and_then(|pane| pane.rows.iter().find(|message| message.key == row))
                    .or_else(|| {
                        context
                            .thread
                            .then(|| self.thread_original())
                            .flatten()
                            .filter(|message| message.key == row)
                    });
                if current
                    .and_then(|row| row.formatted.as_ref())
                    .is_some_and(|current| std::sync::Arc::ptr_eq(current, &document))
                {
                    let allowed = document.image_keys();
                    keys.retain(|key| allowed.contains(key));
                    keys.truncate(interconnect::image_media::MAX_INLINE_IMAGES);
                    self.inline_demand
                        .insert((context.thread, context.generation, row), (document, keys));
                }
            }
            Message::RowsVisible {
                thread,
                generation,
                keys,
            } => {
                let selected = if thread {
                    &self.selection.thread
                } else {
                    &self.selection.room
                };
                if selected
                    .as_ref()
                    .is_some_and(|(current, _)| *current == generation)
                {
                    self.visible_rows[usize::from(thread)] = keys;
                }
            }
            Message::WindowFocused(focused) => {
                self.window_focused = focused;
                if !focused {
                    self.stop_image_playback();
                }
            }
            Message::WindowResized(width) => self.window_width = Some(width),
            Message::TimelineVisible {
                thread,
                generation,
                keys,
                metrics,
            } => {
                self.read_visible(thread, generation, &keys, metrics);
                if thread
                    && self.selection.conversation_visible
                    && self.pending_scroll[1].is_none()
                    && let Some((current, root)) = &self.selection.thread
                    && *current == generation
                    && let Some(pane) = current_pane(&self.model.thread, generation)
                    && pane.error.is_none()
                    && !matches!(pane.history, History::Loading)
                    && let Some((_, room)) = &self.selection.room
                {
                    if let Some((view, target)) = self.thread_scroll_target {
                        let offset = metrics.maximum() - metrics.translation;
                        if view != generation
                            || (offset - target.min(metrics.maximum())).abs() > 1.0
                        {
                            return Task::none();
                        }
                        self.thread_scroll_target = None;
                    }
                    let keys: Vec<_> = keys
                        .into_iter()
                        .filter(|key| {
                            pane.rows.iter().any(|row| {
                                row.key == *key && row.thread_root.as_deref() == Some(root)
                            })
                        })
                        .collect();
                    self.thread_activity.viewed(room, root, &keys);
                }
            }
            Message::OpenThreadLatest(root) if !self.closing && !self.stopped => {
                if let Some((_, room)) = &self.selection.room {
                    self.scroll_positions.insert(
                        DraftKey {
                            room: room.clone(),
                            root: Some(root.clone()),
                        },
                        0.0,
                    );
                    if let Some((generation, current)) = &self.selection.thread
                        && *current == root
                    {
                        self.pending_scroll[1] = None;
                        self.thread_scroll_target = Some((*generation, 0.0));
                        return jump_to_newest(true);
                    }
                    return self.update(Message::Thread(root));
                }
            }
            Message::DemoThreadReply if self.demo.is_some() => {
                self.demo.as_mut().unwrap().incoming();
                self.refresh_demo();
            }
            Message::TimelineMeasured {
                thread,
                generation,
                revision,
                metrics,
            } => {
                let selected = if thread {
                    &self.selection.thread
                } else {
                    &self.selection.room
                };
                let snapshot = if thread {
                    &self.model.thread
                } else {
                    &self.model.room
                };
                if selected
                    .as_ref()
                    .is_some_and(|(current, _)| *current == generation)
                    && let Some(pane) = current_pane(snapshot, generation)
                    && pane.history_revision == revision
                    && metrics.height.is_finite()
                    && metrics.height > 0.0
                    && metrics.content_height.is_finite()
                    && metrics.translation.is_finite()
                {
                    self.timeline_metrics[usize::from(thread)] =
                        Some((generation, revision, metrics));
                    let top = self.pending_scroll[usize::from(thread)]
                        .map_or(metrics.translation, |offset| metrics.maximum() - offset);
                    if matches!(pane.history, History::Ready) && top < metrics.buffer() {
                        return self.update(Message::LoadOlder {
                            thread,
                            generation,
                            revision,
                        });
                    }
                }
            }
            Message::JumpToNewest { thread, generation } => {
                let selected = if thread {
                    &self.selection.thread
                } else {
                    &self.selection.room
                };
                if selected
                    .as_ref()
                    .is_some_and(|(current, _)| *current == generation)
                {
                    self.pending_scroll[usize::from(thread)] = None;
                    if thread {
                        self.thread_scroll_target = Some((generation, 0.0));
                    }
                    if let Some(key) = self.draft_key(thread) {
                        self.scroll_positions.insert(key, 0.0);
                    }
                    return jump_to_newest(thread);
                }
            }

            Message::Scrolled {
                thread,
                generation,
                offset,
            } => {
                let selected = if thread {
                    &self.selection.thread
                } else {
                    &self.selection.room
                };
                if selected
                    .as_ref()
                    .is_some_and(|(current, _)| *current == generation)
                    && self.pending_scroll[usize::from(thread)].is_none()
                    && offset.is_finite()
                    && let Some(key) = self.draft_key(thread)
                {
                    self.scroll_positions.insert(key, offset.max(0.0));
                    if thread {
                        self.thread_scroll_target = None;
                    }
                }
            }
            Message::Space(space) if !self.closing && !self.stopped => {
                self.shell.space = space;
                self.shell.query.clear();
            }
            Message::Section(section) => {
                let section = self.section_key(&section);
                if !self.shell.collapsed.remove(&section) {
                    self.shell.collapsed.insert(section);
                }
            }
            Message::Search(query) => self.shell.query = query,
            Message::SearchOpen => {
                if let Some(id) = self.visible_rooms().first().cloned() {
                    let task = self.update(Message::Room(id));
                    return task.chain(iced::widget::operation::focus("room-composer"));
                }
            }
            Message::AdjacentRoom(previous) => {
                let rooms = self.visible_rooms();
                if !rooms.is_empty() {
                    let index = rooms.iter().position(|id| {
                        self.selection
                            .room
                            .as_ref()
                            .is_some_and(|(_, selected)| selected == id)
                    });
                    let next = match (index, previous) {
                        (Some(index), true) => (index + rooms.len() - 1) % rooms.len(),
                        (Some(index), false) => (index + 1) % rooms.len(),
                        (None, true) => rooms.len() - 1,
                        (None, false) => 0,
                    };
                    return self.update(Message::Room(rooms[next].clone()));
                }
            }
            Message::OpenLastThread => {
                if let Some(root) =
                    self.model
                        .room
                        .as_ref()
                        .and_then(|p| {
                            p.rows.iter().rev().find(|row| {
                                row.kind == RowKind::Message && row.thread_root.is_none()
                            })
                        })
                        .and_then(|r| r.event_id.clone())
                {
                    let task = self.update(Message::Thread(root));
                    return task.chain(iced::widget::operation::focus("thread-composer"));
                }
            }
            Message::SearchFocus => {
                self.settings.navigation_touched = true;
                self.shell.navigation = true;
                return iced::widget::operation::focus("room-search")
                    .chain(iced::widget::operation::select_all("room-search"));
            }
            Message::ToggleNavigation => {
                self.settings.navigation_touched = true;
                self.shell.navigation = !self.shell.navigation;
            }
            Message::Focus(backwards) => {
                return if backwards {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            Message::Escape => {
                if !self.shell.query.is_empty() {
                    self.shell.query.clear();
                } else {
                    return self
                        .update(if self.selection.members.is_some() {
                            Message::Members(members::Event::Close)
                        } else {
                            Message::CloseThread
                        })
                        .chain(iced::widget::operation::focus("room-composer"));
                }
            }

            Message::Markdown(event) => action_task = self.markdown_event(event),
            Message::Composing(active) => {
                self.composing = active;
                if !active && let Some(key) = self.emoji_completion.editing.clone() {
                    self.refresh_emoji_completion(&key);
                }
            }
            Message::Edit(key, action) if !self.closing && !self.stopped => {
                if (self.draft_key(false).as_ref() == Some(&key)
                    || self.draft_key(true).as_ref() == Some(&key))
                    && let Some(draft) = self.drafts.get_mut(&key)
                    && !draft.pending
                {
                    let edit = matches!(action, text_editor::Action::Edit(_));
                    let refresh_completion = !matches!(action, text_editor::Action::Scroll { .. });
                    draft.content.perform(action);
                    if edit {
                        draft.markdown.clear_preview();
                        self.typing_edited(&key);
                    }
                    if refresh_completion {
                        self.refresh_emoji_completion(&key);
                    }
                }
            }
            Message::Send(key)
                if !self.closing
                    && !self.stopped
                    && !self.composing
                    && self.pending_ops < MAX_PENDING_OPERATIONS =>
            {
                if self.attachment_picking(&key)
                    || (self.draft_key(false).as_ref() != Some(&key)
                        && self.draft_key(true).as_ref() != Some(&key))
                {
                    return Task::none();
                }
                if self.demo.is_some() {
                    if let Some(draft) = self.drafts.get_mut(&key) {
                        let body = draft.content.text();
                        if !draft.pending && (!body.trim().is_empty() || draft.attachment.is_some())
                        {
                            if draft.attachment.is_none()
                                && interconnect::composition::needs_conversion(
                                    &body,
                                    draft.markdown.format,
                                )
                            {
                                let format = draft.markdown.format;
                                let account = self.account_id.clone();
                                draft.pending = true;
                                draft.error = None;
                                self.pending_ops += 1;
                                return Task::perform(
                                    interconnect::composition::prepare(body, format),
                                    move |result| {
                                        Message::Markdown(markdown_composer::Event::DemoSent {
                                            key: key.clone(),
                                            account: account.clone(),
                                            result,
                                        })
                                    },
                                );
                            }
                            self.demo.as_mut().unwrap().send_attachment_or_text(
                                &key,
                                body,
                                draft.attachment.take(),
                            );
                            draft.content = text_editor::Content::new();
                            draft.markdown.clear_preview();
                            draft.error = None;
                            self.refresh_demo();
                        }
                    }
                    return jump_to_newest(key.root.is_some());
                }
                if let Some(composer) = self.composer_for(&key)
                    && let Some(draft) = self.drafts.get_mut(&key)
                    && !draft.pending
                    && (!draft.content.text().trim().is_empty() || draft.attachment.is_some())
                {
                    let body = draft.content.text();
                    let format = draft.markdown.format;
                    let attachment = draft.attachment.clone();
                    let account = self.account_id.clone();
                    draft.pending = true;
                    draft.error = None;
                    self.pending_ops += 1;
                    return Task::perform(
                        async move {
                            if let Some(attachment) = attachment {
                                composer.send_attachment(attachment, body).await
                            } else {
                                composer.send_as(body, format).await
                            }
                        },
                        move |result| Message::Sent(key.clone(), account.clone(), result),
                    );
                }
            }
            Message::Reorder(event) => return self.reorder_event(event),
            Message::SectionsReordered(account, request, result) => {
                self.pending_ops -= 1;
                self.sections_reordered(account, request, result);
            }
            Message::SectionChanged(account, request, result) => {
                self.pending_ops -= 1;
                self.section_changed(account, request, result);
            }
            Message::RoomSectionMoved(account, request, result)
            | Message::RoomMarkedUnread(account, request, result) => {
                self.pending_ops -= 1;
                self.room_menu_finished(account, request, result);
            }
            Message::Sent(key, account, result) => {
                self.pending_ops -= 1;
                if account == self.account_id
                    && let Some(draft) = self.drafts.get_mut(&key)
                {
                    draft.pending = false;
                    match result {
                        Ok(()) => {
                            draft.attachment = None;
                            draft.content = text_editor::Content::new();
                            draft.markdown.clear_preview();
                            draft.error = None;
                        }
                        Err(error) => {
                            draft.error = Some(format!("Could not queue message: {error}"))
                        }
                    }
                }
            }
            Message::Retry(retry)
                if !self.closing && !self.stopped && self.pending_ops < MAX_PENDING_OPERATIONS =>
            {
                let id = retry.transaction.clone();
                if self.retrying.insert(id.clone()) {
                    self.retry_errors.remove(&id);
                    self.pending_ops += 1;
                    return Task::perform(retry.retry(), move |result| {
                        Message::Retried(id.clone(), result)
                    });
                }
            }
            Message::Retried(id, result) => {
                self.pending_ops -= 1;
                self.retrying.remove(&id);
                if let Err(error) = result {
                    self.retry_errors.insert(id, error);
                }
            }
            Message::Login(_) | Message::Verification(_) => unreachable!(),
            Message::Control(sender) => {
                sender.send_replace(self.selection.clone());
                self.control = Some(sender);
            }
            Message::State(mut state) => {
                let bookmarks_changed = self.model.bookmarks.entries != state.bookmarks.entries
                    || self.model.bookmarks.loading != state.bookmarks.loading;
                if self.demo.is_none()
                    && (state.authentication.is_some() || state.account_id != self.account_id)
                {
                    self.new_chat = None;
                    self.create_room = None;
                    self.pending_created_space = None;
                    self.pending_created_room = None;
                    self.bookmarks.reset();
                }
                if let Some(room) = &self.pending_created_room {
                    if state.rooms.iter().any(|r| r.id == room.id) {
                        self.pending_created_room = None;
                    } else {
                        std::sync::Arc::make_mut(&mut state.rooms).push(room.clone());
                    }
                }
                if let Some((room, parent)) = &self.pending_created_space {
                    if let Some(space) = std::sync::Arc::make_mut(&mut state.spaces)
                        .iter_mut()
                        .find(|s| &s.id == parent)
                    {
                        if space.children.contains(room) {
                            self.pending_created_space = None;
                        } else {
                            space.children.push(room.clone());
                        }
                    } else {
                        self.pending_created_space = None;
                    }
                }
                if let Some(account_id) = &state.account_id {
                    if self
                        .account_id
                        .as_ref()
                        .is_some_and(|previous| previous != account_id)
                    {
                        self.drafts.clear();
                        self.scroll_positions.clear();
                        self.pending_scroll = [None, None];
                        self.selection.room = None;
                        self.selection.thread = None;
                        self.preferences_ready = false;
                        self.pending_navigation = None;
                    }
                    self.account_id = Some(account_id.clone());
                }
                if state.authentication.is_some()
                    || state.verification.control.is_none() && self.demo.is_none()
                {
                    self.verification.open = false;
                }
                self.model = *state;
                if bookmarks_changed && self.bookmarks.open {
                    action_task = self.load_bookmark_previews();
                }
                if let Some(room) = self.restore_navigation() {
                    return self.update(Message::Room(room));
                }
                if self.model.navigation_error.is_none() {
                    if let Scope::Space(id) = &self.shell.space
                        && !self.model.spaces.iter().any(|s| &s.id == id)
                    {
                        self.shell.space = Scope::All;
                    }
                    if self
                        .selection
                        .room
                        .as_ref()
                        .is_some_and(|(_, id)| !self.model.rooms.iter().any(|r| &r.id == id))
                    {
                        self.selection.room = None;
                        self.selection.thread = None;
                    }
                }
            }
            Message::Stopped(result) => {
                self.stopped = true;
                match result {
                    Ok(()) if self.closing => return iced::exit(),
                    Ok(()) => self.model.status = "Disconnected. Session retained.".into(),
                    Err(error) => {
                        self.model.status = format!("{error} — session retained. Close to exit.")
                    }
                }
            }
            Message::Close if self.demo.is_some() => return iced::exit(),
            Message::Close => {
                self.bookmarks.close();
                if self.stopped && self.pending_ops == 0 {
                    return iced::exit();
                }
                self.closing = true;
                self.selection.shutdown = self.pending_ops == 0;
                self.model.status = "Stopping synchronization…".into();
            }
            Message::Exit => return self.update(Message::Close),
            Message::Room(id) if !self.closing && !self.stopped => {
                self.bookmarks.close();
                self.visible_rows = Default::default();
                self.generation += 1;
                self.selection.members = None;
                self.selection.room = Some((self.generation, id));
                self.selection.thread = None;
                self.pending_scroll = [Some(self.saved_scroll(false)), None];
                self.drafts
                    .entry(self.draft_key(false).unwrap())
                    .or_default();
            }
            Message::Thread(id)
                if !self.closing && !self.stopped && self.selection.room.is_some() =>
            {
                self.bookmarks.close();
                self.visible_rows[1].clear();
                self.generation += 1;
                self.selection.members = None;
                self.selection.thread = Some((self.generation, id));
                self.pending_scroll[1] = Some(self.saved_scroll(true));
                if let Some(key) = self.draft_key(true) {
                    self.drafts.entry(key).or_default();
                }
            }
            Message::LoadOlder {
                thread,
                generation,
                revision,
            } if !self.closing && !self.stopped => {
                let selected = if thread {
                    &self.selection.thread
                } else {
                    &self.selection.room
                };
                let snapshot = if thread {
                    &mut self.model.thread
                } else {
                    &mut self.model.room
                };
                if selected
                    .as_ref()
                    .is_some_and(|(current, _)| *current == generation)
                    && let Some(pane) = snapshot.as_mut().filter(|p| p.generation == generation)
                    && pane.history_revision == revision
                    && matches!(pane.history, History::Ready | History::Failed(_))
                    && let Some(sender) = &pane.load_older
                    && sender.try_send(revision).is_ok()
                {
                    pane.history = History::Loading;
                    pane.load_older = None;
                }
            }
            Message::CloseThread => {
                self.visible_rows[1].clear();
                self.selection.thread = None;
                self.pending_scroll[1] = None;
                self.pending_scroll[0] = Some(self.saved_scroll(false));
            }
            _ => {}
        }
        if navigation {
            self.refresh_demo();
        }
        if observe_threads
            && let Some((generation, room)) = &self.selection.room
            && let Some(pane) = current_pane(&self.model.room, *generation)
        {
            self.thread_activity
                .observe(room, &pane.rows, &pane.history);
        }
        if self.selection.room.is_none() || self.closing || self.stopped {
            self.selection.members = None;
        }
        if self.selection.typing.as_ref().is_some_and(|typing| {
            self.selection
                .room
                .as_ref()
                .is_none_or(|(generation, _)| *generation != typing.generation)
        }) || self.closing
            || self.stopped
        {
            self.stop_typing();
        }
        self.validate_section_drag();
        self.validate_context_menu();
        self.validate_download();
        self.validate_invite();
        self.validate_pin_error();
        self.validate_topic_editor();
        self.validate_reaction_picker();
        self.validate_composer_picker();
        self.validate_emoji_completion();
        if self.closing && self.pending_ops == 0 {
            if self.stopped {
                return iced::exit();
            }
            self.selection.shutdown = true;
        }
        if self.preferences_ready && self.pending_navigation.is_none() && self.demo.is_none() {
            let mut collapsed: Vec<_> = self.shell.collapsed.iter().cloned().collect();
            collapsed.sort();
            self.selection.preferences = Some(Preferences {
                scope: self.shell.space.clone(),
                room: self.selection.room.as_ref().map(|(_, id)| id.clone()),
                collapsed,
                ..Default::default()
            });
        }
        if refresh_media {
            self.refresh_media_demand();
        }
        self.selection.bookmark_rooms = if self.bookmarks.open && !self.model.bookmarks.loading {
            let mut rooms: Vec<_> = self
                .model
                .bookmarks
                .entries
                .iter()
                .skip(self.bookmarks.start())
                .take(self.bookmarks.limit())
                .map(|entry| entry.pointer.room_id.clone())
                .collect();
            rooms.sort();
            rooms.dedup();
            rooms
        } else {
            Vec::new()
        };
        self.update_notification_context();
        self.validate_read_targets();
        if let Some(control) = &self.control {
            // Navigation represents desired state, so superseded selections can be replaced.
            control.send_if_modified(|s| {
                if *s == self.selection {
                    false
                } else {
                    *s = self.selection.clone();
                    true
                }
            });
        }
        let scroll = Task::batch([action_task, self.restore_scroll()]);
        if opening_image && self.image_view.is_some() {
            Task::batch([scroll, iced::widget::operation::focus("image-viewer")])
        } else if closing_image {
            Task::batch([
                scroll,
                iced::widget::operation::focus(if self.selection.thread.is_some() {
                    "thread-composer"
                } else {
                    "room-composer"
                }),
            ])
        } else {
            scroll
        }
    }
    fn inline_assets(&self) -> inline_image::Assets {
        inline_image::Assets {
            remote: self.model.images.clone(),
            local: self.local_inline.clone(),
        }
    }
    fn refresh_media_demand(&mut self) {
        if self.closing || self.stopped {
            self.image_view = None;
        }
        self.validate_image_view();
        self.selection.avatars.clear();
        self.selection.images.clear();
        if self.demo.is_some() || self.closing || self.stopped {
            return;
        }
        let original = self.thread_original().cloned();
        let room = &self.model.room;
        let thread = &self.model.thread;
        let selected_room = &self.selection.room;
        let selected_thread = &self.selection.thread;
        self.inline_demand
            .retain(|(is_thread, generation, key), (document, _)| {
                let selected = if *is_thread {
                    selected_thread
                } else {
                    selected_room
                };
                if !selected
                    .as_ref()
                    .is_some_and(|(current, _)| current == generation)
                {
                    return false;
                }
                let pane = if *is_thread { thread } else { room };
                current_pane(pane, *generation)
                    .and_then(|pane| pane.rows.iter().find(|row| row.key == *key))
                    .or_else(|| {
                        is_thread
                            .then_some(original.as_ref())
                            .flatten()
                            .filter(|row| row.key == *key)
                    })
                    .and_then(|row| row.formatted.as_ref())
                    .is_some_and(|current| std::sync::Arc::ptr_eq(current, document))
            });
        if let Some(uri) = &self.model.account_avatar_url {
            self.selection.avatars.push(uri.clone());
        }
        for uri in self
            .model
            .spaces
            .iter()
            .filter_map(|s| s.avatar_url.as_ref())
        {
            if !self.selection.avatars.contains(uri)
                && self.selection.avatars.len() < interconnect::media::MAX_REQUESTED
            {
                self.selection.avatars.push(uri.clone());
            }
        }
        if let Some(view) = &self.image_view {
            self.selection.images.push(view.original.clone());
            return;
        }
        for uri in self
            .invite_avatars()
            .into_iter()
            .chain(self.new_chat_avatars())
        {
            if !self.selection.avatars.contains(&uri)
                && self.selection.avatars.len() < interconnect::media::MAX_REQUESTED
            {
                self.selection.avatars.push(uri);
            }
        }
        for uri in self
            .visible_members()
            .into_iter()
            .filter_map(|m| m.avatar_url.clone())
            .collect::<Vec<_>>()
        {
            if !self.selection.avatars.contains(&uri)
                && self.selection.avatars.len() < interconnect::media::MAX_REQUESTED
            {
                self.selection.avatars.push(uri);
            }
        }
        let mut previews = 0;
        if self.bookmarks.open {
            return;
        }
        for (index, (selected, snapshot)) in [
            (&self.selection.room, &self.model.room),
            (&self.selection.thread, &self.model.thread),
        ]
        .into_iter()
        .enumerate()
        {
            let Some((generation, _)) = selected else {
                continue;
            };
            let Some(pane) = current_pane(snapshot, *generation) else {
                continue;
            };
            let rows = thread_activity::project(
                &pane.rows,
                (index == 1).then_some(selected.as_ref().unwrap().1.as_str()),
            );
            let visible: HashSet<_> = self.visible_rows[index]
                .iter()
                .map(String::as_str)
                .collect();
            for (position, projected) in rows.iter().copied().enumerate() {
                let thread_activity::Row::Event(row) = projected else {
                    continue;
                };
                let previous = position.checked_sub(1).and_then(|i| match rows[i] {
                    thread_activity::Row::Event(row) => Some(row),
                    _ => None,
                });
                if visible.contains(projected.key().as_str())
                    && let Some(key) = row.image.as_ref().and_then(|image| image.preview.as_ref())
                    && previews < interconnect::image_media::MAX_PREVIEWS
                    && !self.selection.images.contains(key)
                {
                    self.selection.images.push(key.clone());
                    previews += 1;
                }
                if (row.kind == RowKind::Activity || !presentation::grouped(previous, row))
                    && visible.contains(projected.key().as_str())
                    && let Some(uri) = row.avatar_url.as_ref()
                    && !self.selection.avatars.contains(uri)
                    && self.selection.avatars.len() < interconnect::media::MAX_REQUESTED
                {
                    self.selection.avatars.push(uri.clone());
                }
            }
        }
        let mut inline_count = 0;
        for (_, keys) in self.inline_demand.values() {
            for key in keys {
                if !self.selection.images.contains(key)
                    && inline_count < interconnect::image_media::MAX_INLINE_IMAGES
                {
                    self.selection.images.push(key.clone());
                    inline_count += 1;
                }
            }
        }
    }
    fn saved_scroll(&self, thread: bool) -> f32 {
        self.draft_key(thread)
            .and_then(|key| self.scroll_positions.get(&key).copied())
            .unwrap_or(0.0)
    }
    fn restore_scroll(&mut self) -> Task<Message> {
        if self.bookmarks.open {
            return Task::none();
        }
        let mut tasks = Vec::new();
        for thread in [false, true] {
            let index = usize::from(thread);
            let selected = if thread {
                &self.selection.thread
            } else {
                &self.selection.room
            };
            let snapshot = if thread {
                &self.model.thread
            } else {
                &self.model.room
            };
            if let Some(offset) = self.pending_scroll[index]
                && let Some((generation, _)) = selected
                && let Some(pane) = current_pane(snapshot, *generation)
                && pane.error.is_none()
                && !matches!(pane.history, History::Loading)
                && (offset == 0.0
                    || pane.load_older.is_none()
                    || matches!(pane.history, History::Failed(_))
                    || self.timeline_metrics[index].is_some_and(|(view, revision, metrics)| {
                        view == *generation
                            && revision == pane.history_revision
                            && metrics.maximum() - offset >= metrics.buffer()
                    }))
            {
                self.pending_scroll[index] = None;
                if thread {
                    self.thread_scroll_target = Some((*generation, offset));
                }
                tasks.push(iced::widget::operation::scroll_to(
                    if thread {
                        "thread-timeline"
                    } else {
                        "room-timeline"
                    },
                    iced::widget::operation::AbsoluteOffset { x: 0.0, y: offset },
                ));
            }
        }
        Task::batch(tasks)
    }
    fn restore_navigation(&mut self) -> Option<String> {
        if self.demo.is_some() {
            return None;
        }
        if !self.preferences_ready
            && let Some(saved) = &self.model.saved_navigation
        {
            self.preferences_ready = true;
            self.shell.collapsed = saved.collapsed.iter().cloned().collect();
            self.pending_navigation = Some(saved.clone());
        }
        let saved = self.pending_navigation.as_ref()?;
        if let Scope::Space(id) = &saved.scope
            && !self.model.spaces.iter().any(|s| &s.id == id)
        {
            return None;
        }
        self.shell.space = saved.scope.clone();
        if let Some(id) = &saved.room
            && !self.model.rooms.iter().any(|r| &r.id == id)
        {
            return None;
        }
        self.pending_navigation.take().and_then(|saved| saved.room)
    }
    fn draft_key(&self, thread: bool) -> Option<DraftKey> {
        let room = self.selection.room.as_ref()?.1.clone();
        let root = if thread {
            Some(self.selection.thread.as_ref()?.1.clone())
        } else {
            None
        };
        Some(DraftKey { room, root })
    }
    fn composer_for(&self, key: &DraftKey) -> Option<interconnect::connected::Composer> {
        let thread = key.root.is_some();
        if self.draft_key(thread).as_ref() != Some(key) {
            return None;
        }
        let generation = if thread {
            self.selection.thread.as_ref()?.0
        } else {
            self.selection.room.as_ref()?.0
        };
        let pane = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        current_pane(pane, generation)?.composer.clone()
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            self.section_drag_subscription(),
            self.image_subscription(),
            if self.demo.is_some() {
                Subscription::run(|| {
                    iced::stream::channel(1, async |_output| {
                        interconnect::diagnostics::Diagnostics::default()
                            .reports()
                            .await;
                    })
                })
            } else {
                Subscription::run_with(self.profile.clone(), |profile| connection(profile))
            },
            // The popup handles Escape itself; a second global Escape could
            // otherwise also close the thread after dismissing the picker.
            if self.create_room.is_some() && !self.composing {
                iced::event::listen_with(create_room::shortcut)
            } else if self.new_chat.is_some() && !self.composing {
                iced::event::listen_with(new_chat::shortcut)
            } else if self.room_picker.is_some() {
                iced::event::listen_with(room_picker::shortcut)
            } else if self.verification.open {
                iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    }) => Some(Message::Escape),
                    _ => None,
                })
            } else if (self.demo.is_none() && self.model.authentication.is_some())
                || self.context_menu.is_some()
                || self.section_dragging()
                || self.composing
                || self.reaction_picker.is_some()
                || self.composer_picker.is_some()
            {
                Subscription::none()
            } else if self.emoji_completion.is_active() {
                iced::event::listen_with(emoji_completion::shortcut)
            } else {
                iced::event::listen_with(|event, _, _| {
                    if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        ..
                    }) = event
                    {
                        shell_shortcut(&key, modifiers)
                    } else {
                        None
                    }
                })
            },
            iced::window::close_requests().map(|_| Message::Close),
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Window(iced::window::Event::Focused) => {
                    Some(Message::WindowFocused(true))
                }
                iced::Event::Window(iced::window::Event::Unfocused) => {
                    Some(Message::WindowFocused(false))
                }
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size.width))
                }
                _ => None,
            }),
            if interconnect::diagnostics::enabled() {
                iced::event::listen_raw(|event, _, _| {
                    if matches!(
                        event,
                        iced::Event::Window(iced::window::Event::RedrawRequested(_))
                    ) {
                        interconnect::diagnostics::redraw();
                    }
                    None
                })
            } else {
                Subscription::none()
            },
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::InputMethod(iced::advanced::input_method::Event::Preedit(text, _)) => {
                    Some(Message::Composing(!text.is_empty()))
                }
                iced::Event::InputMethod(
                    iced::advanced::input_method::Event::Commit(_)
                    | iced::advanced::input_method::Event::Closed,
                ) => Some(Message::Composing(false)),
                _ => None,
            }),
        ])
    }
    fn view(&self) -> Element<'_, Message> {
        if self.demo.is_none()
            && let Some(authentication) = &self.model.authentication
        {
            return self.login_view(authentication);
        }
        let mut layers = iced::widget::stack![self.shell_view()];
        if self.create_room.is_some() {
            layers = layers.push(self.create_room_view());
        }
        if self.new_chat.is_some() {
            layers = layers.push(self.new_chat_view());
        }
        if self.room_picker.is_some() {
            layers = layers.push(self.quick_room_view());
        }
        if self.settings.draft.is_some() {
            layers = layers.push(self.settings_view());
        }
        if self.image_view.is_some() {
            layers = layers.push(self.image_viewer());
        }
        if self.verification.open {
            layers = layers.push(self.verification_view());
        }
        self.context_menu_view(layers)
    }
    fn refresh_demo(&mut self) {
        if let Some(demo) = &mut self.demo {
            self.model.typing = self.selection.room.as_ref().map(|(generation, room)| {
                interconnect::connected::Typing {
                    generation: *generation,
                    names: if room == "studio" {
                        vec!["Alex Chen".into(), "Sam Rivera".into()]
                    } else {
                        Vec::new()
                    },
                }
            });
            self.model.members = self.selection.members.map(|generation| {
                let direct = self
                    .selection
                    .room
                    .as_ref()
                    .and_then(|(_, id)| self.model.rooms.iter().find(|r| &r.id == id))
                    .is_some_and(|r| r.direct);
                let invitations = self
                    .selection
                    .room
                    .as_ref()
                    .and_then(|(_, room)| demo.invitations.get(room))
                    .map_or(&[][..], Vec::as_slice);
                members::demo(generation, direct, invitations)
            });
            self.model.room = self.selection.room.as_ref().map(|(generation, room)| {
                demo.pane(
                    DraftKey {
                        room: room.clone(),
                        root: None,
                    },
                    *generation,
                )
            });
            self.model.pinned = self.selection.room.as_ref().and_then(|(generation, room)| {
                let entry = self.model.rooms.iter().find(|entry| &entry.id == room)?;
                if entry.pinned_events.is_empty() {
                    return None;
                }
                let rows = demo.pinned_rows(room, &entry.pinned_events);
                Some(interconnect::connected::Pinned {
                    generation: *generation,
                    total: entry.pinned_events.len(),
                    rows: rows.into(),
                    loading: false,
                    error: None,
                })
            });
            self.model.thread = self.selection.thread.as_ref().map(|(generation, root)| {
                demo.pane(
                    DraftKey {
                        room: self.selection.room.as_ref().unwrap().1.clone(),
                        root: Some(root.clone()),
                    },
                    *generation,
                )
            });
        }
    }
    fn thread_original(&self) -> Option<&MessageRow> {
        let (generation, root) = self.selection.thread.as_ref()?;
        current_pane(&self.model.thread, *generation)
            .and_then(|pane| {
                pane.rows
                    .iter()
                    .find(|row| row.event_id.as_ref() == Some(root))
            })
            .or_else(|| self.thread_original_in_room())
    }

    fn thread_original_in_room(&self) -> Option<&MessageRow> {
        let (_, root) = self.selection.thread.as_ref()?;
        let (generation, _) = self.selection.room.as_ref()?;
        current_pane(&self.model.room, *generation)?
            .rows
            .iter()
            .find(|row| row.event_id.as_ref() == Some(root))
    }

    fn thread_activity_view<'a>(
        &self,
        root: &str,
        latest: &'a MessageRow,
        original: Option<&MessageRow>,
        replies: usize,
    ) -> Element<'a, Message> {
        let replies = original.map_or(replies, |row| replies.max(row.thread_replies as usize));
        let unread = self
            .selection
            .room
            .as_ref()
            .map_or(0, |(_, room)| self.thread_activity.unread(room, root));
        let topic = original
            .map(|row| thread_activity::excerpt(&row.body, 64))
            .filter(|text| !text.is_empty())
            .map_or_else(|| "a thread".into(), |text| format!("“{text}”"));
        let author = if latest.is_own {
            "You"
        } else {
            &latest.display_name
        };
        let mut heading = row![
            emoji::preview(format!("{author} replied to {topic}"))
                .size(12)
                .color(shell::MUTED)
                .width(Length::Fill),
            text(format!(
                "{replies} {}",
                if replies == 1 { "reply" } else { "replies" }
            ))
            .size(11)
            .color(shell::MUTED),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if unread > 0 {
            heading = heading.push(
                text(format!("• {unread} new"))
                    .size(11)
                    .color(shell::ACCENT),
            );
        }
        let mut details = column![
            heading,
            emoji::preview(thread_activity::excerpt(&latest.body, 120))
                .size(12)
                .color(shell::MUTED)
        ]
        .spacing(3);
        if !latest.send_state.is_empty() && latest.send_state != "Sent" {
            details = details.push(text(latest.send_state.clone()).size(11).color(shell::MUTED));
        }
        let mut open = button(
            row![
                container(text("↳").size(16).color(shell::MUTED)).center_x(32),
                details.width(Length::Fill),
                text("›").size(15).color(shell::MUTED)
            ]
            .spacing(12),
        )
        .padding([6, 0])
        .width(Length::Fill)
        .style(shell::quiet_button);
        if !self.closing && !self.stopped {
            open = open.on_press(Message::OpenThreadLatest(root.to_owned()));
        }
        iced::widget::tooltip(
            open,
            text(presentation::time_detail(latest)).size(12),
            iced::widget::tooltip::Position::Top,
        )
        .into()
    }

    fn thread_original_view(&self) -> Element<'_, Message> {
        let original: Element<'_, Message> = if let Some(message) = self.thread_original() {
            if message.kind == RowKind::Activity {
                selectable::activity_view(
                    message.event_id.as_deref().unwrap_or(&message.body),
                    &message.body,
                    &message.display_name,
                )
            } else {
                let timestamp = iced::widget::tooltip(
                    text(presentation::time_label(message))
                        .size(11)
                        .color(shell::MUTED),
                    text(presentation::time_detail(message)).size(12),
                    iced::widget::tooltip::Position::Top,
                );
                column![
                    row![
                        iced::widget::tooltip(
                            shell::title(&message.display_name, 13),
                            text(&message.sender).size(12),
                            iced::widget::tooltip::Position::Top,
                        ),
                        iced::widget::space().width(Length::Fill),
                        timestamp,
                    ]
                    .spacing(12),
                    selectable::message_view_with_assets(
                        message,
                        self.inline_assets(),
                        self.selection.thread.as_ref().map(|(generation, _)| {
                            inline_image::Context {
                                thread: true,
                                generation: *generation,
                            }
                        })
                    ),
                ]
                .spacing(6)
                .into()
            }
        } else {
            text(
                if self
                    .selection
                    .thread
                    .as_ref()
                    .is_some_and(|(generation, _)| {
                        current_pane(&self.model.thread, *generation).is_none()
                    })
                {
                    "Loading original message…"
                } else {
                    "Original message unavailable"
                },
            )
            .size(12)
            .color(shell::MUTED)
            .into()
        };
        // The SDK's thread cache routes root reactions to the room timeline.
        let original = if let Some(badges) = self
            .thread_original_in_room()
            .or_else(|| self.thread_original())
            .and_then(|message| {
                self.reaction_chips(message, true, self.selection.thread.as_ref()?.0)
            }) {
            column![original, badges].spacing(6).into()
        } else {
            original
        };
        let original =
            if let Some(message) = self.thread_original().filter(|row| row.image.is_some()) {
                let generation = self.selection.thread.as_ref().expect("thread original").0;
                let mut open = button(text("Open image").size(12)).style(shell::quiet_button);
                if !self.closing && !self.stopped {
                    open = open.on_press(Message::Image(images::Event::Open {
                        thread: true,
                        generation,
                        row: message.key.clone(),
                    }));
                }
                column![original, open].spacing(6).into()
            } else {
                original
            };
        let original = if let Some(message) = self
            .thread_original()
            .filter(|row| row.download.is_some() || row.image.is_some())
        {
            let generation = self.selection.thread.as_ref().expect("thread original").0;
            column![original, self.download_actions(message, true, generation)]
                .spacing(6)
                .into()
        } else {
            original
        };
        let original = if let Some(message) = self.thread_original() {
            let generation = self.selection.thread.as_ref().expect("thread original").0;
            self.message_context(
                shell::message_actions(
                    self.bookmark_feedback(
                        self.pin_feedback(original, message, true, generation),
                        message,
                    ),
                    None,
                    self.reaction_action(message, true, generation),
                    self.pin_action(message, true, generation),
                    self.bookmark_action(message, true, generation),
                    self.reaction_popup(
                        true,
                        generation,
                        message.event_id.as_deref().unwrap_or_default(),
                    ),
                ),
                message,
                true,
                generation,
            )
        } else {
            original
        };
        column![
            container(
                container(
                    scrollable(original)
                        .id("thread-original")
                        .direction(shell::scrollbar())
                        .style(shell::scroll_style)
                )
                .max_height(144)
            )
            .padding([12, 24]),
            iced::widget::rule::horizontal(1),
        ]
        .into()
    }

    fn pane(&self, thread: bool) -> Element<'_, Message> {
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let snapshot = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        let Some((generation, selected_id)) = selected else {
            return shell::empty_state(
                "A little room to talk",
                "Choose a conversation from the sidebar.",
            );
        };
        let name = self
            .selection
            .room
            .as_ref()
            .and_then(|(_, id)| self.model.rooms.iter().find(|r| &r.id == id))
            .map(|r| r.name.as_str())
            .unwrap_or("Conversation");
        let heading = if thread { "Thread" } else { name };
        let subtitle = if thread {
            text("A focused conversation")
                .size(12)
                .color(shell::MUTED)
                .into()
        } else {
            self.topic_view(*generation, selected_id)
        };
        let mut header = row![
            column![shell::title(heading, 18), subtitle]
                .spacing(5)
                .width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center);
        if !thread {
            header = header.push(self.member_badge());
        }
        if !thread && self.demo.is_some() && selected_id == "threads" {
            header = header.push(
                button(text("New sample reply").size(12))
                    .style(shell::quiet_button)
                    .on_press(Message::DemoThreadReply),
            );
        }
        if thread {
            header = header.push(iced::widget::tooltip(
                button(container(text("×").size(18)).center_x(28).center_y(28))
                    .padding(0)
                    .style(shell::icon_button)
                    .on_press(Message::CloseThread),
                container(text("Close thread").size(12))
                    .padding(8)
                    .style(container::rounded_box),
                iced::widget::tooltip::Position::Bottom,
            ));
        }
        let mut content = column![
            container(header).padding([18, 24]),
            iced::widget::rule::horizontal(1)
        ]
        .spacing(0);
        if !thread {
            // Keep the timeline's sibling index stable as pins arrive or disappear.
            let pins = self
                .pinned_view(*generation)
                .unwrap_or_else(|| iced::widget::space().height(0).into());
            content = content.push(iced::widget::keyed_column([(*generation, pins)]));
        }
        if thread {
            // Reset the original's own scroll/selection state on thread changes.
            content = content.push(iced::widget::keyed_column([(
                *generation,
                self.thread_original_view(),
            )]));
        }
        let Some(pane) = current_pane(snapshot, *generation) else {
            return content
                .push(shell::empty_state(
                    "Opening conversation…",
                    "Messages will appear here shortly.",
                ))
                .into();
        };
        if let Some(error) = &pane.error {
            return content
                .push(shell::empty_state(
                    "Could not open this conversation",
                    error,
                ))
                .into();
        }
        let mut history_controls = column![].spacing(6);
        if let Some((current, _)) = &self.model.receipt_errors[usize::from(thread)]
            && current == generation
        {
            history_controls = history_controls.push(
                text("Could not mark as read. Refocus this conversation to retry.")
                    .size(12)
                    .color(shell::MUTED),
            );
        }
        match &pane.history {
            History::Loading => {
                history_controls = history_controls.push(
                    text(if thread {
                        "Loading replies…"
                    } else {
                        "Loading messages…"
                    })
                    .size(12)
                    .color(shell::MUTED),
                )
            }
            History::Failed(error) => {
                history_controls = history_controls.push(
                    text(format!(
                        "Could not load history: {error}. Live messages remain active."
                    ))
                    .size(12)
                    .color(shell::MUTED),
                );
                let mut retry = button(text("Retry loading history").size(12))
                    .padding([4, 0])
                    .style(shell::quiet_button);
                if pane.load_older.is_some() && !self.closing && !self.stopped {
                    retry = retry.on_press(Message::LoadOlder {
                        thread,
                        generation: *generation,
                        revision: pane.history_revision,
                    });
                }
                history_controls = history_controls.push(retry);
            }
            History::End if !thread => {
                history_controls = history_controls.push(
                    container(
                        text("Beginning of conversation")
                            .size(11)
                            .color(shell::MUTED),
                    )
                    .padding([12, 0])
                    .center_x(Length::Fill),
                )
            }
            History::Ready | History::End => {}
        }
        let rows = thread_activity::project(&pane.rows, thread.then_some(selected_id.as_str()));
        let mut messages = column![history_controls].spacing(0).padding([16, 24]);
        for (index, projected) in rows.iter().copied().enumerate() {
            let message = projected.message();
            let previous_row = index.checked_sub(1).map(|i| rows[i]);
            let previous = previous_row.and_then(|row| match row {
                thread_activity::Row::Event(event) => Some(event),
                _ => None,
            });
            let activity = message.kind == RowKind::Activity;
            let grouped = presentation::grouped(previous, message);
            let new_date = presentation::date(message).filter(|date| {
                previous_row
                    .and_then(|row| presentation::date(row.message()))
                    .as_ref()
                    != Some(date)
            });
            let mut message_row = column![];
            if let Some(date) = new_date {
                message_row = message_row.push(
                    container(
                        text(date.format("%A, %d %B %Y").to_string())
                            .size(11)
                            .color(shell::MUTED),
                    )
                    .center_x(Length::Fill)
                    .padding([12, 0]),
                );
            }
            if let thread_activity::Row::Thread {
                root,
                latest,
                original,
                replies,
            } = projected
            {
                let summary = self.thread_activity_view(root, latest, original, replies);
                let summary = context_menu::trigger(
                    summary,
                    context_menu::Target::Thread {
                        room: selected_id.clone(),
                        generation: *generation,
                        root: root.into(),
                    },
                );
                message_row = message_row.push(summary);
                messages = messages.push(container(message_row).id(timeline::row_id(index)));
                continue;
            }
            let timestamp = iced::widget::tooltip(
                text(presentation::time_label(message))
                    .size(11)
                    .color(shell::MUTED),
                text(presentation::time_detail(message)).size(12),
                iced::widget::tooltip::Position::Top,
            );
            let body = if activity {
                selectable::activity_view(
                    message.event_id.as_deref().unwrap_or(&message.body),
                    &message.body,
                    &message.display_name,
                )
            } else {
                selectable::message_view_with_assets(
                    message,
                    self.inline_assets(),
                    Some(inline_image::Context {
                        thread,
                        generation: *generation,
                    }),
                )
            };
            let mut entry = if activity {
                column![iced::widget::tooltip(
                    body,
                    text(presentation::time_detail(message)).size(12),
                    iced::widget::tooltip::Position::Top,
                )]
            } else if grouped {
                column![row![body, timestamp].spacing(12)]
            } else {
                column![
                    row![
                        iced::widget::tooltip(
                            shell::title(&message.display_name, 13),
                            text(&message.sender).size(12),
                            iced::widget::tooltip::Position::Top
                        ),
                        iced::widget::space().width(Length::Fill),
                        timestamp
                    ]
                    .spacing(12),
                    body
                ]
                .spacing(6)
            };
            if message.image.is_some() {
                entry = entry.push(self.image_preview(message, thread, *generation));
            }
            if message.download.is_some() || message.image.is_some() {
                entry = entry.push(self.download_actions(message, thread, *generation));
            }
            if let Some(badges) = self.reaction_chips(message, thread, *generation) {
                entry = entry.push(badges);
            }
            if !thread && !message.read_by.is_empty() {
                let count = message.read_by.len();
                let label = if count == 1 {
                    "Read by 1 person".to_owned()
                } else {
                    format!("Read by {count} people")
                };
                entry = entry.push(iced::widget::tooltip(
                    text(label).size(11).color(shell::MUTED),
                    text(message.read_by.join("\n")).size(12),
                    iced::widget::tooltip::Position::Top,
                ));
            }
            if !message.send_state.is_empty() && message.send_state != "Sent" {
                entry = entry.push(text(&message.send_state).size(12));
            }
            if let Some(retry) = &message.retry {
                let mut control = button(if self.retrying.contains(&retry.transaction) {
                    "Retrying…"
                } else {
                    "Retry"
                });
                if !self.closing && !self.stopped && !self.retrying.contains(&retry.transaction) {
                    control = control.on_press(Message::Retry(retry.clone()));
                }
                entry = entry.push(control);
                if let Some(error) = self.retry_errors.get(&retry.transaction) {
                    entry = entry.push(text(error));
                }
            }
            if !thread
                && message.thread_replies > 0
                && let Some(id) = &message.event_id
            {
                let mut label = if message.thread_replies == 1 {
                    "1 reply  ›".into()
                } else {
                    format!("{} replies  ›", message.thread_replies)
                };
                let unread = self
                    .selection
                    .room
                    .as_ref()
                    .map_or(0, |(_, room)| self.thread_activity.unread(room, id));
                if unread > 0 {
                    label = format!("• {label} · {unread} new");
                }
                let mut replies = button(text(label).size(12))
                    .style(shell::quiet_button)
                    .padding([4, 0]);
                if !self.closing && !self.stopped {
                    replies = replies.on_press(Message::Thread(id.clone()));
                }
                entry = entry.push(replies);
            }
            let avatar: Element<'_, Message> = if activity {
                container(shell::activity_avatar(
                    &message.display_name,
                    message
                        .avatar_url
                        .as_ref()
                        .and_then(|uri| self.model.avatars.get(uri)),
                ))
                .center_x(32)
                .into()
            } else if grouped {
                iced::widget::space().width(32).into()
            } else {
                shell::avatar(
                    &message.display_name,
                    message
                        .avatar_url
                        .as_ref()
                        .and_then(|uri| self.model.avatars.get(uri)),
                )
            };
            let body = container(row![avatar, entry.width(Length::Fill)].spacing(12)).padding(
                iced::Padding {
                    top: if activity {
                        if previous.is_some_and(|row| row.kind == RowKind::Activity)
                            && new_date.is_none()
                        {
                            4.0
                        } else {
                            12.0
                        }
                    } else if grouped {
                        5.0
                    } else {
                        18.0
                    },
                    ..iced::Padding::ZERO
                },
            );
            let body: Element<'_, Message> = if !activity && let Some(id) = &message.event_id {
                let reply = (!thread && !self.closing && !self.stopped)
                    .then(|| Message::Thread(id.clone()));
                shell::message_actions(
                    self.bookmark_feedback(
                        self.pin_feedback(body.into(), message, thread, *generation),
                        message,
                    ),
                    reply,
                    self.reaction_action(message, thread, *generation),
                    self.pin_action(message, thread, *generation),
                    self.bookmark_action(message, thread, *generation),
                    self.reaction_popup(thread, *generation, id),
                )
            } else {
                body.into()
            };
            let body = if !activity {
                self.message_context(body, message, thread, *generation)
            } else {
                body
            };
            message_row = message_row.push(body);
            messages = messages.push(container(message_row).id(timeline::row_id(index)));
        }
        if rows.is_empty() && matches!(pane.history, History::Ready | History::End) {
            messages = messages.push(if thread {
                shell::empty_state("No replies yet", "Reply below to start the thread.")
            } else {
                shell::empty_state(
                    "The start of something",
                    "Send the first message in this conversation.",
                )
            });
        }
        let view_generation = *generation;
        let timeline = scrollable(messages)
            .direction(shell::scrollbar())
            .anchor_bottom()
            .on_scroll(move |viewport| Message::Scrolled {
                thread,
                generation: view_generation,
                offset: viewport.absolute_offset().y,
            })
            .style(shell::scroll_style)
            .id(if thread {
                "thread-timeline"
            } else {
                "room-timeline"
            })
            .height(Length::Fill);
        // New view identity resets the scroll state; async rows retain that
        // state and a zero distance from the bottom follows loaded history.
        let keys = rows.iter().map(|row| Some(row.key())).collect();
        let observed = if thread {
            timeline::thread_history_view(
                *generation,
                keys,
                timeline.into(),
                pane.history_revision,
                matches!(pane.history, History::Ready),
                self.selection.conversation_visible
                    && self.pending_scroll[1].is_none()
                    && !matches!(pane.history, History::Loading),
            )
        } else {
            timeline::room_history_view(
                *generation,
                keys,
                timeline.into(),
                pane.history_revision,
                matches!(pane.history, History::Ready),
                self.selection.conversation_visible
                    && self.pending_scroll[0].is_none()
                    && !matches!(pane.history, History::Loading),
            )
        };
        let mut message_area = iced::widget::stack![observed];
        // Keep the timeline in the same widget slot as this overlay appears and
        // disappears. The one-pixel tolerance accounts for layout rounding.
        if self.pending_scroll[usize::from(thread)].is_none() && self.saved_scroll(thread) > 1.0 {
            let jump = iced::widget::tooltip(
                button(container(text("⌄").size(24)).center_x(36).center_y(36))
                    .padding(0)
                    .style(shell::jump_button)
                    .on_press(Message::JumpToNewest {
                        thread,
                        generation: *generation,
                    }),
                container(text("Jump to newest message").size(12))
                    .padding(8)
                    .style(container::rounded_box),
                iced::widget::tooltip::Position::Top,
            );
            message_area = message_area.push(
                container(jump)
                    .center_x(Length::Fill)
                    .align_bottom(Length::Fill)
                    .padding(12),
            );
        }
        content = content.push(message_area);
        if let Some(key) = self.draft_key(thread)
            && let Some(draft) = self.drafts.get(&key)
        {
            let edit_key = key.clone();
            let send_key = key.clone();
            let composing = self.composing;
            let shortcut = self.settings.current.composer.send_shortcut;
            let mut editor = text_editor(&draft.content)
                .id(if thread {
                    "thread-composer"
                } else {
                    "room-composer"
                })
                .highlight_fonts_with::<emoji::Highlighter>(emoji::font(), |font| *font)
                .cursor_blinking(false)
                .height(80)
                .size(14)
                // Keep text and selection above the bottom action row.
                .padding(iced::Padding {
                    top: 8.0,
                    right: 10.0,
                    bottom: 34.0,
                    left: 10.0,
                })
                .style(shell::editor_style)
                .placeholder(if draft.attachment.is_some() {
                    "Add a caption…"
                } else if thread {
                    "Reply to this thread…"
                } else {
                    "Write a message…"
                });
            if !self.closing && !self.stopped && !draft.pending && self.settings.draft.is_none() {
                editor = editor
                    .on_action(move |action| Message::Edit(edit_key.clone(), action))
                    .key_binding(move |press| {
                        self.emoji_completion_binding(&send_key, *generation, &press)
                            .or_else(|| {
                                self.attachment_paste_binding(&send_key, *generation, &press)
                            })
                            .or_else(|| composer_binding(press, composing, &send_key, shortcut))
                    });
            }
            let mut send = button(
                container(text(if draft.pending { "…" } else { "↑" }).size(18))
                    .center_x(26)
                    .center_y(26),
            )
            .padding(0)
            .style(shell::icon_button);
            if !self.closing
                && !self.stopped
                && !self.composing
                && !draft.pending
                && self.pending_ops < MAX_PENDING_OPERATIONS
                && (pane.composer.is_some() || self.demo.is_some())
                && !self.attachment_picking(&key)
                && (!draft.content.text().trim().is_empty() || draft.attachment.is_some())
            {
                send = send.on_press(Message::Send(key.clone()));
            }
            let send = iced::widget::tooltip(
                send,
                container(
                    text(if draft.pending {
                        "Queueing message…"
                    } else if thread {
                        match shortcut {
                            interconnect::settings::SendShortcut::CtrlEnter => {
                                "Send reply (Ctrl+Enter)"
                            }
                            interconnect::settings::SendShortcut::Enter => "Send reply (Enter)",
                        }
                    } else {
                        match shortcut {
                            interconnect::settings::SendShortcut::CtrlEnter => {
                                "Send message (Ctrl+Enter)"
                            }
                            interconnect::settings::SendShortcut::Enter => "Send message (Enter)",
                        }
                    })
                    .size(12),
                )
                .padding(8)
                .style(container::rounded_box),
                iced::widget::tooltip::Position::Top,
            );
            let actions = row![
                self.markdown_controls(&key, *generation),
                iced::widget::space().width(Length::Fill),
                self.attachment_button(&key, *generation),
                self.composer_emoji_button(&key, *generation),
                container(send).id("composer-send")
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center);
            if self.clipboard_pasting(&key) {
                content = content.push(
                    container(text("Reading clipboard…").size(12).color(shell::MUTED))
                        .padding([0, 24]),
                );
            }
            if let Some(chip) = self.attachment_chip(&key, *generation) {
                content = content.push(container(chip).padding([0, 24]));
            }
            // Reserve one static line so typing updates never resize the timeline.
            let label = self.typing_label();
            let label = if thread && !label.is_empty() {
                format!("In this room: {label}")
            } else {
                label
            };
            content = content.push(
                container(
                    text(label)
                        .size(12)
                        .color(shell::MUTED)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .width(Length::Fill),
                )
                .width(Length::Fill)
                .clip(true)
                .height(18)
                .padding([0, 24]),
            );
            if let Some(preview) = self.markdown_preview(&key) {
                content = content.push(container(preview).padding([0, 24]));
            }
            let composer = iced::widget::stack![
                editor,
                container(actions)
                    .align_bottom(Length::Fill)
                    .padding([4, 6]),
            ];
            content = content.push(
                container(self.composer_emoji_host(composer, &key, *generation)).padding([10, 24]),
            );
            if let Some(error) = &draft.error {
                content = content.push(text(error));
            }
        }
        let target = self
            .draft_key(thread)
            .and_then(|key| self.attachment_drop_target(&key, *generation));
        file_drop::view(content.into(), target)
    }
}
fn jump_to_newest(thread: bool) -> Task<Message> {
    // Offsets are measured from the anchor: zero is the bottom here.
    iced::widget::operation::snap_to(
        if thread {
            "thread-timeline"
        } else {
            "room-timeline"
        },
        iced::widget::operation::RelativeOffset::START,
    )
}

fn shell_shortcut(
    key: &iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Option<Message> {
    use iced::keyboard::{Key, key::Named};
    if create_room::is_shortcut(key, modifiers) {
        return Some(Message::CreateRoom(create_room::Event::Open));
    }
    if new_chat::is_shortcut(key, modifiers) {
        return Some(Message::NewChat(new_chat::Event::Open));
    }
    if room_picker::is_shortcut(key, modifiers) {
        return Some(Message::QuickRoom(room_picker::Event::Open));
    }
    match key.as_ref() {
        Key::Character(",") if modifiers.command() => {
            Some(Message::Settings(settings::Event::Open))
        }
        Key::Named(Named::Tab) => Some(Message::Focus(modifiers.shift())),
        Key::Named(Named::Escape) => Some(Message::Escape),
        Key::Named(Named::ArrowUp) if modifiers.alt() => Some(Message::AdjacentRoom(true)),
        Key::Named(Named::ArrowDown) if modifiers.alt() => Some(Message::AdjacentRoom(false)),
        Key::Character("t") if modifiers.alt() => Some(Message::OpenLastThread),
        Key::Character("k") if modifiers.command() => Some(Message::SearchFocus),
        _ => None,
    }
}

fn composer_binding(
    press: text_editor::KeyPress,
    composing: bool,
    key: &DraftKey,
    shortcut: interconnect::settings::SendShortcut,
) -> Option<text_editor::Binding<Message>> {
    if shell_shortcut(&press.key, press.modifiers).is_some() {
        return None;
    }
    if matches!(press.status, text_editor::Status::Focused { .. })
        && press.key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter)
        && (press.modifiers == iced::keyboard::Modifiers::CTRL
            || (shortcut == interconnect::settings::SendShortcut::Enter
                && press.modifiers.is_empty()))
    {
        return (!composing).then(|| text_editor::Binding::Custom(Message::Send(key.clone())));
    }
    text_editor::Binding::from_key_press(press)
}

fn current_pane(snapshot: &Option<Pane>, generation: u64) -> Option<&Pane> {
    snapshot.as_ref().filter(|p| p.generation == generation)
}

fn connection(profile: &str) -> impl iced::futures::Stream<Item = Message> + use<> {
    let profile = profile.to_owned();
    iced::stream::channel(1, async move |mut output| {
        let (control, selection) = watch::channel(Selection::default());
        let (state, mut updates) = watch::channel(Model::default());
        if output.send(Message::Control(control)).await.is_err() {
            return;
        }
        let (login_sender, login) = tokio::sync::mpsc::channel(1);
        if output
            .send(Message::Login(login::Event::Ready(login_sender)))
            .await
            .is_err()
        {
            return;
        }
        let diagnostics = state.borrow().diagnostics.clone();
        let backend = interconnect::connected::run(profile, selection, state, login);
        let result = {
            let forward = async {
                while updates.changed().await.is_ok() {
                    let snapshot = updates.borrow_and_update().clone();
                    if output
                        .send(Message::State(Box::new(snapshot)))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                std::future::pending::<()>().await;
            };
            tokio::pin!(backend, forward);
            tokio::select! {
                result = &mut backend => result.map_err(|e| format!("{e:#}")),
                _ = &mut forward => unreachable!(),
                _ = diagnostics.reports() => unreachable!(),
            }
        };
        let _ = output.send(Message::Stopped(result)).await;
        std::future::pending::<()>().await;
    })
}
/// Iced borrows this executor; main owns the one runtime until all UI state
/// has been destroyed. Dropping Iced's executor cannot shut down SDK cleanup.
struct UiExecutor(tokio::runtime::Handle);
impl iced::Executor for UiExecutor {
    fn new() -> Result<Self, std::io::Error> {
        tokio::runtime::Handle::try_current()
            .map(Self)
            .map_err(std::io::Error::other)
    }
    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        drop(self.0.spawn(future));
    }
    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _entered = self.0.enter();
        f()
    }
    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.0.block_on(future)
    }
}

fn run_ui(run: impl FnOnce() -> iced::Result) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let result = {
        // Covers normal UI teardown and error paths outside Executor::enter.
        let _entered = runtime.enter();
        run()
    };
    let cleanup = interconnect::shutdown_runtime(runtime, std::time::Duration::from_secs(5));
    if let Err(error) = &cleanup {
        eprintln!("{error:#}");
    }
    result?;
    cleanup
}

fn profile_argument(arguments: impl Iterator<Item = String>) -> anyhow::Result<String> {
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        if argument == "--config" {
            arguments.next();
        } else if !argument.starts_with("--") {
            return Ok(argument);
        }
    }
    interconnect::session::default_profile_directory()
}

fn main() -> anyhow::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let mut config = None;
    while let Some(argument) = arguments.next() {
        if argument == "--config" {
            config =
                Some(std::path::PathBuf::from(arguments.next().ok_or_else(
                    || anyhow::anyhow!("--config requires a file path"),
                )?));
        }
    }
    let demo = std::env::args().any(|argument| argument == "--demo");
    let profile = if demo {
        String::new()
    } else {
        profile_argument(std::env::args().skip(1))?
    };
    let startup = settings::Startup::load(config, demo);
    configure_graphics_environment(&startup.graphics())?;
    interconnect::init_tracing();
    run_ui(|| {
        iced::application(
            move || {
                let (mut app, task) = App::boot(startup.clone());
                app.profile = profile.clone();
                (app, task)
            },
            App::update,
            App::view,
        )
        .title("Interconnect")
        .theme(shell::theme())
        .default_font(iced::Font::with_name("Inter"))
        .font(include_bytes!("../../assets/fonts/Inter-Regular.otf").as_slice())
        .font(include_bytes!("../../assets/fonts/Inter-SemiBold.otf").as_slice())
        .window(iced::window::Settings {
            size: iced::Size::new(1240.0, 820.0),
            min_size: Some(iced::Size::new(640.0, 480.0)),
            ..Default::default()
        })
        .executor::<UiExecutor>()
        .subscription(App::subscription)
        .exit_on_close_request(false)
        .run()
    })
}

/// Iced 0.14 reads adapter preference from the environment. Re-exec before
/// starting Tokio/Iced instead of mutating the process environment while other
/// threads may read it. exec preserves PID, arguments, working directory, and
/// inherited display/session variables. GPU opt-out is authoritative; an
/// explicit power-preference environment value otherwise takes priority.
fn configure_graphics_environment(
    graphics: &interconnect::settings::Graphics,
) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let set_power = std::env::var_os("WGPU_POWER_PREF").is_none();
    let force_software = !graphics.gpu_enabled
        && std::env::var_os("ICED_BACKEND").as_deref() != Some(std::ffi::OsStr::new("tiny-skia"));
    if set_power || force_software {
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.args(std::env::args_os().skip(1));
        if set_power {
            command.env(
                "WGPU_POWER_PREF",
                if graphics.low_power { "low" } else { "high" },
            );
        }
        if force_software {
            command.env("ICED_BACKEND", "tiny-skia");
        }
        let error = command.exec();
        return Err(anyhow::Error::new(error).context("Could not configure renderer settings"));
    }
    Ok(())
}

/// Widget/layout tests stay headless and deterministic with the same software
/// renderer variant used by the application's real fallback path.
#[cfg(test)]
fn test_renderer(font: iced::Font, size: iced::Pixels) -> iced::Renderer {
    iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(font, size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn profile_argument_preserves_explicit_paths_and_skips_config_values() {
        for args in [
            vec!["/custom/profile"],
            vec!["--config", "/settings.toml", "/custom/profile"],
            vec!["/custom/profile", "--config", "/settings.toml"],
        ] {
            assert_eq!(
                profile_argument(args.into_iter().map(String::from)).unwrap(),
                "/custom/profile"
            );
        }
        assert_eq!(
            profile_argument(["--config", "/settings.toml"].into_iter().map(String::from)).unwrap(),
            interconnect::session::default_profile_directory().unwrap()
        );
    }

    #[test]
    fn graphics_environment_child() {
        if std::env::var_os("INTERCONNECT_TEST_GRAPHICS_CHILD").is_none() {
            return;
        }
        let startup = settings::Startup::load(
            std::env::var_os("INTERCONNECT_TEST_GRAPHICS_CONFIG").map(std::path::PathBuf::from),
            std::env::var_os("INTERCONNECT_TEST_GRAPHICS_DEMO").is_some(),
        );
        configure_graphics_environment(&startup.graphics()).unwrap();
        println!(
            "graphics-power-preference={}",
            std::env::var("WGPU_POWER_PREF").unwrap()
        );
        println!(
            "graphics-backend={}",
            std::env::var("ICED_BACKEND").unwrap_or_else(|_| "default".into())
        );
    }

    #[test]
    fn graphics_launch_defaults_to_low_power_and_preserves_overrides() {
        let directory = tempfile::tempdir().unwrap();
        for (preference, expected) in [
            (None, "low"),
            (Some("high"), "high"),
            (Some("none"), "none"),
        ] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "tests::graphics_environment_child",
                    "--nocapture",
                ])
                .env(
                    "INTERCONNECT_TEST_GRAPHICS_CONFIG",
                    directory.path().join("missing.toml"),
                )
                .env_remove("INTERCONNECT_TEST_GRAPHICS_DEMO")
                .env("INTERCONNECT_TEST_GRAPHICS_CHILD", "1");
            if let Some(value) = preference {
                child.env("WGPU_POWER_PREF", value);
            } else {
                child.env_remove("WGPU_POWER_PREF");
            }
            let output = child.output().unwrap();
            assert!(output.status.success(), "{:?}", output.status);
            assert!(
                String::from_utf8(output.stdout)
                    .unwrap()
                    .contains(&format!("graphics-power-preference={expected}"))
            );
        }
    }

    #[test]
    fn saved_graphics_control_startup_and_gpu_opt_out_overrides_the_environment() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        for (gpu, low_power, demo, expected_backend, expected_power) in [
            (true, false, false, "wgpu", "high"),
            (false, true, false, "tiny-skia", "low"),
            (false, false, false, "tiny-skia", "high"),
            (false, false, true, "wgpu", "low"),
        ] {
            std::fs::write(
                &path,
                format!("[graphics]\ngpu_enabled = {gpu}\nlow_power = {low_power}\n"),
            )
            .unwrap();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "tests::graphics_environment_child",
                    "--nocapture",
                ])
                .env("INTERCONNECT_TEST_GRAPHICS_CHILD", "1")
                .env("INTERCONNECT_TEST_GRAPHICS_CONFIG", &path)
                .env("ICED_BACKEND", "wgpu")
                .env_remove("WGPU_POWER_PREF");
            if demo {
                command.env("INTERCONNECT_TEST_GRAPHICS_DEMO", "1");
            } else {
                command.env_remove("INTERCONNECT_TEST_GRAPHICS_DEMO");
            }
            let output = command.output().unwrap();
            assert!(output.status.success());
            let output = String::from_utf8(output.stdout).unwrap();
            assert!(
                output.contains(&format!("graphics-power-preference={expected_power}")),
                "{output}"
            );
            assert!(
                output.contains(&format!("graphics-backend={expected_backend}")),
                "{output}"
            );
        }
    }

    pub(super) fn demo_app() -> App {
        let mut app = App::new();
        app.demo = Some(demo::Demo::default());
        app.model.bookmarks.complete = true;
        app.model.rooms = demo::rooms().into();
        app.model.sections = demo::sections().into();
        app.model.spaces = demo::spaces().into();
        app.model.images = demo::images();
        app.local_inline = demo::inline_assets();
        app.shell.space = Scope::Space("studio-space".into());
        let _ = app.update(Message::Room("studio".into()));
        app
    }

    #[test]
    fn space_icons_follow_metadata_deduplicate_and_release_demand() {
        let mut app = demo_app();
        let first: matrix_sdk::ruma::OwnedMxcUri = "mxc://example.org/space".into();
        let next: matrix_sdk::ruma::OwnedMxcUri = "mxc://example.org/new-space".into();
        let spaces = std::sync::Arc::make_mut(&mut app.model.spaces);
        spaces[0].avatar_url = Some(first.clone());
        spaces[1].avatar_url = Some(first.clone());
        app.refresh_media_demand();
        assert!(
            app.selection.avatars.is_empty(),
            "demo never requests media"
        );
        app.demo = None;
        app.refresh_media_demand();
        assert_eq!(app.selection.avatars, vec![first.clone()]);
        let _ = app.update(Message::Space(Scope::All));
        assert_eq!(app.selection.avatars, vec![first]);

        let mut model = app.model.clone();
        let spaces = std::sync::Arc::make_mut(&mut model.spaces);
        spaces[0].avatar_url = None;
        spaces[1].avatar_url = Some(next.clone());
        let _ = app.update(Message::State(Box::new(model)));
        assert_eq!(app.selection.avatars, vec![next]);

        let mut model = app.model.clone();
        model.spaces = Default::default();
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.selection.avatars.is_empty());
    }

    #[test]
    fn account_avatar_demand_survives_navigation_and_releases_on_removal() {
        let mut app = demo_app();
        let uri: matrix_sdk::ruma::OwnedMxcUri = "mxc://example.org/account".into();
        app.model.account_avatar_url = Some(uri.clone());
        app.refresh_media_demand();
        assert!(
            app.selection.avatars.is_empty(),
            "demo never requests media"
        );
        app.demo = None;
        app.refresh_media_demand();
        assert_eq!(app.selection.avatars, vec![uri.clone()]);
        let _ = app.update(Message::Room("threads".into()));
        assert_eq!(app.selection.avatars, vec![uri]);
        let mut model = app.model.clone();
        model.account_avatar_url = None;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.selection.avatars.is_empty());
    }

    #[test]
    fn inline_demand_rejects_stale_documents_generations_and_offscreen_objects() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("inline-images".into()));
        app.demo = None;
        let generation = app.selection.room.as_ref().unwrap().0;
        let row = app.model.room.as_ref().unwrap().rows[0].clone();
        let document = row.formatted.clone().unwrap();
        let keys = document.image_keys();
        let report = |generation, document, keys| Message::InlineImagesDemand {
            context: inline_image::Context {
                thread: false,
                generation,
            },
            row: row.key.clone(),
            document,
            keys,
        };
        let _ = app.update(report(generation, document.clone(), keys.clone()));
        assert!(keys.iter().all(|key| app.selection.images.contains(key)));
        let _ = app.update(report(generation + 1, document.clone(), Vec::new()));
        assert!(
            keys.iter().all(|key| app.selection.images.contains(key)),
            "stale empty reports cannot cancel current demand"
        );
        let _ = app.update(report(generation, document.clone(), Vec::new()));
        assert!(
            keys.iter().all(|key| !app.selection.images.contains(key)),
            "leaving the viewport cancels demand"
        );
        let _ = app.update(report(generation, document.clone(), keys.clone()));
        let mut model = app.model.clone();
        let edited = Arc::make_mut(&mut model.room.as_mut().unwrap().rows);
        edited[0].formatted = Some(Arc::new(
            interconnect::message_document::Document::parse("<p>edited</p>").unwrap(),
        ));
        let _ = app.update(Message::State(Box::new(model)));
        let _ = app.update(report(generation, document, keys.clone()));
        assert!(
            keys.iter().all(|key| !app.selection.images.contains(key)),
            "old document reports cannot reload edited or redacted images"
        );
    }
    #[test]
    fn avatar_demand_uses_current_visible_rows_and_profile_updates() {
        let mut app = demo_app();
        app.demo = None;
        let generation = app.selection.room.as_ref().unwrap().0;
        let mut model = app.model.clone();
        let pane = model.room.as_mut().unwrap();
        let mut rows = pane.rows.as_ref().clone();
        let row = rows
            .iter_mut()
            .find(|row| row.kind == RowKind::Message && row.thread_root.is_none())
            .unwrap();
        let key = row.key.clone();
        let uri: matrix_sdk::ruma::OwnedMxcUri = "mxc://example.org/first-avatar".into();
        row.avatar_url = Some(uri.clone());
        pane.rows = Arc::new(rows);
        let _ = app.update(Message::State(Box::new(model)));
        let _ = app.update(Message::RowsVisible {
            thread: false,
            generation: generation + 1,
            keys: vec![key.clone()],
        });
        assert!(
            app.selection.avatars.is_empty(),
            "stale viewport must not load media"
        );
        let _ = app.update(Message::RowsVisible {
            thread: false,
            generation,
            keys: vec![key.clone()],
        });
        assert_eq!(app.selection.avatars, vec![uri]);
        let mut model = app.model.clone();
        let rows = Arc::make_mut(&mut model.room.as_mut().unwrap().rows);
        let uri: matrix_sdk::ruma::OwnedMxcUri = "mxc://example.org/new-avatar".into();
        rows.iter_mut()
            .find(|row| row.key == key)
            .unwrap()
            .avatar_url = Some(uri.clone());
        let _ = app.update(Message::State(Box::new(model)));
        assert_eq!(
            app.selection.avatars,
            vec![uri],
            "profile changes must refresh a stationary viewport"
        );
        let _ = app.update(Message::Room("threads".into()));
        assert!(
            app.selection.avatars.is_empty(),
            "navigation must release old demand"
        );
    }

    #[test]
    fn demo_does_not_request_avatars_even_with_media_metadata() {
        let mut app = demo_app();
        let pane = app.model.room.as_mut().unwrap();
        let rows = Arc::make_mut(&mut pane.rows);
        rows[0].avatar_url = Some("mxc://example.org/demo".into());
        let key = rows[0].key.clone();
        let generation = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::RowsVisible {
            thread: false,
            generation,
            keys: vec![key],
        });
        assert!(app.selection.avatars.is_empty());
    }

    #[test]
    fn demo_messages_and_drafts_stay_with_their_room_and_thread() {
        let mut app = demo_app();
        let room_key = app.draft_key(false).unwrap();
        let _ = app.update(Message::Thread("proposal".into()));
        let thread_key = app.draft_key(true).unwrap();
        assert_eq!(
            app.model.thread.as_ref().unwrap().rows[0]
                .event_id
                .as_deref(),
            Some("proposal")
        );
        app.drafts.get_mut(&room_key).unwrap().content =
            text_editor::Content::with_text("room draft");
        app.drafts.get_mut(&thread_key).unwrap().content =
            text_editor::Content::with_text("a local reply");
        let _ = app.update(Message::Send(thread_key.clone()));
        assert_eq!(
            app.model
                .room
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.event_id.as_deref() == Some("proposal"))
                .unwrap()
                .thread_replies,
            1
        );
        assert_eq!(app.pending_ops, 0);
        assert!(app.control.is_none());
        assert!(app.composer_for(&thread_key).is_none());
        assert!(app.drafts[&thread_key].content.text().trim().is_empty());
        assert_eq!(app.drafts[&room_key].content.text().trim(), "room draft");
        let _ = app.update(Message::Space(Scope::Space("home-space".into())));
        assert!(app.selection.thread.is_some());
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
        let _ = app.update(Message::Room("home".into()));
        assert!(app.selection.thread.is_none());
        let _ = app.update(Message::Space(Scope::Space("studio-space".into())));
        let _ = app.update(Message::Room("studio".into()));
        let _ = app.update(Message::Thread("proposal".into()));
        assert_eq!(
            app.model
                .thread
                .as_ref()
                .unwrap()
                .rows
                .last()
                .unwrap()
                .body
                .trim(),
            "a local reply"
        );
        assert_eq!(app.drafts[&room_key].content.text().trim(), "room draft");
    }

    #[test]
    fn thread_activity_unread_requires_a_focused_current_visible_thread() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("threads".into()));
        let _ = app.update(Message::DemoThreadReply);
        let _ = app.update(Message::DemoThreadReply);
        assert_eq!(app.thread_activity.unread("threads", "thread-plan"), 2);
        let _ = app.update(Message::Room("home".into()));
        let _ = app.update(Message::Room("threads".into()));
        assert_eq!(app.thread_activity.unread("threads", "thread-plan"), 2);
        let _ = app.update(Message::OpenThreadLatest("thread-plan".into()));
        let generation = app.selection.thread.as_ref().unwrap().0;
        let visible = |generation, key: &str| Message::TimelineVisible {
            thread: true,
            generation,
            keys: vec![key.into()],
            metrics: timeline::Metrics {
                height: 100.0,
                content_height: 600.0,
                translation: 500.0,
            },
        };
        let _ = app.update(visible(generation, "incoming-1"));
        assert_eq!(
            app.thread_activity.unread("threads", "thread-plan"),
            2,
            "unfocused windows do not mark replies viewed"
        );
        let _ = app.update(Message::WindowFocused(true));
        let _ = app.update(visible(generation - 1, "incoming-1"));
        assert_eq!(app.thread_activity.unread("threads", "thread-plan"), 2);
        app.thread_scroll_target = Some((generation, 200.0));
        let _ = app.update(visible(generation, "incoming-1"));
        assert_eq!(
            app.thread_activity.unread("threads", "thread-plan"),
            2,
            "wait for saved reading position before acknowledging a transient bottom layout"
        );
        app.thread_scroll_target = Some((generation, 0.0));
        let _ = app.update(visible(generation, "incoming-1"));
        assert_eq!(app.thread_activity.unread("threads", "thread-plan"), 1);
        app.pending_scroll[1] = Some(0.0);
        let _ = app.update(visible(generation, "incoming-2"));
        assert_eq!(app.thread_activity.unread("threads", "thread-plan"), 1);
        let _ = app.update(visible(generation, "incoming-2"));
        assert_eq!(app.thread_activity.unread("threads", "thread-plan"), 0);
        let _ = app.update(Message::DemoThreadReply);
        assert_eq!(
            app.thread_activity.unread("threads", "thread-plan"),
            1,
            "an open thread alone does not acknowledge an arrival"
        );
    }

    #[test]
    fn activity_click_targets_latest_and_keeps_drafts_and_room_scroll() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("threads".into()));
        let room = app.draft_key(false).unwrap();
        app.scroll_positions.insert(room.clone(), 250.0);
        app.drafts.get_mut(&room).unwrap().content = text_editor::Content::with_text("room draft");
        let _ = app.update(Message::Thread("thread-plan".into()));
        let thread = app.draft_key(true).unwrap();
        app.scroll_positions.insert(thread.clone(), 120.0);
        app.drafts.get_mut(&thread).unwrap().content =
            text_editor::Content::with_text("thread draft");
        let generation = app.selection.thread.as_ref().unwrap().0;
        let _ = app.update(Message::OpenThreadLatest("thread-plan".into()));
        assert_eq!(
            app.selection.thread.as_ref().unwrap().0,
            generation,
            "an already open thread is not restarted"
        );
        assert_eq!(app.saved_scroll(true), 0.0);
        assert_eq!(app.saved_scroll(false), 250.0);
        assert_eq!(app.drafts[&thread].content.text().trim(), "thread draft");
        assert_eq!(app.drafts[&room].content.text().trim(), "room draft");
        let _ = app.update(Message::CloseThread);
        let _ = app.update(Message::OpenThreadLatest("older-thread".into()));
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "older-thread");
        assert!(
            app.model
                .thread
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.key == "old-root-reply")
        );
    }

    #[test]
    fn keyboard_thread_opening_skips_trailing_activity() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("activity".into()));
        let _ = app.update(Message::OpenLastThread);
        assert_eq!(
            app.selection.thread.as_ref().unwrap().1,
            "activity-followup"
        );
    }

    #[test]
    fn activity_layout_is_compact_and_selectable_in_rooms_and_threads() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe {
            rows: Vec<(String, String, iced::Rectangle)>,
            containers: HashMap<widget::Id, iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if let Some(id) = id {
                    self.containers.insert(id.clone(), bounds);
                }
            }
            fn custom(
                &mut self,
                _: Option<&widget::Id>,
                bounds: iced::Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(state) = state.downcast_ref::<selectable::State>() {
                    self.rows
                        .push((state.id.clone(), state.body.clone(), bounds));
                }
            }
        }
        let mut app = demo_app();
        let _ = app.update(Message::Room("activity".into()));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for width in [320.0, 720.0] {
            for thread in [false, true] {
                if thread {
                    // Reuse the mixed room fixture as thread replies; SDK kind
                    // and selection behavior must survive either pane layout.
                    let mut pane = app.model.room.clone().unwrap();
                    pane.generation = 100;
                    app.model.thread = Some(pane);
                    app.selection.thread = Some((100, "$other-root".into()));
                }
                let mut view = app.pane(thread);
                let mut tree = widget::Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 640.0)),
                );
                let mut probe = Probe::default();
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                let activity = probe.containers[&timeline::row_id(1)];
                let message = probe.containers[&timeline::row_id(3)];
                assert!(activity.height < message.height / 2.0);
                let (_, text, bounds) = probe
                    .rows
                    .iter()
                    .find(|row| row.0 == "activity-topic")
                    .unwrap();
                assert!(text.starts_with("Jamie Davis · Changed the room topic"));
                assert!(bounds.width > 0.0 && bounds.width < width);
                assert_eq!(probe.rows.len(), 8, "one selectable body per event");
            }
        }
    }

    #[test]
    fn reaction_badges_appear_in_rooms_originals_and_replies_and_follow_root_updates() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe(usize);
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, _: iced::Rectangle) {
                if id == Some(&widget::Id::new("message-reactions")) {
                    self.0 += 1;
                }
            }
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        for thread in [false, true] {
            let mut tree = widget::Tree::empty();
            for width in [300.0, 700.0] {
                let mut view = app.pane(thread);
                tree.diff(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 640.0)),
                );
                let mut probe = Probe::default();
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                assert_eq!(
                    probe.0, 2,
                    "thread={thread}: badges appear on the original and reply, with no duplicates"
                );
            }
        }
        let mut tree = widget::Tree::empty();
        // Leave the thread snapshot unchanged while the room removes reactions.
        for (has_reactions, expected) in [(true, 2), (false, 1)] {
            if !has_reactions {
                Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
                    .iter_mut()
                    .find(|row| row.key == "review")
                    .unwrap()
                    .reactions
                    .clear();
            }
            let mut view = app.pane(true);
            tree.diff(view.as_widget());
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(360.0, 640.0)),
            );
            let mut probe = Probe::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            assert_eq!(
                probe.0, expected,
                "room root updates reach the pinned original"
            );
        }
    }

    #[test]
    fn thread_original_uses_current_snapshots_and_room_fallback() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        assert_eq!(
            app.thread_original().unwrap().event_id.as_deref(),
            Some("review")
        );
        let thread = app.model.thread.as_mut().unwrap();
        std::sync::Arc::make_mut(&mut thread.rows)[0].body = "updated original".into();
        assert_eq!(app.thread_original().unwrap().body, "updated original");
        // A loading/stale thread can use the current room, never a stale pane.
        app.model.thread.as_mut().unwrap().generation += 1;
        assert_ne!(app.thread_original().unwrap().body, "updated original");
        app.model.thread = None;
        assert_eq!(
            app.thread_original().unwrap().event_id.as_deref(),
            Some("review")
        );
        app.model.room.as_mut().unwrap().generation += 1;
        assert!(app.thread_original().is_none());
    }

    #[test]
    fn thread_original_is_bounded_and_stays_above_scrolling_replies_once() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe {
            ids: Vec<String>,
            original: Option<iced::Rectangle>,
            replies: Option<iced::Rectangle>,
            replies_translation: f32,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn custom(
                &mut self,
                _: Option<&widget::Id>,
                _: iced::Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(state) = state.downcast_ref::<selectable::State>() {
                    self.ids.push(state.id.clone());
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("thread-original")) {
                    self.original = Some(bounds);
                } else if id == Some(&widget::Id::new("thread-timeline")) {
                    self.replies = Some(bounds);
                    self.replies_translation = translation.y;
                }
            }
        }
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let rows = std::sync::Arc::make_mut(&mut app.model.thread.as_mut().unwrap().rows);
        rows[0].body = "A long original message.\n".repeat(80);
        let reply = rows[1].clone();
        for index in 0..40 {
            let mut reply = reply.clone();
            reply.event_id = Some(format!("reply-{index}-extra"));
            rows.push(reply);
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut pane = app.pane(true);
        let mut tree = widget::Tree::new(&pane);
        let node = pane.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, iced::Size::new(360.0, 640.0)),
        );
        let mut before = Probe::default();
        pane.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut before,
        );
        assert_eq!(before.ids.iter().filter(|id| *id == "review").count(), 1);
        let original = before.original.unwrap();
        let replies = before.replies.unwrap();
        assert!(original.height <= 144.0);
        assert!(original.y + original.height <= replies.y);
        assert!(
            replies.height > 100.0,
            "long originals leave space for replies"
        );
        pane.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut widget::operation::scrollable::scroll_to::<()>(
                widget::Id::new("thread-timeline"),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(120.0),
                },
            ),
        );
        let mut after = Probe::default();
        pane.as_widget_mut()
            .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut after);
        assert_eq!(after.original, before.original);
        assert_eq!(after.replies, before.replies);
        assert_ne!(after.replies_translation, before.replies_translation);
    }

    #[test]
    fn saved_navigation_waits_for_metadata_and_restores_without_a_thread() {
        let mut app = App::new();
        let prefs = Preferences {
            scope: Scope::Space("home-space".into()),
            room: Some("studio".into()),
            collapsed: vec!["saved-section".into()],
            ..Default::default()
        };
        let mut model = Model {
            saved_navigation: Some(prefs.clone()),
            ..Default::default()
        };
        let _ = app.update(Message::State(Box::new(model.clone())));
        assert!(app.selection.room.is_none());
        assert!(
            app.selection.preferences.is_none(),
            "do not overwrite while cache is empty"
        );
        assert!(app.pending_navigation.is_some());
        model.rooms = demo::rooms().into();
        model.spaces = demo::spaces().into();
        let _ = app.update(Message::State(Box::new(model)));
        assert_eq!(app.shell.space, prefs.scope);
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
        assert!(app.selection.thread.is_none());
        assert!(app.shell.collapsed.contains("saved-section"));
        assert!(app.pending_navigation.is_none());
        assert_eq!(app.selection.preferences, Some(prefs));
    }

    #[test]
    fn explicit_navigation_overrides_an_unavailable_saved_room() {
        let mut app = App::new();
        let model = Model {
            rooms: demo::rooms().into(),
            spaces: demo::spaces().into(),
            saved_navigation: Some(Preferences {
                scope: Scope::Space("missing-space".into()),
                room: Some("missing-room".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let _ = app.update(Message::State(Box::new(model.clone())));
        assert!(app.pending_navigation.is_some());
        let _ = app.update(Message::Space(Scope::Space("studio-space".into())));
        let selected = app.selection.room.clone();
        let _ = app.update(Message::State(Box::new(model)));
        assert_eq!(app.shell.space, Scope::Space("studio-space".into()));
        assert_eq!(app.selection.room, selected);
        assert!(app.pending_navigation.is_none());
        assert_eq!(
            app.selection.preferences.as_ref().unwrap().scope,
            app.shell.space
        );
    }

    #[test]
    fn live_metadata_preserves_space_identity_and_clears_removed_rooms() {
        let mut app = demo_app();
        app.demo = None;
        let mut state = app.model.clone();
        std::sync::Arc::make_mut(&mut state.spaces).reverse();
        let _ = app.update(Message::State(Box::new(state.clone())));
        assert_eq!(app.shell.space, Scope::Space("studio-space".into()));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
        let _ = app.update(Message::Thread("root".into()));
        state.rooms = state
            .rooms
            .iter()
            .filter(|r| r.id != "studio")
            .cloned()
            .collect::<Vec<_>>()
            .into();
        let _ = app.update(Message::State(Box::new(state.clone())));
        assert!(app.selection.room.is_none() && app.selection.thread.is_none());
        state.spaces = Default::default();
        let _ = app.update(Message::State(Box::new(state)));
        assert_eq!(app.shell.space, Scope::All);
    }

    #[test]
    fn space_clicks_preserve_conversation_through_metadata_updates() {
        let mut app = demo_app();
        app.demo = None;
        app.preferences_ready = true;
        let _ = app.update(Message::Thread("proposal".into()));
        let room = app.selection.room.clone();
        let thread = app.selection.thread.clone();
        let key = app.draft_key(true).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            text_editor::Content::with_text("unfinished reply");
        for scope in [Scope::Space("home-space".into()), Scope::Direct, Scope::All] {
            let _ = app.update(Message::Space(scope.clone()));
            let _ = app.update(Message::State(Box::new(app.model.clone())));
            assert_eq!(app.shell.space, scope);
            assert_eq!(app.selection.room, room);
            assert_eq!(app.selection.thread, thread);
            assert_eq!(app.drafts[&key].content.text().trim(), "unfinished reply");
            assert_eq!(
                app.selection.preferences.as_ref().unwrap().room.as_deref(),
                Some("studio")
            );
        }
        let _ = app.update(Message::Space(Scope::Space("home-space".into())));
        assert!(!app.visible_rooms().contains(&"studio".into()));
        let _ = app.update(Message::AdjacentRoom(false));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "home");
        assert!(app.selection.thread.is_none());
    }

    #[test]
    fn custom_section_identity_survives_rename_and_space_filtering() {
        let mut app = demo_app();
        let tag = "element.io.section.demo-design";
        assert_eq!(
            app.model
                .rooms
                .iter()
                .find(|r| r.id == "engineering")
                .unwrap()
                .section_in(&app.model.sections),
            tag
        );
        let room = app.selection.room.clone();
        let _ = app.update(Message::Section(tag.into()));
        let key = app.section_key(tag);
        assert!(app.shell.collapsed.contains(&key));
        let mut model = app.model.clone();
        std::sync::Arc::make_mut(&mut model.sections)
            .iter_mut()
            .find(|s| s.id == tag)
            .unwrap()
            .name = "Renamed".into();
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.shell.collapsed.contains(&key));
        assert_eq!(app.selection.room, room);
        let _ = app.update(Message::Search("Engineering".into()));
        assert_eq!(app.visible_rooms(), ["engineering"]);
        let _ = app.update(Message::Space(Scope::Space("home-space".into())));
        assert!(!app.visible_rooms().contains(&"engineering".into()));
        assert!(!app.shell.collapsed.contains(&app.section_key(tag)));
        assert_eq!(app.selection.room, room);
    }

    #[test]
    fn search_and_keyboard_navigation_respect_selected_space() {
        let mut app = demo_app();
        let _ = app.update(Message::Search("MAYA".into()));
        assert_eq!(app.visible_rooms(), ["maya"]);
        let _ = app.update(Message::SearchOpen);
        assert_eq!(app.selection.room.as_ref().unwrap().1, "maya");
        let _ = app.update(Message::Space(Scope::Space("home-space".into())));
        assert!(app.shell.query.is_empty());
        assert_eq!(app.visible_rooms(), ["home", "alex"]);
        let _ = app.update(Message::AdjacentRoom(true));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "alex");
        let _ = app.update(Message::Search("no matching room".into()));
        let _ = app.update(Message::SearchOpen);
        assert_eq!(app.selection.room.as_ref().unwrap().1, "alex");
    }

    #[test]
    fn scroll_positions_are_per_conversation_and_wait_for_loaded_history() {
        let mut app = demo_app();
        let first = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::Scrolled {
            thread: false,
            generation: first,
            offset: 320.0,
        });
        let _ = app.update(Message::Thread("proposal".into()));
        let thread_generation = app.selection.thread.as_ref().unwrap().0;
        let _ = app.update(Message::Scrolled {
            thread: true,
            generation: thread_generation,
            offset: 80.0,
        });
        let _ = app.update(Message::Room("home".into()));
        assert_eq!(app.saved_scroll(false), 0.0);
        app.demo = None;
        let _ = app.update(Message::Room("studio".into()));
        let generation = app.selection.room.as_ref().unwrap().0;
        assert_eq!(app.pending_scroll[0], Some(320.0));
        // A delayed event from the old view must not overwrite the remembered offset.
        let _ = app.update(Message::Scrolled {
            thread: false,
            generation: first,
            offset: 0.0,
        });
        assert_eq!(app.saved_scroll(false), 320.0);
        let mut model = app.model.clone();
        model.room = Some(Pane {
            generation,
            rows: Default::default(),
            error: None,
            history: History::Loading,
            history_revision: 0,
            load_older: None,
            composer: None,
        });
        let _ = app.update(Message::State(Box::new(model.clone())));
        let _ = app.update(Message::Scrolled {
            thread: false,
            generation,
            offset: 0.0,
        });
        assert_eq!(app.pending_scroll[0], Some(320.0));
        assert_eq!(app.saved_scroll(false), 320.0);
        model.room.as_mut().unwrap().history = History::Ready;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.pending_scroll[0].is_none());
        let _ = app.update(Message::Thread("proposal".into()));
        assert_eq!(app.pending_scroll[1], Some(80.0));
        let _ = app.update(Message::JumpToNewest {
            thread: false,
            generation: first,
        });
        assert_eq!(app.saved_scroll(false), 320.0);
        let _ = app.update(Message::JumpToNewest {
            thread: false,
            generation,
        });
        assert_eq!(app.saved_scroll(false), 0.0);
        assert_eq!(app.saved_scroll(true), 80.0);
    }

    #[test]
    fn timeline_layout_starts_at_bottom_and_keeps_position_across_history_controls() {
        use iced::advanced::{layout, widget};
        struct Probe {
            distance: Option<f32>,
            translation: f32,
            viewport_top: f32,
            anchor: Option<(widget::Id, f32)>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.distance = Some((content.height - bounds.height).max(0.0) - translation.y);
                    self.translation = translation.y;
                    self.viewport_top = bounds.y;
                }
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                let y = bounds.y - self.viewport_top - self.translation;
                if self.anchor.is_none()
                    && y + bounds.height > 0.0
                    && let Some(id) = id
                {
                    self.anchor = Some((id.clone(), y));
                }
            }
        }
        fn inspect(
            app: &App,
            tree: &mut widget::Tree,
            set_offset: Option<f32>,
        ) -> (f32, Option<(widget::Id, f32)>) {
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut pane = app.pane(false);
            tree.diff(pane.as_widget());
            let node = pane.as_widget_mut().layout(
                tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(600.0, 700.0)),
            );
            if let Some(y) = set_offset {
                pane.as_widget_mut().operate(
                    tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        widget::Id::new("room-timeline"),
                        iced::widget::operation::AbsoluteOffset {
                            x: Some(0.0),
                            y: Some(y),
                        },
                    ),
                );
            }
            let mut probe = Probe {
                distance: None,
                translation: 0.0,
                viewport_top: 0.0,
                anchor: None,
            };
            pane.as_widget_mut()
                .operate(tree, layout::Layout::new(&node), &renderer, &mut probe);
            (probe.distance.unwrap(), probe.anchor)
        }
        let mut app = demo_app();
        let _ = app.update(Message::Room("busy".into()));
        let mut tree = widget::Tree::empty();
        let rows = app.model.room.as_ref().unwrap().rows.clone();
        app.model.room.as_mut().unwrap().rows = Default::default();
        app.model.room.as_mut().unwrap().history = History::Loading;
        assert!(inspect(&app, &mut tree, None).0.abs() < 1.0);
        app.model.room.as_mut().unwrap().rows = rows;
        assert!(inspect(&app, &mut tree, None).0.abs() < 1.0);
        assert!((inspect(&app, &mut tree, Some(120.0)).0 - 120.0).abs() < 1.0);
        app.model.room.as_mut().unwrap().history = History::Loading;
        assert!((inspect(&app, &mut tree, None).0 - 120.0).abs() < 1.0);
        let anchor = inspect(&app, &mut tree, None).1.unwrap();
        app.model.room.as_mut().unwrap().history = History::Ready;
        let after = inspect(&app, &mut tree, None).1.unwrap();
        assert_eq!(after.0, anchor.0);
        assert!((after.1 - anchor.1).abs() < 1.0);
        // Replacing a view cannot inherit another view's internal widget offset.
        let _ = app.update(Message::Room("busy".into()));
        assert!(inspect(&app, &mut tree, None).0.abs() < 1.0);
    }

    #[test]
    fn focused_search_schedules_redraw_only_when_blinking_is_enabled() {
        use iced::advanced::{Shell, Widget, layout, mouse, widget};
        use iced::window::RedrawRequest;
        for blinking in [false, true] {
            let mut editor = iced::widget::text_input("Find a room", "fixture")
                .on_input(std::convert::identity)
                .cursor_blinking(blinking);
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut tree =
                widget::Tree::new(&editor as &dyn Widget<String, iced::Theme, iced::Renderer>);
            tree.state
                .downcast_mut::<iced::widget::text_input::State<
                    <iced::Renderer as iced::advanced::text::Renderer>::Paragraph,
                >>()
                .focus();
            let node = Widget::layout(
                &mut editor,
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(300.0, 110.0)),
            );
            let now = iced::time::Instant::now();
            // First redraw resolves focus style; later redraws must not arm a
            // blink deadline when disabled, even across multiple blink phases.
            for milliseconds in [0, 750, 1250] {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                editor.update(
                    &mut tree,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(
                        now + std::time::Duration::from_millis(milliseconds),
                    )),
                    layout::Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(iced::Size::new(300.0, 110.0)),
                );
                if milliseconds > 0 {
                    if blinking {
                        assert!(matches!(shell.redraw_request(), RedrawRequest::At(_)));
                    } else {
                        assert_eq!(shell.redraw_request(), RedrawRequest::Wait);
                    }
                }
            }
        }
    }

    #[test]
    fn focused_editor_schedules_redraw_only_when_blinking_is_enabled() {
        use iced::advanced::text::highlighter::PlainText;
        use iced::advanced::widget::operation::Focusable;
        use iced::advanced::{Shell, Widget, layout, mouse, widget};
        use iced::window::RedrawRequest;
        for blinking in [false, true] {
            let content = text_editor::Content::with_text("caret fixture");
            let mut editor = text_editor(&content)
                .on_action(std::convert::identity)
                .cursor_blinking(blinking);
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut tree = widget::Tree::new(
                &editor as &dyn Widget<text_editor::Action, iced::Theme, iced::Renderer>,
            );
            tree.state
                .downcast_mut::<text_editor::State<PlainText>>()
                .focus();
            let node = editor.layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(300.0, 110.0)),
            );
            let now = iced::time::Instant::now();
            // First redraw resolves focus style; later redraws must not arm a
            // blink deadline when disabled, even across multiple blink phases.
            for milliseconds in [0, 750, 1250] {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                editor.update(
                    &mut tree,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(
                        now + std::time::Duration::from_millis(milliseconds),
                    )),
                    layout::Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(iced::Size::new(300.0, 110.0)),
                );
                if milliseconds > 0 {
                    if blinking {
                        assert!(matches!(shell.redraw_request(), RedrawRequest::At(_)));
                    } else {
                        assert_eq!(shell.redraw_request(), RedrawRequest::Wait);
                    }
                }
            }
        }
    }

    #[test]
    fn sqlite_cleanup_outlives_iced_executor_and_runs_before_return() {
        use iced::Executor;
        let directory = tempfile::tempdir().unwrap();
        let (finished, received) = std::sync::mpsc::channel();
        struct Cleanup(std::sync::mpsc::Sender<()>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let finished = self.0.clone();
                drop(tokio::task::spawn_blocking(move || {
                    finished.send(()).unwrap()
                }));
            }
        }
        assert!(UiExecutor::new().is_err());
        run_ui(|| {
            let executor = UiExecutor::new().unwrap();
            let client = executor.block_on(async {
                matrix_sdk::Client::builder()
                    .homeserver_url("http://127.0.0.1:1")
                    .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
                    .sqlite_store(directory.path(), Some("test-only-passphrase"))
                    .build()
                    .await
                    .unwrap()
            });
            // Match Iced's problematic order: executor first, UI-owned SDK data last.
            drop(executor);
            drop(client);
            drop(Cleanup(finished));
            Ok(())
        })
        .unwrap();
        received.try_recv().unwrap();
        assert!(tokio::runtime::Handle::try_current().is_err());
    }

    #[test]
    fn drafts_survive_navigation_and_send_results_apply_to_original_target() {
        let mut app = App::new();
        let _ = app.update(Message::Room("!one:example.org".into()));
        let room_key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&room_key).unwrap().content =
            text_editor::Content::with_text("room draft");
        let _ = app.update(Message::Thread("$thread".into()));
        let thread_key = app.draft_key(true).unwrap();
        app.drafts.get_mut(&thread_key).unwrap().content =
            text_editor::Content::with_text("thread draft");
        let _ = app.update(Message::CloseThread);
        let _ = app.update(Message::Thread("$thread".into()));
        assert_eq!(
            app.drafts[&thread_key].content.text().trim_end(),
            "thread draft"
        );
        app.drafts.get_mut(&room_key).unwrap().pending = true;
        app.pending_ops = 1;
        let _ = app.update(Message::Room("!two:example.org".into()));
        let second = app.draft_key(false).unwrap();
        app.drafts.get_mut(&second).unwrap().content =
            text_editor::Content::with_text("unrelated draft");
        let _ = app.update(Message::Sent(
            room_key.clone(),
            app.account_id.clone(),
            Err("queue failed".into()),
        ));
        assert_eq!(
            app.drafts[&room_key].content.text().trim_end(),
            "room draft"
        );
        assert!(app.drafts[&room_key].error.is_some());
        app.drafts.get_mut(&room_key).unwrap().pending = true;
        app.pending_ops = 1;
        let _ = app.update(Message::Close);
        assert!(
            !app.selection.shutdown,
            "wait for queue admission before stopping sync"
        );
        let _ = app.update(Message::Sent(
            room_key.clone(),
            app.account_id.clone(),
            Ok(()),
        ));
        assert!(app.selection.shutdown);
        assert!(app.drafts[&room_key].content.text().trim().is_empty());
        assert_eq!(
            app.drafts[&second].content.text().trim_end(),
            "unrelated draft"
        );
    }

    #[test]
    fn composer_shortcuts_respect_focus_newlines_and_ime() {
        let key = DraftKey {
            room: "!one:example.org".into(),
            root: None,
        };
        let press = text_editor::KeyPress {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
            modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Enter),
            modifiers: iced::keyboard::Modifiers::CTRL,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        };
        assert!(matches!(
            composer_binding(press.clone(), false, &key, Default::default()),
            Some(text_editor::Binding::Custom(Message::Send(_)))
        ));
        assert!(composer_binding(press.clone(), true, &key, Default::default()).is_none());
        let mut unfocused = press.clone();
        unfocused.status = text_editor::Status::Active;
        assert!(composer_binding(unfocused, false, &key, Default::default()).is_none());
        let mut plain = press.clone();
        plain.modifiers = iced::keyboard::Modifiers::empty();
        assert!(matches!(
            composer_binding(plain.clone(), false, &key, Default::default()),
            Some(text_editor::Binding::Enter)
        ));
        use interconnect::settings::SendShortcut::Enter;
        assert!(matches!(
            composer_binding(plain.clone(), false, &key, Enter),
            Some(text_editor::Binding::Custom(Message::Send(_)))
        ));
        assert!(composer_binding(plain.clone(), true, &key, Enter).is_none());
        plain.modifiers = iced::keyboard::Modifiers::SHIFT;
        assert!(matches!(
            composer_binding(plain, false, &key, Enter),
            Some(text_editor::Binding::Enter)
        ));
        assert!(matches!(
            composer_binding(press, false, &key, Enter),
            Some(text_editor::Binding::Custom(Message::Send(_)))
        ));
        assert!(matches!(
            shell_shortcut(
                &iced::keyboard::Key::Character(",".into()),
                iced::keyboard::Modifiers::COMMAND
            ),
            Some(Message::Settings(settings::Event::Open))
        ));
    }

    #[test]
    fn stale_snapshots_are_rejected_after_navigation() {
        let snapshot = Some(Pane {
            generation: 1,
            rows: Default::default(),
            error: None,
            history: History::Ready,
            history_revision: 0,
            load_older: None,
            composer: None,
        });
        assert!(current_pane(&snapshot, 1).is_some());
        assert!(current_pane(&snapshot, 2).is_none());
    }
    #[test]
    fn pagination_rejects_stale_clicks_and_disables_duplicate_requests() {
        let mut app = App::new();
        let _ = app.update(Message::Room("!room:example.org".into()));
        let generation = app.selection.room.as_ref().unwrap().0;
        let (sender, mut requests) = tokio::sync::mpsc::channel(1);
        app.model.room = Some(Pane {
            generation,
            rows: Default::default(),
            error: None,
            history: History::Ready,
            history_revision: 0,
            load_older: Some(sender),
            composer: None,
        });
        let _ = app.update(Message::LoadOlder {
            thread: false,
            generation: generation + 1,
            revision: 0,
        });
        assert!(requests.try_recv().is_err());
        let _ = app.update(Message::LoadOlder {
            thread: false,
            generation,
            revision: 0,
        });
        assert!(matches!(
            app.model.room.as_ref().unwrap().history,
            History::Loading
        ));
        assert_eq!(requests.try_recv(), Ok(0));
        let _ = app.update(Message::LoadOlder {
            thread: false,
            generation,
            revision: 0,
        });
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn automatic_history_buffers_rooms_and_threads_and_stops_on_failure() {
        for thread in [false, true] {
            let mut app = App::new();
            let _ = app.update(Message::Room("!room:example.org".into()));
            if thread {
                let _ = app.update(Message::Thread("$root".into()));
            }
            let generation = if thread {
                app.selection.thread.as_ref()
            } else {
                app.selection.room.as_ref()
            }
            .unwrap()
            .0;
            let (sender, mut requests) = tokio::sync::mpsc::channel(1);
            let pane = Pane {
                generation,
                rows: Default::default(),
                error: None,
                history: History::Ready,
                history_revision: 1,
                load_older: Some(sender.clone()),
                composer: None,
            };
            if thread {
                app.model.thread = Some(pane);
            } else {
                app.model.room = Some(pane);
            }
            let measured =
                |revision, height, content_height, translation| Message::TimelineMeasured {
                    thread,
                    generation,
                    revision,
                    metrics: timeline::Metrics {
                        height,
                        content_height,
                        translation,
                    },
                };
            // Fill one screen plus three screens of older history, rejecting old pages.
            let _ = app.update(measured(0, 500.0, 1000.0, 500.0));
            assert!(requests.try_recv().is_err());
            let _ = app.update(measured(1, 500.0, 1000.0, 500.0));
            assert_eq!(requests.try_recv(), Ok(1));
            let _ = app.update(measured(1, 500.0, 1000.0, 500.0));
            assert!(requests.try_recv().is_err());
            let pane = if thread {
                app.model.thread.as_mut()
            } else {
                app.model.room.as_mut()
            }
            .unwrap();
            pane.history = History::Ready;
            pane.history_revision = 2;
            pane.load_older = Some(sender.clone());
            let _ = app.update(measured(2, 500.0, 3000.0, 2500.0));
            assert!(requests.try_recv().is_err(), "a sufficient buffer settles");
            // Scrolling toward the top consumes the buffer and fetches the next page.
            let _ = app.update(measured(2, 500.0, 3000.0, 1400.0));
            assert_eq!(requests.try_recv(), Ok(2));
            let pane = if thread {
                app.model.thread.as_mut()
            } else {
                app.model.room.as_mut()
            }
            .unwrap();
            pane.history = History::Failed("offline".into());
            pane.history_revision = 3;
            pane.load_older = Some(sender);
            let _ = app.update(measured(3, 500.0, 3000.0, 0.0));
            assert!(
                requests.try_recv().is_err(),
                "failures do not retry in a loop"
            );
            let _ = app.update(Message::LoadOlder {
                thread,
                generation,
                revision: 2,
            });
            assert!(requests.try_recv().is_err());
            let _ = app.update(Message::LoadOlder {
                thread,
                generation,
                revision: 3,
            });
            assert_eq!(requests.try_recv(), Ok(3));
        }
    }

    #[test]
    fn history_refills_for_resizing_and_before_restoring_a_distant_position() {
        let mut app = App::new();
        let _ = app.update(Message::Room("!room:example.org".into()));
        let generation = app.selection.room.as_ref().unwrap().0;
        let (sender, mut requests) = tokio::sync::mpsc::channel(1);
        app.model.room = Some(Pane {
            generation,
            rows: Default::default(),
            error: None,
            history: History::Ready,
            history_revision: 1,
            load_older: Some(sender.clone()),
            composer: None,
        });
        let measured = |revision, height, content_height| Message::TimelineMeasured {
            thread: false,
            generation,
            revision,
            metrics: timeline::Metrics {
                height,
                content_height,
                translation: content_height - height,
            },
        };
        let _ = app.update(measured(1, 400.0, 2000.0));
        assert!(requests.try_recv().is_err());
        let _ = app.update(measured(1, 800.0, 2000.0));
        assert_eq!(
            requests.try_recv(),
            Ok(1),
            "a larger viewport needs more buffer"
        );
        app.pending_scroll[0] = Some(3200.0);
        let pane = app.model.room.as_mut().unwrap();
        pane.history = History::Ready;
        pane.history_revision = 2;
        pane.load_older = Some(sender.clone());
        let _ = app.update(measured(2, 500.0, 4500.0));
        assert_eq!(requests.try_recv(), Ok(2));
        assert_eq!(
            app.pending_scroll[0],
            Some(3200.0),
            "do not clamp a saved position before fetching it"
        );
        let pane = app.model.room.as_mut().unwrap();
        pane.history = History::Ready;
        pane.history_revision = 3;
        pane.load_older = Some(sender);
        let _ = app.update(measured(3, 500.0, 6500.0));
        assert!(requests.try_recv().is_err());
        assert!(app.pending_scroll[0].is_none());
    }

    #[test]
    fn switching_rooms_clears_thread_and_advances_generation() {
        let mut app = App::new();
        let _ = app.update(Message::Room("!one:example.org".into()));
        let first = app.selection.room.clone();
        let _ = app.update(Message::Thread("$root".into()));
        let _ = app.update(Message::Room("!two:example.org".into()));
        assert!(app.selection.thread.is_none());
        assert!(app.selection.room.as_ref().unwrap().0 > first.unwrap().0);
        let _ = app.update(Message::Close);
        let selected = app.selection.clone();
        let _ = app.update(Message::Room("!three:example.org".into()));
        assert_eq!(app.selection, selected);
    }
}
