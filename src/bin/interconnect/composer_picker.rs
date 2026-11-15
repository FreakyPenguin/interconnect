//! Insert picker selections into a captured draft; no SDK or send operations.
use super::{App, DraftKey, Message, emoji_picker, popover, shell};
use iced::{
    Element, Task,
    widget::{button, container, text, text_editor, tooltip},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    key: DraftKey,
    generation: u64,
}
pub struct State {
    target: Target,
    picker: emoji_picker::State,
    cursor: iced::advanced::text::editor::Cursor,
}
impl State {
    pub fn dismiss(&self) -> Message {
        Message::ComposerEmoji(Event::Picker {
            target: self.target.clone(),
            event: emoji_picker::Event::Close,
        })
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(Target),
    Picker {
        target: Target,
        event: emoji_picker::Event,
    },
}
fn focus(target: &Target) -> Task<Message> {
    iced::widget::operation::focus(if target.key.root.is_some() {
        "thread-composer"
    } else {
        "room-composer"
    })
}
impl App {
    fn composer_picker_valid(&self, target: &Target) -> bool {
        let thread = target.key.root.is_some();
        let selected = if thread {
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
            && self.settings.draft.is_none()
            && self.image_view.is_none()
            && self.reaction_picker.is_none()
            && selected
                .as_ref()
                .is_some_and(|(generation, _)| *generation == target.generation)
            && self.draft_key(thread).as_ref() == Some(&target.key)
            && super::current_pane(pane, target.generation).is_some_and(|pane| pane.error.is_none())
            && self
                .drafts
                .get(&target.key)
                .is_some_and(|draft| !draft.pending)
            && (thread
                || !self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width)))
    }
    pub(super) fn validate_composer_picker(&mut self) {
        if self
            .composer_picker
            .as_ref()
            .is_some_and(|state| !self.composer_picker_valid(&state.target))
        {
            self.composer_picker = None;
        }
    }
    pub(super) fn composer_picker_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open(target)
                if self.composer_picker.is_none()
                    && !self.composing
                    && self.composer_picker_valid(&target) =>
            {
                let cursor = self.drafts[&target.key].content.cursor();
                self.composer_picker = Some(State {
                    target,
                    picker: emoji_picker::State::new(emoji_picker::unicode()),
                    cursor,
                });
                return iced::widget::operation::focus(emoji_picker::SEARCH_ID);
            }
            Event::Picker { target, event }
                if self
                    .composer_picker
                    .as_ref()
                    .is_some_and(|state| state.target == target) =>
            {
                if matches!(event, emoji_picker::Event::Close) {
                    self.composer_picker = None;
                    return focus(&target);
                }
                if !self.composer_picker_valid(&target) {
                    self.composer_picker = None;
                    return Task::none();
                }
                if self.composing && matches!(event, emoji_picker::Event::Select(_)) {
                    return Task::none();
                }
                let state = self.composer_picker.as_mut().unwrap();
                if let Some(key) = state.picker.update(emoji_picker::unicode(), event)
                    && let Some(entry) = emoji_picker::unicode().get(&key)
                {
                    let draft = self.drafts.get_mut(&target.key).unwrap();
                    draft.content.move_to(state.cursor);
                    draft
                        .content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            std::sync::Arc::new(entry.glyph.clone()),
                        )));
                    self.composer_picker = None;
                    return focus(&target);
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub(super) fn composer_emoji_button(
        &self,
        key: &DraftKey,
        generation: u64,
    ) -> Element<'_, Message> {
        let target = Target {
            key: key.clone(),
            generation,
        };
        let mut open = button(container(text("☺").size(18)).center_x(26).center_y(26))
            .padding(0)
            .style(shell::icon_button);
        if !self.composing && self.composer_picker_valid(&target) && self.composer_picker.is_none()
        {
            open = open.on_press(Message::ComposerEmoji(Event::Open(target)));
        }
        tooltip(
            container(open).id("composer-emoji"),
            container(text("Insert emoji").size(12))
                .padding(8)
                .style(container::rounded_box),
            tooltip::Position::Top,
        )
        .into()
    }
    pub(super) fn composer_emoji_host<'a>(
        &'a self,
        composer: impl Into<Element<'a, Message>>,
        key: &DraftKey,
        generation: u64,
    ) -> Element<'a, Message> {
        let state = self
            .composer_picker
            .as_ref()
            .filter(|state| &state.target.key == key && state.target.generation == generation);
        if let Some(state) = state {
            let picker =
                emoji_picker::view(&state.picker, emoji_picker::unicode(), true, |event| {
                    Message::ComposerEmoji(Event::Picker {
                        target: state.target.clone(),
                        event,
                    })
                });
            return popover::view(
                composer,
                Some(iced::advanced::widget::Id::new("composer-emoji")),
                Some(emoji_picker::panel(picker)),
                state.dismiss(),
            );
        }
        let (popup, dismiss) = self
            .emoji_completion_popup(key, generation)
            .map_or((None, Message::Escape), |(popup, dismiss)| {
                (Some(popup), dismiss)
            });
        popover::suggestions(composer, popup, dismiss)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::text::editor::{Cursor, Position};
    fn target(app: &App, thread: bool) -> Target {
        Target {
            key: app.draft_key(thread).unwrap(),
            generation: if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            },
        }
    }
    fn open(app: &mut App, target: &Target) {
        let _ = app.update(Message::ComposerEmoji(Event::Open(target.clone())));
        assert!(app.composer_picker.is_some());
    }
    fn pick(app: &mut App, target: &Target, key: &str) {
        let _ = app.update(Message::ComposerEmoji(Event::Picker {
            target: target.clone(),
            event: emoji_picker::Event::Select(key.into()),
        }));
    }
    #[test]
    fn inserts_complete_emoji_at_the_caret_or_over_selection_in_the_captured_draft() {
        for thread in [false, true] {
            let mut app = crate::tests::demo_app();
            let _ = app.update(Message::Thread("review".into()));
            let target = target(&app, thread);
            let other = app.draft_key(!thread).unwrap();
            app.drafts.get_mut(&other).unwrap().content =
                text_editor::Content::with_text("other draft");
            let draft = app.drafts.get_mut(&target.key).unwrap();
            draft.content = text_editor::Content::with_text("Hello world!\nsecond line");
            draft.content.move_to(Cursor {
                position: Position { line: 0, column: 6 },
                selection: Some(Position {
                    line: 0,
                    column: 11,
                }),
            });
            let selection = app.selection.clone();
            let scroll = app.scroll_positions.clone();
            let messages = app.model.room.as_ref().unwrap().rows.len();
            open(&mut app, &target);
            pick(&mut app, &target, "👩🏽‍💻");
            assert!(app.composer_picker.is_none());
            assert_eq!(
                app.drafts[&target.key].content.text(),
                "Hello 👩🏽‍💻!\nsecond line"
            );
            assert!(app.drafts[&target.key].content.selection().is_none());
            open(&mut app, &target);
            pick(&mut app, &target, "❤️");
            assert_eq!(
                app.drafts[&target.key].content.text(),
                "Hello 👩🏽‍💻❤️!\nsecond line"
            );
            assert_eq!(app.drafts[&other].content.text(), "other draft");
            assert!(!app.drafts[&target.key].pending);
            assert_eq!(app.pending_ops, 0);
            assert_eq!(app.model.room.as_ref().unwrap().rows.len(), messages);
            assert_eq!(app.selection, selection);
            assert_eq!(app.scroll_positions, scroll);
        }
    }
    #[test]
    fn cancellation_preserves_selection_and_stale_or_blocked_events_cannot_edit_a_draft() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let old = target(&app, true);
        let draft = app.drafts.get_mut(&old.key).unwrap();
        draft.content = text_editor::Content::with_text("keep selected text");
        draft.content.perform(text_editor::Action::SelectAll);
        let cursor = draft.content.cursor();
        open(&mut app, &old);
        let _ = app.update(Message::Send(old.key.clone()));
        let _ = app.update(Message::Edit(
            old.key.clone(),
            text_editor::Action::Edit(text_editor::Edit::Insert('x')),
        ));
        let _ = app.update(Message::Room("home".into()));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
        let _ = app.update(Message::Escape);
        assert!(app.composer_picker.is_none());
        assert!(
            app.selection.thread.is_some(),
            "Escape only dismisses the picker"
        );
        assert_eq!(app.drafts[&old.key].content.cursor(), cursor);
        assert_eq!(app.drafts[&old.key].content.text(), "keep selected text");
        let _ = app.update(Message::Room("home".into()));
        pick(&mut app, &old, "🚀");
        let _ = app.update(Message::ComposerEmoji(Event::Open(old.clone())));
        assert!(app.composer_picker.is_none());
        assert_eq!(app.drafts[&old.key].content.text(), "keep selected text");
        let current = target(&app, false);
        app.drafts.get_mut(&current.key).unwrap().pending = true;
        let _ = app.update(Message::ComposerEmoji(Event::Open(current.clone())));
        assert!(app.composer_picker.is_none());
        app.drafts.get_mut(&current.key).unwrap().pending = false;
        open(&mut app, &current);
        let _ = app.update(Message::Composing(true));
        pick(&mut app, &current, "🚀");
        assert!(app.composer_picker.is_some());
        let _ = app.update(Message::Composing(false));
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert!(app.composer_picker.is_none());
        let _ = app.update(Message::ComposerEmoji(Event::Open(current)));
        assert!(app.composer_picker.is_none());
    }
    #[test]
    fn hidden_or_removed_composers_dismiss_the_picker_and_reaction_pickers_are_exclusive() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let room = target(&app, false);
        open(&mut app, &room);
        let reaction = app
            .reaction_action(
                app.thread_original().unwrap(),
                true,
                app.selection.thread.as_ref().unwrap().0,
            )
            .unwrap();
        let _ = app.update(reaction.clone());
        assert!(app.reaction_picker.is_none());
        let _ = app.update(Message::WindowResized(640.0));
        assert!(app.composer_picker.is_none());
        let thread = target(&app, true);
        open(&mut app, &thread);
        let mut model = app.model.clone();
        model.thread = None;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(
            app.composer_picker.is_none(),
            "a missing pane removes its popup host"
        );
        app.refresh_demo();
        open(&mut app, &thread);
        let mut model = app.model.clone();
        model.rooms = std::sync::Arc::default();
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.composer_picker.is_none());

        let mut app = crate::tests::demo_app();
        let row = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.key == "review")
            .unwrap();
        let reaction = app
            .reaction_action(row, false, app.selection.room.as_ref().unwrap().0)
            .unwrap();
        let _ = app.update(reaction);
        let room = target(&app, false);
        let _ = app.update(Message::ComposerEmoji(Event::Open(room)));
        assert!(app.composer_picker.is_none());
        assert!(app.reaction_picker.is_some());
    }
    #[test]
    fn muted_button_is_left_of_send_and_mouse_or_enter_inserts_without_sending() {
        use iced::advanced::{
            Layout, Shell, layout, mouse,
            widget::{self, Tree},
        };
        #[derive(Default)]
        struct Probe {
            open: Option<iced::Rectangle>,
            send: Option<iced::Rectangle>,
            choice: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                for (name, target) in [
                    ("composer-emoji", &mut self.open),
                    ("composer-send", &mut self.send),
                    ("emoji-choice:🚀", &mut self.choice),
                ] {
                    if id == Some(&widget::Id::new(name)) {
                        *target = Some(bounds);
                    }
                }
            }
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for thread in [false, true] {
            let mut app = crate::tests::demo_app();
            let _ = app.update(Message::Thread("review".into()));
            app.settings.current.composer.send_shortcut =
                interconnect::settings::SendShortcut::Enter;
            let target = target(&app, thread);
            app.drafts.get_mut(&target.key).unwrap().content =
                text_editor::Content::with_text("draft");
            let messages_before = app.model.room.as_ref().unwrap().rows.len();
            let mut tree = Tree::empty();
            let size = iced::Size::new(300.0, 480.0);
            let mut view = app.pane(thread);
            tree.diff(view.as_widget());
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut probe = Probe::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            let button = probe.open.unwrap();
            let send = probe.send.unwrap();
            assert_eq!(button.width, 26.0);
            assert_eq!(button.height, 26.0);
            assert!(button.x + button.width < send.x);
            assert_eq!(button.y, send.y);
            let cursor = mouse::Cursor::Available(button.center());
            let mut messages = Vec::new();
            for event in [
                iced::Event::Mouse(mouse::Event::CursorMoved {
                    position: button.center(),
                }),
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ] {
                view.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &iced::Rectangle::with_size(size),
                );
            }
            drop(view);
            let open = messages
                .into_iter()
                .find(|message| matches!(message, Message::ComposerEmoji(Event::Open(_))))
                .unwrap();
            let _ = app.update(open);
            if thread {
                let _ = app.update(Message::ComposerEmoji(Event::Picker {
                    target: target.clone(),
                    event: emoji_picker::Event::Search(":rocket:".into()),
                }));
            }
            let mut view = app.pane(thread);
            tree.diff(view.as_widget());
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut overlay_element = view
                .as_widget_mut()
                .overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &iced::Rectangle::with_size(size),
                    iced::Vector::ZERO,
                )
                .unwrap();
            let overlay = overlay_element.as_overlay_mut();
            let node = overlay.layout(&renderer, size);
            overlay.operate(
                Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus(emoji_picker::SEARCH_ID.into()),
            );
            let mut probe = Probe::default();
            overlay.operate(Layout::new(&node), &renderer, &mut probe);
            let choice = probe.choice.expect("picker immediately visible");
            assert!(choice.y >= 0.0 && choice.y + choice.height <= size.height);
            let events = if thread {
                vec![iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                    modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                    physical_key: iced::keyboard::key::Physical::Unidentified(
                        iced::keyboard::key::NativeCode::Unidentified,
                    ),
                    location: iced::keyboard::Location::Standard,
                    modifiers: iced::keyboard::Modifiers::default(),
                    text: None,
                    repeat: false,
                })]
            } else {
                vec![
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                ]
            };
            let mut messages = Vec::new();
            for event in events {
                overlay.update(
                    &event,
                    Layout::new(&node),
                    mouse::Cursor::Available(choice.center()),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                );
            }
            drop(overlay_element);
            drop(view);
            for message in messages {
                let _ = app.update(message);
            }
            assert!(app.composer_picker.is_none());
            assert_eq!(app.drafts[&target.key].content.text(), "🚀draft");
            assert_eq!(app.model.room.as_ref().unwrap().rows.len(), messages_before);
            assert!(!app.drafts[&target.key].pending);
        }
    }
}
