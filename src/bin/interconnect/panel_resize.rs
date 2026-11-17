//! Event-driven panel dividers. Sizes are remembered for this window session.
use super::{App, Message, shell};
use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
        widget::{self, Tree},
    },
};

pub(super) const HANDLE: f32 = 5.0;
pub(super) const NAV_MIN: f32 = 180.0;
pub(super) const NAV_MAX: f32 = 480.0;
pub(super) const SIDE_MIN: f32 = 300.0;
pub(super) const SIDE_MAX: f32 = 640.0;
pub(super) const ROOM_MIN: f32 = 440.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Navigation,
    Thread(u64),
    People(u64),
}

impl App {
    pub(super) fn panel_resize_enabled(&self) -> bool {
        self.context_menu_allowed()
            && self.context_menu.is_none()
            && self.emoji_management.is_none()
            && !self.section_dragging()
    }
    pub(super) fn resize_panel(&mut self, target: Target, width: f32) {
        if !self.panel_resize_enabled() || !width.is_finite() {
            return;
        }
        match target {
            Target::Navigation if self.shell.navigation => {
                self.shell.navigation_width = width.clamp(NAV_MIN, NAV_MAX);
            }
            Target::Thread(generation)
                if self.selection.members.is_none()
                    && self
                        .selection
                        .thread
                        .as_ref()
                        .is_some_and(|(current, _)| *current == generation)
                    && !self.global_search.open
                    && !self.bookmarks.open =>
            {
                self.shell.thread_width = width.clamp(SIDE_MIN, SIDE_MAX);
            }
            Target::People(generation)
                if self.selection.members == Some(generation)
                    && !self.global_search.open
                    && !self.bookmarks.open =>
            {
                self.shell.people_width = width.clamp(SIDE_MIN, SIDE_MAX);
            }
            _ => {}
        }
    }
}

