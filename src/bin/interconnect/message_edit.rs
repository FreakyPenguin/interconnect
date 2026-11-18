//! Per-conversation edits temporarily replace the composer and retain its draft.
use super::{
    App, Draft, DraftKey, MAX_PENDING_OPERATIONS, Message, current_pane, reaction_picker, shell,
};
use iced::{
    Element, Task,
    widget::{button, container, row, text, text_editor},
};
use interconnect::composition::{self, Prepared};

pub struct State {
    event: String,
    request: u64,
    original_mentions: Vec<matrix_sdk::ruma::OwnedUserId>,
    pub(super) saved: Box<Draft>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Start(reaction_picker::Target),
    Latest(DraftKey),
    Cancel(DraftKey),
    Finished {
        key: DraftKey,
        account: Option<String>,
        request: u64,
        // Demo conversion returns the same prepared content used by real edits.
        result: Result<Option<Prepared>, String>,
    },
}
fn message(event: Event) -> Message {
    Message::Editing(event)
}
fn focus(key: &DraftKey) -> Task<Message> {
    iced::widget::operation::focus(if key.root.is_some() {
        "thread-composer"
    } else {
        "room-composer"
    })
}
impl App {
    pub(super) fn restore_deleted_drafts(&mut self, room: &str, event: &str) {
        let mut restored = false;
        for (key, draft) in &mut self.drafts {
            if key.room == room
                && !draft.pending
                && draft
                    .editing
                    .as_ref()
                    .is_some_and(|editing| editing.event == event)
            {
                let editing = draft.editing.take().unwrap();
                *draft = *editing.saved;
                restored = true;
            }
        }
        if restored {
            self.emoji_completion.clear();
            self.user_completion.clear();
        }
    }
    pub(super) fn validate_redacted_edits(&mut self) {
        for thread in [false, true] {
            let Some(key) = self.draft_key(thread) else {
                continue;
            };
            let Some(event) = self
                .drafts
                .get(&key)
                .and_then(|draft| draft.editing.as_ref())
                .map(|editing| editing.event.clone())
            else {
                continue;
            };
            let selected = if thread {
                &self.selection.thread
            } else {
                &self.selection.room
            };
            let Some((generation, _)) = selected else {
                continue;
            };
            let target = reaction_picker::Target {
                room: key.room.clone(),
                thread,
                generation: *generation,
                event: event.clone(),
            };
            let row = self.message_row(&target).or_else(|| {
                let (generation, _) = self.selection.room.as_ref()?;
                super::current_pane(&self.model.room, *generation)?
                    .rows
                    .iter()
                    .find(|row| row.event_id.as_ref() == Some(&event))
            });
            if row.is_some_and(|row| !row.redactable) {
                self.restore_deleted_drafts(&key.room, &event);
            }
        }
    }
    pub(super) fn message_edit_action(
        &self,
        row: &interconnect::connected::MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Message> {
        let target = reaction_picker::Target {
            room: self.selection.room.as_ref()?.1.clone(),
            thread,
            generation,
            event: row.event_id.clone()?,
        };
        self.can_edit_message(&target)
            .then(|| message(Event::Start(target)))
    }

    pub(super) fn can_edit_message(&self, target: &reaction_picker::Target) -> bool {
        !self.composing
            && self.emoji_management.is_none()
            && self.context_menu_allowed()
            && !self.bookmarks.open
            && !self.global_search.open
            && self
                .message_row(target)
                .is_some_and(|row| row.is_own && row.edit_source.is_some())
            && self
                .draft_key(target.thread)
                .as_ref()
                .and_then(|key| self.drafts.get(key))
                .is_some_and(|draft| !draft.pending)
    }
    pub(super) fn message_edit_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Start(target) if self.can_edit_message(&target) => {
                let source = self
                    .message_row(&target)
                    .unwrap()
                    .edit_source
                    .clone()
                    .unwrap();
                let key = self.draft_key(target.thread).unwrap();
                // Replacing an unfinished edit keeps the original message draft.
                let previous = self.drafts.remove(&key).unwrap();
                let mut saved = match previous.editing {
                    Some(editing) => editing.saved,
                    None => Box::new(previous),
                };
                saved.markdown.clear_preview();
                self.generation += 1;
                let mut draft = Draft {
                    composer_height: saved.composer_height,
                    content: text_editor::Content::with_text(&source.body),
                    editing: Some(State {
                        event: target.event,
                        request: self.generation,
                        original_mentions: source
                            .mentions
                            .iter()
                            .chain(
                                source
                                    .named_mentions
                                    .iter()
                                    .filter_map(|mention| mention.target.user_id()),
                            )
                            .chain(
                                source
                                    .mention_links
                                    .iter()
                                    .filter_map(|mention| mention.target.user_id()),
                            )
                            .cloned()
                            .collect::<std::collections::BTreeSet<_>>()
                            .into_iter()
                            .collect(),
                        saved,
                    }),
                    ..Draft::default()
                };
                draft.markdown.format = source.format;
                draft.markdown.emoji = source.emoji;
                draft.markdown.mentions = source.mentions;
                draft.markdown.mention_links = source.mention_links;
                draft.markdown.named_mentions = source.named_mentions;
                draft
                    .content
                    .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                self.drafts.insert(key.clone(), draft);
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.composer_picker = None;
                return focus(&key);
            }
            Event::Latest(key) if self.latest_edit_allowed(&key) => {
                let thread = key.root.is_some();
                let selected = if thread {
                    &self.selection.thread
                } else {
                    &self.selection.room
                };
                let Some((generation, _)) = selected else {
                    return Task::none();
                };
                let pane = if thread {
                    &self.model.thread
                } else {
                    &self.model.room
                };
                let Some(pane) = current_pane(pane, *generation) else {
                    return Task::none();
                };
                let target = pane
                    .rows
                    .iter()
                    .rev()
                    // Room Up skips thread replies, which have their own composer.
                    .filter(|row| {
                        if thread {
                            row.thread_root == key.root
                        } else {
                            row.thread_root.is_none()
                        }
                    })
                    .filter_map(|row| {
                        Some(reaction_picker::Target {
                            room: key.room.clone(),
                            thread,
                            generation: *generation,
                            event: row.event_id.clone()?,
                        })
                    })
                    .find(|target| self.can_edit_message(target));
                if let Some(target) = target {
                    return self.message_edit_event(Event::Start(target));
                }
            }
            Event::Cancel(key)
                if !self.closing
                    && !self.stopped
                    && self.draft_key(key.root.is_some()).as_ref() == Some(&key) =>
            {
                if let Some(draft) = self.drafts.get_mut(&key)
                    && !draft.pending
                    && let Some(editing) = draft.editing.take()
                {
                    *draft = *editing.saved;
                    self.emoji_completion.clear();
                    self.user_completion.clear();
                    return focus(&key);
                }
            }
            Event::Finished {
                key,
                account,
                request,
                result,
            } => {
                self.pending_ops -= 1;
                if account != self.account_id {
                    return Task::none();
                }
                if let Some(draft) = self.drafts.get_mut(&key)
                    && draft
                        .editing
                        .as_ref()
                        .is_some_and(|editing| editing.request == request)
                    && draft.pending
                {
                    draft.pending = false;
                    match result {
                        Ok(prepared) => {
                            let editing = draft.editing.take().unwrap();
                            *draft = *editing.saved;
                            if let Some(prepared) = prepared
                                && let Some(demo) = &mut self.demo
                            {
                                demo.edit_message(&key.room, &editing.event, &prepared);
                                self.refresh_demo();
                            }
                        }
                        Err(error) => {
                            draft.error = Some(format!("Could not edit message: {error}"))
                        }
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }
    fn latest_edit_allowed(&self, key: &DraftKey) -> bool {
        !self.composing
            && self.context_menu.is_none()
            && self.context_menu_allowed()
            && self.draft_key(key.root.is_some()).as_ref() == Some(key)
            && self.drafts.get(key).is_some_and(|draft| {
                !draft.pending
                    && draft.editing.is_none()
                    && draft.attachment.is_none()
                    && draft.content.text().is_empty()
            })
    }
    pub(super) fn message_edit_binding(
        &self,
        key: &DraftKey,
        press: &text_editor::KeyPress,
    ) -> Option<text_editor::Binding<Message>> {
        use iced::keyboard::{Key, key::Named};
        if self.composing
            || !matches!(press.status, text_editor::Status::Focused { .. })
            || !press.modifiers.is_empty()
        {
            return None;
        }
        if press.key == Key::Named(Named::ArrowUp) && self.latest_edit_allowed(key) {
            return Some(text_editor::Binding::Custom(message(Event::Latest(
                key.clone(),
            ))));
        }
        if press.key == Key::Named(Named::Escape) && self.drafts[key].editing.is_some() {
            return Some(text_editor::Binding::Custom(message(Event::Cancel(
                key.clone(),
            ))));
        }
        None
    }
    pub(super) fn save_message_edit(&mut self, key: DraftKey) -> Task<Message> {
        if self.pending_ops >= MAX_PENDING_OPERATIONS {
            return Task::none();
        }
        let Some(draft) = self
            .drafts
            .get(&key)
            .filter(|draft| !draft.pending && !draft.content.text().trim().is_empty())
        else {
            return Task::none();
        };
        let Some(editing) = &draft.editing else {
            return Task::none();
        };
        let event = editing.event.clone();
        let request = editing.request;
        let original_mentions = editing.original_mentions.clone();
        let source = draft.content.text();
        let format = draft.markdown.format;
        let bindings = draft.markdown.bindings();
        let account = self.account_id.clone();
        let demo = self.demo.is_some();
        // A thread original may be projected from the room pane rather than
        // present in the SDK's reply-only thread timeline.
        let room_key = DraftKey {
            room: key.room.clone(),
            root: None,
        };
        let original_in_room = key.root.as_ref() == Some(&event)
            && self.model.room.as_ref().is_some_and(|pane| {
                pane.rows
                    .iter()
                    .any(|row| row.event_id.as_ref() == Some(&event))
            });
        let composer = self.composer_for(if original_in_room { &room_key } else { &key });
        if !demo && composer.is_none() {
            return Task::none();
        }
        let draft = self.drafts.get_mut(&key).unwrap();
        draft.pending = true;
        draft.error = None;
        self.pending_ops += 1;
        Task::perform(
            async move {
                if demo {
                    composition::prepare_with_bindings(source, format, bindings)
                        .await
                        .map(Some)
                } else {
                    composer
                        .unwrap()
                        .edit_message_with_mention_tracking(
                            event,
                            source,
                            format,
                            bindings,
                            original_mentions,
                        )
                        .await
                        .map(|()| None)
                }
            },
            move |result| {
                message(Event::Finished {
                    key: key.clone(),
                    account: account.clone(),
                    request,
                    result,
                })
            },
        )
    }
    pub(super) fn message_edit_banner(&self, key: &DraftKey) -> Option<Element<'_, Message>> {
        let draft = self.drafts.get(key)?;
        draft.editing.as_ref()?;
        Some(
            container(
                row![
                    text("Editing message").size(12).color(shell::MUTED),
                    iced::widget::space().width(iced::Length::Fill),
                    button(text("Cancel").size(12))
                        .padding([2, 0])
                        .style(shell::quiet_button)
                        .on_press_maybe(
                            (!draft.pending).then(|| message(Event::Cancel(key.clone())))
                        ),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(iced::Padding {
                top: 4.0,
                right: 12.0,
                bottom: 0.0,
                left: 12.0,
            })
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo, tests::demo_app};
    use interconnect::composition::Format;

    fn target(app: &App, thread: bool) -> reaction_picker::Target {
        let pane = if thread {
            &app.model.thread
        } else {
            &app.model.room
        };
        let pane = pane.as_ref().unwrap();
        let row = pane
            .rows
            .iter()
            .rev()
            .find(|row| row.is_own && row.edit_source.is_some())
            .unwrap();
        reaction_picker::Target {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            thread,
            generation: pane.generation,
            event: row.event_id.clone().unwrap(),
        }
    }
    fn start(app: &mut App, target: reaction_picker::Target) {
        let _ = app.update(message(Event::Start(target)));
    }
    fn finish(app: &mut App, key: &DraftKey, result: Result<Option<Prepared>, String>) {
        let request = app.drafts[key].editing.as_ref().unwrap().request;
        let _ = app.update(message(Event::Finished {
            key: key.clone(),
            account: app.account_id.clone(),
            request,
            result,
        }));
    }
    #[tokio::test]
    async fn cancel_restores_draft_attachment_format_cursor_and_scroll_after_navigation() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "attachment fixture").unwrap();
        let draft = app.drafts.get_mut(&key).unwrap();
        draft.content = text_editor::Content::with_text("Unsent **literal** draft");
        draft
            .content
            .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        draft.markdown.format = Format::Plain;
        draft.composer_height = Some(177.0);
        draft.attachment = Some(
            interconnect::attachment::Attachment::from_path(file.path().to_owned())
                .await
                .unwrap(),
        );
        let cursor = draft.content.cursor();
        app.scroll_positions.insert(key.clone(), 87.0);
        let row = target(&app, false);
        start(&mut app, row.clone());
        assert_eq!(app.drafts[&key].editing.as_ref().unwrap().event, row.event);
        assert!(app.drafts[&key].attachment.is_none());
        let selection = app.selection.room.clone();
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.selection.room, selection);
        let _ = app.update(Message::Room("general".into()));
        assert!(app.drafts[&key].editing.is_some());
        let _ = app.update(Message::Room(key.room.clone()));
        let _ = app.update(message(Event::Cancel(key.clone())));
        let draft = &app.drafts[&key];
        assert!(draft.editing.is_none());
        assert_eq!(draft.content.text(), "Unsent **literal** draft");
        assert_eq!(draft.content.cursor(), cursor);
        assert_eq!(draft.markdown.format, Format::Plain);
        assert_eq!(draft.composer_height, Some(177.0));
        assert!(draft.attachment.is_some());
        assert_eq!(app.scroll_positions[&key], 87.0);
    }
    #[tokio::test]
    async fn save_updates_room_and_thread_without_appending_or_clearing_other_drafts() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let row = target(&app, false);
        let _ = app.update(Message::Thread(row.event.clone()));
        let thread = app.draft_key(true).unwrap();
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("room draft");
        app.drafts.get_mut(&thread).unwrap().content =
            text_editor::Content::with_text("thread draft");
        let row_count = app.model.room.as_ref().unwrap().rows.len();
        start(&mut app, row.clone());
        app.drafts.get_mut(&key).unwrap().content =
            text_editor::Content::with_text("**Revised** message");
        app.drafts.get_mut(&key).unwrap().markdown.format = Format::Markdown;
        let _ = app.update(Message::Send(key.clone()));
        assert!(app.drafts[&key].pending);
        let _ = app.update(message(Event::Cancel(key.clone())));
        assert!(
            app.drafts[&key].pending,
            "queue admission owns the captured edit"
        );
        let prepared = composition::prepare("**Revised** message".into(), Format::Markdown)
            .await
            .unwrap();
        finish(&mut app, &key, Ok(Some(prepared)));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.model.room.as_ref().unwrap().rows.len(), row_count);
        let edited = app.message_row(&row).unwrap();
        assert_eq!(edited.body, "**Revised** message");
        assert!(edited.edited && edited.formatted.is_some());
        assert_eq!(app.thread_original().unwrap().body, edited.body);
        assert_eq!(app.drafts[&key].content.text(), "room draft");
        assert_eq!(app.drafts[&thread].content.text(), "thread draft");
        assert!(app.drafts[&key].editing.is_none());
    }
    #[test]
    fn errors_keep_edit_and_replaced_edit_retains_original_draft() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let row = target(&app, false);
        app.drafts.get_mut(&key).unwrap().content =
            text_editor::Content::with_text("original draft");
        start(&mut app, row.clone());
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("retry this");
        let _ = app.update(Message::Send(key.clone()));
        finish(&mut app, &key, Err("fixture refusal".into()));
        assert!(!app.drafts[&key].pending);
        assert_eq!(app.drafts[&key].content.text(), "retry this");
        assert!(
            app.drafts[&key]
                .error
                .as_ref()
                .unwrap()
                .contains("fixture refusal")
        );
        start(&mut app, row);
        let _ = app.update(message(Event::Cancel(key.clone())));
        assert_eq!(app.drafts[&key].content.text(), "original draft");
    }
    #[test]
    fn up_only_edits_an_empty_focused_composer_and_ignores_stale_targets() {
        use iced::keyboard::{Key, Modifiers, key::Named};
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let mut press = text_editor::KeyPress {
            key: Key::Named(Named::ArrowUp),
            modified_key: Key::Named(Named::ArrowUp),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            modifiers: Modifiers::empty(),
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        };
        assert!(matches!(
            app.message_edit_binding(&key, &press),
            Some(text_editor::Binding::Custom(_))
        ));
        press.status = text_editor::Status::Active;
        assert!(app.message_edit_binding(&key, &press).is_none());
        press.status = text_editor::Status::Focused { is_hovered: false };
        app.composing = true;
        assert!(app.message_edit_binding(&key, &press).is_none());
        app.composing = false;
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("\n");
        assert!(app.message_edit_binding(&key, &press).is_none());
        let row = target(&app, false);
        let _ = app.update(Message::Room("general".into()));
        start(&mut app, row);
        assert!(app.drafts[&key].editing.is_none());
        let _ = app.update(Message::Room(key.room.clone()));
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::new();
        let _ = app.update(message(Event::Latest(key.clone())));
        assert!(app.drafts[&key].editing.is_some());
        assert!(app.message_edit_binding(&key, &press).is_none());
    }
    #[tokio::test]
    async fn completion_while_editing_preserves_named_mentions_and_restores_unsent_drafts() {
        use iced::keyboard::{Key, Modifiers, key::Named};
        for thread in [false, true] {
            let mut app = demo_app();
            let _ = app.update(Message::WindowFocused(true));
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let key = app.draft_key(thread).unwrap();
            let html = "<p>Hello <a href='https://matrix.to/#/@maya:example.org'>Maya Chen</a> in <a href='https://matrix.to/#/!old:example.org?via=route.example.org'>#Old room</a></p>";
            let prepared = Prepared {
                source: "Hello Maya Chen in #Old room".into(),
                html: Some(html.into()),
                document: interconnect::message_document::Document::parse(html)
                    .map(std::sync::Arc::new),
                mentions: vec!["@maya:example.org".parse().unwrap()],
            };
            app.demo.as_mut().unwrap().send_prepared(&key, prepared);
            app.refresh_demo();
            let draft = app.drafts.get_mut(&key).unwrap();
            draft.content = text_editor::Content::with_text("Unsent @alex:example.org");
            draft
                .markdown
                .mentions
                .push("@alex:example.org".parse().unwrap());
            let target = target(&app, thread);
            let event = target.event.clone();
            start(&mut app, target);
            assert_eq!(app.drafts[&key].markdown.named_mentions.len(), 2);
            let _ = app.update(Message::Edit(
                key.clone(),
                text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(
                    " and @ju".into(),
                ))),
            ));
            let generation = if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            };
            let press = text_editor::KeyPress {
                key: Key::Named(Named::Tab),
                modified_key: Key::Named(Named::Tab),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                modifiers: Modifiers::empty(),
                text: None,
                status: text_editor::Status::Focused { is_hovered: false },
            };
            let Some(text_editor::Binding::Custom(accept)) =
                app.user_completion_binding(&key, generation, &press)
            else {
                panic!("editing should offer user completion");
            };
            let _ = app.update(accept);
            let _ = app.update(Message::Edit(
                key.clone(),
                text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(
                    "and #gen".into(),
                ))),
            ));
            let Some(text_editor::Binding::Custom(accept)) =
                app.user_completion_binding(&key, generation, &press)
            else {
                panic!("editing should offer room completion");
            };
            let _ = app.update(accept);
            assert!(!app.drafts[&key].pending);
            let draft = &app.drafts[&key];
            let prepared = composition::prepare_with_bindings(
                draft.content.text(),
                draft.markdown.format,
                draft.markdown.bindings(),
            )
            .await
            .unwrap();
            assert_eq!(
                prepared
                    .mentions
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
                ["@jules:example.org", "@maya:example.org"]
            );
            assert_eq!(
                prepared
                    .document
                    .as_ref()
                    .unwrap()
                    .text(&Default::default()),
                "Hello Maya Chen in #Old room and Jules and #General "
            );
            let rooms: Vec<_> = prepared
                .document
                .as_ref()
                .unwrap()
                .blocks
                .iter()
                .flat_map(|block| &block.runs)
                .filter_map(|run| run.style.room_mention.as_ref())
                .collect();
            assert_eq!(rooms.len(), 2);
            assert_eq!(rooms[0].room.as_str(), "!old:example.org");
            assert_eq!(rooms[0].via[0].as_str(), "route.example.org");
            assert_eq!(rooms[1].room.as_str(), "!demo-general:example.org");
            let _ = app.update(Message::Send(key.clone()));
            finish(&mut app, &key, Ok(Some(prepared)));
            assert!(app.drafts[&key].editing.is_none());
            assert_eq!(app.drafts[&key].content.text(), "Unsent @alex:example.org");
            assert_eq!(
                app.drafts[&key].markdown.mentions[0].as_str(),
                "@alex:example.org"
            );
            let pane = if thread {
                &app.model.thread
            } else {
                &app.model.room
            };
            let row = pane
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.event_id.as_ref() == Some(&event))
                .unwrap();
            assert!(row.edited);
            let document = row.formatted.as_ref().unwrap();
            let users: Vec<_> = document
                .blocks
                .iter()
                .flat_map(|block| &block.runs)
                .filter_map(|run| run.style.mention.as_deref())
                .map(|id| id.as_str())
                .collect();
            assert_eq!(users, ["@maya:example.org", "@jules:example.org"]);
            // A second edit recovers both the old label and the newly inserted ID.
            let target = self::target(&app, thread);
            start(&mut app, target);
            assert_eq!(app.drafts[&key].markdown.named_mentions.len(), 4);
            let _ = app.update(message(Event::Cancel(key.clone())));
            assert_eq!(app.drafts[&key].content.text(), "Unsent @alex:example.org");
        }
    }

    #[tokio::test]
    async fn plain_text_and_custom_emoji_keep_their_authoring_mode_and_identity() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.demo
            .as_mut()
            .unwrap()
            .send(&key, "Literal **asterisks**".into());
        app.refresh_demo();
        let row = target(&app, false);
        start(&mut app, row);
        assert_eq!(app.drafts[&key].markdown.format, Format::Plain);
        let _ = app.update(message(Event::Cancel(key.clone())));
        let emoji = demo::custom_emoji(&key.room).entries[0].clone();
        let prepared = composition::prepare_with_emoji(
            "Hello :wave:".into(),
            Format::Markdown,
            vec![interconnect::custom_emoji::Selection {
                token: ":wave:".into(),
                emoji: emoji.clone(),
            }],
        )
        .await
        .unwrap();
        app.demo.as_mut().unwrap().send_prepared(&key, prepared);
        app.refresh_demo();
        let row = target(&app, false);
        let event = row.event.clone();
        start(&mut app, row);
        let draft = &app.drafts[&key];
        assert_eq!(draft.markdown.format, Format::Markdown);
        assert_eq!(draft.markdown.emoji.len(), 1);
        assert_eq!(draft.markdown.emoji[0].emoji.uri, emoji.uri);
        let _ = app.update(message(Event::Cancel(key)));
        let _ = app.update(Message::Thread(event));
        let original = target(&app, true);
        start(&mut app, original);
        let thread = app.draft_key(true).unwrap();
        assert_eq!(app.drafts[&thread].markdown.format, Format::Markdown);
        assert_eq!(app.drafts[&thread].markdown.emoji[0].emoji.uri, emoji.uri);
    }
    #[test]
    fn completion_is_scoped_to_account_and_edit_request() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let row = target(&app, false);
        start(&mut app, row);
        let request = app.drafts[&key].editing.as_ref().unwrap().request;
        app.drafts.get_mut(&key).unwrap().pending = true;
        app.pending_ops = 2;
        let _ = app.update(message(Event::Finished {
            key: key.clone(),
            account: Some("@other:example.org".into()),
            request,
            result: Ok(None),
        }));
        let _ = app.update(message(Event::Finished {
            key: key.clone(),
            account: app.account_id.clone(),
            request: request + 1,
            result: Ok(None),
        }));
        assert!(app.drafts[&key].pending);
        assert!(app.drafts[&key].editing.is_some());
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn thread_up_edits_only_replies_and_keeps_a_room_edit_independent() {
        let mut app = demo_app();
        let room = app.draft_key(false).unwrap();
        let room_target = target(&app, false);
        start(&mut app, room_target);
        let _ = app.update(Message::Thread("review".into()));
        let thread = app.draft_key(true).unwrap();
        let _ = app.update(message(Event::Latest(thread.clone())));
        let editing = app.drafts[&thread].editing.as_ref().unwrap();
        assert_eq!(editing.event, "review-reply-2");
        assert!(app.drafts[&room].editing.is_some());
        let _ = app.update(message(Event::Cancel(thread.clone())));
        assert!(app.drafts[&thread].content.text().is_empty());
        assert!(app.drafts[&room].editing.is_some());
        let _ = app.update(message(Event::Cancel(room.clone())));
        assert!(app.drafts[&room].editing.is_none());
    }
    #[test]
    fn edit_controls_fit_room_and_thread_layouts_and_settle_without_timers() {
        use iced::{
            Event as IcedEvent, Rectangle, Size,
            advanced::{Layout, Shell, layout, mouse, widget},
        };
        #[derive(Default)]
        struct Probe(Vec<(String, Rectangle)>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: Rectangle, value: &str) {
                if matches!(value, "Editing message" | "Cancel" | "Save") {
                    self.0.push((value.into(), bounds));
                }
            }
        }
        let mut app = demo_app();
        let room = app.draft_key(false).unwrap();
        let _ = app.update(message(Event::Latest(room)));
        let _ = app.update(Message::Thread("review".into()));
        let thread = app.draft_key(true).unwrap();
        let _ = app.update(message(Event::Latest(thread)));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for size in [Size::new(640.0, 480.0), Size::new(1240.0, 820.0)] {
            let _ = app.update(Message::WindowResized(size.width));
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut probe = Probe::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            for label in ["Editing message", "Cancel", "Save"] {
                let controls: Vec<_> = probe.0.iter().filter(|(value, _)| value == label).collect();
                assert!(!controls.is_empty(), "missing {label} at {size:?}");
                for (_, bounds) in controls {
                    assert!(
                        bounds.x >= 0.0
                            && bounds.y >= 0.0
                            && bounds.x + bounds.width <= size.width + 0.1
                            && bounds.y + bounds.height <= size.height + 0.1,
                        "{label} outside {size:?}: {bounds:?}"
                    );
                }
            }
            view.as_widget_mut().operate(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus(widget::Id::new("thread-composer")),
            );
            for milliseconds in [0, 750, 1500] {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &IcedEvent::Window(iced::window::Event::RedrawRequested(
                        iced::time::Instant::now() + std::time::Duration::from_millis(milliseconds),
                    )),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &Rectangle::with_size(size),
                );
                assert!(!matches!(
                    shell.redraw_request(),
                    iced::window::RedrawRequest::At(_)
                ));
                if milliseconds > 0 {
                    assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                }
            }
        }
    }
    #[tokio::test]
    async fn two_line_edit_has_no_blank_row_between_header_and_text() {
        use iced::advanced::{Layout, layout, widget};
        #[derive(Default)]
        struct Probe {
            header: Option<iced::Rectangle>,
            editor: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, value: &str) {
                if value == "Editing message" {
                    self.header = Some(bounds);
                }
            }
            fn focusable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: &mut dyn widget::operation::Focusable,
            ) {
                if id == Some(&widget::Id::new("room-composer")) {
                    self.editor = Some(bounds);
                }
            }
        }
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let source = "*this is italic*\n**this is bold**";
        let prepared = composition::prepare(source.into(), Format::Markdown)
            .await
            .unwrap();
        app.demo.as_mut().unwrap().send_prepared(&key, prepared);
        app.refresh_demo();
        let row = target(&app, false);
        start(&mut app, row);
        assert_eq!(app.drafts[&key].content.text(), source);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut view = app.pane(false);
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(800.0, 600.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        let header = probe.header.unwrap();
        let editor = probe.editor.unwrap();
        app.drafts[&key].content.with_editor(|content| {
            let buffer = content.buffer();
            let visible = buffer.size().1.unwrap();
            let first = buffer.layout_runs().next().unwrap();
            let top_padding = editor.height - visible - 8.0;
            let gap = editor.y + top_padding + first.line_top - (header.y + header.height);
            assert!(
                gap <= 6.1,
                "edit header leaves a blank-looking row: {gap}px"
            );
            assert!(
                visible >= buffer.metrics().line_height * 2.0,
                "two short lines should fit without a scrollbar: {visible}px"
            );
        });
    }
}
