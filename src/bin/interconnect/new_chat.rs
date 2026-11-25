//! Quick-open people and room-address picker; room joins use the membership dialog.
use super::{App, MAX_PENDING_OPERATIONS, Message, people_picker, reaction_picker, shell};
use iced::{
    Element, Length, Task,
    keyboard::{Key, Modifiers, key::Named},
    widget::{button, column, container, mouse_area, opaque, text},
};
use interconnect::{
    connected::MessageRow,
    navigation::RoomEntry,
    people::{Person, parse_user_id},
};

#[derive(Clone, Debug)]
pub struct Target {
    account: Option<String>,
    room: (u64, String),
    source: Source,
}
#[derive(Clone, Debug)]
enum Source {
    Message(reaction_picker::Target),
    Member(u64, String),
    Pinned(String),
}

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
    OpenUser(Target),
    Mention(matrix_sdk::ruma::OwnedUserId),
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
fn is_room_query(query: &str) -> bool {
    let query = query.trim();
    query.starts_with(['#', '!'])
        || query.starts_with("matrix:")
        || url::Url::parse(query)
            .ok()
            .is_some_and(|url| url.host_str() == Some("matrix.to"))
}
pub fn is_shortcut(key: &Key, modifiers: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character("n" | "N"))
        && modifiers == super::shortcuts::primary(false)
}
pub fn shortcut(
    event: iced::Event,
    status: iced::event::Status,
    _: iced::window::Id,
) -> Option<Message> {
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event
    else {
        return None;
    };
    match key.as_ref() {
        // The search input captures Escape just to unfocus; the picker owns dismissal.
        Key::Named(Named::Escape) if modifiers.is_empty() => Some(message(Event::Close)),
        Key::Named(Named::ArrowUp)
            if modifiers.is_empty() && status != iced::event::Status::Captured =>
        {
            Some(message(Event::Move(true)))
        }
        Key::Named(Named::ArrowDown)
            if modifiers.is_empty() && status != iced::event::Status::Captured =>
        {
            Some(message(Event::Move(false)))
        }
        Key::Named(Named::Tab)
            if super::shortcuts::traversal(modifiers)
                && status != iced::event::Status::Captured =>
        {
            Some(message(Event::Open))
        }
        _ if is_shortcut(&key, modifiers) => Some(message(Event::Open)),
        _ => None,
    }
}
impl App {
    fn direct_target(&self, source: Source, user: &str, is_own: bool) -> Option<Message> {
        if is_own
            || self.account_id.as_deref() == Some(user)
            || parse_user_id(user).is_err()
            || self.closing
            || self.stopped
            || self.composing
            || self.direct_request.is_some()
            || self.create_room_request.is_some()
            || self.pending_ops >= MAX_PENDING_OPERATIONS
        {
            return None;
        }
        Some(message(Event::OpenUser(Target {
            account: self.account_id.clone(),
            room: self.selection.room.clone()?,
            source,
        })))
    }

