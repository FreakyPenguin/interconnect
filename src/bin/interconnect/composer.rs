//! Shared composer surface. Focus follows the real editor without app messages or timers.
use super::{App, DraftKey, Message, shell};
use iced::{
    Border, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree},
    },
    widget::text_editor,
};

pub(super) fn page_binding(
    press: &text_editor::KeyPress,
    composing: bool,
    key: &DraftKey,
    generation: u64,
) -> Option<text_editor::Binding<Message>> {
    use iced::keyboard::{Key, key::Named};

    if composing
        || !matches!(press.status, text_editor::Status::Focused { .. })
        || !press.modifiers.is_empty()
    {
        return None;
    }
    let up = match press.key {
        Key::Named(Named::PageUp) => true,
        Key::Named(Named::PageDown) => false,
        _ => return None,
    };
    Some(text_editor::Binding::Custom(Message::PageConversation {
        key: key.clone(),
        generation,
        up,
    }))
}

pub(super) fn editing_binding(
    press: text_editor::KeyPress,
    composing: bool,
    key: &DraftKey,
) -> Option<text_editor::Binding<Message>> {
    use iced::keyboard::{Key, Modifiers, key::Named};
    use text_editor::{Binding, Motion, Status};

    let word_modifier = if cfg!(target_os = "macos") {
        Modifiers::ALT
    } else {
        Modifiers::CTRL
    };
    if (press.modifiers - Modifiers::SHIFT) == word_modifier {
        let motion = match press.key {
            Key::Named(Named::ArrowLeft | Named::Backspace) => Some(Motion::WordLeft),
            Key::Named(Named::ArrowRight | Named::Delete) => Some(Motion::WordRight),
            _ => None,
        };
        if let Some(motion) = motion {
            if composing || !matches!(press.status, Status::Focused { .. }) {
                return None;
            }
            return Some(match press.key {
                Key::Named(Named::Backspace | Named::Delete) => {
                    Binding::Custom(Message::DeleteWord(key.clone(), motion))
                }
                _ if press.modifiers.shift() => Binding::Select(motion),
                _ => Binding::Move(motion),
            });
        }
    }
    text_editor::Binding::from_key_press(press)
}

impl App {
    pub(super) fn page_conversation(
        &mut self,
        key: DraftKey,
        generation: u64,
        up: bool,
    ) -> iced::Task<Message> {
        let thread = key.root.is_some();
        let index = usize::from(thread);
        let pane = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        if self.composing
            || !self.context_menu_allowed()
            || self.context_menu.is_some()
            || self.emoji_management.is_some()
            || !self.selection.conversation_visible
            || self.pending_scroll[index].is_some()
            || self.draft_key(thread).as_ref() != Some(&key)
            || !selected
                .as_ref()
                .is_some_and(|(view, _)| *view == generation)
            || self.drafts.get(&key).is_none_or(|draft| draft.pending)
            || !pane
                .as_ref()
                .is_some_and(|pane| pane.generation == generation)
        {
            return iced::Task::none();
        }
        let Some((view, _, metrics)) = self.timeline_metrics[index] else {
            return iced::Task::none();
        };
        if view != generation {
            return iced::Task::none();
        }
        // Leave a little overlap so the reader can keep their place. Timelines
        // are bottom-anchored, so paging up increases the saved offset.
        let page = metrics.height * 0.9;
        let offset = (self.saved_scroll(thread) + if up { page } else { -page })
            .clamp(0.0, metrics.maximum());
        self.scroll_positions.insert(key, offset);
        if thread {
            self.thread_scroll_target = Some((generation, offset));
        }
        iced::widget::operation::scroll_to(
            if thread {
                "thread-timeline"
            } else {
                "room-timeline"
            },
            iced::widget::operation::AbsoluteOffset { x: 0.0, y: offset },
        )
    }

    pub(super) fn delete_word(
        &mut self,
        key: DraftKey,
        motion: text_editor::Motion,
    ) -> iced::Task<Message> {
        use text_editor::{Action, Edit};

        if self.composing
            || self.closing
            || self.stopped
            || (self.draft_key(false).as_ref() != Some(&key)
                && self.draft_key(true).as_ref() != Some(&key))
            || self.drafts.get(&key).is_none_or(|draft| draft.pending)
        {
            return iced::Task::none();
        }
        let selection = if self.drafts[&key].content.cursor().selection.is_none() {
            self.update(Message::Edit(key.clone(), Action::Select(motion)))
        } else {
            iced::Task::none()
        };
        // At a document boundary the motion can leave no selection. Deleting
        // only a real range also keeps word deletion safe in RTL paragraphs.
        let deletion = if self.drafts[&key].content.cursor().selection.is_some() {
            self.update(Message::Edit(key, Action::Edit(Edit::Delete)))
        } else {
            iced::Task::none()
        };
        iced::Task::batch([selection, deletion])
    }
}

