//! Native picking and per-conversation staged attachments.
use super::{App, DraftKey, Message, current_pane, shell};
use iced::{
    Element, Task,
    widget::{button, container, row, text, tooltip},
};
use interconnect::attachment::Attachment;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    key: DraftKey,
    generation: u64,
    account: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Pick(Target),
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
            && (thread
                || !self
                    .window_width
                    .is_some_and(|width| self.thread_replaces_room(width)))
            && self.account_id == target.account
            && self.settings.draft.is_none()
            && !self.verification.open
            && self.image_view.is_none()
            && self.reaction_picker.is_none()
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
                .is_some_and(|draft| !draft.pending)
    }

    pub(super) fn attachment_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Pick(target)
                if self.attachment_picker.is_none()
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
            Event::Remove(target) if self.attachment_target_valid(&target) => {
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
            container(
                text("📎")
                    .font(super::emoji::font().unwrap_or_default())
                    .size(16),
            )
            .center_x(26)
            .center_y(26),
        )
        .padding(0)
        .style(shell::icon_button);
        if self.attachment_picker.is_none()
            && !self.composing
            && self.attachment_target_valid(&target)
        {
            pick = pick.on_press(Message::Attachment(Event::Pick(target)));
        }
        tooltip(
            container(pick).id("composer-attachment"),
            container(text("Attach a file (up to 50 MiB)").size(12))
                .padding(8)
                .style(container::rounded_box),
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
        let mut remove = button(text("×").size(16))
            .padding([2, 6])
            .style(shell::icon_button);
        if self.attachment_target_valid(&target) {
            remove = remove.on_press(Message::Attachment(Event::Remove(target)));
        }
        Some(
            container(
                row![
                    text(format!(
                        "{} · {} KiB",
                        attachment.filename,
                        attachment.size.div_ceil(1024)
                    ))
                    .size(12)
                    .width(iced::Length::Fill),
                    remove
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .padding([4, 8])
            .style(container::rounded_box)
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
}
