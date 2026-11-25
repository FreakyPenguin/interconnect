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
use interconnect::message_document::Document;
use std::{
    any::Any,
    borrow::Cow,
    cell::RefCell,
    collections::{BTreeSet, HashMap, VecDeque},
    rc::Rc,
    sync::{Arc, Weak},
    time::Duration,
};

/// Ordered logical text, independent of which rows currently have widgets.
/// Implementations retain lightweight projection indices, not shaped paragraphs.
/// `id` is the selectable body identity (normally `MessageRow::key`).
pub(super) trait Source: Send + Sync {
    fn index(&self, id: &str) -> Option<usize>;
    fn text(&self, index: usize) -> Option<Text<'_>>;
}
pub(super) struct Text<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub body: Cow<'a, str>,
    pub document: Option<&'a Arc<Document>>,
}
/// Model adapter for projected body rows. `indices` contains source row indices
/// in display order and excludes summaries/attachment-only rows without text.
pub(super) fn message_source(
    rows: Arc<Vec<interconnect::connected::MessageRow>>,
    indices: Vec<usize>,
) -> Arc<dyn Source> {
    struct Descriptor {
        source: usize,
        id: String,
        name: String,
        document: Option<Arc<Document>>,
        activity: bool,
    }
    struct Messages {
        rows: Weak<Vec<interconnect::connected::MessageRow>>,
        descriptors: Vec<Option<Descriptor>>,
        by_id: HashMap<String, usize>,
    }
    impl Source for Messages {
        fn index(&self, id: &str) -> Option<usize> {
            self.by_id.get(id).copied()
        }
        fn text(&self, index: usize) -> Option<Text<'_>> {
            let descriptor = self.descriptors.get(index)?.as_ref()?;
            // Tree state must not keep SDK handles alive or turn an in-place
            // append into a clone of every loaded MessageRow. Upgrade only for
            // the explicit selection/copy/reveal query and return owned text.
            let snapshot = self.rows.upgrade()?;
            let row = snapshot.get(descriptor.source)?;
            Some(Text {
                id: &descriptor.id,
                name: &descriptor.name,
                body: Cow::Owned(if descriptor.activity {
                    format!("{} · {}", row.display_name, row.body)
                } else {
                    row.body.clone()
                }),
                document: descriptor.document.as_ref(),
            })
        }
    }
    let descriptors: Vec<_> = indices
        .into_iter()
        .map(|source| {
            let row = rows.get(source)?;
            let activity = row.kind == interconnect::connected::RowKind::Activity;
            Some(Descriptor {
                source,
                id: if activity {
                    row.event_id.as_deref().unwrap_or(&row.body)
                } else {
                    &row.key
                }
                .to_owned(),
                name: if activity { "" } else { &row.display_name }.to_owned(),
                document: if activity {
                    None
                } else {
                    row.formatted.clone()
                },
                activity,
            })
        })
        .collect();
    let by_id = descriptors
        .iter()
        .enumerate()
        .filter_map(|(index, descriptor)| {
            descriptor
                .as_ref()
                .map(|descriptor| (descriptor.id.clone(), index))
        })
        .collect();
    Arc::new(Messages {
        rows: Arc::downgrade(&rows),
        descriptors,
        by_id,
    })
}