    pub(super) fn sender_chat_action(
        &self,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Message> {
        self.direct_target(
            Source::Message(reaction_picker::Target {
                room: self.selection.room.as_ref()?.1.clone(),
                thread,
                generation,
                event: row.event_id.clone()?,
            }),
            &row.sender,
            row.is_own,
        )
    }

    pub(super) fn pinned_chat_action(&self, row: &MessageRow) -> Option<Message> {
        self.direct_target(Source::Pinned(row.key.clone()), &row.sender, row.is_own)
    }

    pub(super) fn member_chat_action(
        &self,
        member: &interconnect::connected::Member,
        generation: u64,
    ) -> Option<Message> {
        self.direct_target(
            Source::Member(generation, member.user_id.clone()),
            &member.user_id,
            self.demo.is_some() && member.user_id == "@you:example.org",
        )
    }

    fn direct_person(&self, target: &Target) -> Option<Person> {
        if self.account_id != target.account || self.selection.room.as_ref() != Some(&target.room) {
            return None;
        }
        let row = match &target.source {
            Source::Message(target) => self.message_row(target)?,
            Source::Pinned(key) => self
                .model
                .pinned
                .as_ref()
                .filter(|pins| pins.generation == target.room.0)?
                .rows
                .iter()
                .find(|row| &row.key == key)?,
            Source::Member(generation, user) => {
                let member = self
                    .model
                    .members
                    .as_ref()
                    .filter(|members| {
                        members.generation == *generation
                            && self.selection.members == Some(*generation)
                    })?
                    .rows
                    .iter()
                    .find(|member| &member.user_id == user)?;
                self.member_chat_action(member, *generation)?;
                return Some(Person {
                    user_id: member.user_id.clone(),
                    name: member.name.clone(),
                    avatar_url: member.avatar_url.clone(),
                });
            }
        };
        self.direct_target(target.source.clone(), &row.sender, row.is_own)?;
        Some(Person {
            user_id: row.sender.clone(),
            name: row.display_name.clone(),
            avatar_url: row.avatar_url.clone(),
        })
    }

    pub(super) fn new_chat_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Mention(user) => {
                if self.new_chat.is_some()
                    || self.message_delete.is_some()
                    || self.composing
                    || self.direct_request.is_some()
                    || self.create_room_request.is_some()
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self.account_id.as_deref() == Some(user.as_str())
                    || (self.demo.is_some() && user.as_str() == "@you:example.org")
                {
                    return Task::none();
                }
                // Use the normal dialog guards before either navigating or offering
                // creation. Merely clicking a mention never writes to the server.
                let focus = self.new_chat_event(Event::Open);
                let Some(state) = &mut self.new_chat else {
                    return Task::none();
                };
                if let Some(room) = self
                    .model
                    .rooms
                    .iter()
                    .find(|room| {
                        room.direct
                            && room.direct_user.as_deref() == Some(user.as_str())
                            && (1..=2).contains(&room.member_count)
                    })
                    .map(|room| room.id.clone())
                {
                    self.new_chat = None;
                    self.pending_navigation = None;
                    return self
                        .update(Message::Room(room))
                        .chain(iced::widget::operation::focus("room-composer"));
                }
                state.picker.query = user.to_string();
                state.picker.selected = Some(Person {
                    user_id: user.to_string(),
                    name: user.to_string(),
                    avatar_url: None,
                });
                return focus;
            }
            Event::OpenUser(target) => {
                if self.new_chat.is_some() || self.message_delete.is_some() {
                    return Task::none();
                }
                let Some(person) = self.direct_person(&target) else {
                    return Task::none();
                };
                let _ = self.new_chat_event(Event::Open);
                let Some(state) = &mut self.new_chat else {
                    return Task::none();
                };
                state.picker.query = person.user_id.clone();
                state.picker.selected = Some(person);
                return self.new_chat_event(Event::Submit);
            }
            Event::Open => {
                if self.create_room.is_some()
                    || self.closing
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
                    self.user_completion.clear();
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
                if self.direct_request.is_some()
                    && let Some(cancel) = &self.creation_cancel
                {
                    cancel.cancel();
                }
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
                if let people_picker::Event::Edit(query) = &event
                    && is_room_query(query)
                {
                    state.picker.clear();
                    state.picker.query = query.clone();
                    state.error = None;
                    return Task::none();
                }
                if is_room_query(&state.picker.query)
                    && !matches!(event, people_picker::Event::Edit(_))
                {
                    return Task::none();
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
                    || self.create_room_request.is_some()
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                {
                    return Task::none();
                }
                let Some(state) = &mut self.new_chat else {
                    return Task::none();
                };
                if state.account != self.account_id {
                    return Task::none();
                }
                if is_room_query(&state.picker.query) {
                    let address =
                        match interconnect::room_membership::Address::parse(&state.picker.query) {
                            Ok(address) => address,
                            Err(error) => {
                                state.error = Some(error);
                                return Task::none();
                            }
                        };
                    let state = self.new_chat.take().unwrap();
                    if !self.context_menu_allowed() {
                        self.new_chat = Some(state);
                        return Task::none();
                    }
                    return self.membership_event(super::membership::Event::Link(address));
                }
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
                            direct_user: Some(user.clone()),
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
                let cancel = tokio_util::sync::CancellationToken::new();
                self.creation_cancel = Some(cancel.clone());
                return Task::perform(
                    async move {
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => Err("Chat creation cancelled. Check your room list before trying again.".into()),
                            result = tokio::time::timeout(std::time::Duration::from_secs(45), directory.open_direct(user)) => result.unwrap_or_else(|_| Err("Chat creation timed out. Check your room list before trying again.".into())),
                        }
                    },
                    move |result| message(Event::Completed(request, account.clone(), result)),
                );
            }
            Event::Completed(request, account, result) => {
                if self.direct_request != Some(request) {
                    return Task::none();
                }
                self.direct_request = None;
                self.creation_cancel = None;
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
                                self.pending_created_room = Some(room.clone());
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
        iced::widget::responsive(move |size| self.new_chat_sized(size)).into()
    }
    fn new_chat_sized(&self, size: iced::Size) -> Element<'_, Message> {
        let state = self.new_chat.as_ref().unwrap();
        let request = state.request;
        let busy = self.direct_request.is_some() || self.create_room_request.is_some();
        let room_address = is_room_query(&state.picker.query);
        let mut content = column![
            shell::title("New conversation", 18),
            state
                .picker
                .quick_view(&self.model.avatars, !busy, room_address)
                .map(move |event| message(Event::Picker(request, event))),
        ]
        .spacing(10);
        if busy {
            content = content.push(super::tooltips::icon_button(
                super::icons::Kind::Close,
                "Cancel opening chat",
                Some(message(Event::Close)),
            ));
        }
        if room_address || state.picker.selected.is_some() {
            let mut start = button(
                text(if room_address {
                    "Open or join room…"
                } else {
                    "Open chat"
                })
                .size(13),
            )
            .style(shell::quiet_button);
            if !busy {
                start = start.on_press(message(Event::Submit));
            }
            content = content.push(start);
        }
        if let Some(error) = &state.error {
            content = content.push(text(error).size(12).style(shell::muted_text));
        }
        content = content.push(
            text(if busy {
                "Opening chat… · Esc cancels · A room may already have been created"
            } else {
                "Name or room address · ↑ ↓ select · Enter open · Esc close"
            })
            .size(11)
            .style(shell::muted_text),
        );
        let card = container(content)
            .padding(12)
            .width((size.width - 32.0).clamp(1.0, 560.0))
            .height(
                (154.0
                    + state.picker.results.len().min(5) as f32 * 54.0
                    + if room_address || state.picker.selected.is_some() {
                        36.0
                    } else {
                        0.0
                    })
                .min((size.height - 96.0).clamp(1.0, 440.0)),
            )
            .style(|theme| container::Style {
                border: iced::Border::default().rounded(12),
                ..shell::surface(theme, shell::palette(theme).background)
            });
        opaque(
            mouse_area(
                container(opaque(mouse_area(card).on_press(message(Event::Ignore))))
                    .center_x(Length::Fill)
                    .height(Length::Fill)
                    .padding([48, 16])
                    .style(|theme| shell::surface(theme, shell::palette(theme).scrim)),
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

    fn sender_action(app: &App, thread: bool) -> (Message, String) {
        let generation = if thread {
            app.selection.thread.as_ref().unwrap().0
        } else {
            app.selection.room.as_ref().unwrap().0
        };
        let pane = if thread {
            &app.model.thread
        } else {
            &app.model.room
        };
        pane.as_ref()
            .unwrap()
            .rows
            .iter()
            .find_map(|row| {
                app.sender_chat_action(row, thread, generation)
                    .map(|action| (action, row.sender.clone()))
            })
            .unwrap()
    }

    fn mention(user: &str) -> Message {
        message(Event::Mention(user.parse().unwrap()))
    }

    #[test]
    fn dismissal_and_quit_cancel_creation_and_release_pending_operation() {
        for quit in [false, true] {
            let mut app = demo_app();
            let request = open(&mut app);
            app.demo = None;
            app.direct_request = Some(request);
            app.pending_ops = 1;
            let cancel = tokio_util::sync::CancellationToken::new();
            app.creation_cancel = Some(cancel.clone());
            let selection = app.selection.room.clone();
            let _ = app.update(if quit {
                Message::Close
            } else {
                message(Event::Close)
            });
            assert!(cancel.is_cancelled());
            let _ = app.update(message(Event::Completed(
                request,
                None,
                Err("Cancelled".into()),
            )));
            assert_eq!(app.pending_ops, 0);
            assert!(app.direct_request.is_none());
            assert!(app.creation_cancel.is_none());
            assert_eq!(app.selection.room, selection);
            if quit {
                assert!(app.selection.shutdown);
            }
        }
    }

    #[test]
    fn mention_opens_existing_dm_and_preserves_room_and_thread_drafts() {
        for thread in [false, true] {
            let mut app = demo_app();
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let key = app.draft_key(thread).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("unfinished message");
            app.scroll_positions.insert(key.clone(), 87.0);
            let scope = app.shell.space.clone();
            let count = app.model.rooms.len();
            let _ = app.update(mention("@maya:example.org"));
            assert_eq!(app.selection.room.as_ref().unwrap().1, "maya");
            assert!(app.new_chat.is_none());
            assert!(app.direct_request.is_none());
            assert_eq!(app.model.rooms.len(), count);
            assert_eq!(app.pending_ops, 0);
            assert_eq!(app.shell.space, scope);
            assert_eq!(app.drafts[&key].content.text(), "unfinished message");
            assert_eq!(app.scroll_positions[&key], 87.0);
        }
    }

    #[test]
    fn mention_without_known_dm_offers_creation_and_can_be_dismissed() {
        let mut app = demo_app();
        let selection = app.selection.clone();
        let count = app.model.rooms.len();
        let _ = app.update(mention("@jules:example.org"));
        let state = app.new_chat.as_ref().unwrap();
        assert_eq!(state.picker.query, "@jules:example.org");
        assert_eq!(
            state.picker.selected.as_ref().unwrap().user_id,
            "@jules:example.org"
        );
        assert_eq!(app.selection, selection);
        assert_eq!(app.model.rooms.len(), count);
        assert_eq!(app.pending_ops, 0);
        assert!(app.direct_request.is_none());
        let _ = app.update(Message::Escape);
        assert!(app.new_chat.is_none());
        assert_eq!(app.selection, selection);
        let _ = app.update(mention("@jules:example.org"));
        let _ = app.update(message(Event::Submit));
        assert_eq!(
            app.selection.room.as_ref().unwrap().1,
            "demo-dm:@jules:example.org"
        );
        assert!(app.new_chat.is_none());
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::Room("studio".into()));
        let _ = app.update(mention("@jules:example.org"));
        assert!(app.new_chat.is_none());
        assert_eq!(app.model.rooms.len(), count + 1);
    }

    #[test]
    fn mention_does_not_treat_a_group_as_a_one_to_one_dm() {
        let mut app = demo_app();
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "maya")
            .unwrap()
            .member_count = 5;
        let _ = app.update(mention("@maya:example.org"));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
        assert_eq!(
            app.new_chat.as_ref().unwrap().picker.query,
            "@maya:example.org"
        );
        assert!(app.direct_request.is_none());
    }

    #[test]
    fn mention_actions_respect_self_busy_lifecycle_and_modal_guards() {
        for mode in 0..10 {
            let mut app = demo_app();
            match mode {
                0 => app.account_id = Some("@maya:example.org".into()),
                1 => app.direct_request = Some(999),
                2 => app.create_room_request = Some(999),
                3 => app.composing = true,
                4 => app.closing = true,
                5 => app.stopped = true,
                6 => {
                    let _ = app.update(Message::QuickRoom(super::super::room_picker::Event::Open));
                }
                7 => {
                    let _ = app.update(Message::Settings(super::super::settings::Event::Open));
                }
                8 => {
                    let _ = app.update(Message::Membership(super::super::membership::Event::Join));
                }
                _ => {
                    let _ = app.update(message(Event::Open));
                }
            }
            let selection = app.selection.clone();
            let count = app.model.rooms.len();
            let _ = app.update(mention("@maya:example.org"));
            assert_eq!(app.selection.room, selection.room, "mode {mode}");
            assert_eq!(app.selection.thread, selection.thread, "mode {mode}");
            assert_eq!(app.model.rooms.len(), count);
            if mode != 9 {
                assert!(app.new_chat.is_none(), "mode {mode}");
            }
        }
    }

    #[test]
    fn sender_clicks_open_the_exact_identity_and_preserve_room_and_thread_drafts() {
        for thread in [false, true] {
            let mut app = demo_app();
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let key = app.draft_key(thread).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("unfinished reply");
            app.scroll_positions.insert(key.clone(), 87.0);
            let scope = app.shell.space.clone();
            let room = app.selection.room.as_ref().unwrap().1.clone();
            let (action, user) = sender_action(&app, thread);
            let _ = app.update(action);
            let dm = format!("demo-dm:{user}");
            assert_eq!(app.selection.room.as_ref().unwrap().1, dm);
            assert!(app.new_chat.is_none());
            assert_eq!(app.shell.space, scope);
            assert_eq!(app.drafts[&key].content.text(), "unfinished reply");
            assert_eq!(app.scroll_positions[&key], 87.0);
            let _ = app.update(Message::Room(room));
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let (action, _) = sender_action(&app, thread);
            let _ = app.update(action);
            assert_eq!(
                app.model.rooms.iter().filter(|room| room.id == dm).count(),
                1
            );
            assert_eq!(app.pending_ops, 0);
        }
    }

    #[test]
    fn sender_actions_reject_stale_views_accounts_self_ids_and_busy_operations() {
        for mode in 0..7 {
            let mut app = demo_app();
            let (action, _) = sender_action(&app, false);
            match mode {
                0 => {
                    let _ = app.update(Message::Room("general".into()));
                }
                1 => app.account_id = Some("@other:example.org".into()),
                2 => app.direct_request = Some(999),
                3 => app.create_room_request = Some(999),
                4 => app.composing = true,
                5 => app.closing = true,
                _ => app.stopped = true,
            }
            let selection = app.selection.clone();
            let rooms = app.model.rooms.clone();
            let _ = app.update(action);
            assert_eq!(app.selection.room, selection.room);
            assert_eq!(app.selection.thread, selection.thread);
            assert_eq!(app.model.rooms.len(), rooms.len());
            assert!(app.new_chat.is_none());
        }
        let mut app = demo_app();
        let (action, _) = sender_action(&app, false);
        let Message::NewChat(Event::OpenUser(target)) = action else {
            unreachable!()
        };
        let mut row = app.model.room.as_ref().unwrap().rows[0].clone();
        row.is_own = true;
        assert!(app.sender_chat_action(&row, false, target.room.0).is_none());
        row.is_own = false;
        row.sender = "invalid user".into();
        assert!(app.sender_chat_action(&row, false, target.room.0).is_none());
        row.sender = "@me:example.org".into();
        app.account_id = Some(row.sender.clone());
        assert!(app.sender_chat_action(&row, false, target.room.0).is_none());

        let _ = app.update(Message::Thread("review".into()));
        let (action, _) = sender_action(&app, true);
        let _ = app.update(Message::CloseThread);
        let selected = app.selection.room.clone();
        let _ = app.update(action);
        assert_eq!(app.selection.room, selected);
        assert!(app.new_chat.is_none());
    }

    #[test]
    fn members_and_pins_open_chats_and_reject_obsolete_snapshots() {
        for pinned in [false, true] {
            for stale in [false, true] {
                let mut app = demo_app();
                let (action, user) = if pinned {
                    let row = &app.model.pinned.as_ref().unwrap().rows[0];
                    (app.pinned_chat_action(row).unwrap(), row.sender.clone())
                } else {
                    let _ = app.update(Message::Members(super::super::members::Event::Open));
                    let members = app.model.members.as_ref().unwrap();
                    let member = &members.rows[0];
                    (
                        app.member_chat_action(member, members.generation).unwrap(),
                        member.user_id.clone(),
                    )
                };
                if stale {
                    if pinned {
                        app.model.pinned = None;
                    } else {
                        let _ = app.update(Message::Members(super::super::members::Event::Retry));
                    }
                }
                let selected = app.selection.room.clone();
                let _ = app.update(action);
                if stale {
                    assert_eq!(app.selection.room, selected);
                } else {
                    assert_eq!(
                        app.selection.room.as_ref().unwrap().1,
                        format!("demo-dm:{user}")
                    );
                }
            }
        }
    }

    #[test]
    fn rendered_sender_names_dispatch_one_click_in_rooms_threads_people_and_pins() {
        use iced::advanced::{Layout, Shell, layout, mouse, widget};
        struct FindName<'a> {
            name: &'a str,
            bounds: Option<iced::Rectangle>,
        }
        impl widget::Operation for FindName<'_> {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, text: &str) {
                if text == self.name && self.bounds.is_none() {
                    self.bounds = Some(bounds);
                }
            }
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for source in 0..5 {
            let mut app = demo_app();
            let generation = app.selection.room.as_ref().unwrap().0;
            if source == 1 || source == 4 {
                let _ = app.update(Message::Thread("review".into()));
            } else if source == 2 {
                let _ = app.update(Message::Members(super::super::members::Event::Open));
            } else if source == 3 {
                let _ = app.update(Message::TogglePins(generation));
            }
            let user = if source == 1 {
                "@maya:example.org"
            } else {
                "@alex:example.org"
            };
            let name = if source == 1 {
                "Maya Chen"
            } else {
                "Alex Rivera"
            };
            let messages = {
                let mut view = match source {
                    0 => app.pane(false),
                    1 => app.pane(true),
                    2 => app.members_view(),
                    3 => app.pinned_view(generation).unwrap(),
                    _ => app.thread_original_view(),
                };
                let size = iced::Size::new(800.0, 1600.0);
                let mut tree = widget::Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, size),
                );
                let mut probe = FindName { name, bounds: None };
                view.as_widget_mut()
                    .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
                let point = probe.bounds.expect("sender is visible").center();
                let mut messages = Vec::new();
                for event in [
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                ] {
                    view.as_widget_mut().update(
                        &mut tree,
                        &event,
                        Layout::new(&node),
                        mouse::Cursor::Available(point),
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut Shell::new(&mut messages),
                        &iced::Rectangle::with_size(size),
                    );
                }
                messages
            };
            let mut actions = messages
                .into_iter()
                .filter(|message| matches!(message, Message::NewChat(Event::OpenUser(_))));
            let action = actions.next().expect("name emits a DM action");
            assert!(actions.next().is_none());
            let _ = app.update(action);
            assert_eq!(
                app.selection.room.as_ref().unwrap().1,
                format!("demo-dm:{user}")
            );
        }
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
    fn room_addresses_prefill_join_without_writes_and_preserve_drafts_on_cancel() {
        for input in [
            "  #public:example.org  ",
            "!public:example.org",
            "https://matrix.to/#/!public:example.org?via=route.example.org",
            "matrix:r/public:example.org",
        ] {
            let mut app = demo_app();
            let _ = app.update(Message::Thread("review".into()));
            let selection = app.selection.clone();
            let scope = app.shell.space.clone();
            let key = app.draft_key(true).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("unfinished reply");
            app.scroll_positions.insert(key.clone(), 87.0);
            let rooms = app.model.rooms.len();
            let request = open(&mut app);
            let _ = app.update(message(Event::Picker(request, People::Edit(input.into()))));
            let _ = app.update(message(Event::Picker(request, People::Submit)));
            assert!(app.new_chat.is_none());
            assert!(app.membership.open);
            assert_eq!(app.selection, selection);
            assert_eq!(app.model.rooms.len(), rooms);
            assert_eq!(app.pending_ops, 0);
            let _ = app.update(Message::Membership(super::super::membership::Event::Close));
            assert_eq!(app.selection, selection);
            assert_eq!(app.shell.space, scope);
            assert_eq!(app.drafts[&key].content.text(), "unfinished reply");
            assert_eq!(app.scroll_positions[&key], 87.0);

            let request = open(&mut app);
            let _ = app.update(message(Event::Picker(request, People::Edit(input.into()))));
            let _ = app.update(message(Event::Submit));
            let _ = app.update(Message::Membership(super::super::membership::Event::Submit));
            assert!(!app.membership.open);
            let room = app
                .model
                .rooms
                .iter()
                .find(|room| {
                    app.selection
                        .room
                        .as_ref()
                        .is_some_and(|(_, id)| id == &room.id)
                })
                .unwrap();
            // Demo retains the submitted address as the room name, including routing hints.
            assert_eq!(
                interconnect::room_membership::Address::parse(&room.name).unwrap(),
                interconnect::room_membership::Address::parse(input).unwrap()
            );
        }
    }

    #[test]
    fn joined_room_ids_and_aliases_open_from_new_chat_without_joining() {
        for input in ["!demo-general:example.org", "#general:example.org"] {
            let mut app = demo_app();
            let rooms = app.model.rooms.len();
            let request = open(&mut app);
            let _ = app.update(message(Event::Picker(request, People::Edit(input.into()))));
            let _ = app.update(message(Event::Submit));
            assert_eq!(app.selection.room.as_ref().unwrap().1, "general");
            assert!(app.new_chat.is_none());
            assert!(!app.membership.open);
            assert_eq!(app.model.rooms.len(), rooms);
            assert_eq!(app.pending_ops, 0);
        }
    }

    #[test]
    fn room_input_rejects_late_people_results_and_invalid_addresses_then_returns_to_people() {
        let mut app = demo_app();
        let selected = app.selection.room.clone();
        let rooms = app.model.rooms.len();
        let request = open(&mut app);
        let _ = app.update(message(Event::Picker(request, People::Edit("ja".into()))));
        populate(&mut app);
        for input in [
            "#",
            "matrix:invalid",
            "matrix:u/person:example.org",
            "https://matrix.to/#/invalid",
        ] {
            let _ = app.update(message(Event::Picker(request, People::Edit(input.into()))));
            for old in 0..10 {
                let _ = app.update(message(Event::Picker(
                    request,
                    People::Loaded(
                        old,
                        Ok(SearchResults {
                            people: vec![Person {
                                user_id: "@jamie:example.org".into(),
                                name: "Jamie".into(),
                                avatar_url: None,
                            }],
                            limited: false,
                        }),
                    ),
                )));
            }
            let _ = app.update(message(Event::Picker(
                request,
                People::Select("@jamie:example.org".into()),
            )));
            let state = app.new_chat.as_ref().unwrap();
            assert_eq!(state.picker.query, input);
            assert!(state.picker.results.is_empty());
            assert!(state.picker.selected.is_none());
            let _ = app.update(message(Event::Submit));
            assert!(app.new_chat.as_ref().unwrap().error.is_some());
            assert!(!app.membership.open);
            assert_eq!(app.selection.room, selected);
            assert_eq!(app.model.rooms.len(), rooms);
            assert_eq!(app.pending_ops, 0);
        }
        let _ = app.update(message(Event::Picker(
            request,
            People::Edit("@jamie:example.org".into()),
        )));
        assert!(app.new_chat.as_ref().unwrap().error.is_none());
        let _ = app.update(message(Event::Submit));
        assert_eq!(
            app.selection.room.as_ref().unwrap().1,
            "demo-dm:@jamie:example.org"
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
        assert!(app.pending_created_room.is_some());
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
        assert!(app.pending_created_room.is_none());
    }

    #[test]
    fn picker_enter_submits_once_and_backdrop_closes() {
        for room in [false, true] {
            check_picker_layout_and_keys(room);
        }
    }

    fn check_picker_layout_and_keys(room: bool) {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        let request = open(&mut app);
        if room {
            let _ = app.update(message(Event::Picker(
                request,
                People::Edit("#public:example.org".into()),
            )));
        } else {
            populate(&mut app);
        }
        let mut view = app.new_chat_view();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(640.0, 480.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
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
        {
            let modifiers = Modifiers::COMMAND;
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
