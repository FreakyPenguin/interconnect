//! Timeline selection uses message IDs and logical text positions, never layout
//! text. Clipboard output preserves source whitespace and excludes UI chrome.
use super::{Message, selectable};
use iced::advanced::text::editor::{Cursor, Position};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, mouse,
    widget::{self, Tree},
};
use iced::widget::text_editor::{Action, Motion};
use iced::{Event, Point, Rectangle, Renderer, Theme, Vector};
use std::{any::Any, time::Duration};

type Content<'a> = dyn Widget<Message, Theme, Renderer> + 'a;
#[derive(Clone)]
struct Endpoint {
    id: String,
    body: String,
    position: Position,
}
#[derive(Default)]
pub struct Selection {
    range: Option<(Endpoint, Endpoint)>,
    dragging: bool,
    pointer: Option<Point>,
}
#[derive(Clone)]
struct Row {
    id: String,
    name: String,
    body: String,
    bounds: Rectangle,
    cursor: Cursor,
    clicks: u64,
}
#[derive(Default)]
struct Read {
    rows: Vec<Row>,
    viewport: Rectangle,
    translation: Vector,
    maximum: f32,
    thread: bool,
}
impl widget::Operation for Read {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn scrollable(
        &mut self,
        id: Option<&widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _: &mut dyn widget::operation::Scrollable,
    ) {
        self.viewport = bounds;
        self.translation = translation;
        self.maximum = (content.height - bounds.height).max(0.0);
        self.thread = id == Some(&widget::Id::new("thread-timeline"));
    }
    fn custom(&mut self, _: Option<&widget::Id>, bounds: Rectangle, state: &mut dyn Any) {
        if let Some(state) = state.downcast_mut::<selectable::State>() {
            self.rows.push(Row {
                id: state.id.clone(),
                name: state.name.clone(),
                body: state.body.clone(),
                bounds: bounds - self.translation,
                cursor: state.content.cursor(),
                clicks: state.clicks,
            });
        }
    }
}
fn read(
    content: &mut Content<'_>,
    tree: &mut Tree,
    layout: Layout<'_>,
    renderer: &Renderer,
) -> Read {
    let mut result = Read::default();
    content.operate(tree, layout, renderer, &mut result);
    result
}
fn endpoint(row: &Row, position: Position) -> Endpoint {
    Endpoint {
        id: row.id.clone(),
        body: row.body.clone(),
        position,
    }
}
fn start() -> Position {
    Position { line: 0, column: 0 }
}
fn end(body: &str) -> Position {
    // Match the editor's logical lines; CRLF is a single line ending.
    let content = iced::widget::text_editor::Content::<Renderer>::with_text(body);
    let line = content.line_count().saturating_sub(1);
    Position {
        line,
        column: content.line(line).map_or(0, |line| line.text.len()),
    }
}
fn order(position: Position) -> (usize, usize) {
    (position.line, position.column)
}
// Translate logical editor positions back to the source. Soft wrapping never
// adds line breaks, and original indentation, blank lines, and CRLF survive.
fn fragment(body: &str, cursor: Cursor) -> Option<&str> {
    let content = iced::widget::text_editor::Content::<Renderer>::with_text(body);
    let offset = |position: Position| {
        let mut offset = 0;
        for (index, line) in content.lines().enumerate() {
            if index == position.line {
                return (position.column <= line.text.len()).then_some(offset + position.column);
            }
            offset += line.text.len() + line.ending.as_str().len();
        }
        None
    };
    let a = offset(cursor.selection?)?;
    let b = offset(cursor.position)?;
    body.get(a.min(b)..a.max(b))
}
struct Apply {
    ranges: Vec<(String, Cursor)>,
}
impl widget::Operation for Apply {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn custom(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn Any) {
        if let Some(state) = state.downcast_mut::<selectable::State>() {
            let cursor = self
                .ranges
                .iter()
                .find(|(id, _)| *id == state.id)
                .map(|(_, cursor)| *cursor);
            state.selected = cursor.is_some();
            let cursor = cursor.unwrap_or(Cursor {
                position: start(),
                selection: None,
            });
            if state.content.cursor() != cursor {
                // Iced's move_to(None) alone does not clear an old selection.
                state.content.perform(Action::Move(Motion::DocumentStart));
                state.content.move_to(cursor);
            }
        }
    }
}
impl Selection {
    fn ranges(&self, rows: &[Row]) -> Option<Vec<(String, Cursor)>> {
        let (a, b) = self.range.as_ref()?;
        let ai = rows
            .iter()
            .position(|row| row.id == a.id && row.body == a.body)?;
        let bi = rows
            .iter()
            .position(|row| row.id == b.id && row.body == b.body)?;
        let (lo, hi, from, to) = if (ai, order(a.position)) <= (bi, order(b.position)) {
            (ai, bi, a.position, b.position)
        } else {
            (bi, ai, b.position, a.position)
        };
        Some(
            (lo..=hi)
                .map(|i| {
                    (
                        rows[i].id.clone(),
                        Cursor {
                            selection: Some(if i == lo { from } else { start() }),
                            position: if i == hi { to } else { end(&rows[i].body) },
                        },
                    )
                })
                .collect(),
        )
    }
    pub fn restore(
        &mut self,
        content: &mut Content<'_>,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
    ) {
        if self.range.is_none() {
            return;
        }
        let rows = read(content, tree, layout, renderer).rows;
        let ranges = self.ranges(&rows);
        if ranges.is_none() {
            self.range = None;
            self.dragging = false;
        }
        content.operate(
            tree,
            layout,
            renderer,
            &mut Apply {
                ranges: ranges.unwrap_or_default(),
            },
        );
    }
    fn clear(
        &mut self,
        content: &mut Content<'_>,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
    ) {
        if self.range.take().is_some() {
            content.operate(tree, layout, renderer, &mut Apply { ranges: vec![] });
        }
        self.dragging = false;
    }
    fn extend(
        &mut self,
        content: &mut Content<'_>,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        point: Point,
    ) {
        let data = read(content, tree, layout, renderer);
        let point = Point::new(
            point.x,
            point
                .y
                .clamp(data.viewport.y, data.viewport.y + data.viewport.height),
        );
        let Some(row) = data
            .rows
            .iter()
            .find(|row| point.y < row.bounds.y + row.bounds.height)
            .or(data.rows.last())
        else {
            return;
        };
        struct Hit {
            id: String,
            point: Point,
            position: Position,
        }
        impl widget::Operation for Hit {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn custom(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn Any) {
                if let Some(state) = state.downcast_mut::<selectable::State>()
                    && state.id == self.id
                {
                    state.hit(self.point);
                    self.position = state.content.cursor().position;
                }
            }
        }
        let position = if point.y < row.bounds.y {
            start()
        } else if point.y >= row.bounds.y + row.bounds.height {
            end(&row.body)
        } else {
            let mut hit = Hit {
                id: row.id.clone(),
                point: Point::new(
                    (point.x - row.bounds.x).clamp(0.0, row.bounds.width),
                    point.y - row.bounds.y,
                ),
                position: start(),
            };
            content.operate(tree, layout, renderer, &mut hit);
            hit.position
        };
        if let Some((_, b)) = &mut self.range {
            *b = endpoint(row, position);
        }
        self.restore(content, tree, layout, renderer);
    }
    fn copy(
        &self,
        content: &mut Content<'_>,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
    ) -> Option<String> {
        let rows = read(content, tree, layout, renderer).rows;
        let ranges = self.ranges(&rows)?;
        let multi = ranges.len() > 1;
        let mut parts = Vec::new();
        for (id, cursor) in ranges {
            let row = rows.iter().find(|row| row.id == id)?;
            if let Some(selected) = fragment(&row.body, cursor).filter(|text| !text.is_empty()) {
                parts.push(if multi && !row.name.is_empty() {
                    format!("{}:\n{selected}", row.name.replace(['\r', '\n'], " "))
                } else {
                    selected.to_owned()
                });
            }
        }
        (!parts.is_empty()).then(|| parts.join("\n\n"))
    }
    // Own the drag once it starts in a body. Headers/gutters/actions are never
    // selection sources; crossing them simply connects adjacent message text.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        content: &mut Content<'_>,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
        generation: u64,
    ) {
        if let Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) = event
        {
            if modifiers.command()
                && key.to_latin(*physical_key) == Some('c')
                && self.range.is_some()
            {
                if let Some(text) = self.copy(content, tree, layout, renderer) {
                    clipboard.write(iced::advanced::clipboard::Kind::Standard, text);
                }
                shell.capture_event();
                return;
            }
            // Shell shortcuts transfer focus or navigate. Do not let a retained
            // transcript selection steal Copy from the search field/composer.
            if super::shell_shortcut(key, *modifiers).is_some() {
                self.clear(content, tree, layout, renderer);
                shell.request_redraw();
            }
        }
        if matches!(event, Event::Window(iced::window::Event::Unfocused)) {
            self.dragging = false;
        }
        if self.dragging
            && matches!(
                event,
                Event::Mouse(mouse::Event::CursorMoved { .. })
                    | Event::Window(iced::window::Event::RedrawRequested(_))
            )
        {
            if let Some(point) = cursor.position() {
                self.pointer = Some(point);
            }
            if let Some(point) = self.pointer {
                let data = read(content, tree, layout, renderer);
                let edge = 24.0;
                let dy = if point.x >= data.viewport.x
                    && point.x <= data.viewport.x + data.viewport.width
                {
                    if point.y < data.viewport.y + edge {
                        -10.0
                    } else if point.y > data.viewport.y + data.viewport.height - edge {
                        10.0
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                let next = (data.translation.y + dy).clamp(0.0, data.maximum);
                let mut scrolled = false;
                if dy != 0.0 && (next - data.translation.y).abs() > 1.0 {
                    if matches!(
                        event,
                        Event::Window(iced::window::Event::RedrawRequested(_))
                    ) {
                        struct Scroll(f32);
                        impl widget::Operation for Scroll {
                            fn traverse(&mut self, _: &mut dyn FnMut(&mut dyn widget::Operation)) {}
                            fn scrollable(
                                &mut self,
                                _: Option<&widget::Id>,
                                _: Rectangle,
                                _: Rectangle,
                                _: Vector,
                                state: &mut dyn widget::operation::Scrollable,
                            ) {
                                state.scroll_to(iced::widget::operation::AbsoluteOffset {
                                    x: None,
                                    y: Some(self.0),
                                });
                            }
                        }
                        let offset = data.maximum - next;
                        content.operate(tree, layout, renderer, &mut Scroll(offset));
                        scrolled = true;
                        shell.publish(Message::Scrolled {
                            thread: data.thread,
                            generation,
                            offset,
                        });
                    }
                    shell.request_redraw_at(iced::time::Instant::now() + Duration::from_millis(16));
                }
                if scrolled || matches!(event, Event::Mouse(_)) {
                    self.extend(content, tree, layout, renderer, point);
                }
                if matches!(event, Event::Mouse(_)) {
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            return;
        }
        if self.dragging
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            )
        {
            if let Some(point) = cursor.position()
                && Some(point) != self.pointer
            {
                self.extend(content, tree, layout, renderer, point);
            }
            self.dragging = false;
            // Release the underlying editor's local drag too.
            content.update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            shell.request_redraw();
            return;
        }
        let pressed = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        );
        let hit = if pressed {
            let data = read(content, tree, layout, renderer);
            cursor
                .position()
                .filter(|p| data.viewport.contains(*p))
                .and_then(|p| data.rows.into_iter().find(|row| row.bounds.contains(p)))
        } else {
            None
        };
        if pressed {
            if hit.is_none() {
                self.clear(content, tree, layout, renderer);
            } else {
                // Keep the clicked editor's caret until it interprets double /
                // triple clicks, then replace the range and clear other bodies.
                self.range = None;
                self.dragging = false;
            }
            shell.request_redraw();
        }
        // Cross-message keyboard events must not be delivered independently to
        // every highlighted editor. Copy/Escape above are timeline operations.
        if matches!(event, Event::Keyboard(_))
            && self.range.as_ref().is_some_and(|(a, b)| a.id != b.id)
        {
            return;
        }
        content.update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
        if let Some(hit) = hit {
            let data = read(content, tree, layout, renderer);
            if let Some(row) = data
                .rows
                .iter()
                .find(|row| row.id == hit.id && row.clicks != hit.clicks)
            {
                self.range = Some((
                    endpoint(row, row.cursor.selection.unwrap_or(row.cursor.position)),
                    endpoint(row, row.cursor.position),
                ));
                self.dragging = true;
                self.pointer = cursor.position();
                self.restore(content, tree, layout, renderer);
            } else {
                content.operate(tree, layout, renderer, &mut Apply { ranges: vec![] });
            }
        } else if matches!(event, Event::Keyboard(_))
            && let Some((a, b)) = &mut self.range
        {
            let data = read(content, tree, layout, renderer);
            if let Some(row) = data.rows.iter().find(|row| row.id == a.id) {
                *a = endpoint(row, row.cursor.selection.unwrap_or(row.cursor.position));
                *b = endpoint(row, row.cursor.position);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::layout;
    use iced::widget::{button, column, container, row, scrollable, space, text};
    use iced::{Element, Size};
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
    fn fixture<'a>(rows: &'a [(&str, &str, &str)], generation: u64) -> Element<'a, Message> {
        let messages = column(rows.iter().enumerate().map(|(index, (id, name, body))| {
            container(column![
                row![text(*name), text("12:34")],
                selectable::named_view(id, body, name),
                button("Reply").on_press(Message::Thread((*id).into())),
                space().height(12),
            ])
            .id(super::super::timeline::row_id(index))
            .into()
        }));
        super::super::timeline::view(
            generation,
            rows.iter().map(|(id, _, _)| Some((*id).into())).collect(),
            scrollable(messages)
                .id("room-timeline")
                .anchor_bottom()
                .height(200)
                .into(),
        )
    }
    struct Harness {
        tree: Tree,
        renderer: Renderer,
        clipboard: ClipboardText,
    }
    impl Harness {
        fn new() -> Self {
            Self {
                tree: Tree::empty(),
                renderer: crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0)),
                clipboard: ClipboardText::default(),
            }
        }
        fn layout(&mut self, element: &mut Element<'_, Message>) -> layout::Node {
            self.tree.diff(element.as_widget());
            element.as_widget_mut().layout(
                &mut self.tree,
                &self.renderer,
                &layout::Limits::new(Size::ZERO, Size::new(300.0, 200.0)),
            )
        }
        fn read(&mut self, element: &mut Element<'_, Message>, node: &layout::Node) -> Read {
            read(
                element.as_widget_mut(),
                &mut self.tree,
                Layout::new(node),
                &self.renderer,
            )
        }
        fn event(
            &mut self,
            element: &mut Element<'_, Message>,
            node: &layout::Node,
            event: Event,
            point: Point,
        ) -> (iced::window::RedrawRequest, Vec<Message>) {
            let mut messages = vec![];
            let mut shell = Shell::new(&mut messages);
            element.as_widget_mut().update(
                &mut self.tree,
                &event,
                Layout::new(node),
                mouse::Cursor::Available(point),
                &self.renderer,
                &mut self.clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, 200.0)),
            );
            (shell.redraw_request(), messages)
        }
        fn copy(&mut self, element: &mut Element<'_, Message>, node: &layout::Node) -> String {
            self.event(
                element,
                node,
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
                Point::ORIGIN,
            );
            self.clipboard.0.clone()
        }
    }
    fn down() -> Event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }
    fn up() -> Event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    }
    fn moved(point: Point) -> Event {
        Event::Mouse(mouse::Event::CursorMoved { position: point })
    }
    fn redraw() -> Event {
        Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))
    }
    #[test]
    fn formatted_and_plain_messages_share_cross_row_copy_and_idle_behavior() {
        let mut formatted = crate::demo::Demo::default()
            .pane(
                crate::DraftKey {
                    room: "studio".into(),
                    root: None,
                },
                1,
            )
            .rows[0]
            .clone();
        formatted.key = "formatted".into();
        formatted.display_name = "Alice".into();
        formatted.formatted = Some(std::sync::Arc::new(
            interconnect::message_document::Document::parse(
                "<p>café <b>bold 👋</b></p><p>second</p>",
            )
            .unwrap(),
        ));
        let mut h = Harness::new();
        let mut element = super::super::timeline::view(
            1,
            vec![Some("formatted".into()), Some("plain".into())],
            scrollable(
                column![
                    selectable::message_view(&formatted),
                    selectable::named_view("plain", "goodbye", "Bob")
                ]
                .spacing(6),
            )
            .id("room-timeline")
            .height(200)
            .into(),
        );
        let node = h.layout(&mut element);
        let data = h.read(&mut element, &node);
        let from = Point::new(data.rows[0].bounds.x, data.rows[0].bounds.y + 5.0);
        let to = Point::new(data.rows[1].bounds.x + 250.0, data.rows[1].bounds.y + 5.0);
        h.event(&mut element, &node, down(), from);
        h.event(&mut element, &node, moved(to), to);
        h.event(&mut element, &node, up(), to);
        assert_eq!(
            h.copy(&mut element, &node),
            "Alice:\ncafé bold 👋\n\nsecond\n\nBob:\ngoodbye"
        );
        let (redraw, messages) = h.event(&mut element, &node, redraw(), to);
        assert_eq!(redraw, iced::window::RedrawRequest::Wait);
        assert!(messages.is_empty());
    }
    #[test]
    fn intrinsic_image_arrival_preserves_the_visible_message_anchor() {
        fn view<'a>(
            rows: &'a [interconnect::connected::MessageRow],
            assets: crate::inline_image::Assets,
        ) -> Element<'a, Message> {
            super::super::timeline::view(
                1,
                rows.iter().map(|row| Some(row.key.clone())).collect(),
                scrollable(column(rows.iter().enumerate().map(|(index, row)| {
                    container(selectable::message_view_with_assets(
                        row,
                        assets.clone(),
                        None,
                    ))
                    .id(super::super::timeline::row_id(index))
                    .into()
                })))
                .id("room-timeline")
                .anchor_bottom()
                .height(200)
                .into(),
            )
        }
        let mut row = crate::demo::Demo::default()
            .pane(
                crate::DraftKey {
                    room: "studio".into(),
                    root: None,
                },
                1,
            )
            .rows[0]
            .clone();
        row.formatted = Some(std::sync::Arc::new(
            interconnect::message_document::Document::parse(
                "<p>text <img src='mxc://example.org/sizing' alt='diagram' width='40'> after</p>",
            )
            .unwrap(),
        ));
        let rows: Vec<_> = (0..20)
            .map(|index| {
                let mut row = row.clone();
                row.key = format!("image-{index}");
                row
            })
            .collect();
        let mut h = Harness::new();
        let mut element = view(&rows, crate::inline_image::Assets::default());
        let node = h.layout(&mut element);
        h.event(
            &mut element,
            &node,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: 3.0 },
            }),
            Point::new(100.0, 100.0),
        );
        let before = h.read(&mut element, &node);
        assert!(
            before.maximum - before.translation.y > 1.0,
            "read away from the bottom"
        );
        let anchor = before
            .rows
            .iter()
            .find(|row| row.bounds.y + row.bounds.height > before.viewport.y)
            .unwrap();
        let (id, y) = (anchor.id.clone(), anchor.bounds.y);
        let key = interconnect::image_media::Key::inline("mxc://example.org/sizing".into());
        let mut assets = crate::inline_image::Assets::default();
        assets.remote.ready = std::sync::Arc::new(
            [(
                key,
                iced::widget::image::Handle::from_rgba(4, 8, vec![0; 4 * 8 * 4]),
            )]
            .into_iter()
            .collect(),
        );
        let mut element = view(&rows, assets);
        let node = h.layout(&mut element);
        let after = h.read(&mut element, &node);
        let anchor = after.rows.iter().find(|row| row.id == id).unwrap();
        assert!(
            (anchor.bounds.y - y).abs() <= 1.0,
            "anchor moved from {y} to {}",
            anchor.bounds.y
        );
    }
    #[test]
    fn cross_message_copy_uses_alt_text_and_never_embedded_layout_markers() {
        let mut row = crate::demo::Demo::default()
            .pane(
                crate::DraftKey {
                    room: "studio".into(),
                    root: None,
                },
                1,
            )
            .rows[0]
            .clone();
        row.key = "image".into();
        row.display_name = "Alice".into();
        row.formatted = Some(std::sync::Arc::new(
            interconnect::message_document::Document::parse(
                "<p>before <img alt='diagram café 👋' width='32' height='40'> after</p>",
            )
            .unwrap(),
        ));
        for reverse in [false, true] {
            let mut h = Harness::new();
            let mut element = super::super::timeline::view(
                1,
                vec![Some("image".into()), Some("plain".into())],
                scrollable(
                    column![
                        selectable::message_view(&row),
                        selectable::named_view("plain", "bye", "Bob")
                    ]
                    .spacing(6),
                )
                .id("room-timeline")
                .height(200)
                .into(),
            );
            let node = h.layout(&mut element);
            let data = h.read(&mut element, &node);
            let from = Point::new(data.rows[0].bounds.x, data.rows[0].bounds.y + 5.0);
            let to = Point::new(data.rows[1].bounds.x + 250.0, data.rows[1].bounds.y + 5.0);
            let (from, to) = if reverse { (to, from) } else { (from, to) };
            h.event(&mut element, &node, down(), from);
            h.event(&mut element, &node, moved(to), to);
            h.event(&mut element, &node, up(), to);
            assert_eq!(
                h.copy(&mut element, &node),
                "Alice:\nbefore diagram café 👋 after\n\nBob:\nbye"
            );
        }
    }
    #[test]
    fn message_links_activate_on_click_and_never_on_selection_drags() {
        for plain in [false, true] {
            let mut row = crate::demo::Demo::default()
                .pane(
                    crate::DraftKey {
                        room: "studio".into(),
                        root: None,
                    },
                    1,
                )
                .rows[0]
                .clone();
            row.key = "link".into();
            row.formatted = Some(std::sync::Arc::new(
                interconnect::message_document::Document::parse(
                    "<p><a href='https://example.org/'>a long link label for selecting</a></p>",
                )
                .unwrap(),
            ));
            if plain {
                row.body = "https://example.org/a/long/path/for/selecting".into();
                row.formatted = interconnect::message_document::Document::plain_links(&row.body)
                    .map(std::sync::Arc::new);
            }
            let destination = if plain {
                "https://example.org/a/long/path/for/selecting"
            } else {
                "https://example.org/"
            };
            let mut h = Harness::new();
            let mut element = super::super::timeline::view(
                1,
                vec![Some("link".into())],
                scrollable(selectable::message_view(&row))
                    .id("room-timeline")
                    .height(200)
                    .into(),
            );
            let node = h.layout(&mut element);
            let data = h.read(&mut element, &node);
            let point = data.rows[0].bounds.position() + iced::Vector::new(5.0, 10.0);
            h.event(&mut element, &node, down(), point);
            let (_, messages) = h.event(&mut element, &node, up(), point);
            assert!(
                messages
                    .iter()
                    .any(|m| matches!(m, Message::OpenMessageLink(url) if url == destination))
            );
            let end = point + iced::Vector::new(90.0, 0.0);
            h.event(&mut element, &node, down(), point);
            h.event(&mut element, &node, moved(end), end);
            let (_, messages) = h.event(&mut element, &node, up(), end);
            assert!(
                !messages
                    .iter()
                    .any(|m| matches!(m, Message::OpenMessageLink(_)))
            );
            assert!(!h.copy(&mut element, &node).is_empty());
        }
    }
    #[test]
    fn mixed_activity_and_messages_copy_inline_names_once_and_preserve_wrapping() {
        let topic =
            "Changed the room topic to A calm place for longer discussions and useful references";
        for reverse in [false, true] {
            let mut h = Harness::new();
            let mut element = super::super::timeline::view(
                1,
                vec![
                    Some("one".into()),
                    Some("activity".into()),
                    Some("two".into()),
                ],
                scrollable(
                    column![
                        selectable::named_view("one", "hello", "Alice"),
                        selectable::activity_view("activity", topic, "Alice"),
                        selectable::named_view("two", "goodbye", "Bob"),
                    ]
                    .spacing(6),
                )
                .id("room-timeline")
                .height(200)
                .into(),
            );
            let node = h.layout(&mut element);
            let data = h.read(&mut element, &node);
            let first = data.rows[0].bounds;
            let last = data.rows[2].bounds;
            assert!(
                data.rows[1].bounds.height > first.height,
                "the activity wraps"
            );
            let a = Point::new(first.x, first.y + 5.0);
            let b = Point::new(last.x + last.width - 1.0, last.y + last.height - 5.0);
            let (from, to) = if reverse { (b, a) } else { (a, b) };
            h.event(&mut element, &node, down(), from);
            h.event(&mut element, &node, moved(to), to);
            h.event(&mut element, &node, up(), to);
            let expected = format!("Alice:\nhello\n\nAlice · {topic}\n\nBob:\ngoodbye");
            assert_eq!(h.copy(&mut element, &node), expected);
            let node = h.layout(&mut element);
            assert_eq!(h.copy(&mut element, &node), expected);
            assert_eq!(
                h.event(&mut element, &node, redraw(), to).0,
                iced::window::RedrawRequest::Wait
            );
        }
    }
    #[test]
    fn forward_and_backward_copy_include_names_but_no_layout_or_actions() {
        let rows = [
            ("one", "Alice", "hello café 👋"),
            ("two", "Bob", "  indented\n\nnext line"),
            ("three", "Alice", "goodbye world"),
        ];
        // Tall enough for all three messages; use outer limits matching its height.
        for reverse in [false, true] {
            let mut h = Harness::new();
            let mut element = fixture(&rows[..2], 1);
            let node = h.layout(&mut element);
            let data = h.read(&mut element, &node);
            let first = &data.rows[0];
            let last = &data.rows[1];
            let a = Point::new(first.bounds.x + 30.0, first.bounds.y + 5.0);
            let b = Point::new(
                last.bounds.x + 40.0,
                last.bounds.y + last.bounds.height - 5.0,
            );
            let (from, to) = if reverse { (b, a) } else { (a, b) };
            h.event(&mut element, &node, down(), from);
            h.event(&mut element, &node, moved(to), to);
            h.event(&mut element, &node, up(), to);
            let copied = h.copy(&mut element, &node);
            assert!(copied.starts_with("Alice:\n"), "{copied:?}");
            assert!(copied.contains("\n\nBob:\n  indented\n\n"), "{copied:?}");
            assert!(!copied.contains("Reply"));
            assert!(!copied.contains("12:34"));
            assert!(copied.contains("café 👋"));
            let original = copied.clone();
            // Arrivals and a new widget layout do not change the selected IDs/text.
            let mut element = fixture(&rows, 1);
            let node = h.layout(&mut element);
            assert_eq!(h.copy(&mut element, &node), original);
            // Nor does prepending history and moving both endpoints to new slots.
            let prepended = [("old", "Older", "history"), rows[0], rows[1], rows[2]];
            let mut element = fixture(&prepended, 1);
            let node = h.layout(&mut element);
            assert_eq!(h.copy(&mut element, &node), original);
            assert_eq!(
                h.event(&mut element, &node, redraw(), to).0,
                iced::window::RedrawRequest::Wait
            );
        }
    }
    #[test]
    fn edge_drag_scrolls_only_until_release_and_keeps_intermediate_messages() {
        let rows = [
            ("one", "Alice", "first"),
            ("two", "Bob", "second"),
            ("three", "Carol", "third"),
            ("four", "Dave", "last"),
        ];
        let mut h = Harness::new();
        let mut element = fixture(&rows, 1);
        let node = h.layout(&mut element);
        let data = h.read(&mut element, &node);
        let bottom = data.rows.last().unwrap().bounds;
        let from = Point::new(bottom.x + 100.0, bottom.y + 5.0);
        h.event(&mut element, &node, down(), from);
        let top = Point::new(1.0, 1.0);
        h.event(&mut element, &node, moved(top), top);
        let (request, messages) = h.event(&mut element, &node, redraw(), top);
        assert!(matches!(request, iced::window::RedrawRequest::At(_)));
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Scrolled { .. }))
        );
        for _ in 0..40 {
            h.event(&mut element, &node, redraw(), top);
        }
        h.event(&mut element, &node, up(), top);
        assert_eq!(
            h.copy(&mut element, &node),
            "Alice:\nfirst\n\nBob:\nsecond\n\nCarol:\nthird\n\nDave:\nlast"
        );
        assert_eq!(
            h.event(&mut element, &node, redraw(), top).0,
            iced::window::RedrawRequest::Wait
        );
    }
    #[test]
    fn copied_fragments_preserve_source_whitespace_and_unicode() {
        for body in [
            "  code\n\n    café 👋\n",
            "  code\r\n\r\n    café 👋\r\n",
            "first\rsecond",
            "a\nb",
        ] {
            assert_eq!(
                fragment(
                    body,
                    Cursor {
                        selection: Some(start()),
                        position: end(body)
                    }
                ),
                Some(body)
            );
        }
        let body = "é🙂 hello\nnext";
        assert_eq!(
            fragment(
                body,
                Cursor {
                    selection: Some(Position { line: 0, column: 2 }),
                    position: Position { line: 0, column: 6 }
                }
            ),
            Some("🙂")
        );
    }
    #[test]
    fn selection_clears_on_navigation_edit_and_outside_click() {
        let rows = [("one", "Alice", "hello"), ("two", "Bob", "world")];
        for clear in ["navigation", "edit", "outside", "keyboard", "removed"] {
            let mut h = Harness::new();
            let mut element = fixture(&rows, 1);
            let node = h.layout(&mut element);
            let data = h.read(&mut element, &node);
            let a = Point::new(0.0, data.rows[0].bounds.y + 5.0);
            let b = Point::new(100.0, data.rows[1].bounds.y + 5.0);
            h.event(&mut element, &node, down(), a);
            h.event(&mut element, &node, moved(b), b);
            h.event(&mut element, &node, up(), b);
            assert_eq!(h.copy(&mut element, &node), "Alice:\nhello\n\nBob:\nworld");
            let edited = [("one", "Alice", "edited"), rows[1]];
            let mut element = fixture(
                if clear == "edit" {
                    &edited
                } else if clear == "removed" {
                    &rows[1..]
                } else {
                    &rows
                },
                if clear == "navigation" { 2 } else { 1 },
            );
            let node = h.layout(&mut element);
            if clear == "outside" {
                h.event(&mut element, &node, down(), Point::new(350.0, 250.0));
            }
            if clear == "keyboard" {
                h.event(
                    &mut element,
                    &node,
                    Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                        physical_key: iced::keyboard::key::Physical::Unidentified(
                            iced::keyboard::key::NativeCode::Unidentified,
                        ),
                        location: iced::keyboard::Location::Standard,
                        modifiers: iced::keyboard::Modifiers::empty(),
                        text: None,
                        repeat: false,
                    }),
                    Point::ORIGIN,
                );
            }
            h.clipboard.0 = "clipboard untouched".into();
            assert_eq!(h.copy(&mut element, &node), "clipboard untouched");
        }
    }
    #[test]
    fn edge_drag_scrolls_down_and_stops_at_end() {
        let rows = [
            ("one", "Alice", "first"),
            ("two", "Bob", "second"),
            ("three", "Carol", "third"),
            ("four", "Dave", "last"),
        ];
        let mut h = Harness::new();
        let mut element = fixture(&rows, 1);
        let node = h.layout(&mut element);
        let maximum = h.read(&mut element, &node).maximum;
        element.as_widget_mut().operate(
            &mut h.tree,
            Layout::new(&node),
            &h.renderer,
            &mut widget::operation::scrollable::scroll_to(
                widget::Id::new("room-timeline"),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(maximum),
                },
            ),
        );
        let data = h.read(&mut element, &node);
        let from = Point::new(0.0, data.rows[0].bounds.y + 5.0);
        h.event(&mut element, &node, down(), from);
        let bottom = Point::new(100.0, 199.0);
        h.event(&mut element, &node, moved(bottom), bottom);
        assert!(matches!(
            h.event(&mut element, &node, redraw(), bottom).0,
            iced::window::RedrawRequest::At(_)
        ));
        for _ in 0..40 {
            h.event(&mut element, &node, redraw(), bottom);
        }
        assert_eq!(
            h.event(&mut element, &node, redraw(), bottom).0,
            iced::window::RedrawRequest::Wait
        );
        h.event(&mut element, &node, up(), bottom);
        assert_eq!(
            h.copy(&mut element, &node),
            "Alice:\nfirst\n\nBob:\nsecond\n\nCarol:\nthird\n\nDave:\nlast"
        );
    }
    #[test]
    fn double_click_keeps_word_selection_through_layout_and_copy() {
        let rows = [("one", "Alice", "hello world")];
        let mut h = Harness::new();
        let mut element = fixture(&rows, 1);
        let node = h.layout(&mut element);
        let data = h.read(&mut element, &node);
        let point = Point::new(50.0, data.rows[0].bounds.y + 5.0);
        for _ in 0..2 {
            h.event(&mut element, &node, down(), point);
            h.event(&mut element, &node, redraw(), point);
            h.event(&mut element, &node, up(), point);
        }
        assert_eq!(h.copy(&mut element, &node), "world");
        let node = h.layout(&mut element);
        assert_eq!(h.copy(&mut element, &node), "world");
    }
}
