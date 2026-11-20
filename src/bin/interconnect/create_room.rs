//! Room creation form, account-bound writes and partial space-link recovery.
use super::{App, MAX_PENDING_OPERATIONS, Message, shell};
use iced::{
    Element, Length, Task,
    keyboard::{Key, Modifiers, key::Named},
    widget::{
        button, checkbox, column, container, mouse_area, opaque, pick_list, row, scrollable, text,
        text_input,
    },
};
use interconnect::{
    navigation::Scope,
    room_creation::{Created, History, Options, Visibility},
};

const NAME: &str = "create-room-name";
const TOPIC: &str = "create-room-topic";
#[derive(Clone, Debug, PartialEq, Eq)]
struct Space {
    id: Option<String>,
    name: String,
}
impl std::fmt::Display for Space {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}
pub struct State {
    request: u64,
    account: Option<String>,
    thread: bool,
    options: Options,
    spaces: Vec<Space>,
    error: Option<String>,
    created: Option<Created>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    OpenSpace,
    Close,
    Ignore,
    Focus(bool),
    Name(String),
    Topic(String),
    Space(Option<String>),
    Visibility(Visibility),
    Encrypted(bool),
    History(History),
    Listed(bool),
    Submit,
    OpenCreated,
    Completed(u64, Option<String>, Box<Result<Created, String>>),
}
fn message(event: Event) -> Message {
    Message::CreateRoom(event)
}
pub fn is_shortcut(key: &Key, modifiers: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character("n" | "N"))
        && (modifiers.control() || modifiers.command())
        && modifiers.shift()
        && !modifiers.alt()
}
pub fn shortcut(
    event: iced::Event,
    status: iced::event::Status,
    _: iced::window::Id,
) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event
    else {
        return None;
    };
    match key.as_ref() {
        Key::Named(Named::Escape) if status != iced::event::Status::Captured => {
            Some(message(Event::Close))
        }
        Key::Named(Named::Enter) if modifiers.command() => Some(message(Event::Submit)),
        Key::Named(Named::Tab) => Some(message(Event::Focus(modifiers.shift()))),
        _ => None,
    }
}
impl App {
    pub(super) fn can_create_space(&self) -> bool {
        !self.closing
            && !self.stopped
            && match &self.shell.space {
                Scope::Home | Scope::All => true,
                Scope::Space(id) => self
                    .model
                    .spaces
                    .iter()
                    .any(|s| &s.id == id && s.can_manage_rooms),
                Scope::Direct => false,
            }
    }
    pub(super) fn create_room_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open | Event::OpenSpace => {
                let is_space = matches!(event, Event::OpenSpace);
                if is_space && !self.can_create_space() {
                    return Task::none();
                }
                if self.closing
                    || self.stopped
                    || self.new_chat.is_some()
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
                if self.create_room.is_none() {
                    self.stop_typing();
                    self.emoji_completion.clear();
                    self.user_completion.clear();
                    self.generation += 1;
                    let mut spaces = vec![Space {
                        id: None,
                        name: "No space".into(),
                    }];
                    spaces.extend(self.model.spaces.iter().map(|s| Space {
                        id: Some(s.id.clone()),
                        name: s.name.clone(),
                    }));
                    let space = match &self.shell.space {
                        Scope::Space(id) if spaces.iter().any(|s| s.id.as_ref() == Some(id)) => {
                            Some(id.clone())
                        }
                        _ => None,
                    };
                    self.create_room = Some(State {
                        request: self.generation,
                        account: self.account_id.clone(),
                        thread: self.selection.thread.is_some(),
                        options: Options {
                            is_space,
                            encrypted: !is_space,
                            space,
                            ..Default::default()
                        },
                        spaces,
                        error: None,
                        created: None,
                    });
                }
                return iced::widget::operation::focus(NAME);
            }
            Event::Close => {
                if self.create_room_request.is_some()
                    && let Some(cancel) = &self.creation_cancel
                {
                    cancel.cancel();
                }
                if let Some(state) = self.create_room.take() {
                    return iced::widget::operation::focus(if state.thread {
                        "thread-composer"
                    } else {
                        "room-composer"
                    });
                }
            }
            Event::Submit => {
                if self.closing
                    || self.stopped
                    || self.composing
                    || self.create_room_request.is_some()
                    || self.direct_request.is_some()
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                {
                    return Task::none();
                }
                let Some(state) = &mut self.create_room else {
                    return Task::none();
                };
                let request = state.request;
                let account = state.account.clone();
                let mut validation = state.options.clone();
                if self.demo.is_some() {
                    validation.space = validation
                        .space
                        .as_ref()
                        .map(|_| "!demo:example.org".into());
                }
                if let Err(error) = validation.validate() {
                    state.error = Some(error);
                    return Task::none();
                }
                if state
                    .options
                    .space
                    .as_ref()
                    .is_some_and(|id| !self.model.spaces.iter().any(|s| &s.id == id))
                {
                    state.error = Some("The selected space is no longer available. Choose another space or No space.".into());
                    return Task::none();
                }
                if self.demo.is_some() {
                    let mut created = state.created.clone().unwrap_or_else(|| Created {
                        is_space: state.options.is_space,
                        room: interconnect::navigation::RoomEntry {
                            id: format!("demo-room:{request}"),
                            name: state.options.name.trim().into(),
                            topic: state.options.topic.trim().into(),
                            member_count: 1,
                            ..Default::default()
                        },
                        space: state.options.space.clone(),
                        link_error: None,
                    });
                    created.link_error = None;
                    self.pending_ops += 1;
                    self.create_room_request = Some(request);
                    return self.create_room_event(Event::Completed(
                        request,
                        account,
                        Box::new(Ok(created)),
                    ));
                }
                let Some(service) = self.model.people_directory.clone() else {
                    state.error = Some("The account is not ready yet.".into());
                    return Task::none();
                };
                state.error = None;
                self.pending_ops += 1;
                self.create_room_request = Some(request);
                let options = state.options.clone();
                let created = state.created.clone();
                let cancel = tokio_util::sync::CancellationToken::new();
                self.creation_cancel = Some(cancel.clone());
                return Task::perform(
                    async move {
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => Err("Room creation cancelled. Check your room list before trying again.".into()),
                            result = tokio::time::timeout(std::time::Duration::from_secs(60), async move {
                                if let Some(created) = created {
                                    Ok(service.link_created_room(created).await)
                                } else {
                                    service.create_room(options).await
                                }
                            }) => result.unwrap_or_else(|_| Err("Room creation timed out. Check your room list before trying again.".into())),
                        }
                    },
                    move |result| {
                        message(Event::Completed(request, account.clone(), Box::new(result)))
                    },
                );
            }
            Event::Completed(request, account, result) => {
                if self.create_room_request != Some(request) {
                    return Task::none();
                }
                self.create_room_request = None;
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
                    .create_room
                    .as_ref()
                    .is_some_and(|s| s.request == request && s.account == account);
                match *result {
                    Ok(created) => {
                        if created.is_space {
                            let entry = interconnect::navigation::SpaceEntry {
                                id: created.room.id.clone(),
                                name: created.room.name.clone(),
                                can_manage_rooms: true,
                                avatar_url: None,
                                children: vec![],
                            };
                            if !self.model.spaces.iter().any(|s| s.id == entry.id) {
                                std::sync::Arc::make_mut(&mut self.model.spaces)
                                    .push(entry.clone());
                                if self.demo.is_none() {
                                    self.pending_created_subspace = Some(entry);
                                }
                            }
                        }
                        if !created.is_space
                            && !self.model.rooms.iter().any(|r| r.id == created.room.id)
                        {
                            std::sync::Arc::make_mut(&mut self.model.rooms)
                                .push(created.room.clone());
                            if self.demo.is_none() {
                                self.pending_created_room = Some(created.room.clone());
                            }
                        }
                        if created.link_error.is_none()
                            && let Some(id) = &created.space
                            && let Some(space) = std::sync::Arc::make_mut(&mut self.model.spaces)
                                .iter_mut()
                                .find(|s| &s.id == id)
                        {
                            if !space.children.contains(&created.room.id) {
                                space.children.push(created.room.id.clone());
                            }
                            if self.demo.is_none() {
                                self.pending_created_space =
                                    Some((created.room.id.clone(), id.clone()));
                            }
                        }
                        if visible {
                            let incomplete = created.link_error.is_some();
                            self.create_room.as_mut().unwrap().created = Some(created);
                            if !incomplete {
                                return self.create_room_event(Event::OpenCreated);
                            }
                        }
                    }
                    Err(error) if visible => self.create_room.as_mut().unwrap().error = Some(error),
                    Err(_) => {}
                }
            }
            Event::OpenCreated => {
                if self.create_room_request.is_some() || self.closing || self.stopped {
                    return Task::none();
                }
                if let Some(created) = self.create_room.as_mut().and_then(|s| s.created.take()) {
                    self.create_room = None;
                    self.pending_navigation = None;
                    if created.is_space {
                        return Task::none();
                    }
                    let task = self.update(Message::Room(created.room.id));
                    return task.chain(iced::widget::operation::focus("room-composer"));
                }
            }
            Event::Focus(backwards) => {
                if self.create_room.is_some() {
                    return iced::advanced::widget::operate(focus_step(backwards));
                }
            }
            Event::Ignore => {}
            edit => {
                if self.create_room_request.is_some() {
                    return Task::none();
                }
                if let Some(state) = &mut self.create_room
                    && state.created.is_none()
                {
                    match edit {
                        Event::Name(value) => state.options.name = value,
                        Event::Topic(value) => state.options.topic = value,
                        Event::Space(id)
                            if !state.options.is_space
                                && state.spaces.iter().any(|s| s.id == id) =>
                        {
                            state.options.space = id
                        }
                        Event::Visibility(value) => {
                            state.options.visibility = value;
                            if value == Visibility::Private {
                                state.options.listed = false;
                            }
                        }
                        Event::Encrypted(value) if !state.options.is_space => {
                            state.options.encrypted = value
                        }
                        Event::History(value) => state.options.history = value,
                        Event::Listed(value) => state.options.listed = value,
                        _ => {}
                    }
                    if (state.options.encrypted || state.options.visibility == Visibility::Private)
                        && state.options.history == History::WorldReadable
                    {
                        state.options.history = History::Joined;
                    }
                    state.error = None;
                }
            }
        }
        Task::none()
    }
    pub(super) fn create_room_view(&self) -> Element<'_, Message> {
        iced::widget::responsive(move |size| self.create_room_card(size)).into()
    }
    fn create_room_card(&self, size: iced::Size) -> Element<'_, Message> {
        let state = self.create_room.as_ref().unwrap();
        let busy = self.create_room_request.is_some() || self.direct_request.is_some();
        let editable = !busy && state.created.is_none();
        let options = &state.options;
        let kind = if options.is_space {
            if options.space.is_some() {
                "subspace"
            } else {
                "space"
            }
        } else {
            "room"
        };
        let mut name = text_input(
            if options.is_space {
                "Space name"
            } else {
                "Room name"
            },
            &options.name,
        )
        .id(NAME)
        .cursor_blinking(false)
        .padding([7, 10])
        .size(13)
        .style(shell::search_style);
        let mut topic = text_input(
            if options.is_space {
                "What is this space for? (optional)"
            } else {
                "What is this room for? (optional)"
            },
            &options.topic,
        )
        .id(TOPIC)
        .cursor_blinking(false)
        .padding([7, 10])
        .size(13)
        .style(shell::search_style);
        if editable {
            name = name.on_input(|v| message(Event::Name(v)));
            topic = topic.on_input(|v| message(Event::Topic(v)));
        }
        let selected = state.spaces.iter().find(|s| s.id == options.space).cloned();
        let space: Element<'_, Message> = if editable && !options.is_space {
            pick_list(state.spaces.clone(), selected, |s| {
                message(Event::Space(s.id))
            })
            .text_size(13)
            .padding([7, 10])
            .keyboard_navigation(true)
            .style(shell::select_style)
            .width(Length::Fill)
            .into()
        } else {
            text(if options.is_space && options.space.is_none() {
                "None · root space".into()
            } else {
                selected.map_or_else(|| "No space".into(), |s| s.name)
            })
            .size(14)
            .into()
        };
        let visibility = select(
            &[Visibility::Private, Visibility::Public],
            options.visibility,
            editable,
            |v| message(Event::Visibility(v)),
        );
        let histories = if !options.encrypted && options.visibility == Visibility::Public {
            vec![
                History::Joined,
                History::Invited,
                History::Shared,
                History::WorldReadable,
            ]
        } else {
            vec![History::Joined, History::Invited, History::Shared]
        };
        let history = select(&histories, options.history, editable, |v| {
            message(Event::History(v))
        });
        let mut encrypted = checkbox(options.encrypted)
            .keyboard_navigation(true)
            .label("Encrypt messages")
            .text_size(13)
            .size(16)
            .spacing(9);
        let mut listed = checkbox(options.listed)
            .keyboard_navigation(true)
            .label("List in the homeserver's public room directory")
            .text_size(13)
            .size(16)
            .spacing(9);
        if editable {
            encrypted = encrypted.on_toggle(|v| message(Event::Encrypted(v)));
            if options.visibility == Visibility::Public {
                listed = listed.on_toggle(|v| message(Event::Listed(v)));
            }
        }
        let mut content = column![
            shell::title(
                match kind {
                    "space" => "Create a space",
                    "subspace" => "Create a subspace",
                    _ => "Create a room",
                },
                22
            ),
            text("Name").size(12).style(shell::muted_text),
            name,
            text("Topic").size(12).style(shell::muted_text),
            topic,
            text(if options.is_space {
                "Parent space"
            } else {
                "Space"
            })
            .size(12)
            .style(shell::muted_text),
            space,
            text(if options.is_space && options.space.is_none() {
                "This space will appear at the top level."
            } else {
                "Adding to a space requires permission to manage its rooms."
            })
            .size(11)
            .style(shell::muted_text),
            text(if options.is_space {
                "Visibility"
            } else {
                "Room type"
            })
            .size(12)
            .style(shell::muted_text),
            visibility,
            listed,
        ]
        .spacing(8);
        if !options.is_space {
            content = content
                .push(encrypted)
                .push(
                    text("Encryption cannot be disabled after creation.")
                        .size(11)
                        .style(shell::muted_text),
                )
                .push(text("Message history").size(12).style(shell::muted_text))
                .push(history);
        }
        if options.encrypted {
            content = content.push(
                text("Older encrypted messages also require the corresponding encryption keys.")
                    .size(11)
                    .style(shell::muted_text),
            );
        }
        if let Some(error) = &state.error {
            content = content.push(text(error).size(12).style(shell::accent_text));
        }
        if let Some(created) = &state.created
            && let Some(error) = &created.link_error
        {
            content = content
                .push(shell::title(
                    if options.is_space {
                        "Space created · parent link incomplete"
                    } else {
                        "Room created · space link incomplete"
                    },
                    14,
                ))
                .push(text(error).size(12).style(shell::muted_text));
        }
        if busy {
            content = content.push(text("Cancelling stops waiting; a room may already have been created. Check your room list before trying again.").size(12).style(shell::muted_text));
        }
        let close = button(text(if busy { "Cancel creation" } else { "Cancel" }).size(13))
            .padding([7, 14])
            .keyboard_navigation(true)
            .style(shell::quiet_button)
            .on_press(message(Event::Close));
        let submit_label = text(if busy {
            format!("Creating {kind}…")
        } else if state.created.is_some() {
            "Retry adding to space".into()
        } else {
            format!("Create {kind}")
        })
        .size(13);
        let submit_icon = if state.created.is_some() {
            super::icons::Kind::Refresh
        } else if options.is_space {
            super::icons::Kind::SpaceAdd
        } else {
            super::icons::Kind::RoomAdd
        };
        let mut submit = button(
            row![super::icons::icon(submit_icon), submit_label]
                .spacing(7)
                .align_y(iced::Alignment::Center),
        )
        .padding([7, 14])
        .keyboard_navigation(true)
        .style(shell::primary_button);
        if !busy && !options.name.trim().is_empty() {
            submit = submit.on_press(message(Event::Submit));
        }
        let mut actions = row![iced::widget::space().width(Length::Fill), close].spacing(8);
        if state.created.is_some() && !busy {
            actions = actions.push(
                button(
                    text(if options.is_space {
                        "Done"
                    } else {
                        "Open room"
                    })
                    .size(13),
                )
                .padding([7, 14])
                .keyboard_navigation(true)
                .style(shell::quiet_button)
                .on_press(message(Event::OpenCreated)),
            );
        }
        actions = actions.push(submit);
        let card = container(
            column![
                scrollable(content)
                    .height(Length::Fill)
                    .direction(shell::scrollbar())
                    .style(shell::scroll_style),
                actions
            ]
            .spacing(18),
        )
        .id("create-room-dialog")
        .padding(20)
        .width(540)
        .height((size.height - 32.0).clamp(1.0, 640.0))
        .style(|theme| container::Style {
            border: iced::Border::default().rounded(12),
            ..shell::surface(theme, shell::palette(theme).background)
        });
        super::focus_scope::dismissible(
            opaque(
                mouse_area(
                    container(card)
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .padding(16)
                        .style(|theme| shell::surface(theme, shell::palette(theme).scrim)),
                )
                .on_press(message(Event::Ignore))
                .on_scroll(|_| message(Event::Ignore)),
            ),
            "create-room-focus",
            Message::Escape,
        )
    }
}
fn select<'a, T: Clone + Eq + std::fmt::Display + 'a>(
    values: &[T],
    selected: T,
    enabled: bool,
    on_select: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    if enabled {
        pick_list(values.to_vec(), Some(selected), on_select)
            .text_size(13)
            .padding([7, 10])
            .keyboard_navigation(true)
            .style(shell::select_style)
            .width(Length::Fill)
            .into()
    } else {
        text(selected.to_string()).size(14).into()
    }
}

