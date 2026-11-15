//! Read-only message text, using Iced's Unicode-aware selection engine.
//! Selection lives in the widget tree, never in SDK snapshots or drafts.
use super::{Message, shell};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
    widget::{self, Tree},
};
use iced::widget::text_editor::{self, Action, Binding};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme};
use std::borrow::Cow;

#[derive(Clone, Copy)]
enum Appearance {
    Message,
    Activity,
}

#[cfg(test)]
pub fn view<'a>(id: &'a str, body: &'a str) -> Element<'a, Message> {
    named_view(id, body, "")
}
pub fn named_view<'a>(id: &'a str, body: &'a str, name: &'a str) -> Element<'a, Message> {
    Element::new(Selectable {
        id,
        body: Cow::Borrowed(body),
        name,
        appearance: Appearance::Message,
    })
}
pub fn activity_view<'a>(id: &'a str, body: &'a str, name: &'a str) -> Element<'a, Message> {
    Element::new(Selectable {
        id,
        body: Cow::Owned(format!("{name} · {body}")),
        // The inline name is already part of the selectable/copyable text.
        name: "",
        appearance: Appearance::Activity,
    })
}
struct Selectable<'a> {
    name: &'a str,
    id: &'a str,
    body: Cow<'a, str>,
    appearance: Appearance,
}
pub(super) struct State {
    pub id: String,
    pub name: String,
    pub body: String,
    pub content: text_editor::Content,
    pub selected: bool,
    pub clicks: u64,
}
// The editor exposes raw word/line anchors through cursor(), but its copy()
// expands them. Normalize to explicit logical positions so the timeline can
// preserve exactly the same selection across later layouts.
fn normalize_selection(content: &mut text_editor::Content) {
    use iced::advanced::text::editor::{Cursor, Position};
    let Some(selected) = content.selection().filter(|text| !text.is_empty()) else {
        return;
    };
    let lines: Vec<_> = content.lines().map(|line| line.text.into_owned()).collect();
    let position = content.cursor().position;
    let offset = lines
        .iter()
        .take(position.line)
        .map(|line| line.len() + 1)
        .sum::<usize>()
        + position.column;
    let normalized = lines.join("\n");
    if let Some((begin, _)) = normalized
        .match_indices(&selected)
        .find(|(begin, _)| *begin <= offset && offset <= *begin + selected.len())
    {
        let logical = |offset: usize| {
            let prefix = &normalized[..offset];
            Position {
                line: prefix.bytes().filter(|byte| *byte == b'\n').count(),
                column: prefix.rsplit('\n').next().unwrap_or("").len(),
            }
        };
        let cursor = Cursor {
            position: logical(begin + selected.len()),
            selection: Some(logical(begin)),
        };
        content.move_to(cursor);
    }
}
fn editor(
    content: &text_editor::Content,
    appearance: Appearance,
) -> text_editor::TextEditor<'_, iced::advanced::text::highlighter::PlainText, Action> {
    iced::widget::text_editor(content)
        .size(match appearance {
            Appearance::Message => 14,
            Appearance::Activity => 12,
        })
        .padding(0)
        .cursor_blinking(false)
        .show_cursor(false)
        .on_action(|action| action)
        .key_binding(|key| {
            Binding::from_key_press(key).filter(|binding| {
                matches!(
                    binding,
                    Binding::Copy
                        | Binding::Move(_)
                        | Binding::Select(_)
                        | Binding::SelectAll
                        | Binding::SelectWord
                        | Binding::SelectLine
                        | Binding::Unfocus
                )
            })
        })
        .style(move |_, _| text_editor::Style {
            background: iced::Color::TRANSPARENT.into(),
            border: iced::Border::default(),
            value: match appearance {
                Appearance::Message => shell::INK,
                Appearance::Activity => shell::MUTED,
            },
            placeholder: shell::MUTED,
            selection: iced::Color::from_rgba8(73, 121, 211, 0.25),
        })
}
impl Widget<Message, Theme, Renderer> for Selectable<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            id: self.id.into(),
            name: self.name.into(),
            selected: false,
            clicks: 0,
            body: self.body.to_string(),
            content: text_editor::Content::with_text(&self.body),
        })
    }
    fn children(&self) -> Vec<Tree> {
        let content = text_editor::Content::new();
        vec![Tree::new(Element::from(editor(&content, self.appearance)))]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        state.name = self.name.into();
        if state.id != self.id || state.body != self.body.as_ref() {
            tree.state = self.state();
            tree.children = self.children();
        }
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        use iced::advanced::widget::operation::Focusable;
        let state = tree.state.downcast_mut::<State>();
        let was_selected = state.selected;
        operation.custom(None, layout.bounds(), state);
        if was_selected && !state.selected {
            tree.children[0]
                .state
                .downcast_mut::<text_editor::State<iced::advanced::text::highlighter::PlainText>>()
                .unfocus();
        }
        if state.selected {
            tree.children[0]
                .state
                .downcast_mut::<text_editor::State<iced::advanced::text::highlighter::PlainText>>()
                .focus();
        }
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_ref::<State>();
        editor(&state.content, self.appearance).layout(&mut tree.children[0], renderer, limits)
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
        // Wheel input belongs to the conversation; IME can never edit a message.
        if matches!(
            event,
            Event::InputMethod(_) | Event::Mouse(mouse::Event::WheelScrolled { .. })
        ) {
            return;
        }
        let state = tree.state.downcast_mut::<State>();
        let mut actions = Vec::new();
        let mut local = Shell::new(&mut actions);
        editor(&state.content, self.appearance).update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local,
            viewport,
        );
        if local.is_event_captured() {
            shell.capture_event();
        }
        shell.request_redraw_at(local.redraw_request());
        for action in actions {
            if !action.is_edit() {
                if matches!(
                    action,
                    Action::Click(_) | Action::SelectWord | Action::SelectLine
                ) {
                    state.clicks = state.clicks.wrapping_add(1);
                }
                let expand = matches!(action, Action::SelectWord | Action::SelectLine);
                state.content.perform(action);
                if expand {
                    normalize_selection(&mut state.content);
                }
                shell.request_redraw();
            }
        }
        // Do not forward the editor's input-method request: this is read-only text.
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        editor(&state.content, self.appearance).draw(
            &tree.children[0],
            renderer,
            theme,
            defaults,
            layout,
            cursor,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        _: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        _: &Renderer,
    ) -> mouse::Interaction {
        if cursor
            .position()
            .is_some_and(|p| layout.bounds().contains(p) && viewport.contains(p))
        {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct ClipboardText(String);
    impl Clipboard for ClipboardText {
        fn read(&self, _: iced::advanced::clipboard::Kind) -> Option<String> {
            Some(self.0.clone())
        }
        fn write(&mut self, _: iced::advanced::clipboard::Kind, text: String) {
            self.0 = text;
        }
    }
    #[test]
    fn partial_selection_copies_but_cannot_edit_or_schedule_a_timer() {
        let mut view = Selectable {
            id: "$one",
            name: "Alice",
            body: Cow::Borrowed("hello world café 👋"),
            appearance: Appearance::Message,
        };
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
        let node = view.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 300.0)),
        );
        let mut clipboard = ClipboardText::default();
        let mut dispatch = |event, point| {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, 300.0)),
            );
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
            assert!(matches!(
                shell.input_method(),
                iced::advanced::InputMethod::Disabled
            ));
        };
        let origin = iced::Point::new(0.0, 5.0);
        dispatch(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            origin,
        );
        let end = iced::Point::new(32.0, 5.0);
        dispatch(
            Event::Mouse(mouse::Event::CursorMoved { position: end }),
            end,
        );
        dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            end,
        );
        let key = |value: &str, modifiers| {
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character(value.into()),
                modified_key: iced::keyboard::Key::Character(value.into()),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers,
                text: Some(value.into()),
                repeat: false,
            })
        };
        dispatch(key("c", iced::keyboard::Modifiers::COMMAND), end);
        dispatch(key("x", iced::keyboard::Modifiers::COMMAND), end);
        dispatch(key("v", iced::keyboard::Modifiers::COMMAND), end);
        dispatch(key("z", iced::keyboard::Modifiers::empty()), end);
        dispatch(
            Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            end,
        );
        assert!(!clipboard.0.is_empty());
        assert!(clipboard.0.len() < view.body.len());
        assert!(view.body.starts_with(&clipboard.0));
        assert_eq!(tree.state.downcast_ref::<State>().content.text(), view.body);
        // A different event taking the same widget slot cannot inherit selection.
        view.id = "$two";
        view.diff(&mut tree);
        assert!(
            tree.state
                .downcast_ref::<State>()
                .content
                .selection()
                .is_none()
        );
    }
    #[test]
    fn selection_and_reply_work_inside_a_scrolled_hover_row() {
        use iced::widget::{column, container, scrollable, space};
        let mut element: Element<'_, Message> = scrollable(column![
            space().height(400),
            shell::message_actions(
                container(view("$one", "hello world café 👋")).padding(iced::Padding {
                    top: 5.0,
                    ..iced::Padding::ZERO
                }),
                "$one",
                true,
            )
        ])
        .anchor_bottom()
        .height(200)
        .into();
        let renderer = Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&element);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 200.0)),
        );
        struct BodyBounds(Option<Rectangle>);
        impl widget::Operation for BodyBounds {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn custom(
                &mut self,
                _: Option<&widget::Id>,
                bounds: Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if state.is::<State>() {
                    self.0 = Some(bounds);
                }
            }
        }
        let mut bounds = BodyBounds(None);
        element
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut bounds);
        assert_eq!(bounds.0.unwrap().width, 300.0, "actions reserve no gutter");
        let mut clipboard = ClipboardText::default();
        let mut messages = Vec::new();
        let mut dispatch = |event, point| {
            let mut shell = Shell::new(&mut messages);
            element.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, 200.0)),
            );
        };
        let start = iced::Point::new(1.0, 191.0);
        let end = iced::Point::new(32.0, 191.0);
        dispatch(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            start,
        );
        dispatch(
            Event::Mouse(mouse::Event::CursorMoved { position: end }),
            end,
        );
        dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            end,
        );
        dispatch(
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character("c".into()),
                modified_key: iced::keyboard::Key::Character("c".into()),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers: iced::keyboard::Modifiers::COMMAND,
                text: Some("c".into()),
                repeat: false,
            }),
            end,
        );
        let reply = iced::Point::new(280.0, 189.0);
        dispatch(
            Event::Mouse(mouse::Event::CursorMoved { position: reply }),
            reply,
        );
        dispatch(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            reply,
        );
        dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            reply,
        );
        assert!(!clipboard.0.is_empty());
        assert!("hello world café 👋".starts_with(&clipboard.0));
        assert!(clipboard.0.len() < "hello world café 👋".len());
        assert_eq!(
            messages
                .iter()
                .filter(|message| matches!(message, Message::Thread(id) if id == "$one"))
                .count(),
            1,
            "the floating action opens exactly one thread"
        );
    }
}
