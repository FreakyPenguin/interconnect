//! Native picking and per-conversation staged attachments.
use super::{App, DraftKey, Message, current_pane, shell};
use iced::{
    Element, Task,
    widget::{button, column, container, row, scrollable, text, tooltip},
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
    Drop(Target, Vec<std::path::PathBuf>),
    Paste(Target),
    Pasted(Target, Result<clipboard::Content, clipboard::Error>),
    Picked(Target, Result<Vec<Attachment>, String>),
    Remove(Target, usize),
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
            && !self.space_settings.is_open()
            && (thread
                || !self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width)))
            && self.room_can_compose(&target.key.room)
            && self.account_id == target.account
            && self.emoji_management.is_none()
            && self.settings.draft.is_none()
            && self.room_picker.is_none()
            && self.new_chat.is_none()
            && self.create_room.is_none()
            && !self.room_settings.is_open()
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
                self.user_completion.clear();
                return Task::perform(
                    async move {
                        let result = match rfd::AsyncFileDialog::new()
                            .set_title("Attach files")
                            .pick_files()
                            .await
                        {
                            Some(files) => {
                                prepare_files(
                                    files
                                        .into_iter()
                                        .map(|file| file.path().to_owned())
                                        .collect(),
                                )
                                .await
                            }
                            None => Ok(Vec::new()),
                        };
                        (target, result)
                    },
                    |(target, result)| Message::Attachment(Event::Picked(target, result)),
                );
            }
            Event::Drop(target, paths)
                if self
                    .attachment_drop_target(&target.key, target.generation)
                    .as_ref()
                    == Some(&target) =>
            {
                self.attachment_picker = Some(target.clone());
                self.emoji_completion.clear();
                self.user_completion.clear();
                return Task::perform(prepare_files(paths), move |result| {
                    Message::Attachment(Event::Picked(target.clone(), result))
                });
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
                self.user_completion.clear();
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
                let key = target.key.clone();
                self.finish_paste(target, result);
                return self.refresh_user_completion(&key);
            }
            Event::Picked(target, result) if self.attachment_picker.as_ref() == Some(&target) => {
                self.attachment_picker = None;
                if self.attachment_target_valid(&target) {
                    let draft = self.drafts.get_mut(&target.key).unwrap();
                    match result {
                        Ok(attachments) if !attachments.is_empty() => {
                            draft.attachments.extend(attachments);
                            draft.error = None;
                        }
                        Ok(_) => {}
                        Err(error) => draft.error = Some(error),
                    }
                    return iced::widget::operation::focus(if target.key.root.is_some() {
                        "thread-composer"
                    } else {
                        "room-composer"
                    });
                }
            }
            Event::Remove(target, index)
                if self.clipboard_paste.is_none() && self.attachment_target_valid(&target) =>
            {
                let draft = self.drafts.get_mut(&target.key).unwrap();
                if index < draft.attachments.len() {
                    draft.attachments.remove(index);
                }
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
            Ok(clipboard::Content::Attachments(attachments)) => {
                draft.attachments.extend(attachments);
                draft.error = None;
            }
            Ok(clipboard::Content::Attachment(attachment)) => {
                draft.attachments.push(attachment);
                draft.error = None;
            }
            Ok(clipboard::Content::Text(text)) => {
                // A slow clipboard owner must not overwrite intervening typing
                // or move the user's selection back to an obsolete caret.
                if draft.content.text() == paste.text && draft.content.cursor() == paste.cursor {
                    draft.perform(iced::widget::text_editor::Action::Edit(
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
            "Attach files (up to 50 MiB each)",
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
        if draft.attachments.is_empty() {
            return None;
        }
        let target = self.attachment_target(key, generation);
        let chips = draft
            .attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| {
                let mut remove = button(super::icons::icon(super::icons::Kind::Close))
                    .padding([2, 6])
                    .style(shell::icon_button);
                if self.clipboard_paste.is_none() && self.attachment_target_valid(&target) {
                    remove =
                        remove.on_press(Message::Attachment(Event::Remove(target.clone(), index)));
                }
                container(
                    row![
                        super::tooltips::tooltip(
                            super::single_line::label(&attachment.filename, 12),
                            &attachment.filename,
                            tooltip::Position::Top,
                        ),
                        text(format!("{} KiB", attachment.size.div_ceil(1024)))
                            .size(11)
                            .style(shell::muted_text),
                        remove
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .padding([4, 8])
                .style(|theme| container::Style {
                    border: iced::Border {
                        color: shell::palette(theme).line,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..shell::surface(theme, shell::palette(theme).toolbar)
                })
                .into()
            });
        Some(
            scrollable(column(chips).spacing(4))
                .height(iced::Length::Fixed(
                    (draft.attachments.len() as f32 * 36.0).min(108.0),
                ))
                .into(),
        )
    }
}

async fn prepare_files(paths: Vec<std::path::PathBuf>) -> Result<Vec<Attachment>, String> {
    let mut attachments = Vec::with_capacity(paths.len());
    for path in paths {
        attachments.push(Attachment::from_path(path).await?);
    }
    Ok(attachments)
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

    fn picked(app: &mut App, target: &Target, result: Result<Vec<Attachment>, String>) {
        app.attachment_picker = Some(target.clone());
        let _ = app.update(Message::Attachment(Event::Picked(target.clone(), result)));
    }

    #[tokio::test]
    async fn selections_append_remove_individually_and_send_in_order_with_one_caption() {
        for thread in [false, true] {
            let (dir, first) = fixture().await;
            let path = dir.path().join("second.txt");
            std::fs::write(&path, "second").unwrap();
            let second = Attachment::from_path(path).await.unwrap();
            let mut app = demo_app();
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let target = target(&app, thread);
            picked(&mut app, &target, Ok(vec![first.clone(), second.clone()]));
            begin_paste(&mut app, &target);
            let _ = app.update(Message::Attachment(Event::Pasted(
                target.clone(),
                Ok(clipboard::Content::Attachments(vec![first, second])),
            )));
            assert_eq!(app.drafts[&target.key].attachments.len(), 4);
            let _ = app.update(Message::Attachment(Event::Remove(target.clone(), 1)));
            let _ = app.update(Message::Attachment(Event::Remove(target.clone(), 1)));
            assert_eq!(
                app.drafts[&target.key]
                    .attachments
                    .iter()
                    .map(|a| a.filename.as_str())
                    .collect::<Vec<_>>(),
                vec!["fixture.txt", "second.txt"]
            );
            picked(&mut app, &target, Ok(vec![])); // Cancelling preserves the list.
            picked(&mut app, &target, Err("invalid selection".into()));
            assert_eq!(app.drafts[&target.key].attachments.len(), 2);
            let draft = app.drafts.get_mut(&target.key).unwrap();
            draft.content = iced::widget::text_editor::Content::with_text("caption");
            draft.reply = Some(if thread { "review-reply-1" } else { "review" }.into());
            // Demo must not read or upload files at send time.
            drop(dir);
            let _ = app.update(Message::Send(target.key.clone()));
            let pane = if thread {
                app.model.thread.as_ref()
            } else {
                app.model.room.as_ref()
            }
            .unwrap();
            let rows = &pane.rows[pane.rows.len() - 2..];
            assert_eq!(rows[0].body, "fixture.txt\ncaption");
            assert_eq!(rows[1].body, "second.txt");
            assert!(rows.iter().all(|row| row.reply.is_some()));
            assert!(app.drafts[&target.key].attachments.is_empty());
            assert_eq!(app.pending_ops, 0);
        }
    }

    #[tokio::test]
    async fn long_attachment_lists_keep_send_visible_in_small_room_and_thread_panes() {
        use iced::advanced::{Layout, layout, widget};
        #[derive(Default)]
        struct Probe(Option<iced::Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("composer-send")) {
                    self.0 = Some(bounds);
                }
            }
        }
        let (_dir, mut attachment) = fixture().await;
        attachment.filename = "Long attachment name 🌍 ".repeat(30);
        for thread in [false, true] {
            let mut app = demo_app();
            let _ = app.update(Message::Thread("review".into()));
            let target = target(&app, thread);
            picked(&mut app, &target, Ok(vec![attachment.clone(); 50]));
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
            let size = iced::Size::new(300.0, 446.0);
            let mut view = app.pane(thread);
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut probe = Probe::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            let send = probe.0.unwrap();
            assert!(send.x >= 0.0 && send.y >= 0.0);
            assert!(send.x + send.width <= size.width);
            assert!(send.y + send.height <= size.height);
        }
    }

    #[tokio::test]
    async fn batch_preparation_is_atomic_and_preserves_selection_order() {
        let (dir, _) = fixture().await;
        let first = dir.path().join("fixture.txt");
        let second = dir.path().join("second.txt");
        std::fs::write(&second, "second").unwrap();
        let prepared = prepare_files(vec![second.clone(), first.clone()])
            .await
            .unwrap();
        assert_eq!(
            prepared
                .iter()
                .map(|a| a.filename.as_str())
                .collect::<Vec<_>>(),
            vec!["second.txt", "fixture.txt"]
        );
        assert!(
            prepare_files(vec![first, dir.path().join("missing"), second])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn partial_queue_completion_retains_only_unsent_files_and_ignores_old_accounts() {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let room = target(&app, false);
        picked(&mut app, &room, Ok(vec![attachment.clone(); 3]));
        let draft = app.drafts.get_mut(&room.key).unwrap();
        draft.content = iced::widget::text_editor::Content::with_text("caption");
        draft.reply = Some("review".into());
        draft.pending = true;
        app.pending_ops = 1;
        let _ = app.update(Message::AttachmentsSent(
            room.key.clone(),
            Some("old account".into()),
            2,
            Err("stale".into()),
        ));
        assert_eq!(app.drafts[&room.key].attachments.len(), 3);
        assert_eq!(app.drafts[&room.key].content.text().trim(), "caption");
        app.pending_ops = 1;
        let _ = app.update(Message::AttachmentsSent(
            room.key.clone(),
            app.account_id.clone(),
            0,
            Err("first failed".into()),
        ));
        assert_eq!(app.drafts[&room.key].attachments.len(), 3);
        assert_eq!(app.drafts[&room.key].content.text().trim(), "caption");
        // Completion must still update the original draft after navigating away.
        let _ = app.update(Message::Room("threads".into()));
        app.pending_ops = 1;
        let _ = app.update(Message::AttachmentsSent(
            room.key.clone(),
            app.account_id.clone(),
            1,
            Err("second failed".into()),
        ));
        let draft = &app.drafts[&room.key];
        assert_eq!(draft.attachments.len(), 2);
        assert!(draft.content.text().trim().is_empty());
        assert_eq!(draft.reply.as_deref(), Some("review"));
        assert!(!draft.pending);
        assert!(draft.error.as_ref().unwrap().contains("second failed"));
        assert_eq!(app.pending_ops, 0);
        app.pending_ops = 1;
        let _ = app.update(Message::AttachmentsSent(
            room.key.clone(),
            app.account_id.clone(),
            0,
            Ok(()),
        ));
        assert!(app.drafts[&room.key].attachments.is_empty());
        assert!(app.drafts[&room.key].reply.is_none());
    }

    #[tokio::test]
    async fn room_and_thread_attachments_stay_in_their_drafts_and_demo_sends_are_local() {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let room = target(&app, false);
        picked(&mut app, &room, Ok(vec![attachment.clone()]));
        let _ = app.update(Message::Thread("$kickoff".into()));
        let thread = target(&app, true);
        picked(&mut app, &thread, Ok(vec![attachment]));
        app.drafts.get_mut(&thread.key).unwrap().content =
            iced::widget::text_editor::Content::with_text("caption");
        let _ = app.update(Message::CloseThread);
        let _ = app.update(Message::Thread("$kickoff".into()));
        assert!(!app.drafts[&room.key].attachments.is_empty());
        assert!(!app.drafts[&thread.key].attachments.is_empty());
        let _ = app.update(Message::Send(thread.key.clone()));
        assert!(app.drafts[&thread.key].attachments.is_empty());
        assert!(!app.drafts[&room.key].attachments.is_empty());
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
        assert!(app.drafts[&room.key].attachments.is_empty());
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
        picked(&mut app, &original, Ok(vec![attachment.clone()]));
        assert!(app.drafts[&original.key].attachments.is_empty());
        let current = target(&app, false);
        app.account_id = Some("@other:example.org".into());
        picked(&mut app, &current, Ok(vec![attachment.clone()]));
        assert!(app.drafts[&original.key].attachments.is_empty());
        let current = target(&app, false);
        picked(&mut app, &current, Ok(vec![attachment]));
        picked(&mut app, &current, Ok(vec![]));
        assert!(!app.drafts[&original.key].attachments.is_empty());
        let _ = app.update(Message::Attachment(Event::Remove(current, 0)));
        assert!(app.drafts[&original.key].attachments.is_empty());
    }

    #[tokio::test]
    async fn queue_failure_preserves_attachment_caption_and_other_conversations() {
        let (_dir, attachment) = fixture().await;
        let mut app = demo_app();
        let room = target(&app, false);
        picked(&mut app, &room, Ok(vec![attachment]));
        app.drafts.get_mut(&room.key).unwrap().content =
            iced::widget::text_editor::Content::with_text("keep caption");
        app.drafts.get_mut(&room.key).unwrap().pending = true;
        app.pending_ops = 1;
        let _ = app.update(Message::Attachment(Event::Remove(room.clone(), 0)));
        assert!(!app.drafts[&room.key].attachments.is_empty());
        let _ = app.update(Message::Room("threads".into()));
        let _ = app.update(Message::Sent(
            room.key.clone(),
            app.account_id.clone(),
            Err("fixture queue error".into()),
        ));
        assert!(!app.drafts[&room.key].attachments.is_empty());
        assert!(app.drafts[&room.key].error.is_some());
        assert_eq!(app.drafts[&room.key].content.text().trim(), "keep caption");
        // Completion from a previous account must not clear this account's draft.
        app.pending_ops = 1;
        let _ = app.update(Message::Sent(
            room.key.clone(),
            Some("@old:example.org".into()),
            Ok(()),
        ));
        assert!(!app.drafts[&room.key].attachments.is_empty());
        app.pending_ops = 1;
        let _ = app.update(Message::Sent(
            room.key.clone(),
            app.account_id.clone(),
            Ok(()),
        ));
        assert!(app.drafts[&room.key].attachments.is_empty());
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
        assert!(!app.drafts[&room.key].attachments.is_empty());
        assert_eq!(app.drafts[&room.key].content.text().trim(), "caption");
        let _ = app.update(Message::Thread("$kickoff".into()));
        let thread = target(&app, true);
        begin_paste(&mut app, &thread);
        let _ = app.update(Message::Attachment(Event::Pasted(
            thread.clone(),
            Ok(clipboard::Content::Attachment(attachment)),
        )));
        assert!(!app.drafts[&thread.key].attachments.is_empty());
        assert!(!app.drafts[&room.key].attachments.is_empty());
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
        assert!(app.drafts[&room.key].attachments.is_empty());
        let current = target(&app, false);
        begin_paste(&mut app, &current);
        app.account_id = Some("@different:example.org".into());
        let _ = app.update(Message::Attachment(Event::Pasted(
            current.clone(),
            Ok(clipboard::Content::Attachment(attachment.clone())),
        )));
        assert!(app.drafts[&current.key].attachments.is_empty());
        let current = target(&app, false);
        app.drafts.get_mut(&current.key).unwrap().attachments = vec![attachment];
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
        assert!(!app.drafts[&current.key].attachments.is_empty());
        begin_paste(&mut app, &current);
        let _ = app.update(Message::Attachment(Event::Pasted(
            current.clone(),
            Err(clipboard::Error::Invalid("fixture failure".into())),
        )));
        assert!(!app.drafts[&current.key].attachments.is_empty());
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
        assert!(app.drafts[&room.key].attachments.is_empty());
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
            vec!["/fixture".into()],
        )));
        assert!(app.attachment_picker.is_some());
        let _ = app.update(Message::Attachment(Event::Picked(
            room.clone(),
            Ok(vec![attachment.clone()]),
        )));
        assert!(!app.drafts[&room.key].attachments.is_empty());
        picked(&mut app, &room, Err("fixture rejection".into()));
        assert_eq!(app.drafts[&room.key].content.text().trim(), "caption");
        assert!(!app.drafts[&room.key].attachments.is_empty());
        assert_eq!(
            app.drafts[&room.key].error.as_deref(),
            Some("fixture rejection")
        );
        let _ = app.update(Message::Attachment(Event::Drop(
            room.clone(),
            vec!["/fixture".into()],
        )));
        let _ = app.update(Message::Room("threads".into()));
        let _ = app.update(Message::Attachment(Event::Picked(
            room.clone(),
            Ok(vec![attachment]),
        )));
        assert!(app.attachment_picker.is_none());
        let current = app.draft_key(false).unwrap();
        assert!(app.drafts[&current].attachments.is_empty());
        let _ = app.update(Message::Attachment(Event::Drop(
            room,
            vec!["/stale".into()],
        )));
        assert!(app.attachment_picker.is_none());
        app.window_width = Some(640.0);
        let _ = app.update(Message::Thread("$kickoff".into()));
        let key = app.draft_key(false).unwrap();
        let generation = app.selection.room.as_ref().unwrap().0;
        assert!(app.attachment_drop_target(&key, generation).is_none());
    }
}
