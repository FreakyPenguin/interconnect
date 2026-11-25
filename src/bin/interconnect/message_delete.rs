//! Confirmed, account-scoped redactions. The popup never changes a draft.
use super::{App, MAX_PENDING_OPERATIONS, Message, reaction_picker::Target, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, text, text_input},
};
use interconnect::connected::{MessageRow, RowKind};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sending {
    account: Option<String>,
    room: String,
    event: String,
}
pub struct State {
    target: Target,
    account: Option<String>,
    request: u64,
    transaction: matrix_sdk::ruma::OwnedTransactionId,
    pending: bool,
    attempted: bool,
    reason: String,
    error: Option<String>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(Target),
    Reason(u64, String),
    Ignore,
    Confirm(u64),
    Cancel(u64),
    Finished {
        target: Target,
        account: Option<String>,
        request: u64,
        result: Result<(), String>,
    },
}
impl State {
    pub fn dismiss(&self) -> Message {
        message(Event::Cancel(self.request))
    }
}
fn confirm_key(event: iced::Event, status: iced::event::Status, _: iced::window::Id) -> Option<()> {
    (status != iced::event::Status::Captured
        && matches!(event, iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        modifiers, repeat: false, ..
    }) if modifiers.is_empty()))
    .then_some(())
}
fn message(event: Event) -> Message {
    Message::Deletion(event)
}
fn sending(account: Option<String>, target: &Target) -> Sending {
    Sending {
        account,
        room: target.room.clone(),
        event: target.event.clone(),
    }
}
impl App {
    fn deleted_message_cleanup(&mut self, room: &str, event: &str) {
        self.restore_deleted_drafts(room, event);
        self.global_search.message_deleted(room, event);
        self.bookmarks.message_deleted(room, event);
    }

    pub(super) fn delete_subscription(&self) -> iced::Subscription<Message> {
        if let Some(state) = &self.message_delete
            && !state.pending
            && !self.composing
        {
            iced::event::listen_with(confirm_key)
                .with(state.request)
                .map(|(request, ())| message(Event::Confirm(request)))
        } else {
            iced::Subscription::none()
        }
    }

