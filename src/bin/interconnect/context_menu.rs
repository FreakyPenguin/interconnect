//! Event-driven context menus. Resolve actions again when selected so stale
//! conversation generations and changed permissions cannot dispatch old actions.
use super::{App, Message, popover, reaction_picker, shell};
use iced::{
    Element, Point, Task,
    widget::{button, column, container, text},
};
use interconnect::connected::MessageRow;
#[path = "section_management.rs"]
mod section_management;
#[path = "space_management.rs"]
pub(super) mod space_management;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Account,
    Sections,
    Section(String),
    Room(String),
    Message(reaction_picker::Target),
    Link {
        message: reaction_picker::Target,
        url: String,
    },
    Thread {
        room: String,
        generation: u64,
        root: String,
    },
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(Target, Point),
    Dismiss,
    Choose(Target, String),
    Move(Target, String),
    Back,
    EditSectionName(String),
    SaveSection,
    CancelSection,
    AccountMove(usize, bool),
}
pub struct State {
    target: Target,
    account: Option<String>,
    position: Point,
    sections: bool,
    form: Option<section_management::Form>,
    pending: Option<u64>,
    error: Option<String>,
}
enum Action {
    CreateSection,
    RenameSection,
    RemoveSection,
    Sections,
    MarkUnread,
    RemoveFromSpace,
    Copy(String),
    Dispatch(Box<Message>),
}
// Keep menu actions compact as asynchronous result messages grow.
fn dispatch(message: Message) -> Action {
    Action::Dispatch(Box::new(message))
}
fn message(event: Event) -> Message {
    Message::ContextMenu(event)
}

pub fn trigger<'a>(base: impl Into<Element<'a, Message>>, target: Target) -> Element<'a, Message> {
    popover::context_trigger(
        base,
        move |position| message(Event::Open(target.clone(), position)),
        message(Event::Dismiss),
    )
}
struct LinkAt {
    cursor: Point,
    url: Option<String>,
}
impl iced::advanced::widget::Operation for LinkAt {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
        operate(self);
    }
    fn custom(
        &mut self,
        _: Option<&iced::advanced::widget::Id>,
        bounds: iced::Rectangle,
        state: &mut dyn std::any::Any,
    ) {
        if bounds.contains(self.cursor)
            && let Some(state) = state.downcast_ref::<super::selectable::State>()
        {
            self.url = state
                .rich
                .as_ref()
                .and_then(|rich| rich.link_at(self.cursor - (bounds.position() - Point::ORIGIN)))
                .map(str::to_owned);
        }
    }
}
impl App {
    pub(super) fn context_menu_allowed(&self) -> bool {
        if self.sign_out_open || self.membership.open || self.draft_persistence.close_requested {
            return false;
        }
        !self.closing
            && !self.stopped
            && (self.demo.is_some() || self.model.authentication.is_none())
            && self.settings.draft.is_none()
            && !self.verification.open
            && self.image_view.is_none()
            && self.reaction_picker.is_none()
            && self.composer_picker.is_none()
            && self.room_picker.is_none()
            && self.new_chat.is_none()
            && self.create_room.is_none()
            && self.invite.is_none()
            && self.topic_editor.is_none()
            && !self.room_settings.is_open()
            && self.message_delete.is_none()
            && self.attachment_picker.is_none()
            && self.clipboard_paste.is_none()
    }
    pub(super) fn message_context<'a>(
        &self,
        body: Element<'a, Message>,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Element<'a, Message> {
        let Some(event) = &row.event_id else {
            return body;
        };
        let Some((_, room)) = &self.selection.room else {
            return body;
        };
        let target = reaction_picker::Target {
            room: room.clone(),
            thread,
            generation,
            event: event.clone(),
        };
        popover::context_trigger_with(
            body,
            move |position, cursor, body, tree, layout, renderer| {
                let mut hit = LinkAt { cursor, url: None };
                body.operate(tree, layout, renderer, &mut hit);
                let target = match hit.url {
                    Some(url) => Target::Link {
                        message: target.clone(),
                        url,
                    },
                    None => Target::Message(target.clone()),
                };
                message(Event::Open(target, position))
            },
            message(Event::Dismiss),
        )
    }

