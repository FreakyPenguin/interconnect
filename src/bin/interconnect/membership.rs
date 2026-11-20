//! Account-bound membership dialogs; closing a dialog never retargets a write.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, text, text_input},
};
use interconnect::{connected::Model, navigation::Scope, room_membership::Joined};

#[derive(Default)]
pub struct State {
    pub open: bool,
    mode: Mode,
    input: String,
    link_event: Option<matrix_sdk::ruma::OwnedEventId>,
    generation: u64,
    pending: Option<(u64, u64)>,
    error: Option<String>,
    settling: Vec<Change>,
    leave_plan: Option<interconnect::room_leave::Plan>,
    leave_load: Option<iced::task::Handle>,
}
#[derive(Clone, Debug, Default)]
enum Mode {
    #[default]
    Invitations,
    Join,
    Leave(String),
}
#[derive(Clone, Debug)]
enum Change {
    Joined(Joined),
    Removed(String),
}
impl Change {
    fn apply(&self, model: &mut Model) {
        match self {
            Self::Joined(joined) => {
                std::sync::Arc::make_mut(&mut model.invitations).retain(|r| r.id != joined.id);
                if joined.space {
                    if !model.spaces.iter().any(|r| r.id == joined.id) {
                        std::sync::Arc::make_mut(&mut model.spaces).push(joined.space());
                    }
                } else if !model.rooms.iter().any(|r| r.id == joined.id) {
                    std::sync::Arc::make_mut(&mut model.rooms).push(joined.room());
                }
            }
            Self::Removed(id) => {
                std::sync::Arc::make_mut(&mut model.invitations).retain(|r| &r.id != id);
                std::sync::Arc::make_mut(&mut model.rooms).retain(|r| &r.id != id);
                std::sync::Arc::make_mut(&mut model.spaces).retain(|r| &r.id != id);
            }
        }
    }
    fn confirmed(&self, model: &Model) -> bool {
        match self {
            Self::Joined(joined) => {
                !model.invitations.iter().any(|r| r.id == joined.id)
                    && (model.rooms.iter().any(|r| r.id == joined.id)
                        || model.spaces.iter().any(|r| r.id == joined.id))
            }
            Self::Removed(id) => {
                !model.invitations.iter().any(|r| &r.id == id)
                    && !model.rooms.iter().any(|r| &r.id == id)
                    && !model.spaces.iter().any(|r| &r.id == id)
            }
        }
    }
}
impl State {
    pub(super) fn cancel_leave_read(&mut self) {
        self.leave_load = None;
        self.leave_plan = None;
    }
    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn record_removed(&mut self, id: String, model: &mut Model, retain: bool) {
        let change = Change::Removed(id);
        change.apply(model);
        if retain {
            self.settling.push(change);
        }
    }
    pub(super) fn record_joined(&mut self, joined: Joined, model: &mut Model, retain: bool) {
        let change = Change::Joined(joined);
        change.apply(model);
        if retain {
            self.settling.push(change);
        }
    }
    pub fn reset(&mut self) {
        self.open = false;
        self.link_event = None;
        self.error = None;
        self.settling.clear();
        self.leave_plan = None;
        self.leave_load = None;
    }
    pub fn reconcile(&mut self, model: &mut Model) {
        self.settling.retain(|change| !change.confirmed(model));
        for change in &self.settling {
            change.apply(model);
        }
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Link(interconnect::room_membership::Address),
    MessageLink(
        interconnect::room_membership::Address,
        matrix_sdk::ruma::OwnedEventId,
    ),
    Join,
    Leave(String),
    ReloadLeave,
    ReviewMembers,
    Ignore,
    LeavePrepared(
        u64,
        Option<String>,
        String,
        Result<interconnect::room_leave::Plan, String>,
    ),
    Input(String),
    Submit,
    Close,
    Respond(String, bool),
    Finished {
        request: u64,
        account: Option<String>,
        room: String,
        result: Result<Option<Joined>, String>,
    },
}
fn message(event: Event) -> Message {
    Message::Membership(event)
}

impl App {
    pub(super) fn membership_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::ReviewMembers if self.membership.open && self.membership.pending.is_none() => {
                let Mode::Leave(room) = &self.membership.mode else {
                    return Task::none();
                };
                let room = room.clone();
                let _ = self.membership_event(Event::Close);
                return self.update(Message::SpaceSettings(
                    super::space_settings::Event::OpenMembers(room),
                ));
            }
            Event::ReloadLeave
                if self.membership.open
                    && self.membership.pending.is_none()
                    && !self.closing
                    && !self.stopped
                    && (self.demo.is_some() || self.model.authentication.is_none()) =>
            {
                let Mode::Leave(id) = &self.membership.mode else {
                    return Task::none();
                };
                let id = id.clone();
                self.membership.leave_load = None;
                self.membership.leave_plan = None;
                self.membership.error = None;
                self.generation += 1;
                self.membership.generation = self.generation;
                if self.demo.is_some() {
                    let privileged = self
                        .model
                        .rooms
                        .iter()
                        .find(|r| r.id == id)
                        .is_some_and(|r| r.can_edit_topic);
                    self.membership.leave_plan =
                        Some(interconnect::room_leave::Plan::demo(id, privileged));
                } else if let Some(directory) = self.model.people_directory.clone() {
                    let generation = self.membership.generation;
                    let account = self.account_id.clone();
                    let target = id.clone();
                    let (task, handle) =
                        Task::perform(directory.prepare_room_leave(id), move |result| {
                            message(Event::LeavePrepared(
                                generation,
                                account.clone(),
                                target.clone(),
                                result,
                            ))
                        })
                        .abortable();
                    self.membership.leave_load = Some(handle.abort_on_drop());
                    return task;
                } else {
                    self.membership.error = Some(
                        "The account is not ready. Reload to check leave consequences.".into(),
                    );
                }
            }
            Event::LeavePrepared(generation, account, id, result) => {
                if self.membership.open
                    && !self.closing
                    && !self.stopped
                    && generation == self.membership.generation
                    && account == self.account_id
                    && matches!(&self.membership.mode, Mode::Leave(target) if *target == id)
                {
                    self.membership.leave_load = None;
                    match result {
                        Ok(plan) => self.membership.leave_plan = Some(plan),
                        Err(error) => self.membership.error = Some(error),
                    }
                }
            }

