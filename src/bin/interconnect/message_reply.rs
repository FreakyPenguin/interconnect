//! Reply intent belongs to each composer, alongside its unsent text and media.
use super::{App, DraftKey, Message, MessageRow, current_pane, reaction_picker, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, text, text_editor},
};
use interconnect::connected::ReplyPreview;

#[derive(Clone, Debug)]
pub enum Event {
    Start(reaction_picker::Target),
    Cancel(DraftKey),
}
fn message(event: Event) -> Message {
    Message::Replying(event)
}
impl App {
    pub(super) fn can_reply(&self, target: &reaction_picker::Target) -> bool {
        self.context_menu_allowed()
            && !self.composing
            && self.emoji_management.is_none()
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.space_settings.is_open()
            && self
                .message_row(target)
                .is_some_and(|row| row.kind == super::RowKind::Message && row.redactable)
            && self
                .draft_key(target.thread)
                .and_then(|key| self.drafts.get(&key))
                .is_some_and(|draft| !draft.pending && draft.editing.is_none())
    }
    pub(super) fn reply_action(
        &self,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Message> {
        let target = reaction_picker::Target {
            room: self.selection.room.as_ref()?.1.clone(),
            thread,
            generation,
            event: row.event_id.clone()?,
        };
        self.can_reply(&target)
            .then(|| message(Event::Start(target)))
    }
    pub(super) fn message_reply_event(&mut self, event: Event) -> Task<Message> {
        let key = match event {
            Event::Start(target) if self.can_reply(&target) => {
                let key = self.draft_key(target.thread).unwrap();
                self.drafts.get_mut(&key).unwrap().reply = Some(target.event);
                key
            }
            Event::Cancel(key)
                if self.context_menu_allowed()
                    && !self.composing
                    && (self.draft_key(false).as_ref() == Some(&key)
                        || self.draft_key(true).as_ref() == Some(&key))
                    && self
                        .drafts
                        .get(&key)
                        .is_some_and(|draft| !draft.pending && draft.editing.is_none()) =>
            {
                self.drafts.get_mut(&key).unwrap().reply = None;
                key
            }
            _ => return Task::none(),
        };
        self.emoji_completion.clear();
        self.user_completion.clear();
        iced::widget::operation::focus(if key.root.is_some() {
            "thread-composer"
        } else {
            "room-composer"
        })
    }
    fn reply_row(&self, thread: bool, event: &str) -> Option<&MessageRow> {
        // Prefer current rows to embedded context: edits and redactions must be
        // reflected immediately, without retaining an old quote in a draft.
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
        let (generation, _) = selection.as_ref()?;
        current_pane(pane, *generation)?
            .rows
            .iter()
            .find(|row| row.event_id.as_deref() == Some(event))
            .or_else(|| {
                thread
                    .then(|| self.thread_original_in_room())
                    .flatten()
                    .filter(|row| row.event_id.as_deref() == Some(event))
            })
    }
    pub(super) fn reply_banner(&self, key: &DraftKey) -> Option<Element<'_, Message>> {
        let draft = self.drafts.get(key)?;
        let event = draft.reply.as_deref()?;
        let preview = self
            .reply_row(key.root.is_some(), event)
            .and_then(ReplyPreview::from_row)
            .unwrap_or_else(|| ReplyPreview {
                event: event.to_owned(),
                sender: None,
                body: "Original message unavailable".into(),
            });
        Some(
            container(
                row![
                    column![
                        text(preview.sender.map_or_else(
                            || "Replying to message".into(),
                            |name| format!("Replying to {name}")
                        ))
                        .size(12)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .width(Length::Fill)
                        .style(shell::muted_text),
                        text(preview.body)
                            .size(12)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .width(Length::Fill)
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button(text("Cancel").size(12))
                        .padding([2, 0])
                        .style(shell::quiet_button)
                        .on_press_maybe(
                            (!draft.pending).then(|| message(Event::Cancel(key.clone())))
                        ),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .clip(true)
            .padding([6, 12])
            .into(),
        )
    }
    pub(super) fn reply_context(
        &self,
        row: &MessageRow,
        thread: bool,
    ) -> Option<Element<'_, Message>> {
        let preview = row.reply.as_ref()?;
        let preview = self
            .reply_row(thread, &preview.event)
            .and_then(ReplyPreview::from_row)
            .unwrap_or_else(|| preview.clone());
        Some(
            container(
                column![
                    text(
                        preview
                            .sender
                            .map_or_else(|| "↪ Reply".into(), |name| format!("↪ {name}"))
                    )
                    .size(12)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill)
                    .style(shell::muted_text),
                    text(preview.body)
                        .size(12)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .width(Length::Fill)
                        .style(shell::muted_text),
                ]
                .spacing(2),
            )
            .clip(true)
            .padding([4, 8])
            .style(|theme| {
                let mut style = shell::composer_toolbar(theme);
                style.border = iced::Border {
                    color: shell::palette(theme).accent.scale_alpha(0.4),
                    width: 1.0,
                    radius: 4.0.into(),
                };
                style
            })
            .into(),
        )
    }
    pub(super) fn reply_binding(
        &self,
        key: &DraftKey,
        press: &text_editor::KeyPress,
    ) -> Option<text_editor::Binding<Message>> {
        use iced::keyboard::{Key, key::Named};
        (!self.composing
            && matches!(press.status, text_editor::Status::Focused { .. })
            && press.modifiers.is_empty()
            && press.key == Key::Named(Named::Escape)
            && self.drafts.get(key).is_some_and(|draft| {
                draft.reply.is_some() && draft.editing.is_none() && !draft.pending
            }))
        .then(|| text_editor::Binding::Custom(message(Event::Cancel(key.clone()))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    fn target(app: &App, thread: bool, event: &str) -> reaction_picker::Target {
        reaction_picker::Target {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            thread,
            event: event.into(),
            generation: if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            },
        }
    }
    #[tokio::test]
    async fn replies_preserve_text_and_stay_with_room_and_thread_drafts() {
        let mut app = demo_app();
        let room = app.draft_key(false).unwrap();
        app.drafts.get_mut(&room).unwrap().content =
            text_editor::Content::with_text("unsent draft");
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "reply attachment fixture").unwrap();
        app.drafts.get_mut(&room).unwrap().attachments = vec![
            interconnect::attachment::Attachment::from_path(file.path().to_owned())
                .await
                .unwrap(),
        ];
        let filename = app.drafts[&room]
            .attachments
            .first()
            .unwrap()
            .filename
            .clone();
        let original = target(&app, false, "review");
        let _ = app.update(message(Event::Start(original.clone())));
        assert_eq!(app.drafts[&room].reply.as_deref(), Some("review"));
        assert_eq!(app.drafts[&room].content.text(), "unsent draft");
        let _ = app.update(Message::Thread("review".into()));
        let thread = app.draft_key(true).unwrap();
        let reply = target(&app, true, "review-reply-1");
        let _ = app.update(message(Event::Start(reply)));
        assert_eq!(app.drafts[&thread].reply.as_deref(), Some("review-reply-1"));
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.draft_key(false).as_ref(), Some(&room));
        assert_eq!(app.drafts[&room].reply.as_deref(), Some("review"));
        let _ = app.update(Message::Escape);
        assert!(app.drafts[&thread].reply.is_none());
        assert!(
            app.selection.thread.is_some(),
            "Escape cancels the reply before closing its thread"
        );
        let _ = app.update(Message::CloseThread);
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(message(Event::Start(original.clone())));
        assert!(
            app.drafts[&app.draft_key(false).unwrap()].reply.is_none(),
            "old row targets are rejected"
        );
        let _ = app.update(Message::Room("studio".into()));
        assert_eq!(app.drafts[&room].reply.as_deref(), Some("review"));
        let _ = app.update(message(Event::Cancel(room.clone())));
        assert_eq!(app.drafts[&room].content.text(), "unsent draft");
        assert!(app.drafts[&room].reply.is_none());
        assert_eq!(
            app.drafts[&room].attachments.first().unwrap().filename,
            filename
        );
    }
    #[test]
    fn demo_send_is_an_ordinary_reply_and_edit_cancel_restores_reply_intent() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let original = target(&app, false, "review");
        let _ = app.update(message(Event::Start(original.clone())));
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("local answer");
        // Opening an edit temporarily replaces the entire composer, including reply intent.
        let own = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.is_own && row.edit_source.is_some())
            .unwrap()
            .event_id
            .clone()
            .unwrap();
        let own = target(&app, false, &own);
        let _ = app.update(Message::Editing(super::super::message_edit::Event::Start(
            own,
        )));
        assert!(app.drafts[&key].reply.is_none());
        let _ = app.update(message(Event::Start(original)));
        assert!(
            app.drafts[&key].reply.is_none(),
            "reply cannot replace an edit"
        );
        let _ = app.update(Message::Editing(super::super::message_edit::Event::Cancel(
            key.clone(),
        )));
        assert_eq!(app.drafts[&key].reply.as_deref(), Some("review"));
        let count = app.model.room.as_ref().unwrap().rows.len();
        let _ = app.update(Message::Send(key.clone()));
        let rows = &app.model.room.as_ref().unwrap().rows;
        assert_eq!(rows.len(), count + 1);
        let sent = rows.last().unwrap();
        assert_eq!(sent.body, "local answer");
        assert!(sent.thread_root.is_none());
        assert_eq!(sent.reply.as_ref().unwrap().event, "review");
        assert!(app.drafts[&key].reply.is_none());
        assert!(app.drafts[&key].content.text().is_empty());
        assert!(app.selection.thread.is_none());
    }
    #[test]
    fn pending_and_failed_sends_keep_reply_intent_until_admitted() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let original = target(&app, false, "review");
        let _ = app.update(message(Event::Start(original.clone())));
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("retry this");
        app.drafts.get_mut(&key).unwrap().pending = true;
        app.pending_ops = 1;
        let _ = app.update(message(Event::Cancel(key.clone())));
        assert_eq!(app.drafts[&key].reply.as_deref(), Some("review"));
        let _ = app.update(Message::Sent(
            key.clone(),
            app.account_id.clone(),
            Err("offline".into()),
        ));
        assert!(!app.drafts[&key].pending);
        assert_eq!(app.drafts[&key].reply.as_deref(), Some("review"));
        assert_eq!(app.drafts[&key].content.text(), "retry this");
        app.pending_ops = 1;
        let _ = app.update(Message::Sent(
            key.clone(),
            Some("old-account".into()),
            Ok(()),
        ));
        assert_eq!(app.drafts[&key].reply.as_deref(), Some("review"));
        app.pending_ops = 1;
        let _ = app.update(Message::Sent(key.clone(), app.account_id.clone(), Ok(())));
        assert!(app.drafts[&key].reply.is_none());
    }
    #[tokio::test]
    async fn markdown_completion_keeps_reply_on_failure_and_sends_in_thread() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let key = app.draft_key(true).unwrap();
        let original = target(&app, true, "review-reply-1");
        let _ = app.update(message(Event::Start(original)));
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("**answer**");
        let _ = app.update(Message::Send(key.clone()));
        assert!(app.drafts[&key].pending);
        let _ = app.update(Message::Markdown(
            super::super::markdown_composer::Event::DemoSent {
                key: key.clone(),
                account: app.account_id.clone(),
                result: Err("conversion failed".into()),
            },
        ));
        assert_eq!(app.drafts[&key].reply.as_deref(), Some("review-reply-1"));
        let _ = app.update(Message::Send(key.clone()));
        let prepared = interconnect::composition::prepare(
            "**answer**".into(),
            interconnect::composition::Format::Markdown,
        )
        .await
        .unwrap();
        let _ = app.update(Message::Markdown(
            super::super::markdown_composer::Event::DemoSent {
                key: key.clone(),
                account: app.account_id.clone(),
                result: Ok(prepared),
            },
        ));
        let rows = &app.model.thread.as_ref().unwrap().rows;
        let sent = rows.last().unwrap();
        assert_eq!(sent.thread_root.as_deref(), Some("review"));
        assert_eq!(sent.reply.as_ref().unwrap().event, "review-reply-1");
        assert!(sent.formatted.is_some());
        assert!(app.drafts[&key].reply.is_none());
    }
    #[test]
    fn long_reply_banners_fit_and_captured_escape_is_not_dispatched_twice() {
        use iced::advanced::{
            layout,
            subscription::{self, Event as RuntimeEvent},
            widget,
        };
        use iced::futures::{FutureExt, StreamExt};
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let original = target(&app, false, "review");
        let _ = app.update(message(Event::Start(original)));
        let rows = std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows);
        let row = rows.iter_mut().find(|row| row.key == "review").unwrap();
        row.display_name = "A long sender name 👩🏽‍💻 ".repeat(100);
        row.body = "A multiline original\nsecond line ".repeat(100);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for width in [200.0, 400.0, 800.0] {
            let mut view = app.reply_banner(&key).unwrap();
            let mut tree = widget::Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 400.0)),
            );
            assert!(
                node.size().height <= 48.0,
                "banner grew at width {width}: {:?}",
                node.size()
            );
            assert!(node.size().width <= width);
        }
        let outputs: Vec<_> = subscription::into_recipes(app.subscription())
            .into_iter()
            .filter_map(|recipe| {
                let input = RuntimeEvent::Interaction {
                    window: iced::window::Id::unique(),
                    status: iced::event::Status::Captured,
                    event: iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        modified_key: iced::keyboard::Key::Named(
                            iced::keyboard::key::Named::Escape,
                        ),
                        physical_key: iced::keyboard::key::Physical::Code(
                            iced::keyboard::key::Code::Escape,
                        ),
                        location: iced::keyboard::Location::Standard,
                        modifiers: iced::keyboard::Modifiers::empty(),
                        text: None,
                        repeat: false,
                    }),
                };
                recipe
                    .stream(Box::pin(iced::futures::stream::iter([input])))
                    .next()
                    .now_or_never()
                    .flatten()
            })
            .collect();
        assert!(
            outputs.is_empty(),
            "the composer already handled this Escape"
        );
    }
}