    fn context_actions(&self, target: &Target) -> Vec<(&'static str, Action)> {
        if !self.context_menu_allowed()
            || ((self.bookmarks.open || self.global_search.open)
                && matches!(
                    target,
                    Target::Message(_) | Target::Link { .. } | Target::Thread { .. }
                ))
        {
            return Vec::new();
        }
        match target {
            Target::Account => {
                let mut actions = vec![
                    (
                        "Settings…",
                        dispatch(Message::Settings(super::settings::Event::Open)),
                    ),
                    ("Find room…", dispatch(Message::SearchFocus)),
                    (
                        "Join room…",
                        dispatch(Message::Membership(super::membership::Event::Join)),
                    ),
                    (
                        "Invitations…",
                        dispatch(Message::Membership(super::membership::Event::Open)),
                    ),
                ];
                if self.demo.is_some() || self.model.verification.control.is_some() {
                    actions.push((
                        if self.model.verification.incoming.is_some() {
                            "Review verification request…"
                        } else if self.model.verification.trust
                            == matrix_sdk::encryption::VerificationState::Verified
                        {
                            "Device verified…"
                        } else {
                            "Verify device…"
                        },
                        dispatch(Message::Verification(super::verification::Event::Open)),
                    ));
                }
                if self.pending_ops == 0 {
                    actions.push((
                        "Sign out…",
                        dispatch(Message::SignOut(super::sign_out::Event::Open)),
                    ));
                }
                actions
            }
            Target::Sections => vec![("New section…", Action::CreateSection)],
            Target::Section(tag) => {
                if !self.model.sections.iter().any(|section| &section.id == tag) {
                    return Vec::new();
                }
                let mut actions = Vec::new();
                if tag.starts_with("element.io.section.") {
                    actions.extend([
                        ("Rename section…", Action::RenameSection),
                        ("Remove section…", Action::RemoveSection),
                    ]);
                }
                if let Some(tag) = interconnect::room_sections::reorder_tag(tag) {
                    let order = interconnect::room_sections::section_order(&self.model.sections);
                    if let Some(index) = order.iter().position(|id| id == tag) {
                        if index > 0 {
                            actions.push((
                                "Move section up",
                                dispatch(Message::Reorder(super::section_reorder::Event::Step(
                                    tag.into(),
                                    true,
                                ))),
                            ));
                        }
                        if index + 1 < order.len() {
                            actions.push((
                                "Move section down",
                                dispatch(Message::Reorder(super::section_reorder::Event::Step(
                                    tag.into(),
                                    false,
                                ))),
                            ));
                        }
                    }
                }
                actions
            }
            Target::Room(id) => {
                if !self.model.rooms.iter().any(|room| &room.id == id) {
                    return Vec::new();
                }
                let mut actions = vec![
                    ("Open room", dispatch(Message::Room(id.clone()))),
                    ("Mark as unread", Action::MarkUnread),
                    ("Move to section…", Action::Sections),
                    ("Copy room ID", Action::Copy(id.clone())),
                ];
                if let Ok(room) = matrix_sdk::ruma::RoomId::parse(id) {
                    actions.push((
                        "Copy room link",
                        Action::Copy(room.matrix_to_uri().to_string()),
                    ));
                }
                actions.push((
                    "Add room to space…",
                    dispatch(Message::QuickRoom(
                        super::room_picker::Event::AddRoomToSpace(id.clone()),
                    )),
                ));
                if self.removable_space(id).is_some() {
                    actions.push(("Remove room from space", Action::RemoveFromSpace));
                }
                actions.push((
                    "Leave room…",
                    dispatch(Message::Membership(super::membership::Event::Leave(
                        id.clone(),
                    ))),
                ));
                actions
            }

            Target::Thread {
                room,
                generation,
                root,
            } => {
                if self.selection.room.as_ref() != Some(&(*generation, room.clone()))
                    || !super::current_pane(&self.model.room, *generation).is_some_and(|pane| {
                        pane.rows
                            .iter()
                            .any(|row| row.thread_root.as_ref() == Some(root))
                    })
                {
                    return Vec::new();
                }
                vec![
                    (
                        "Open thread",
                        dispatch(Message::OpenThreadLatest(root.clone())),
                    ),
                    ("Copy thread ID", Action::Copy(root.clone())),
                ]
            }
            Target::Link { message, url } => {
                let Some(row) = self.message_row(message) else {
                    return Vec::new();
                };
                // Revalidate the destination after edits or other timeline updates.
                if !row.formatted.as_ref().is_some_and(|document| {
                    document
                        .blocks
                        .iter()
                        .flat_map(|block| &block.runs)
                        .any(|run| run.style.link.as_ref() == Some(url))
                }) {
                    return Vec::new();
                }
                let mut actions = self.context_actions(&Target::Message(message.clone()));
                actions.insert(0, ("Copy link URL", Action::Copy(url.clone())));
                actions
            }
            Target::Message(target) => {
                let Some(row) = self.message_row(target) else {
                    return Vec::new();
                };
                let mut actions = vec![("Copy text", Action::Copy(row.body.clone()))];
                if let (Ok(room), Ok(event)) = (
                    matrix_sdk::ruma::RoomId::parse(&target.room),
                    matrix_sdk::ruma::EventId::parse(&target.event),
                ) {
                    actions.push((
                        "Copy message URL",
                        Action::Copy(room.matrix_to_event_uri(event).to_string()),
                    ));
                }
                if self.can_reply(target) {
                    actions.push((
                        "Reply",
                        dispatch(Message::Replying(super::message_reply::Event::Start(
                            target.clone(),
                        ))),
                    ));
                }
                if self.can_edit_message(target) {
                    actions.push((
                        "Edit message",
                        dispatch(Message::Editing(super::message_edit::Event::Start(
                            target.clone(),
                        ))),
                    ));
                }
                if let Some(action) = self.delete_action(row, target.thread, target.generation) {
                    actions.push(("Delete message…", dispatch(action)));
                }
                if !target.thread {
                    actions.push((
                        "Reply in thread",
                        dispatch(Message::Thread(target.event.clone())),
                    ));
                }
                if let Some(action) = self.reaction_action(row, target.thread, target.generation) {
                    actions.push(("Add reaction…", dispatch(action)));
                }
                if let Some(action) = self.pin_action(row, target.thread, target.generation) {
                    let unpin = matches!(
                        &action,
                        Message::Pin(super::pin_action::Event::Set(_, false))
                    );
                    actions.push((
                        if unpin {
                            "Unpin message"
                        } else {
                            "Pin message"
                        },
                        dispatch(action),
                    ));
                }
                actions
            }
        }
    }
    pub(super) fn validate_context_menu(&mut self) {
        if self.context_menu.as_ref().is_some_and(|state| {
            state.account != self.account_id || self.context_actions(&state.target).is_empty()
        }) {
            self.context_menu = None;
        }
    }
    pub(super) fn context_menu_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::EditSectionName(_) | Event::SaveSection | Event::CancelSection => {
                return self.section_form_event(event);
            }
            Event::Open(target, position) => {
                if !self.context_actions(&target).is_empty() {
                    let account = target == Target::Account;
                    self.context_menu = Some(State {
                        target,
                        account: self.account_id.clone(),
                        position,
                        sections: false,
                        form: None,
                        pending: None,
                        error: None,
                    });
                    self.emoji_completion.clear();
                    self.user_completion.clear();
                    if account {
                        return iced::widget::operation::focus("account-action:0");
                    }
                }
            }
            Event::Dismiss => {
                let account = self
                    .context_menu
                    .as_ref()
                    .is_some_and(|state| state.target == Target::Account);
                self.context_menu = None;
                if account {
                    return iced::widget::operation::focus(super::space_navigation::ACCOUNT);
                }
            }
            Event::AccountMove(index, backwards) => {
                if self
                    .context_menu
                    .as_ref()
                    .is_some_and(|state| state.target == Target::Account)
                {
                    let count = self.context_actions(&Target::Account).len();
                    if count > 0 {
                        let next = if backwards {
                            (index + count - 1) % count
                        } else {
                            (index + 1) % count
                        };
                        return iced::widget::operation::focus(format!("account-action:{next}"));
                    }
                }
            }
            Event::Back => {
                if let Some(state) = &mut self.context_menu
                    && state.pending.is_none()
                {
                    state.sections = false;
                    state.error = None;
                }
            }
            Event::Move(target, destination) => {
                if !self.context_menu_allowed()
                    || !self.context_menu.as_ref().is_some_and(|state| {
                        state.target == target
                            && state.sections
                            && state.form.is_none()
                            && state.pending.is_none()
                    })
                    || self.pending_ops >= super::MAX_PENDING_OPERATIONS
                    || !self
                        .move_destinations()
                        .iter()
                        .any(|(id, _)| id == &destination)
                {
                    return Task::none();
                }
                let Target::Room(room_id) = target else {
                    return Task::none();
                };
                if !self.model.rooms.iter().any(|room| room.id == room_id) {
                    return Task::none();
                }
                if self.demo.is_some() {
                    let room = std::sync::Arc::make_mut(&mut self.model.rooms)
                        .iter_mut()
                        .find(|room| room.id == room_id)
                        .unwrap();
                    room.tags
                        .retain(|tag| !interconnect::room_sections::is_section_tag(tag));
                    if destination != "chats" {
                        room.tags.push(destination.clone());
                    }
                    room.favorite = destination == "m.favourite";
                    self.context_menu = None;
                } else if let Some(directory) = self.model.people_directory.clone() {
                    self.generation += 1;
                    let request = self.generation;
                    let account = self.account_id.clone();
                    let state = self.context_menu.as_mut().unwrap();
                    state.pending = Some(request);
                    state.error = None;
                    self.pending_ops += 1;
                    return Task::perform(
                        directory.move_room_to_section(room_id, destination),
                        move |result| Message::RoomSectionMoved(account.clone(), request, result),
                    );
                }
            }
            Event::Choose(target, label) => {
                if !self
                    .context_menu
                    .as_ref()
                    .is_some_and(|state| state.target == target && state.account == self.account_id)
                {
                    return Task::none();
                }
                if self.context_menu.as_ref().is_some_and(|state| {
                    state.sections || state.form.is_some() || state.pending.is_some()
                }) {
                    return Task::none();
                }
                if let Some((_, action)) = self
                    .context_actions(&target)
                    .into_iter()
                    .find(|(name, _)| *name == label)
                {
                    return match action {
                        Action::CreateSection | Action::RenameSection | Action::RemoveSection => {
                            self.open_section_form(action)
                        }
                        Action::MarkUnread => self.mark_room_unread(target),
                        Action::RemoveFromSpace => self.remove_room_from_selected_space(target),
                        Action::Sections => {
                            self.context_menu.as_mut().unwrap().sections = true;
                            Task::none()
                        }
                        Action::Copy(value) => {
                            self.context_menu = None;
                            iced::clipboard::write(value)
                        }
                        Action::Dispatch(action) => {
                            self.context_menu = None;
                            self.update(*action)
                        }
                    };
                }
            }
        }
        Task::none()
    }
    fn mark_room_unread(&mut self, target: Target) -> Task<Message> {
        let Target::Room(id) = target else {
            return Task::none();
        };
        if self.pending_ops >= super::MAX_PENDING_OPERATIONS
            || (self.demo.is_none() && self.model.people_directory.is_none())
        {
            return Task::none();
        }
        if let Some((generation, room)) = &self.selection.room
            && *room == id
        {
            // Keep the explicit flag until this conversation is opened again.
            // Publish now: context-menu events bypass the normal update epilogue.
            self.manual_unread_generation = Some(*generation);
            self.selection.read = [None, None];
            self.publish_notification_context();
        }
        if self.demo.is_some() {
            if let Some(room) = std::sync::Arc::make_mut(&mut self.model.rooms)
                .iter_mut()
                .find(|room| room.id == id)
            {
                room.unread = true;
            }
            self.context_menu = None;
            return Task::none();
        }
        let directory = self.model.people_directory.clone().unwrap();
        self.generation += 1;
        let request = self.generation;
        let account = self.account_id.clone();
        let state = self.context_menu.as_mut().unwrap();
        state.pending = Some(request);
        state.error = None;
        self.pending_ops += 1;
        Task::perform(directory.mark_room_unread(id), move |result| {
            Message::RoomMarkedUnread(account.clone(), request, result)
        })
    }
    fn move_destinations(&self) -> Vec<(String, String)> {
        self.model
            .sections
            .iter()
            .filter_map(|section| {
                if section.id == "Rooms" {
                    Some(("chats".into(), "Default".into()))
                } else if interconnect::room_sections::is_section_tag(&section.id) {
                    Some((section.id.clone(), section.name.clone()))
                } else {
                    None
                }
            })
            .collect()
    }
    pub(super) fn room_menu_finished(
        &mut self,
        account: Option<String>,
        request: u64,
        result: Result<(), String>,
    ) {
        if account != self.account_id {
            return;
        }
        if self
            .context_menu
            .as_ref()
            .is_some_and(|state| state.pending == Some(request))
        {
            match result {
                Ok(()) => self.context_menu = None,
                Err(error) => {
                    let state = self.context_menu.as_mut().unwrap();
                    state.pending = None;
                    state.error = Some(error);
                }
            }
        } else if let Err(error) = result {
            self.model.navigation_error = Some(error);
        }
    }
    pub(super) fn context_menu_view<'a>(
        &'a self,
        base: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let popup = self.context_menu.as_ref().and_then(|state| {
            let actions = self.context_actions(&state.target);
            if actions.is_empty() {
                return None;
            }
            let items = if state.form.is_some() {
                column![self.section_form_view(state)]
            } else if state.sections {
                let mut items = column![
                    button(text("‹ Move to section").size(13))
                        .padding([7, 10])
                        .style(shell::quiet_button)
                        .on_press_maybe(state.pending.is_none().then(|| message(Event::Back)))
                ]
                .spacing(2);
                for (id, name) in self.move_destinations() {
                    let current = match &state.target {
                        Target::Room(room) => self
                            .model
                            .rooms
                            .iter()
                            .find(|entry| &entry.id == room)
                            .is_some_and(|entry| {
                                let section = entry.section_in(&self.model.sections);
                                section == id
                                    || (id == "chats"
                                        && matches!(section, "Rooms" | "Direct messages"))
                            }),
                        _ => false,
                    };
                    let label = if current { format!("✓ {name}") } else { name };
                    items = items.push(
                        button(text(label).size(13))
                            .width(iced::Length::Fill)
                            .padding([7, 10])
                            .style(shell::quiet_button)
                            .on_press_maybe(
                                (state.pending.is_none()
                                    && self.pending_ops < super::MAX_PENDING_OPERATIONS
                                    && (self.demo.is_some()
                                        || self.model.people_directory.is_some()))
                                .then(|| message(Event::Move(state.target.clone(), id))),
                            ),
                    );
                }
                if state.pending.is_some() {
                    items = items.push(text("Moving…").size(12));
                }
                if let Some(error) = &state.error {
                    items = items.push(text(error).size(12));
                }
                items
            } else {
                let heading = if state.target == Target::Account {
                    column![
                        self.account_menu_heading(),
                        iced::widget::rule::horizontal(1)
                    ]
                    .spacing(2)
                } else {
                    column![].spacing(2)
                };
                let mut items =
                    actions
                        .into_iter()
                        .enumerate()
                        .fold(heading, |items, (index, (label, _))| {
                            let action = message(Event::Choose(state.target.clone(), label.into()));
                            let control = button(text(label).size(13))
                                .width(iced::Length::Fill)
                                .padding([7, 10])
                                .style(shell::quiet_button)
                                .on_press_maybe(state.pending.is_none().then(|| action.clone()));
                            let control: Element<'_, Message> = if state.target == Target::Account {
                                super::navigation_control::view(
                                    control,
                                    format!("account-action:{index}"),
                                    action,
                                    move |key, modifiers| {
                                        use iced::keyboard::{Key, key::Named};
                                        match key {
                                            Key::Named(Named::ArrowUp) => {
                                                Some(message(Event::AccountMove(index, true)))
                                            }
                                            Key::Named(Named::ArrowDown) => {
                                                Some(message(Event::AccountMove(index, false)))
                                            }
                                            Key::Named(Named::Tab) => Some(message(
                                                Event::AccountMove(index, modifiers.shift()),
                                            )),
                                            _ => None,
                                        }
                                    },
                                )
                            } else {
                                control.into()
                            };
                            items.push(control)
                        });
                if state.pending.is_some() {
                    let removing = self
                        .space_management
                        .as_ref()
                        .is_some_and(|removal| state.pending == Some(removal.request));
                    items =
                        items.push(text(if removing { "Removing…" } else { "Saving…" }).size(12));
                }
                if let Some(error) = &state.error {
                    items = items.push(text(error).size(12));
                }
                if state.target == Target::Sections {
                    for section in self
                        .model
                        .sections
                        .iter()
                        .filter(|section| section.id.starts_with("element.io.section."))
                    {
                        items = items.push(
                            button(text(section.name.clone()).size(13))
                                .width(iced::Length::Fill)
                                .padding([7, 10])
                                .style(shell::quiet_button)
                                .on_press(message(Event::Open(
                                    Target::Section(section.id.clone()),
                                    state.position,
                                ))),
                        );
                    }
                }
                items
            };
            Some(
                container(iced::widget::scrollable(items).height(iced::Length::Shrink))
                    .max_height(360)
                    .width(if state.form.is_some() {
                        280
                    } else if state.target == Target::Account {
                        260
                    } else if state.sections {
                        240
                    } else {
                        200
                    })
                    .padding(5)
                    .style(|theme| container::Style {
                        border: iced::Border {
                            color: shell::palette(theme).line,
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..shell::surface(theme, shell::palette(theme).background)
                    })
                    .into(),
            )
        });
        if self
            .context_menu
            .as_ref()
            .is_some_and(|state| state.target == Target::Account)
        {
            return popover::view(
                base,
                Some("account-menu-button".into()),
                popup,
                message(Event::Dismiss),
            );
        }
        popover::at_position(
            base,
            self.context_menu.as_ref().map(|state| state.position),
            popup,
            message(Event::Dismiss),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn target(app: &App, thread: bool, event: &str) -> Target {
        Target::Message(reaction_picker::Target {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            generation: if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            },
            thread,
            event: event.into(),
        })
    }
    fn open(app: &mut App, target: &Target) {
        let _ = app.update(message(Event::Open(
            target.clone(),
            Point::new(630.0, 470.0),
        )));
    }
    fn choose(app: &mut App, target: &Target, label: &str) {
        let _ = app.update(message(Event::Choose(target.clone(), label.into())));
    }
    #[test]
    fn right_click_copies_hit_destination_and_rejects_edited_or_stale_links() {
        use iced::advanced::{Layout, Shell, layout, mouse, widget::Tree};
        use iced::{Event as Input, Rectangle, Size, Vector};
        let mut app = demo_app();
        let _ = app.update(Message::Room("formatting".into()));
        let generation = app.selection.room.as_ref().unwrap().0;
        let mut row = super::super::current_pane(&app.model.room, generation)
            .unwrap()
            .rows
            .iter()
            .find(|row| row.event_id.as_deref() == Some("format-links"))
            .unwrap()
            .clone();
        for document in [
            interconnect::message_document::Document::parse(concat!(
                "<a href='https://example.org/first'>first</a> ",
                "<a href='https://example.org/second?q=1&amp;x=2'>second</a>"
            ))
            .unwrap(),
            interconnect::message_document::Document::plain_links(concat!(
                "https://example.org/first ",
                "https://example.org/second?q=1&x=2"
            ))
            .unwrap(),
        ] {
            row.formatted = Some(std::sync::Arc::new(document));
            // Install the same document that the rendered row uses for action revalidation.
            if let Some(pane) = &mut app.model.room {
                std::sync::Arc::make_mut(&mut pane.rows)
                    .iter_mut()
                    .find(|r| r.event_id == row.event_id)
                    .unwrap()
                    .formatted = row.formatted.clone();
            }
            let mut view = app.message_context(
                super::super::selectable::message_view(&row),
                &row,
                false,
                generation,
            );
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
            let mut tree = Tree::new(&view);
            let size = Size::new(700.0, 200.0);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            let viewport = Rectangle::with_size(size);
            let translation = Vector::new(0.0, -40.0);
            let _ = view.as_widget_mut().overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &viewport,
                translation,
            );
            // Scan glyph positions to exercise both destinations and empty row space.
            let mut found = Vec::new();
            for x in (0..700).step_by(3) {
                let cursor = Point::new(x as f32, 10.0);
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &Input::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
                    Layout::new(&node),
                    mouse::Cursor::Available(cursor),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &viewport,
                );
                assert!(shell.is_event_captured());
                if let [Message::ContextMenu(Event::Open(target, position))] = &messages[..] {
                    assert_eq!(*position, cursor + translation);
                    if !found.contains(target) {
                        found.push(target.clone());
                    }
                } else {
                    panic!("expected a single context menu event");
                }
            }
            drop(view);
            assert!(found.iter().any(|t| matches!(t, Target::Message(_))));
            for url in [
                "https://example.org/first",
                "https://example.org/second?q=1&x=2",
            ] {
                let link = found
                    .iter()
                    .find(|t| matches!(t, Target::Link { url: hit, .. } if hit == url))
                    .unwrap();
                open(&mut app, link);
                assert!(
                    matches!(&app.context_actions(link)[0], ("Copy link URL", Action::Copy(value)) if value == url)
                );
                choose(&mut app, link, "Copy link URL");
                assert!(app.context_menu.is_none());
            }
            let link = found
                .iter()
                .find(|t| matches!(t, Target::Link { .. }))
                .unwrap();
            let mut stale = link.clone();
            if let Target::Link { message, .. } = &mut stale {
                message.generation = message.generation.wrapping_add(1);
            }
            open(&mut app, &stale);
            assert!(app.context_menu.is_none());
            assert!(app.context_actions(&stale).is_empty());
            open(&mut app, link);
            if let Some(pane) = &mut app.model.room {
                std::sync::Arc::make_mut(&mut pane.rows)
                    .iter_mut()
                    .find(|r| r.event_id == row.event_id)
                    .unwrap()
                    .formatted = None;
            }
            assert!(app.context_actions(link).is_empty());
            app.validate_context_menu();
            assert!(app.context_menu.is_none());
        }
    }

    #[test]
    fn message_urls_identify_room_events_and_thread_replies_and_reject_stale_targets() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let synthetic = target(&app, false, "review");
        assert!(!app.context_actions(&synthetic).is_empty());
        assert!(
            !app.context_actions(&synthetic)
                .iter()
                .any(|(label, _)| *label == "Copy message URL")
        );
        app.selection.room.as_mut().unwrap().1 = "!room:example.org".into();
        for (thread, old_event, event) in [
            (false, "review", "$root:example.org"),
            (true, "review-reply-1", "$reply/with+symbols"),
        ] {
            let pane = if thread {
                &mut app.model.thread
            } else {
                &mut app.model.room
            };
            std::sync::Arc::make_mut(&mut pane.as_mut().unwrap().rows)
                .iter_mut()
                .find(|row| row.event_id.as_deref() == Some(old_event))
                .unwrap()
                .event_id = Some(event.into());
            let message = target(&app, thread, event);
            open(&mut app, &message);
            let actions = app.context_actions(&message);
            let url = actions
                .iter()
                .find_map(|(label, action)| match action {
                    Action::Copy(value) if *label == "Copy message URL" => Some(value),
                    _ => None,
                })
                .unwrap();
            assert!(url.starts_with("https://matrix.to/#/"));
            let parsed = matrix_sdk::ruma::MatrixToUri::parse(url).unwrap();
            assert_eq!(
                parsed.id(),
                &matrix_sdk::ruma::matrix_uri::MatrixId::Event(
                    matrix_sdk::ruma::RoomOrAliasId::parse("!room:example.org").unwrap(),
                    matrix_sdk::ruma::EventId::parse(event).unwrap(),
                )
            );
            choose(&mut app, &message, "Copy message URL");
            assert!(app.context_menu.is_none());
            let mut stale = message.clone();
            if let Target::Message(target) = &mut stale {
                target.generation += 1;
            }
            assert!(app.context_actions(&stale).is_empty());
        }
        std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
            .iter_mut()
            .find(|row| row.event_id.as_deref() == Some("$root:example.org"))
            .unwrap()
            .event_id = Some("local-only".into());
        let local = target(&app, false, "local-only");
        assert!(!app.context_actions(&local).is_empty());
        assert!(
            !app.context_actions(&local)
                .iter()
                .any(|(label, _)| *label == "Copy message URL")
        );
    }

    #[test]
    fn room_and_dm_links_use_the_room_id_and_preserve_conversation_state() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 75.0);
        let selection = app.selection.clone();
        for direct in [false, true] {
            let id = if direct {
                "!dm:example.org"
            } else {
                "!room:example.org"
            };
            std::sync::Arc::make_mut(&mut app.model.rooms).push(
                interconnect::navigation::RoomEntry {
                    id: id.into(),
                    direct,
                    canonical_alias: Some("#alias:example.org".into()),
                    ..Default::default()
                },
            );
            let target = Target::Room(id.into());
            open(&mut app, &target);
            let actions = app.context_actions(&target);
            assert!(
                actions
                    .iter()
                    .any(|(label, action)| matches!(action, Action::Copy(value)
                if *label == "Copy room link" && value == &format!("https://matrix.to/#/{id}")))
            );
            choose(&mut app, &target, "Copy room link");
            assert!(app.context_menu.is_none());
            assert_eq!(app.selection, selection);
            assert_eq!(app.drafts[&key].content.text(), "unsent");
            assert_eq!(app.scroll_positions[&key], 75.0);
            open(&mut app, &target);
            std::sync::Arc::make_mut(&mut app.model.rooms).retain(|room| room.id != id);
            app.validate_context_menu();
            assert!(app.context_menu.is_none());
            assert!(app.context_actions(&target).is_empty());
        }
        assert!(
            !app.context_actions(&Target::Room("general".into()))
                .iter()
                .any(|(label, _)| *label == "Copy room link")
        );
    }

    #[test]
    fn room_moves_replace_membership_without_switching_conversation_or_disturbing_drafts() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let selected = app.selection.room.clone();
        let thread = app.selection.thread.clone();
        let draft = app.draft_key(true).unwrap();
        app.drafts.get_mut(&draft).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(draft.clone(), 91.0);
        let target = Target::Room("general".into());
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|r| r.id == "general")
            .unwrap()
            .tags = vec![
            "m.favourite".into(),
            "element.io.section.orphan".into(),
            "u.keep".into(),
        ];
        for destination in [
            "element.io.section.demo-design",
            "m.lowpriority",
            "m.favourite",
            "chats",
        ] {
            open(&mut app, &target);
            choose(&mut app, &target, "Move to section…");
            assert!(app.context_menu.as_ref().unwrap().sections);
            let _ = app.update(message(Event::Move(target.clone(), destination.into())));
            assert!(app.context_menu.is_none());
            let room = app.model.rooms.iter().find(|r| r.id == "general").unwrap();
            assert!(room.tags.contains(&"u.keep".into()));
            assert!(!room.tags.contains(&"element.io.section.orphan".into()));
            assert_eq!(room.favorite, destination == "m.favourite");
            assert_eq!(
                room.section_in(&app.model.sections),
                if destination == "chats" {
                    "Rooms"
                } else {
                    destination
                }
            );
            assert_eq!(app.selection.room, selected);
            assert_eq!(app.selection.thread, thread);
            assert_eq!(app.drafts[&draft].content.text(), "Unsent");
            assert_eq!(app.scroll_positions[&draft], 91.0);
        }
    }
    #[test]
    fn section_choices_reject_dismissal_removed_destinations_and_pending_or_modal_actions() {
        let mut app = demo_app();
        let target = Target::Room("general".into());
        open(&mut app, &target);
        choose(&mut app, &target, "Move to section…");
        let before = app.model.rooms.clone();
        let _ = app.update(Message::Escape);
        let _ = app.update(message(Event::Move(target.clone(), "m.lowpriority".into())));
        assert_eq!(app.model.rooms, before);
        open(&mut app, &target);
        choose(&mut app, &target, "Move to section…");
        std::sync::Arc::make_mut(&mut app.model.sections)
            .retain(|s| s.id != "element.io.section.demo-design");
        let _ = app.update(message(Event::Move(
            target.clone(),
            "element.io.section.demo-design".into(),
        )));
        assert_eq!(app.model.rooms, before);
        app.context_menu.as_mut().unwrap().pending = Some(77);
        let _ = app.update(message(Event::Move(target.clone(), "m.lowpriority".into())));
        assert_eq!(app.model.rooms, before);
        app.context_menu.as_mut().unwrap().pending = None;
        app.verification.open = true;
        let _ = app.update(message(Event::Move(target, "m.lowpriority".into())));
        assert_eq!(app.model.rooms, before);
    }
    #[test]
    fn move_completion_rejects_other_accounts_keeps_new_menus_and_finishes_shutdown() {
        let mut app = demo_app();
        let target = Target::Room("general".into());
        open(&mut app, &target);
        choose(&mut app, &target, "Move to section…");
        app.context_menu.as_mut().unwrap().pending = Some(77);
        app.pending_ops = 1;
        let _ = app.update(Message::RoomSectionMoved(
            app.account_id.clone(),
            77,
            Err("Refused".into()),
        ));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(
            app.context_menu.as_ref().unwrap().error.as_deref(),
            Some("Refused")
        );
        app.pending_ops = 1;
        let _ = app.update(Message::RoomSectionMoved(
            Some("other account".into()),
            77,
            Ok(()),
        ));
        assert!(app.context_menu.is_some());
        app.pending_ops = 1;
        open(&mut app, &Target::Room("studio".into()));
        let _ = app.update(Message::RoomSectionMoved(
            app.account_id.clone(),
            77,
            Ok(()),
        ));
        assert_eq!(
            app.context_menu.as_ref().unwrap().target,
            Target::Room("studio".into())
        );
        app.pending_ops = 1;
        app.closing = true;
        let _ = app.update(Message::RoomSectionMoved(
            app.account_id.clone(),
            77,
            Err("Failed after dismissal".into()),
        ));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
        assert!(app.context_menu.is_none());
        assert_eq!(
            app.model.navigation_error.as_deref(),
            Some("Failed after dismissal")
        );
    }

    #[test]
    fn thread_summary_opens_its_thread_and_open_menus_block_attachment_drops() {
        let mut app = demo_app();
        let (generation, room) = app.selection.room.clone().unwrap();
        let key = app.draft_key(false).unwrap();
        assert!(app.attachment_drop_target(&key, generation).is_some());
        let summary = Target::Thread {
            room,
            generation,
            root: "review".into(),
        };
        open(&mut app, &summary);
        assert!(app.context_menu.is_some());
        assert!(app.attachment_drop_target(&key, generation).is_none());
        choose(&mut app, &summary, "Open thread");
        assert!(app.context_menu.is_none());
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "review");
        assert!(app.attachment_drop_target(&key, generation).is_some());
    }
    #[test]
    fn room_menu_preserves_open_conversation_drafts_and_scroll_until_open_is_chosen() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent draft");
        app.scroll_positions.insert(key.clone(), 70.0);
        let selected = app.selection.room.clone();
        let room = Target::Room("general".into());
        open(&mut app, &room);
        assert!(app.context_menu.is_some());
        assert_eq!(app.selection.room, selected);
        let _ = app.update(Message::Escape);
        assert!(app.context_menu.is_none());
        choose(&mut app, &room, "Open room");
        assert_eq!(app.selection.room, selected);
        open(&mut app, &room);
        choose(&mut app, &room, "Open room");
        assert_eq!(app.selection.room.as_ref().unwrap().1, "general");
        assert_eq!(app.drafts[&key].content.text(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 70.0);
    }
    #[test]
    fn message_actions_revalidate_permissions_and_reject_navigation_and_dismissed_choices() {
        let mut app = demo_app();
        let row = target(&app, false, "review");
        open(&mut app, &row);
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .can_pin = false;
        choose(&mut app, &row, "Pin message");
        assert!(
            !app.model
                .rooms
                .iter()
                .find(|room| room.id == "studio")
                .unwrap()
                .pinned_events
                .contains(&"review".into())
        );
        open(&mut app, &row);
        choose(&mut app, &row, "Reply in thread");
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "review");
        let thread = app.selection.thread.clone();
        let reply = target(&app, true, "review-reply-1");
        open(&mut app, &reply);
        assert!(
            app.context_actions(&reply)
                .iter()
                .any(|(label, _)| *label == "Add reaction…")
        );
        assert!(
            matches!(&app.context_actions(&reply)[0].1, Action::Copy(value) if !value.is_empty())
        );
        let _ = app.update(Message::Escape);
        assert_eq!(
            app.selection.thread, thread,
            "Escape dismisses only the menu"
        );
        open(&mut app, &reply);
        let _ = app.update(Message::Room("general".into()));
        choose(&mut app, &reply, "Add reaction…");
        assert!(app.reaction_picker.is_none());
        assert!(app.context_actions(&reply).is_empty());
        open(&mut app, &reply);
        assert!(app.context_menu.is_none());
    }
    #[test]
    fn thread_original_actions_and_modal_and_focus_dismissal() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let original = target(&app, true, "review");
        open(&mut app, &original);
        choose(&mut app, &original, "Add reaction…");
        assert!(app.context_menu.is_none());
        assert!(app.reaction_picker.is_some());
        open(&mut app, &original);
        assert!(app.context_menu.is_none());
        let _ = app.update(Message::Escape);
        open(&mut app, &original);
        let _ = app.update(Message::WindowFocused(false));
        assert!(app.context_menu.is_none());
        open(&mut app, &original);
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert!(app.context_menu.is_none());
        open(&mut app, &original);
        assert!(app.context_menu.is_none());
    }
    #[test]
    fn edit_menu_only_offers_own_messages_and_rechecks_the_source_on_selection() {
        let mut app = demo_app();
        let own = target(&app, false, "proposal");
        let other = target(&app, false, "review");
        assert!(
            app.context_actions(&own)
                .iter()
                .any(|(label, _)| *label == "Edit message")
        );
        assert!(
            !app.context_actions(&other)
                .iter()
                .any(|(label, _)| *label == "Edit message")
        );
        open(&mut app, &own);
        let rows = std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows);
        let source = rows
            .iter_mut()
            .find(|row| row.key == "proposal")
            .unwrap()
            .edit_source
            .take();
        choose(&mut app, &own, "Edit message");
        let key = app.draft_key(false).unwrap();
        assert!(app.drafts[&key].editing.is_none());
        let rows = std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows);
        rows.iter_mut()
            .find(|row| row.key == "proposal")
            .unwrap()
            .edit_source = source;
        open(&mut app, &own);
        choose(&mut app, &own, "Edit message");
        assert!(app.context_menu.is_none());
        assert!(app.drafts[&key].editing.is_some());
    }
}
