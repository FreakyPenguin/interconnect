//! Room topic presentation and an asynchronous editor bound to a room generation.
use super::{App, MAX_PENDING_OPERATIONS, Message, current_pane, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, row, text, text_editor, tooltip},
};

pub struct State {
    target: (u64, String),
    request: u64,
    content: text_editor::Content,
    pending: bool,
    error: Option<String>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(u64, String),
    Edit(text_editor::Action),
    Save,
    Cancel,
    Saved((u64, String), u64, Result<(), String>),
}
fn message(event: Event) -> Message {
    Message::Topic(event)
}
impl App {
    fn topic_editable(&self, target: &(u64, String)) -> bool {
        !self.closing
            && !self.stopped
            && self.selection.room.as_ref() == Some(target)
            && self
                .model
                .rooms
                .iter()
                .any(|room| room.id == target.1 && room.can_edit_topic)
    }
    pub(super) fn validate_topic_editor(&mut self) {
        if self
            .topic_editor
            .as_ref()
            .is_some_and(|state| !self.topic_editable(&state.target))
        {
            self.topic_editor = None;
        }
    }
    pub(super) fn topic_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open(generation, room) => {
                let target = (generation, room);
                if !self.topic_editable(&target)
                    || self.topic_editor.is_some()
                    || self.settings.draft.is_some()
                    || self.verification.open
                    || self.image_view.is_some()
                    || self.reaction_picker.is_some()
                    || self.composer_picker.is_some()
                {
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
                    request: self.generation,
                    content: text_editor::Content::with_text(topic),
                    pending: false,
                    error: None,
                });
                self.emoji_completion.clear();
                return iced::widget::operation::focus("room-topic");
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
            Event::Save => {
                let Some(state) = &self.topic_editor else {
                    return Task::none();
                };
                if state.pending
                    || !self.topic_editable(&state.target)
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                {
                    return Task::none();
                }
                let target = state.target.clone();
                let request = state.request;
                let topic = state.content.text();
                if self.demo.is_some() {
                    if let Some(room) = std::sync::Arc::make_mut(&mut self.model.rooms)
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
                    state.target == target && state.request == request && state.pending
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
        }
        Task::none()
    }
    pub(super) fn topic_view<'a>(&'a self, generation: u64, room_id: &str) -> Element<'a, Message> {
        if let Some(state) = &self.topic_editor {
            let mut save = button(text(if state.pending { "Saving…" } else { "Save" }).size(12))
                .style(shell::quiet_button);
            let mut cancel = button(text("Cancel").size(12)).style(shell::quiet_button);
            if !state.pending {
                cancel = cancel.on_press(message(Event::Cancel));
                if self.pending_ops < MAX_PENDING_OPERATIONS
                    && (self.demo.is_some()
                        || current_pane(&self.model.room, generation)
                            .is_some_and(|pane| pane.composer.is_some()))
                {
                    save = save.on_press(message(Event::Save));
                }
            }
            let editor = text_editor(&state.content)
                .id("room-topic")
                .cursor_blinking(false)
                .style(shell::editor_style)
                .placeholder("Room topic")
                .size(12)
                .height(64)
                .on_action(|action| message(Event::Edit(action)));
            let mut content = column![editor, row![save, cancel].spacing(6)].spacing(5);
            if let Some(error) = &state.error {
                content = content.push(text(error).size(12));
            }
            return content.into();
        }
        let room = self.model.rooms.iter().find(|room| room.id == room_id);
        let topic = room
            .map(|room| room.topic.as_str())
            .filter(|topic| !topic.trim().is_empty())
            .unwrap_or("No topic");
        let label = super::tooltips::tooltip(
            text(topic.replace(['\n', '\r'], " "))
                .width(Length::Fill)
                .size(12)
                .color(shell::MUTED)
                .wrapping(text::Wrapping::None),
            topic,
            tooltip::Position::Bottom,
        );
        let mut content = row![label].spacing(6).align_y(iced::Alignment::Center);
        if room.is_some_and(|room| room.can_edit_topic) && !self.closing && !self.stopped {
            content = content.push(super::tooltips::tooltip(
                button(text("✎").size(14))
                    .width(24)
                    .height(24)
                    .padding([0, 4])
                    .style(shell::icon_button)
                    .on_press(message(Event::Open(generation, room_id.to_owned()))),
                "Edit room topic",
                tooltip::Position::Bottom,
            ));
        }
        content.width(Length::Fill).into()
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
    fn long_topic_keeps_the_edit_button_inside_a_narrow_header() {
        use iced::advanced::{layout, widget};
        let mut app = demo_app();
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .topic = "A very long topic with many details ".repeat(30);
        let (generation, room) = app.selection.room.as_ref().unwrap();
        let mut view = app.topic_view(*generation, room);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(240.0, 300.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        let pencil = node.children().last().unwrap();
        assert!(pencil.size().width > 10.0);
        assert!(pencil.bounds().x + pencil.size().width <= 240.0);
        assert!(node.size().height < 40.0);
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
}
