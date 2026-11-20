//! Explicit cancellation of captured local echoes, independent of message drafts.
use super::{App, DraftKey, MAX_PENDING_OPERATIONS, Message, MessageRow, current_pane, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, text},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    account: Option<String>,
    key: DraftKey,
    generation: u64,
    transaction: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Sending {
    account: Option<String>,
    room: String,
    transaction: String,
}
impl Target {
    fn sending(&self) -> Sending {
        Sending {
            account: self.account.clone(),
            room: self.key.room.clone(),
            transaction: self.transaction.clone(),
        }
    }
}
#[derive(Default)]
pub struct State {
    confirm: Option<Target>,
    pending: HashSet<Sending>,
    errors: HashMap<Sending, String>,
    feedback: Option<(Target, String)>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(Target),
    Keep(Target),
    Confirm(Target),
    Finished(Target, Result<bool, String>),
    DismissFeedback(Target),
}
fn message(event: Event) -> Message {
    Message::QueuedSend(event)
}
impl App {
    fn queued_send_row(&self, target: &Target) -> Option<&MessageRow> {
        if target.account != self.account_id
            || self.draft_key(target.key.root.is_some()).as_ref() != Some(&target.key)
        {
            return None;
        }
        let thread = target.key.root.is_some();
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        if selected.as_ref()?.0 != target.generation {
            return None;
        }
        let pane = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        current_pane(pane, target.generation)?
            .rows
            .iter()
            .find(|row| {
                row.is_own
                    && row.cancel_send.as_ref().is_some_and(|handle| {
                        handle.transaction == target.transaction && handle.room == target.key.room
                    })
            })
    }
    fn queued_send_allowed(&self, target: &Target) -> bool {
        self.context_menu_allowed()
            && self.selection.conversation_visible
            && (target.key.root.is_none() || self.selection.members.is_none())
            && (target.key.root.is_some()
                || !self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width)))
            && !self.composing
            && self.emoji_management.is_none()
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.space_settings.is_open()
            && !self.retrying.contains(&target.transaction)
            && self.pending_ops < MAX_PENDING_OPERATIONS
            && self.queued_send_row(target).is_some()
            && !self.queued_sends.pending.contains(&target.sending())
    }
    pub(super) fn queued_send_confirmation_open(&self) -> bool {
        self.queued_sends.confirm.is_some()
    }
    pub(super) fn queued_send_pending(&self, transaction: &str) -> bool {
        self.queued_sends
            .pending
            .iter()
            .any(|sending| sending.account == self.account_id && sending.transaction == transaction)
    }
    pub(super) fn validate_queued_send(&mut self) {
        if self
            .queued_sends
            .confirm
            .as_ref()
            .is_some_and(|target| self.queued_send_row(target).is_none())
        {
            self.queued_sends.confirm = None;
        }
        self.queued_sends
            .errors
            .retain(|sending, _| sending.account == self.account_id);
        if self
            .queued_sends
            .feedback
            .as_ref()
            .is_some_and(|(target, _)| target.account != self.account_id)
        {
            self.queued_sends.feedback = None;
        }
    }
    pub(super) fn queued_send_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open(target) if self.queued_send_allowed(&target) => {
                self.queued_sends.errors.remove(&target.sending());
                if self
                    .queued_sends
                    .feedback
                    .as_ref()
                    .is_some_and(|(current, _)| current.sending() == target.sending())
                {
                    self.queued_sends.feedback = None;
                }
                self.queued_sends.confirm = Some(target);
            }
            Event::Keep(target) if self.queued_sends.confirm.as_ref() == Some(&target) => {
                self.queued_sends.confirm = None;
                return iced::widget::operation::focus(if target.key.root.is_some() {
                    "thread-composer"
                } else {
                    "room-composer"
                });
            }
            Event::Confirm(target)
                if self.queued_sends.confirm.as_ref() == Some(&target)
                    && self.queued_send_allowed(&target) =>
            {
                let handle = self
                    .queued_send_row(&target)
                    .unwrap()
                    .cancel_send
                    .clone()
                    .unwrap();
                self.queued_sends.confirm = None;
                self.queued_sends.pending.insert(target.sending());
                self.pending_ops += 1;
                let composer = if target.key.root.is_some() {
                    "thread-composer"
                } else {
                    "room-composer"
                };
                return Task::batch([
                    Task::perform(handle.cancel(), move |result| {
                        message(Event::Finished(target.clone(), result))
                    }),
                    iced::widget::operation::focus(composer),
                ]);
            }
            Event::Finished(target, result) => {
                if !self.queued_sends.pending.remove(&target.sending()) {
                    return Task::none();
                }
                self.pending_ops -= 1;
                if target.account != self.account_id {
                    return Task::none();
                }
                let error = match result {
                    Ok(true) => None,
                    Ok(false) => Some("This message has already left the send queue. Check its status; if it was sent, use Delete message.".to_owned()),
                    Err(error) => Some(format!("Could not cancel send: {error}")),
                };
                if let Some(error) = error {
                    self.queued_sends
                        .errors
                        .insert(target.sending(), error.clone());
                    // The local echo may have become a synced message or its pane
                    // may have closed while cancellation was running.
                    self.queued_sends.feedback = Some((target, error));
                }
            }
            Event::DismissFeedback(target)
                if self
                    .queued_sends
                    .feedback
                    .as_ref()
                    .is_some_and(|(current, _)| current == &target) =>
            {
                self.queued_sends.feedback = None;
                self.queued_sends.errors.remove(&target.sending());
            }
            _ => {}
        }
        Task::none()
    }
    pub(super) fn queued_send_feedback(&self) -> Option<Element<'_, Message>> {
        let (target, error) = self
            .queued_sends
            .feedback
            .as_ref()
            .filter(|(target, _)| target.account == self.account_id)?;
        let room = self
            .model
            .rooms
            .iter()
            .find(|room| room.id == target.key.room)
            .map_or(target.key.room.as_str(), |room| room.name.as_str());
        Some(
            row![
                text(format!("{room}: {error}"))
                    .size(12)
                    .width(Length::Fill)
                    .style(shell::danger_text),
                super::tooltips::icon_button(
                    super::icons::Kind::Close,
                    "Dismiss send cancellation error",
                    Some(message(Event::DismissFeedback(target.clone())))
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
        )
    }
    pub(super) fn queued_send_controls(
        &self,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Element<'_, Message>> {
        let handle = row.cancel_send.as_ref()?;
        let key = self.draft_key(thread)?;
        if !row.is_own || handle.room != key.room {
            return None;
        }
        let target = Target {
            account: self.account_id.clone(),
            key,
            generation,
            transaction: handle.transaction.clone(),
        };
        let sending = target.sending();
        let mut content = column![].spacing(6);
        if self.queued_sends.pending.contains(&sending) {
            content = content.push(text("Cancelling send…").size(11).style(shell::muted_text));
        } else if self.queued_sends.confirm.as_ref() == Some(&target) {
            content = content.push(text("Cancel this send? Pending uploads will be stopped. If sending has started, Matrix may finish sending and request removal; others may see the message first.").size(12).style(shell::muted_text));
            content = content.push(
                row![
                    button(text("Keep sending").size(12))
                        .keyboard_navigation(true)
                        .style(shell::quiet_button)
                        .on_press(message(Event::Keep(target.clone()))),
                    button(text("Cancel send").size(12))
                        .keyboard_navigation(true)
                        .style(button::danger)
                        .on_press_maybe(
                            self.queued_send_allowed(&target)
                                .then(|| message(Event::Confirm(target.clone())))
                        ),
                ]
                .spacing(8),
            );
        } else {
            content = content.push(
                button(text("Cancel send…").size(12))
                    .keyboard_navigation(true)
                    .style(shell::quiet_button)
                    .on_press_maybe(
                        self.queued_send_allowed(&target)
                            .then(|| message(Event::Open(target.clone()))),
                    ),
            );
        }
        if let Some(error) = self.queued_sends.errors.get(&sending) {
            content = content.push(text(error).size(12).style(shell::danger_text));
        }
        let content = container(content).width(Length::Fill);
        Some(if self.queued_sends.confirm.as_ref() == Some(&target) {
            super::focus_scope::dismissible(
                content,
                "queued-send-confirmation",
                message(Event::Keep(target)),
            )
        } else {
            content.into()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    use iced::widget::text_editor;

    fn target(app: &App, transaction: &str) -> Target {
        Target {
            account: app.account_id.clone(),
            key: app.draft_key(false).unwrap(),
            generation: app.selection.room.as_ref().unwrap().0,
            transaction: transaction.into(),
        }
    }
    #[test]
    fn stale_confirmation_without_a_current_local_echo_cannot_cancel() {
        for interrupt in ["gone", "room", "account", "close"] {
            let mut app = demo_app();
            let source = target(&app, "fixture-transaction");
            app.queued_sends.confirm = Some(source.clone());
            match interrupt {
                "room" => {
                    let _ = app.update(Message::Room("general".into()));
                }
                "account" => app.account_id = Some("@other:example.org".into()),
                "close" => app.closing = true,
                _ => {}
            }
            let selected = app.selection.room.clone();
            let _ = app.update(message(Event::Confirm(source)));
            assert_eq!(app.pending_ops, 0, "{interrupt}");
            assert!(app.queued_sends.pending.is_empty(), "{interrupt}");
            assert_eq!(app.selection.room, selected, "{interrupt}");
        }
    }
    #[test]
    fn cancellation_completion_is_deduplicated_and_does_not_change_drafts_or_navigation() {
        let mut app = demo_app();
        let source = target(&app, "fixture-transaction");
        let key = source.key.clone();
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("unsent draft");
        app.scroll_positions.insert(key.clone(), 175.0);
        app.queued_sends.pending.insert(source.sending());
        app.pending_ops = 1;
        let _ = app.update(Message::Room("general".into()));
        let selected = app.selection.room.clone();
        let _ = app.update(message(Event::Finished(source.clone(), Ok(true))));
        assert_eq!(app.pending_ops, 0);
        assert!(!app.queued_send_pending(&source.transaction));
        assert_eq!(app.drafts[&key].content.text(), "unsent draft");
        assert_eq!(app.scroll_positions[&key], 175.0);
        assert_eq!(app.selection.room, selected);
        let _ = app.update(message(Event::Finished(
            source,
            Err("late duplicate".into()),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(app.model.navigation_error.is_none());
    }
    #[test]
    fn old_account_completion_cannot_publish_a_cancellation_error() {
        let mut app = demo_app();
        let source = target(&app, "fixture-transaction");
        app.queued_sends.pending.insert(source.sending());
        app.pending_ops = 1;
        app.account_id = Some("@replacement:example.org".into());
        let _ = app.update(message(Event::Finished(
            source,
            Err("old account storage error".into()),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(app.model.navigation_error.is_none());
        assert!(app.queued_sends.errors.is_empty());
    }
    #[test]
    fn already_sent_cancellation_failure_remains_visible_after_echo_disappears() {
        let mut app = demo_app();
        let source = target(&app, "fixture-transaction");
        app.queued_sends.pending.insert(source.sending());
        app.pending_ops = 1;
        let _ = app.update(message(Event::Finished(source, Ok(false))));
        assert_eq!(app.pending_ops, 0);
        let (_, error) = app.queued_sends.feedback.as_ref().unwrap();
        assert!(error.contains("already left the send queue"));
        assert!(error.contains("Delete message"));
        let mut snapshot = app.model.clone();
        snapshot.navigation_error = None;
        let _ = app.update(Message::State(Box::new(snapshot)));
        assert!(
            app.queued_send_feedback().is_some(),
            "feedback survives SDK snapshots and lost echoes"
        );
        assert!(
            app.utility_banner().is_some(),
            "feedback appears outside the source row"
        );
        let source = app.queued_sends.feedback.as_ref().unwrap().0.clone();
        let _ = app.update(message(Event::DismissFeedback(source)));
        assert!(app.queued_sends.feedback.is_none());
    }
}
