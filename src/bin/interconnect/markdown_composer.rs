//! On-demand previews retain source drafts and never parse on keystrokes.
use super::{App, DraftKey, Message, current_pane, jump_to_newest, selectable, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, scrollable, text, tooltip},
};
use interconnect::composition::{self, Format, Prepared};
use std::sync::Arc;

#[derive(Default)]
pub struct State {
    pub format: Format,
    pub emoji: Vec<interconnect::custom_emoji::Selection>,
    request: u64,
    task: Option<iced::task::Handle>,
    preview: Option<Preview>,
}
impl State {
    pub fn insert_emoji(
        &mut self,
        emoji: &interconnect::custom_emoji::Emoji,
        source: &str,
    ) -> Option<String> {
        self.emoji.retain(|choice| source.contains(&choice.token));
        if let Some(choice) = self.emoji.iter().find(|choice| choice.emoji == *emoji) {
            return Some(choice.token.clone());
        }
        if self.emoji.len() >= 256 {
            return None;
        }
        let mut token = format!(":{}:", emoji.shortcode);
        let mut index = 2;
        while source.contains(&token) || self.emoji.iter().any(|choice| choice.token == token) {
            token = format!(":{}/{index}:", emoji.shortcode);
            index += 1;
        }
        self.emoji.push(interconnect::custom_emoji::Selection {
            token: token.clone(),
            emoji: emoji.clone(),
        });
        self.clear_preview();
        Some(token)
    }

    pub fn clear_preview(&mut self) {
        self.request = self.request.wrapping_add(1);
        self.task = None;
        self.preview = None;
    }
}
struct Preview {
    source: String,
    format: Format,
    result: Option<Result<Arc<Prepared>, String>>,
}
#[derive(Clone, Debug)]
pub struct Target {
    key: DraftKey,
    generation: u64,
    account: Option<String>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Mode(Target),
    Preview(Target),
    Prepared {
        target: Target,
        request: u64,
        result: Result<Arc<Prepared>, String>,
    },
    DemoSent {
        key: DraftKey,
        account: Option<String>,
        result: Result<Prepared, String>,
    },
}
impl App {
    pub(super) fn preview_emoji_images(&self) -> Vec<interconnect::image_media::Key> {
        [false, true]
            .into_iter()
            .filter_map(|thread| self.draft_key(thread))
            .filter_map(|key| self.drafts.get(&key))
            .filter(|draft| {
                draft.attachment.is_none()
                    && draft.markdown.preview.as_ref().is_some_and(|preview| {
                        preview.source == draft.content.text()
                            && preview.format == draft.markdown.format
                    })
            })
            .flat_map(|draft| {
                draft
                    .markdown
                    .emoji
                    .iter()
                    .filter(|choice| draft.content.text().contains(&choice.token))
            })
            .map(|choice| interconnect::image_media::Key::inline(choice.emoji.uri.clone()))
            .take(32)
            .collect()
    }

