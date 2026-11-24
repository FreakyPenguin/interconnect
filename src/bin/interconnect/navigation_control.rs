//! Focus and keyboard activation for navigation buttons, without blink timers.
use super::{Message, shell};
use iced::{
    Border, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree, operation::Focusable},
    },
    keyboard::{Key, Modifiers, key::Named},
};

pub fn view<'a>(
    base: impl Into<Element<'a, Message>>,
    id: impl Into<widget::Id>,
    activate: Message,
    keys: impl Fn(&Key, Modifiers) -> Option<Message> + 'a,
) -> Element<'a, Message> {
    Element::new(Control {
        base: base.into(),
        id: id.into(),
        activate,
        keys: Box::new(keys),
    })
}

pub fn plain<'a>(
    base: impl Into<Element<'a, Message>>,
    id: impl Into<widget::Id>,
    activate: Message,
) -> Element<'a, Message> {
    view(base, id, activate, |key, modifiers| {
        (matches!(key, Key::Named(Named::Tab)) && (modifiers - Modifiers::SHIFT).is_empty())
            .then_some(Message::Focus(modifiers.shift()))
    })
}

type KeyHandler<'a> = Box<dyn Fn(&Key, Modifiers) -> Option<Message> + 'a>;
struct Control<'a> {
    base: Element<'a, Message>,
    id: widget::Id,
    activate: Message,
    keys: KeyHandler<'a>,
}
#[derive(Default)]
struct State {
    focused: bool,
    id: Option<widget::Id>,
}
impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.focused = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
    }
}
impl Widget<Message, Theme, Renderer> for Control<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            id: Some(self.id.clone()),
            ..State::default()
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.base)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.id.as_ref() != Some(&self.id) {
            state.id = Some(self.id.clone());
            state.focused = false;
        }
        tree.diff_children(std::slice::from_ref(&self.base));
    }
    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // A collapsed sidebar stays mounted, but its controls must not regain
        // stale keyboard focus when it becomes visible beside another editor.
        if limits.max().width <= 0.0 || limits.max().height <= 0.0 {
            tree.state.downcast_mut::<State>().focused = false;
        }
        self.base
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
        operation.focusable(
            Some(&self.id),
            layout.bounds(),
            tree.state.downcast_mut::<State>(),
        );
        self.base
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        if tree.state.downcast_ref::<State>().focused
            && let Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat,
                ..
            }) = event
        {
            if *repeat && super::space_navigation::is_toggle_shortcut(key, *modifiers) {
                shell.capture_event();
                return;
            }
            let action = (self.keys)(key, *modifiers).or_else(|| {
                (!repeat
                    && modifiers.is_empty()
                    && matches!(key, Key::Named(Named::Enter | Named::Space)))
                .then(|| self.activate.clone())
            });
            if let Some(action) = action {
                shell.publish(action);
                shell.capture_event();
                return;
            }
        }
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) {
            tree.state.downcast_mut::<State>().focused =
                cursor.is_over(layout.bounds()) && cursor.is_over(*viewport);
        }
        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
        self.base.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if tree.state.downcast_ref::<State>().focused {
            use iced::advanced::Renderer as _;
            renderer.with_layer(*viewport, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: layout.bounds(),
                        border: Border {
                            color: shell::palette(theme).accent,
                            width: 1.5,
                            radius: 6.0.into(),
                        },
                        ..renderer::Quad::default()
                    },
                    iced::Color::TRANSPARENT,
                )
            });
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.base.as_widget().mouse_interaction(
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
        self.base.as_widget_mut().overlay(
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

    #[test]
    fn buttons_participate_in_focus_operations_and_capture_activation_once() {
        let mut view = plain(
            iced::widget::button("Home")
                .on_press(Message::Space(interconnect::navigation::Scope::Home)),
            "home-test",
            Message::Space(interconnect::navigation::Scope::Home),
        );
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view);
        let size = Size::new(200.0, 100.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        view.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut widget::operation::focusable::focus::<()>("home-test".into()),
        );
        for (key, repeat, expected) in [
            (Named::Enter, false, true),
            (Named::Enter, true, false),
            (Named::Space, false, true),
        ] {
            let event = Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: Key::Named(key),
                modified_key: Key::Named(key),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers: Modifiers::default(),
                text: None,
                repeat,
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
            assert_eq!(shell.is_event_captured(), expected);
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            assert_eq!(messages.len(), usize::from(expected));
        }
        let _ = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::ZERO),
        );
        assert!(!tree.state.downcast_ref::<State>().focused);
    }
}
