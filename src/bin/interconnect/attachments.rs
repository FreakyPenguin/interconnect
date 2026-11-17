//! Native picking and per-conversation staged attachments.
use super::{App, DraftKey, Message, current_pane, shell};
use iced::{
    Element, Task,
    widget::{button, container, row, text, tooltip},
};
use interconnect::{attachment::Attachment, clipboard};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    key: DraftKey,
    generation: u64,
    account: Option<String>,
}

pub struct Paste {
    target: Target,
    text: String,
    cursor: iced::advanced::text::editor::Cursor,
}

#[derive(Clone, Debug)]
pub enum Event {
    Pick(Target),
    Drop(Target, std::path::PathBuf),
    DropRejected(Target),
    Paste(Target),
    Pasted(Target, Result<clipboard::Content, clipboard::Error>),
    Picked(Target, Result<Option<Attachment>, String>),
    Remove(Target),
}

impl App {
    fn attachment_target_valid(&self, target: &Target) -> bool {
        let thread = target.key.root.is_some();
        let selection = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let pane = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        !self.closing
            && !self.stopped
            && !self.bookmarks.open
            && !self.global_search.open
            && (thread
                || !self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width)))
            && self.account_id == target.account
            && self.emoji_management.is_none()
            && self.settings.draft.is_none()
            && self.room_picker.is_none()
            && self.new_chat.is_none()
            && self.create_room.is_none()
            && !self.verification.open
            && self.image_view.is_none()
            && self.reaction_picker.is_none()
            && self.context_menu.is_none()
            && !self.section_dragging()
            && self.composer_picker.is_none()
            && self.draft_key(thread).as_ref() == Some(&target.key)
            && selection
                .as_ref()
                .is_some_and(|(generation, _)| *generation == target.generation)
            && current_pane(pane, target.generation).is_some_and(|pane| {
                pane.error.is_none() && (pane.composer.is_some() || self.demo.is_some())
            })
            && self
                .drafts
                .get(&target.key)
                .is_some_and(|draft| !draft.pending && draft.editing.is_none())
    }

    pub(super) fn attachment_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Pick(target)
                if self.attachment_picker.is_none()
                    && self.clipboard_paste.is_none()
                    && !self.composing
                    && self.attachment_target_valid(&target) =>
            {
                self.attachment_picker = Some(target.clone());
                self.emoji_completion.clear();
                return Task::perform(
                    async move {
                        let result = match rfd::AsyncFileDialog::new()
                            .set_title("Attach a file")
                            .pick_file()
                            .await
                        {
                            Some(file) => Attachment::from_path(file.path().to_owned())
                                .await
                                .map(Some),
                            None => Ok(None),
                        };
                        (target, result)
                    },
                    |(target, result)| Message::Attachment(Event::Picked(target, result)),
                );
            }
            Event::Drop(target, path)
                if self
                    .attachment_drop_target(&target.key, target.generation)
                    .as_ref()
                    == Some(&target) =>
            {
                self.attachment_picker = Some(target.clone());
                self.emoji_completion.clear();
                return Task::perform(Attachment::from_path(path), move |result| {
                    Message::Attachment(Event::Picked(target.clone(), result.map(Some)))
                });
            }
            Event::DropRejected(target)
                if self
                    .attachment_drop_target(&target.key, target.generation)
                    .as_ref()
                    == Some(&target) =>
            {
                self.drafts.get_mut(&target.key).unwrap().error =
                    Some("Drop one file at a time.".into());
            }
            Event::Paste(target)
                if self.attachment_picker.is_none()
                    && self.clipboard_paste.is_none()
                    && !self.composing
                    && self.attachment_target_valid(&target) =>
            {
                let draft = &self.drafts[&target.key];
                self.clipboard_paste = Some(Paste {
                    target: target.clone(),
                    text: draft.content.text(),
                    cursor: draft.content.cursor(),
                });
                self.emoji_completion.clear();
                return Task::perform(clipboard::read(), move |result| {
                    Message::Attachment(Event::Pasted(target.clone(), result))
                });
            }
            Event::Pasted(target, result)
                if self
                    .clipboard_paste
                    .as_ref()
                    .is_some_and(|paste| paste.target == target) =>
            {
                if self.attachment_target_valid(&target)
                    && matches!(result, Err(clipboard::Error::Unavailable))
                {
                    // Preserve normal text paste when the window clipboard works
                    // but the native file/image provider is unavailable.
                    return iced::clipboard::read().map(move |text| {
                        Message::Attachment(Event::Pasted(
                            target.clone(),
                            text.map(clipboard::Content::Text).ok_or_else(|| {
                                clipboard::Error::Invalid(
                                    "Could not read clipboard files or images.".into(),
                                )
                            }),
                        ))
                    });
                }
                self.finish_paste(target, result);
            }
            Event::Picked(target, result) if self.attachment_picker.as_ref() == Some(&target) => {
                self.attachment_picker = None;
                if self.attachment_target_valid(&target) {
                    let draft = self.drafts.get_mut(&target.key).unwrap();
                    match result {
                        Ok(Some(attachment)) => {
                            draft.attachment = Some(attachment);
                            draft.error = None;
                        }
                        Ok(None) => {}
                        Err(error) => draft.error = Some(error),
                    }
                    return iced::widget::operation::focus(if target.key.root.is_some() {
                        "thread-composer"
                    } else {
                        "room-composer"
                    });
                }
            }
            Event::Remove(target)
                if self.clipboard_paste.is_none() && self.attachment_target_valid(&target) =>
            {
                let draft = self.drafts.get_mut(&target.key).unwrap();
                draft.attachment = None;
                draft.error = None;
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn attachment_picking(&self, key: &DraftKey) -> bool {
        self.attachment_picker
            .as_ref()
            .is_some_and(|target| &target.key == key)
            || self.clipboard_pasting(key)
    }

    pub(super) fn clipboard_pasting(&self, key: &DraftKey) -> bool {
        self.clipboard_paste
            .as_ref()
            .is_some_and(|paste| &paste.target.key == key)
    }

    fn finish_paste(
        &mut self,
        target: Target,
        result: Result<clipboard::Content, clipboard::Error>,
    ) {
        let Some(paste) = self.clipboard_paste.take() else {
            return;
        };
        if !self.attachment_target_valid(&target) || self.composing {
            return;
        }
        let draft = self.drafts.get_mut(&target.key).unwrap();
        match result {
            Ok(clipboard::Content::Attachment(attachment)) => {
                draft.attachment = Some(attachment);
                draft.error = None;
            }
            Ok(clipboard::Content::Text(text)) => {
                // A slow clipboard owner must not overwrite intervening typing
                // or move the user's selection back to an obsolete caret.
                if draft.content.text() == paste.text && draft.content.cursor() == paste.cursor {
                    draft
                        .content
                        .perform(iced::widget::text_editor::Action::Edit(
                            iced::widget::text_editor::Edit::Paste(std::sync::Arc::new(text)),
                        ));
                    draft.error = None;
                    self.typing_edited(&target.key);
                    self.refresh_emoji_completion(&target.key);
                } else {
                    draft.error = Some("Draft changed before paste completed. Paste again.".into());
                }
            }
            Ok(clipboard::Content::Empty) => {}
            Err(clipboard::Error::Invalid(error)) => draft.error = Some(error),
            Err(clipboard::Error::Unavailable) => {
                draft.error = Some("Could not read the clipboard.".into())
            }
        }
    }

    pub(super) fn attachment_paste_binding(
        &self,
        key: &DraftKey,
        generation: u64,
        press: &iced::widget::text_editor::KeyPress,
    ) -> Option<iced::widget::text_editor::Binding<Message>> {
        use iced::widget::text_editor::Binding;
        if self.composing || !self.attachment_target_valid(&self.attachment_target(key, generation))
        {
            return None;
        }
        matches!(
            Binding::<Message>::from_key_press(press.clone()),
            Some(Binding::Paste)
        )
        .then(|| {
            Binding::Custom(Message::Attachment(Event::Paste(
                self.attachment_target(key, generation),
            )))
        })
    }

    pub(super) fn attachment_drop_target(&self, key: &DraftKey, generation: u64) -> Option<Target> {
        let target = self.attachment_target(key, generation);
        (!self.composing
            && self.attachment_picker.is_none()
            && self.clipboard_paste.is_none()
            && self.attachment_target_valid(&target))
        .then_some(target)
    }

    fn attachment_target(&self, key: &DraftKey, generation: u64) -> Target {
        Target {
            key: key.clone(),
            generation,
            account: self.account_id.clone(),
        }
    }

    pub(super) fn attachment_button(
        &self,
        key: &DraftKey,
        generation: u64,
    ) -> Element<'_, Message> {
        let target = self.attachment_target(key, generation);
        let mut pick = button(
            container(super::icons::icon(super::icons::Kind::Attach))
                .center_x(28)
                .center_y(28),
        )
        .padding(0)
        .style(shell::icon_button);
        if self.attachment_picker.is_none()
            && self.clipboard_paste.is_none()
            && !self.composing
            && self.attachment_target_valid(&target)
        {
            pick = pick.on_press(Message::Attachment(Event::Pick(target)));
        }
        super::tooltips::tooltip(
            container(pick).id("composer-attachment"),
            "Attach a file (up to 50 MiB)",
            tooltip::Position::Top,
        )
        .into()
    }

    pub(super) fn attachment_chip(
        &self,
        key: &DraftKey,
        generation: u64,
    ) -> Option<Element<'_, Message>> {
        let draft = self.drafts.get(key)?;
        let attachment = draft.attachment.as_ref()?;
        let target = self.attachment_target(key, generation);
        let mut remove = button(super::icons::icon(super::icons::Kind::Close))
            .padding([2, 6])
            .style(shell::icon_button);
        if self.clipboard_paste.is_none() && self.attachment_target_valid(&target) {
            remove = remove.on_press(Message::Attachment(Event::Remove(target)));
        }
        Some(
            container(
                row![
                    super::tooltips::tooltip(
                        super::single_line::label(&attachment.filename, 12),
                        &attachment.filename,
                        tooltip::Position::Top,
                    ),
                    text(format!("{} KiB", attachment.size.div_ceil(1024)))
                        .size(11)
                        .color(shell::MUTED),
                    remove
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .padding([4, 8])
            .style(|_| container::Style {
                border: iced::Border {
                    color: shell::LINE,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..shell::surface(iced::Color::from_rgb8(248, 249, 251))
            })
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::demo_app;
    use super::*;

    async fn fixture() -> (tempfile::TempDir, Attachment) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.txt");
        std::fs::write(&path, "local attachment fixture").unwrap();
        let attachment = Attachment::from_path(path).await.unwrap();
        (dir, attachment)
    }

    fn target(app: &App, thread: bool) -> Target {
        let key = app.draft_key(thread).unwrap();
        let generation = if thread {
            app.selection.thread.as_ref().unwrap().0
        } else {
            app.selection.room.as_ref().unwrap().0
        };
        app.attachment_target(&key, generation)
    }

    fn picked(app: &mut App, target: &Target, result: Result<Option<Attachment>, String>) {
        app.attachment_picker = Some(target.clone());
        let _ = app.update(Message::Attachment(Event::Picked(target.clone(), result)));
    }

    #[tokio::test]
    async fn room_and_thread_attachments_stay_in_their_drafts_and_demo_sends_are_local() {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let room = target(&app, false);
        picked(&mut app, &room, Ok(Some(attachment.clone())));
        let _ = app.update(Message::Thread("$kickoff".into()));
        let thread = target(&app, true);
        picked(&mut app, &thread, Ok(Some(attachment)));
        app.drafts.get_mut(&thread.key).unwrap().content =
            iced::widget::text_editor::Content::with_text("caption");
        let _ = app.update(Message::CloseThread);
        let _ = app.update(Message::Thread("$kickoff".into()));
        assert!(app.drafts[&room.key].attachment.is_some());
        assert!(app.drafts[&thread.key].attachment.is_some());
        let _ = app.update(Message::Send(thread.key.clone()));
        assert!(app.drafts[&thread.key].attachment.is_none());
        assert!(app.drafts[&room.key].attachment.is_some());
        assert!(
            app.model
                .thread
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.body == "fixture.txt\ncaption"
                    && row.thread_root.as_deref() == Some("$kickoff"))
        );
        let _ = app.update(Message::Send(room.key.clone()));
        assert!(app.drafts[&room.key].attachment.is_none());
        assert!(
            app.model
                .room
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.body == "fixture.txt" && row.thread_root.is_none())
        );
        assert_eq!(app.pending_ops, 0);
    }

    #[tokio::test]
    async fn picker_rejects_navigation_and_account_changes_and_cancel_preserves_draft() {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let original = target(&app, false);
        let _ = app.update(Message::Room("threads".into()));
        let _ = app.update(Message::Room(original.key.room.clone()));
        picked(&mut app, &original, Ok(Some(attachment.clone())));
        assert!(app.drafts[&original.key].attachment.is_none());
        let current = target(&app, false);
        app.account_id = Some("@other:example.org".into());
        picked(&mut app, &current, Ok(Some(attachment.clone())));
        assert!(app.drafts[&original.key].attachment.is_none());
        let current = target(&app, false);
        picked(&mut app, &current, Ok(Some(attachment)));
        picked(&mut app, &current, Ok(None));
        assert!(app.drafts[&original.key].attachment.is_some());
        let _ = app.update(Message::Attachment(Event::Remove(current)));
        assert!(app.drafts[&original.key].attachment.is_none());
    }

    #[tokio::test]
    async fn queue_failure_preserves_attachment_caption_and_other_conversations() {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let room = target(&app, false);
        picked(&mut app, &room, Ok(Some(attachment)));
        app.drafts.get_mut(&room.key).unwrap().content =
            iced::widget::text_editor::Content::with_text("keep caption");
        app.drafts.get_mut(&room.key).unwrap().pending = true;
        app.pending_ops = 1;
        let _ = app.update(Message::Attachment(Event::Remove(room.clone())));
        assert!(app.drafts[&room.key].attachment.is_some());
        let _ = app.update(Message::Room("threads".into()));
        let _ = app.update(Message::Sent(
            room.key.clone(),
            app.account_id.clone(),
            Err("fixture queue error".into()),
        ));
        assert!(app.drafts[&room.key].attachment.is_some());
        assert!(app.drafts[&room.key].error.is_some());
        assert_eq!(app.drafts[&room.key].content.text().trim(), "keep caption");
        // Completion from a previous account must not clear this account's draft.
        app.pending_ops = 1;
        let _ = app.update(Message::Sent(
            room.key.clone(),
            Some("@old:example.org".into()),
            Ok(()),
        ));
        assert!(app.drafts[&room.key].attachment.is_some());
        app.pending_ops = 1;
        let _ = app.update(Message::Sent(
            room.key.clone(),
            app.account_id.clone(),
            Ok(()),
        ));
        assert!(app.drafts[&room.key].attachment.is_none());
        assert!(app.drafts[&room.key].content.text().trim().is_empty());
    }
    fn begin_paste(app: &mut App, target: &Target) {
        // Drop the task without polling: unit tests never access the OS clipboard.
        let _ = app.update(Message::Attachment(Event::Paste(target.clone())));
        assert!(app.clipboard_paste.is_some());
    }

    #[tokio::test]
    async fn pasted_media_is_staged_in_only_its_room_or_thread_and_keeps_the_caption() {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let room = target(&app, false);
        app.drafts.get_mut(&room.key).unwrap().content =
            iced::widget::text_editor::Content::with_text("caption");
        begin_paste(&mut app, &room);
        let _ = app.update(Message::Send(room.key.clone()));
        assert_eq!(app.drafts[&room.key].content.text().trim(), "caption");
        let _ = app.update(Message::Attachment(Event::Pasted(
            room.clone(),
            Ok(clipboard::Content::Attachment(attachment.clone())),
        )));
        assert!(app.clipboard_paste.is_none());
        assert!(app.drafts[&room.key].attachment.is_some());
        assert_eq!(app.drafts[&room.key].content.text().trim(), "caption");
        let _ = app.update(Message::Thread("$kickoff".into()));
        let thread = target(&app, true);
        begin_paste(&mut app, &thread);
        let _ = app.update(Message::Attachment(Event::Pasted(
            thread.clone(),
            Ok(clipboard::Content::Attachment(attachment)),
        )));
        assert!(app.drafts[&thread.key].attachment.is_some());
        assert!(app.drafts[&room.key].attachment.is_some());
        assert_eq!(app.pending_ops, 0);
    }

    #[tokio::test]
    async fn paste_rejects_navigation_accounts_and_late_text_but_keeps_existing_attachments_on_error()
     {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let room = target(&app, false);
        begin_paste(&mut app, &room);
        let _ = app.update(Message::Room("threads".into()));
        let _ = app.update(Message::Attachment(Event::Pasted(
            room.clone(),
            Ok(clipboard::Content::Attachment(attachment.clone())),
        )));
        assert!(app.drafts[&room.key].attachment.is_none());
        let current = target(&app, false);
        begin_paste(&mut app, &current);
        app.account_id = Some("@different:example.org".into());
        let _ = app.update(Message::Attachment(Event::Pasted(
            current.clone(),
            Ok(clipboard::Content::Attachment(attachment.clone())),
        )));
        assert!(app.drafts[&current.key].attachment.is_none());
        let current = target(&app, false);
        app.drafts.get_mut(&current.key).unwrap().attachment = Some(attachment);
        begin_paste(&mut app, &current);
        let _ = app.update(Message::Edit(
            current.key.clone(),
            iced::widget::text_editor::Action::Edit(iced::widget::text_editor::Edit::Insert('x')),
        ));
        let _ = app.update(Message::Attachment(Event::Pasted(
            current.clone(),
            Ok(clipboard::Content::Text("old paste".into())),
        )));
        assert_eq!(app.drafts[&current.key].content.text().trim(), "x");
        assert!(app.drafts[&current.key].attachment.is_some());
        begin_paste(&mut app, &current);
        let _ = app.update(Message::Attachment(Event::Pasted(
            current.clone(),
            Err(clipboard::Error::Invalid("fixture failure".into())),
        )));
        assert!(app.drafts[&current.key].attachment.is_some());
        assert_eq!(
            app.drafts[&current.key].error.as_deref(),
            Some("fixture failure")
        );
    }

    #[test]
    fn text_paste_replaces_selection_and_native_failure_uses_window_clipboard() {
        use iced::advanced::text::editor::{Cursor, Position};
        let mut app = demo_app();
        let room = target(&app, false);
        let draft = app.drafts.get_mut(&room.key).unwrap();
        draft.content = iced::widget::text_editor::Content::with_text("hello world");
        draft.content.move_to(Cursor {
            position: Position {
                line: 0,
                column: 11,
            },
            selection: Some(Position { line: 0, column: 6 }),
        });
        begin_paste(&mut app, &room);
        let _ = app.update(Message::Attachment(Event::Pasted(
            room.clone(),
            Err(clipboard::Error::Unavailable),
        )));
        assert!(app.clipboard_paste.is_some());
        let _ = app.update(Message::Attachment(Event::Pasted(
            room.clone(),
            Ok(clipboard::Content::Text("🌍\nnext line".into())),
        )));
        assert_eq!(
            app.drafts[&room.key].content.text().trim_end(),
            "hello 🌍\nnext line"
        );
        assert!(app.drafts[&room.key].attachment.is_none());
        assert!(app.clipboard_paste.is_none());
    }

    #[test]
    fn paste_shortcut_requires_composer_focus_and_respects_ime() {
        use iced::widget::text_editor::{Binding, KeyPress, Status};
        let mut app = demo_app();
        let room = target(&app, false);
        let mut press = KeyPress {
            key: iced::keyboard::Key::Character("v".into()),
            modified_key: iced::keyboard::Key::Character("v".into()),
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyV),
            modifiers: iced::keyboard::Modifiers::COMMAND,
            text: None,
            status: Status::Focused { is_hovered: false },
        };
        assert!(matches!(
            app.attachment_paste_binding(&room.key, room.generation, &press),
            Some(Binding::Custom(Message::Attachment(Event::Paste(_))))
        ));
        app.composing = true;
        assert!(
            app.attachment_paste_binding(&room.key, room.generation, &press)
                .is_none()
        );
        app.composing = false;
        press.status = Status::Active;
        assert!(
            app.attachment_paste_binding(&room.key, room.generation, &press)
                .is_none()
        );
    }
    #[test]
    fn real_room_and_thread_editors_route_paste_once_without_reading_text_synchronously() {
        use iced::advanced::{
            Layout, Shell, layout, mouse,
            widget::{self, Tree},
        };
        for thread in [false, true] {
            let mut app = demo_app();
            let _ = app.update(Message::Thread("$kickoff".into()));
            let expected = target(&app, thread);
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let size = iced::Size::new(600.0, 700.0);
            let mut pane = app.pane(thread);
            let mut tree = Tree::new(&pane);
            let node =
                pane.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            pane.as_widget_mut().operate(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus(
                    if thread {
                        "thread-composer"
                    } else {
                        "room-composer"
                    }
                    .into(),
                ),
            );
            let event = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character("v".into()),
                modified_key: iced::keyboard::Key::Character("v".into()),
                physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyV),
                modifiers: iced::keyboard::Modifiers::COMMAND,
                location: iced::keyboard::Location::Standard,
                text: None,
                repeat: false,
            });
            let mut messages = Vec::new();
            pane.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
            drop(pane);
            assert_eq!(messages.iter().filter(|message| matches!(message, Message::Attachment(Event::Paste(target)) if target == &expected)).count(), 1);
            assert!(
                !messages
                    .iter()
                    .any(|message| matches!(message, Message::Edit(..) | Message::Send(_)))
            );
            for message in messages {
                let _ = app.update(message);
            }
            assert!(
                app.clipboard_paste
                    .as_ref()
                    .is_some_and(|paste| paste.target == expected)
            );
        }
    }
    #[tokio::test]
    async fn drops_stage_only_current_targets_and_preserve_caption_and_attachment_on_rejection() {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let room = target(&app, false);
        app.drafts.get_mut(&room.key).unwrap().content =
            iced::widget::text_editor::Content::with_text("caption");
        let _ = app.update(Message::Attachment(Event::Drop(
            room.clone(),
            "/fixture".into(),
        )));
        assert!(app.attachment_picker.is_some());
        let _ = app.update(Message::Attachment(Event::Picked(
            room.clone(),
            Ok(Some(attachment.clone())),
        )));
        assert!(app.drafts[&room.key].attachment.is_some());
        let _ = app.update(Message::Attachment(Event::DropRejected(room.clone())));
        assert_eq!(app.drafts[&room.key].content.text().trim(), "caption");
        assert!(app.drafts[&room.key].attachment.is_some());
        assert_eq!(
            app.drafts[&room.key].error.as_deref(),
            Some("Drop one file at a time.")
        );
        let _ = app.update(Message::Attachment(Event::Drop(
            room.clone(),
            "/fixture".into(),
        )));
        let _ = app.update(Message::Room("threads".into()));
        let _ = app.update(Message::Attachment(Event::Picked(
            room.clone(),
            Ok(Some(attachment)),
        )));
        assert!(app.attachment_picker.is_none());
        let current = app.draft_key(false).unwrap();
        assert!(app.drafts[&current].attachment.is_none());
        let _ = app.update(Message::Attachment(Event::Drop(room, "/stale".into())));
        assert!(app.attachment_picker.is_none());
        app.window_width = Some(640.0);
        let _ = app.update(Message::Thread("$kickoff".into()));
        let key = app.draft_key(false).unwrap();
        let generation = app.selection.room.as_ref().unwrap().0;
        assert!(app.attachment_drop_target(&key, generation).is_none());
    }
}