struct Semantic {
    document: Arc<Document>,
    revealed: BTreeSet<usize>,
    code_views: HashMap<usize, super::formatted::CodeView>,
}
struct Logical {
    name: String,
    body: String,
}

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
    suspended: bool,
    pub(super) original_offset: Option<f32>,
    pointer: Option<Point>,
    source: Option<Arc<dyn Source>>,
    semantic: HashMap<String, Semantic>,
}
pub(super) type Shared = Rc<RefCell<Selection>>;
#[derive(Clone, Default)]
struct Conversation {
    timeline: Shared,
    original: Shared,
}
/// Bounded session-only semantics, no widget trees or SDK handles.
#[derive(Default)]
pub(super) struct Conversations(RefCell<VecDeque<(super::DraftKey, Conversation)>>);
impl Conversations {
    fn state(&self, key: super::DraftKey) -> Conversation {
        let mut entries = self.0.borrow_mut();
        let state = entries
            .iter()
            .position(|(saved, _)| saved == &key)
            .and_then(|index| entries.remove(index))
            .map_or_else(Conversation::default, |(_, state)| state);
        entries.push_back((key, state.clone()));
        while entries.len() > 64 {
            entries.pop_front();
        }
        state
    }
    pub fn get(&self, key: super::DraftKey) -> Shared {
        self.state(key).timeline
    }
    pub fn original(&self, key: super::DraftKey) -> Shared {
        self.state(key).original
    }
    pub fn prefers_event_anchor(&self, key: &super::DraftKey) -> bool {
        self.0
            .borrow()
            .iter()
            .find(|(saved, _)| saved == key)
            .is_some_and(|(_, state)| {
                state
                    .timeline
                    .borrow()
                    .semantic
                    .values()
                    .any(|semantic| !semantic.code_views.is_empty())
            })
    }
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }
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
    original: bool,
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
        self.original = id == Some(&widget::Id::new("thread-original"));
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
fn same_document(a: &Arc<Document>, b: &Arc<Document>) -> bool {
    Arc::ptr_eq(a, b) || a.as_ref() == b.as_ref()
}
impl Selection {
    pub(super) fn suspend_drag(&mut self) {
        self.dragging = false;
        self.pointer = None;
        self.suspended = false;
    }
    pub(super) fn set_source(&mut self, source: Option<Arc<dyn Source>>) {
        if match (&self.source, &source) {
            (Some(previous), Some(current)) => Arc::ptr_eq(previous, current),
            (None, None) => true,
            _ => false,
        } {
            return;
        }
        self.source = source;
        // Drop state for removed/edited messages without materializing text.
        if let Some(source) = &self.source {
            self.semantic.retain(|id, semantic| {
                source
                    .index(id)
                    .and_then(|index| source.text(index))
                    .and_then(|text| text.document)
                    .is_some_and(|document| same_document(document, &semantic.document))
            });
        }
    }
    fn logical(&self, index: usize) -> Option<Logical> {
        let text = self.source.as_ref()?.text(index)?;
        let body = text.document.map_or_else(
            || text.body.into_owned(),
            |document| {
                let revealed = self
                    .semantic
                    .get(text.id)
                    .filter(|semantic| same_document(document, &semantic.document))
                    .map(|semantic| &semantic.revealed);
                document.text(revealed.unwrap_or(&BTreeSet::new()))
            },
        );
        Some(Logical {
            name: text.name.to_owned(),
            body,
        })
    }
    fn extent(&self) -> Option<(usize, usize, Position, Position)> {
        let source = self.source.as_ref()?;
        let (a, b) = self.range.as_ref()?;
        let ai = source.index(&a.id)?;
        let bi = source.index(&b.id)?;
        if self.logical(ai)?.body != a.body || self.logical(bi)?.body != b.body {
            return None;
        }
        Some(if (ai, order(a.position)) <= (bi, order(b.position)) {
            (ai, bi, a.position, b.position)
        } else {
            (bi, ai, b.position, a.position)
        })
    }
    /// Capture only semantic spoiler state before a mounted subtree is evicted.
    pub(super) fn capture_semantics(
        &mut self,
        content: &mut Content<'_>,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
    ) {
        struct Capture<'a>(&'a mut HashMap<String, Semantic>);
        impl widget::Operation for Capture<'_> {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn custom(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn Any) {
                let Some(state) = state.downcast_mut::<selectable::State>() else {
                    return;
                };
                let Some(rich) = &state.rich else {
                    return;
                };
                if self
                    .0
                    .get(&state.id)
                    .is_some_and(|saved| !same_document(&saved.document, &rich.document))
                {
                    self.0.remove(&state.id);
                }
                // Reveals are monotonic. A newly mounted, not-yet-restored widget
                // must not erase the saved nonempty state with its default state.
                if !rich.revealed().is_empty() || !rich.code_views.is_empty() {
                    self.0.insert(
                        state.id.clone(),
                        Semantic {
                            document: rich.document.clone(),
                            revealed: rich.revealed().clone(),
                            code_views: rich.code_views.clone(),
                        },
                    );
                }
            }
        }
        content.operate(tree, layout, renderer, &mut Capture(&mut self.semantic));
    }
    /// Returns true if restored spoilers changed text and require another layout.
    pub(super) fn restore_semantics(
        &self,
        content: &mut Content<'_>,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
    ) -> bool {
        struct Restore<'a> {
            semantic: &'a HashMap<String, Semantic>,
            changed: bool,
        }
        impl widget::Operation for Restore<'_> {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn custom(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn Any) {
                let Some(state) = state.downcast_mut::<selectable::State>() else {
                    return;
                };
                let Some(rich) = &mut state.rich else {
                    return;
                };
                if rich.semantics_restored {
                    return;
                }
                rich.semantics_restored = true;
                let Some(saved) = self.semantic.get(&state.id) else {
                    return;
                };
                if same_document(&saved.document, &rich.document) {
                    let revealed = rich.restore_revealed(&saved.revealed);
                    let code = rich.restore_code_views(&saved.code_views);
                    if revealed {
                        state.body = rich.text();
                        state.content = iced::widget::text_editor::Content::with_text(&state.body);
                    }
                    self.changed |= revealed || code;
                }
            }
        }
        let mut restore = Restore {
            semantic: &self.semantic,
            changed: false,
        };
        content.operate(tree, layout, renderer, &mut restore);
        restore.changed
    }
    fn ranges(&self, rows: &[Row]) -> Option<Vec<(String, Cursor)>> {
        if let Some(source) = &self.source {
            let (lo, hi, from, to) = self.extent()?;
            return Some(
                rows.iter()
                    .filter_map(|row| {
                        let index = source.index(&row.id)?;
                        (lo..=hi).contains(&index).then(|| {
                            (
                                row.id.clone(),
                                Cursor {
                                    selection: Some(if index == lo { from } else { start() }),
                                    position: if index == hi { to } else { end(&row.body) },
                                },
                            )
                        })
                    })
                    .collect(),
            );
        }
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
        self.capture_semantics(content, tree, layout, renderer);
        if self.range.is_none() {
            return;
        }
        if self.suspended {
            content.operate(tree, layout, renderer, &mut Apply { ranges: vec![] });
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
        if self.source.is_some() {
            let (lo, hi, from, to) = self.extent()?;
            let mut parts = Vec::new();
            for index in lo..=hi {
                let Some(row) = self.logical(index) else {
                    continue;
                };
                let cursor = Cursor {
                    selection: Some(if index == lo { from } else { start() }),
                    position: if index == hi { to } else { end(&row.body) },
                };
                if let Some(selected) = fragment(&row.body, cursor).filter(|text| !text.is_empty())
                {
                    parts.push(if lo != hi && !row.name.is_empty() {
                        format!("{}:\n{selected}", row.name.replace(['\r', '\n'], " "))
                    } else {
                        selected.to_owned()
                    });
                }
            }
            return (!parts.is_empty()).then(|| parts.join("\n\n"));
        }
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
        self.capture_semantics(content, tree, layout, renderer);
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
                && !self.suspended
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
                self.suspended = true;
                self.dragging = false;
                content.operate(tree, layout, renderer, &mut Apply { ranges: vec![] });
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
                        let offset = if data.original {
                            next
                        } else {
                            data.maximum - next
                        };
                        content.operate(tree, layout, renderer, &mut Scroll(offset));
                        scrolled = true;
                        if !data.original {
                            shell.publish(Message::Scrolled {
                                thread: data.thread,
                                generation,
                                offset,
                            });
                        }
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
        if pressed {
            struct Controls {
                point: Option<Point>,
                translation: Vector,
                hit: bool,
            }
            impl widget::Operation for Controls {
                fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                    f(self);
                }
                fn scrollable(
                    &mut self,
                    _: Option<&widget::Id>,
                    _: Rectangle,
                    _: Rectangle,
                    translation: Vector,
                    _: &mut dyn widget::operation::Scrollable,
                ) {
                    self.translation = translation;
                }
                fn custom(
                    &mut self,
                    _: Option<&widget::Id>,
                    bounds: Rectangle,
                    state: &mut dyn Any,
                ) {
                    if let Some(state) = state.downcast_ref::<selectable::State>()
                        && let Some(rich) = &state.rich
                        && let Some(point) = self.point
                        && (bounds - self.translation).contains(point)
                    {
                        self.hit |= rich
                            .code_control(
                                Point::ORIGIN + (point - (bounds - self.translation).position()),
                            )
                            .is_some();
                    }
                }
            }
            let data = read(content, tree, layout, renderer);
            let mut controls = Controls {
                point: cursor
                    .position()
                    .filter(|point| data.viewport.contains(*point)),
                translation: Vector::ZERO,
                hit: false,
            };
            content.operate(tree, layout, renderer, &mut controls);
            if controls.hit {
                content.update(
                    tree, event, layout, cursor, renderer, clipboard, shell, viewport,
                );
                return;
            }
        }
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
            self.suspended = false;
            if hit.is_none() {
                // A sidebar or thread-close press transfers focus before its
                // navigation message arrives. Keep the range for the saved
                // conversation, while yielding Copy to the newly focused view.
                self.suspended = true;
                self.dragging = false;
                content.operate(tree, layout, renderer, &mut Apply { ranges: vec![] });
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
            && !self.suspended
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
            self.layout_size(element, Size::new(300.0, 200.0))
        }
        fn layout_size(&mut self, element: &mut Element<'_, Message>, size: Size) -> layout::Node {
            self.tree.diff(element.as_widget());
            element.as_widget_mut().layout(
                &mut self.tree,
                &self.renderer,
                &layout::Limits::new(Size::ZERO, size),
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
    struct ModelText {
        id: String,
        name: String,
        body: String,
        document: Option<Arc<Document>>,
    }
    struct Model(Vec<ModelText>);
    impl Source for Model {
        fn index(&self, id: &str) -> Option<usize> {
            self.0.iter().position(|row| row.id == id)
        }
        fn text(&self, index: usize) -> Option<Text<'_>> {
            let row = self.0.get(index)?;
            Some(Text {
                id: &row.id,
                name: &row.name,
                body: Cow::Borrowed(&row.body),
                document: row.document.as_ref(),
            })
        }
    }
    fn logical_endpoint(id: &str, body: &str, position: Position) -> Endpoint {
        Endpoint {
            id: id.into(),
            body: body.into(),
            position,
        }
    }

    fn code_view<'a>(
        rows: &'a Arc<Vec<interconnect::connected::MessageRow>>,
        shared: Shared,
        generation: u64,
    ) -> Element<'a, Message> {
        let source = message_source(rows.clone(), vec![0]);
        super::super::timeline::virtual_history_view(
            generation,
            Arc::new(
                [(super::super::timeline::row_id(0), "$code".into())]
                    .into_iter()
                    .collect(),
            ),
            scrollable(
                container(selectable::message_view(&rows[0])).id(super::super::timeline::row_id(0)),
            )
            .id("room-timeline")
            .anchor_bottom()
            .height(iced::Length::Fill)
            .into(),
            (false, 1, true),
            true,
            None,
            None,
            Default::default(),
            None,
            source,
            shared,
        )
    }
    fn code_toggle_point(
        h: &mut Harness,
        element: &mut Element<'_, Message>,
        node: &layout::Node,
    ) -> Point {
        #[derive(Default)]
        struct Find {
            point: Option<Point>,
            translation: Vector,
        }
        impl widget::Operation for Find {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                _: Option<&widget::Id>,
                _: Rectangle,
                _: Rectangle,
                translation: Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                self.translation = translation;
            }
            fn custom(&mut self, _: Option<&widget::Id>, bounds: Rectangle, state: &mut dyn Any) {
                if let Some(state) = state.downcast_ref::<selectable::State>()
                    && let Some(button) = state.rich.as_ref().and_then(|rich| {
                        rich.code_button(0, super::super::formatted::CodeAction::Toggle)
                    })
                {
                    self.point = Some(
                        bounds.position() + (button.center() - Point::ORIGIN) - self.translation,
                    );
                }
            }
        }
        let mut find = Find::default();
        element
            .as_widget_mut()
            .operate(&mut h.tree, Layout::new(node), &h.renderer, &mut find);
        find.point.expect("code toggle icon")
    }
    #[test]
    fn code_scrollbar_drag_and_bottom_expand_preserve_selection_and_settle_without_timers() {
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
        row.key = "code".into();
        row.event_id = Some("$code".into());
        let body = "let café = long_identifier(); ".repeat(200);
        row.formatted = Some(Arc::new(
            Document::parse(&format!("<pre><code>{body}</code></pre>")).unwrap(),
        ));
        let rows = Arc::new(vec![row]);
        let shared = Shared::default();
        let mut h = Harness::new();
        let mut element = code_view(&rows, shared.clone(), 1);
        h.layout(&mut element);
        shared.borrow_mut().range = Some((
            logical_endpoint("code", &body, start()),
            logical_endpoint("code", &body, end(&body)),
        ));
        let node = h.layout(&mut element);
        #[derive(Default)]
        struct Bar(Option<super::super::formatted::CodeScrollbar>, bool);
        impl widget::Operation for Bar {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn custom(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn Any) {
                if let Some(state) = state.downcast_ref::<selectable::State>()
                    && let Some(rich) = &state.rich
                {
                    self.0 = rich.code_scrollbar(0);
                    self.1 = rich.dragging_code();
                }
            }
        }
        let geometry = |h: &mut Harness,
                        element: &mut Element<'_, Message>,
                        node: &layout::Node| {
            let mut bar = Bar::default();
            element
                .as_widget_mut()
                .operate(&mut h.tree, Layout::new(node), &h.renderer, &mut bar);
            bar
        };
        let initial = geometry(&mut h, &mut element, &node).0.unwrap();
        h.event(&mut element, &node, down(), initial.thumb.center());
        let node = h.layout(&mut element);
        let outside = Point::new(500.0, initial.track.center_y());
        h.event(&mut element, &node, moved(outside), outside);
        let dragged = geometry(&mut h, &mut element, &node);
        let right = dragged.0.unwrap();
        assert!(dragged.1);
        assert!(
            (right.thumb.x + right.thumb.width - right.track.x - right.track.width).abs() < 0.1
        );
        h.event(&mut element, &node, up(), outside);
        assert!(!geometry(&mut h, &mut element, &node).1);
        assert_eq!(h.copy(&mut element, &node), body);
        // The overlay covers the last preview rows, immediately above the bar.
        let expand = Point::new(initial.track.center_x(), initial.track.y - 16.0);
        h.event(&mut element, &node, down(), expand);
        let node = h.layout(&mut element);
        assert!(shared.borrow().semantic["code"].code_views[&0].expanded);
        assert_eq!(h.copy(&mut element, &node), body);
        // Losing window focus cancels the scrollbar drag without a timer.
        let bar = geometry(&mut h, &mut element, &node).0.unwrap();
        let press = Point::new(bar.track.center_x(), bar.track.center_y());
        h.event(&mut element, &node, down(), press);
        h.event(
            &mut element,
            &node,
            Event::Window(iced::window::Event::Unfocused),
            press,
        );
        let cancelled = geometry(&mut h, &mut element, &node).0.unwrap();
        h.event(&mut element, &node, moved(outside), outside);
        assert_eq!(
            geometry(&mut h, &mut element, &node).0.unwrap().thumb,
            cancelled.thumb
        );
        assert!(!geometry(&mut h, &mut element, &node).1);
        let (request, _) = h.event(&mut element, &node, redraw(), Point::ORIGIN);
        assert!(!matches!(request, iced::window::RedrawRequest::At(_)));
    }
    #[test]
    fn pinned_thread_original_restores_selection_code_state_and_its_own_scroll_after_loading() {
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
        row.key = "original".into();
        row.formatted = Some(Arc::new(
            Document::parse(&format!(
                "<pre><code>{}</code></pre>",
                "let café = 42; // 👩🏽‍💻\n".repeat(50)
            ))
            .unwrap(),
        ));
        let body = row.formatted.as_ref().unwrap().text(&BTreeSet::new());
        let cache = Conversations::default();
        let key = crate::DraftKey {
            room: "studio".into(),
            root: Some("$original".into()),
        };
        let shared = cache.original(key.clone());
        let build = |generation| {
            super::super::timeline::retained_original(
                generation,
                scrollable(selectable::message_view(&row))
                    .id("thread-original")
                    .height(iced::Length::Fill)
                    .into(),
                cache.original(key.clone()),
                true,
            )
        };
        let mut h = Harness::new();
        let mut original = build(1);
        let node = h.layout(&mut original);
        let toggle = code_toggle_point(&mut h, &mut original, &node);
        h.event(&mut original, &node, down(), toggle);
        h.layout(&mut original);
        shared.borrow_mut().range = Some((
            logical_endpoint("original", &body, start()),
            logical_endpoint("original", &body, end(&body)),
        ));
        let node = h.layout(&mut original);
        original.as_widget_mut().operate(
            &mut h.tree,
            Layout::new(&node),
            &h.renderer,
            &mut widget::operation::scrollable::scroll_to::<()>(
                "thread-original".into(),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(75.0),
                },
            ),
        );
        assert_eq!(shared.borrow().original_offset, Some(75.0));
        assert_eq!(h.copy(&mut original, &node), body);
        h.event(&mut original, &node, down(), Point::new(350.0, 250.0));
        assert!(
            shared.borrow().range.is_some(),
            "a thread-close press must retain its logical selection"
        );
        assert!(shared.borrow().suspended);
        drop(original);
        let mut loading = super::super::timeline::retained_original(
            2,
            scrollable(text("Loading original…"))
                .id("thread-original")
                .height(iced::Length::Fill)
                .into(),
            shared.clone(),
            false,
        );
        h.layout(&mut loading);
        assert!(shared.borrow().range.is_some());
        assert_eq!(shared.borrow().original_offset, Some(75.0));
        drop(loading);
        let mut reopened = build(3);
        let node = h.layout_size(&mut reopened, Size::new(260.0, 200.0));
        let read = h.read(&mut reopened, &node);
        assert_eq!(read.translation.y, 75.0);
        assert!(read.maximum > 500.0);
        assert_eq!(h.copy(&mut reopened, &node), body);
        assert!(shared.borrow().semantic["original"].code_views[&0].expanded);
        assert!(
            cache.get(key).borrow().range.is_none(),
            "original and reply selections are independent"
        );
    }
    #[test]
    fn code_controls_keep_selection_and_expansion_anchor_and_restore_across_conversations() {
        for keyboard in [false, true] {
            check_code_selection_and_anchor(keyboard);
        }
    }
    fn check_code_selection_and_anchor(keyboard: bool) {
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
        row.key = "code".into();
        row.event_id = Some("$code".into());
        row.formatted = Some(Arc::new(
            Document::parse(&format!(
                "<pre><code>{}</code></pre>",
                "let café = 42; // 👩🏽‍💻\n".repeat(50)
            ))
            .unwrap(),
        ));
        let body = row.formatted.as_ref().unwrap().text(&BTreeSet::new());
        let rows = Arc::new(vec![row]);
        let conversations = Conversations::default();
        let room = crate::DraftKey {
            room: "technical".into(),
            root: None,
        };
        let shared = conversations.get(room.clone());
        let mut h = Harness::new();
        let mut element = code_view(&rows, shared.clone(), 1);
        let size = Size::new(320.0, 400.0);
        h.layout_size(&mut element, size);
        shared.borrow_mut().range = Some((
            logical_endpoint("code", &body, start()),
            logical_endpoint("code", &body, end(&body)),
        ));
        let node = h.layout_size(&mut element, size);
        let before = h.read(&mut element, &node).rows[0].bounds.y;
        if keyboard {
            let mut operation: Box<dyn widget::Operation> =
                Box::new(widget::operation::focusable::focus_next::<()>());
            loop {
                element.as_widget_mut().operate(
                    &mut h.tree,
                    Layout::new(&node),
                    &h.renderer,
                    operation.as_mut(),
                );
                if let widget::operation::Outcome::Chain(next) = operation.finish() {
                    operation = next;
                } else {
                    break;
                }
            }
            h.event(
                &mut element,
                &node,
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                    modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
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
        } else {
            let toggle = code_toggle_point(&mut h, &mut element, &node);
            h.event(&mut element, &node, down(), toggle);
        }
        let node = h.layout_size(&mut element, size);
        assert!((h.read(&mut element, &node).rows[0].bounds.y - before).abs() <= 1.0);
        assert_eq!(h.copy(&mut element, &node), body);
        assert!(shared.borrow().semantic["code"].code_views[&0].expanded);
        assert!(conversations.prefers_event_anchor(&room));
        let (request, reports) = h.event(&mut element, &node, redraw(), Point::ORIGIN);
        assert!(!matches!(request, iced::window::RedrawRequest::At(_)));
        assert!(
            reports.iter().any(
                |message| matches!(message, Message::Scrolled { offset, .. } if *offset > 100.0)
            )
        );
        let (_, settled) = h.event(&mut element, &node, redraw(), Point::ORIGIN);
        assert!(
            !settled
                .iter()
                .any(|message| matches!(message, Message::Scrolled { .. }))
        );
        h.event(&mut element, &node, down(), Point::new(350.0, 250.0));
        assert!(
            shared.borrow().range.is_some(),
            "sidebar navigation preserves the saved range"
        );
        drop(element);
        // A different conversation and a reopened thread get independent state.
        let other = conversations.get(crate::DraftKey {
            room: "technical".into(),
            root: Some("$thread".into()),
        });
        assert!(other.borrow().range.is_none());
        let mut other_view = code_view(&rows, other, 2);
        h.layout_size(&mut other_view, size);
        drop(other_view);
        let mut restored = code_view(&rows, conversations.get(room), 3);
        let node = h.layout_size(&mut restored, Size::new(260.0, 400.0));
        assert_eq!(h.copy(&mut restored, &node), body);
        assert!(
            h.read(&mut restored, &node).maximum > 500.0,
            "expanded layout must survive room navigation and resizing"
        );
        let maximum = h.read(&mut restored, &node).maximum;
        restored.as_widget_mut().operate(
            &mut h.tree,
            Layout::new(&node),
            &h.renderer,
            &mut widget::operation::scrollable::scroll_to::<()>(
                "room-timeline".into(),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(maximum),
                },
            ),
        );
        let node = h.layout_size(&mut restored, Size::new(260.0, 400.0));
        let toggle = code_toggle_point(&mut h, &mut restored, &node);
        h.event(&mut restored, &node, down(), toggle);
        let node = h.layout_size(&mut restored, size);
        assert!(!shared.borrow().semantic["code"].code_views[&0].expanded);
        assert!(
            h.read(&mut restored, &node).maximum < 10.0,
            "collapse removes expanded layout work"
        );
        assert_eq!(h.copy(&mut restored, &node), body);
    }
    #[test]
    fn conversation_text_state_is_bounded_and_cleared_explicitly() {
        let cache = Conversations::default();
        for i in 0..100 {
            cache.get(crate::DraftKey {
                room: i.to_string(),
                root: None,
            });
        }
        assert_eq!(cache.0.borrow().len(), 64);
        cache.clear();
        assert!(cache.0.borrow().is_empty());
    }
    #[test]
    fn logical_selection_survives_both_endpoints_unmounted_and_copies_intermediate_canonical_text()
    {
        let document = Arc::new(Document::parse("<p>café <img src='mxc://example.org/a' alt='diagram'> <span data-mx-spoiler>private</span> <span data-mx-maths='x^2'>x squared</span></p>").unwrap());
        let middle = document.text(&BTreeSet::new());
        assert!(!middle.contains("private"));
        assert!(middle.contains("diagram"));
        for reverse in [false, true] {
            let mut selection = Selection::default();
            selection.set_source(Some(Arc::new(Model(vec![
                ModelText {
                    id: "a".into(),
                    name: "Alice".into(),
                    body: "start\r\n  keep\tspace".into(),
                    document: None,
                },
                ModelText {
                    id: "middle".into(),
                    name: "Maya".into(),
                    body: "fallback must never be copied".into(),
                    document: Some(document.clone()),
                },
                ModelText {
                    id: "b".into(),
                    name: "Bob".into(),
                    body: "ending".into(),
                    document: None,
                },
            ]))));
            let a = logical_endpoint("a", "start\r\n  keep\tspace", start());
            let b = logical_endpoint("b", "ending", end("ending"));
            selection.range = Some(if reverse { (b, a) } else { (a, b) });
            let mut h = Harness::new();
            let mut element: Element<'_, Message> =
                scrollable(selectable::named_view("middle", &middle, "Maya"))
                    .id("room-timeline")
                    .height(200)
                    .into();
            let node = h.layout(&mut element);
            selection.restore(
                element.as_widget_mut(),
                &mut h.tree,
                Layout::new(&node),
                &h.renderer,
            );
            assert!(selection.range.is_some());
            let text = selection
                .copy(
                    element.as_widget_mut(),
                    &mut h.tree,
                    Layout::new(&node),
                    &h.renderer,
                )
                .unwrap();
            assert_eq!(
                text,
                format!("Alice:\nstart\r\n  keep\tspace\n\nMaya:\n{middle}\n\nBob:\nending")
            );
            let mounted = h.read(&mut element, &node);
            assert_eq!(mounted.rows.len(), 1);
            assert_eq!(mounted.rows[0].cursor.selection, Some(start()));
            assert_eq!(mounted.rows[0].cursor.position, end(&middle));
            // A real source edit still invalidates the old logical range.
            selection.set_source(Some(Arc::new(Model(vec![
                ModelText {
                    id: "a".into(),
                    name: "Alice".into(),
                    body: "edited".into(),
                    document: None,
                },
                ModelText {
                    id: "b".into(),
                    name: "Bob".into(),
                    body: "ending".into(),
                    document: None,
                },
            ]))));
            selection.restore(
                element.as_widget_mut(),
                &mut h.tree,
                Layout::new(&node),
                &h.renderer,
            );
            assert!(selection.range.is_none());
        }
    }

    #[test]
    fn message_source_does_not_retain_loaded_rows_or_force_copy_on_append() {
        let row = crate::demo::Demo::default()
            .pane(
                crate::DraftKey {
                    room: "studio".into(),
                    root: None,
                },
                1,
            )
            .rows[0]
            .clone();
        let mut loaded = Arc::new(vec![row.clone()]);
        let source = message_source(loaded.clone(), vec![0]);
        assert_eq!(Arc::strong_count(&loaded), 1);
        assert_eq!(source.text(0).unwrap().body, row.body);
        assert_eq!(
            Arc::strong_count(&loaded),
            1,
            "text queries only upgrade temporarily"
        );
        let first_body_allocation = loaded[0].body.as_ptr();
        Arc::make_mut(&mut loaded).push(row);
        assert_eq!(
            loaded[0].body.as_ptr(),
            first_body_allocation,
            "appending must not clone the existing row contents"
        );
        assert!(
            source.text(0).is_none(),
            "an old weak source becomes unavailable until the next layout source refresh"
        );
        let source = message_source(loaded.clone(), vec![0]);
        assert!(source.text(0).is_some());
        drop(loaded);
        assert!(
            source.text(0).is_none(),
            "widget state cannot retain an abandoned snapshot"
        );
    }

    #[test]
    fn spoiler_semantics_survive_widget_eviction_but_do_not_cross_document_revisions() {
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
        row.key = "rich".into();
        row.formatted = Some(Arc::new(
            Document::parse("before <span data-mx-spoiler>revealed secret</span> after").unwrap(),
        ));
        let mut selection = Selection::default();
        let loaded_rows = Arc::new(vec![row.clone()]);
        selection.set_source(Some(message_source(loaded_rows.clone(), vec![0])));
        let mut h = Harness::new();
        let mut element = selectable::message_view(&row);
        let node = h.layout(&mut element);
        struct Reveal;
        impl widget::Operation for Reveal {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn custom(&mut self, _: Option<&widget::Id>, _: Rectangle, state: &mut dyn Any) {
                let Some(state) = state.downcast_mut::<selectable::State>() else {
                    return;
                };
                let rich = state.rich.as_mut().unwrap();
                rich.restore_revealed(&BTreeSet::from([1]));
                state.body = rich.text();
                state.content = iced::widget::text_editor::Content::with_text(&state.body);
            }
        }
        element
            .as_widget_mut()
            .operate(&mut h.tree, Layout::new(&node), &h.renderer, &mut Reveal);
        selection.capture_semantics(
            element.as_widget_mut(),
            &mut h.tree,
            Layout::new(&node),
            &h.renderer,
        );
        assert!(
            selection
                .logical(0)
                .unwrap()
                .body
                .contains("revealed secret")
        );
        drop(element);
        h.tree = Tree::empty(); // A bounded virtual list evicts the heavy tree.
        let mut remounted = selectable::message_view(&row);
        let node = h.layout(&mut remounted);
        assert!(
            !h.read(&mut remounted, &node).rows[0]
                .body
                .contains("revealed secret")
        );
        assert!(selection.restore_semantics(
            remounted.as_widget_mut(),
            &mut h.tree,
            Layout::new(&node),
            &h.renderer
        ));
        let node = h.layout(&mut remounted);
        assert!(
            h.read(&mut remounted, &node).rows[0]
                .body
                .contains("revealed secret")
        );
        assert!(!selection.restore_semantics(
            remounted.as_widget_mut(),
            &mut h.tree,
            Layout::new(&node),
            &h.renderer
        ));
        drop(remounted);
        row.formatted = Some(Arc::new(
            Document::parse("before <span data-mx-spoiler>different secret</span> after").unwrap(),
        ));
        let edited_rows = Arc::new(vec![row.clone()]);
        selection.set_source(Some(message_source(edited_rows.clone(), vec![0])));
        let mut edited = selectable::message_view(&row);
        let node = h.layout(&mut edited);
        assert!(!selection.restore_semantics(
            edited.as_widget_mut(),
            &mut h.tree,
            Layout::new(&node),
            &h.renderer
        ));
        assert!(
            !selection
                .logical(0)
                .unwrap()
                .body
                .contains("different secret")
        );
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
                        interconnect::settings::ThemeMode::Light,
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
        for kind in 0..3 {
            let plain = kind == 1;
            let mention = kind == 2;
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
            if mention {
                row.formatted = Some(std::sync::Arc::new(
                    interconnect::message_document::Document::parse(
                        "<a href='https://matrix.to/#/@maya:example.org'>a long mention label for selecting</a>"
                    ).unwrap(),
                ));
            }
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
                    .any(|m| if mention {
                        matches!(m, Message::NewChat(super::super::new_chat::Event::Mention(user)) if user.as_str() == "@maya:example.org")
                    } else {
                        matches!(m, Message::OpenMessageLink(url) if url == destination)
                    })
            );
            let end = point + iced::Vector::new(90.0, 0.0);
            h.event(&mut element, &node, down(), point);
            h.event(&mut element, &node, moved(end), end);
            let (_, messages) = h.event(&mut element, &node, up(), end);
            assert!(
                !messages
                    .iter()
                    .any(|m| matches!(m, Message::OpenMessageLink(_) | Message::NewChat(_)))
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
