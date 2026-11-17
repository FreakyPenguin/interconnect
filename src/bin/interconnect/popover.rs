//! Anchored, viewport-clamped popup hosting arbitrary content. No timers.
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{self, Tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

pub fn view<'a, Message: Clone + 'a>(
    base: impl Into<Element<'a, Message>>,
    anchor_id: Option<widget::Id>,
    content: Option<Element<'a, Message>>,
    dismiss: Message,
) -> Element<'a, Message> {
    make_view(base, anchor_id, content, dismiss, true, AnchorEdge::End)
}

/// Align a modal popup to the start of its anchor, keeping viewport clamping.
pub fn start_aligned<'a, Message: Clone + 'a>(
    base: impl Into<Element<'a, Message>>,
    anchor_id: Option<widget::Id>,
    content: Option<Element<'a, Message>>,
    dismiss: Message,
) -> Element<'a, Message> {
    make_view(base, anchor_id, content, dismiss, true, AnchorEdge::Start)
}

#[derive(Clone, Copy)]
enum AnchorEdge {
    Start,
    End,
}

/// A context trigger captures only right presses inside the visible widget.
pub fn context_trigger<'a, Message: Clone + 'a>(
    base: impl Into<Element<'a, Message>>,
    open: impl Fn(iced::Point) -> Message + 'a,
    dismiss: Message,
) -> Element<'a, Message> {
    Element::new(Popover {
        base: base.into(),
        anchor_id: None,
        content: None,
        dismiss,
        modal: true,
        edge: AnchorEdge::End,
        position: None,
        on_context: Some(Box::new(open)),
    })
}

/// Host a modal menu at a captured window position.
pub fn at_position<'a, Message: Clone + 'a>(
    base: impl Into<Element<'a, Message>>,
    position: Option<iced::Point>,
    content: Option<Element<'a, Message>>,
    dismiss: Message,
) -> Element<'a, Message> {
    Element::new(Popover {
        base: base.into(),
        anchor_id: None,
        content,
        dismiss,
        modal: true,
        edge: AnchorEdge::End,
        position,
        on_context: None,
    })
}

