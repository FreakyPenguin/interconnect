//! Captured message reports with explicit submission and account-scoped completion.
use super::{App, MAX_PENDING_OPERATIONS, Message, reaction_picker::Target, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, row, text, text_input},
};
use interconnect::connected::{MessageRow, RowKind};

pub struct State {
    target: Target,
    account: Option<String>,
    request: u64,
    reason: String,
    pending: Option<u64>,
    sent: bool,
    error: Option<String>,
}
impl State {
    pub fn dismiss(&self) -> Message {
        message(Event::Cancel(self.request))
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(Target),
    Reason(u64, String),
    Submit(u64),
    Cancel(u64),
    Ignore,
    Finished {
        request: u64,
        operation: u64,
        account: Option<String>,
        target: Target,
        result: Result<(), String>,
    },
}
fn message(event: Event) -> Message {
    Message::Reporting(event)
}

impl App {
    fn report_eligible(&self, target: &Target) -> bool {
        !self.closing
            && !self.stopped
            && (self.demo.is_some() || self.model.authentication.is_none())
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.space_settings.is_open()
            && !self.room_settings.is_open()
            && (target.thread
                || !self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width)))
            && (self.demo.is_some() || self.reaction_composer(target).is_some())
            && self
                .message_row(target)
                .is_some_and(|row| row.kind == RowKind::Message && row.redactable)
    }
    pub(super) fn report_action(
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
            && self.report_eligible(&target)
            && !self.reporting.values().any(|(account, active)| {
                account == &self.account_id
                    && active.room == target.room
                    && active.event == target.event
            }))
        .then(|| message(Event::Open(target)))
    }
    pub(super) fn validate_message_report(&mut self) {
        if self.message_report.as_ref().is_some_and(|state| {
            state.account != self.account_id || !self.report_eligible(&state.target)
        }) {
            self.message_report = None;
        }
    }
    pub(super) fn report_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open(target) => {
                if !self.message_row(&target).is_some_and(|row| {
                    self.report_action(row, target.thread, target.generation)
                        .is_some()
                }) {
                    return Task::none();
                }
                self.generation += 1;
                self.message_report = Some(State {
                    target,
                    account: self.account_id.clone(),
                    request: self.generation,
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
                return iced::widget::operation::focus("report-reason");
            }
            Event::Reason(request, reason) => {
                if let Some(state) = &mut self.message_report
                    && state.request == request
                    && state.pending.is_none()
                    && !state.sent
                {
                    state.reason = reason;
                    state.error = None;
                }
            }
            Event::Cancel(request) => {
                if let Some(state) = &self.message_report
                    && state.request == request
                {
                    let thread = state.target.thread;
                    self.message_report = None;
                    return iced::widget::operation::focus(if thread {
                        "thread-composer"
                    } else {
                        "room-composer"
                    });
                }
            }
            Event::Submit(request) => {
                let Some(state) = self.message_report.as_ref().filter(|s| {
                    s.request == request
                        && s.account == self.account_id
                        && s.pending.is_none()
                        && !s.sent
                        && self.report_eligible(&s.target)
                }) else {
                    return Task::none();
                };
                if self.pending_ops >= MAX_PENDING_OPERATIONS || self.composing {
                    return Task::none();
                }
                let reason = match interconnect::message_reporting::validate_reason(&state.reason) {
                    Ok(reason) => reason.to_owned(),
                    Err(error) => {
                        self.message_report.as_mut().unwrap().error = Some(error);
                        return Task::none();
                    }
                };
                let target = state.target.clone();
                let account = state.account.clone();
                if self
                    .reporting
                    .values()
                    .any(|(a, t)| a == &account && t.room == target.room && t.event == target.event)
                {
                    return Task::none();
                }
                if self.demo.is_some() {
                    self.message_report.as_mut().unwrap().sent = true;
                    return Task::none();
                }
                let Some(composer) = self.reaction_composer(&target) else {
                    return Task::none();
                };
                self.generation += 1;
                let operation = self.generation;
                self.reporting
                    .insert(operation, (account.clone(), target.clone()));
                self.pending_ops += 1;
                let state = self.message_report.as_mut().unwrap();
                state.pending = Some(operation);
                state.error = None;
                let event = target.event.clone();
                return Task::perform(composer.report_message(event, reason), move |result| {
                    message(Event::Finished {
                        request,
                        operation,
                        account: account.clone(),
                        target: target.clone(),
                        result,
                    })
                });
            }
            Event::Finished {
                request,
                operation,
                account,
                target,
                result,
            } => {
                if self.reporting.get(&operation) != Some(&(account.clone(), target.clone())) {
                    return Task::none();
                }
                self.reporting.remove(&operation);
                self.pending_ops = self.pending_ops.saturating_sub(1);
                if account != self.account_id {
                    return Task::none();
                }
                if let Some(state) = &mut self.message_report
                    && state.request == request
                    && state.target == target
                    && state.pending == Some(operation)
                {
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
    pub(super) fn report_dialog(&self) -> Element<'_, Message> {
        let state = self.message_report.as_ref().unwrap();
        let request = state.request;
        let busy = state.pending.is_some();
        let modal = iced::widget::responsive(move |size| {
            let mut body = column![
                text("Send a report to your homeserver administrators for review. Reporting does not remove the message or ban its sender.").size(13),
            ].spacing(12);
            if let Some(row) = self.message_row(&state.target) {
                let room = self
                    .model
                    .rooms
                    .iter()
                    .find(|room| room.id == state.target.room)
                    .map_or("Room", |room| room.name.as_str());
                let preview: String = row.body.chars().take(240).collect();
                body = body
                    .push(
                        text(format!("{room} · {}", row.display_name))
                            .size(13)
                            .style(shell::muted_text),
                    )
                    .push(text(preview).size(13));
            }
            if state.sent {
                body = body.push(
                    text(if self.demo.is_some() {
                        "Report simulated (demo). No report was sent."
                    } else {
                        "Report sent to your homeserver administrators."
                    })
                    .size(14),
                );
            } else {
                let mut input = text_input("Reason for reporting…", &state.reason)
                    .id("report-reason")
                    .cursor_blinking(false)
                    .padding(10);
                if !busy {
                    input = input.on_input(move |value| message(Event::Reason(request, value)));
                }
                body = body.push(input).push(
                    text("Include only the details needed to review this message.")
                        .size(12)
                        .style(shell::muted_text),
                );
            }
            if let Some(error) = &state.error {
                body = body.push(text(error).size(13).style(shell::danger_text));
            }
            let mut actions = row![
                iced::widget::space().width(Length::Fill),
                button(text("Close").size(13))
                    .keyboard_navigation(true)
                    .padding([7, 12])
                    .style(shell::quiet_button)
                    .on_press(message(Event::Cancel(request)))
            ]
            .spacing(8);
            if !state.sent {
                actions = actions.push(
                    button(
                        text(if busy {
                            "Sending report…"
                        } else {
                            "Send report"
                        })
                        .size(13),
                    )
                    .keyboard_navigation(true)
                    .padding([7, 12])
                    .style(shell::primary_button)
                    .on_press_maybe(
                        (!busy
                            && !self.composing
                            && self.pending_ops < MAX_PENDING_OPERATIONS
                            && interconnect::message_reporting::validate_reason(&state.reason)
                                .is_ok())
                        .then_some(message(Event::Submit(request))),
                    ),
                );
            }
            super::dialog::modal(
                super::dialog::frame(
                    size,
                    540.0,
                    390.0,
                    "message-report",
                    shell::title("Report message", 20),
                    body,
                    actions,
                ),
                message(Event::Ignore),
            )
        });
        super::focus_scope::dismissible(
            modal,
            "message-report-focus",
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
            event: if thread { "review-reply-2" } else { "review" }.into(),
        }
    }
    fn open(app: &mut App, target: &Target) -> u64 {
        let _ = app.update(message(Event::Open(target.clone())));
        app.message_report.as_ref().unwrap().request
    }
    fn arm(app: &mut App) -> Event {
        app.generation += 1;
        let operation = app.generation;
        let state = app.message_report.as_mut().unwrap();
        state.pending = Some(operation);
        app.reporting
            .insert(operation, (state.account.clone(), state.target.clone()));
        app.pending_ops += 1;
        Event::Finished {
            request: state.request,
            operation,
            account: state.account.clone(),
            target: state.target.clone(),
            result: Ok(()),
        }
    }
    #[test]
    fn room_and_thread_reports_require_reason_and_submission_and_preserve_conversations() {
        for thread in [false, true] {
            let mut app = demo_app();
            let _ = app.update(Message::WindowFocused(true));
            let _ = app.update(Message::Thread("review".into()));
            let key = app.draft_key(thread).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                text_editor::Content::with_text("unfinished");
            app.scroll_positions.insert(key.clone(), 125.0);
            let target = target(&app, thread);
            let row = app.message_row(&target).unwrap().clone();
            let request = open(&mut app, &target);
            assert!(!app.selection.conversation_visible);
            let _ = app.update(message(Event::Submit(request)));
            assert!(app.message_report.as_ref().unwrap().error.is_some());
            let _ = app.update(message(Event::Reason(request, "Spam".into())));
            let _ = app.update(Message::Edit(
                key.clone(),
                text_editor::Action::Edit(text_editor::Edit::Insert('x')),
            ));
            let _ = app.update(Message::Send(key.clone()));
            let _ = app.update(message(Event::Submit(request)));
            assert!(app.message_report.as_ref().unwrap().sent);
            assert_eq!(app.message_row(&target).unwrap().body, row.body);
            assert_eq!(app.pending_ops, 0);
            let _ = app.update(Message::Escape);
            assert!(app.message_report.is_none());
            assert!(app.selection.conversation_visible);
            assert_eq!(app.drafts[&key].content.text(), "unfinished");
            assert_eq!(app.scroll_positions[&key], 125.0);
        }
    }
    #[test]
    fn stale_targets_cancelled_prompts_and_new_accounts_cannot_receive_late_reports() {
        let mut app = demo_app();
        let original = target(&app, false);
        let old = open(&mut app, &original);
        let _ = app.update(Message::Escape);
        let _ = app.update(message(Event::Submit(old)));
        assert!(app.reporting.is_empty());
        open(&mut app, &original);
        let completed = arm(&mut app);
        let _ = app.update(Message::Escape);
        let mut next = original.clone();
        next.event = "proposal".into();
        let current = open(&mut app, &next);
        let _ = app.update(message(completed.clone()));
        let _ = app.update(message(completed));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.message_report.as_ref().unwrap().request, current);
        assert!(!app.message_report.as_ref().unwrap().sent);
        let completed = arm(&mut app);
        app.account_id = Some("@another:example.org".into());
        let _ = app.update(message(completed));
        assert!(app.message_report.is_none());
        assert_eq!(app.pending_ops, 0);
        let mut stale = original;
        stale.generation += 1;
        let _ = app.update(message(Event::Open(stale)));
        assert!(app.message_report.is_none());
    }
    #[test]
    fn errors_preserve_reason_and_admitted_reports_finish_shutdown_after_dismissal() {
        let mut app = demo_app();
        let target = target(&app, false);
        let request = open(&mut app, &target);
        let _ = app.update(message(Event::Reason(request, "Spam".into())));
        let mut failed = arm(&mut app);
        if let Event::Finished { result, .. } = &mut failed {
            *result = Err("fixture refusal".into());
        }
        let _ = app.update(message(failed));
        let state = app.message_report.as_ref().unwrap();
        assert_eq!(state.reason, "Spam");
        assert!(state.error.is_some() && state.pending.is_none());
        let finished = arm(&mut app);
        app.demo = None;
        let _ = app.update(Message::Close);
        assert!(!app.selection.shutdown);
        let _ = app.update(message(finished));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
    }
    #[test]
    fn escape_from_the_rendered_form_dismisses_once_before_and_after_submission() {
        use iced::advanced::{Shell, layout, mouse, widget};
        use iced::keyboard::{
            Key, Modifiers,
            key::{Named, NativeCode, Physical},
        };
        for sent in [false, true] {
            let mut app = demo_app();
            let target = target(&app, false);
            let request = open(&mut app, &target);
            app.message_report.as_mut().unwrap().sent = sent;
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let size = iced::Size::new(640.0, 480.0);
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let key = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: Key::Named(Named::Escape),
                modified_key: Key::Named(Named::Escape),
                physical_key: Physical::Unidentified(NativeCode::Unidentified),
                location: iced::keyboard::Location::Standard,
                modifiers: Modifiers::empty(),
                text: None,
                repeat: false,
            });
            let mut messages = Vec::new();
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
                matches!(&messages[..], [Message::Reporting(Event::Cancel(current))] if *current == request)
            );
        }
    }
    #[test]
    fn report_dialog_fits_small_windows_and_has_no_redraw_timer() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        let target = target(&app, false);
        open(&mut app, &target);
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
        }
    }
}
