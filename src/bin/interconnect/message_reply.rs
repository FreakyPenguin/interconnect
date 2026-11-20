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
    Jump {
        source: reaction_picker::Target,
        event: String,
    },
}
fn message(event: Event) -> Message {
    Message::Replying(event)
}
impl App {
    pub(super) fn can_reply(&self, target: &reaction_picker::Target) -> bool {
        self.context_menu_allowed()
            && self.room_can_compose(&target.room)
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
        if let Event::Jump { source, event } = event {
            return self.jump_to_reply(source, event);
        }
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
    fn jump_to_reply(&mut self, source: reaction_picker::Target, event: String) -> Task<Message> {
        if !self.context_menu_allowed()
            || self.composing
            || !self
                .message_row(&source)
                .is_some_and(|row| row.reply.as_ref().is_some_and(|reply| reply.event == event))
        {
            return Task::none();
        }
        // A known thread reply belongs in its thread pane. For an unloaded
        // original, retain the source thread; its root itself lives in the room.
        let root = self
            .reply_row(source.thread, &event)
            .and_then(|row| row.thread_root.clone())
            .or_else(|| {
                source
                    .thread
                    .then_some(self.selection.thread.as_ref())
                    .flatten()
                    .filter(|(_, root)| root != &event)
                    .map(|(_, root)| root.clone())
            });
        let thread = root.is_some();
        if let Some(root) = root {
            self.select_thread(root);
        } else {
            self.select_room(source.room);
        }
        let generation = if thread {
            self.selection.thread.as_ref().unwrap().0
        } else {
            self.selection.room.as_ref().unwrap().0
        };
        // Selecting again gives the asynchronous history view a new generation,
        // rejecting snapshots still arriving from the previous navigation.
        self.selection.event_focus[usize::from(thread)] = Some((generation, event));
        self.pending_scroll[usize::from(thread)] = None;
        Task::none()
    }
    fn reply_jump_action(&self, row: &MessageRow, thread: bool) -> Option<Message> {
        let selection = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        Some(message(Event::Jump {
            source: reaction_picker::Target {
                room: self.selection.room.as_ref()?.1.clone(),
                thread,
                generation: selection.as_ref()?.0,
                event: row.event_id.clone()?,
            },
            event: row.reply.as_ref()?.event.clone(),
        }))
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
                    super::tooltips::icon_button(
                        super::icons::Kind::Close,
                        "Cancel reply",
                        (!draft.pending).then(|| message(Event::Cancel(key.clone()))),
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
            button(
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
                }),
            )
            .padding(0)
            .width(Length::Fill)
            .style(button::text)
            .keyboard_navigation(true)
            .on_press_maybe(self.reply_jump_action(row, thread))
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
    fn quote(app: &mut App, thread: bool, source: &str, original: &str) -> Message {
        let pane = if thread {
            app.model.thread.as_mut().unwrap()
        } else {
            app.model.room.as_mut().unwrap()
        };
        let row = std::sync::Arc::make_mut(&mut pane.rows)
            .iter_mut()
            .find(|row| row.event_id.as_deref() == Some(source))
            .unwrap();
        row.reply = Some(ReplyPreview {
            event: original.into(),
            sender: None,
            body: "Original message unavailable".into(),
        });
        let row = row.clone();
        app.reply_jump_action(&row, thread).unwrap()
    }
    #[test]
    fn reply_jump_refreshes_generation_and_preserves_drafts_and_reading_position() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 175.0);
        let old = app.selection.room.as_ref().unwrap().0;
        let jump = quote(&mut app, false, "review", "welcome");
        let (control, observed) = tokio::sync::watch::channel(app.selection.clone());
        app.control = Some(control);
        let _ = app.update(jump);
        let generation = app.selection.room.as_ref().unwrap().0;
        assert_ne!(generation, old);
        assert_eq!(
            app.selection.event_focus[0],
            Some((generation, "welcome".into()))
        );
        assert!(app.pending_scroll[0].is_none());
        assert_eq!(app.model.room.as_ref().unwrap().generation, generation);
        assert_eq!(app.drafts[&key].content.text(), "unsent");
        assert_eq!(app.scroll_positions[&key], 175.0);
        assert_eq!(*observed.borrow(), app.selection);
        assert!(app.selection.thread.is_none());
    }
    #[test]
    fn reply_jump_routes_known_thread_events_unloaded_replies_and_thread_roots() {
        let mut app = demo_app();
        let jump = quote(&mut app, false, "review", "review-reply-1");
        let _ = app.update(jump);
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "review");
        let generation = app.selection.thread.as_ref().unwrap().0;
        assert_eq!(
            app.selection.event_focus[1],
            Some((generation, "review-reply-1".into()))
        );
        for original in ["review-reply-1", "$unloaded"] {
            let jump = quote(&mut app, true, "review-reply-2", original);
            let _ = app.update(jump);
            let generation = app.selection.thread.as_ref().unwrap().0;
            assert_eq!(app.selection.thread.as_ref().unwrap().1, "review");
            assert_eq!(
                app.selection.event_focus[1],
                Some((generation, original.into()))
            );
            assert!(app.pending_scroll[1].is_none());
        }
        let jump = quote(&mut app, true, "review-reply-2", "review");
        let _ = app.update(jump);
        assert!(app.selection.thread.is_none());
        let generation = app.selection.room.as_ref().unwrap().0;
        assert_eq!(
            app.selection.event_focus[0],
            Some((generation, "review".into()))
        );
    }
    #[test]
    fn reply_jump_rejects_stale_sources_changed_quotes_and_blocked_input() {
        for blocked in 0..6 {
            let mut app = demo_app();
            let jump = quote(&mut app, false, "review", "welcome");
            match blocked {
                0 => {
                    let _ = app.update(Message::Room("general".into()));
                }
                1 => {
                    let _ = quote(&mut app, false, "review", "direction");
                }
                2 => app.composing = true,
                3 => app.closing = true,
                4 => app.stopped = true,
                _ => {
                    let _ = app.update(Message::Membership(super::super::membership::Event::Join));
                }
            }
            let selection = app.selection.clone();
            let _ = app.update(jump);
            assert_eq!(app.selection.room, selection.room, "blocked case {blocked}");
            assert_eq!(
                app.selection.thread, selection.thread,
                "blocked case {blocked}"
            );
            assert_eq!(
                app.selection.event_focus, selection.event_focus,
                "blocked case {blocked}"
            );
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