            Event::Link(_) | Event::MessageLink(_, _) if self.context_menu_allowed() => {
                let (address, event) = match event {
                    Event::Link(address) => (address, None),
                    Event::MessageLink(address, event) => (address, Some(event)),
                    _ => unreachable!(),
                };
                if let Some(room) = self.model.rooms.iter().find(|room| {
                    room.id == address.room.as_str()
                        || room.canonical_alias.as_deref() == Some(address.room.as_str())
                        || (self.demo.is_some()
                            && super::user_completion::room_target(
                                room,
                                true,
                                self.account_id.as_deref(),
                            )
                            .and_then(|target| target.room_id().cloned())
                            .as_ref()
                                == Some(&address.room))
                }) {
                    let task = self.update(Message::Room(room.id.clone()));
                    if let Some(event) = event {
                        self.focus_link_event(event);
                    }
                    return task;
                }
                if event.is_none()
                    && self
                        .model
                        .spaces
                        .iter()
                        .any(|space| space.id == address.room.as_str())
                {
                    return self.update(Message::Space(Scope::Space(address.room.to_string())));
                }
                let task = self.membership_event(Event::Join);
                if self.membership.open {
                    self.membership.link_event = event;
                    self.membership.input = interconnect::composition::MentionTarget::Room {
                        room_id: address.room,
                        via: address.via,
                    }
                    .uri()
                    .unwrap_or_default();
                }
                return task;
            }
            Event::Open | Event::Join | Event::Leave(_) if self.context_menu_allowed() => {
                self.membership.mode = match event {
                    Event::Join => Mode::Join,
                    Event::Leave(id) => {
                        if !self.model.rooms.iter().any(|r| r.id == id) {
                            return Task::none();
                        }
                        Mode::Leave(id)
                    }
                    _ => Mode::Invitations,
                };
                self.membership.input.clear();
                self.membership.link_event = None;
                self.context_menu = None;
                self.stop_typing();
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.generation += 1;
                self.membership.generation = self.generation;
                self.membership.open = true;
                self.membership.error = None;
                self.membership.leave_plan = None;
                self.membership.leave_load = None;
                let focus =
                    iced::widget::operation::focus(if matches!(self.membership.mode, Mode::Join) {
                        "join-room-address"
                    } else {
                        "membership-dialog"
                    });
                let load = if matches!(self.membership.mode, Mode::Leave(_)) {
                    self.membership_event(Event::ReloadLeave)
                } else {
                    Task::none()
                };
                return Task::batch([focus, load]);
            }
            Event::Close => {
                self.membership.leave_plan = None;
                self.membership.leave_load = None;
                self.membership.open = false;
                self.membership.link_event = None;
                return iced::widget::operation::focus("room-composer");
            }
            Event::Input(value)
                if self.membership.open
                    && matches!(self.membership.mode, Mode::Join)
                    && self.membership.pending.is_none() =>
            {
                self.membership.input = value;
                self.membership.link_event = None;
                self.membership.error = None;
            }
            Event::Submit
                if self.membership.open
                    && self.membership.pending.is_none()
                    && !self.closing
                    && !self.stopped
                    && !self.composing
                    && self.pending_ops < super::MAX_PENDING_OPERATIONS
                    && (self.demo.is_some() || self.model.authentication.is_none()) =>
            {
                let (room, joining) = match &self.membership.mode {
                    Mode::Join => {
                        if let Err(error) =
                            interconnect::room_membership::Address::parse(&self.membership.input)
                        {
                            self.membership.error = Some(error);
                            return Task::none();
                        }
                        (self.membership.input.trim().to_owned(), true)
                    }
                    Mode::Leave(id)
                        if self.membership.leave_plan.is_some()
                            && self.membership.leave_load.is_none()
                            && self.model.rooms.iter().any(|r| &r.id == id) =>
                    {
                        (id.clone(), false)
                    }
                    _ => return Task::none(),
                };
                let service = self.model.people_directory.clone();
                if self.demo.is_none() && service.is_none() {
                    return Task::none();
                }
                self.generation += 1;
                let request = self.generation;
                let account = self.account_id.clone();
                self.membership.pending = Some((request, self.membership.generation));
                self.membership.error = None;
                self.pending_ops += 1;
                if self.demo.is_some() {
                    let joined = joining.then(|| Joined {
                        id: interconnect::room_membership::Address::parse(&room)
                            .ok()
                            .and_then(|address| {
                                address
                                    .room
                                    .as_str()
                                    .parse::<matrix_sdk::ruma::OwnedRoomId>()
                                    .ok()
                            })
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| format!("!demo-joined-{request}:example.org")),
                        name: room.clone(),
                        space: false,
                    });
                    return self.membership_event(Event::Finished {
                        request,
                        account,
                        room,
                        result: Ok(joined),
                    });
                }
                let target = room.clone();
                let leave_plan = self.membership.leave_plan.take();
                return Task::perform(
                    async move {
                        if joining {
                            service.unwrap().join_room(target).await.map(Some)
                        } else {
                            service
                                .unwrap()
                                .leave_room_checked(
                                    target,
                                    leave_plan.expect("reviewed leave warning"),
                                )
                                .await
                                .map(|()| None)
                        }
                    },
                    move |result| {
                        message(Event::Finished {
                            request,
                            account: account.clone(),
                            room: room.clone(),
                            result,
                        })
                    },
                );
            }
            Event::Respond(room, accept)
                if self.membership.open
                    && matches!(self.membership.mode, Mode::Invitations)
                    && self.membership.pending.is_none()
                    && !self.closing
                    && !self.stopped
                    && self.pending_ops < super::MAX_PENDING_OPERATIONS
                    && (self.demo.is_some() || self.model.authentication.is_none()) =>
            {
                let Some(invitation) = self.model.invitations.iter().find(|r| r.id == room) else {
                    return Task::none();
                };
                let joined = accept.then(|| Joined {
                    id: room.clone(),
                    name: invitation.name.clone(),
                    space: invitation.space,
                });
                self.generation += 1;
                let request = self.generation;
                let account = self.account_id.clone();
                let service = self.model.people_directory.clone();
                if self.demo.is_none() && service.is_none() {
                    return Task::none();
                }
                self.membership.pending = Some((request, self.membership.generation));
                self.membership.error = None;
                self.pending_ops += 1;
                if self.demo.is_some() {
                    return self.membership_event(Event::Finished {
                        request,
                        account,
                        room,
                        result: Ok(joined),
                    });
                }
                let target = room.clone();
                return Task::perform(
                    service.unwrap().respond_to_invitation(target, accept),
                    move |result| {
                        message(Event::Finished {
                            request,
                            account: account.clone(),
                            room: room.clone(),
                            result,
                        })
                    },
                );
            }
            Event::Finished {
                request,
                account,
                room,
                result,
            } => {
                let Some((pending, origin)) = self.membership.pending else {
                    return Task::none();
                };
                if pending != request {
                    return Task::none();
                }
                self.membership.pending = None;
                self.pending_ops -= 1;
                if self.closing
                    || self.stopped
                    || self.account_id != account
                    || (self.demo.is_none() && self.model.authentication.is_some())
                {
                    return Task::none();
                }
                let visible = self.membership.open && self.membership.generation == origin;
                match result {
                    Ok(joined) => {
                        let change = joined
                            .clone()
                            .map(Change::Joined)
                            .unwrap_or(Change::Removed(room.clone()));
                        change.apply(&mut self.model);
                        if matches!(&change, Change::Removed(_)) {
                            if self
                                .pending_created_room
                                .as_ref()
                                .is_some_and(|r| r.id == room)
                            {
                                self.pending_created_room = None;
                            }
                            if self
                                .selection
                                .room
                                .as_ref()
                                .is_some_and(|(_, id)| id == &room)
                            {
                                self.stop_typing();
                                self.selection.room = None;
                                self.selection.thread = None;
                                self.selection.members = None;
                                self.selection.event_focus = [None, None];
                                self.pending_room_focus = None;
                            }
                            if visible && matches!(self.membership.mode, Mode::Leave(_)) {
                                self.membership.open = false;
                            }
                        }
                        if self.demo.is_none() {
                            self.membership.settling.push(change);
                        }
                        if let Some(joined) = joined
                            && visible
                        {
                            self.membership.open = false;
                            if joined.space {
                                self.shell.space = Scope::Space(joined.id);
                            } else {
                                self.shell.space = Scope::All;
                                let task = self.update(Message::Room(joined.id));
                                if let Some(event) = self.membership.link_event.take() {
                                    self.focus_link_event(event);
                                }
                                return task;
                            }
                        }
                    }
                    Err(error) if visible => {
                        self.membership.leave_plan = None;
                        self.membership.error = Some(error);
                    }
                    Err(_) => {}
                }
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn membership_view(&self) -> Element<'_, Message> {
        let modal = iced::widget::responsive(|size| {
            let busy = self.membership.pending.is_some();
            let mut entries = column![].spacing(14).width(Length::Fill);
            let title = match &self.membership.mode {
                Mode::Invitations => {
                    for invitation in self.model.invitations.iter() {
                        let mut entry = column![
                            shell::title(&invitation.name, 15),
                            text(&invitation.id)
                                .size(11)
                                .style(shell::muted_text)
                                .wrapping(text::Wrapping::Glyph),
                        ]
                        .spacing(8);
                        if let Some(inviter) = &invitation.inviter {
                            entry = entry.push(text(format!("Invited by {inviter}")).size(12));
                        }
                        if invitation.space {
                            entry = entry.push(text("Space invitation").size(12));
                        }
                        entry = entry.push(
                            row![
                                button(text("Decline").size(13))
                                    .keyboard_navigation(true)
                                    .padding([7, 10])
                                    .style(shell::quiet_button)
                                    .on_press_maybe((!busy).then(|| message(Event::Respond(
                                        invitation.id.clone(),
                                        false
                                    )))),
                                button(text("Accept invitation").size(13))
                                    .keyboard_navigation(true)
                                    .padding([7, 10])
                                    .style(shell::primary_button)
                                    .on_press_maybe((!busy).then(|| message(Event::Respond(
                                        invitation.id.clone(),
                                        true
                                    )))),
                            ]
                            .spacing(8),
                        );
                        entries = entries.push(
                            container(entry)
                                .padding(14)
                                .width(Length::Fill)
                                .style(shell::result_row),
                        );
                    }
                    if self.model.invitations.is_empty() {
                        entries = entries.push(shell::empty_state(
                            "No invitations",
                            "New room and space invitations will appear here.",
                        ));
                    }
                    "Invitations"
                }
                Mode::Join => {
                    entries = entries.push(text("Room address or link").size(13));
                    let input =
                        text_input("#room:example.org or room link", &self.membership.input)
                            .id("join-room-address")
                            .cursor_blinking(false)
                            .style(shell::search_style)
                            .size(13)
                            .padding([7, 10]);
                    entries = entries.push(if busy {
                        input
                    } else {
                        input
                            .on_input(|value| message(Event::Input(value)))
                            .on_submit(message(Event::Submit))
                    });
                    entries = entries.push(text("Use a room alias, room ID, matrix.to link or matrix: link. Private rooms may require an invitation.").size(12).style(shell::muted_text));
                    "Join a room"
                }
                Mode::Leave(id) => {
                    let name = self
                        .model
                        .rooms
                        .iter()
                        .find(|r| &r.id == id)
                        .map_or(id.as_str(), |r| r.name.as_str());
                    entries = entries.push(shell::title(name, 16));
                    if let Some(plan) = &self.membership.leave_plan {
                        let concern = if plan.last_admin {
                            Some("You are the last member who can manage permissions.")
                        } else if plan.last_member {
                            Some(
                                "You are the last joined member. This room may become inaccessible.",
                            )
                        } else if plan.no_remaining_inviter {
                            Some("No remaining member can invite you back to this room.")
                        } else {
                            None
                        };
                        if let Some(concern) = concern {
                            entries = entries.push(
                                container(
                                    column![
                                        shell::title("Before you leave", 14)
                                            .style(shell::danger_text),
                                        text(concern).size(13),
                                        button(text("Review members & permissions").size(13))
                                            .keyboard_navigation(true)
                                            .padding([7, 10])
                                            .style(shell::quiet_button)
                                            .on_press_maybe(
                                                (!busy).then_some(message(Event::ReviewMembers))
                                            ),
                                    ]
                                    .spacing(8),
                                )
                                .padding(12)
                                .width(Length::Fill)
                                .style(|theme| {
                                    shell::surface(theme, shell::palette(theme).error_surface)
                                }),
                            );
                        }
                        for warning in plan.warnings() {
                            entries = entries.push(text(warning).size(13));
                        }
                    } else if self.membership.leave_load.is_some() {
                        entries = entries.push(
                            text("Checking encryption, permissions, and remaining members…")
                                .size(13),
                        );
                    }
                    entries = entries.push(
                        button(text("Refresh leave details").size(13))
                            .keyboard_navigation(true)
                            .padding([7, 10])
                            .style(shell::quiet_button)
                            .on_press_maybe(
                                (!busy && self.membership.leave_load.is_none())
                                    .then_some(message(Event::ReloadLeave)),
                            ),
                    );
                    "Leave this room?"
                }
            };
            let mut actions = column![].spacing(8).width(Length::Fill);
            if busy {
                actions = actions.push(
                    text("Updating membership…")
                        .size(12)
                        .style(shell::muted_text),
                );
            }
            if let Some(error) = &self.membership.error {
                actions = actions.push(text(error).size(13).style(shell::danger_text));
            }
            let mut buttons = row![
                iced::widget::space().width(Length::Fill),
                button(
                    text(if matches!(self.membership.mode, Mode::Invitations) {
                        "Close"
                    } else {
                        "Cancel"
                    })
                    .size(13)
                )
                .keyboard_navigation(true)
                .padding([7, 14])
                .style(shell::quiet_button)
                .on_press(message(Event::Close)),
            ]
            .spacing(8);
            if !matches!(self.membership.mode, Mode::Invitations) {
                let leaving = matches!(self.membership.mode, Mode::Leave(_));
                buttons = buttons.push(
                    button(text(if leaving { "Leave room" } else { "Join room" }).size(13))
                        .keyboard_navigation(true)
                        .padding([7, 14])
                        .style(if leaving {
                            button::danger
                        } else {
                            shell::primary_button
                        })
                        .on_press_maybe(
                            (!busy
                                && !self.closing
                                && (if leaving {
                                    self.membership.leave_plan.is_some()
                                        && self.membership.leave_load.is_none()
                                } else {
                                    !self.membership.input.trim().is_empty()
                                }))
                            .then_some(message(Event::Submit)),
                        ),
                );
            }
            actions = actions.push(buttons);
            let heading = if matches!(self.membership.mode, Mode::Leave(_)) {
                shell::title(title, 20).style(shell::danger_text)
            } else {
                shell::title(title, 20)
            };
            let header = row![
                heading,
                iced::widget::space().width(Length::Fill),
                button(super::icons::icon(super::icons::Kind::Close))
                    .keyboard_navigation(true)
                    .padding(6)
                    .style(shell::icon_button)
                    .on_press(message(Event::Close)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            let preferred_height = match self.membership.mode {
                Mode::Invitations => {
                    (160.0 + self.model.invitations.len() as f32 * 144.0).clamp(260.0, 620.0)
                }
                Mode::Join => 310.0,
                Mode::Leave(_) => 680.0,
            };
            super::dialog::modal(
                super::dialog::frame(
                    size,
                    560.0,
                    preferred_height,
                    "membership",
                    header,
                    entries,
                    actions,
                ),
                message(Event::Ignore),
            )
        });
        super::focus_scope::dismissible(modal, "membership-focus", Message::Escape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    use interconnect::room_membership::{Address, Invitation};

    fn app() -> App {
        let mut app = demo_app();
        app.model.invitations = vec![Invitation {
            id: "!invite:example.org".into(),
            name: "A new room".into(),
            inviter: Some("@other:example.org".into()),
            space: false,
        }]
        .into();
        app
    }

    #[test]
    fn room_links_open_known_ids_and_aliases_or_prefill_join_without_submitting() {
        for address in ["!demo-general:example.org", "#general:example.org"] {
            let mut app = app();
            let key = app.draft_key(false).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("unsent");
            let _ = app.update(message(Event::Link(Address::parse(address).unwrap())));
            assert_eq!(app.selection.room.as_ref().unwrap().1, "general");
            assert_eq!(app.drafts[&key].content.text().trim_end(), "unsent");
            assert!(!app.membership.open);
            assert_eq!(app.pending_ops, 0);
        }
        let mut app = app();
        let selected = app.selection.room.clone();
        let _ = app.update(message(Event::Link(
            Address::parse("https://matrix.to/#/!unknown:example.org?via=route.example.org")
                .unwrap(),
        )));
        assert!(app.membership.open);
        assert!(app.membership.input.contains("via=route.example.org"));
        assert_eq!(app.selection.room, selected);
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn join_validation_and_cancel_keep_the_original_draft_then_join_opens_the_result() {
        let mut app = app();
        let selected = app.selection.room.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unfinished");
        let _ = app.update(message(Event::Join));
        let _ = app.update(message(Event::Input("not a room".into())));
        let _ = app.update(message(Event::Submit));
        assert!(app.membership.error.is_some());
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Close));
        assert_eq!(app.selection.room, selected);
        let _ = app.update(message(Event::Join));
        let _ = app.update(message(Event::Input("!new:example.org".into())));
        let _ = app.update(message(Event::Submit));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "!new:example.org");
        assert_eq!(app.drafts[&key].content.text().trim_end(), "unfinished");
    }

    #[test]
    fn leave_requires_confirmation_and_preserves_other_conversations() {
        for leave_selected in [false, true] {
            let mut app = app();
            let selected = app.selection.room.clone();
            let key = app.draft_key(false).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("unfinished");
            let id = if leave_selected {
                selected.as_ref().unwrap().1.clone()
            } else {
                "general".into()
            };
            let _ = app.update(message(Event::Leave(id.clone())));
            let _ = app.update(message(Event::Close));
            assert!(app.model.rooms.iter().any(|r| r.id == id));
            let _ = app.update(message(Event::Leave(id.clone())));
            let _ = app.update(message(Event::Submit));
            assert!(!app.model.rooms.iter().any(|r| r.id == id));
            assert_eq!(app.drafts[&key].content.text().trim_end(), "unfinished");
            if leave_selected {
                assert!(app.selection.room.is_none());
                assert!(app.selection.thread.is_none());
            } else {
                assert_eq!(app.selection.room, selected);
            }
            assert_eq!(app.pending_ops, 0);
        }
    }

    #[test]
    fn leave_preview_is_required_and_late_warnings_cannot_replace_a_new_dialog() {
        let mut app = app();
        let id = app.selection.room.as_ref().unwrap().1.clone();
        let _ = app.update(message(Event::Leave(id.clone())));
        let old = app.membership.generation;
        let plan = app.membership.leave_plan.take().unwrap();
        assert!(
            plan.warnings()
                .iter()
                .any(|warning| warning.contains("encrypted"))
        );
        let _ = app.update(message(Event::Submit));
        assert_eq!(app.pending_ops, 0);
        assert!(app.model.rooms.iter().any(|room| room.id == id));
        let _ = app.update(message(Event::ReloadLeave));
        let current = app.membership.generation;
        assert_ne!(old, current);
        let _ = app.update(message(Event::LeavePrepared(
            old,
            app.account_id.clone(),
            id.clone(),
            Err("late preview".into()),
        )));
        assert!(app.membership.error.is_none());
        assert!(app.membership.leave_plan.is_some());
        let _ = app.update(message(Event::Close));
        let _ = app.update(message(Event::Join));
        let _ = app.update(message(Event::LeavePrepared(
            current,
            app.account_id.clone(),
            id,
            Ok(plan),
        )));
        assert!(matches!(app.membership.mode, Mode::Join));
        assert!(app.membership.leave_plan.is_none());
    }
    #[test]
    fn room_settings_leave_opens_the_warning_without_leaving_or_losing_the_draft() {
        let mut app = app();
        let id = app.selection.room.as_ref().unwrap().1.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("keep this");
        let _ = app.update(Message::RoomSettings(crate::room_settings::Event::Open(
            id.clone(),
        )));
        let _ = app.update(Message::RoomSettings(crate::room_settings::Event::Leave));
        assert!(app.membership.open && !app.room_settings.is_open());
        assert!(matches!(&app.membership.mode, Mode::Leave(room) if *room == id));
        assert!(app.membership.leave_plan.is_some());
        assert!(app.model.rooms.iter().any(|room| room.id == id));
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Close));
        assert_eq!(app.drafts[&key].content.text().trim_end(), "keep this");
    }
    #[test]
    fn closing_and_declining_preserve_conversation_drafts_and_reading_position() {
        let mut app = app();
        let room = app.selection.room.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 84.0);
        let _ = app.update(message(Event::Open));
        let _ = app.update(Message::Room("general".into()));
        assert_eq!(app.selection.room, room);
        let _ = app.update(message(Event::Close));
        assert_eq!(app.model.invitations.len(), 1);
        let _ = app.update(message(Event::Open));
        let _ = app.update(message(Event::Respond("!invite:example.org".into(), false)));
        assert!(app.model.invitations.is_empty());
        assert_eq!(app.selection.room, room);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "unsent");
        assert_eq!(app.scroll_positions[&key], 84.0);
        assert_eq!(app.pending_ops, 0);
    }

    #[test]
    fn accepting_opens_rooms_but_space_acceptance_only_changes_the_filter() {
        for space in [false, true] {
            let mut app = app();
            let room = app.selection.room.clone();
            std::sync::Arc::make_mut(&mut app.model.invitations)[0].space = space;
            let _ = app.update(message(Event::Open));
            let _ = app.update(message(Event::Respond("!invite:example.org".into(), true)));
            assert!(!app.membership.open);
            assert!(app.model.invitations.is_empty());
            if space {
                assert_eq!(app.selection.room, room);
                assert_eq!(app.shell.space, Scope::Space("!invite:example.org".into()));
            } else {
                assert_eq!(
                    app.selection.room.as_ref().unwrap().1,
                    "!invite:example.org"
                );
            }
        }
    }

    #[test]
    fn late_results_cannot_switch_a_dismissed_dialog_or_another_account() {
        let mut app = app();
        let room = app.selection.room.clone();
        app.membership.pending = Some((50, 10));
        app.pending_ops = 1;
        let _ = app.update(message(Event::Finished {
            request: 49,
            account: None,
            room: "!invite:example.org".into(),
            result: Ok(None),
        }));
        assert_eq!(app.pending_ops, 1);
        let _ = app.update(message(Event::Finished {
            request: 50,
            account: None,
            room: "!invite:example.org".into(),
            result: Ok(Some(Joined {
                id: "!invite:example.org".into(),
                name: "Accepted".into(),
                space: false,
            })),
        }));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.selection.room, room);
        app.membership.pending = Some((51, 10));
        app.pending_ops = 1;
        app.account_id = Some("@new:example.org".into());
        let _ = app.update(message(Event::Finished {
            request: 51,
            account: None,
            room: "studio".into(),
            result: Ok(None),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.model.rooms.iter().any(|r| r.id == "studio"));
    }

    #[test]
    fn confirmed_writes_survive_an_older_navigation_snapshot() {
        let mut app = app();
        let mut old = app.model.clone();
        let change = Change::Joined(Joined {
            id: "!invite:example.org".into(),
            name: "Accepted".into(),
            space: false,
        });
        app.membership.settling.push(change);
        app.membership.reconcile(&mut old);
        assert!(old.invitations.is_empty());
        assert!(old.rooms.iter().any(|r| r.id == "!invite:example.org"));
        app.membership.reconcile(&mut old);
        assert!(app.membership.settling.is_empty());
    }

    #[test]
    fn invitation_dialog_fits_small_windows_and_has_no_idle_redraw_timer() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = app();
        let _ = app.update(message(Event::Open));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for leave in [false, true] {
            if leave {
                let _ = app.update(message(Event::Close));
                let id = app.selection.room.as_ref().unwrap().1.clone();
                let _ = app.update(message(Event::Leave(id)));
            }
            for size in [
                iced::Size::new(640.0, 480.0),
                iced::Size::new(1240.0, 820.0),
            ] {
                let mut view = app.view();
                let mut tree = widget::Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                assert_eq!(node.size(), size);
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(
                        iced::time::Instant::now(),
                    )),
                    layout::Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                assert!(messages.is_empty());
            }
        }
    }
}
