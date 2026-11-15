#![recursion_limit = "256"]
#[path = "matrix-ui/demo.rs"]
mod demo;
#[path = "matrix-ui/presentation.rs"]
mod presentation;
#[path = "matrix-ui/selectable.rs"]
mod selectable;
#[path = "matrix-ui/selection.rs"]
mod selection;
#[path = "matrix-ui/shell.rs"]
mod shell;
#[path = "matrix-ui/timeline.rs"]
mod timeline;

use chatclient::connected::{History, MessageRow, Model, Pane, RetrySend, Selection};
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
    scroll_positions: HashMap<DraftKey, f32>,
    pending_scroll: [Option<f32>; 2],
    timeline_metrics: [Option<(u64, u64, timeline::Metrics)>; 2],
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
            scroll_positions: HashMap::new(),
            pending_scroll: [None, None],
            timeline_metrics: [None, None],
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
        match message {
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
                    return jump_to_newest(key.root.is_some());
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
                self.pending_scroll = [Some(self.saved_scroll(false)), None];
                self.drafts
                    .entry(self.draft_key(false).unwrap())
                    .or_default();
            }
            Message::Thread(id)
                if !self.closing && !self.stopped && self.selection.room.is_some() =>
            {
                self.generation += 1;
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
                self.selection.thread = None;
                self.pending_scroll[1] = None;
                self.pending_scroll[0] = Some(self.saved_scroll(false));
            }
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
        self.restore_scroll()
    }
    fn saved_scroll(&self, thread: bool) -> f32 {
        self.draft_key(thread)
            .and_then(|key| self.scroll_positions.get(&key).copied())
            .unwrap_or(0.0)
    }
    fn restore_scroll(&mut self) -> Task<Message> {
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
    fn thread_original(&self) -> Option<&MessageRow> {
        let (generation, root) = self.selection.thread.as_ref()?;
        current_pane(&self.model.thread, *generation)
            .and_then(|pane| {
                pane.rows
                    .iter()
                    .find(|row| row.event_id.as_ref() == Some(root))
            })
            .or_else(|| {
                let (generation, _) = self.selection.room.as_ref()?;
                current_pane(&self.model.room, *generation)?
                    .rows
                    .iter()
                    .find(|row| row.event_id.as_ref() == Some(root))
            })
    }

    fn thread_original_view(&self) -> Element<'_, Message> {
        let original: Element<'_, Message> = if let Some(message) = self.thread_original() {
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
                selectable::named_view(
                    message.event_id.as_deref().unwrap_or(&message.body),
                    &message.body,
                    &message.display_name,
                ),
            ]
            .spacing(6)
            .into()
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
        let rows: Vec<_> = pane
            .rows
            .iter()
            .filter(|message| !thread || message.event_id.as_ref() != Some(selected_id))
            .collect();
        let mut messages = column![history_controls].spacing(0).padding([16, 24]);
        for (index, message) in rows.iter().copied().enumerate() {
            let previous = index.checked_sub(1).map(|i| rows[i]);
            let grouped = presentation::grouped(previous, message);
            let new_date = presentation::date(message)
                .filter(|date| previous.and_then(presentation::date).as_ref() != Some(date));
            let timestamp = iced::widget::tooltip(
                text(presentation::time_label(message))
                    .size(11)
                    .color(shell::MUTED),
                text(presentation::time_detail(message)).size(12),
                iced::widget::tooltip::Position::Top,
            );
            let body = selectable::named_view(
                message.event_id.as_deref().unwrap_or(&message.body),
                &message.body,
                &message.display_name,
            );
            let mut entry = if grouped {
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
                let label = if message.thread_replies == 1 {
                    "1 reply  ›".into()
                } else {
                    format!("{} replies  ›", message.thread_replies)
                };
                let mut replies = button(text(label).size(12))
                    .style(shell::quiet_button)
                    .padding([4, 0]);
                if !self.closing && !self.stopped {
                    replies = replies.on_press(Message::Thread(id.clone()));
                }
                entry = entry.push(replies);
            }
            let avatar: Element<'_, Message> = if grouped {
                iced::widget::space().width(32).into()
            } else {
                shell::avatar(&message.display_name)
            };
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
            let body = container(row![avatar, entry.width(Length::Fill)].spacing(12)).padding(
                iced::Padding {
                    top: if grouped { 5.0 } else { 18.0 },
                    ..iced::Padding::ZERO
                },
            );
            let body: Element<'_, Message> = if !thread && let Some(id) = &message.event_id {
                shell::message_actions(body, id, !self.closing && !self.stopped)
            } else {
                body.into()
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
        let mut message_area = iced::widget::stack![timeline::history_view(
            *generation,
            rows.iter().map(|row| row.event_id.clone()).collect(),
            timeline.into(),
            thread,
            pane.history_revision,
            matches!(pane.history, History::Ready),
        )];
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
            let mut editor = text_editor(&draft.content)
                .id(if thread {
                    "thread-composer"
                } else {
                    "room-composer"
                })
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
                && !draft.content.text().trim().is_empty()
            {
                send = send.on_press(Message::Send(key));
            }
            let send = iced::widget::tooltip(
                send,
                container(
                    text(if draft.pending {
                        "Queueing message…"
                    } else if thread {
                        "Send reply (Ctrl+Enter)"
                    } else {
                        "Send message (Ctrl+Enter)"
                    })
                    .size(12),
                )
                .padding(8)
                .style(container::rounded_box),
                iced::widget::tooltip::Position::Top,
            );
            let actions = row![iced::widget::space().width(Length::Fill), send]
                .align_y(iced::Alignment::Center);
            let composer = iced::widget::stack![
                editor,
                container(actions)
                    .align_bottom(Length::Fill)
                    .padding([4, 6]),
            ];
            content = content.push(container(composer).padding([10, 24]));
            if let Some(error) = &draft.error {
                content = content.push(text(error));
            }
        }
        content.into()
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
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
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
            let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
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