pub(super) fn divider(
    target: Target,
    width: f32,
    max: f32,
    enabled: bool,
) -> Element<'static, Message> {
    Element::new(Divider {
        target,
        width,
        max,
        enabled,
    })
}
struct Divider {
    target: Target,
    width: f32,
    max: f32,
    enabled: bool,
}
#[derive(Default)]
struct State {
    target: Option<Target>,
    drag: Option<(f32, f32)>,
    last: Option<f32>,
    hovered: bool,
}
impl Widget<Message, Theme, Renderer> for Divider {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            target: Some(self.target),
            ..Default::default()
        })
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.target != Some(self.target) || !self.enabled {
            state.drag = None;
            state.last = None;
            state.target = Some(self.target);
        }
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(HANDLE), Length::Fill)
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::Node::new(limits.resolve(Length::Fixed(HANDLE), Length::Fill, Size::ZERO))
    }
    fn operate(
        &mut self,
        _: &mut Tree,
        layout: Layout<'_>,
        _: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let id = match self.target {
            Target::Navigation => "navigation-divider",
            Target::Thread(_) => "thread-divider",
            Target::People(_) => "people-divider",
        };
        operation.container(Some(&widget::Id::new(id)), layout.bounds());
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let hovered = self.enabled && cursor.is_over(layout.bounds()) && cursor.is_over(*viewport);
        if hovered != state.hovered {
            state.hovered = hovered;
            shell.request_redraw();
        }
        if !self.enabled
            || matches!(
                event,
                Event::Mouse(mouse::Event::CursorLeft)
                    | Event::Window(
                        iced::window::Event::Unfocused | iced::window::Event::Resized(_)
                    )
                    | Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    })
            )
        {
            if state.drag.take().is_some() {
                shell.request_redraw();
            }
            return;
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if hovered => {
                state.drag = Some((cursor.position().unwrap().x, self.width));
                state.last = Some(self.width);
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some((x, width)) = state.drag {
                    let (min, direction) = match self.target {
                        Target::Navigation => (NAV_MIN, 1.0),
                        _ => (SIDE_MIN, -1.0),
                    };
                    let width =
                        (width + direction * (position.x - x)).clamp(min, self.max.max(min));
                    if state.last != Some(width) {
                        state.last = Some(width);
                        shell.publish(Message::ResizePanel(self.target, width));
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.drag.take().is_some() =>
            {
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let state = tree.state.downcast_ref::<State>();
        let active = self.enabled && (state.drag.is_some() || cursor.is_over(layout.bounds()));
        let bounds = Rectangle {
            x: layout.bounds().center_x() - 0.5,
            width: 1.0,
            ..layout.bounds()
        };
        if let Some(bounds) = bounds.intersection(viewport) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    ..Default::default()
                },
                if active {
                    shell::ACCENT.scale_alpha(0.65)
                } else {
                    shell::LINE
                },
            );
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        _: &Renderer,
    ) -> mouse::Interaction {
        if self.enabled
            && (tree.state.downcast_ref::<State>().drag.is_some()
                || (cursor.is_over(layout.bounds()) && cursor.is_over(*viewport)))
        {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Vector};

    #[derive(Default)]
    struct Bounds {
        navigation: Option<Rectangle>,
        side: Option<Rectangle>,
    }
    impl widget::Operation for Bounds {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
            f(self);
        }
        fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
            if id == Some(&widget::Id::new("navigation-divider")) {
                self.navigation = Some(bounds);
            }
            if id == Some(&widget::Id::new("thread-divider"))
                || id == Some(&widget::Id::new("people-divider"))
            {
                self.side = Some(bounds);
            }
        }
    }

    #[test]
    fn full_shell_drags_survive_rebuilds_and_preserve_drafts_and_reading_positions() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(1440.0, 900.0);
        for panel in 0..3 {
            let mut app = crate::tests::demo_app();
            if panel == 1 {
                let _ = app.update(Message::Thread("review".into()));
            }
            if panel == 2 {
                let _ = app.update(Message::Members(super::super::members::Event::Open));
            }
            let room = app.draft_key(false).unwrap();
            app.drafts.get_mut(&room).unwrap().content =
                iced::widget::text_editor::Content::with_text("unsent draft");
            app.scroll_positions.insert(room.clone(), 123.0);
            let selected = app.selection.clone();
            let mut tree = Tree::empty();
            let mut grip = Point::ORIGIN;
            for step in 0..4 {
                let mut view = app.shell_view();
                tree.diff(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                let mut bounds = Bounds::default();
                view.as_widget_mut()
                    .operate(&mut tree, Layout::new(&node), &renderer, &mut bounds);
                if step == 0 {
                    grip = if panel == 0 {
                        bounds.navigation.unwrap()
                    } else {
                        bounds.side.unwrap()
                    }
                    .center();
                    assert_eq!(
                        view.as_widget().mouse_interaction(
                            &tree,
                            Layout::new(&node),
                            mouse::Cursor::Available(grip),
                            &Rectangle::with_size(size),
                            &renderer
                        ),
                        mouse::Interaction::ResizingHorizontally
                    );
                }
                let point =
                    grip + Vector::new(if panel == 0 { 40.0 } else { -40.0 } * step as f32, 0.0);
                let event = match step {
                    0 => Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    3 => Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                    _ => Event::Mouse(mouse::Event::CursorMoved { position: point }),
                };
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &Rectangle::with_size(size),
                );
                assert!(!matches!(
                    shell.redraw_request(),
                    iced::window::RedrawRequest::At(_)
                ));
                drop(view);
                for message in messages {
                    let _ = app.update(message);
                }
            }
            let width = match panel {
                0 => app.shell.navigation_width,
                1 => app.shell.thread_width,
                _ => app.shell.people_width,
            };
            assert_eq!(width, if panel == 0 { 322.0 } else { 440.0 });
            assert_eq!(app.selection, selected);
            assert_eq!(app.drafts[&room].content.text().trim_end(), "unsent draft");
            assert_eq!(app.scroll_positions[&room], 123.0);
        }
    }

    #[test]
    fn drag_limits_cancellation_and_stale_or_modal_updates() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut handle = Divider {
            target: Target::Navigation,
            width: 242.0,
            max: 400.0,
            enabled: true,
        };
        let mut tree = Tree::new(&handle as &dyn Widget<Message, Theme, Renderer>);
        let node = handle.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(5.0, 600.0)),
        );
        let dispatch = |handle: &mut Divider, tree: &mut Tree, event: Event, point: Point| {
            let mut messages = Vec::new();
            handle.update(
                tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(Size::new(1440.0, 900.0)),
            );
            messages
        };
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        dispatch(&mut handle, &mut tree, press.clone(), Point::new(2.0, 20.0));
        for (x, expected) in [
            (2000.0, Some(400.0)),
            (2000.0, None),
            (-1000.0, Some(NAV_MIN)),
        ] {
            let point = Point::new(x, 20.0);
            let messages = dispatch(
                &mut handle,
                &mut tree,
                Event::Mouse(mouse::Event::CursorMoved { position: point }),
                point,
            );
            assert_eq!(
                messages
                    .iter()
                    .find_map(|m| if let Message::ResizePanel(_, width) = m {
                        Some(*width)
                    } else {
                        None
                    }),
                expected
            );
        }
        for event in [
            Event::Window(iced::window::Event::Unfocused),
            Event::Mouse(mouse::Event::CursorLeft),
            Event::Window(iced::window::Event::Resized(Size::new(1000.0, 600.0))),
        ] {
            dispatch(&mut handle, &mut tree, press.clone(), Point::new(2.0, 20.0));
            dispatch(&mut handle, &mut tree, event, Point::new(2.0, 20.0));
            assert!(tree.state.downcast_ref::<State>().drag.is_none());
        }
        dispatch(&mut handle, &mut tree, press.clone(), Point::new(2.0, 20.0));
        handle.target = Target::People(2);
        handle.diff(&mut tree);
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let generation = app.selection.thread.as_ref().unwrap().0;
        app.resize_panel(Target::Thread(generation + 1), 500.0);
        assert_eq!(app.shell.thread_width, 360.0);
        app.resize_panel(Target::Thread(generation), 500.0);
        assert_eq!(app.shell.thread_width, 500.0);
        let _ = app.update(Message::Members(super::super::members::Event::Open));
        app.resize_panel(Target::Thread(generation), 550.0);
        assert_eq!(app.shell.thread_width, 500.0);
        app.resize_panel(Target::Navigation, f32::NAN);
        assert_eq!(app.shell.navigation_width, 242.0);
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        app.resize_panel(Target::Navigation, 400.0);
        assert_eq!(app.shell.navigation_width, 242.0);
        assert!(app.side_panel_replaces_room(640.0));
        assert!(!app.side_panel_replaces_room(1440.0));
    }
}
