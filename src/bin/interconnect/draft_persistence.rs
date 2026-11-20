//! Change-triggered draft snapshots and a single serialized, debounced writer.
use super::{App, Draft, DraftKey, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, row, text},
};
use interconnect::drafts::{Entry, Snapshot, Store};
use std::sync::Arc;

#[derive(Default)]
pub struct State {
    pub error: Option<String>,
    pub close_requested: bool,
    initialized: bool,
    account: Option<String>,
    store: Option<Arc<Store>>,
    current: Arc<Snapshot>,
    revision: u64,
    saved_revision: u64,
    pending: Option<(u64, u64, u64)>,
    timer: Option<iced::task::Handle>,
    timer_revision: Option<u64>,
    discard_close: bool,
}
impl State {
    pub(super) fn dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub(super) fn can_retry(&self) -> bool {
        self.store.is_some() && self.pending.is_none()
    }
    pub(super) fn blocks_sign_out(&self) -> bool {
        self.dirty() && self.store.is_some()
    }
    fn cancel_timer(&mut self) {
        self.timer = None;
        self.timer_revision = None;
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Due(u64, u64),
    Saved {
        request: u64,
        result: Result<(), String>,
    },
    Retry,
    CancelClose,
    DiscardClose,
    Ignore,
}
fn message(event: Event) -> Message {
    Message::DraftPersistence(event)
}

/// Saving an edit retains the original unsent draft rather than turning an
/// unfinished edit into a new message when the app restarts.
fn entry(key: &DraftKey, draft: &Draft) -> Option<Entry> {
    let draft = draft
        .editing
        .as_ref()
        .map_or(draft, |edit| edit.saved.as_ref());
    let text = draft.content.text();
    if text.is_empty() && draft.reply.is_none() {
        return None;
    }
    Some(Entry {
        room: key.room.clone(),
        root: key.root.clone(),
        reply: draft.reply.clone(),
        emoji: draft
            .markdown
            .emoji
            .iter()
            .filter(|choice| text.contains(&choice.token))
            .cloned()
            .collect(),
        mentions: draft
            .markdown
            .mentions
            .iter()
            .filter(|user| text.contains(user.as_str()))
            .cloned()
            .collect(),
        named_mentions: draft.markdown.named_mentions.clone(),
        text,
        format: draft.markdown.format,
    })
}

pub fn may_change(message: &Message) -> bool {
    matches!(
        message,
        Message::Edit(_, iced::widget::text_editor::Action::Edit(_))
            | Message::DeleteWord(..)
            | Message::Send(_)
            | Message::Sent(..)
            | Message::AttachmentsSent(..)
            | Message::Markdown(_)
            | Message::Editing(_)
            | Message::Replying(_)
            | Message::ComposerEmoji(_)
            | Message::EmojiCompletion(_)
            | Message::UserCompletion(_)
            | Message::Attachment(_)
    )
}

impl App {
    pub(super) fn restore_persistent_drafts(&mut self) {
        if self.demo.is_some() {
            return;
        }
        let Some(account) = self.account_id.clone() else {
            return;
        };
        let Some(loaded) = self.model.saved_drafts.clone() else {
            return;
        };
        if self.draft_persistence.account.as_ref() == Some(&account) {
            match &loaded {
                Ok(loaded)
                    if self
                        .draft_persistence
                        .store
                        .as_ref()
                        .is_some_and(|store| store.id() == loaded.store.id()) =>
                {
                    return;
                }
                Err(_)
                    if self.draft_persistence.initialized
                        && self.draft_persistence.store.is_none() =>
                {
                    return;
                }
                _ => {}
            }
        }
        self.draft_persistence.cancel_timer();
        self.draft_persistence.account = Some(account);
        self.draft_persistence.initialized = true;
        self.draft_persistence.revision = 0;
        self.draft_persistence.saved_revision = 0;
        match loaded {
            Ok(loaded) => {
                self.draft_persistence.store = Some(loaded.store);
                self.draft_persistence.current = loaded.saved.clone();
                self.draft_persistence.error = None;
                for saved in &loaded.saved.entries {
                    let key = DraftKey {
                        room: saved.room.clone(),
                        root: saved.root.clone(),
                    };
                    // An existing local draft, including an intentionally cleared
                    // one, wins over a snapshot from an earlier session.
                    self.drafts.entry(key).or_insert_with(|| {
                        let mut draft = Draft {
                            reply: saved.reply.clone(),
                            content: iced::widget::text_editor::Content::with_text(&saved.text),
                            ..Default::default()
                        };
                        draft.markdown.format = saved.format;
                        draft.markdown.emoji = saved.emoji.clone();
                        draft.markdown.mentions = saved.mentions.clone();
                        draft.markdown.named_mentions = saved.named_mentions.clone();
                        draft
                    });
                }
            }
            Err(error) => {
                self.draft_persistence.store = None;
                self.draft_persistence.current = Arc::default();
                self.draft_persistence.error = Some(error);
            }
        }
        self.capture_persistent_drafts();
    }

