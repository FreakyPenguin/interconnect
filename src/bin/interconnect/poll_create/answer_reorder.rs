//! Local answer dragging: commit on release, keep keyed editors, and schedule no timers.
use super::{Event, message};
use crate::{Message, shell};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{self, Tree},
};
use iced::{Element, Event as Input, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

pub(super) fn view<'a>(
    content: Element<'a, Message>,
    identity: (u64, u64),
    keys: &[usize],
    enabled: bool,
) -> Element<'a, Message> {
    Element::new(Answers {
        content,
        identity,
        keys: keys.to_vec(),
        enabled,
    })
}
struct Answers<'a> {
    content: Element<'a, Message>,
    identity: (u64, u64),
    keys: Vec<usize>,
    enabled: bool,
}
#[derive(Default)]
struct State {
    identity: Option<(u64, u64)>,
    keys: Vec<usize>,
    drag: Option<Drag>,
}
#[derive(Clone, Copy)]
struct Drag {
    from: usize,
    origin: Point,
    active: bool,
    slot: Option<usize>,
}
fn destination(from: usize, slot: usize) -> Option<usize> {
    let to = slot.saturating_sub(usize::from(slot > from));
    (to != from).then_some(to)
}
fn slot_at(layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) -> Option<usize> {
    let point = cursor.position()?;
    if !layout.bounds().contains(point) || !viewport.contains(point) {
        return None;
    }
    Some(
        layout
            .children()
            .position(|row| point.y < row.bounds().center_y())
            .unwrap_or_else(|| layout.children().count()),
    )
}
#[derive(Default)]
struct Focused(Option<Rectangle>);
impl widget::Operation for Focused {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn focusable(
        &mut self,
        _: Option<&widget::Id>,
        bounds: Rectangle,
        state: &mut dyn widget::operation::Focusable,
    ) {
        if state.is_focused() {
            self.0 = Some(bounds);
        }
    }
}
impl Widget<Message, Theme, Renderer> for Answers<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            identity: Some(self.identity),
            keys: self.keys.clone(),
            drag: None,
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.identity != Some(self.identity) || !self.enabled {
            state.drag = None;
        }
        if state.identity.map(|identity| identity.0) != Some(self.identity.0) {
            tree.children[0] = Tree::new(&self.content);
        } else if state.keys != self.keys {
            // Reconcile the bounded list by identity, including arbitrary permutations.
            // Each entire row retains its input cursor, selection, and focus.
            let mut previous: Vec<_> = std::mem::take(&mut tree.children[0].children)
                .into_iter()
                .map(Some)
                .collect();
            tree.children[0].children = self
                .keys
                .iter()
                .map(|key| {
                    state
                        .keys
                        .iter()
                        .position(|previous| previous == key)
                        .and_then(|index| previous[index].take())
                        .unwrap_or_else(Tree::empty)
                })
                .collect();
        }
        state.identity = Some(self.identity);
        state.keys.clone_from(&self.keys);
        tree.children[0].diff(self.content.as_widget());
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
        if tree.state.downcast_ref::<State>().drag.is_some() {
            operation.custom(
                None,
                layout.bounds(),
                &mut crate::focus_scope::EscapeCapture,
            );
        }
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Input,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let dragging = tree.state.downcast_ref::<State>().drag.is_some();
        let escape = matches!(
            event,
            Input::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            })
        );
        let cancel = !self.enabled
            || matches!(
                event,
                Input::Window(iced::window::Event::Unfocused)
                    | Input::Mouse(
                        mouse::Event::CursorLeft
                            | mouse::Event::WheelScrolled { .. }
                            | mouse::Event::ButtonPressed(mouse::Button::Right)
                    )
            );
        if dragging && (cancel || escape) {
            tree.state.downcast_mut::<State>().drag = None;
            shell.request_redraw();
            if escape {
                shell.capture_event();
                return;
            }
        }
        if self.enabled
            && matches!(
                event,
                Input::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
            && cursor.is_over(*viewport)
            && let Some(from) = layout.children().position(|row| {
                row.children()
                    .next()
                    .is_some_and(|handle| cursor.is_over(handle.bounds()))
            })
        {
            tree.state.downcast_mut::<State>().drag = Some(Drag {
                from,
                origin: cursor.position().expect("cursor over handle"),
                active: false,
                slot: None,
            });
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        if let Some(mut drag) = tree.state.downcast_ref::<State>().drag {
            match event {
                Input::Mouse(mouse::Event::CursorMoved { .. }) => {
                    let active = drag.active
                        || cursor.position().is_some_and(|point| {
                            (point.x - drag.origin.x).hypot(point.y - drag.origin.y) >= 4.0
                        });
                    let slot = active.then(|| slot_at(layout, cursor, viewport)).flatten();
                    if drag.active != active || drag.slot != slot {
                        shell.request_redraw();
                    }
                    drag.active = active;
                    drag.slot = slot;
                    tree.state.downcast_mut::<State>().drag = Some(drag);
                    shell.capture_event();
                    return;
                }
                Input::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    tree.state.downcast_mut::<State>().drag = None;
                    if drag.active
                        && let Some(to) = slot_at(layout, cursor, viewport)
                            .and_then(|slot| destination(drag.from, slot))
                    {
                        shell.publish(message(Event::Reorder {
                            serial: self.identity.0,
                            revision: self.identity.1,
                            from: drag.from,
                            to,
                        }));
                    }
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                Input::Keyboard(_) => {
                    shell.capture_event();
                    return;
                }
                _ => {}
            }
        }
        if self.enabled
            && let Input::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event
            && *modifiers == iced::keyboard::Modifiers::ALT
            && matches!(
                key,
                iced::keyboard::Key::Named(
                    iced::keyboard::key::Named::ArrowUp | iced::keyboard::key::Named::ArrowDown
                )
            )
        {
            let mut focus = Focused::default();
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                &mut focus,
            );
            if let Some(from) = focus.0.and_then(|bounds| {
                layout
                    .children()
                    .position(|row| row.bounds().contains(bounds.center()))
            }) {
                let up = *key == iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp);
                let to = if up {
                    from.checked_sub(1)
                } else {
                    (from + 1 < layout.children().count()).then_some(from + 1)
                };
                if let Some(to) = to {
                    shell.publish(message(Event::Reorder {
                        serial: self.identity.0,
                        revision: self.identity.1,
                        from,
                        to,
                    }));
                }
                shell.capture_event();
                return;
            }
        }
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
        let drag = tree
            .state
            .downcast_ref::<State>()
            .drag
            .filter(|drag| drag.active);
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            if drag.is_some() {
                mouse::Cursor::Unavailable
            } else {
                cursor
            },
            viewport,
        );
        if let Some(drag) = drag {
            if let Some(bounds) = layout.children().nth(drag.from).map(|row| row.bounds()) {
                renderer.with_layer(*viewport, |renderer| {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds,
                            border: iced::Border {
                                color: shell::palette(theme).accent,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        },
                        iced::Color::TRANSPARENT,
                    )
                });
            }
            if let Some(slot) = drag
                .slot
                .filter(|slot| destination(drag.from, *slot).is_some())
            {
                let bounds = layout.bounds();
                let y = if slot == 0 {
                    bounds.y
                } else if let Some(next) = layout.children().nth(slot) {
                    let previous = layout.children().nth(slot - 1).unwrap().bounds();
                    (previous.y + previous.height + next.bounds().y) / 2.0
                } else {
                    bounds.y + bounds.height - 2.0
                };
                if let Some(bounds) = (Rectangle {
                    x: bounds.x,
                    y,
                    width: bounds.width,
                    height: 2.0,
                })
                .intersection(viewport)
                {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds,
                            ..Default::default()
                        },
                        shell::palette(theme).accent,
                    );
                }
            }
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
        if let Some(drag) = tree.state.downcast_ref::<State>().drag {
            return if drag.active {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::Grab
            };
        }
        if self.enabled
            && cursor.is_over(*viewport)
            && layout.children().any(|row| {
                row.children()
                    .next()
                    .is_some_and(|handle| cursor.is_over(handle.bounds()))
            })
        {
            return mouse::Interaction::Grab;
        }
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
        if tree.state.downcast_ref::<State>().drag.is_some() {
            return None;
        }
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
    use iced::widget::{container, row, space, text_input};

    fn answers(ids: &[usize], revision: u64, enabled: bool) -> Element<'static, Message> {
        view(
            iced::widget::column(ids.iter().map(|&id| {
                row![
                    container(space()).width(28).height(32),
                    text_input("", ["A", "B", "C"][id])
                        .id(format!("answer-{id}"))
                        .cursor_blinking(false)
                        .padding(8)
                        .on_input(move |value| message(Event::Answer(7, id, value))),
                ]
                .spacing(8)
                .into()
            }))
            .width(Length::Fill)
            .spacing(10)
            .into(),
            (7, revision),
            ids,
            enabled,
        )
    }
    fn layout(
        view: &mut Element<'_, Message>,
        tree: &mut Tree,
        renderer: &Renderer,
    ) -> layout::Node {
        view.as_widget_mut().layout(
            tree,
            renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 200.0)),
        )
    }
    fn input(
        view: &mut Element<'_, Message>,
        tree: &mut Tree,
        renderer: &Renderer,
        node: &layout::Node,
        event: Input,
        point: Point,
    ) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            tree,
            &event,
            Layout::new(node),
            mouse::Cursor::Available(point),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::new(Point::ORIGIN, Size::new(400.0, 200.0)),
        );
        messages
    }
    fn press() -> Input {
        Input::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }
    fn release() -> Input {
        Input::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    }
    fn movement(point: Point) -> Input {
        Input::Mouse(mouse::Event::CursorMoved { position: point })
    }
    fn key(named: iced::keyboard::key::Named, modifiers: iced::keyboard::Modifiers) -> Input {
        Input::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(named),
            modified_key: iced::keyboard::Key::Named(named),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }
    #[test]
    fn poll_answer_drag_starts_only_on_handles_and_commits_only_a_valid_release() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut view = answers(&[0, 1, 2], 0, true);
        let mut tree = Tree::new(&view);
        let node = layout(&mut view, &mut tree, &renderer);
        let rows: Vec<_> = Layout::new(&node).children().collect();
        let handle = rows[0].children().next().unwrap().bounds().center();
        let field = rows[0].children().nth(1).unwrap().bounds().center();
        input(&mut view, &mut tree, &renderer, &node, press(), field);
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
        input(&mut view, &mut tree, &renderer, &node, release(), field);
        input(&mut view, &mut tree, &renderer, &node, press(), handle);
        assert!(input(&mut view, &mut tree, &renderer, &node, release(), handle).is_empty());
        input(&mut view, &mut tree, &renderer, &node, press(), handle);
        // Rebuilding the same form must preserve the in-progress drag.
        let mut rebuilt = answers(&[0, 1, 2], 0, true);
        tree.diff(rebuilt.as_widget());
        let node = layout(&mut rebuilt, &mut tree, &renderer);
        assert!(tree.state.downcast_ref::<State>().drag.is_some());
        let bottom = Point::new(100.0, node.size().height - 1.0);
        input(
            &mut rebuilt,
            &mut tree,
            &renderer,
            &node,
            movement(bottom),
            bottom,
        );
        assert!(matches!(
            input(&mut rebuilt, &mut tree, &renderer, &node, release(), bottom).as_slice(),
            [Message::PollCreate(Event::Reorder {
                serial: 7,
                revision: 0,
                from: 0,
                to: 2
            })]
        ));
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
        input(&mut rebuilt, &mut tree, &renderer, &node, press(), handle);
        let outside = Point::new(450.0, 150.0);
        input(
            &mut rebuilt,
            &mut tree,
            &renderer,
            &node,
            movement(outside),
            outside,
        );
        assert!(
            input(
                &mut rebuilt,
                &mut tree,
                &renderer,
                &node,
                release(),
                outside
            )
            .is_empty()
        );
    }
    #[test]
    fn poll_answer_drag_cancels_on_focus_loss_scrolling_and_replaced_or_disabled_forms() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let handle = Point::new(12.0, 16.0);
        for cancel in [
            Input::Window(iced::window::Event::Unfocused),
            Input::Mouse(mouse::Event::CursorLeft),
            Input::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            Input::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
            }),
        ] {
            let mut view = answers(&[0, 1, 2], 0, true);
            let mut tree = Tree::new(&view);
            let node = layout(&mut view, &mut tree, &renderer);
            input(&mut view, &mut tree, &renderer, &node, press(), handle);
            input(&mut view, &mut tree, &renderer, &node, cancel, handle);
            assert!(tree.state.downcast_ref::<State>().drag.is_none());
        }
        for (revision, enabled) in [(1, true), (0, false)] {
            let mut view = answers(&[0, 1, 2], 0, true);
            let mut tree = Tree::new(&view);
            let node = layout(&mut view, &mut tree, &renderer);
            input(&mut view, &mut tree, &renderer, &node, press(), handle);
            let rebuilt = answers(&[0, 1, 2], revision, enabled);
            tree.diff(rebuilt.as_widget());
            assert!(tree.state.downcast_ref::<State>().drag.is_none());
        }
    }
    #[test]
    fn poll_answer_escape_cancels_the_drag_before_dismissing_the_form() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut view = crate::focus_scope::dismissible(
            answers(&[0, 1, 2], 0, true),
            "answers-focus",
            message(Event::Close(7)),
        );
        let mut tree = Tree::new(&view);
        let node = layout(&mut view, &mut tree, &renderer);
        let handle = Point::new(12.0, 16.0);
        input(&mut view, &mut tree, &renderer, &node, press(), handle);
        let escape = key(
            iced::keyboard::key::Named::Escape,
            iced::keyboard::Modifiers::empty(),
        );
        assert!(
            input(
                &mut view,
                &mut tree,
                &renderer,
                &node,
                escape.clone(),
                handle
            )
            .is_empty()
        );
        assert!(matches!(
            input(&mut view, &mut tree, &renderer, &node, escape, handle).as_slice(),
            [Message::PollCreate(Event::Close(7))]
        ));
    }
    #[test]
    fn poll_answer_keyboard_moves_preserve_the_focused_editor_with_its_key() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut view = answers(&[0, 1, 2], 0, true);
        let mut tree = Tree::new(&view);
        let node = layout(&mut view, &mut tree, &renderer);
        view.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut widget::operation::focusable::focus::<()>("answer-0".into()),
        );
        let alt_down = key(
            iced::keyboard::key::Named::ArrowDown,
            iced::keyboard::Modifiers::ALT,
        );
        assert!(matches!(
            input(
                &mut view,
                &mut tree,
                &renderer,
                &node,
                alt_down,
                Point::ORIGIN
            )
            .as_slice(),
            [Message::PollCreate(Event::Reorder { from: 0, to: 1, .. })]
        ));
        let mut rebuilt = answers(&[1, 0, 2], 1, true);
        tree.diff(rebuilt.as_widget());
        let node = layout(&mut rebuilt, &mut tree, &renderer);
        let mut focus = Focused::default();
        rebuilt
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut focus);
        assert!(
            Layout::new(&node)
                .children()
                .nth(1)
                .unwrap()
                .bounds()
                .contains(focus.0.unwrap().center())
        );
        let alt_up = key(
            iced::keyboard::key::Named::ArrowUp,
            iced::keyboard::Modifiers::ALT,
        );
        assert!(matches!(
            input(
                &mut rebuilt,
                &mut tree,
                &renderer,
                &node,
                alt_up,
                Point::ORIGIN
            )
            .as_slice(),
            [Message::PollCreate(Event::Reorder { from: 1, to: 0, .. })]
        ));
    }
}
