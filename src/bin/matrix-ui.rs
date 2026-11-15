#![recursion_limit = "256"]
use chatclient::connected::{History, Model, Pane, RetrySend, Selection};
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
}
#[derive(Clone, Debug)]
enum Message {
    Control(watch::Sender<Selection>),
    State(Model),
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
        Self {
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
        }
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        chatclient::diagnostics::update();
        match message {
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
            Message::State(state) => self.model = state,
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
        if self.closing && self.pending_ops == 0 {
            if self.stopped {
                return iced::exit();
            }
            self.selection.shutdown = true;
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
        Task::none()
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
            Subscription::run_with(self.profile.clone(), |profile| connection(profile)),
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
        let mut rooms = column![text("Rooms").size(22)].spacing(8);
        for room in self.model.rooms.iter() {
            let selected = self
                .selection
                .room
                .as_ref()
                .is_some_and(|(_, id)| id == &room.id);
            let label = if selected {
                format!("› {}", room.name)
            } else {
                room.name.clone()
            };
            let mut control = button(text(label)).width(Length::Fill);
            if !self.closing && !self.stopped {
                control = control.on_press(Message::Room(room.id.clone()));
            }
            rooms = rooms.push(control);
        }
        let mut panes = row![
            scrollable(rooms).width(240).height(Length::Fill),
            container(self.pane(false)).width(Length::FillPortion(2)),
        ]
        .spacing(16)
        .height(Length::Fill);
        if self.selection.thread.is_some() {
            panes = panes.push(container(self.pane(true)).width(Length::FillPortion(2)));
        }
        let mut content = column![
            text("Matrix — connected prototype").size(24),
            text(&self.model.status),
            panes
        ]
        .spacing(12)
        .padding(16);
        if self.stopped {
            content = content.push(button("Exit").on_press(Message::Exit));
        }
        content.into()
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
            return text("Select an encrypted room to read messages.").into();
        };
        let mut content =
            column![text(if thread { "Thread" } else { "Room timeline" }).size(22)].spacing(10);
        if thread {
            content = content.push(button("Close thread").on_press(Message::CloseThread));
        }
        let Some(pane) = current_pane(snapshot, *generation) else {
            return content.push(text("Loading…")).into();
        };
        if let Some(error) = &pane.error {
            return content.push(text(error)).into();
        }
        match &pane.history {
            History::Loading => content = content.push(text("Loading room history…")),
            History::Failed(error) => {
                content = content.push(text(format!(
                    "Could not load history: {error}. Live messages remain active."
                )))
            }
            History::Ready => {}
            History::End => content = content.push(text("Beginning of history reached.")),
        }
        if !thread && !matches!(pane.history, History::End) {
            let label = match pane.history {
                History::Loading => "Loading…",
                History::Failed(_) => "Retry loading history",
                _ => "Load older",
            };
            let mut control = button(label);
            if pane.load_older.is_some() && !self.closing && !self.stopped {
                control = control.on_press(Message::LoadOlder(*generation));
            }
            content = content.push(control);
        }
        let mut messages = column![].spacing(12);
        for message in pane.rows.iter() {
            let mut entry = column![text(&message.sender).size(14), text(&message.body)].spacing(4);
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
                entry = entry.push(button("Open thread").on_press(Message::Thread(id.clone())));
            }
            messages = messages.push(container(entry).padding(8));
        }
        if pane.rows.is_empty() && matches!(pane.history, History::Ready | History::End) {
            messages = messages.push(text("No messages loaded."));
        }
        content = content.push(scrollable(messages).height(Length::Fill));
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
                .cursor_blinking(false)
                .height(110)
                .placeholder("Write a message…");
            if !self.closing && !self.stopped && !draft.pending {
                editor = editor
                    .on_action(move |action| Message::Edit(edit_key.clone(), action))
                    .key_binding(move |press| composer_binding(press, composing, &send_key));
            }
            let mut send = button(if draft.pending { "Queueing…" } else { "Send" });
            if !self.closing
                && !self.stopped
                && !self.composing
                && !draft.pending
                && self.pending_ops < MAX_PENDING_OPERATIONS
                && pane.composer.is_some()
                && !draft.content.text().trim().is_empty()
            {
                send = send.on_press(Message::Send(key));
            }
            content = content
                .push(text(target).size(14))
                .push(editor)
                .push(row![send, text("Enter: new line · Ctrl+Enter: send").size(12)].spacing(8));
            if let Some(error) = &draft.error {
                content = content.push(text(error));
            }
        }
        content.into()
    }
}
fn composer_binding(
    press: text_editor::KeyPress,
    composing: bool,
    key: &DraftKey,
) -> Option<text_editor::Binding<Message>> {
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
                    if output.send(Message::State(snapshot)).await.is_err() {
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
            .executor::<UiExecutor>()
            .subscription(App::subscription)
            .exit_on_close_request(false)
            .run()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