/// Suggestions leave keyboard/IME input and outside clicks with the editor.
pub fn suggestions<'a, Message: Clone + 'a>(
    base: impl Into<Element<'a, Message>>,
    content: Option<Element<'a, Message>>,
    dismiss: Message,
) -> Element<'a, Message> {
    make_view(base, None, content, dismiss, false, AnchorEdge::Start)
}
fn make_view<'a, Message: Clone + 'a>(
    base: impl Into<Element<'a, Message>>,
    anchor_id: Option<widget::Id>,
    content: Option<Element<'a, Message>>,
    dismiss: Message,
    modal: bool,
    edge: AnchorEdge,
) -> Element<'a, Message> {
    Element::new(Popover {
        base: base.into(),
        anchor_id,
        content,
        dismiss,
        modal,
        edge,
        position: None,
        on_context: None,
    })
}
struct Popover<'a, Message> {
    base: Element<'a, Message>,
    anchor_id: Option<widget::Id>,
    content: Option<Element<'a, Message>>,
    dismiss: Message,
    modal: bool,
    edge: AnchorEdge,
    position: Option<iced::Point>,
    on_context: Option<Box<dyn Fn(iced::Point) -> Message + 'a>>,
}
#[derive(Default)]
struct State {
    translation: Vector,
}
impl<Message: Clone> Widget<Message, Theme, Renderer> for Popover<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![
            Tree::new(&self.base),
            self.content.as_ref().map_or_else(Tree::empty, Tree::new),
        ]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(self.base.as_widget());
        if let Some(content) = &self.content {
            tree.children[1].diff(content.as_widget());
        } else {
            tree.children[1] = Tree::empty();
        }
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
        if let Some(open) = &self.on_context
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
            )
            && let Some(position) = cursor.position()
            && layout.bounds().contains(position)
            && viewport.contains(position)
        {
            // Scrollables give children a translated cursor. Overlay traversal
            // supplies the inverse translation into window coordinates.
            shell.publish(open(
                position + tree.state.downcast_ref::<State>().translation,
            ));
            shell.capture_event();
            return;
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
        tree.state.downcast_mut::<State>().translation = translation;
        if let Some(content) = &mut self.content {
            // Host outside hover/tooltip wrappers so their overlays cannot
            // mask the popup. Only measure its inner anchor while open.
            struct FindAnchor<'a> {
                id: &'a widget::Id,
                bounds: Option<Rectangle>,
            }
            impl widget::Operation for FindAnchor<'_> {
                fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                    operate(self);
                }
                fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                    if id == Some(self.id) {
                        self.bounds = Some(bounds);
                    }
                }
            }
            let bounds = if let Some(id) = &self.anchor_id {
                let mut find = FindAnchor { id, bounds: None };
                self.base.as_widget_mut().operate(
                    &mut tree.children[0],
                    layout,
                    renderer,
                    &mut find,
                );
                find.bounds.unwrap_or(layout.bounds())
            } else {
                layout.bounds()
            };
            Some(overlay::Element::new(Box::new(Popup {
                content,
                tree: &mut tree.children[1],
                anchor: bounds + translation,
                dismiss: self.dismiss.clone(),
                modal: self.modal,
                edge: self.edge,
                position: self.position,
            })))
        } else {
            self.base.as_widget_mut().overlay(
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
    modal: bool,
    edge: AnchorEdge,
    position: Option<iced::Point>,
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
        let x = (if let Some(position) = self.position {
            position.x
        } else if matches!(self.edge, AnchorEdge::End) {
            self.anchor.x + self.anchor.width - size.width
        } else {
            self.anchor.x
        })
        .clamp(8.0, (bounds.width - size.width - 8.0).max(8.0));
        let anchor_y = self.position.map_or(self.anchor.y, |point| point.y);
        let below = self
            .position
            .map_or(self.anchor.y + self.anchor.height + 4.0, |point| point.y);
        let y = if below + size.height <= bounds.height - 8.0 {
            below
        } else {
            anchor_y - size.height - 4.0
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
        if !self.modal && matches!(event, Event::Keyboard(_) | Event::InputMethod(_)) {
            return;
        }
        let panel = layout.children().next().unwrap();
        let outside_press = match event {
            Event::Mouse(mouse::Event::ButtonPressed(_)) => !cursor.is_over(panel.bounds()),
            Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => {
                !panel.bounds().contains(*position)
            }
            _ => false,
        };
        if outside_press
            || (self.modal
                && matches!(
                    event,
                    Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    })
                ))
        {
            shell.publish(self.dismiss.clone());
            if self.modal {
                shell.capture_event();
            }
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
        if (self.modal || cursor.is_over(panel.bounds()))
            && matches!(
                event,
                Event::Mouse(_) | Event::Touch(_) | Event::Keyboard(_) | Event::InputMethod(_)
            )
        {
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
    fn context_trigger_uses_window_cursor_and_ignores_clipped_content_and_left_clicks() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut view = context_trigger(container(space()).width(200).height(200), Some, None);
        let mut tree = Tree::new(&view);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 400.0)),
        );
        let viewport = Rectangle::new(iced::Point::new(100.0, 100.0), Size::new(100.0, 100.0));
        let translation = Vector::new(0.0, -80.0);
        let _ = view.as_widget_mut().overlay(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &viewport,
            translation,
        );
        for (button, position, opens) in [
            (mouse::Button::Right, iced::Point::new(150.0, 150.0), true),
            (mouse::Button::Right, iced::Point::new(50.0, 50.0), false),
            (mouse::Button::Left, iced::Point::new(150.0, 150.0), false),
        ] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &Event::Mouse(mouse::Event::ButtonPressed(button)),
                Layout::new(&node),
                mouse::Cursor::Available(position),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &viewport,
            );
            assert_eq!(shell.is_event_captured(), opens);
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
            assert_eq!(
                messages,
                if opens {
                    vec![Some(position + translation)]
                } else {
                    vec![]
                }
            );
        }
    }

    #[test]
    fn context_popup_clamps_cursor_position_and_escape_captures_once() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(640.0, 480.0);
        for position in [
            iced::Point::new(100.0, 100.0),
            iced::Point::new(638.0, 478.0),
        ] {
            let mut view = at_position(
                space(),
                Some(position),
                Some(container(space()).width(200).height(150).into()),
                true,
            );
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            let mut overlay = view
                .as_widget_mut()
                .overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &Rectangle::with_size(size),
                    Vector::ZERO,
                )
                .unwrap();
            let overlay = overlay.as_overlay_mut();
            let node = overlay.layout(&renderer, size);
            let bounds = node.children()[0].bounds();
            if position.x == 100.0 {
                assert_eq!(bounds.position(), position);
            }
            assert!(bounds.x >= 8.0 && bounds.y >= 8.0);
            assert!(bounds.x + bounds.width <= 632.0 && bounds.y + bounds.height <= 472.0);
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            overlay.update(
                &Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    physical_key: iced::keyboard::key::Physical::Code(
                        iced::keyboard::key::Code::Escape,
                    ),
                    location: iced::keyboard::Location::Standard,
                    modifiers: iced::keyboard::Modifiers::default(),
                    text: None,
                    repeat: false,
                }),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
            );
            assert!(shell.is_event_captured());
            assert_eq!(messages, vec![true]);
        }
    }

    #[test]
    fn suggestions_pass_editor_input_and_outside_clicks_through() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut popup = suggestions(
            button("editor").on_press(false),
            Some(container(space()).width(100).height(100).into()),
            true,
        );
        let mut tree = Tree::new(&popup);
        let size = Size::new(300.0, 400.0);
        let node = popup.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let mut element = popup
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &Rectangle::with_size(size),
                Vector::ZERO,
            )
            .unwrap();
        let overlay = element.as_overlay_mut();
        let node = overlay.layout(&renderer, size);
        for (event, dismiss) in [
            (
                Event::InputMethod(iced::advanced::input_method::Event::Commit("s".into())),
                false,
            ),
            (
                Event::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
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
            assert!(!shell.is_event_captured());
            assert_eq!(messages, if dismiss { vec![true] } else { vec![] });
        }
    }

    #[test]
    fn popup_uses_inner_anchor_and_scroll_translation_instead_of_the_message_bounds() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let base = iced::widget::column![
            space().height(100),
            container(button("+").on_press(false))
                .width(28)
                .height(20)
                .id("inner-anchor"),
            space().height(80),
        ]
        .width(300);
        let content = container(space()).width(80).height(40).into();
        let mut popup = view(
            base,
            Some(widget::Id::new("inner-anchor")),
            Some(content),
            true,
        );
        let mut tree = Tree::new(&popup);
        let size = Size::new(640.0, 480.0);
        let node = popup.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let mut overlay = popup
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &Rectangle::with_size(size),
                Vector::new(100.0, 30.0),
            )
            .unwrap();
        let node = overlay.as_overlay_mut().layout(&renderer, size);
        assert_eq!(
            node.children()[0].bounds(),
            Rectangle::new(iced::Point::new(48.0, 154.0), Size::new(80.0, 40.0))
        );
    }

    #[test]
    fn composer_start_alignment_preserves_default_end_alignment_and_window_clamping() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(1280.0, 900.0);
        for (start, anchor_x, expected_x) in [
            (true, 350.0, 350.0),
            (false, 350.0, 58.0),
            (true, 1170.0, 952.0),
        ] {
            let content = Some(container(space()).width(320).height(400).into());
            let anchor = container(space()).width(28).height(28);
            let mut popup = if start {
                start_aligned(anchor, None, content, true)
            } else {
                view(anchor, None, content, true)
            };
            let mut tree = Tree::new(&popup);
            let node = popup.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            let mut overlay = popup
                .as_widget_mut()
                .overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &Rectangle::with_size(size),
                    Vector::new(anchor_x, 850.0),
                )
                .unwrap();
            let overlay = overlay.as_overlay_mut();
            let node = overlay.layout(&renderer, size);
            let bounds = node.children()[0].bounds();
            assert_eq!(bounds.x, expected_x);
            assert_eq!(bounds.y, 446.0, "composer popup opens above its toolbar");
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            overlay.update(
                &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
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
            assert_eq!(messages, vec![true]);
        }
    }

    #[test]
    fn popup_escapes_pane_clamps_to_window_and_captures_outside_clicks_without_relayout() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for size in [Size::new(640.0, 480.0), Size::new(280.0, 200.0)] {
            for open in [false, true] {
                let content =
                    open.then(|| Element::from(container(space()).width(320).height(400)));
                let mut popup = view(button("add").on_press(false), None, content, true);
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