pub(super) fn frame<'a>(
    content: Element<'a, Message>,
    editor: &'static str,
) -> Element<'a, Message> {
    Element::new(Frame {
        content,
        editor: widget::Id::new(editor),
    })
}
struct Frame<'a> {
    content: Element<'a, Message>,
    editor: widget::Id,
}
#[derive(Default)]
struct State {
    focused: bool,
}
struct Focus<'a> {
    id: &'a widget::Id,
    focused: bool,
}
impl widget::Operation for Focus<'_> {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn focusable(
        &mut self,
        id: Option<&widget::Id>,
        _: Rectangle,
        state: &mut dyn widget::operation::Focusable,
    ) {
        if id == Some(self.id) {
            self.focused = state.is_focused();
        }
    }
}
impl Frame<'_> {
    fn sync_focus(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) -> bool {
        let mut focus = Focus {
            id: &self.editor,
            focused: false,
        };
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut focus);
        let state = tree.state.downcast_mut::<State>();
        let changed = state.focused != focus.focused;
        state.focused = focus.focused;
        changed
    }
}
impl Widget<Message, Theme, Renderer> for Frame<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
        self.sync_focus(tree, layout, renderer);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if self.sync_focus(tree, layout, renderer) {
            shell.request_redraw();
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        if !layout.bounds().intersects(viewport) {
            return;
        }
        let focused = tree.state.downcast_ref::<State>().focused;
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: Border {
                    color: if focused {
                        shell::palette(theme).accent
                    } else {
                        shell::palette(theme).line
                    },
                    width: if focused { 1.5 } else { 1.0 },
                    radius: 10.0.into(),
                },
                ..Default::default()
            },
            shell::palette(theme).background,
        );
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Key, Modifiers, key::Named};
    use iced::widget::{column, text, text_editor};

    fn word_modifier() -> Modifiers {
        if cfg!(target_os = "macos") {
            Modifiers::ALT
        } else {
            Modifiers::CTRL
        }
    }

    #[test]
    fn page_keys_scroll_the_focused_composers_conversation_and_preserve_drafts() {
        for thread in [false, true] {
            let mut app = crate::tests::demo_app();
            let _ = app.update(Message::WindowFocused(true));
            let _ = app.update(Message::Thread("review".into()));
            for pane in [&mut app.model.room, &mut app.model.thread] {
                let pane = pane.as_mut().unwrap();
                let sample = pane.rows[0].clone();
                pane.rows = std::sync::Arc::new(
                    (0..60)
                        .map(|index| {
                            let mut row = sample.clone();
                            row.key = format!("page-row-{index}");
                            row.event_id = Some(row.key.clone());
                            row.body = "A message in the scrollable conversation.".into();
                            row
                        })
                        .collect(),
                );
            }
            let key = app.draft_key(thread).unwrap();
            let other = app.draft_key(!thread).unwrap();
            let draft = &mut app.drafts.get_mut(&key).unwrap().content;
            *draft = text_editor::Content::with_text(&"A draft line\n".repeat(30));
            draft.move_to(text_editor::Cursor {
                position: text_editor::Position {
                    line: 15,
                    column: 7,
                },
                selection: Some(text_editor::Position {
                    line: 14,
                    column: 2,
                }),
            });
            let before = (draft.text(), draft.cursor(), draft.selection());
            app.drafts.get_mut(&other).unwrap().content =
                text_editor::Content::with_text("other draft");
            app.scroll_positions.insert(other.clone(), 123.0);
            let selection = app.selection.clone();

            press(&mut app, thread, Named::PageUp, Modifiers::empty());
            let metrics = app.timeline_metrics[usize::from(thread)].unwrap().2;
            assert!(metrics.maximum() > metrics.height * 2.0);
            assert!((app.saved_scroll(thread) - metrics.height * 0.9).abs() < 0.1);
            // Holding the key produces repeated presses, each advancing a page.
            press(&mut app, thread, Named::PageUp, Modifiers::empty());
            assert!((app.saved_scroll(thread) - metrics.height * 1.8).abs() < 0.1);
            press(&mut app, thread, Named::PageDown, Modifiers::empty());
            assert!((app.saved_scroll(thread) - metrics.height * 0.9).abs() < 0.1);
            press(&mut app, thread, Named::PageDown, Modifiers::empty());
            assert!(app.saved_scroll(thread) < 0.1);

            let draft = &app.drafts[&key].content;
            assert_eq!((draft.text(), draft.cursor(), draft.selection()), before);
            assert_eq!(app.drafts[&other].content.text(), "other draft");
            assert_eq!(app.scroll_positions[&other], 123.0);
            assert_eq!(app.selection.room, selection.room);
            assert_eq!(app.selection.thread, selection.thread);
            assert_eq!(app.selection.event_focus, selection.event_focus);
            assert_eq!(app.pending_ops, 0);
        }
    }

    #[test]
    fn page_binding_requires_focus_and_unmodified_keys_and_yields_to_ime() {
        let key = DraftKey {
            room: "studio".into(),
            root: None,
        };
        for named in [Named::PageUp, Named::PageDown] {
            let mut press = text_editor::KeyPress {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                modifiers: Modifiers::empty(),
                text: None,
                status: text_editor::Status::Focused { is_hovered: false },
            };
            assert!(page_binding(&press, false, &key, 42).is_some());
            assert!(page_binding(&press, true, &key, 42).is_none());
            for modifiers in [
                Modifiers::SHIFT,
                Modifiers::CTRL,
                Modifiers::ALT,
                Modifiers::LOGO,
            ] {
                press.modifiers = modifiers;
                assert!(page_binding(&press, false, &key, 42).is_none());
            }
            press.modifiers = Modifiers::SHIFT;
            assert!(matches!(
                editing_binding(press.clone(), false, &key),
                Some(text_editor::Binding::Select(_))
            ));
            press.modifiers = Modifiers::empty();
            for status in [text_editor::Status::Active, text_editor::Status::Disabled] {
                press.status = status;
                assert!(page_binding(&press, false, &key, 42).is_none());
            }
        }
    }

    #[test]
    fn conversation_paging_clamps_and_rejects_stale_or_blocked_requests() {
        for thread in [false, true] {
            for blocked in [
                "none",
                "closing",
                "stopped",
                "ime",
                "pending",
                "settings",
                "hidden",
                "restoring",
                "stale-key",
                "stale-view",
                "stale-metrics",
                "no-metrics",
            ] {
                let mut app = crate::tests::demo_app();
                let _ = app.update(Message::WindowFocused(true));
                let _ = app.update(Message::Thread("review".into()));
                let mut key = app.draft_key(thread).unwrap();
                let mut generation = if thread {
                    app.selection.thread.as_ref().unwrap().0
                } else {
                    app.selection.room.as_ref().unwrap().0
                };
                let index = usize::from(thread);
                app.timeline_metrics[index] = Some((
                    generation,
                    0,
                    super::super::timeline::Metrics {
                        height: 200.0,
                        content_height: 800.0,
                        translation: 600.0,
                    },
                ));
                match blocked {
                    "none" => {}
                    "closing" => app.closing = true,
                    "stopped" => app.stopped = true,
                    "ime" => app.composing = true,
                    "pending" => app.drafts.get_mut(&key).unwrap().pending = true,
                    "settings" => {
                        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
                    }
                    "hidden" => app.selection.conversation_visible = false,
                    "restoring" => app.pending_scroll[index] = Some(100.0),
                    "stale-key" => key.room = "different-room".into(),
                    "stale-view" => generation += 1,
                    "stale-metrics" => app.timeline_metrics[index].as_mut().unwrap().0 += 1,
                    "no-metrics" => app.timeline_metrics[index] = None,
                    _ => unreachable!(),
                }
                for _ in 0..5 {
                    let task = app.update(Message::PageConversation {
                        key: key.clone(),
                        generation,
                        up: true,
                    });
                    assert_eq!(task.units() > 0, blocked == "none", "{thread} {blocked}");
                }
                assert_eq!(
                    app.saved_scroll(thread),
                    if blocked == "none" { 600.0 } else { 0.0 }
                );
                for _ in 0..5 {
                    let _ = app.update(Message::PageConversation {
                        key: key.clone(),
                        generation,
                        up: false,
                    });
                }
                assert_eq!(app.saved_scroll(thread), 0.0);
                assert_eq!(app.saved_scroll(!thread), 0.0);
            }
        }
    }

    fn press(app: &mut App, thread: bool, key: Named, modifiers: Modifiers) {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(640.0, 480.0);
        let mut view = app.pane(thread);
        let mut tree = Tree::new(&view);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        view.as_widget_mut().operate(
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
        let event = Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers,
            // Some platforms report control text; motions must still work.
            text: Some("\0".into()),
            repeat: false,
        });
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &event,
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(size),
        );
        assert!(shell.is_event_captured());
        assert!(!matches!(
            shell.redraw_request(),
            iced::window::RedrawRequest::At(_)
        ));
        drop(view);
        for message in messages {
            assert!(!matches!(message, Message::Send(_)));
            let _ = app.update(message);
        }
    }

    #[test]
    fn word_shortcuts_edit_only_the_focused_room_or_thread_draft() {
        use text_editor::{Cursor, Position};

        for thread in [false, true] {
            let mut app = crate::tests::demo_app();
            let _ = app.update(Message::Thread("review".into()));
            let key = app.draft_key(thread).unwrap();
            let other = app.draft_key(!thread).unwrap();
            app.drafts.get_mut(&other).unwrap().content =
                text_editor::Content::with_text("other draft");
            let selection = app.selection.clone();
            let scroll = app.scroll_positions.clone();
            // Text, caret (line/byte), selection anchor, key, Shift,
            // expected text, expected caret, expected selected text.
            for (text, caret, anchor, named, shift, expected, end, selected) in [
                (
                    "one two three",
                    (0, 7),
                    None,
                    Named::Backspace,
                    false,
                    "one  three",
                    (0, 4),
                    None,
                ),
                (
                    "one two three",
                    (0, 4),
                    None,
                    Named::Delete,
                    false,
                    "one  three",
                    (0, 4),
                    None,
                ),
                (
                    "one two three",
                    (0, 7),
                    None,
                    Named::ArrowLeft,
                    false,
                    "one two three",
                    (0, 4),
                    None,
                ),
                (
                    "one two three",
                    (0, 4),
                    None,
                    Named::ArrowRight,
                    false,
                    "one two three",
                    (0, 7),
                    None,
                ),
                (
                    "one two three",
                    (0, 7),
                    None,
                    Named::ArrowLeft,
                    true,
                    "one two three",
                    (0, 4),
                    Some("two"),
                ),
                (
                    "one two three",
                    (0, 4),
                    None,
                    Named::ArrowRight,
                    true,
                    "one two three",
                    (0, 7),
                    Some("two"),
                ),
                (
                    "one two",
                    (0, 0),
                    None,
                    Named::Backspace,
                    false,
                    "one two",
                    (0, 0),
                    None,
                ),
                (
                    "one two",
                    (0, 7),
                    None,
                    Named::Delete,
                    false,
                    "one two",
                    (0, 7),
                    None,
                ),
                ("", (0, 0), None, Named::Backspace, false, "", (0, 0), None),
                ("", (0, 0), None, Named::Delete, false, "", (0, 0), None),
                (
                    "one two",
                    (0, 6),
                    Some((0, 5)),
                    Named::Backspace,
                    false,
                    "one to",
                    (0, 5),
                    None,
                ),
                (
                    "one two",
                    (0, 5),
                    Some((0, 6)),
                    Named::Delete,
                    false,
                    "one to",
                    (0, 5),
                    None,
                ),
                (
                    "café naïve",
                    (0, 12),
                    None,
                    Named::Backspace,
                    false,
                    "café ",
                    (0, 6),
                    None,
                ),
                (
                    "e\u{301}lan 🙂 tail",
                    (0, 6),
                    None,
                    Named::Backspace,
                    false,
                    " 🙂 tail",
                    (0, 0),
                    None,
                ),
                (
                    "one,  two!",
                    (0, 6),
                    None,
                    Named::Backspace,
                    false,
                    "two!",
                    (0, 0),
                    None,
                ),
                (
                    "one\ntwo",
                    (1, 0),
                    None,
                    Named::Backspace,
                    false,
                    "onetwo",
                    (0, 3),
                    None,
                ),
                (
                    "one\ntwo",
                    (0, 3),
                    None,
                    Named::Delete,
                    false,
                    "onetwo",
                    (0, 3),
                    None,
                ),
                (
                    "שלום",
                    (0, 8),
                    None,
                    Named::Backspace,
                    false,
                    "שלום",
                    (0, 8),
                    None,
                ),
                (
                    "שלום",
                    (0, 0),
                    None,
                    Named::Delete,
                    false,
                    "שלום",
                    (0, 0),
                    None,
                ),
            ] {
                let content = &mut app.drafts.get_mut(&key).unwrap().content;
                *content = text_editor::Content::with_text(text);
                content.move_to(Cursor {
                    position: Position {
                        line: caret.0,
                        column: caret.1,
                    },
                    selection: anchor.map(|(line, column)| Position { line, column }),
                });
                press(
                    &mut app,
                    thread,
                    named,
                    word_modifier()
                        | if shift {
                            Modifiers::SHIFT
                        } else {
                            Modifiers::empty()
                        },
                );
                let content = &app.drafts[&key].content;
                assert_eq!(content.text(), expected, "{thread} {named:?} {text:?}");
                assert_eq!(
                    content.cursor().position,
                    Position {
                        line: end.0,
                        column: end.1
                    },
                    "{named:?} {text:?}"
                );
                assert_eq!(
                    content.selection().as_deref(),
                    selected,
                    "{named:?} {text:?}"
                );
                assert_eq!(app.drafts[&other].content.text(), "other draft");
                assert_eq!(app.selection, selection);
                assert_eq!(app.scroll_positions, scroll);
                assert_eq!(app.pending_ops, 0);
            }
        }
    }

    #[test]
    fn word_shortcuts_require_focus_and_yield_during_ime_composition() {
        for named in [
            Named::ArrowLeft,
            Named::ArrowRight,
            Named::Backspace,
            Named::Delete,
        ] {
            for status in [
                text_editor::Status::Active,
                text_editor::Status::Disabled,
                text_editor::Status::Focused { is_hovered: false },
            ] {
                let key = DraftKey {
                    room: "studio".into(),
                    root: None,
                };
                let press = text_editor::KeyPress {
                    key: Key::Named(named),
                    modified_key: Key::Named(named),
                    physical_key: iced::keyboard::key::Physical::Unidentified(
                        iced::keyboard::key::NativeCode::Unidentified,
                    ),
                    modifiers: word_modifier(),
                    text: None,
                    status,
                };
                assert!(editing_binding(press.clone(), true, &key).is_none());
                assert_eq!(
                    editing_binding(press, false, &key).is_some(),
                    matches!(status, text_editor::Status::Focused { .. })
                );
            }
        }
    }

    #[test]
    fn word_deletion_rejects_blocked_or_stale_drafts() {
        for blocked in ["closing", "stopped", "ime", "pending", "settings", "stale"] {
            let mut app = crate::tests::demo_app();
            let key = app.draft_key(false).unwrap();
            let content = &mut app.drafts.get_mut(&key).unwrap().content;
            *content = text_editor::Content::with_text("one two");
            content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
            let before = content.cursor();
            match blocked {
                "closing" => app.closing = true,
                "stopped" => app.stopped = true,
                "ime" => app.composing = true,
                "pending" => app.drafts.get_mut(&key).unwrap().pending = true,
                "settings" => {
                    let _ = app.update(Message::Settings(super::super::settings::Event::Open));
                }
                "stale" => {
                    let _ = app.update(Message::Room("home".into()));
                }
                _ => unreachable!(),
            }
            let _ = app.update(Message::DeleteWord(
                key.clone(),
                text_editor::Motion::WordLeft,
            ));
            let content = &app.drafts[&key].content;
            assert_eq!(content.text(), "one two", "{blocked}");
            assert_eq!(content.cursor(), before, "{blocked}");
        }
    }

    #[test]
    fn surface_tracks_editor_focus_and_settles_without_timers() {
        let content = text_editor::Content::with_text("A draft\nwith several\nlines");
        let editor = text_editor(&content)
            .id("room-composer")
            .height(56)
            .cursor_blinking(false)
            .style(shell::composer_editor_style)
            .on_action(|_| Message::Escape);
        let mut view = frame(column![editor, text("Toolbar")].into(), "room-composer");
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view);
        let size = Size::new(300.0, 200.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        for focused in [true, false, true] {
            let mut operation = if focused {
                widget::operation::focusable::focus(widget::Id::new("room-composer"))
            } else {
                widget::operation::focusable::focus(widget::Id::new("another-field"))
            };
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut operation);
            assert_eq!(tree.state.downcast_ref::<State>().focused, focused);
            for milliseconds in [0, 750, 1500] {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &Event::Window(iced::window::Event::RedrawRequested(
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
        assert_eq!(content.text(), "A draft\nwith several\nlines");
    }
}