    fn delete_eligible(&self, target: &Target) -> bool {
        !self.closing
            && !self.stopped
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.space_settings.is_open()
            && (self.demo.is_some() || self.model.authentication.is_none())
            && (target.thread
                || !self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width)))
            && (self.demo.is_some() || self.reaction_composer(target).is_some())
            && self.message_row(target).is_some_and(|row| {
                row.kind == RowKind::Message
                    && row.redactable
                    && self.model.rooms.iter().any(|room| {
                        room.id == target.room
                            && if row.is_own {
                                room.can_redact_own
                            } else {
                                room.can_redact_other
                            }
                    })
            })
    }
    pub(super) fn delete_action(
        &self,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Message> {
        let target = Target {
            room: self.selection.room.as_ref()?.1.clone(),
            thread,
            generation,
            event: row.event_id.clone()?,
        };
        (self.context_menu_allowed()
            && !self.composing
            && self.emoji_management.is_none()
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.space_settings.is_open()
            && self.delete_eligible(&target)
            && !self
                .deleting
                .contains(&sending(self.account_id.clone(), &target)))
        .then(|| message(Event::Open(target)))
    }
    pub(super) fn validate_message_delete(&mut self) {
        if self.message_delete.as_ref().is_some_and(|state| {
            state.account != self.account_id || !self.delete_eligible(&state.target)
        }) {
            self.message_delete = None;
        }
    }
    pub(super) fn delete_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open(target) => {
                if !self.message_row(&target).is_some_and(|row| {
                    self.delete_action(row, target.thread, target.generation)
                        .is_some()
                }) {
                    return Task::none();
                }
                self.generation += 1;
                self.message_delete = Some(State {
                    target,
                    account: self.account_id.clone(),
                    request: self.generation,
                    transaction: matrix_sdk::ruma::TransactionId::new(),
                    pending: false,
                    attempted: false,
                    reason: String::new(),
                    error: None,
                });
                self.context_menu = None;
                self.pending_room_focus = None;
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.stop_typing();
                // Defocus the underlying composer while the modal popup owns input.
                return iced::widget::operation::focus("delete-confirmation");
            }
            Event::Cancel(request) => {
                if let Some(state) = &self.message_delete
                    && state.request == request
                {
                    let thread = state.target.thread;
                    self.message_delete = None;
                    return iced::widget::operation::focus(if thread {
                        "thread-composer"
                    } else {
                        "room-composer"
                    });
                }
            }
            Event::Reason(request, reason) => {
                if let Some(state) = &mut self.message_delete
                    && state.request == request
                    && !state.pending
                    && !state.attempted
                {
                    state.reason = reason;
                    state.error = None;
                }
            }
            Event::Ignore => {}
            Event::Confirm(request) => {
                let Some(state) = self.message_delete.as_ref().filter(|state| {
                    state.request == request
                        && !state.pending
                        && state.account == self.account_id
                        && self.delete_eligible(&state.target)
                }) else {
                    return Task::none();
                };
                if self.pending_ops >= MAX_PENDING_OPERATIONS || self.composing {
                    return Task::none();
                }
                if let Err(error) = interconnect::moderation::reason(&state.reason) {
                    self.message_delete.as_mut().unwrap().error = Some(error);
                    return Task::none();
                }
                let reason = state.reason.clone();
                let target = state.target.clone();
                if self.demo.is_some() {
                    self.demo
                        .as_mut()
                        .unwrap()
                        .delete_message(&target.room, &target.event);
                    self.message_delete = None;
                    self.deleted_message_cleanup(&target.room, &target.event);
                    self.refresh_demo();
                    return Task::none();
                }
                let Some(composer) = self.reaction_composer(&target) else {
                    return Task::none();
                };
                let transaction = state.transaction.clone();
                let account = state.account.clone();
                if !self.deleting.insert(sending(account.clone(), &target)) {
                    return Task::none();
                }
                let state = self.message_delete.as_mut().unwrap();
                state.pending = true;
                state.attempted = true;
                state.error = None;
                self.pending_ops += 1;
                let event = target.event.clone();
                return Task::perform(
                    composer.delete_message_with_reason(event, transaction, reason),
                    move |result| {
                        message(Event::Finished {
                            target: target.clone(),
                            account: account.clone(),
                            request,
                            result,
                        })
                    },
                );
            }
            Event::Finished {
                target,
                account,
                request,
                result,
            } => {
                if !self.deleting.remove(&sending(account.clone(), &target)) {
                    return Task::none();
                }
                self.pending_ops -= 1;
                if account != self.account_id {
                    return Task::none();
                }
                if result.is_ok() {
                    self.deleted_message_cleanup(&target.room, &target.event);
                }
                if let Some(state) = &mut self.message_delete
                    && state.request == request
                    && state.target == target
                    && state.pending
                {
                    match result {
                        Ok(()) => self.message_delete = None,
                        Err(error) => {
                            state.pending = false;
                            state.error = Some(format!("Could not delete message: {error}"));
                        }
                    }
                }
            }
        }
        Task::none()
    }
    pub(super) fn delete_popup(
        &self,
        thread: bool,
        generation: u64,
        event: &str,
    ) -> Option<(Element<'_, Message>, Message)> {
        let state = self.message_delete.as_ref().filter(|state| {
            state.target.thread == thread
                && state.target.generation == generation
                && state.target.event == event
        })?;
        let request = state.request;
        let mut confirm = button(
            text(if state.pending {
                "Deleting…"
            } else {
                "Delete message"
            })
            .size(12),
        )
        .padding([6, 10])
        .style(button::danger);
        if !state.pending
            && self.pending_ops < MAX_PENDING_OPERATIONS
            && self.delete_eligible(&state.target)
            && !self.composing
            && interconnect::moderation::reason(&state.reason).is_ok()
        {
            confirm = confirm.on_press(message(Event::Confirm(request)));
        }
        let mut content = column![
            shell::title("Delete this message?", 15),
            text("Delete it for everyone in the room. This cannot be undone.")
                .size(12)
                .style(shell::muted_text),
        ]
        .spacing(10);
        let mut reason = text_input("Reason (optional)", &state.reason)
            .cursor_blinking(false)
            .padding(8);
        if !state.pending && !state.attempted {
            reason = reason
                .on_input(move |value| message(Event::Reason(request, value)))
                .on_submit(message(Event::Ignore));
        }
        content = content.push(reason);
        content = content.push(
            text("The reason may be visible in room history.")
                .size(11)
                .style(shell::muted_text),
        );
        if state.error.is_none()
            && let Err(error) = interconnect::moderation::reason(&state.reason)
        {
            content = content.push(text(error).size(12).style(shell::danger_text));
        }
        if state.attempted {
            content = content.push(
                text("The reason stays fixed when retrying this deletion.")
                    .size(11)
                    .style(shell::muted_text),
            );
        }
        if let Some(error) = &state.error {
            content = content.push(text(error).size(12).style(shell::danger_text));
        }
        content = content.push(
            row![
                iced::widget::space().width(Length::Fill),
                container(super::tooltips::icon_button(
                    super::icons::Kind::Close,
                    if state.pending {
                        "Close deletion progress"
                    } else {
                        "Cancel message deletion"
                    },
                    Some(message(Event::Cancel(request))),
                ))
                .id("cancel-message-delete"),
                container(confirm).id("confirm-message-delete"),
            ]
            .spacing(8),
        );
        Some((
            container(content)
                .id("delete-confirmation")
                .width(300)
                .padding(16)
                .style(|theme| iced::widget::container::Style {
                    background: Some(shell::palette(theme).background.into()),
                    border: iced::Border {
                        color: shell::palette(theme).line,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                })
                .into(),
            message(Event::Cancel(request)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{message_edit, tests::demo_app};
    use iced::widget::text_editor;
    use std::sync::Arc;

    fn make_target(app: &App, thread: bool, event: &str) -> Target {
        Target {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            thread,
            generation: if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            },
            event: event.into(),
        }
    }
    fn open(app: &mut App, target: &Target) -> u64 {
        let _ = app.update(message(Event::Open(target.clone())));
        app.message_delete.as_ref().unwrap().request
    }
    fn arm(app: &mut App) -> (Target, Option<String>, u64) {
        let state = app.message_delete.as_mut().unwrap();
        state.pending = true;
        let tuple = (state.target.clone(), state.account.clone(), state.request);
        assert!(app.deleting.insert(sending(tuple.1.clone(), &tuple.0)));
        app.pending_ops += 1;
        tuple
    }
    fn finish(app: &mut App, operation: (Target, Option<String>, u64), result: Result<(), String>) {
        let _ = app.update(message(Event::Finished {
            target: operation.0,
            account: operation.1,
            request: operation.2,
            result,
        }));
    }
    #[test]
    fn confirmation_cancel_and_deletion_preserve_drafts_scroll_and_thread_identity() {
        let mut app = demo_app();
        let _ = app.update(Message::WindowFocused(true));
        let _ = app.update(Message::Thread("review".into()));
        let room = app.draft_key(false).unwrap();
        let thread = app.draft_key(true).unwrap();
        app.drafts.get_mut(&room).unwrap().content = text_editor::Content::with_text("room draft");
        app.drafts.get_mut(&thread).unwrap().content =
            text_editor::Content::with_text("reply draft");
        app.scroll_positions.insert(room.clone(), 120.0);
        app.scroll_positions.insert(thread.clone(), 45.0);
        let scroll = app.scroll_positions.clone();
        let target = make_target(&app, true, "review-reply-2");
        let original = app.message_row(&target).unwrap().clone();
        let request = open(&mut app, &target);
        assert!(!app.selection.conversation_visible);
        assert_eq!(
            app.message_row(&target).unwrap().body,
            original.body,
            "opening never deletes"
        );
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert!(app.settings.draft.is_none());
        let _ = app.update(Message::Edit(
            room.clone(),
            text_editor::Action::Edit(text_editor::Edit::Insert('x')),
        ));
        let _ = app.update(Message::Escape);
        assert!(app.message_delete.is_none());
        assert!(app.selection.conversation_visible);
        let _ = app.update(message(Event::Confirm(request)));
        assert_eq!(app.message_row(&target).unwrap().body, original.body);
        let request = open(&mut app, &target);
        let _ = app.update(message(Event::Confirm(request)));
        let row = app.message_row(&target).unwrap();
        assert_eq!(row.body, "Message removed");
        assert_eq!(row.timestamp_ms, original.timestamp_ms);
        assert_eq!(row.thread_root, original.thread_root);
        assert!(!row.redactable && !row.can_react && row.edit_source.is_none());
        assert!(
            app.model
                .room
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.key == target.event && !row.redactable)
        );
        assert_eq!(app.drafts[&room].content.text(), "room draft");
        assert_eq!(app.drafts[&thread].content.text(), "reply draft");
        assert_eq!(app.scroll_positions, scroll);
        assert!(app.message_delete.is_none());
    }
    #[test]
    fn permissions_stale_confirmations_and_redacted_targets_are_revalidated() {
        let mut app = demo_app();
        let own = make_target(&app, false, "proposal");
        let other = make_target(&app, false, "review");
        assert!(
            app.delete_action(app.message_row(&own).unwrap(), false, own.generation)
                .is_some()
        );
        assert!(
            app.delete_action(app.message_row(&other).unwrap(), false, other.generation)
                .is_none()
        );
        Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == own.room)
            .unwrap()
            .can_redact_other = true;
        let request = open(&mut app, &other);
        let mut model = app.model.clone();
        Arc::make_mut(&mut model.rooms)
            .iter_mut()
            .find(|room| room.id == own.room)
            .unwrap()
            .can_redact_other = false;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.message_delete.is_none());
        let _ = app.update(message(Event::Confirm(request)));
        assert!(app.message_row(&other).unwrap().redactable);
        let request = open(&mut app, &own);
        let _ = app.update(Message::Room("general".into()));
        assert!(app.message_delete.is_none());
        let _ = app.update(message(Event::Confirm(request)));
        let _ = app.update(Message::Room(own.room.clone()));
        assert!(
            app.message_row(&make_target(&app, false, &own.event))
                .unwrap()
                .redactable
        );
        let own = make_target(&app, false, &own.event);
        let request = open(&mut app, &own);
        app.pending_ops = MAX_PENDING_OPERATIONS;
        let _ = app.update(message(Event::Confirm(request)));
        assert!(app.message_row(&own).unwrap().redactable);
        app.pending_ops = 0;
        let mut model = app.model.clone();
        Arc::make_mut(&mut model.room.as_mut().unwrap().rows)
            .iter_mut()
            .find(|row| row.key == own.event)
            .unwrap()
            .redactable = false;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.message_delete.is_none());
        let _ = app.update(message(Event::Open(own)));
        assert!(app.message_delete.is_none());
    }
    #[test]
    fn deleting_an_editing_original_restores_both_composers_and_releases_media() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("proposal".into()));
        let room = app.draft_key(false).unwrap();
        let thread = app.draft_key(true).unwrap();
        for key in [&room, &thread] {
            app.drafts.get_mut(key).unwrap().content =
                text_editor::Content::with_text("keep draft");
        }
        for is_thread in [false, true] {
            let target = make_target(&app, is_thread, "proposal");
            let _ = app.update(Message::Editing(message_edit::Event::Start(target)));
        }
        let target = make_target(&app, true, "proposal");
        let request = open(&mut app, &target);
        let _ = app.update(message(Event::Confirm(request)));
        for key in [&room, &thread] {
            assert!(app.drafts[key].editing.is_none());
            assert_eq!(app.drafts[key].content.text(), "keep draft");
        }
        assert!(!app.thread_original().unwrap().redactable);
        let _ = app.update(Message::Room("images".into()));
        let media = make_target(&app, false, "sample-gif");
        assert!(app.message_row(&media).unwrap().image.is_some());
        let request = open(&mut app, &media);
        let _ = app.update(message(Event::Confirm(request)));
        let row = app.message_row(&media).unwrap();
        assert!(row.image.is_none() && row.download.is_none() && row.formatted.is_none());
        assert!(row.reactions.is_empty());
    }
    #[test]
    fn deletion_reason_is_validated_preserved_and_fixed_after_an_attempt() {
        let mut app = demo_app();
        let target = make_target(&app, false, "proposal");
        let request = open(&mut app, &target);
        let _ = app.update(message(Event::Reason(
            request,
            "x".repeat(interconnect::moderation::MAX_REASON_BYTES + 1),
        )));
        let _ = app.update(message(Event::Confirm(request)));
        assert!(app.message_delete.as_ref().unwrap().error.is_some());
        assert!(app.message_row(&target).unwrap().redactable);
        let _ = app.update(message(Event::Reason(request, "Spam".into())));
        let _ = app.update(message(Event::Reason(request + 1, "stale".into())));
        let operation = arm(&mut app);
        app.message_delete.as_mut().unwrap().attempted = true;
        finish(&mut app, operation, Err("fixture refusal".into()));
        let _ = app.update(message(Event::Reason(request, "changed".into())));
        assert_eq!(app.message_delete.as_ref().unwrap().reason, "Spam");
        let _ = app.update(message(Event::Confirm(request)));
        assert!(app.message_delete.is_none());
    }
    #[test]
    fn retry_keeps_transaction_and_late_completions_do_not_replace_another_confirmation() {
        let mut app = demo_app();
        let own = make_target(&app, false, "proposal");
        let request = open(&mut app, &own);
        let transaction = app.message_delete.as_ref().unwrap().transaction.clone();
        let operation = arm(&mut app);
        finish(&mut app, operation, Err("fixture refusal".into()));
        let state = app.message_delete.as_ref().unwrap();
        assert!(!state.pending && state.error.as_ref().unwrap().contains("fixture refusal"));
        assert_eq!(state.transaction, transaction);
        let old = arm(&mut app);
        let _ = app.update(message(Event::Cancel(request)));
        let next = make_target(&app, false, "proposal-followup");
        let next_request = open(&mut app, &next);
        finish(&mut app, old, Ok(()));
        assert_eq!(app.message_delete.as_ref().unwrap().request, next_request);
        assert_eq!(app.pending_ops, 0);
        let old = arm(&mut app);
        app.account_id = Some("@another:example.org".into());
        finish(&mut app, old, Err("old account refusal".into()));
        assert_eq!(app.pending_ops, 0);
        assert!(app.message_delete.is_none());
    }
    #[test]
    fn confirmed_redaction_finishes_during_shutdown_without_waiting_for_a_popup() {
        let mut app = demo_app();
        let own = make_target(&app, false, "proposal");
        open(&mut app, &own);
        let operation = arm(&mut app);
        app.demo = None;
        let _ = app.update(Message::Close);
        assert!(!app.selection.shutdown);
        finish(&mut app, operation, Ok(()));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
    }
    #[test]
    fn hover_delete_opens_a_clamped_confirmation_and_only_confirm_sends() {
        use iced::{
            Event as IcedEvent, Rectangle, Size, Vector,
            advanced::{Layout, Shell, layout, mouse, widget},
        };
        #[derive(Default)]
        struct Probe {
            delete: Option<Rectangle>,
            confirm: Option<Rectangle>,
            popup: Option<Rectangle>,
            translation: Vector,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                if id == Some(&widget::Id::new("delete-message")) {
                    self.delete = Some(bounds - self.translation);
                }
                if id == Some(&widget::Id::new("confirm-message-delete")) {
                    self.confirm = Some(bounds);
                }
                if id == Some(&widget::Id::new("delete-confirmation")) {
                    self.popup = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                _: Rectangle,
                _: Rectangle,
                translation: Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.translation = translation;
                }
            }
        }
        let mut app = demo_app();
        let target = make_target(&app, false, "proposal");
        let original = app.message_row(&target).unwrap().clone();
        app.model.room.as_mut().unwrap().rows = Arc::new(vec![original.clone()]);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(360.0, 480.0);
        let mut tree = widget::Tree::empty();
        let mut view = app.pane(false);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &IcedEvent::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(probe.delete.unwrap().center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        drop(view);
        let open = messages
            .into_iter()
            .find(|message| matches!(message, Message::Deletion(Event::Open(_))))
            .unwrap();
        let _ = app.update(open);
        assert_eq!(app.message_row(&target).unwrap().body, original.body);
        let mut view = app.pane(false);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut popup_overlay = view
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &Rectangle::with_size(size),
                Vector::ZERO,
            )
            .unwrap();
        let overlay = popup_overlay.as_overlay_mut();
        let node = overlay.layout(&renderer, size);
        let mut probe = Probe::default();
        overlay.operate(Layout::new(&node), &renderer, &mut probe);
        let popup = probe.popup.unwrap();
        assert!(
            popup.x >= 8.0
                && popup.y >= 8.0
                && popup.x + popup.width <= size.width - 8.0 + 0.1
                && popup.y + popup.height <= size.height - 8.0 + 0.1,
            "{popup:?}"
        );
        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            overlay.update(
                &IcedEvent::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(probe.confirm.unwrap().center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
            );
        }
        drop(popup_overlay);
        drop(view);
        let confirm = messages
            .into_iter()
            .find(|message| matches!(message, Message::Deletion(Event::Confirm(_))))
            .unwrap();
        let _ = app.update(confirm);
        assert!(!app.message_row(&target).unwrap().redactable);
    }
    #[test]
    fn a_remote_redaction_that_removes_a_thread_reply_restores_its_saved_draft() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let key = app.draft_key(true).unwrap();
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("unsent reply");
        let target = make_target(&app, true, "review-reply-2");
        let _ = app.update(Message::Editing(message_edit::Event::Start(target.clone())));
        assert!(app.drafts[&key].editing.is_some());
        let mut model = app.model.clone();
        Arc::make_mut(&mut model.thread.as_mut().unwrap().rows)
            .retain(|row| row.key != target.event);
        let row = Arc::make_mut(&mut model.room.as_mut().unwrap().rows)
            .iter_mut()
            .find(|row| row.key == target.event)
            .unwrap();
        row.redactable = false;
        row.edit_source = None;
        row.body = "Message removed".into();
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.drafts[&key].editing.is_none());
        assert_eq!(app.drafts[&key].content.text(), "unsent reply");
    }
}