    fn markdown_target_valid(&self, target: &Target) -> bool {
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
            && !self.composing
            && self.account_id == target.account
            && self.draft_key(thread).as_ref() == Some(&target.key)
            && selection
                .as_ref()
                .is_some_and(|(generation, _)| *generation == target.generation)
            && current_pane(pane, target.generation).is_some_and(|pane| pane.error.is_none())
            && self
                .drafts
                .get(&target.key)
                .is_some_and(|draft| !draft.pending && draft.attachment.is_none())
    }
    pub(super) fn markdown_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Mode(target) if self.markdown_target_valid(&target) => {
                let state = &mut self.drafts.get_mut(&target.key).unwrap().markdown;
                state.format = match state.format {
                    Format::Markdown => Format::Plain,
                    Format::Plain => Format::Markdown,
                };
                state.clear_preview();
            }
            Event::Preview(target) if self.markdown_target_valid(&target) => {
                let draft = self.drafts.get_mut(&target.key).unwrap();
                let source = draft.content.text();
                let state = &mut draft.markdown;
                state.request = state.request.wrapping_add(1);
                state.task = None;
                if state.preview.as_ref().is_some_and(|preview| {
                    preview.source == source && preview.format == state.format
                }) {
                    state.preview = None;
                    return Task::none();
                }
                if source.trim().is_empty() {
                    return Task::none();
                }
                let request = state.request;
                let format = state.format;
                let emoji = state.emoji.clone();
                state.preview = Some(Preview {
                    source: source.clone(),
                    format,
                    result: None,
                });
                let (task, handle) = Task::perform(
                    async move {
                        composition::prepare_with_emoji(source, format, emoji)
                            .await
                            .map(Arc::new)
                    },
                    move |result| {
                        Message::Markdown(Event::Prepared {
                            target: target.clone(),
                            request,
                            result,
                        })
                    },
                )
                .abortable();
                state.task = Some(handle.abort_on_drop());
                return task;
            }
            Event::Prepared {
                target,
                request,
                result,
            } if self.account_id == target.account => {
                if let Some(draft) = self.drafts.get_mut(&target.key)
                    && draft.markdown.request == request
                    && draft.markdown.preview.as_ref().is_some_and(|preview| {
                        preview.source != draft.content.text()
                            || preview.format != draft.markdown.format
                    })
                {
                    draft.markdown.clear_preview();
                    return Task::none();
                }
                if let Some(draft) = self.drafts.get_mut(&target.key)
                    && draft.markdown.request == request
                    && let Some(preview) = &mut draft.markdown.preview
                    && preview.source == draft.content.text()
                    && preview.format == draft.markdown.format
                {
                    preview.result = Some(result);
                    draft.markdown.task = None;
                }
            }
            Event::DemoSent {
                key,
                account,
                result,
            } => {
                self.pending_ops -= 1;
                if account != self.account_id || self.demo.is_none() {
                    return Task::none();
                }
                if let Some(draft) = self.drafts.get_mut(&key) {
                    draft.pending = false;
                    match result {
                        Ok(prepared) => {
                            self.demo.as_mut().unwrap().send_prepared(&key, prepared);
                            draft.markdown.emoji.clear();
                            draft.content = iced::widget::text_editor::Content::new();
                            draft.markdown.clear_preview();
                            draft.error = None;
                            self.refresh_demo();
                            if self.draft_key(key.root.is_some()).as_ref() == Some(&key) {
                                return jump_to_newest(key.root.is_some());
                            }
                        }
                        Err(error) => {
                            draft.error = Some(error);
                        }
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub(super) fn markdown_controls(
        &self,
        key: &DraftKey,
        generation: u64,
    ) -> Element<'_, Message> {
        let draft = &self.drafts[key];
        if draft.attachment.is_some() {
            return text("Plain text caption")
                .size(11)
                .color(shell::MUTED)
                .into();
        }
        let target = Target {
            key: key.clone(),
            generation,
            account: self.account_id.clone(),
        };
        let label = match draft.markdown.format {
            Format::Markdown => "Markdown",
            Format::Plain => "Plain text",
        };
        let mut mode = button(text(label).size(11))
            .padding([3, 5])
            .style(shell::quiet_button);
        let source = draft.content.text();
        let open = draft.markdown.preview.as_ref().is_some_and(|preview| {
            preview.source == source && preview.format == draft.markdown.format
        });
        let mut preview = button(text(if open { "Hide preview" } else { "Preview" }).size(11))
            .padding([3, 5])
            .style(shell::quiet_button);
        if self.markdown_target_valid(&target) && self.settings.draft.is_none() {
            mode = mode.on_press(Message::Markdown(Event::Mode(target.clone())));
            if !source.trim().is_empty() {
                preview = preview.on_press(Message::Markdown(Event::Preview(target)));
            }
        }
        row![
            super::tooltips::tooltip(
                mode,
                "Switch between Markdown and literal text",
                tooltip::Position::Top
            ),
            preview
        ]
        .spacing(2)
        .into()
    }
    pub(super) fn markdown_preview<'a>(&'a self, key: &DraftKey) -> Option<Element<'a, Message>> {
        let draft = self.drafts.get(key)?;
        if draft.attachment.is_some() {
            return None;
        }
        let preview = draft.markdown.preview.as_ref().filter(|preview| {
            preview.source == draft.content.text() && preview.format == draft.markdown.format
        })?;
        let body = match &preview.result {
            None => text("Preparing preview…")
                .size(12)
                .color(shell::MUTED)
                .into(),
            Some(Err(error)) => text(error).size(12).color(shell::MUTED).into(),
            Some(Ok(prepared)) => selectable::preview_view(
                &prepared.source,
                prepared.document.as_ref(),
                super::inline_image::Assets {
                    remote: self.model.images.clone(),
                    local: self.local_inline.clone(),
                },
            ),
        };
        Some(
            container(
                column![
                    text("Preview").size(11).color(shell::MUTED),
                    container(scrollable(body).height(Length::Shrink)).max_height(130)
                ]
                .spacing(6),
            )
            .padding(10)
            .width(Length::Fill)
            .style(container::rounded_box)
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor::{self, Action, Edit};
    fn target(app: &App, thread: bool) -> Target {
        Target {
            key: app.draft_key(thread).unwrap(),
            generation: if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            },
            account: app.account_id.clone(),
        }
    }
    #[test]
    fn format_is_per_conversation_and_never_rewrites_source_or_scroll() {
        let mut app = crate::tests::demo_app();
        let room = target(&app, false);
        app.drafts.get_mut(&room.key).unwrap().content =
            text_editor::Content::with_text("**room draft**");
        app.scroll_positions.insert(room.key.clone(), 42.0);
        let _ = app.update(Message::Markdown(Event::Mode(room.clone())));
        assert_eq!(app.drafts[&room.key].markdown.format, Format::Plain);
        assert_eq!(app.drafts[&room.key].content.text(), "**room draft**");
        assert_eq!(app.scroll_positions[&room.key], 42.0);
        let _ = app.update(Message::Thread("proposal".into()));
        let thread = target(&app, true);
        assert_eq!(app.drafts[&thread.key].markdown.format, Format::Markdown);
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.draft_key(false).as_ref(), Some(&room.key));
        assert_eq!(app.drafts[&room.key].markdown.format, Format::Plain);
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(Message::Room(room.key.room.clone()));
        assert_eq!(app.drafts[&room.key].markdown.format, Format::Plain);
    }
    #[tokio::test]
    async fn previews_reject_edits_mode_changes_old_requests_and_other_accounts() {
        let mut app = crate::tests::demo_app();
        let target = target(&app, false);
        let source = "**preview**";
        app.drafts.get_mut(&target.key).unwrap().content = text_editor::Content::with_text(source);
        let _ = app.update(Message::Markdown(Event::Preview(target.clone())));
        let request = app.drafts[&target.key].markdown.request;
        assert!(app.markdown_preview(&target.key).is_some());
        let prepared = Arc::new(
            composition::prepare(source.into(), Format::Markdown)
                .await
                .unwrap(),
        );
        let _ = app.update(Message::Edit(
            target.key.clone(),
            Action::Edit(Edit::Insert('x')),
        ));
        let _ = app.update(Message::Markdown(Event::Prepared {
            target: target.clone(),
            request,
            result: Ok(prepared.clone()),
        }));
        assert!(app.markdown_preview(&target.key).is_none());
        app.drafts.get_mut(&target.key).unwrap().content = text_editor::Content::with_text(source);
        let _ = app.update(Message::Markdown(Event::Preview(target.clone())));
        let current = app.drafts[&target.key].markdown.request;
        let _ = app.update(Message::Markdown(Event::Prepared {
            target: target.clone(),
            request,
            result: Ok(prepared.clone()),
        }));
        assert!(
            app.drafts[&target.key]
                .markdown
                .preview
                .as_ref()
                .unwrap()
                .result
                .is_none()
        );
        let _ = app.update(Message::Markdown(Event::Prepared {
            target: target.clone(),
            request: current,
            result: Ok(prepared.clone()),
        }));
        assert!(
            app.drafts[&target.key]
                .markdown
                .preview
                .as_ref()
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .is_ok()
        );
        let _ = app.update(Message::Markdown(Event::Mode(target.clone())));
        assert!(app.markdown_preview(&target.key).is_none());
        let _ = app.update(Message::Markdown(Event::Preview(target.clone())));
        let current = app.drafts[&target.key].markdown.request;
        app.account_id = Some("@other:example.org".into());
        let _ = app.update(Message::Markdown(Event::Prepared {
            target: target.clone(),
            request: current,
            result: Ok(prepared),
        }));
        assert!(
            app.drafts[&target.key]
                .markdown
                .preview
                .as_ref()
                .unwrap()
                .result
                .is_none()
        );
    }
    #[tokio::test]
    async fn demo_markdown_send_renders_locally_and_retains_the_other_draft() {
        let mut app = crate::tests::demo_app();
        let room = target(&app, false);
        app.drafts.get_mut(&room.key).unwrap().content =
            text_editor::Content::with_text("room draft");
        let _ = app.update(Message::Thread("proposal".into()));
        let thread = target(&app, true);
        let source = "**hello** café 👋";
        app.drafts.get_mut(&thread.key).unwrap().content = text_editor::Content::with_text(source);
        let _ = app.update(Message::Send(thread.key.clone()));
        assert!(app.drafts[&thread.key].pending);
        assert_eq!(app.pending_ops, 1);
        assert_eq!(app.drafts[&thread.key].content.text(), source);
        let prepared = composition::prepare(source.into(), Format::Markdown)
            .await
            .unwrap();
        let _ = app.update(Message::Markdown(Event::DemoSent {
            key: thread.key.clone(),
            account: thread.account,
            result: Ok(prepared),
        }));
        let row = app.model.thread.as_ref().unwrap().rows.last().unwrap();
        assert_eq!(row.body, source);
        assert!(row.formatted.as_ref().unwrap().blocks[0].runs[0].style.bold);
        assert!(app.drafts[&thread.key].content.text().is_empty());
        assert_eq!(app.drafts[&room.key].content.text(), "room draft");
        assert_eq!(app.pending_ops, 0);
        assert!(app.control.is_none());
    }
    #[tokio::test]
    async fn failed_formatting_preserves_source_and_returns_to_editable_state() {
        let mut app = crate::tests::demo_app();
        let target = target(&app, false);
        let source = "**keep this source**".to_owned();
        app.drafts.get_mut(&target.key).unwrap().content = text_editor::Content::with_text(&source);
        let _ = app.update(Message::Send(target.key.clone()));
        let result = Err("fixture formatting failure".to_owned());
        let _ = app.update(Message::Markdown(Event::DemoSent {
            key: target.key.clone(),
            account: target.account,
            result,
        }));
        assert_eq!(app.drafts[&target.key].content.text(), source);
        assert!(!app.drafts[&target.key].pending);
        assert!(app.drafts[&target.key].error.is_some());
        assert_eq!(app.pending_ops, 0);
    }
}
