//! Read-only message text, using Iced's Unicode-aware selection engine.
//! Selection lives in the widget tree, never in SDK snapshots or drafts.
use super::{Message, shell};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
    widget::{self, Tree},
};
use iced::widget::text_editor::{self, Action, Binding};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme};

pub fn view<'a>(id: &'a str, body: &'a str) -> Element<'a, Message> {
    Element::new(Selectable { id, body })
}
struct Selectable<'a> {
    id: &'a str,
    body: &'a str,
}
struct State {
    id: String,
    body: String,
    content: text_editor::Content,
}
fn editor(
    content: &text_editor::Content,
) -> text_editor::TextEditor<'_, iced::advanced::text::highlighter::PlainText, Action> {
    iced::widget::text_editor(content)
        .size(14)
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
        .style(|_, _| text_editor::Style {
            background: iced::Color::TRANSPARENT.into(),
            border: iced::Border::default(),
            value: shell::INK,
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
            body: self.body.into(),
            content: text_editor::Content::with_text(self.body),
        })
    }
    fn children(&self) -> Vec<Tree> {
        let content = text_editor::Content::new();
        vec![Tree::new(Element::from(editor(&content)))]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.id != self.id || state.body != self.body {
            tree.state = self.state();
            tree.children = self.children();
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
        editor(&state.content).layout(&mut tree.children[0], renderer, limits)
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
        editor(&state.content).update(
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
                state.content.perform(action);
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
        editor(&state.content).draw(
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
            body: "hello world café 👋",
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
}