fn focus_step<T: Send + 'static>(backwards: bool) -> impl iced::advanced::widget::Operation<T> {
    super::focus_scope::cycle("create-room-dialog", backwards)
}

#[cfg(test)]
mod tests {
    use super::super::tests::demo_app;
    use super::*;
    fn open(app: &mut App) -> u64 {
        let _ = app.update(message(Event::Open));
        app.create_room.as_ref().unwrap().request
    }
    fn created(id: &str, error: Option<&str>) -> Created {
        Created {
            is_space: false,
            room: interconnect::navigation::RoomEntry {
                id: id.into(),
                name: "New room".into(),
                ..Default::default()
            },
            space: Some("studio-space".into()),
            link_error: error.map(str::to_owned),
        }
    }
    #[test]
    fn home_and_all_rooms_use_the_same_action_to_create_unlinked_root_spaces() {
        for scope in [Scope::Home, Scope::All] {
            let mut app = demo_app();
            let _ = app.update(Message::Space(scope.clone()));
            let selection = app.selection.clone();
            let key = app.draft_key(false).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("draft");
            app.scroll_positions.insert(key.clone(), 120.0);
            let parents = app
                .model
                .spaces
                .iter()
                .map(|s| (s.id.clone(), s.children.clone()))
                .collect::<Vec<_>>();
            assert!(app.can_create_space());
            let _ = app.update(message(Event::OpenSpace));
            let state = app.create_room.as_ref().unwrap();
            let id = format!("demo-room:{}", state.request);
            assert!(state.options.is_space);
            assert!(!state.options.encrypted);
            assert!(state.options.space.is_none());
            let _ = app.update(message(Event::Space(Some("studio-space".into()))));
            let _ = app.update(message(Event::Encrypted(true)));
            assert!(app.create_room.as_ref().unwrap().options.space.is_none());
            assert!(!app.create_room.as_ref().unwrap().options.encrypted);
            let _ = app.update(message(Event::Name("Root space".into())));
            let _ = app.update(message(Event::Submit));
            assert!(app.create_room.is_none());
            assert!(
                app.model
                    .spaces
                    .iter()
                    .any(|s| s.id == id && s.name == "Root space")
            );
            assert!(!app.model.rooms.iter().any(|r| r.id == id));
            for (parent, children) in parents {
                assert_eq!(
                    app.model
                        .spaces
                        .iter()
                        .find(|s| s.id == parent)
                        .unwrap()
                        .children,
                    children
                );
            }
            assert_eq!(app.selection.room, selection.room);
            assert_eq!(app.selection.thread, selection.thread);
            assert_eq!(app.shell.space, scope);
            assert_eq!(app.drafts[&key].content.text(), "draft");
            assert_eq!(app.scroll_positions[&key], 120.0);
            assert_eq!(app.pending_ops, 0);
        }
        let mut app = demo_app();
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "studio-space")
            .unwrap()
            .can_manage_rooms = false;
        assert!(!app.can_create_space());
        let _ = app.update(message(Event::OpenSpace));
        assert!(app.create_room.is_none());
    }
    #[test]
    fn root_space_creation_survives_queued_navigation_without_inventing_a_parent_link() {
        let mut app = demo_app();
        let _ = app.update(Message::Space(Scope::Home));
        app.demo = None;
        app.model.authentication = None;
        let stale = app.model.clone();
        let _ = app.update(message(Event::OpenSpace));
        let request = app.create_room.as_ref().unwrap().request;
        app.create_room_request = Some(request);
        app.pending_ops = 1;
        let selection = app.selection.room.clone();
        let _ = app.update(message(Event::Completed(
            request,
            app.account_id.clone(),
            Box::new(Ok(Created {
                is_space: true,
                room: interconnect::navigation::RoomEntry {
                    id: "!root:example.org".into(),
                    name: "Root".into(),
                    ..Default::default()
                },
                space: None,
                link_error: None,
            })),
        )));
        assert!(app.pending_created_subspace.is_some());
        assert!(app.pending_created_space.is_none());
        let _ = app.update(Message::State(Box::new(stale)));
        assert!(app.model.spaces.iter().any(|s| s.id == "!root:example.org"));
        assert!(!app.model.rooms.iter().any(|r| r.id == "!root:example.org"));
        assert!(
            app.model
                .spaces
                .iter()
                .all(|s| !s.children.contains(&"!root:example.org".into()))
        );
        assert_eq!(app.selection.room, selection);
        assert_eq!(app.shell.space, Scope::Home);
        let confirmed = app.model.clone();
        let _ = app.update(Message::State(Box::new(confirmed)));
        assert!(app.pending_created_subspace.is_none());
    }
    #[test]
    fn subspace_creation_preserves_conversation_and_links_only_in_space_tree() {
        let mut app = demo_app();
        let selection = app.selection.clone();
        let scope = app.shell.space.clone();
        let _ = app.update(message(Event::OpenSpace));
        let state = app.create_room.as_ref().unwrap();
        assert!(state.options.is_space);
        assert!(!state.options.encrypted);
        let id = format!("demo-room:{}", state.request);
        let _ = app.update(message(Event::Name("Nested".into())));
        let _ = app.update(message(Event::Submit));
        assert!(app.create_room.is_none());
        assert_eq!(app.selection, selection);
        assert_eq!(app.shell.space, scope);
        assert!(app.model.spaces.iter().any(|s| s.id == id));
        assert!(!app.model.rooms.iter().any(|r| r.id == id));
        assert!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "studio-space")
                .unwrap()
                .children
                .contains(&id)
        );
        app.shell.space = Scope::Direct;
        let _ = app.update(message(Event::OpenSpace));
        assert!(app.create_room.is_none());
    }
    #[test]
    fn dismissal_and_quit_cancel_creation_and_release_pending_operation() {
        for quit in [false, true] {
            let mut app = demo_app();
            let request = open(&mut app);
            app.demo = None;
            app.create_room_request = Some(request);
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
                Box::new(Err("Cancelled".into())),
            )));
            assert_eq!(app.pending_ops, 0);
            assert!(app.create_room_request.is_none());
            assert!(app.creation_cancel.is_none());
            assert_eq!(app.selection.room, selection);
            if quit {
                assert!(app.selection.shutdown);
            }
        }
    }

    #[test]
    fn defaults_follow_current_space_and_demo_creation_preserves_drafts_and_filters() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("draft");
        app.scroll_positions.insert(key.clone(), 150.0);
        app.shell.query = "design".into();
        let scope = app.shell.space.clone();
        let request = open(&mut app);
        let state = app.create_room.as_ref().unwrap();
        assert_eq!(state.options.space.as_deref(), Some("studio-space"));
        assert_eq!(state.options.visibility, Visibility::Private);
        assert!(state.options.encrypted);
        assert_eq!(state.options.history, History::Joined);
        let _ = app.update(message(Event::Submit));
        assert!(app.create_room.as_ref().unwrap().error.is_some());
        let _ = app.update(message(Event::Name(" Room name ".into())));
        let _ = app.update(message(Event::Topic("Topic".into())));
        let _ = app.update(message(Event::Submit));
        let room = format!("demo-room:{request}");
        assert_eq!(app.selection.room.as_ref().unwrap().1, room);
        assert!(app.model.room.as_ref().unwrap().rows.is_empty());
        assert!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "studio-space")
                .unwrap()
                .children
                .contains(&room)
        );
        assert_eq!(app.shell.space, scope);
        assert_eq!(app.shell.query, "design");
        assert_eq!(app.drafts[&key].content.text(), "draft");
        assert_eq!(app.scroll_positions[&key], 150.0);
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn no_space_defaults_and_visibility_changes_keep_options_consistent() {
        let mut app = demo_app();
        app.shell.space = Scope::Direct;
        open(&mut app);
        assert!(app.create_room.as_ref().unwrap().options.space.is_none());
        let _ = app.update(message(Event::Visibility(Visibility::Public)));
        let _ = app.update(message(Event::Listed(true)));
        let _ = app.update(message(Event::Encrypted(false)));
        let _ = app.update(message(Event::History(History::WorldReadable)));
        assert_eq!(
            app.create_room.as_ref().unwrap().options.history,
            History::WorldReadable
        );
        let _ = app.update(message(Event::Encrypted(true)));
        assert_eq!(
            app.create_room.as_ref().unwrap().options.history,
            History::Joined
        );
        let _ = app.update(message(Event::Visibility(Visibility::Private)));
        assert!(!app.create_room.as_ref().unwrap().options.listed);
    }
    #[test]
    fn cancellation_preserves_thread_and_excludes_other_dialogs() {
        let mut app = demo_app();
        let _ = app.update(Message::OpenLastThread);
        let selection = app.selection.clone();
        open(&mut app);
        let _ = app.update(Message::NewChat(super::super::new_chat::Event::Open));
        let _ = app.update(Message::QuickRoom(super::super::room_picker::Event::Open));
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        let _ = app.update(Message::AdjacentRoom(false));
        assert!(app.new_chat.is_none());
        assert!(app.room_picker.is_none());
        assert!(app.settings.draft.is_none());
        let _ = app.update(Message::Escape);
        assert!(app.create_room.is_none());
        assert_eq!(app.selection, selection);
    }
    #[test]
    fn partial_creation_keeps_room_and_retry_links_the_same_room() {
        let mut app = demo_app();
        let request = open(&mut app);
        let _ = app.update(message(Event::Name("Room name".into())));
        let selection = app.selection.room.clone();
        app.create_room_request = Some(request);
        app.pending_ops = 1;
        let _ = app.update(message(Event::Completed(
            request,
            None,
            Box::new(Ok(created("partial-room", Some("Space refused")))),
        )));
        assert_eq!(app.selection.room, selection);
        assert!(
            app.create_room
                .as_ref()
                .unwrap()
                .created
                .as_ref()
                .unwrap()
                .link_error
                .is_some()
        );
        let _ = app.update(message(Event::Name("different".into())));
        assert_eq!(app.create_room.as_ref().unwrap().options.name, "Room name");
        let _ = app.update(message(Event::Submit));
        assert!(app.create_room.is_none());
        assert_eq!(app.selection.room.as_ref().unwrap().1, "partial-room");
        assert_eq!(
            app.model
                .rooms
                .iter()
                .filter(|r| r.id == "partial-room")
                .count(),
            1
        );
        assert!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "studio-space")
                .unwrap()
                .children
                .contains(&"partial-room".into())
        );
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn late_results_do_not_navigate_new_forms_accounts_or_shutdown() {
        for mode in 0..3 {
            let mut app = demo_app();
            let request = open(&mut app);
            let selection = app.selection.room.clone();
            app.create_room_request = Some(request);
            app.pending_ops = 1;
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
                Box::new(Ok(created("late-room", None))),
            )));
            assert_eq!(app.selection.room, selection);
            assert_eq!(app.pending_ops, 0);
            if mode == 2 {
                assert!(app.selection.shutdown);
            }
            let _ = app.update(message(Event::Completed(
                request,
                None,
                Box::new(Err("Duplicate".into())),
            )));
            assert_eq!(app.pending_ops, 0);
        }
    }
    #[test]
    fn shift_shortcut_is_distinct_from_direct_chat_shortcut() {
        for base in [Modifiers::CTRL, Modifiers::COMMAND] {
            assert!(matches!(
                super::super::shell_shortcut(&Key::Character("N".into()), base | Modifiers::SHIFT),
                Some(Message::CreateRoom(Event::Open))
            ));
            assert!(matches!(
                super::super::shell_shortcut(&Key::Character("n".into()), base),
                Some(Message::NewChat(_))
            ));
        }
        assert!(!is_shortcut(&Key::Character("n".into()), Modifiers::SHIFT));
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use iced::advanced::{
        Shell, layout, mouse,
        widget::{
            self, Operation,
            operation::{self, Outcome},
        },
    };

    #[test]
    fn dialog_traps_tab_focus_captures_backdrop_and_has_no_caret_timer() {
        let mut app = super::super::tests::demo_app();
        let _ = app.update(message(Event::Open));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let size = iced::Size::new(640.0, 480.0);
        let mut view = app.view();
        let mut tree = widget::Tree::new(&view);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        assert_eq!(node.size(), size);
        view.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut operation::focusable::focus::<()>(NAME.into()),
        );
        for expected in (0..8).map(|i| Some(i % 7)) {
            let mut count = operation::map(
                operation::scope("create-room-dialog".into(), operation::focusable::count()),
                move |count| {
                    assert_eq!(count.total, 7);
                    assert_eq!(count.focused, expected);
                },
            );
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut count,
            );
            assert!(matches!(count.finish(), Outcome::Some(())));
            let mut next: Box<dyn Operation<()>> = Box::new(focus_step(false));
            loop {
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    next.as_mut(),
                );
                match next.finish() {
                    Outcome::Chain(chained) => next = chained,
                    _ => break,
                }
            }
        }
        let now = iced::time::Instant::now();
        for millis in [0, 750, 1250] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    now + std::time::Duration::from_millis(millis),
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
        let mut messages = Vec::new();
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
            [Message::CreateRoom(Event::Ignore)]
        ));
    }
    #[test]
    fn created_room_and_space_link_survive_queued_navigation_snapshots() {
        let mut app = super::super::tests::demo_app();
        app.demo = None;
        app.model.authentication = None;
        app.model.account_id = Some("@me:example.org".into());
        app.account_id = app.model.account_id.clone();
        let _ = app.update(message(Event::Open));
        let request = app.create_room.as_ref().unwrap().request;
        let stale = app.model.clone();
        app.pending_ops = 1;
        app.create_room_request = Some(request);
        let _ = app.update(message(Event::Completed(
            request,
            app.account_id.clone(),
            Box::new(Ok(Created {
                is_space: false,
                room: interconnect::navigation::RoomEntry {
                    id: "!new:example.org".into(),
                    name: "Room".into(),
                    ..Default::default()
                },
                space: Some("studio-space".into()),
                link_error: None,
            })),
        )));
        let selection = app.selection.room.clone();
        let _ = app.update(Message::State(Box::new(stale)));
        assert_eq!(app.selection.room, selection);
        assert!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "studio-space")
                .unwrap()
                .children
                .contains(&"!new:example.org".into())
        );
        assert!(app.pending_created_room.is_some());
        assert!(app.pending_created_space.is_some());
        let confirmed = app.model.clone();
        let _ = app.update(Message::State(Box::new(confirmed)));
        assert!(app.pending_created_room.is_none());
        assert!(app.pending_created_space.is_none());
    }
}