    pub(super) fn capture_persistent_drafts(&mut self) {
        if self.demo.is_some()
            || !self.draft_persistence.initialized
            || self.account_id != self.draft_persistence.account
        {
            return;
        }
        let mut entries: Vec<_> = self
            .drafts
            .iter()
            .filter_map(|(key, draft)| entry(key, draft))
            .collect();
        entries.sort_by(|a, b| (&a.room, &a.root).cmp(&(&b.room, &b.root)));
        if self.draft_persistence.current.entries != entries {
            self.draft_persistence.current = Arc::new(Snapshot { entries });
            self.draft_persistence.revision += 1;
            self.draft_persistence.cancel_timer();
            if self.draft_persistence.store.is_some() {
                self.draft_persistence.error = None;
            }
        }
    }

    /// Normal close waits for message writes as well as the newest draft save.
    pub(super) fn defer_close_for_drafts(&mut self) -> bool {
        if self.demo.is_some()
            || !self.draft_persistence.initialized
            || self.draft_persistence.discard_close
        {
            return false;
        }
        if self.draft_persistence.dirty() || self.pending_ops > 0 {
            self.draft_persistence.close_requested = true;
            self.draft_persistence.cancel_timer();
            self.stop_typing();
            return true;
        }
        false
    }

    pub(super) fn save_drafts(&mut self) -> Task<Message> {
        if !self.draft_persistence.dirty()
            || self.draft_persistence.pending.is_some()
            || self.pending_ops >= super::MAX_PENDING_OPERATIONS
            || (self.sign_out_open && self.selection.sign_out != 0)
        {
            return Task::none();
        }
        let Some(store) = self.draft_persistence.store.clone() else {
            return Task::none();
        };
        self.draft_persistence.cancel_timer();
        self.generation += 1;
        let request = self.generation;
        self.draft_persistence.pending =
            Some((request, store.id(), self.draft_persistence.revision));
        self.pending_ops += 1;
        Task::perform(
            store.save(self.draft_persistence.current.clone()),
            move |result| message(Event::Saved { request, result }),
        )
    }

