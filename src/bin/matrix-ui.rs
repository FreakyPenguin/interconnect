use chatclient::connected::{History, Model, Pane, Selection};
use iced::{
    Element, Length, Subscription, Task,
    futures::SinkExt,
    widget::{button, column, container, row, scrollable, text},
};
use tokio::sync::watch;

struct App {
    profile: String,
    model: Model,
    selection: Selection,
    control: Option<watch::Sender<Selection>>,
    generation: u64,
    closing: bool,
    stopped: bool,
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
        }
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
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
                if self.stopped {
                    return iced::exit();
                }
                self.closing = true;
                self.selection.shutdown = true;
                self.model.status = "Stopping synchronization…".into();
            }
            Message::Exit => return iced::exit(),
            Message::Room(id) if !self.closing && !self.stopped => {
                self.generation += 1;
                self.selection.room = Some((self.generation, id));
                self.selection.thread = None;
            }
            Message::Thread(id) if !self.closing && !self.stopped => {
                self.generation += 1;
                self.selection.thread = Some((self.generation, id));
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
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run_with(self.profile.clone(), |profile| connection(profile)),
            iced::window::close_requests().map(|_| Message::Close),
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
        content
            .push(scrollable(messages).height(Length::Fill))
            .into()
    }
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
            }
        };
        let _ = output.send(Message::Stopped(result)).await;
        std::future::pending::<()>().await;
    })
}
fn main() -> iced::Result {
    chatclient::init_tracing();
    iced::application(App::new, App::update, App::view)
        .subscription(App::subscription)
        .exit_on_close_request(false)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_snapshots_are_rejected_after_navigation() {
        let snapshot = Some(Pane {
            generation: 1,
            rows: Default::default(),
            error: None,
            history: History::Ready,
            load_older: None,
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
