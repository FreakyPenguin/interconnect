//! Quick-open people picker; only choosing a person creates a server-side room.
use super::{App, MAX_PENDING_OPERATIONS, Message, people_picker, shell};
use iced::{
    Element, Length, Task,
    keyboard::{Key, Modifiers, key::Named},
    widget::{column, container, mouse_area, opaque, text},
};
use interconnect::{navigation::RoomEntry, people::parse_user_id};

pub struct State {
    request: u64,
    account: Option<String>,
    thread: bool,
    picker: people_picker::State,
    error: Option<String>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Picker(u64, people_picker::Event),
    Move(bool),
    Submit,
    Close,
    Ignore,
    Completed(u64, Option<String>, Result<RoomEntry, String>),
}
fn message(event: Event) -> Message {
    Message::NewChat(event)
}
pub fn is_shortcut(key: &Key, modifiers: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character("n" | "N"))
        && (modifiers.control() || modifiers.command())
        && !modifiers.alt()
        && !modifiers.shift()
}
pub fn shortcut(
    event: iced::Event,
    _: iced::event::Status,
    _: iced::window::Id,
) -> Option<Message> {
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event
    else {
        return None;
    };
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(message(Event::Close)),
        Key::Named(Named::ArrowUp) => Some(message(Event::Move(true))),
        Key::Named(Named::ArrowDown) => Some(message(Event::Move(false))),
        Key::Named(Named::Tab) => Some(message(Event::Open)),
        _ if is_shortcut(&key, modifiers) => Some(message(Event::Open)),
        _ => None,
    }
}
impl App {
    pub(super) fn new_chat_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open => {
                if self.closing
                    || self.stopped
                    || self.room_picker.is_some()
                    || self.settings.draft.is_some()
                    || self.verification.open
                    || self.image_view.is_some()
                    || self.reaction_picker.is_some()
                    || self.composer_picker.is_some()
                    || self.invite.is_some()
                    || self.topic_editor.is_some()
                    || (self.demo.is_none() && self.model.authentication.is_some())
                {
                    return Task::none();
                }
                if self.new_chat.is_none() {
                    self.stop_typing();
                    self.emoji_completion.clear();
                    self.generation += 1;
                    self.new_chat = Some(State {
                        request: self.generation,
                        account: self.account_id.clone(),
                        thread: self.selection.thread.is_some(),
                        picker: Default::default(),
                        error: None,
                    });
                }
                return iced::widget::operation::focus(people_picker::SEARCH_ID);
            }
            Event::Close => {
                if let Some(state) = self.new_chat.take() {
                    return iced::widget::operation::focus(if state.thread {
                        "thread-composer"
                    } else {
                        "room-composer"
                    });
                }
            }
            Event::Move(previous) => {
                if let Some(state) = &self.new_chat {
                    return self.new_chat_event(Event::Picker(
                        state.request,
                        people_picker::Event::Move(previous),
                    ));
                }
            }
            Event::Picker(request, event) => {
                let Some(state) = &mut self.new_chat else {
                    return Task::none();
                };
                if state.request != request || self.direct_request.is_some() {
                    return Task::none();
                }
                if matches!(event, people_picker::Event::Submit) {
                    return self.new_chat_event(Event::Submit);
                }
                let choose = matches!(event, people_picker::Event::Select(_));
                state.error = None;
                let task = state
                    .picker
                    .update(
                        event,
                        self.model.people_directory.clone(),
                        self.demo.is_some(),
                    )
                    .map(move |event| message(Event::Picker(request, event)));
                if choose && state.picker.selected.is_some() {
                    return self.new_chat_event(Event::Submit);
                }
                return task;
            }
            Event::Submit => {
                if self.closing
                    || self.stopped
                    || self.composing
                    || self.direct_request.is_some()
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                {
                    return Task::none();
                }
                let Some(state) = &mut self.new_chat else {
                    return Task::none();
                };
                let user = parse_user_id(&state.picker.query).or_else(|error| {
                    state
                        .picker
                        .results
                        .get(state.picker.highlighted)
                        .map(|p| parse_user_id(&p.user_id))
                        .unwrap_or(Err(error))
                });
                let user = match user {
                    Ok(user) => user.to_string(),
                    Err(error) => {
                        state.error = Some(error);
                        return Task::none();
                    }
                };
                if self.account_id.as_ref() == Some(&user) {
                    state.error = Some("Choose someone other than yourself.".into());
                    return Task::none();
                }
                let person = state
                    .picker
                    .selected
                    .as_ref()
                    .filter(|p| p.user_id == user)
                    .or_else(|| state.picker.results.iter().find(|p| p.user_id == user));
                let request = state.request;
                let account = state.account.clone();
                if self.demo.is_some() {
                    let id = format!("demo-dm:{user}");
                    let room = self
                        .model
                        .rooms
                        .iter()
                        .find(|r| r.id == id)
                        .cloned()
                        .unwrap_or_else(|| RoomEntry {
                            id,
                            name: person.map_or_else(|| user.clone(), |p| p.name.clone()),
                            direct: true,
                            member_count: 1,
                            ..Default::default()
                        });
                    state.picker.cancel_search();
                    self.pending_ops += 1;
                    self.direct_request = Some(request);
                    return self.new_chat_event(Event::Completed(request, account, Ok(room)));
                }
                let Some(directory) = self.model.people_directory.clone() else {
                    state.error = Some("The server directory is not ready yet.".into());
                    return Task::none();
                };
                state.picker.cancel_search();
                state.error = None;
                self.pending_ops += 1;
                self.direct_request = Some(request);
                // A creation write is not aborted on dismissal; shutdown waits for it.
                return Task::perform(directory.open_direct(user), move |result| {
                    message(Event::Completed(request, account.clone(), result))
                });
            }
            Event::Completed(request, account, result) => {
                if self.direct_request != Some(request) {
                    return Task::none();
                }
                self.direct_request = None;
                self.pending_ops -= 1;
                if self.closing
                    || self.stopped
                    || self.account_id != account
                    || (self.demo.is_none() && self.model.authentication.is_some())
                {
                    return Task::none();
                }
                let visible = self
                    .new_chat
                    .as_ref()
                    .is_some_and(|s| s.request == request && s.account == account);
                match result {
                    Ok(room) => {
                        // Older queued snapshots may predate the room. Retain it until
                        // navigation publishes it; completion after dismissal never navigates.
                        if !self.model.rooms.iter().any(|r| r.id == room.id) {
                            std::sync::Arc::make_mut(&mut self.model.rooms).push(room.clone());
                            if self.demo.is_none() {
                                self.pending_direct_room = Some(room.clone());
                            }
                        }
                        if visible {
                            self.new_chat = None;
                            self.pending_navigation = None;
                            let task = if self
                                .selection
                                .room
                                .as_ref()
                                .is_some_and(|(_, id)| id == &room.id)
                            {
                                Task::none()
                            } else {
                                self.update(Message::Room(room.id))
                            };
                            return task.chain(iced::widget::operation::focus("room-composer"));
                        }
                    }
                    Err(error) if visible => self.new_chat.as_mut().unwrap().error = Some(error),
                    Err(_) => {}
                }
            }
            Event::Ignore => {}
        }
        Task::none()
    }
    pub(super) fn new_chat_avatars(&self) -> Vec<matrix_sdk::ruma::OwnedMxcUri> {
        self.new_chat
            .as_ref()
            .into_iter()
            .flat_map(|s| s.picker.results.iter())
            .filter_map(|p| p.avatar_url.clone())
            .collect()
    }
    pub(super) fn new_chat_view(&self) -> Element<'_, Message> {
        let state = self.new_chat.as_ref().unwrap();
        let request = state.request;
        let busy = self.direct_request.is_some();
        let mut content = column![
            state
                .picker
                .quick_view(&self.model.avatars, !busy)
                .map(move |event| message(Event::Picker(request, event))),
        ]
        .spacing(10);
        if let Some(error) = &state.error {
            content = content.push(text(error).size(12).color(shell::MUTED));
        }
        content = content.push(
            text(if busy {
                "Opening chat…"
            } else {
                "New direct chat · Type a name · ↑ ↓ select · Enter open · Esc close"
            })
            .size(11)
            .color(shell::MUTED),
        );
        let card = container(content)
            .padding(12)
            .width(560)
            .max_height(390)
            .style(|_| container::Style {
                border: iced::Border::default().rounded(12),
                ..shell::surface(iced::Color::WHITE)
            });
        opaque(
            mouse_area(
                container(opaque(mouse_area(card).on_press(message(Event::Ignore))))
                    .center_x(Length::Fill)
                    .height(Length::Fill)
                    .padding([48, 16])
                    .style(|_| shell::surface(iced::Color::BLACK.scale_alpha(0.25))),
            )
            .on_press(message(Event::Close))
            .on_scroll(|_| message(Event::Ignore)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{people_picker::Event as People, tests::demo_app};
    use super::*;
    use interconnect::people::{Person, SearchResults};
    fn open(app: &mut App) -> u64 {
        let _ = app.update(message(Event::Open));
        app.new_chat.as_ref().unwrap().request
    }
    fn populate(app: &mut App) {
        let state = app.new_chat.as_mut().unwrap();
        state.picker.query = "ja".into();
        state.picker.results = ["Jordan", "Jamie"]
            .into_iter()
            .map(|name| Person {
                name: name.into(),
                user_id: format!("@{}:example.org", name.to_lowercase()),
                avatar_url: None,
            })
            .collect();
    }
    #[test]
    fn keyboard_selection_creates_once_and_keeps_drafts_and_filters() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("draft");
        app.scroll_positions.insert(key.clone(), 123.0);
        let scope = app.shell.space.clone();
        let request = open(&mut app);
        populate(&mut app);
        let _ = app.update(message(Event::Move(false)));
        assert_eq!(app.new_chat.as_ref().unwrap().picker.highlighted, 1);
        let _ = app.update(message(Event::Picker(request, People::Submit)));
        assert_eq!(
            app.selection.room.as_ref().unwrap().1,
            "demo-dm:@jamie:example.org"
        );
        assert_eq!(
            app.model
                .rooms
                .iter()
                .filter(|r| r.id == "demo-dm:@jamie:example.org")
                .count(),
            1
        );
        assert_eq!(app.pending_ops, 0);
        assert!(app.model.room.as_ref().unwrap().rows.is_empty());
        assert_eq!(app.shell.space, scope);
        assert_eq!(app.drafts[&key].content.text(), "draft");
        assert_eq!(app.scroll_positions[&key], 123.0);
        let request = open(&mut app);
        populate(&mut app);
        let generation = app.selection.room.clone();
        let _ = app.update(message(Event::Picker(
            request,
            People::Select("@jamie:example.org".into()),
        )));
        assert_eq!(app.selection.room, generation);
        assert_eq!(
            app.model
                .rooms
                .iter()
                .filter(|r| r.id == "demo-dm:@jamie:example.org")
                .count(),
            1
        );
    }
    #[test]
    fn cancel_preserves_thread_and_rejects_old_picker_events() {
        let mut app = demo_app();
        let _ = app.update(Message::OpenLastThread);
        let selection = app.selection.clone();
        let old = open(&mut app);
        let _ = app.update(Message::AdjacentRoom(false));
        let _ = app.update(Message::Escape);
        assert_eq!(app.selection, selection);
        let request = open(&mut app);
        assert_ne!(request, old);
        let _ = app.update(message(Event::Picker(
            old,
            People::Loaded(1, Ok(SearchResults::default())),
        )));
        let _ = app.update(message(Event::Picker(
            old,
            People::Select("@jamie:example.org".into()),
        )));
        assert_eq!(app.selection.room, selection.room);
        assert!(app.new_chat.as_ref().unwrap().picker.query.is_empty());
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn raw_ids_work_without_results_and_invalid_or_self_ids_do_not_create() {
        let mut app = demo_app();
        app.selection.room = None;
        app.model.rooms = Vec::new().into();
        app.account_id = Some("@me:example.org".into());
        let request = open(&mut app);
        for input in ["nobody", "@me:example.org"] {
            let _ = app.update(message(Event::Picker(request, People::Edit(input.into()))));
            let _ = app.update(message(Event::Submit));
            assert!(app.new_chat.as_ref().unwrap().error.is_some());
            assert!(app.model.rooms.is_empty());
        }
        let _ = app.update(message(Event::Picker(
            request,
            People::Edit("@remote:other.org".into()),
        )));
        let _ = app.update(message(Event::Submit));
        assert_eq!(
            app.selection.room.as_ref().unwrap().1,
            "demo-dm:@remote:other.org"
        );
    }
    #[test]
    fn late_creation_cannot_navigate_a_new_picker_or_account_and_shutdown_waits() {
        for mode in [0, 1, 2] {
            let mut app = demo_app();
            let request = open(&mut app);
            app.direct_request = Some(request);
            app.pending_ops = 1;
            let selected = app.selection.room.clone();
            let _ = app.update(message(Event::Close));
            if mode == 0 {
                open(&mut app);
            }
            if mode == 1 {
                app.account_id = Some("@other:example.org".into());
            }
            if mode == 2 {
                app.demo = None;
                let _ = app.update(Message::Close);
                assert!(!app.selection.shutdown);
            }
            let _ = app.update(message(Event::Completed(
                request,
                None,
                Ok(RoomEntry {
                    id: "late".into(),
                    direct: true,
                    ..Default::default()
                }),
            )));
            assert_eq!(app.pending_ops, 0);
            assert_eq!(app.selection.room, selected);
            if mode == 2 {
                assert!(app.selection.shutdown);
            }
            let _ = app.update(message(Event::Completed(
                request,
                None,
                Err("duplicate".into()),
            )));
            assert_eq!(app.pending_ops, 0);
        }
    }
    #[test]
    fn creation_survives_older_navigation_snapshot_and_errors_allow_retry() {
        let mut app = demo_app();
        app.demo = None;
        app.model.authentication = None;
        app.account_id = Some("@me:example.org".into());
        app.model.account_id = app.account_id.clone();
        let request = open(&mut app);
        app.direct_request = Some(request);
        app.pending_ops = 1;
        let _ = app.update(message(Event::Completed(
            request,
            app.account_id.clone(),
            Err("Server refused".into()),
        )));
        assert!(app.new_chat.as_ref().unwrap().error.is_some());
        assert_eq!(app.pending_ops, 0);
        app.direct_request = Some(request);
        app.pending_ops = 1;
        let stale = app.model.clone();
        let _ = app.update(message(Event::Completed(
            request,
            app.account_id.clone(),
            Ok(RoomEntry {
                id: "!new:example.org".into(),
                name: "Peer".into(),
                direct: true,
                ..Default::default()
            }),
        )));
        assert!(app.pending_direct_room.is_some());
        let selected = app.selection.room.clone();
        let _ = app.update(Message::State(Box::new(stale)));
        assert_eq!(app.selection.room, selected);
        assert!(
            app.model
                .rooms
                .iter()
                .any(|room| room.id == "!new:example.org")
        );
        let confirmed = app.model.clone();
        let _ = app.update(Message::State(Box::new(confirmed)));
        assert!(app.pending_direct_room.is_none());
    }

    #[test]
    fn picker_fits_minimum_window_and_enter_submits_once_without_idle_timer() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        let request = open(&mut app);
        populate(&mut app);
        let mut view = app.new_chat_view();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(640.0, 480.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        assert_eq!(node.size(), size);
        view.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut widget::operation::focusable::focus(people_picker::SEARCH_ID.into()),
        );
        let event = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: Key::Named(Named::Enter),
            modified_key: Key::Named(Named::Enter),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        });
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &event,
            layout::Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        assert!(shell.is_event_captured());
        assert!(
            matches!(messages.as_slice(), [Message::NewChat(Event::Picker(r, People::Submit))] if *r == request)
        );
        assert!(
            shortcut(
                event,
                iced::event::Status::Captured,
                iced::window::Id::unique()
            )
            .is_none()
        );
        for millis in [0, 750, 1250] {
            messages.clear();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now() + std::time::Duration::from_millis(millis),
                )),
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            if millis > 0 {
                assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            }
        }
        messages.clear();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            layout::Layout::new(&node),
            mouse::Cursor::Available(iced::Point::new(5.0, 10.0)),
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        assert!(shell.is_event_captured());
        assert!(matches!(
            messages.as_slice(),
            [Message::NewChat(Event::Close)]
        ));
    }

    #[test]
    fn shortcuts_and_other_pickers_do_not_overlap() {
        let mut app = demo_app();
        for modifiers in [Modifiers::CTRL, Modifiers::COMMAND] {
            assert!(matches!(
                super::super::shell_shortcut(&Key::Character("n".into()), modifiers),
                Some(Message::NewChat(Event::Open))
            ));
        }
        assert!(!is_shortcut(
            &Key::Character("n".into()),
            Modifiers::empty()
        ));
        open(&mut app);
        let _ = app.update(Message::QuickRoom(super::super::room_picker::Event::Open));
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert!(app.room_picker.is_none());
        assert!(app.settings.draft.is_none());
        let _ = app.update(message(Event::Close));
        let _ = app.update(Message::QuickRoom(super::super::room_picker::Event::Open));
        let _ = app.update(message(Event::Open));
        assert!(app.new_chat.is_none());
    }
}
