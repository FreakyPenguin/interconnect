#![recursion_limit = "256"]
#[path = "matrix-ui/demo.rs"]
mod demo;
#[path = "matrix-ui/shell.rs"]
mod shell;

use chatclient::connected::{History, Model, Pane, RetrySend, Selection};
use chatclient::navigation::Scope;
use chatclient::navigation_preferences::Preferences;
use iced::{
    Element, Length, Subscription, Task,
    futures::SinkExt,
    widget::{button, column, container, row, scrollable, text, text_editor},
};
use std::collections::{HashMap, HashSet};
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DraftKey {
    room: String,
    root: Option<String>,
}
#[derive(Default)]
struct Draft {
    content: text_editor::Content,
    pending: bool,
    error: Option<String>,
}

const MAX_PENDING_OPERATIONS: usize = 8;

struct App {
    profile: String,
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
}
#[derive(Clone, Debug)]
enum Message {
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
    Stopped(Result<(), String>),
    Room(String),
    Thread(String),
    CloseThread,
    LoadOlder(u64),
    Close,
    Exit,
    Edit(DraftKey, text_editor::Action),
    Send(DraftKey),
    Sent(DraftKey, Result<(), String>),
    Retry(RetrySend),
    Retried(String, Result<(), String>),
    Composing(bool),
}
impl App {
    fn new() -> Self {
        let mut app = Self {
            profile: std::env::args()
                .nth(1)
                .unwrap_or_else(|| ".matrix-spike".into()),
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
            demo: None,
            preferences_ready: false,
            pending_navigation: None,
        };
        if std::env::args().any(|arg| arg == "--demo") {
            app.demo = Some(demo::Demo::default());
            app.model.rooms = demo::rooms().into();
            app.model.spaces = demo::spaces().into();
            app.shell.space = Scope::Space("studio-space".into());
            app.model.status = "Design preview · local sample data".into();
            let _ = app.update(Message::Room("studio".into()));
        }
        app
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        chatclient::diagnostics::update();
        if matches!(
            &message,
            Message::Space(_) | Message::Room(_) | Message::Section(_)
        ) {
            self.pending_navigation = None;
        }

        let navigation = matches!(
            &message,
            Message::Room(_) | Message::Thread(_) | Message::CloseThread
        );
        let scroll_target = match &message {
            Message::Room(_) => Some("room-timeline"),
            Message::Thread(_) => Some("thread-timeline"),
            _ => None,
        };
        match message {
            Message::Space(space) if !self.closing && !self.stopped => {
                self.shell.space = space;
                self.shell.query.clear();
                let rooms = self.visible_rooms();
                if !self
                    .selection
                    .room
                    .as_ref()
                    .is_some_and(|(_, id)| rooms.contains(id))
                {
                    if let Some(room) = rooms.first() {
                        return self.update(Message::Room(room.clone()));
                    }
                    self.selection.room = None;
                    self.selection.thread = None;
                }
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
                    let index = rooms
                        .iter()
                        .position(|id| {
                            self.selection
                                .room
                                .as_ref()
                                .is_some_and(|(_, selected)| selected == id)
                        })
                        .unwrap_or(0);
                    let next = if previous {
                        (index + rooms.len() - 1) % rooms.len()
                    } else {
                        (index + 1) % rooms.len()
                    };
                    return self.update(Message::Room(rooms[next].clone()));
                }
            }
            Message::OpenLastThread => {
                if let Some(root) = self
                    .model
                    .room
                    .as_ref()
                    .and_then(|p| p.rows.last())
                    .and_then(|r| r.event_id.clone())
                {
                    let task = self.update(Message::Thread(root));
                    return task.chain(iced::widget::operation::focus("thread-composer"));
                }
            }
            Message::SearchFocus => {
                self.shell.navigation = true;
                return iced::widget::operation::focus("room-search")
                    .chain(iced::widget::operation::select_all("room-search"));
            }
            Message::ToggleNavigation => self.shell.navigation = !self.shell.navigation,
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
                        .update(Message::CloseThread)
                        .chain(iced::widget::operation::focus("room-composer"));
                }
            }

            Message::Composing(active) => self.composing = active,
            Message::Edit(key, action) if !self.closing && !self.stopped => {
                if (self.draft_key(false).as_ref() == Some(&key)
                    || self.draft_key(true).as_ref() == Some(&key))
                    && let Some(draft) = self.drafts.get_mut(&key)
                    && !draft.pending
                {
                    draft.content.perform(action);
                }
            }
            Message::Send(key)
                if !self.closing
                    && !self.stopped
                    && !self.composing
                    && self.pending_ops < MAX_PENDING_OPERATIONS =>
            {
                if self.demo.is_some() {
                    if let Some(draft) = self.drafts.get_mut(&key) {
                        let body = draft.content.text();
                        if !body.trim().is_empty() {
                            self.demo.as_mut().unwrap().send(&key, body);
                            draft.content = text_editor::Content::new();
                            self.refresh_demo();
                        }
                    }
                    return iced::widget::operation::snap_to_end(if key.root.is_some() {
                        "thread-timeline"
                    } else {
                        "room-timeline"
                    });
                }
                if let Some(composer) = self.composer_for(&key)
                    && let Some(draft) = self.drafts.get_mut(&key)
                    && !draft.pending
                    && !draft.content.text().trim().is_empty()
                {
                    let body = draft.content.text();
                    draft.pending = true;
                    draft.error = None;
                    self.pending_ops += 1;
                    return Task::perform(composer.send(body), move |result| {
                        Message::Sent(key.clone(), result)
                    });
                }
            }
            Message::Sent(key, result) => {
                self.pending_ops -= 1;
                if let Some(draft) = self.drafts.get_mut(&key) {
                    draft.pending = false;
                    match result {
                        Ok(()) => {
                            draft.content = text_editor::Content::new();
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
            Message::Control(sender) => {
                sender.send_replace(self.selection.clone());
                self.control = Some(sender);
            }
            Message::State(state) => {
                self.model = *state;
                if let Some(room) = self.restore_navigation() {
                    return self.update(Message::Room(room));
                }
                if self.model.navigation_error.is_none() {
                    if let Scope::Space(id) = &self.shell.space
                        && !self.model.spaces.iter().any(|s| &s.id == id)
                    {
                        self.shell.space = Scope::All;
                    }
                    let members =
                        chatclient::navigation::members(&self.shell.space, &self.model.spaces);
                    if self.selection.room.as_ref().is_some_and(|(_, id)| {
                        !self.model.rooms.iter().any(|r| {
                            &r.id == id
                                && chatclient::navigation::includes(&self.shell.space, &members, r)
                        })
                    }) {
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
                if self.stopped && self.pending_ops == 0 {
                    return iced::exit();
                }
                self.closing = true;
                self.selection.shutdown = self.pending_ops == 0;
                self.model.status = "Stopping synchronization…".into();
            }
            Message::Exit => return self.update(Message::Close),
            Message::Room(id) if !self.closing && !self.stopped => {
                self.generation += 1;
                self.selection.room = Some((self.generation, id));
                self.selection.thread = None;
                self.drafts
                    .entry(self.draft_key(false).unwrap())
                    .or_default();
            }
            Message::Thread(id) if !self.closing && !self.stopped => {
                self.generation += 1;
                self.selection.thread = Some((self.generation, id));
                if let Some(key) = self.draft_key(true) {
                    self.drafts.entry(key).or_default();
                }
            }
            Message::LoadOlder(generation) if !self.closing && !self.stopped => {
                if self
                    .selection
                    .room
                    .as_ref()
                    .is_some_and(|(current, _)| *current == generation)
                    && let Some(pane) = self
                        .model
                        .room
                        .as_mut()
                        .filter(|p| p.generation == generation)
                    && let Some(sender) = &pane.load_older
                    && sender.try_send(()).is_ok()
                {
                    pane.history = History::Loading;
                    pane.load_older = None;
                }
            }
            Message::CloseThread => self.selection.thread = None,
            _ => {}
        }
        if navigation {
            self.refresh_demo();
        }
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
        scroll_target.map_or_else(Task::none, iced::widget::operation::snap_to_end)
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
        if let Some(id) = &saved.room {
            let members = chatclient::navigation::members(&saved.scope, &self.model.spaces);
            if !self
                .model
                .rooms
                .iter()
                .any(|r| &r.id == id && chatclient::navigation::includes(&saved.scope, &members, r))
            {
                return None;
            }
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
    fn composer_for(&self, key: &DraftKey) -> Option<chatclient::connected::Composer> {
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
            if self.demo.is_some() {
                Subscription::run(|| {
                    iced::stream::channel(1, async |_output| {
                        chatclient::diagnostics::Diagnostics::default()
                            .reports()
                            .await;
                    })
                })
            } else {
                Subscription::run_with(self.profile.clone(), |profile| connection(profile))
            },
            if self.composing {
                Subscription::none()
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
            if chatclient::diagnostics::enabled() {
                iced::event::listen_raw(|event, _, _| {
                    if matches!(
                        event,
                        iced::Event::Window(iced::window::Event::RedrawRequested(_))
                    ) {
                        chatclient::diagnostics::redraw();
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
        self.shell_view()
    }
    fn refresh_demo(&mut self) {
        if let Some(demo) = &mut self.demo {
            self.model.room = self.selection.room.as_ref().map(|(generation, room)| {
                demo.pane(
                    DraftKey {
                        room: room.clone(),
                        root: None,
                    },
                    *generation,
                )
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
        let Some((generation, _)) = selected else {
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
        let mut header = row![
            column![
                shell::title(heading, 18),
                text(if thread {
                    "A focused conversation"
                } else {
                    "Messages"
                })
                .size(12)
                .color(shell::MUTED)
            ]
            .spacing(5),
            iced::widget::space().width(Length::Fill)
        ]
        .align_y(iced::Alignment::Center);
        if thread {
            header = header.push(
                button("Close")
                    .style(shell::quiet_button)
                    .on_press(Message::CloseThread),
            );
        }
        let mut content = column![
            container(header).padding([18, 24]),
            iced::widget::rule::horizontal(1)
        ]
        .spacing(0);
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
        match &pane.history {
            History::Loading => content = content.push(text("Loading room history…")),
            History::Failed(error) => {
                content = content.push(text(format!(
                    "Could not load history: {error}. Live messages remain active."
                )))
            }
            History::Ready => {}
            History::End => {
                content = content.push(
                    container(
                        text("Beginning of conversation")
                            .size(11)
                            .color(shell::MUTED),
                    )
                    .padding([12, 24])
                    .center_x(Length::Fill),
                )
            }
        }
        if !thread && !matches!(pane.history, History::End) {
            let label = match pane.history {
                History::Loading => "Loading…",
                History::Failed(_) => "Retry loading history",
                _ => "Load older",
            };
            let mut control = button(label).style(shell::quiet_button);
            if pane.load_older.is_some() && !self.closing && !self.stopped {
                control = control.on_press(Message::LoadOlder(*generation));
            }
            content = content.push(control);
        }
        let mut messages = column![].spacing(20).padding([16, 24]);
        for message in pane.rows.iter() {
            let mut entry = column![
                shell::title(&message.sender, 13),
                text(&message.body).size(14)
            ]
            .spacing(6);
            if !message.send_state.is_empty() {
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
                && !self.closing
                && !self.stopped
                && let Some(id) = &message.event_id
            {
                entry = entry.push(
                    button(text("Reply in thread  ›").size(12))
                        .style(shell::quiet_button)
                        .padding([4, 0])
                        .on_press(Message::Thread(id.clone())),
                );
            }
            messages = messages
                .push(row![shell::avatar(&message.sender), entry.width(Length::Fill)].spacing(12));
        }
        if pane.rows.is_empty() && matches!(pane.history, History::Ready | History::End) {
            messages = messages.push(shell::empty_state(
                "The start of something",
                "Send the first message in this conversation.",
            ));
        }
        content = content.push(
            scrollable(messages)
                .direction(shell::scrollbar())
                .style(shell::scroll_style)
                .id(if thread {
                    "thread-timeline"
                } else {
                    "room-timeline"
                })
                .height(Length::Fill),
        );
        if let Some(key) = self.draft_key(thread)
            && let Some(draft) = self.drafts.get(&key)
        {
            let target = if thread {
                format!(
                    "Reply in thread · {}",
                    self.model
                        .rooms
                        .iter()
                        .find(|r| r.id == key.room)
                        .map(|r| r.name.as_str())
                        .unwrap_or(&key.room)
                )
            } else {
                format!(
                    "Message room · {}",
                    self.model
                        .rooms
                        .iter()
                        .find(|r| r.id == key.room)
                        .map(|r| r.name.as_str())
                        .unwrap_or(&key.room)
                )
            };
            let edit_key = key.clone();
            let send_key = key.clone();
            let composing = self.composing;
            let mut editor = text_editor(&draft.content)
                .id(if thread {
                    "thread-composer"
                } else {
                    "room-composer"
                })
                .cursor_blinking(false)
                .height(88)
                .padding(12)
                .style(shell::editor_style)
                .placeholder(if thread {
                    "Reply to this thread…"
                } else {
                    "Write a message…"
                });
            if !self.closing && !self.stopped && !draft.pending {
                editor = editor
                    .on_action(move |action| Message::Edit(edit_key.clone(), action))
                    .key_binding(move |press| composer_binding(press, composing, &send_key));
            }
            let mut send = button(if draft.pending { "Queueing…" } else { "Send" })
                .padding([7, 16])
                .style(shell::send_button);
            if !self.closing
                && !self.stopped
                && !self.composing
                && !draft.pending
                && self.pending_ops < MAX_PENDING_OPERATIONS
                && (pane.composer.is_some() || self.demo.is_some())
                && !draft.content.text().trim().is_empty()
            {
                send = send.on_press(Message::Send(key));
            }
            let composer = column![
                text(target).size(11).color(shell::MUTED),
                editor,
                row![
                    text(if self.demo.is_some() {
                        "Local preview · messages stay here"
                    } else {
                        "Ctrl+Enter to send"
                    })
                    .size(11)
                    .color(shell::MUTED),
                    iced::widget::space().width(Length::Fill),
                    send
                ]
                .align_y(iced::Alignment::Center)
            ]
            .spacing(9);
            content = content.push(container(composer).padding([16, 24]));
            if let Some(error) = &draft.error {
                content = content.push(text(error));
            }
        }
        content.into()
    }
}
fn shell_shortcut(
    key: &iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Option<Message> {
    use iced::keyboard::{Key, key::Named};
    match key.as_ref() {
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
) -> Option<text_editor::Binding<Message>> {
    if shell_shortcut(&press.key, press.modifiers).is_some() {
        return None;
    }
    if matches!(press.status, text_editor::Status::Focused { .. })
        && press.key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter)
        && press.modifiers == iced::keyboard::Modifiers::CTRL
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
        let diagnostics = state.borrow().diagnostics.clone();
        let backend = chatclient::connected::run(profile, selection, state);
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
    let cleanup = chatclient::shutdown_runtime(runtime, std::time::Duration::from_secs(5));
    if let Err(error) = &cleanup {
        eprintln!("{error:#}");
    }
    result?;
    cleanup
}

fn main() -> anyhow::Result<()> {
    chatclient::init_tracing();
    run_ui(|| {
        iced::application(App::new, App::update, App::view)
            .title("Matrix")
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

#[cfg(test)]
mod tests {
    use super::*;
    fn demo_app() -> App {
        let mut app = App::new();
        app.demo = Some(demo::Demo::default());
        app.model.rooms = demo::rooms().into();
        app.model.spaces = demo::spaces().into();
        app.shell.space = Scope::Space("studio-space".into());
        let _ = app.update(Message::Room("studio".into()));
        app
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
        assert_eq!(app.pending_ops, 0);
        assert!(app.control.is_none());
        assert!(app.composer_for(&thread_key).is_none());
        assert!(app.drafts[&thread_key].content.text().trim().is_empty());
        assert_eq!(app.drafts[&room_key].content.text().trim(), "room draft");
        let _ = app.update(Message::Space(Scope::Space("home-space".into())));
        assert!(app.selection.thread.is_none());
        assert!(
            app.model
                .room
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .all(|r| r.body.trim() != "a local reply")
        );
        let _ = app.update(Message::Space(Scope::Space("studio-space".into())));
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
    fn saved_navigation_waits_for_metadata_and_restores_without_a_thread() {
        let mut app = App::new();
        let prefs = Preferences {
            scope: Scope::Space("home-space".into()),
            room: Some("alex".into()),
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
        assert_eq!(app.selection.room.as_ref().unwrap().1, "alex");
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
    fn focused_search_schedules_redraw_only_when_blinking_is_enabled() {
        use iced::advanced::{Shell, Widget, layout, mouse, widget};
        use iced::window::RedrawRequest;
        for blinking in [false, true] {
            let mut editor = iced::widget::text_input("Find a room", "fixture")
                .on_input(std::convert::identity)
                .cursor_blinking(blinking);
            let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
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
            let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
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
        let _ = app.update(Message::Sent(room_key.clone(), Err("queue failed".into())));
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
        let _ = app.update(Message::Sent(room_key.clone(), Ok(())));
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
            composer_binding(press.clone(), false, &key),
            Some(text_editor::Binding::Custom(Message::Send(_)))
        ));
        assert!(composer_binding(press.clone(), true, &key).is_none());
        let mut unfocused = press.clone();
        unfocused.status = text_editor::Status::Active;
        assert!(composer_binding(unfocused, false, &key).is_none());
        let mut plain = press;
        plain.modifiers = iced::keyboard::Modifiers::empty();
        assert!(matches!(
            composer_binding(plain, false, &key),
            Some(text_editor::Binding::Enter)
        ));
    }

    #[test]
    fn stale_snapshots_are_rejected_after_navigation() {
        let snapshot = Some(Pane {
            generation: 1,
            rows: Default::default(),
            error: None,
            history: History::Ready,
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
            load_older: Some(sender),
            composer: None,
        });
        let _ = app.update(Message::LoadOlder(generation + 1));
        assert!(requests.try_recv().is_err());
        let _ = app.update(Message::LoadOlder(generation));
        assert!(matches!(
            app.model.room.as_ref().unwrap().history,
            History::Loading
        ));
        assert_eq!(requests.try_recv(), Ok(()));
        let _ = app.update(Message::LoadOlder(generation));
        assert!(requests.try_recv().is_err());
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
