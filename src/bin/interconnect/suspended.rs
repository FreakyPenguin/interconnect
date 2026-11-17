//! Retain a covered dialog's widget state without painting or handling input.
use super::Message;
use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree},
    },
};

pub(super) fn view(content: Element<'_, Message>, suspended: bool) -> Element<'_, Message> {
    Element::new(Suspended { content, suspended })
}
struct Suspended<'a> {
    content: Element<'a, Message>,
    suspended: bool,
}
impl Widget<Message, Theme, Renderer> for Suspended<'_> {
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
        if !self.suspended {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        }
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
        if !self.suspended {
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
        if !self.suspended {
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
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.suspended {
            mouse::Interaction::None
        } else {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if self.suspended {
            None
        } else {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell;
    use std::{cell::Cell, rc::Rc};
    struct Probe(Rc<Cell<usize>>);
    impl Widget<Message, Theme, Renderer> for Probe {
        fn tag(&self) -> widget::tree::Tag {
            widget::tree::Tag::of::<u32>()
        }
        fn state(&self) -> widget::tree::State {
            widget::tree::State::new(7_u32)
        }
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fixed(100.0), Length::Fixed(100.0))
        }
        fn layout(&mut self, _: &mut Tree, _: &Renderer, _: &layout::Limits) -> layout::Node {
            layout::Node::new(Size::new(100.0, 100.0))
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            _: &Event,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &Renderer,
            _: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            _: &Rectangle,
        ) {
            *tree.state.downcast_mut::<u32>() += 1;
            shell.publish(Message::Exit);
        }
        fn draw(
            &self,
            _: &Tree,
            _: &mut Renderer,
            _: &Theme,
            _: &renderer::Style,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &Rectangle,
        ) {
            self.0.set(self.0.get() + 1);
        }
        fn overlay<'b>(
            &'b mut self,
            _: &'b mut Tree,
            _: Layout<'b>,
            _: &Renderer,
            _: &Rectangle,
            _: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
            self.0.set(self.0.get() + 1);
            None
        }
    }
    #[test]
    fn suspension_retains_widget_state_and_blocks_drawing_input_and_overlays() {
        let calls = Rc::new(Cell::new(0));
        let mut renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::empty();
        for suspended in [false, true, false] {
            let mut view = view(Element::new(Probe(calls.clone())), suspended);
            tree.diff(view.as_widget());
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0)),
            );
            let before = *tree.children[0].state.downcast_ref::<u32>();
            let mut messages = Vec::new();
            let viewport = Rectangle::with_size(Size::new(100.0, 100.0));
            for event in [
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Keyboard(iced::keyboard::Event::ModifiersChanged(
                    iced::keyboard::Modifiers::CTRL,
                )),
            ] {
                view.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    mouse::Cursor::Available(iced::Point::new(10.0, 10.0)),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &viewport,
                );
            }
            let count = calls.get();
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &shell::theme(),
                &renderer::Style {
                    text_color: shell::INK,
                },
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &viewport,
            );
            assert!(
                view.as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &viewport,
                        Vector::ZERO
                    )
                    .is_none()
            );
            assert_eq!(calls.get() - count, if suspended { 0 } else { 2 });
            assert_eq!(messages.len(), if suspended { 0 } else { 2 });
            assert_eq!(
                *tree.children[0].state.downcast_ref::<u32>(),
                before + if suspended { 0 } else { 2 }
            );
            assert!(before >= 7, "diff must preserve child state");
        }
    }
}
