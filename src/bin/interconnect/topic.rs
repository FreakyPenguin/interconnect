//! Cached Markdown topics and a bounded reader/editor bound to a room generation.
use super::{App, MAX_PENDING_OPERATIONS, Message, current_pane, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, scrollable, text, text_editor, tooltip},
};
use interconnect::{
    composition,
    message_document::{Document, Inline, Run},
};
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

pub struct State {
    target: (u64, String),
    account: Option<String>,
    request: u64,
    content: text_editor::Content,
    pending: bool,
    error: Option<String>,
}

pub struct Rendered {
    target: (u64, String),
    account: Option<String>,
    source: String,
    request: u64,
    result: Option<Presentation>,
    task: Option<iced::task::Handle>,
}
#[derive(Clone, Debug)]
pub struct Presentation {
    document: Option<Arc<Document>>,
    excerpt: String,
    snippet: Vec<Run>,
    fallback: bool,
}
#[derive(Clone, Debug)]
pub enum Event {
    View(u64, String),
    Open(u64, String),
    Edit(text_editor::Action),
    Native(super::native_editing::Command),
    Save,
    Cancel,
    Copy(u64, String),
    Ignore,
    Rendered(u64, Presentation),
    Saved((u64, String), u64, Result<(), String>),
}
fn message(event: Event) -> Message {
    Message::Topic(event)
}
fn excerpt(source: &str) -> String {
    let source = source
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let mut graphemes = source.graphemes(true);
    let mut label: String = graphemes.by_ref().take(256).collect();
    if graphemes.next().is_some() {
        label.push('…');
    }
    label
}
async fn prepare(source: String) -> Presentation {
    // Reuse composition's bounded workers, escaping, safe links, and image-alt
    // handling. A topic never downloads images or produces notification mentions.
    let result = if source.len() <= 64 * 1024 {
        composition::prepare(source.clone(), composition::Format::Markdown).await
    } else {
        Err(String::new())
    };
    let fallback = result.is_err();
    tokio::task::spawn_blocking(move || {
        let document = result
            .ok()
            .and_then(|prepared| prepared.document)
            .or_else(|| Document::plain_links(&source).map(Arc::new));
        let displayed = document.as_ref().map(|doc| doc.text(&Default::default()));
        let excerpt = excerpt(displayed.as_deref().unwrap_or(&source));
        let snippet = document
            .as_ref()
            .and_then(|doc| {
                doc.visible_blocks(&Default::default())
                    .into_iter()
                    .find_map(|block| {
                        let value: String =
                            block.runs.iter().map(|run| run.content.text()).collect();
                        let first = value.lines().find(|line| !line.trim().is_empty())?;
                        let start = value.find(first)?;
                        let end =
                            start + first.graphemes(true).take(256).map(str::len).sum::<usize>();
                        let mut offset = 0;
                        let mut runs = Vec::new();
                        for run in block.runs {
                            let text = run.content.text();
                            let from = start.max(offset);
                            let to = end.min(offset + text.len());
                            if from < to {
                                let mut style = run.style.clone();
                                if matches!(
                                    block.kind,
                                    interconnect::message_document::BlockKind::Heading(_)
                                ) {
                                    style.bold = true;
                                }
                                runs.push(Run {
                                    content: Inline::Text(text[from - offset..to - offset].into()),
                                    style,
                                });
                            }
                            offset += text.len();
                        }
                        if end < start + first.len() {
                            runs.push(Run {
                                content: Inline::Text("…".into()),
                                style: Default::default(),
                            });
                        }
                        Some(runs)
                    })
            })
            .unwrap_or_default();
        Presentation {
            excerpt,
            snippet,
            document,
            fallback,
        }
    })
    .await
    .unwrap_or_else(|_| Presentation {
        document: None,
        excerpt: String::new(),
        snippet: Vec::new(),
        fallback: true,
    })
}
impl App {
    fn topic_target_valid(&self, target: &(u64, String)) -> bool {
        !self.closing
            && !self.stopped
            && self.selection.room.as_ref() == Some(target)
            && self.model.rooms.iter().any(|room| room.id == target.1)
    }
    fn topic_editable(&self, target: &(u64, String)) -> bool {
        self.topic_target_valid(target)
            && self
                .model
                .rooms
                .iter()
                .any(|room| room.id == target.1 && room.can_edit_topic)
    }
    pub(super) fn validate_topic_editor(&mut self) {
        if self.expanded_topic.as_ref().is_some_and(|target| {
            !self.topic_target_valid(target)
                || self
                    .topic_render
                    .as_ref()
                    .is_some_and(|rendered| rendered.account != self.account_id)
        }) {
            self.expanded_topic = None;
        }
        if self.topic_editor.as_ref().is_some_and(|state| {
            state.account != self.account_id
                || !self.topic_target_valid(&state.target)
                || !self.topic_editable(&state.target)
        }) {
            self.topic_editor = None;
        }
    }
    /// Only the selected topic is retained. Dropping/replacing it aborts publication;
    /// already-started blocking work is bounded by composition's shared semaphore.
    pub(super) fn refresh_topic(&mut self) -> Task<Message> {
        let Some(target) = self
            .selection
            .room
            .as_ref()
            .filter(|target| self.topic_target_valid(target))
        else {
            self.topic_render = None;
            return Task::none();
        };
        let source = &self
            .model
            .rooms
            .iter()
            .find(|room| room.id == target.1)
            .unwrap()
            .topic;
        if self.topic_render.as_ref().is_some_and(|rendered| {
            rendered.target == *target
                && rendered.account == self.account_id
                && rendered.source == *source
        }) {
            return Task::none();
        }
        self.generation += 1;
        let request = self.generation;
        let mut rendered = Rendered {
            target: target.clone(),
            account: self.account_id.clone(),
            source: source.clone(),
            request,
            result: None,
            task: None,
        };
        let task = if source.trim().is_empty() {
            Task::none()
        } else {
            let (task, handle) = Task::perform(prepare(source.clone()), move |result| {
                message(Event::Rendered(request, result))
            })
            .abortable();
            rendered.task = Some(handle.abort_on_drop());
            task
        };
        self.topic_render = Some(rendered);
        task
    }
    fn topic_presentation(&self) -> Option<&Presentation> {
        self.topic_render
            .as_ref()
            .filter(|rendered| {
                rendered.account == self.account_id
                    && self.topic_target_valid(&rendered.target)
                    && self
                        .model
                        .rooms
                        .iter()
                        .any(|room| room.id == rendered.target.1 && room.topic == rendered.source)
            })
            .and_then(|rendered| rendered.result.as_ref())
    }
    pub(super) fn topic_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Rendered(request, result) => {
                if let Some(rendered) = &mut self.topic_render
                    && rendered.request == request
                    && rendered.account == self.account_id
                    && self.selection.room.as_ref() == Some(&rendered.target)
                    && self
                        .model
                        .rooms
                        .iter()
                        .any(|room| room.id == rendered.target.1 && room.topic == rendered.source)
                {
                    rendered.task = None;
                    rendered.result = Some(result);
                }
            }
            Event::View(generation, room) => {
                let target = (generation, room);
                if self.topic_target_valid(&target) && self.context_menu_allowed() {
                    self.expanded_topic = if self.expanded_topic.as_ref() == Some(&target) {
                        None
                    } else {
                        Some(target)
                    };
                }
            }
            Event::Open(generation, room) => {
                let target = (generation, room);
                if !self.topic_editable(&target) || !self.context_menu_allowed() {
                    return Task::none();
                }
                let topic = &self
                    .model
                    .rooms
                    .iter()
                    .find(|r| r.id == target.1)
                    .unwrap()
                    .topic;
                self.generation += 1;
                self.topic_editor = Some(State {
                    target,
                    account: self.account_id.clone(),
                    request: self.generation,
                    content: text_editor::Content::with_text(topic),
                    pending: false,
                    error: None,
                });
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.stop_typing();
                return iced::widget::operation::focus("room-topic");
            }
            Event::Native(command) => {
                if let Some(state) = &mut self.topic_editor
                    && !state.pending
                    && !self.composing
                {
                    command.perform(&mut state.content);
                }
            }
            Event::Edit(action) => {
                if let Some(state) = &mut self.topic_editor
                    && (!state.pending || !action.is_edit())
                {
                    state.content.perform(action);
                }
            }
            Event::Cancel => {
                if self
                    .topic_editor
                    .as_ref()
                    .is_some_and(|state| !state.pending)
                {
                    self.topic_editor = None;
                }
            }
            Event::Copy(generation, room) => {
                let target = (generation, room);
                if self.expanded_topic.as_ref() == Some(&target)
                    && self.topic_target_valid(&target)
                    && let Some(room) = self.model.rooms.iter().find(|room| room.id == target.1)
                {
                    return iced::clipboard::write(room.topic.clone());
                }
            }
            Event::Save => {
                let Some(state) = &self.topic_editor else {
                    return Task::none();
                };
                if state.pending
                    || state.account != self.account_id
                    || !self.topic_editable(&state.target)
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                {
                    return Task::none();
                }
                let target = state.target.clone();
                let request = state.request;
                let topic = state.content.text();
                if self.demo.is_some() {
                    if let Some(room) = Arc::make_mut(&mut self.model.rooms)
                        .iter_mut()
                        .find(|room| room.id == target.1)
                    {
                        room.topic = topic;
                    }
                    self.topic_editor = None;
                } else if let Some(composer) =
                    current_pane(&self.model.room, target.0).and_then(|pane| pane.composer.clone())
                {
                    let state = self.topic_editor.as_mut().unwrap();
                    state.pending = true;
                    state.error = None;
                    self.pending_ops += 1;
                    return Task::perform(composer.set_topic(topic), move |result| {
                        message(Event::Saved(target.clone(), request, result))
                    });
                }
            }
            Event::Saved(target, request, result) => {
                self.pending_ops -= 1;
                if self.topic_editor.as_ref().is_some_and(|state| {
                    state.target == target
                        && state.request == request
                        && state.pending
                        && state.account == self.account_id
                }) {
                    match result {
                        Ok(()) => self.topic_editor = None,
                        Err(error) => {
                            let state = self.topic_editor.as_mut().unwrap();
                            state.pending = false;
                            state.error = Some(error);
                        }
                    }
                }
            }
            Event::Ignore => {}
        }
        Task::none()
    }
    pub(super) fn topic_view<'a>(
        &'a self,
        generation: u64,
        room_id: &str,
        max_height: f32,
    ) -> Element<'a, Message> {
        let room = self.model.rooms.iter().find(|room| room.id == room_id);
        let topic = room.map_or("", |room| room.topic.as_str());
        let editable =
            room.is_some_and(|room| room.can_edit_topic) && !self.closing && !self.stopped;
        if topic.trim().is_empty() && !editable {
            return iced::widget::space().height(0).into();
        }
        let empty = topic.trim().is_empty();
        let label = if empty {
            "Add a topic…".to_owned()
        } else {
            self.topic_presentation()
                .filter(|p| !p.excerpt.is_empty())
                .map_or_else(|| excerpt(topic), |p| p.excerpt.clone())
        };
        let expanded = self
            .expanded_topic
            .as_ref()
            .is_some_and(|target| target.0 == generation && target.1 == room_id);
        let snippet: Element<'_, Message> = self
            .topic_presentation()
            .filter(|p| !p.snippet.is_empty())
            .map_or_else(
                || super::single_line::label(label, 12).into(),
                |p| super::topic_snippet::view(&p.snippet, self.effective_theme()),
            );
        let summary = super::tooltips::tooltip(
            button(
                row![
                    snippet,
                    super::icons::sized(
                        if expanded {
                            super::icons::Kind::ChevronUp
                        } else {
                            super::icons::Kind::ChevronDown
                        },
                        12.0
                    )
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            )
            .keyboard_navigation(true)
            .padding(0)
            .width(Length::Fill)
            .style(|theme, _| iced::widget::button::Style {
                text_color: shell::palette(theme).muted,
                ..Default::default()
            })
            .on_press(message(if empty {
                Event::Open(generation, room_id.to_owned())
            } else {
                Event::View(generation, room_id.to_owned())
            })),
            if empty {
                "Add a room topic"
            } else if expanded {
                "Collapse room topic"
            } else {
                "Expand room topic"
            },
            tooltip::Position::Bottom,
        );
        let mut content = column![summary].spacing(6);
        if expanded {
            let presentation = self.topic_presentation();
            let mut body = column![super::selectable::preview_view(
                topic,
                presentation.and_then(|p| p.document.as_ref()),
                Default::default(),
                self.effective_theme()
            )]
            .spacing(8);
            if presentation.is_some_and(|p| p.fallback) {
                body = body.push(text("This topic is shown as plain text because its formatting is too large or complex.").size(12).style(shell::muted_text));
            }
            let mut actions = row![
                button(text("Copy Markdown").size(12))
                    .keyboard_navigation(true)
                    .style(shell::quiet_button)
                    .on_press(message(Event::Copy(generation, room_id.to_owned())))
            ]
            .spacing(6);
            if editable {
                actions = actions.push(super::tooltips::icon_button(
                    super::icons::Kind::Edit,
                    "Edit room topic",
                    Some(message(Event::Open(generation, room_id.to_owned()))),
                ));
            }
            content = content
                .push(
                    container(
                        scrollable(body)
                            .id("expanded-room-topic-body")
                            .height(Length::Shrink)
                            .direction(shell::scrollbar())
                            .style(shell::scroll_style),
                    )
                    .max_height(max_height)
                    .width(Length::Fill),
                )
                .push(container(actions).id("expanded-room-topic-actions"));
        }
        content.into()
    }
    pub(super) fn topic_dialog(&self) -> Element<'_, Message> {
        iced::widget::responsive(move |size| self.topic_dialog_sized(size)).into()
    }
    fn topic_dialog_sized(&self, size: iced::Size) -> Element<'_, Message> {
        let state = self.topic_editor.as_ref().unwrap();
        let close = super::tooltips::icon_button(
            super::icons::Kind::Close,
            "Cancel topic changes",
            (!state.pending).then_some(message(Event::Cancel)),
        );
        let header = row![text("Edit room topic").size(18).width(Length::Fill), close]
            .align_y(iced::Alignment::Center);
        let mut footer = row![].spacing(8).align_y(iced::Alignment::Center);
        let body: Element<'_, Message> = {
            let can_save = !state.pending
                && self.pending_ops < MAX_PENDING_OPERATIONS
                && (self.demo.is_some()
                    || current_pane(&self.model.room, state.target.0)
                        .is_some_and(|pane| pane.composer.is_some()));
            let mut feedback =
                column![text("Markdown supported").size(12).style(shell::muted_text)]
                    .spacing(4)
                    .width(Length::Fill);
            if let Some(error) = &state.error {
                feedback = feedback.push(text(error).size(12));
            }
            footer = footer.push(feedback);
            footer = footer.push(super::tooltips::icon_action(
                super::icons::Kind::Save,
                if state.pending {
                    "Saving room topic…"
                } else {
                    "Save room topic"
                },
                can_save.then_some(message(Event::Save)),
                shell::primary_button,
            ));
            text_editor(&state.content)
                .id("room-topic")
                .cursor_blinking(false)
                .style(shell::editor_style)
                .placeholder("Room topic (Markdown supported)")
                .size(14)
                .height((size.height - 240.0).clamp(100.0, 320.0))
                .on_action(|action| message(Event::Edit(action)))
                .key_binding(|press| {
                    super::native_editing::binding(press, |command| message(Event::Native(command)))
                })
                .into()
        };
        super::focus_scope::dismissible(
            super::dialog::modal(
                super::dialog::frame(size, 720.0, 560.0, "room-topic", header, body, footer),
                message(Event::Ignore),
            ),
            "room-topic-scope",
            Message::Escape,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn open(app: &mut App) {
        let (generation, room) = app.selection.room.clone().unwrap();
        let _ = app.update(message(Event::Open(generation, room)));
    }
    #[test]
    fn long_topic_stays_single_line_and_remains_clickable_in_a_narrow_header() {
        use iced::advanced::{layout, widget};
        let mut app = demo_app();
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .topic = "A very long topic with many details ".repeat(30);
        let (generation, room) = app.selection.room.as_ref().unwrap();
        let mut view = app.topic_view(*generation, room, 120.0);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(240.0, 300.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        assert!(node.size().width <= 240.0);
        assert!(node.size().height <= 18.0);
        let mut messages = Vec::new();
        for event in [
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            )),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                layout::Layout::new(&node),
                iced::mouse::Cursor::Available(iced::Point::new(12.0, 7.0)),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        assert!(messages.iter().any(|event| matches!(event,
            Message::Topic(Event::View(current, id)) if current == generation && id == room)));
    }
    #[test]
    fn demo_topic_save_clear_cancel_and_read_only_room() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            text_editor::Content::with_text("Unsent message");
        app.scroll_positions.insert(key.clone(), 123.0);
        open(&mut app);
        app.topic_editor.as_mut().unwrap().content =
            text_editor::Content::with_text("New topic\nSecond line");
        let _ = app.update(message(Event::Save));
        assert_eq!(
            app.model
                .rooms
                .iter()
                .find(|r| r.id == "studio")
                .unwrap()
                .topic,
            "New topic\nSecond line"
        );
        open(&mut app);
        app.topic_editor.as_mut().unwrap().content = text_editor::Content::new();
        let _ = app.update(message(Event::Cancel));
        assert!(
            !app.model
                .rooms
                .iter()
                .find(|r| r.id == "studio")
                .unwrap()
                .topic
                .is_empty()
        );
        open(&mut app);
        app.topic_editor.as_mut().unwrap().content = text_editor::Content::new();
        let _ = app.update(message(Event::Save));
        assert!(
            app.model
                .rooms
                .iter()
                .find(|r| r.id == "studio")
                .unwrap()
                .topic
                .is_empty()
        );
        assert_eq!(app.drafts[&key].content.text(), "Unsent message");
        assert_eq!(app.scroll_positions[&key], 123.0);
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::Room("release".into()));
        open(&mut app);
        assert!(app.topic_editor.is_none());
    }
    #[test]
    fn navigation_and_permission_changes_reject_stale_editor_and_completion() {
        let mut app = demo_app();
        open(&mut app);
        let target = app.topic_editor.as_ref().unwrap().target.clone();
        let request = app.topic_editor.as_ref().unwrap().request;
        app.topic_editor.as_mut().unwrap().pending = true;
        app.pending_ops = 1;
        let _ = app.update(Message::Room("general".into()));
        assert!(app.topic_editor.is_none());
        open(&mut app);
        let _ = app.update(message(Event::Saved(
            target,
            request,
            Err("Late failure".into()),
        )));
        assert!(app.topic_editor.as_ref().unwrap().error.is_none());
        assert_eq!(app.pending_ops, 0);
        let mut model = app.model.clone();
        std::sync::Arc::make_mut(&mut model.rooms)
            .iter_mut()
            .find(|r| r.id == "general")
            .unwrap()
            .can_edit_topic = false;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.topic_editor.is_none());
        open(&mut app);
        assert!(app.topic_editor.is_none());
    }
    fn set_topic(app: &mut App, value: &str) {
        let room = app.selection.room.as_ref().unwrap().1.clone();
        Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|r| r.id == room)
            .unwrap()
            .topic = value.into();
    }
    fn read(app: &mut App) {
        let (generation, room) = app.selection.room.clone().unwrap();
        let _ = app.update(message(Event::View(generation, room)));
    }
    #[tokio::test]
    async fn markdown_topics_share_safe_formatting_links_and_literal_fallback() {
        use interconnect::message_document::{BlockKind, Inline};
        let source = "# Welcome\n\n**Design** and *plans* with ~~old~~ notes.\n\n- [Reference](https://example.org)\n- `literal *code*`\n\n![Alternative](https://example.org/image.png)\n\n<script>inert</script>";
        let rendered = prepare(source.into()).await;
        assert!(!rendered.fallback);
        let doc = rendered.document.unwrap();
        let displayed = doc.text(&Default::default());
        assert!(displayed.contains("Welcome"));
        assert!(displayed.contains("literal *code*"));
        assert!(displayed.contains("Alternative"));
        assert!(displayed.contains("<script>inert</script>"));
        assert!(!displayed.contains("**Design**"));
        assert!(
            doc.blocks
                .iter()
                .any(|b| matches!(b.kind, BlockKind::Heading(_)))
        );
        let runs: Vec<_> = doc.blocks.iter().flat_map(|b| &b.runs).collect();
        assert!(runs.iter().any(|r| r.style.bold));
        assert!(
            runs.iter()
                .any(|r| r.style.link.as_deref() == Some("https://example.org/"))
        );
        assert!(
            !runs
                .iter()
                .any(|r| matches!(r.content, Inline::Image { .. }))
        );
        let unsafe_link = prepare("[unsafe](javascript:alert)".into()).await;
        assert!(
            unsafe_link
                .document
                .unwrap()
                .blocks
                .iter()
                .flat_map(|b| &b.runs)
                .all(|r| r.style.link.is_none())
        );
        let huge = prepare("*large* ".repeat(10_000)).await;
        assert!(huge.fallback);
        assert!(huge.document.is_none());
        assert!(huge.excerpt.graphemes(true).count() <= 257);
    }
    #[tokio::test]
    async fn presentation_reuses_unchanged_content_and_rejects_stale_results() {
        let mut app = demo_app();
        set_topic(&mut app, "**First** topic");
        let _ = app.refresh_topic();
        let first = app.topic_render.as_ref().unwrap().request;
        let result = prepare("**First** topic".into()).await;
        let _ = app.update(message(Event::Rendered(first, result.clone())));
        assert_eq!(app.topic_presentation().unwrap().excerpt, "First topic");
        let document = app.topic_presentation().unwrap().document.clone().unwrap();
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.topic_render.as_ref().unwrap().request, first);
        assert!(Arc::ptr_eq(
            &document,
            app.topic_presentation().unwrap().document.as_ref().unwrap()
        ));
        set_topic(&mut app, "**Second** topic");
        let _ = app.update(message(Event::Rendered(first, result.clone())));
        assert!(app.topic_presentation().is_none());
        let second = app.topic_render.as_ref().unwrap().request;
        assert_ne!(first, second);
        let _ = app.update(message(Event::Rendered(first, result.clone())));
        assert!(app.topic_presentation().is_none());
        app.account_id = Some("another account".into());
        let _ = app.update(message(Event::Rendered(second, result)));
        assert!(app.topic_presentation().is_none());
        assert_ne!(app.topic_render.as_ref().unwrap().request, second);
        app.closing = true;
        let _ = app.refresh_topic();
        assert!(app.topic_render.is_none());
    }
    #[test]
    fn read_only_topics_open_copy_and_dismiss_without_writing_or_losing_drafts() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("release".into()));
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 87.0);
        set_topic(&mut app, "**Read only**\nA long topic");
        read(&mut app);
        assert!(app.topic_editor.is_none());
        assert!(app.expanded_topic.is_some());
        let (generation, room) = app.selection.room.clone().unwrap();
        assert!(app.topic_event(Event::Copy(generation, room)).units() > 0);
        open(&mut app);
        assert!(app.topic_editor.is_none());
        assert!(app.expanded_topic.is_some());
        let _ = app.update(message(Event::Save));
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::Escape);
        assert!(app.topic_editor.is_none());
        assert_eq!(app.drafts[&key].content.text(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 87.0);
    }
    #[test]
    fn reader_links_navigate_internally_and_editor_keeps_markdown_source() {
        let mut app = demo_app();
        set_topic(
            &mut app,
            "**Design** and [general](https://matrix.to/#/!demo-general:example.org)",
        );
        read(&mut app);
        let task = app.update(Message::OpenMessageLink("https://example.org/".into()));
        assert!(task.units() > 0);
        assert!(
            app.expanded_topic.is_some(),
            "external links retain the expanded topic"
        );
        let _ = app.update(Message::OpenMessageLink(
            "https://matrix.to/#/!demo-general:example.org".into(),
        ));
        assert!(app.expanded_topic.is_none());
        assert_eq!(app.selection.room.as_ref().unwrap().1, "general");
        set_topic(&mut app, "**Source**\n- First\n- Second");
        read(&mut app);
        open(&mut app);
        let state = app.topic_editor.as_ref().unwrap();

        assert_eq!(state.content.text(), "**Source**\n- First\n- Second");
        let source = state.content.text();
        let _ = app.update(Message::QuickRoom(super::super::room_picker::Event::Open));
        assert!(app.room_picker.is_none());
        let key = app.draft_key(false).unwrap();
        let _ = app.update(Message::Send(key));
        assert_eq!(app.topic_editor.as_ref().unwrap().content.text(), source);
        let _ = app.update(Message::Escape);
        assert!(app.topic_editor.is_none());
    }
    #[test]
    fn long_reader_and_editor_keep_actions_visible_at_minimum_size() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe {
            header: Option<iced::Rectangle>,
            actions: Option<iced::Rectangle>,
            body: Option<iced::Rectangle>,
            controls: usize,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("room-topic-header")) {
                    self.header = Some(bounds);
                }
                if id == Some(&widget::Id::new("room-topic-actions"))
                    || id == Some(&widget::Id::new("expanded-room-topic-actions"))
                {
                    self.actions = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: iced::Rectangle,
                _: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-topic-body"))
                    || id == Some(&widget::Id::new("expanded-room-topic-body"))
                {
                    self.body = Some(bounds);
                }
            }
            fn focusable(
                &mut self,
                _: Option<&widget::Id>,
                _: iced::Rectangle,
                _: &mut dyn widget::operation::Focusable,
            ) {
                self.controls += 1;
            }
        }
        let mut app = demo_app();
        set_topic(&mut app, &"A long topic with many lines.\n".repeat(100));
        read(&mut app);
        for editing in [false, true] {
            if editing {
                open(&mut app);
            }
            for size in [
                iced::Size::new(640.0, 480.0),
                iced::Size::new(1400.0, 900.0),
            ] {
                let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
                let (generation, room) = app.selection.room.as_ref().unwrap();
                let mut view = if editing {
                    app.topic_dialog_sized(size)
                } else {
                    app.topic_view(*generation, room, 120.0)
                };
                let mut tree = widget::Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(if editing { size } else { iced::Size::ZERO }, size),
                );
                let mut probe = Probe::default();
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                let actions = probe.actions.unwrap();
                let body = probe.body.unwrap();
                if editing {
                    let header = probe.header.unwrap();
                    assert!(header.y >= 0.0 && header.height >= 50.0);
                    assert!(body.height >= 100.0);
                    assert!(actions.height >= 50.0);
                    assert!(body.y >= header.y + header.height);
                } else {
                    assert!(body.height <= 120.0 && body.height >= 80.0);
                    assert!(actions.height >= 24.0);
                    assert!(node.size().height <= 180.0);
                }
                assert!(actions.y + actions.height <= size.height);
                assert!(body.y + body.height <= actions.y);
                assert!(probe.controls >= 3);
            }
        }
    }
    #[test]
    fn inline_disclosure_preserves_composition_and_keeps_the_timeline_usable() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe {
            send: Option<iced::Rectangle>,
            timeline: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("composer-send")) {
                    self.send = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: iced::Rectangle,
                _: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.timeline = Some(bounds);
                }
            }
        }
        let mut app = demo_app();
        set_topic(&mut app, &"A long topic.\n".repeat(100));
        let key = app.draft_key(false).unwrap();
        app.scroll_positions.insert(key.clone(), 85.0);
        read(&mut app);
        let target = app.selection.room.clone().unwrap();
        let _ = app.update(message(Event::View(target.0 - 1, target.1.clone())));
        assert_eq!(app.expanded_topic.as_ref(), Some(&target));
        let _ = app.update(Message::Edit(
            key.clone(),
            text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("Still typing".into()))),
        ));
        assert_eq!(app.drafts[&key].content.text(), "Still typing");
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.expanded_topic.as_ref(), Some(&target));
        assert_eq!(app.scroll_positions[&key], 85.0);
        for size in [iced::Size::new(333.0, 446.0), iced::Size::new(900.0, 850.0)] {
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
            let mut view = app.pane(false);
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut probe = Probe::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            let send = probe.send.unwrap();
            assert_eq!(send.height, 32.0);
            assert!(send.y + send.height <= size.height);
            assert!(probe.timeline.unwrap().height >= 40.0);
        }
        read(&mut app);
        assert!(app.expanded_topic.is_none());
        read(&mut app);
        assert_eq!(app.expanded_topic.as_ref(), Some(&target));
        let _ = app.update(Message::Escape);
        assert!(app.expanded_topic.is_none());
        assert_eq!(app.drafts[&key].content.text(), "Still typing");
    }
}
