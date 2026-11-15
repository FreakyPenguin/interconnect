//! Anchored, viewport-clamped popup hosting arbitrary content. No timers.
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{self, Tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

pub fn view<'a, Message: Clone + 'a>(
    anchor: impl Into<Element<'a, Message>>,
    content: Option<Element<'a, Message>>,
    dismiss: Message,
) -> Element<'a, Message> {
    Element::new(Popover {
        anchor: anchor.into(),
        content,
        dismiss,
    })
}
struct Popover<'a, Message> {
    anchor: Element<'a, Message>,
    content: Option<Element<'a, Message>>,
    dismiss: Message,
}
impl<Message: Clone> Widget<Message, Theme, Renderer> for Popover<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        struct Identity;
        widget::tree::Tag::of::<Identity>()
    }
    fn children(&self) -> Vec<Tree> {
        vec![
            Tree::new(&self.anchor),
            self.content.as_ref().map_or_else(Tree::empty, Tree::new),
        ]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(self.anchor.as_widget());
        if let Some(content) = &self.content {
            tree.children[1].diff(content.as_widget());
        } else {
            tree.children[1] = Tree::empty();
        }
    }
    fn size(&self) -> Size<Length> {
        self.anchor.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.anchor
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
        self.anchor
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
        self.anchor.as_widget_mut().update(
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
        self.anchor.as_widget().draw(
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
        self.anchor.as_widget().mouse_interaction(
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
        if let Some(content) = &mut self.content {
            Some(overlay::Element::new(Box::new(Popup {
                content,
                tree: &mut tree.children[1],
                anchor: layout.bounds() + translation,
                dismiss: self.dismiss.clone(),
            })))
        } else {
            self.anchor.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}
struct Popup<'a, 'b, Message> {
    content: &'a mut Element<'b, Message>,
    tree: &'a mut Tree,
    anchor: Rectangle,
    dismiss: Message,
}
impl<Message: Clone> overlay::Overlay<Message, Theme, Renderer> for Popup<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        // Use window bounds so a popup can escape a narrow conversation pane.
        let available = Size::new(
            (bounds.width - 16.0).max(1.0),
            (bounds.height - 16.0).max(1.0),
        );
        let node = self.content.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, available),
        );
        let size = node.size();
        let x = (self.anchor.x + self.anchor.width - size.width)
            .clamp(8.0, (bounds.width - size.width - 8.0).max(8.0));
        let below = self.anchor.y + self.anchor.height + 4.0;
        let y = if below + size.height <= bounds.height - 8.0 {
            below
        } else {
            self.anchor.y - size.height - 4.0
        };
        let y = y.clamp(8.0, (bounds.height - size.height - 8.0).max(8.0));
        layout::Node::with_children(bounds, vec![node.move_to(iced::Point::new(x, y))])
    }
    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            self.tree,
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let panel = layout.children().next().unwrap();
        let outside_press = match event {
            Event::Mouse(mouse::Event::ButtonPressed(_)) => !cursor.is_over(panel.bounds()),
            Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => {
                !panel.bounds().contains(*position)
            }
            _ => false,
        };
        if outside_press
            || matches!(
                event,
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
            )
        {
            shell.publish(self.dismiss.clone());
            shell.capture_event();
            return;
        }
        self.content.as_widget_mut().update(
            self.tree,
            event,
            panel,
            cursor,
            renderer,
            clipboard,
            shell,
            &panel.bounds(),
        );
        if matches!(
            event,
            Event::Mouse(_) | Event::Touch(_) | Event::Keyboard(_) | Event::InputMethod(_)
        ) {
            shell.capture_event();
        }
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let panel = layout.children().next().unwrap();
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            panel,
            cursor,
            &Rectangle::with_size(layout.bounds().size()),
        );
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let panel = layout.children().next().unwrap();
        self.content.as_widget().mouse_interaction(
            self.tree,
            panel,
            cursor,
            &Rectangle::with_size(layout.bounds().size()),
            renderer,
        )
    }
    fn overlay<'a>(
        &'a mut self,
        layout: Layout<'a>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        let panel = layout.children().next().unwrap();
        self.content.as_widget_mut().overlay(
            self.tree,
            panel,
            renderer,
            &Rectangle::with_size(layout.bounds().size()),
            Vector::ZERO,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::{button, container, space};

    #[test]
    fn popup_escapes_pane_clamps_to_window_and_captures_outside_clicks_without_relayout() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for size in [Size::new(640.0, 480.0), Size::new(280.0, 200.0)] {
            for open in [false, true] {
                let content =
                    open.then(|| Element::from(container(space()).width(320).height(400)));
                let mut popup = view(button("add").on_press(false), content, true);
                let mut tree = Tree::empty();
                tree.diff(popup.as_widget());
                let node = popup.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                assert!(
                    node.size().width < 80.0 && node.size().height < 50.0,
                    "popup must not change the anchor layout"
                );
                let viewport = Rectangle::new(
                    iced::Point::new(size.width - 100.0, 0.0),
                    Size::new(100.0, size.height),
                );
                let overlay = popup.as_widget_mut().overlay(
                    &mut tree,
                    Layout::with_offset(Vector::new(size.width - 50.0, size.height - 30.0), &node),
                    &renderer,
                    &viewport,
                    Vector::ZERO,
                );
                let Some(mut overlay) = overlay else {
                    assert!(!open);
                    continue;
                };
                let overlay = overlay.as_overlay_mut();
                let node = overlay.layout(&renderer, size);
                let bounds = node.children()[0].bounds();
                assert!(bounds.x >= 8.0 && bounds.y >= 8.0);
                assert!(bounds.x + bounds.width <= size.width - 7.9);
                assert!(bounds.y + bounds.height <= size.height - 7.9);
                assert!(
                    bounds.x < viewport.x,
                    "popup can extend outside the conversation pane"
                );
                for (event, closes) in [
                    (
                        Event::Mouse(mouse::Event::WheelScrolled {
                            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
                        }),
                        false,
                    ),
                    (
                        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                        true,
                    ),
                ] {
                    let mut messages = Vec::new();
                    let mut shell = Shell::new(&mut messages);
                    overlay.update(
                        &event,
                        Layout::new(&node),
                        mouse::Cursor::Available(iced::Point::new(1.0, 1.0)),
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut shell,
                    );
                    assert!(shell.is_event_captured());
                    assert!(!matches!(
                        shell.redraw_request(),
                        iced::window::RedrawRequest::At(_)
                    ));
                    assert_eq!(messages, if closes { vec![true] } else { vec![] });
                }
            }
        }
    }
}
