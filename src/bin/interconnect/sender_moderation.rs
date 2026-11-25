//! Message-sender moderation with captured room/account permissions and explicit writes.
use super::{App, MAX_PENDING_OPERATIONS, Message, reaction_picker::Target, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, row, text, text_input},
};
use interconnect::{
    connected::RowKind,
    room_management::{BanDetails, BannedMember, Management, MemberChange, Sender},
};

#[derive(Clone, Debug)]
pub struct Prepared {
    pub sender: Sender,
    pub control: Option<Management>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Remove,
    Ban,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sending {
    account: Option<String>,
    target: Target,
    user: String,
}
pub struct State {
    target: Target,
    account: Option<String>,
    request: u64,
    action: Action,
    prepared: Prepared,
    reason: String,
    pending: Option<u64>,
    sent: bool,
    error: Option<String>,
}
impl State {
    pub fn dismiss(&self) -> Message {
        message(Event::Cancel(self.request))
    }
    fn change(&self) -> MemberChange {
        match self.action {
            Action::Remove => MemberChange::Remove {
                user: self.prepared.sender.user_id.clone(),
                invited: self.prepared.sender.invited,
            },
            Action::Ban => MemberChange::BanId(self.prepared.sender.user_id.clone()),
        }
    }
    fn allowed(&self) -> bool {
        match self.action {
            Action::Remove => self.prepared.sender.can_remove,
            Action::Ban => self.prepared.sender.can_ban,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(Target, Option<String>, Action, Box<Prepared>),
    Reason(u64, String),
    Confirm(u64),
    Cancel(u64),
    Ignore,
    Finished {
        request: u64,
        operation: u64,
        target: Target,
        account: Option<String>,
        user: String,
        result: Result<(), String>,
    },
}
fn message(event: Event) -> Message {
    Message::SenderModeration(event)
}
impl App {
    pub(super) fn sender_moderation_pending(&self, room: &str, user: &str) -> bool {
        self.inviting.contains(room)
            || self
                .sender_moderation_writes
                .values()
                .any(|s| s.account == self.account_id && s.target.room == room && s.user == user)
    }
    pub(super) fn sender_moderation_eligible(&self, target: &Target, user: &str) -> bool {
        !self.closing
            && !self.stopped
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.space_settings.is_open()
            && !self.room_settings.is_open()
            && (self.demo.is_some() || self.model.authentication.is_none())
            && (target.thread
                || !self
                    .window_width
                    .is_some_and(|w| self.side_panel_replaces_room(w)))
            && self.message_row(target).is_some_and(|row| {
                row.kind == RowKind::Message
                    && !row.is_own
                    && row.sender == user
                    && Some(user) != self.account_id.as_deref()
            })
    }
    pub(super) fn demo_sender_moderation(&self, target: &Target, user: &str) -> Option<Prepared> {
        let demo = self.demo.as_ref()?;
        let room = self.model.rooms.iter().find(|r| r.id == target.room)?;
        let members = demo.members(0, &target.room, room.direct);
        let member = members.rows.iter().find(|m| m.user_id == user);
        let protected = member.is_some_and(|m| m.role == "Admin") || user == "@you:example.org";
        let banned = demo
            .banned_members
            .get(&target.room)
            .is_some_and(|rows| rows.iter().any(|m| m.user_id == user));
        Some(Prepared {
            sender: Sender {
                user_id: user.into(),
                name: member.map_or_else(|| user.into(), |m| m.name.clone()),
                invited: member.is_some_and(|m| m.invited),
                can_remove: room.can_invite && !protected && member.is_some(),
                can_ban: room.can_invite && !protected && !banned,
            },
            control: None,
        })
    }
    pub(super) fn validate_sender_moderation(&mut self) {
        if self.sender_moderation.as_ref().is_some_and(|s| {
            s.account != self.account_id
                || !self.sender_moderation_eligible(&s.target, &s.prepared.sender.user_id)
        }) {
            self.sender_moderation = None;
        }
    }
    fn sender_moderation_success(&mut self, target: &Target, change: &MemberChange, reason: &str) {
        let user = match change {
            MemberChange::Remove { user, .. } | MemberChange::BanId(user) => user,
            _ => return,
        };
        if let Some(demo) = &mut self.demo {
            let direct = self
                .model
                .rooms
                .iter()
                .any(|r| r.id == target.room && r.direct);
            let members = demo.members(0, &target.room, direct);
            demo.managed_members.insert(
                target.room.clone(),
                members
                    .rows
                    .iter()
                    .filter(|m| m.user_id != *user)
                    .cloned()
                    .collect(),
            );
            if let Some(rows) = demo.invitations.get_mut(&target.room) {
                rows.retain(|m| m.user_id != *user);
            }
            if matches!(change, MemberChange::BanId(_)) {
                let banned = demo.banned_members.entry(target.room.clone()).or_default();
                if !banned.iter().any(|m| m.user_id == *user) {
                    banned.push(BannedMember {
                        user_id: user.clone(),
                        name: members
                            .rows
                            .iter()
                            .find(|m| m.user_id == *user)
                            .map_or_else(|| user.clone(), |m| m.name.clone()),
                        can_unban: true,
                        details: Some(BanDetails {
                            actor: Some("@you:example.org".into()),
                            timestamp_ms: Some(chrono::Utc::now().timestamp_millis()),
                            reason: interconnect::moderation::reason(reason).ok().flatten(),
                        }),
                    });
                }
            }
            self.refresh_demo();
        } else if self
            .selection
            .room
            .as_ref()
            .is_some_and(|(_, room)| room == &target.room)
            && let Some(snapshot) = &mut self.model.members
            && Some(snapshot.generation) == self.selection.members
        {
            crate::space_settings::member_management::apply_snapshot(snapshot, change);
        }
    }
    pub(super) fn sender_moderation_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open(target, account, action, prepared) => {
                let prepared = if self.demo.is_some() {
                    let Some(current) =
                        self.demo_sender_moderation(&target, &prepared.sender.user_id)
                    else {
                        return Task::none();
                    };
                    Box::new(current)
                } else {
                    prepared
                };
                if account != self.account_id
                    || !self.context_menu_allowed()
                    || self.composing
                    || !self.sender_moderation_eligible(&target, &prepared.sender.user_id)
                    || self.sender_moderation_pending(&target.room, &prepared.sender.user_id)
                    || !(match action {
                        Action::Remove => prepared.sender.can_remove,
                        Action::Ban => prepared.sender.can_ban,
                    })
                    || (self.demo.is_none() && prepared.control.is_none())
                {
                    return Task::none();
                }
                self.generation += 1;
                self.sender_moderation = Some(State {
                    target,
                    account,
                    request: self.generation,
                    action,
                    prepared: *prepared,
                    reason: String::new(),
                    pending: None,
                    sent: false,
                    error: None,
                });
                self.context_menu = None;
                self.pending_room_focus = None;
                self.pending_thread_focus = None;
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.stop_typing();
                return iced::widget::operation::focus("sender-moderation-reason");
            }
            Event::Reason(request, reason) => {
                if let Some(state) = &mut self.sender_moderation
                    && state.request == request
                    && state.pending.is_none()
                    && !state.sent
                {
                    state.reason = reason;
                    state.error = None;
                }
            }
            Event::Cancel(request) => {
                if let Some(state) = &self.sender_moderation
                    && state.request == request
                {
                    let thread = state.target.thread;
                    self.sender_moderation = None;
                    return iced::widget::operation::focus(if thread {
                        "thread-composer"
                    } else {
                        "room-composer"
                    });
                }
            }
            Event::Confirm(request) => {
                let Some(state) = self.sender_moderation.as_ref().filter(|s| {
                    s.request == request
                        && s.account == self.account_id
                        && s.pending.is_none()
                        && !s.sent
                        && s.allowed()
                        && self.sender_moderation_eligible(&s.target, &s.prepared.sender.user_id)
                }) else {
                    return Task::none();
                };
                if self.composing
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self.sender_moderation_pending(
                        &state.target.room,
                        &state.prepared.sender.user_id,
                    )
                {
                    return Task::none();
                }
                if let Err(error) = interconnect::moderation::reason(&state.reason) {
                    self.sender_moderation.as_mut().unwrap().error = Some(error);
                    return Task::none();
                }
                if self.demo.is_some()
                    && !self
                        .demo_sender_moderation(&state.target, &state.prepared.sender.user_id)
                        .is_some_and(|current| match state.action {
                            Action::Remove => current.sender.can_remove,
                            Action::Ban => current.sender.can_ban,
                        })
                {
                    self.sender_moderation.as_mut().unwrap().error = Some("Moderation permissions or membership changed. Close and reopen the message menu.".into());
                    return Task::none();
                }
                let target = state.target.clone();
                let account = state.account.clone();
                let reason = state.reason.clone();
                let change = state.change();
                if self.demo.is_some() {
                    self.sender_moderation_success(&target, &change, &reason);
                    self.sender_moderation.as_mut().unwrap().sent = true;
                    return Task::none();
                }
                let Some(control) = state.prepared.control.clone() else {
                    return Task::none();
                };
                let user = state.prepared.sender.user_id.clone();
                self.generation += 1;
                let operation = self.generation;
                self.sender_moderation_writes.insert(
                    operation,
                    Sending {
                        account: account.clone(),
                        target: target.clone(),
                        user: user.clone(),
                    },
                );
                self.pending_ops += 1;
                self.inviting.insert(target.room.clone());
                let state = self.sender_moderation.as_mut().unwrap();
                state.pending = Some(operation);
                state.error = None;
                return Task::perform(
                    control.change_member_with_reason(change, reason),
                    move |result| {
                        message(Event::Finished {
                            request,
                            operation,
                            target: target.clone(),
                            account: account.clone(),
                            user: user.clone(),
                            result,
                        })
                    },
                );
            }
            Event::Finished {
                request,
                operation,
                target,
                account,
                user,
                result,
            } => {
                if self.sender_moderation_writes.get(&operation)
                    != Some(&Sending {
                        account: account.clone(),
                        target: target.clone(),
                        user,
                    })
                {
                    return Task::none();
                }
                self.sender_moderation_writes.remove(&operation);
                self.inviting.remove(&target.room);
                self.pending_ops = self.pending_ops.saturating_sub(1);
                if account != self.account_id {
                    return Task::none();
                }
                if let Some(state) = self.sender_moderation.as_ref().filter(|s| {
                    s.request == request && s.target == target && s.pending == Some(operation)
                }) {
                    if result.is_ok() {
                        let change = state.change();
                        let reason = state.reason.clone();
                        self.sender_moderation_success(&target, &change, &reason);
                    }
                    let state = self.sender_moderation.as_mut().unwrap();
                    state.pending = None;
                    match result {
                        Ok(()) => state.sent = true,
                        Err(error) => state.error = Some(error),
                    }
                }
            }
            Event::Ignore => {}
        }
        Task::none()
    }
    pub(super) fn sender_moderation_dialog(&self) -> Element<'_, Message> {
        let state = self.sender_moderation.as_ref().unwrap();
        let request = state.request;
        let busy = state.pending.is_some();
        let banning = state.action == Action::Ban;
        let revoking = !banning && state.prepared.sender.invited;
        let modal = iced::widget::responsive(move |size| {
            let room = self
                .model
                .rooms
                .iter()
                .find(|r| r.id == state.target.room)
                .map_or("Room", |r| r.name.as_str());
            let mut body = column![
                text(format!("{} · {}", room, state.prepared.sender.name)).size(14),
                text(&state.prepared.sender.user_id).size(12).style(shell::muted_text),
                text(if banning { "They will be removed and unable to join or be invited until unbanned. This applies only to this room." } else if revoking { "Revoke this person's pending invitation to this room." } else { "Remove this person from this room. They may rejoin if its access rules allow it." }).size(13),
            ].spacing(12);
            if state.sent {
                body = body.push(
                    text(if self.demo.is_some() {
                        "Moderation applied locally (demo)."
                    } else {
                        "Moderation confirmed by the server."
                    })
                    .size(14),
                );
            } else {
                body = body.push(
                    text_input("Reason (optional, visible in room history)", &state.reason)
                        .id("sender-moderation-reason")
                        .cursor_blinking(false)
                        .padding(10)
                        .on_input_maybe(
                            (!busy)
                                .then_some(move |reason| message(Event::Reason(request, reason))),
                        ),
                );
            }
            if state.error.is_none()
                && let Err(error) = interconnect::moderation::reason(&state.reason)
            {
                body = body.push(text(error).size(13).style(shell::danger_text));
            }
            if let Some(error) = &state.error {
                body = body.push(text(error).size(13).style(shell::danger_text));
            }
            let mut actions = row![
                iced::widget::space().width(Length::Fill),
                button(text("Close").size(13))
                    .padding([7, 12])
                    .style(shell::quiet_button)
                    .keyboard_navigation(true)
                    .on_press(message(Event::Cancel(request)))
            ]
            .spacing(8);
            if !state.sent {
                actions = actions.push(
                    button(
                        text(if busy {
                            "Applying…"
                        } else if banning {
                            "Ban sender"
                        } else {
                            "Remove sender"
                        })
                        .size(13),
                    )
                    .padding([7, 12])
                    .style(button::danger)
                    .keyboard_navigation(true)
                    .on_press_maybe(
                        (!busy
                            && !self.composing
                            && self.pending_ops < MAX_PENDING_OPERATIONS
                            && interconnect::moderation::reason(&state.reason).is_ok())
                        .then_some(message(Event::Confirm(request))),
                    ),
                );
            }
            super::dialog::modal(
                super::dialog::frame(
                    size,
                    540.0,
                    390.0,
                    "sender-moderation",
                    shell::title(
                        if banning {
                            "Ban sender?"
                        } else {
                            "Remove sender?"
                        },
                        20,
                    ),
                    body,
                    actions,
                ),
                message(Event::Ignore),
            )
        });
        super::focus_scope::dismissible(
            modal,
            "sender-moderation-focus",
            message(Event::Cancel(request)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    use iced::widget::text_editor;

    fn target(app: &App, thread: bool) -> Target {
        Target {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            thread,
            generation: if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            },
            event: if thread { "review-reply-1" } else { "welcome" }.into(),
        }
    }
    fn open(app: &mut App, target: &Target, action: Action) -> u64 {
        let user = app.message_row(target).unwrap().sender.clone();
        let prepared = app.demo_sender_moderation(target, &user).unwrap();
        let _ = app.update(message(Event::Open(
            target.clone(),
            app.account_id.clone(),
            action,
            Box::new(prepared),
        )));
        app.sender_moderation.as_ref().unwrap().request
    }
    fn arm(app: &mut App) -> Event {
        app.generation += 1;
        let operation = app.generation;
        let state = app.sender_moderation.as_mut().unwrap();
        state.pending = Some(operation);
        let sending = Sending {
            account: state.account.clone(),
            target: state.target.clone(),
            user: state.prepared.sender.user_id.clone(),
        };
        app.inviting.insert(sending.target.room.clone());
        app.sender_moderation_writes
            .insert(operation, sending.clone());
        app.pending_ops += 1;
        Event::Finished {
            request: state.request,
            operation,
            account: sending.account,
            target: sending.target,
            user: sending.user,
            result: Ok(()),
        }
    }
    #[test]
    fn sender_moderation_preserves_room_thread_drafts_scroll_and_space_scope() {
        for thread in [false, true] {
            for action in [Action::Remove, Action::Ban] {
                let mut app = demo_app();
                let _ = app.update(Message::WindowFocused(true));
                let _ = app.update(Message::Thread("review".into()));
                let room = app.selection.room.clone();
                let scope = app.shell.space.clone();
                let key = app.draft_key(thread).unwrap();
                app.drafts.get_mut(&key).unwrap().content =
                    text_editor::Content::with_text("unfinished");
                app.scroll_positions.insert(key.clone(), 125.0);
                let target = target(&app, thread);
                let original = app.message_row(&target).unwrap().body.clone();
                let user = app.message_row(&target).unwrap().sender.clone();
                let request = open(&mut app, &target, action);
                assert!(!app.selection.conversation_visible);
                let _ = app.update(message(Event::Reason(request, "Spam".into())));
                let _ = app.update(Message::Edit(
                    key.clone(),
                    text_editor::Action::Edit(text_editor::Edit::Insert('x')),
                ));
                let _ = app.update(Message::Send(key.clone()));
                let _ = app.update(Message::Escape);
                let _ = app.update(message(Event::Confirm(request)));
                assert!(
                    app.demo
                        .as_ref()
                        .unwrap()
                        .members(0, &target.room, false)
                        .rows
                        .iter()
                        .any(|m| m.user_id == user)
                );
                let request = open(&mut app, &target, action);
                let _ = app.update(message(Event::Reason(request, "Spam".into())));
                let _ = app.update(message(Event::Confirm(request)));
                assert!(app.sender_moderation.as_ref().unwrap().sent);
                assert!(
                    !app.demo
                        .as_ref()
                        .unwrap()
                        .members(0, &target.room, false)
                        .rows
                        .iter()
                        .any(|m| m.user_id == user)
                );
                assert_eq!(
                    app.demo
                        .as_ref()
                        .unwrap()
                        .banned_members
                        .get(&target.room)
                        .is_some_and(|rows| rows.iter().any(|m| m.user_id == user)),
                    action == Action::Ban
                );
                assert_eq!(app.message_row(&target).unwrap().body, original);
                let _ = app.update(Message::Escape);
                assert!(app.selection.conversation_visible);
                assert_eq!(app.selection.room, room);
                assert_eq!(app.shell.space, scope);
                assert_eq!(app.drafts[&key].content.text(), "unfinished");
                assert_eq!(app.scroll_positions[&key], 125.0);
                assert_eq!(app.pending_ops, 0);
            }
        }
    }
    #[test]
    fn space_filtering_keeps_the_source_room_and_sender_of_an_open_confirmation() {
        let mut app = demo_app();
        let target = target(&app, false);
        let selected = app.selection.room.clone();
        let request = open(&mut app, &target, Action::Ban);
        let _ = app.update(Message::Space(interconnect::navigation::Scope::Space(
            "home-space".into(),
        )));
        assert_eq!(app.selection.room, selected);
        assert_eq!(app.sender_moderation.as_ref().unwrap().target, target);
        let _ = app.update(message(Event::Confirm(request)));
        assert!(
            app.demo
                .as_ref()
                .unwrap()
                .banned_members
                .contains_key(&target.room)
        );
        assert!(
            !app.demo
                .as_ref()
                .unwrap()
                .banned_members
                .contains_key("home-space")
        );
    }
    #[test]
    fn permission_loss_invalid_reasons_and_old_completions_cannot_change_new_dialogs() {
        let mut app = demo_app();
        let target = target(&app, false);
        let request = open(&mut app, &target, Action::Ban);
        let _ = app.update(message(Event::Reason(
            request,
            "x".repeat(interconnect::moderation::MAX_REASON_BYTES + 1),
        )));
        let _ = app.update(message(Event::Confirm(request)));
        assert!(app.sender_moderation.as_ref().unwrap().error.is_some());
        let _ = app.update(message(Event::Reason(request, "Spam".into())));
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|r| r.id == target.room)
            .unwrap()
            .can_invite = false;
        let _ = app.update(message(Event::Confirm(request)));
        assert!(!app.sender_moderation.as_ref().unwrap().sent);
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|r| r.id == target.room)
            .unwrap()
            .can_invite = true;
        let mut failed = arm(&mut app);
        if let Event::Finished { result, .. } = &mut failed {
            *result = Err("fixture refusal".into());
        }
        let _ = app.update(message(failed));
        assert_eq!(app.sender_moderation.as_ref().unwrap().reason, "Spam");
        let old = arm(&mut app);
        let _ = app.update(Message::Escape);
        let mut next = target.clone();
        next.event = "details".into();
        // An admitted write blocks the same sender through another message.
        let prepared = app
            .demo_sender_moderation(&next, "@maya:example.org")
            .unwrap();
        let _ = app.update(message(Event::Open(
            next.clone(),
            app.account_id.clone(),
            Action::Ban,
            Box::new(prepared),
        )));
        assert!(app.sender_moderation.is_none());
        let _ = app.update(message(old.clone()));
        let _ = app.update(message(old));
        assert_eq!(app.pending_ops, 0);
        let current = open(&mut app, &next, Action::Ban);
        let old = arm(&mut app);
        app.account_id = Some("@another:example.org".into());
        let _ = app.update(message(old));
        assert!(app.sender_moderation.is_none());
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Confirm(current)));
        assert!(app.sender_moderation_writes.is_empty());
    }
    #[test]
    fn a_late_completion_leaves_another_rooms_confirmation_and_admission_intact() {
        let mut app = demo_app();
        let original = target(&app, false);
        open(&mut app, &original, Action::Ban);
        let completed = arm(&mut app);
        let _ = app.update(Message::Escape);
        let _ = app.update(Message::Room("general".into()));
        let current = Target {
            room: "general".into(),
            thread: false,
            generation: app.selection.room.as_ref().unwrap().0,
            event: "hello".into(),
        };
        let request = open(&mut app, &current, Action::Ban);
        let newer = arm(&mut app);
        let _ = app.update(message(completed.clone()));
        let _ = app.update(message(completed));
        assert_eq!(app.sender_moderation.as_ref().unwrap().request, request);
        assert!(!app.sender_moderation.as_ref().unwrap().sent);
        assert_eq!(app.pending_ops, 1);
        assert!(!app.inviting.contains("studio"));
        assert!(app.inviting.contains("general"));
        let _ = app.update(message(newer));
        assert_eq!(app.pending_ops, 0);
        assert!(!app.inviting.contains("general"));
    }
    #[test]
    fn admitted_sender_writes_release_shutdown_and_navigation_invalidates_prompts() {
        let mut app = demo_app();
        let original = target(&app, false);
        let request = open(&mut app, &original, Action::Remove);
        let _ = app.update(Message::Room("general".into()));
        assert!(app.sender_moderation.is_none());
        let _ = app.update(message(Event::Confirm(request)));
        let _ = app.update(Message::Room(original.room.clone()));
        let current = target(&app, false);
        open(&mut app, &current, Action::Ban);
        let completed = arm(&mut app);
        app.demo = None;
        let _ = app.update(Message::Close);
        assert!(!app.selection.shutdown);
        let _ = app.update(message(completed));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
    }
    #[test]
    fn sender_dialog_is_bounded_and_escape_dismisses_without_idle_redraws() {
        use iced::advanced::{Shell, layout, mouse, widget};
        use iced::keyboard::{
            Key, Modifiers,
            key::{Named, NativeCode, Physical},
        };
        let mut app = demo_app();
        let target = target(&app, false);
        let request = open(&mut app, &target, Action::Ban);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for size in [
            iced::Size::new(640.0, 480.0),
            iced::Size::new(1240.0, 820.0),
        ] {
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
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
            let key = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: Key::Named(Named::Escape),
                modified_key: Key::Named(Named::Escape),
                physical_key: Physical::Unidentified(NativeCode::Unidentified),
                location: iced::keyboard::Location::Standard,
                modifiers: Modifiers::empty(),
                text: None,
                repeat: false,
            });
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &key,
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert!(shell.is_event_captured());
            assert!(
                matches!(&messages[..], [Message::SenderModeration(Event::Cancel(current))] if *current == request)
            );
        }
    }
}
