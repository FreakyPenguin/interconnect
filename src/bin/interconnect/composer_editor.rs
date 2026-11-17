//! Native editor scrolling with a quiet scrollbar and an event-driven resize grip.
use super::{DraftKey, Message, shell};
use iced::{
    Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree},
    },
    widget::text_editor,
};

pub(super) const MIN_HEIGHT: f32 = 56.0;
pub(super) const MAX_HEIGHT: f32 = 320.0;
const GRIP: f32 = 8.0;

pub(super) fn view<'a>(
    editor: Element<'a, Message>,
    content: &'a text_editor::Content,
    key: &DraftKey,
    generation: u64,
    height: f32,
    max_height: f32,
    enabled: bool,
) -> Element<'a, Message> {
    Element::new(Editor {
        editor,
        content,
        key: key.clone(),
        generation,
        height: height.clamp(MIN_HEIGHT, max_height),
        max_height,
        enabled,
    })
}
struct Editor<'a> {
    editor: Element<'a, Message>,
    content: &'a text_editor::Content,
    key: DraftKey,
    generation: u64,
    height: f32,
    max_height: f32,
    enabled: bool,
}
#[derive(Clone, Copy, Debug, Default)]
struct Metrics {
    offset: f32,
    total: f32,
    visible: f32,
    line: f32,
}
impl Metrics {
    fn maximum(self) -> f32 {
        (self.total - self.visible).max(0.0)
    }
    fn thumb(self, rail: Rectangle) -> Option<Rectangle> {
        let maximum = self.maximum();
        if maximum <= 0.5 || rail.height <= 0.0 {
            return None;
        }
        let height =
            (rail.height * self.visible / self.total).clamp(18.0_f32.min(rail.height), rail.height);
        Some(Rectangle {
            y: rail.y + (rail.height - height) * (self.offset / maximum).clamp(0.0, 1.0),
            height,
            ..rail
        })
    }
}
#[derive(Clone, Copy)]
enum Drag {
    Resize { y: f32, height: f32 },
    Scroll { grab: f32 },
}
#[derive(Default)]
struct State {
    key: Option<DraftKey>,
    generation: u64,
    drag: Option<Drag>,
    metrics: Metrics,
    last_height: Option<f32>,
    hovered: bool,
}
impl Editor<'_> {
    fn rail(bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x + bounds.width - 9.0,
            y: bounds.y + GRIP + 12.0,
            width: 4.0,
            height: (bounds.height - GRIP - 20.0).max(0.0),
        }
    }
    fn scroll_to(
        &self,
        state: &State,
        rail: Rectangle,
        y: f32,
        grab: f32,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(thumb) = state.metrics.thumb(rail) else {
            return;
        };
        let travel = rail.height - thumb.height;
        if travel <= 0.0 {
            return;
        }
        let fraction = ((y - rail.y - grab) / travel).clamp(0.0, 1.0);
        if (fraction <= 0.0 && state.metrics.offset <= 0.5)
            || (fraction >= 1.0 && state.metrics.offset >= state.metrics.maximum() - 0.5)
        {
            return;
        }
        let lines = if fraction >= 1.0 {
            i32::MAX / 2
        } else if fraction <= 0.0 {
            i32::MIN / 2
        } else {
            ((fraction * state.metrics.maximum() - state.metrics.offset) / state.metrics.line)
                .round() as i32
        };
        if lines != 0 {
            shell.publish(Message::Edit(
                self.key.clone(),
                text_editor::Action::Scroll { lines },
            ));
        }
    }
}
impl Widget<Message, Theme, Renderer> for Editor<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            key: Some(self.key.clone()),
            generation: self.generation,
            ..Default::default()
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.editor)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.key.as_ref() != Some(&self.key)
            || state.generation != self.generation
            || !self.enabled
        {
            state.drag = None;
            state.last_height = None;
            state.key = Some(self.key.clone());
            state.generation = self.generation;
        }
        tree.diff_children(std::slice::from_ref(&self.editor));
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.height + GRIP))
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.shrink(Size::new(0.0, GRIP)).height(self.height);
        let node = self
            .editor
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits);
        let size = Size::new(node.size().width, node.size().height + GRIP);
        tree.state.downcast_mut::<State>().metrics = self.content.with_editor(|editor| {
            let buffer = editor.buffer();
            let line = buffer.metrics().line_height;
            let scroll = buffer.scroll();
            let mut total = 0.0;
            let mut offset = scroll.vertical;
            for (index, paragraph) in buffer.lines.iter().enumerate() {
                // Offscreen paragraphs are lazy; refine their extent when visited.
                let height = paragraph.layout_opt().map_or(line, |rows| {
                    rows.iter()
                        .map(|row| row.line_height_opt.unwrap_or(line))
                        .sum()
                });
                total += height;
                if index < scroll.line {
                    offset += height;
                }
            }
            Metrics {
                offset,
                total,
                visible: (size.height - GRIP - 20.0).max(0.0),
                line,
            }
        });
        layout::Node::with_children(size, vec![node.move_to(Point::new(0.0, GRIP))])
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.container(
            Some(&widget::Id::new("composer-editor-surface")),
            layout.bounds(),
        );
        self.editor.as_widget_mut().operate(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
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
        let state = tree.state.downcast_mut::<State>();
        let hovered = cursor.is_over(layout.bounds()) && cursor.is_over(*viewport);
        if hovered != state.hovered {
            state.hovered = hovered;
            shell.request_redraw();
        }
        if (!self.enabled
            || matches!(
                event,
                Event::Mouse(mouse::Event::CursorLeft)
                    | Event::Window(
                        iced::window::Event::Unfocused | iced::window::Event::Resized(_)
                    )
            ))
            && state.drag.take().is_some()
        {
            shell.request_redraw();
        }
        let bounds = layout.bounds();
        let rail = Self::rail(bounds);
        if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
            match state.drag {
                Some(Drag::Resize { y, height }) => {
                    let height = (height + y - position.y).clamp(MIN_HEIGHT, self.max_height);
                    if state.last_height != Some(height) {
                        state.last_height = Some(height);
                        shell.publish(Message::ResizeComposer {
                            key: self.key.clone(),
                            generation: self.generation,
                            height,
                        });
                    }
                    shell.capture_event();
                    return;
                }
                Some(Drag::Scroll { grab }) => {
                    self.scroll_to(state, rail, position.y, grab, shell);
                    shell.capture_event();
                    return;
                }
                None => {}
            }
        }
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) && state.drag.take().is_some()
        {
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        if self.enabled
            && cursor.is_over(*viewport)
            && cursor.is_over(bounds)
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
        {
            let point = cursor.position().unwrap();
            if point.y <= bounds.y + GRIP {
                state.drag = Some(Drag::Resize {
                    y: point.y,
                    height: self.height,
                });
                state.last_height = Some(self.height);
                shell.capture_event();
                shell.request_redraw();
                return;
            }
            if let Some(thumb) = state.metrics.thumb(rail)
                && point.x >= rail.x - 4.0
                && point.y >= rail.y
                && point.y <= rail.y + rail.height
            {
                let grab = if thumb.contains(point) {
                    point.y - thumb.y
                } else {
                    thumb.height / 2.0
                };
                state.drag = Some(Drag::Scroll { grab });
                self.scroll_to(state, rail, point.y, grab, shell);
                shell.capture_event();
                shell.request_redraw();
                return;
            }
        }
        self.editor.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.children().next().unwrap(),
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
        self.editor.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
            viewport,
        );
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let over = cursor.is_over(bounds) && cursor.is_over(*viewport);
        let grip = Rectangle {
            x: bounds.center_x() - 14.0,
            y: bounds.y + 3.0,
            width: 28.0,
            height: 2.0,
        };
        if let Some(bounds) = grip.intersection(viewport) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border::default().rounded(1),
                    ..Default::default()
                },
                if over || state.drag.is_some() {
                    shell::MUTED.scale_alpha(0.65)
                } else {
                    shell::LINE
                },
            );
        }
        if let Some(thumb) = state.metrics.thumb(Self::rail(bounds))
            && let Some(bounds) = thumb.intersection(viewport)
        {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border::default().rounded(2),
                    ..Default::default()
                },
                if over || state.drag.is_some() {
                    Color::from_rgb8(167, 177, 191)
                } else {
                    Color::from_rgb8(202, 209, 219)
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
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        if self.enabled
            && (matches!(state.drag, Some(Drag::Resize { .. }))
                || (cursor.is_over(*viewport)
                    && cursor.is_over(bounds)
                    && cursor
                        .position()
                        .is_some_and(|point| point.y <= bounds.y + GRIP)))
        {
            return mouse::Interaction::ResizingVertically;
        }
        let rail = Self::rail(bounds);
        if self.enabled
            && (matches!(state.drag, Some(Drag::Scroll { .. }))
                || (state.metrics.thumb(rail).is_some()
                    && cursor.is_over(*viewport)
                    && cursor.position().is_some_and(|p| {
                        p.x >= rail.x - 4.0
                            && p.x <= bounds.x + bounds.width
                            && p.y >= rail.y
                            && p.y <= rail.y + rail.height
                    })))
        {
            return mouse::Interaction::Pointer;
        }
        self.editor.as_widget().mouse_interaction(
            &tree.children[0],
            layout.children().next().unwrap(),
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
        self.editor.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::Widget;

    fn editor<'a>(content: &'a text_editor::Content) -> Editor<'a> {
        let key = DraftKey {
            room: "room".into(),
            root: None,
        };
        let target = key.clone();
        let native = text_editor(content)
            .height(Length::Fill)
            .size(14)
            .padding(iced::Padding {
                top: 12.0,
                right: 18.0,
                bottom: 8.0,
                left: 12.0,
            })
            .cursor_blinking(false)
            .on_action(move |action| Message::Edit(target.clone(), action));
        Editor {
            editor: native.into(),
            content,
            key,
            generation: 7,
            height: 56.0,
            max_height: 200.0,
            enabled: true,
        }
    }
    fn dispatch(
        editor: &mut Editor<'_>,
        tree: &mut Tree,
        renderer: &Renderer,
        node: &layout::Node,
        event: Event,
        point: Point,
    ) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        editor.update(
            tree,
            &event,
            Layout::new(node),
            mouse::Cursor::Available(point),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(Size::new(300.0, 400.0)),
        );
        assert!(!matches!(
            shell.redraw_request(),
            iced::window::RedrawRequest::At(_)
        ));
        messages
    }
    #[test]
    fn resize_is_clamped_deduplicated_and_canceled_on_release_focus_loss_or_new_draft() {
        let content = text_editor::Content::new();
        let mut editor = editor(&content);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&editor as &dyn Widget<Message, Theme, Renderer>);
        let node = editor.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 400.0)),
        );
        let grip = Point::new(150.0, 3.0);
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            press.clone(),
            grip,
        );
        for (y, expected) in [
            (-97.0, Some(156.0)),
            (-900.0, Some(200.0)),
            (-900.0, None),
            (900.0, Some(56.0)),
        ] {
            let point = Point::new(150.0, y);
            let messages = dispatch(
                &mut editor,
                &mut tree,
                &renderer,
                &node,
                Event::Mouse(mouse::Event::CursorMoved { position: point }),
                point,
            );
            let height = messages.iter().find_map(|message| match message {
                Message::ResizeComposer {
                    generation: 7,
                    height,
                    ..
                } => Some(*height),
                _ => None,
            });
            assert_eq!(height, expected);
            editor.diff(&mut tree); // A new app view must not cancel an ongoing drag.
        }
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Point::new(500.0, 500.0),
        );
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            press.clone(),
            grip,
        );
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Window(iced::window::Event::Unfocused),
            grip,
        );
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
        dispatch(&mut editor, &mut tree, &renderer, &node, press, grip);
        editor.key.room = "another-room".into();
        editor.diff(&mut tree);
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
    }
    #[test]
    fn scrollbar_tracks_wrapping_and_scrolls_without_changing_text_or_selection() {
        let source =
            "A paragraph with enough words to wrap across several visual lines.\n".repeat(40);
        let mut content = text_editor::Content::with_text(&source);
        content.move_to(iced::advanced::text::editor::Cursor {
            position: iced::advanced::text::editor::Position { line: 0, column: 1 },
            selection: Some(iced::advanced::text::editor::Position { line: 0, column: 0 }),
        });
        let cursor = content.cursor();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut editor = editor(&content);
        let mut tree = Tree::new(&editor as &dyn Widget<Message, Theme, Renderer>);
        let limits = layout::Limits::new(Size::ZERO, Size::new(300.0, 400.0));
        let node = editor.layout(&mut tree, &renderer, &limits);
        let rail = Editor::rail(Rectangle::with_size(node.size()));
        let metrics = tree.state.downcast_ref::<State>().metrics;
        assert!(metrics.thumb(rail).is_some());
        let bottom = Point::new(rail.center_x(), rail.y + rail.height);
        let messages = dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bottom,
        );
        drop(editor);
        for message in messages {
            if let Message::Edit(_, action) = message {
                content.perform(action);
            }
        }
        assert_eq!(content.cursor(), cursor);
        assert_eq!(content.text(), source);
        let mut editor = self::editor(&content);
        let node = editor.layout(&mut tree, &renderer, &limits);
        let metrics = tree.state.downcast_ref::<State>().metrics;
        assert!(metrics.offset > 0.0);
        assert!(metrics.offset >= metrics.maximum() - metrics.line);
        let thumb = metrics
            .thumb(Editor::rail(Rectangle::with_size(node.size())))
            .unwrap();
        assert!(thumb.y + thumb.height <= rail.y + rail.height + 0.1);
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            bottom,
        );
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            thumb.center(),
        );
        let point = Point::new(rail.center_x(), rail.y);
        let messages = dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::CursorMoved { position: point }),
            point,
        );
        drop(editor);
        for message in messages {
            if let Message::Edit(_, action) = message {
                content.perform(action);
            }
        }
        let mut editor = self::editor(&content);
        editor.layout(&mut tree, &renderer, &limits);
        assert!(tree.state.downcast_ref::<State>().metrics.offset < 0.5);
        assert_eq!(content.cursor(), cursor);
        assert_eq!(content.text(), source);
        let short = text_editor::Content::with_text("A short draft");
        let mut short_editor = self::editor(&short);
        let mut short_tree = Tree::new(&short_editor as &dyn Widget<Message, Theme, Renderer>);
        short_editor.layout(&mut short_tree, &renderer, &limits);
        assert!(
            short_tree
                .state
                .downcast_ref::<State>()
                .metrics
                .thumb(rail)
                .is_none()
        );
    }
    #[test]
    fn resize_grip_routes_through_the_full_app_view() {
        #[derive(Default)]
        struct Probe(Option<Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                if id == Some(&widget::Id::new("composer-editor-surface")) {
                    self.0 = Some(bounds);
                }
            }
        }
        let app = crate::tests::demo_app();
        let mut view = app.view();
        let mut tree = Tree::new(&view);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(1280.0, 900.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        let bounds = probe.0.unwrap();
        let grip = Point::new(bounds.center_x(), bounds.y + 3.0);
        let target = grip - Vector::new(0.0, 120.0);
        let mut messages = Vec::new();
        for (event, point) in [
            (
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                grip,
            ),
            (
                Event::Mouse(mouse::Event::CursorMoved { position: target }),
                target,
            ),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::ResizeComposer { height, .. } if *height == 176.0)),
            "grip must emit resize: {bounds:?} {messages:?}"
        );
    }
    #[test]
    fn sizes_are_per_draft_and_stale_or_modal_resize_messages_are_ignored() {
        let mut app = crate::tests::demo_app();
        let room = app.draft_key(false).unwrap();
        let generation = app.selection.room.as_ref().unwrap().0;
        let resize = |key, generation, height| Message::ResizeComposer {
            key,
            generation,
            height,
        };
        let _ = app.update(resize(room.clone(), generation, 200.0));
        let _ = app.update(Message::Thread("review".into()));
        let thread = app.draft_key(true).unwrap();
        let _ = app.update(resize(
            thread.clone(),
            app.selection.thread.as_ref().unwrap().0,
            130.0,
        ));
        assert_eq!(app.drafts[&room].composer_height, Some(200.0));
        assert_eq!(app.drafts[&thread].composer_height, Some(130.0));
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.drafts[&room].composer_height, Some(200.0));
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(resize(room.clone(), generation, 300.0));
        let _ = app.update(Message::Room(room.room.clone()));
        let _ = app.update(resize(room.clone(), generation, 300.0));
        let current = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(resize(room.clone(), current, f32::NAN));
        let _ = app.settings_update(crate::settings::Event::Open);
        let _ = app.update(resize(room.clone(), current, 300.0));
        assert_eq!(app.drafts[&room].composer_height, Some(200.0));
    }
}
