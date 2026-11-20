//! Event-driven keyboard focus confined to an active dialog or page.
//! Traversal never reaches covered conversation controls and schedules no timers.
use iced::advanced::widget::operation::{Focusable, Scrollable};
use iced::keyboard::{Key, key::Named};
use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree},
    },
};

pub(super) fn view<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    id: &'static str,
) -> Element<'a, Message> {
    Element::new(Scope {
        content: content.into(),
        id,
        on_escape: None,
    })
}

/// Escape dismisses a form in one press, while an open child dropdown or IME
/// retains precedence. Text fields otherwise consume Escape just to unfocus.
pub(super) fn dismissible<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    id: &'static str,
    dismiss: Message,
) -> Element<'a, Message> {
    Element::new(Scope {
        content: content.into(),
        id,
        on_escape: Some(Box::new(move || dismiss.clone())),
    })
}

pub(super) fn cycle<T: Send + 'static>(
    id: &'static str,
    backwards: bool,
) -> impl widget::Operation<T> {
    use widget::operation::{self, focusable};
    let next: fn(focusable::Count) -> FocusAt = if backwards {
        |count| FocusAt {
            target: count
                .focused
                .map_or(count.total.saturating_sub(1), |index| {
                    (index + count.total.saturating_sub(1)) % count.total.max(1)
                }),
            index: 0,
        }
    } else {
        |count| FocusAt {
            target: count
                .focused
                .map_or(0, |index| (index + 1) % count.total.max(1)),
            index: 0,
        }
    };
    operation::scope(id.into(), operation::then(focusable::count(), next))
}
struct Scope<'a, Message> {
    content: Element<'a, Message>,
    id: &'static str,
    on_escape: Option<Box<dyn Fn() -> Message + 'a>>,
}
#[derive(Default)]
struct State {
    id: Option<&'static str>,
    initialized: bool,
    composing: bool,
}
impl<Message> Widget<Message, Theme, Renderer> for Scope<'_, Message> {
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
        let state = tree.state.downcast_mut::<State>();
        if state.id != Some(self.id) {
            state.id = Some(self.id);
            state.initialized = false;
            state.composing = false;
        }
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
        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        if !tree.state.downcast_ref::<State>().initialized {
            let mut count = Count::default();
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                Layout::new(&node),
                renderer,
                &mut count,
            );
            if count.total > 0 {
                if count.focused.is_none() {
                    self.content.as_widget_mut().operate(
                        &mut tree.children[0],
                        Layout::new(&node),
                        renderer,
                        &mut FocusAt {
                            target: 0,
                            index: 0,
                        },
                    );
                }
                tree.state.downcast_mut::<State>().initialized = true;
            }
        }
        node
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.container(Some(&widget::Id::new(self.id)), layout.bounds());
        operation.traverse(&mut |operation| {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        });
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
        match event {
            Event::InputMethod(iced::advanced::input_method::Event::Preedit(text, _)) => {
                tree.state.downcast_mut::<State>().composing = !text.is_empty()
            }
            Event::InputMethod(
                iced::advanced::input_method::Event::Commit(_)
                | iced::advanced::input_method::Event::Closed,
            ) => tree.state.downcast_mut::<State>().composing = false,
            _ => {}
        }
        let tab = matches!(event, Event::Keyboard(iced::keyboard::Event::KeyPressed { key: Key::Named(Named::Tab), modifiers, .. }) if !modifiers.command() && !modifiers.alt() && !modifiers.control());
        if tab && !tree.state.downcast_ref::<State>().composing {
            let backwards = matches!(event, Event::Keyboard(iced::keyboard::Event::KeyPressed { modifiers, .. }) if modifiers.shift());
            let mut count = Count::default();
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                &mut count,
            );
            if count.total > 0 {
                let target = match (count.focused, backwards) {
                    (Some(index), true) => (index + count.total - 1) % count.total,
                    (Some(index), false) => (index + 1) % count.total,
                    (None, true) => count.total - 1,
                    (None, false) => 0,
                };
                self.content.as_widget_mut().operate(
                    &mut tree.children[0],
                    layout,
                    renderer,
                    &mut FocusAt { target, index: 0 },
                );
                let mut focused = FocusedBounds::default();
                self.content.as_widget_mut().operate(
                    &mut tree.children[0],
                    layout,
                    renderer,
                    &mut focused,
                );
                if let Some(bounds) = focused.0 {
                    self.content.as_widget_mut().operate(
                        &mut tree.children[0],
                        layout,
                        renderer,
                        &mut Reveal { focused: bounds },
                    );
                }
                shell.request_redraw();
            }
            shell.capture_event();
            return;
        }
        let escape = self.on_escape.is_some()
            && !tree.state.downcast_ref::<State>().composing
            && matches!(
                event,
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: Key::Named(Named::Escape),
                    repeat: false,
                    ..
                })
            );
        let mut dropdown = OpenDropdown::default();
        if escape {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                &mut dropdown,
            );
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
        if escape && !dropdown.0 {
            shell.publish((self.on_escape.as_ref().expect("escape action"))());
            shell.capture_event();
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

#[derive(Default)]
struct Count {
    total: usize,
    focused: Option<usize>,
}
impl widget::Operation for Count {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn focusable(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.focused = Some(self.total);
        }
        self.total += 1;
    }
}
struct FocusAt {
    target: usize,
    index: usize,
}
impl<T> widget::Operation<T> for FocusAt {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation<T>)) {
        f(self);
    }
    fn focusable(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn Focusable) {
        if self.index == self.target {
            state.focus();
        } else {
            state.unfocus();
        }
        self.index += 1;
    }
}
#[derive(Default)]
struct FocusedBounds(Option<Rectangle>);
impl widget::Operation for FocusedBounds {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn focusable(&mut self, _: Option<&widget::Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.0 = Some(bounds);
        }
    }
}
#[derive(Default)]
struct OpenDropdown(bool);
impl widget::Operation for OpenDropdown {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn custom(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn std::any::Any) {
        if let Some(menu) = state.downcast_ref::<iced::widget::pick_list::KeyboardState>() {
            self.0 |= menu.is_open();
        }
    }
}
struct Reveal {
    focused: Rectangle,
}
impl widget::Operation for Reveal {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn scrollable(
        &mut self,
        _: Option<&widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        if !content.contains(self.focused.center()) {
            return;
        }
        let visible = self.focused - translation;
        let delta = if visible.y < bounds.y {
            visible.y - bounds.y - 4.0
        } else if visible.y + visible.height > bounds.y + bounds.height {
            visible.y + visible.height - bounds.y - bounds.height + 4.0
        } else {
            0.0
        };
        if delta != 0.0 {
            state.scroll_by(
                widget::operation::scrollable::AbsoluteOffset { x: 0.0, y: delta },
                bounds,
                content,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::{button, checkbox, column, pick_list, scrollable, text_input};

    fn key(named: Named, shift: bool, repeat: bool) -> Event {
        Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: Key::Named(named),
            modified_key: Key::Named(named),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: if shift {
                iced::keyboard::Modifiers::SHIFT
            } else {
                iced::keyboard::Modifiers::empty()
            },
            text: None,
            repeat,
        })
    }
    fn update(
        view: &mut Element<'_, u8>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        event: Event,
    ) -> (Vec<u8>, bool, iced::window::RedrawRequest) {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            tree,
            &event,
            Layout::new(node),
            mouse::Cursor::Unavailable,
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(node.size()),
        );
        let captured = shell.is_event_captured();
        let redraw = shell.redraw_request();
        (messages, captured, redraw)
    }
    #[test]
    fn scope_cycles_enabled_controls_activates_once_and_yields_to_ime() {
        let mut view = view(
            column![
                button("Disabled").keyboard_navigation(true),
                button("Apply").keyboard_navigation(true).on_press(1),
                checkbox(false)
                    .keyboard_navigation(true)
                    .label("Preview")
                    .on_toggle(|v| if v { 2 } else { 3 }),
                pick_list(["A", "B"], Some("A"), |_| 4).keyboard_navigation(true),
                text_input("Name", "")
                    .on_input(|_| 5)
                    .cursor_blinking(false),
            ]
            .spacing(8),
            "test-focus",
        );
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view);
        let size = Size::new(400.0, 400.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        assert_eq!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::Enter, false, false)
            )
            .0,
            [1]
        );
        assert!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::Enter, false, true)
            )
            .0
            .is_empty()
        );
        assert!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::Tab, false, false)
            )
            .1
        );
        assert_eq!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::Space, false, false)
            )
            .0,
            [2]
        );
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            key(Named::Tab, false, false),
        );
        assert_eq!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::ArrowDown, false, false)
            )
            .0,
            [4]
        );
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            key(Named::Enter, false, false),
        );
        assert!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::Escape, false, false)
            )
            .1
        );
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            key(Named::Tab, false, false),
        );
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::InputMethod(iced::advanced::input_method::Event::Preedit(
                "候".into(),
                None,
            )),
        );
        assert!(
            !update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::Tab, false, false)
            )
            .1
        );
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::InputMethod(iced::advanced::input_method::Event::Commit("候".into())),
        );
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            key(Named::Tab, false, false),
        );
        assert_eq!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::Enter, false, false)
            )
            .0,
            [1]
        );
        for _ in 0..3 {
            let (_, _, redraw) = update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
            );
            assert_eq!(redraw, iced::window::RedrawRequest::Wait);
        }
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            key(Named::Tab, true, false),
        );
        let mut count = Count::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut count);
        assert_eq!(count.focused, Some(3));
        assert_eq!(count.total, 4);
    }
    #[test]
    fn escape_dismisses_a_focused_input_once_but_closes_a_dropdown_first() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(300.0, 200.0);
        let mut input = dismissible(
            text_input("Name", "value")
                .on_input(|_| 1)
                .cursor_blinking(false),
            "input-escape",
            9,
        );
        let mut tree = Tree::new(&input);
        let node =
            input
                .as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let (messages, captured, _) = update(
            &mut input,
            &mut tree,
            &node,
            &renderer,
            key(Named::Escape, false, false),
        );
        assert_eq!(messages, [9]);
        assert!(captured);
        let mut picker = dismissible(
            pick_list(["A", "B"], Some("A"), |_| 2).keyboard_navigation(true),
            "picker-escape",
            9,
        );
        let mut tree = Tree::new(&picker);
        let node =
            picker
                .as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        update(
            &mut picker,
            &mut tree,
            &node,
            &renderer,
            key(Named::Enter, false, false),
        );
        let (messages, captured, _) = update(
            &mut picker,
            &mut tree,
            &node,
            &renderer,
            key(Named::Escape, false, false),
        );
        assert!(messages.is_empty());
        assert!(captured);
        assert_eq!(
            update(
                &mut picker,
                &mut tree,
                &node,
                &renderer,
                key(Named::Escape, false, false)
            )
            .0,
            [9]
        );
    }

    #[test]
    fn a_visible_hover_tooltip_does_not_consume_dialog_escape() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(300.0, 200.0);
        let mut form = dismissible(
            iced::widget::tooltip(
                text_input("Name", "value")
                    .on_input(|_| 1)
                    .cursor_blinking(false),
                "Field hint",
                iced::widget::tooltip::Position::Bottom,
            )
            .delay(std::time::Duration::ZERO),
            "tooltip-escape",
            9,
        );
        let mut tree = Tree::new(&form);
        let node =
            form.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let viewport = Rectangle::with_size(size);
        let cursor = mouse::Cursor::Available(iced::Point::new(10.0, 10.0));
        let mut messages = Vec::new();
        for event in [
            Event::Mouse(mouse::Event::CursorMoved {
                position: iced::Point::new(10.0, 10.0),
            }),
            Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
        ] {
            form.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
        }
        assert!(
            form.as_widget_mut()
                .overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &viewport,
                    Vector::ZERO
                )
                .is_some()
        );
        assert_eq!(
            update(
                &mut form,
                &mut tree,
                &node,
                &renderer,
                key(Named::Escape, false, false)
            )
            .0,
            [9]
        );
    }

    #[test]
    fn traversal_reveals_controls_below_the_scroll_viewport() {
        let mut view = view(
            scrollable(column![
                button("First").keyboard_navigation(true).on_press(1),
                iced::widget::space().height(600),
                button("Last").keyboard_navigation(true).on_press(2),
            ])
            .height(100),
            "scroll-focus",
        );
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view);
        let size = Size::new(300.0, 100.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        update(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            key(Named::Tab, false, false),
        );
        #[derive(Default)]
        struct Position(f32);
        impl widget::Operation for Position {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                _: Option<&widget::Id>,
                _: Rectangle,
                _: Rectangle,
                translation: Vector,
                _: &mut dyn Scrollable,
            ) {
                self.0 = translation.y;
            }
        }
        let mut position = Position::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut position);
        assert!(position.0 > 500.0);
        assert_eq!(
            update(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                key(Named::Enter, false, false)
            )
            .0,
            [2]
        );
    }
}