    pub(super) fn schedule_draft_save(&mut self) -> Task<Message> {
        if self.demo.is_some() || !self.draft_persistence.initialized || self.sign_out_open {
            return Task::none();
        }
        if self.draft_persistence.close_requested {
            if self.pending_ops == 0
                && (!self.draft_persistence.dirty() || self.draft_persistence.discard_close)
            {
                self.draft_persistence.close_requested = false;
                return self.update_inner(Message::Close);
            }
            return if self.draft_persistence.error.is_none() {
                self.save_drafts()
            } else {
                Task::none()
            };
        }
        if !self.draft_persistence.dirty()
            || self.draft_persistence.pending.is_some()
            || self.draft_persistence.error.is_some()
            || self.draft_persistence.store.is_none()
            || self.draft_persistence.timer_revision == Some(self.draft_persistence.revision)
        {
            return Task::none();
        }
        let revision = self.draft_persistence.revision;
        let id = self.draft_persistence.store.as_ref().unwrap().id();
        let (task, handle) = Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            },
            move |()| message(Event::Due(id, revision)),
        )
        .abortable();
        self.draft_persistence.timer = Some(handle.abort_on_drop());
        self.draft_persistence.timer_revision = Some(revision);
        task
    }

    pub(super) fn draft_persistence_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Due(id, revision)
                if self
                    .draft_persistence
                    .store
                    .as_ref()
                    .is_some_and(|store| store.id() == id)
                    && self.draft_persistence.timer_revision == Some(revision) =>
            {
                self.draft_persistence.cancel_timer();
                return self.save_drafts();
            }
            Event::Saved { request, result } => {
                let Some((pending, id, revision)) = self.draft_persistence.pending else {
                    return Task::none();
                };
                if request != pending {
                    return Task::none();
                }
                self.draft_persistence.pending = None;
                self.pending_ops -= 1;
                if self
                    .draft_persistence
                    .store
                    .as_ref()
                    .is_some_and(|store| store.id() == id)
                {
                    match result {
                        Ok(()) => {
                            self.draft_persistence.saved_revision = revision;
                            self.draft_persistence.error = None;
                        }
                        Err(error) => self.draft_persistence.error = Some(error),
                    }
                }
            }
            Event::Retry if self.draft_persistence.can_retry() => {
                self.draft_persistence.error = None;
                return self.save_drafts();
            }
            Event::CancelClose => {
                self.draft_persistence.close_requested = false;
                self.draft_persistence.discard_close = false;
            }
            Event::DiscardClose
                if self.draft_persistence.close_requested
                    && self.draft_persistence.error.is_some()
                    && self.draft_persistence.pending.is_none() =>
            {
                self.draft_persistence.discard_close = true;
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn draft_close_view(&self) -> Element<'_, Message> {
        let modal = iced::widget::responsive(|size| {
            let failed = self.draft_persistence.error.is_some();
            let title = if failed {
                "Drafts could not be saved"
            } else {
                "Saving drafts before closing…"
            };
            let mut content = column![text(if failed {
                "Keep Interconnect open to retain your drafts, retry saving, or explicitly close without saving."
            } else {
                "Your unsent drafts are being saved. Interconnect will close when saving finishes."
            }).size(13)].spacing(14).width(Length::Fill);
            if let Some(error) = &self.draft_persistence.error {
                content = content.push(text(error).size(13).style(shell::danger_text));
            }
            let mut actions = row![
                iced::widget::space().width(Length::Fill),
                button(text("Keep open").size(13))
                    .keyboard_navigation(true)
                    .padding([7, 14])
                    .style(shell::quiet_button)
                    .on_press(message(Event::CancelClose)),
            ]
            .spacing(8);
            if failed {
                if self.draft_persistence.store.is_some() {
                    actions = actions.push(
                        button(text("Retry save").size(13))
                            .keyboard_navigation(true)
                            .padding([7, 14])
                            .style(shell::primary_button)
                            .on_press(message(Event::Retry)),
                    );
                }
                actions = actions.push(
                    button(text("Close without saving").size(13))
                        .keyboard_navigation(true)
                        .padding([7, 14])
                        .style(button::danger)
                        .on_press_maybe(
                            self.draft_persistence
                                .pending
                                .is_none()
                                .then_some(message(Event::DiscardClose)),
                        ),
                );
            }
            let header = row![
                shell::title(title, 20),
                iced::widget::space().width(Length::Fill),
                button(super::icons::icon(super::icons::Kind::Close))
                    .keyboard_navigation(true)
                    .padding(6)
                    .style(shell::icon_button)
                    .on_press(message(Event::CancelClose)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            super::dialog::modal(
                super::dialog::frame(
                    size,
                    580.0,
                    if failed { 360.0 } else { 280.0 },
                    "draft-close",
                    header,
                    content,
                    actions.wrap(),
                ),
                message(Event::Ignore),
            )
        });
        super::focus_scope::dismissible(
            modal,
            "draft_persistence-focus",
            message(Event::CancelClose),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor::{Action, Edit};
    use interconnect::{composition::Format, drafts::Loaded};
    use std::collections::HashMap;

    async fn loaded(directory: &std::path::Path) -> Loaded {
        Store::open(
            directory.to_owned(),
            "@me:example.org".into(),
            "store".into(),
            HashMap::from([("store".into(), "fixture-key".into())]),
        )
        .await
        .unwrap()
    }
    fn app(loaded: Loaded) -> App {
        let mut app = crate::tests::demo_app();
        app.demo = None;
        app.model.authentication = None;
        app.model.status = "Connected".into();
        app.model.account_id = Some("@me:example.org".into());
        app.account_id = app.model.account_id.clone();
        app.drafts.clear();
        app.selection.room = Some((1, "!room:example.org".into()));
        app.selection.thread = None;
        app.model.saved_drafts = Some(Ok(loaded));
        app.restore_persistent_drafts();
        let key = app.draft_key(false).unwrap();
        app.drafts.entry(key).or_default();
        app
    }
    #[tokio::test]
    async fn reply_targets_survive_encrypted_restore_including_empty_composers() {
        let directory = tempfile::tempdir().unwrap();
        let mut first = app(loaded(directory.path()).await);
        let room = first.draft_key(false).unwrap();
        first.drafts.get_mut(&room).unwrap().reply = Some("$original".into());
        first.drafts.get_mut(&room).unwrap().content =
            iced::widget::text_editor::Content::with_text("reply draft");
        let thread = DraftKey {
            room: room.room.clone(),
            root: Some("$thread".into()),
        };
        first.drafts.insert(
            thread.clone(),
            Draft {
                reply: Some("$thread-reply".into()),
                ..Default::default()
            },
        );
        let snapshot = Snapshot {
            entries: first
                .drafts
                .iter()
                .filter_map(|(key, draft)| entry(key, draft))
                .collect(),
        };
        first
            .draft_persistence
            .store
            .as_ref()
            .unwrap()
            .clone()
            .save(Arc::new(snapshot))
            .await
            .unwrap();
        let mut restored = app(loaded(directory.path()).await);
        assert_eq!(restored.drafts[&room].reply.as_deref(), Some("$original"));
        assert_eq!(restored.drafts[&room].content.text(), "reply draft");
        assert_eq!(
            restored.drafts[&thread].reply.as_deref(),
            Some("$thread-reply")
        );
        assert!(restored.drafts[&thread].content.text().is_empty());
        let _ = restored.update(Message::Escape);
        assert!(restored.drafts[&room].reply.is_none());
        assert!(
            restored
                .draft_persistence
                .current
                .entries
                .iter()
                .find(|entry| entry.root.is_none())
                .unwrap()
                .reply
                .is_none(),
            "Escape must also clear the persisted target"
        );
    }
    fn type_text(app: &mut App, c: char) {
        let key = app.draft_key(false).unwrap();
        let _ = app.update(Message::Edit(key, Action::Edit(Edit::Insert(c))));
    }
    async fn complete_save(app: &mut App) {
        let (request, _, _) = app.draft_persistence.pending.unwrap();
        let result = app
            .draft_persistence
            .store
            .clone()
            .unwrap()
            .save(app.draft_persistence.current.clone())
            .await;
        let _ = app.update(message(Event::Saved { request, result }));
    }

    #[tokio::test]
    async fn restart_restores_room_and_thread_text_format_and_custom_emoji_choices() {
        let directory = tempfile::tempdir().unwrap();
        let mut first = app(loaded(directory.path()).await);
        type_text(&mut first, 'a');
        let key = first.draft_key(false).unwrap();
        first.drafts.get_mut(&key).unwrap().markdown.format = Format::Plain;
        let thread = DraftKey {
            room: key.room.clone(),
            root: Some("$root".into()),
        };
        let mut reply = Draft {
            content: iced::widget::text_editor::Content::with_text(
                "reply :leaf: @maya:example.org and Maya Chen",
            ),
            ..Default::default()
        };
        let emoji = interconnect::custom_emoji::Emoji {
            id: "fixture".into(),
            shortcode: "leaf".into(),
            name: "Leaf".into(),
            pack: "Fixture".into(),
            uri: "mxc://example.org/leaf".into(),
        };
        reply.markdown.insert_emoji(&emoji, "");
        reply
            .markdown
            .mentions
            .push("@maya:example.org".parse().unwrap());
        let start = reply.content.text().find("Maya Chen").unwrap();
        reply
            .markdown
            .named_mentions
            .push(interconnect::composition::NamedMention {
                range: start..start + 9,
                target: interconnect::composition::MentionTarget::User {
                    user_id: "@maya:example.org".parse().unwrap(),
                },
            });
        reply.content.move_to(iced::advanced::text::editor::Cursor {
            position: iced::advanced::text::editor::Position {
                line: 0,
                column: start + 9,
            },
            selection: Some(iced::advanced::text::editor::Position {
                line: 0,
                column: start + 4,
            }),
        });
        reply.perform(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Delete,
        ));
        let start = reply.content.text().len();
        reply.perform(iced::widget::text_editor::Action::Move(
            iced::widget::text_editor::Motion::DocumentEnd,
        ));
        reply.perform(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Paste(Arc::new(" #Room".into())),
        ));
        reply
            .markdown
            .named_mentions
            .push(interconnect::composition::NamedMention {
                range: start + 1..start + 6,
                target: interconnect::composition::MentionTarget::Room {
                    room_id: "!room:example.org".parse().unwrap(),
                    via: vec!["route.example.org".parse().unwrap()],
                },
            });
        let start = reply.content.text().len() + 1;
        reply.perform(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Paste(Arc::new(" @room".into())),
        ));
        reply
            .markdown
            .named_mentions
            .push(interconnect::composition::NamedMention {
                range: start..start + 5,
                target: interconnect::composition::MentionTarget::RoomWide { room_wide: () },
            });
        first.drafts.insert(thread.clone(), reply);
        first.capture_persistent_drafts();
        let expected = first.draft_persistence.current.clone();
        let _ = first.save_drafts();
        complete_save(&mut first).await;
        assert!(!first.draft_persistence.dirty());
        let restored = app(loaded(directory.path()).await);
        assert_eq!(
            restored.drafts[&key].content.text(),
            first.drafts[&key].content.text()
        );
        assert_eq!(restored.drafts[&key].markdown.format, Format::Plain);
        assert_eq!(restored.drafts[&thread].markdown.emoji[0].emoji, emoji);
        assert_eq!(
            restored.drafts[&thread].markdown.mentions[0].as_str(),
            "@maya:example.org"
        );
        assert_eq!(
            restored.drafts[&thread].markdown.named_mentions[0]
                .target
                .user_id()
                .unwrap()
                .as_str(),
            "@maya:example.org"
        );
        let range = restored.drafts[&thread].markdown.named_mentions[0]
            .range
            .clone();
        assert_eq!(&restored.drafts[&thread].content.text()[range], "Maya");
        assert_eq!(restored.drafts[&thread].markdown.named_mentions.len(), 3);
        assert_eq!(
            restored.drafts[&thread].markdown.named_mentions[1]
                .target
                .room_id()
                .unwrap()
                .as_str(),
            "!room:example.org"
        );
        assert!(
            restored.drafts[&thread].markdown.named_mentions[2]
                .target
                .is_room_wide()
        );
        assert_eq!(*restored.draft_persistence.current, *expected);
    }

    #[tokio::test]
    async fn edits_coalesce_and_late_saves_never_mark_newer_text_durable() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(loaded(directory.path()).await);
        type_text(&mut app, 'a');
        let first = app.draft_persistence.revision;
        type_text(&mut app, 'b');
        let current = app.draft_persistence.revision;
        assert!(current > first);
        let id = app.draft_persistence.store.as_ref().unwrap().id();
        let _ = app.update(message(Event::Due(id, first)));
        assert!(app.draft_persistence.pending.is_none());
        let _ = app.update(message(Event::Due(id, current)));
        let pending = app.draft_persistence.pending.unwrap();
        assert_eq!(app.pending_ops, 1);
        type_text(&mut app, 'c');
        assert_eq!(app.draft_persistence.pending, Some(pending));
        let _ = app.update(message(Event::Saved {
            request: pending.0,
            result: Ok(()),
        }));
        assert!(app.draft_persistence.dirty());
        assert_eq!(app.pending_ops, 0);
        let _ = app.save_drafts();
        complete_save(&mut app).await;
        assert!(!app.draft_persistence.dirty());
        assert!(app.draft_persistence.timer.is_none());
    }

    #[tokio::test]
    async fn cleared_or_queued_drafts_are_removed_while_failed_sends_retain_text() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(loaded(directory.path()).await);
        type_text(&mut app, 'a');
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().pending = true;
        app.pending_ops = 1;
        let account = app.account_id.clone();
        let _ = app.update(Message::Sent(
            key.clone(),
            account.clone(),
            Err("fixture failure".into()),
        ));
        assert!(!app.draft_persistence.current.entries.is_empty());
        app.drafts.get_mut(&key).unwrap().pending = true;
        app.pending_ops = 1;
        let _ = app.update(Message::Sent(key, account, Ok(())));
        assert!(app.draft_persistence.current.entries.is_empty());
    }

    #[tokio::test]
    async fn close_flushes_before_shutdown_and_failures_require_an_explicit_choice() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(loaded(directory.path()).await);
        type_text(&mut app, 'x');
        let _ = app.update(Message::Close);
        assert!(app.draft_persistence.close_requested);
        assert!(!app.closing);
        assert!(!app.selection.shutdown);
        let (request, _, _) = app.draft_persistence.pending.unwrap();
        let _ = app.update(message(Event::Saved {
            request,
            result: Err("disk full".into()),
        }));
        assert!(app.draft_persistence.close_requested);
        assert!(!app.closing);
        let _ = app.update(message(Event::CancelClose));
        assert!(!app.draft_persistence.close_requested);
        let _ = app.update(Message::Close);
        let _ = app.update(message(Event::Retry));
        complete_save(&mut app).await;
        assert!(app.closing);
        assert!(app.selection.shutdown);
        assert!(!app.draft_persistence.close_requested);
    }

    #[tokio::test]
    async fn sign_out_waits_for_draft_save_and_a_failed_sign_out_can_still_close_safely() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(loaded(directory.path()).await);
        type_text(&mut app, 'x');
        let _ = app.update(Message::SignOut(crate::sign_out::Event::Open));
        assert!(app.sign_out_open);
        assert!(app.draft_persistence.pending.is_some());
        let _ = app.update(Message::SignOut(crate::sign_out::Event::Confirm));
        assert_eq!(app.selection.sign_out, 0);
        complete_save(&mut app).await;
        let _ = app.update(Message::SignOut(crate::sign_out::Event::Confirm));
        assert_ne!(app.selection.sign_out, 0);
        app.model.sign_out = interconnect::connected::SignOut::Failed {
            error: "offline".into(),
            revoked: false,
        };
        let _ = app.update(Message::Close);
        assert!(!app.sign_out_open);
        assert!(app.selection.shutdown);
    }

    #[tokio::test]
    async fn close_waits_for_send_completion_then_flushes_the_removed_draft() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(loaded(directory.path()).await);
        type_text(&mut app, 'x');
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().pending = true;
        app.pending_ops = 1; // Message queue admission still running.
        let _ = app.update(Message::Close);
        complete_save(&mut app).await;
        assert_eq!(app.pending_ops, 1);
        assert!(!app.closing);
        let account = app.account_id.clone();
        let _ = app.update(Message::Sent(key, account, Ok(())));
        assert!(app.draft_persistence.current.entries.is_empty());
        assert!(app.draft_persistence.pending.is_some());
        assert!(!app.selection.shutdown);
        complete_save(&mut app).await;
        assert!(app.selection.shutdown);
        assert!(loaded(directory.path()).await.saved.entries.is_empty());
    }

    #[tokio::test]
    async fn close_without_saving_requires_a_visible_failure_and_never_starts_another_write() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(loaded(directory.path()).await);
        type_text(&mut app, 'x');
        let _ = app.update(message(Event::DiscardClose));
        assert!(!app.draft_persistence.discard_close);
        let _ = app.update(Message::Close);
        let (request, _, _) = app.draft_persistence.pending.unwrap();
        let _ = app.update(message(Event::Saved {
            request,
            result: Err("disk full".into()),
        }));
        let _ = app.update(message(Event::DiscardClose));
        assert!(app.closing);
        assert!(app.selection.shutdown);
        assert!(app.draft_persistence.pending.is_none());
    }

    #[test]
    fn demo_drafts_never_schedule_storage_or_delay_exit() {
        let mut app = crate::tests::demo_app();
        type_text(&mut app, 'x');
        assert!(app.draft_persistence.timer.is_none());
        assert!(app.draft_persistence.pending.is_none());
        assert!(!app.defer_close_for_drafts());
    }
}
