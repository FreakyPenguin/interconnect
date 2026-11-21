//! Variable-height rows. Only an overscanned viewport owns heavy widget trees.
use super::Message;
use iced::{
    Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree},
    },
};
use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc, sync::Arc};

#[derive(Default)]
pub(super) struct Viewport {
    pub offset: f32,
    pub height: f32,
    pub anchor: Option<(String, f32)>,
    pub target: Option<usize>,
    start: f32,
    end: f32,
    total: f32,
}
impl Viewport {
    pub fn needs_layout(&self) -> bool {
        let top = (self.total - self.height - self.offset).max(0.0);
        (self.start > 0.0 && top < self.start + self.height * 0.5)
            || (self.end < self.total - 1.0 && top + self.height > self.end - self.height * 0.5)
    }
}
pub(super) type SharedViewport = Rc<RefCell<Viewport>>;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Stats {
    pub mounted: usize,
    pub loaded: usize,
}

/// Prefix sums support updates and pixel-to-row lookup without scanning history.
#[derive(Default)]
struct Heights {
    values: Vec<f32>,
    sums: Vec<f32>,
}
impl Heights {
    fn new(values: Vec<f32>) -> Self {
        let values: Vec<_> = values.into_iter().map(|value| value.max(1.0)).collect();
        let mut sums = vec![0.0; values.len() + 1];
        for index in 1..sums.len() {
            sums[index] += values[index - 1];
            let parent = index + (index & index.wrapping_neg());
            if parent < sums.len() {
                sums[parent] += sums[index];
            }
        }
        Self { sums, values }
    }
    fn set(&mut self, index: usize, value: f32) {
        let value = value.max(1.0);
        let delta = value - self.values[index];
        self.values[index] = value;
        let mut i = index + 1;
        while i < self.sums.len() {
            self.sums[i] += delta;
            i += i & i.wrapping_neg();
        }
    }
    fn prefix(&self, mut count: usize) -> f32 {
        let mut sum = 0.0;
        while count > 0 {
            sum += self.sums[count];
            count &= count - 1;
        }
        sum
    }
    fn total(&self) -> f32 {
        self.prefix(self.values.len())
    }
    fn at(&self, y: f32) -> usize {
        let mut index = 0;
        let mut sum = 0.0;
        let mut bit = self.values.len().next_power_of_two();
        while bit != 0 {
            let next = index + bit;
            if next < self.sums.len() && sum + self.sums[next] <= y {
                sum += self.sums[next];
                index = next;
            }
            bit >>= 1;
        }
        index.min(self.values.len().saturating_sub(1))
    }
}
#[derive(Default)]
struct State {
    keys: Arc<Vec<String>>,
    indices: HashMap<String, usize>,
    heights: Heights,
    mounted: Vec<String>,
}
impl State {
    fn sync(&mut self, keys: &Arc<Vec<String>>) {
        if Arc::ptr_eq(&self.keys, keys) {
            return;
        }
        let heights = keys
            .iter()
            .map(|key| {
                self.indices
                    .get(key)
                    .map_or(112.0, |i| self.heights.values[*i])
            })
            .collect();
        self.heights = Heights::new(heights);
        self.indices = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (key.clone(), index))
            .collect();
        self.keys = keys.clone();
    }
    fn top(&self, view: &Viewport) -> f32 {
        let height = view.height.max(100.0);
        if let Some(target) = view.target {
            self.heights.prefix(target) - height * 0.5
        } else if let Some((key, offset)) = &view.anchor {
            self.indices
                .get(key)
                .map_or(self.heights.total() - height - view.offset, |index| {
                    self.heights.prefix(*index) - offset
                })
        } else {
            self.heights.total() - height - view.offset
        }
        .max(0.0)
    }
    fn desired_range(&self, view: &Viewport) -> Range<usize> {
        if self.keys.len() <= 192 {
            return 0..self.keys.len();
        }
        let height = view.height.max(100.0);
        let top = self.top(view);
        self.heights.at((top - 2.0 * height).max(0.0))
            ..(self.heights.at(top + 3.0 * height) + 1).min(self.keys.len())
    }
    fn range(&self, view: &Viewport) -> Range<usize> {
        if self.keys.len() <= 192 {
            return 0..self.keys.len();
        }
        if view.target.is_none()
            && let (Some(first), Some(last)) = (
                self.mounted.first().and_then(|key| self.indices.get(key)),
                self.mounted.last().and_then(|key| self.indices.get(key)),
            )
        {
            let height = view.height.max(100.0);
            let top = self.top(view);
            let start = self.heights.prefix(*first);
            let end = self.heights.prefix(*last + 1);
            let newest_mounted = view.offset > 1.0 || *last + 1 == self.keys.len();
            if last >= first
                && *last + 1 - *first == self.mounted.len()
                && newest_mounted
                && (*first == 0 || top >= start + height * 0.5)
                && (*last + 1 == self.keys.len() || top + height <= end - height * 0.5)
                && end - start <= height * 8.0
            {
                return *first..*last + 1;
            }
        }
        self.desired_range(view)
    }
}

pub(super) fn view<'a>(
    keys: Arc<Vec<String>>,
    viewport: SharedViewport,
    row: impl Fn(usize) -> Element<'a, Message> + 'a,
) -> Element<'a, Message> {
    Element::new(Rows {
        keys,
        viewport,
        row: Box::new(row),
        children: Vec::new(),
    })
}
struct Rows<'a> {
    keys: Arc<Vec<String>>,
    viewport: SharedViewport,
    row: Box<dyn Fn(usize) -> Element<'a, Message> + 'a>,
    children: Vec<Element<'a, Message>>,
}
impl Widget<Message, Theme, Renderer> for Rows<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn diff(&self, _: &mut Tree) {} // Keyed reconciliation happens in layout.
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        tree.state.downcast_mut::<State>().sync(&self.keys);
        let mut nodes = Vec::new();
        let mut range = tree
            .state
            .downcast_ref::<State>()
            .range(&self.viewport.borrow());
        for _ in 0..12 {
            let state = tree.state.downcast_mut::<State>();
            let mut old: HashMap<_, _> = std::mem::take(&mut state.mounted)
                .into_iter()
                .zip(std::mem::take(&mut tree.children))
                .collect();
            self.children.clear();
            nodes.clear();
            let mut changed = false;
            for index in range.clone() {
                let mut row = (self.row)(index);
                let key = &self.keys[index];
                let mut child = old.remove(key).unwrap_or_else(|| Tree::new(&row));
                child.diff(row.as_widget());
                let node = row.as_widget_mut().layout(
                    &mut child,
                    renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(limits.max().width, f32::INFINITY)),
                );
                changed |= (state.heights.values[index] - node.size().height).abs() > 0.5;
                state.heights.set(index, node.size().height);
                state.mounted.push(key.clone());
                self.children.push(row);
                tree.children.push(child);
                nodes.push(node);
            }
            let next = if changed {
                state.desired_range(&self.viewport.borrow())
            } else {
                state.range(&self.viewport.borrow())
            };
            if next == range {
                break;
            }
            range = next;
        }
        let state = tree.state.downcast_ref::<State>();
        // The final range is represented by mounted identities even if the
        // bounded measurement passes did not completely fill the overscan yet.
        let first = state
            .mounted
            .first()
            .and_then(|key| state.indices.get(key))
            .copied()
            .unwrap_or(0);
        let mut y = state.heights.prefix(first);
        for node in &mut nodes {
            node.move_to_mut(Point::new(0.0, y));
            y += node.size().height;
        }
        let mut view = self.viewport.borrow_mut();
        view.start = state.heights.prefix(first);
        view.end = y;
        view.total = state.heights.total();
        layout::Node::with_children(Size::new(limits.max().width, state.heights.total()), nodes)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        #[cfg(test)]
        operation.custom(
            None,
            layout.bounds(),
            &mut Stats {
                mounted: tree.children.len(),
                loaded: self.keys.len(),
            },
        );
        operation.traverse(&mut |operation| {
            for ((child, tree), node) in self
                .children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                child
                    .as_widget_mut()
                    .operate(tree, node, renderer, operation);
            }
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
        for ((child, tree), node) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                tree, event, node, cursor, renderer, clipboard, shell, viewport,
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
        for ((child, tree), node) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            if node.bounds().intersects(viewport) {
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, node, cursor, viewport);
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
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, tree), node)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, node, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
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
    fn prefix_index_matches_updates_and_pixel_boundaries() {
        let mut heights = Heights::new(vec![10.0, 20.0, 30.0]);
        assert_eq!(heights.total(), 60.0);
        assert_eq!(
            (
                heights.at(0.0),
                heights.at(10.0),
                heights.at(29.9),
                heights.at(60.0)
            ),
            (0, 1, 1, 2)
        );
        heights.set(1, 40.0);
        assert_eq!(heights.prefix(2), 50.0);
        assert_eq!(heights.at(49.0), 1);
        assert_eq!(heights.at(50.0), 2);
    }

    #[test]
    fn mounting_and_layout_are_bounded_and_reuse_rows_by_key() {
        use std::cell::Cell;
        let keys = Arc::new((0..10_000).map(|i| format!("row-{i}")).collect());
        let viewport = Rc::new(RefCell::new(Viewport {
            height: 400.0,
            ..Default::default()
        }));
        let calls = Rc::new(Cell::new(0));
        let built = calls.clone();
        let mut rows = view(keys, viewport.clone(), move |i| {
            built.set(built.get() + 1);
            iced::widget::container(iced::widget::space().height(40))
                .id(format!("test-row-{i}"))
                .into()
        });
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&rows);
        let limits = layout::Limits::new(Size::ZERO, Size::new(600.0, f32::INFINITY));
        rows.as_widget_mut().layout(&mut tree, &renderer, &limits);
        assert!(tree.children.len() < 64);
        assert!(
            calls.get() < 256,
            "bounded convergence built {} rows",
            calls.get()
        );
        let previous = tree.state.downcast_ref::<State>().mounted.clone();
        calls.set(0);
        viewport.borrow_mut().offset = 100.0;
        rows.as_widget_mut().layout(&mut tree, &renderer, &limits);
        assert_eq!(previous, tree.state.downcast_ref::<State>().mounted);
        assert_eq!(calls.get(), tree.children.len());
        viewport.borrow_mut().target = Some(5_000);
        rows.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let state = tree.state.downcast_ref::<State>();
        assert!(state.mounted.iter().any(|key| key == "row-5000"));
        assert!(tree.children.len() < 80);
        assert!(!state.mounted.iter().any(|key| previous.contains(key)));
    }

    #[test]
    fn row_anchor_survives_prepending_and_height_corrections() {
        let mut state = State::default();
        let keys = Arc::new((0..1000).map(|i| format!("row-{i}")).collect());
        state.sync(&keys);
        state.heights.set(499, 400.0);
        let viewport = Viewport {
            height: 400.0,
            anchor: Some(("row-500".into(), -10.0)),
            ..Default::default()
        };
        assert!(state.range(&viewport).contains(&500));
        let mut prepended: Vec<_> = (0..200).map(|i| format!("older-{i}")).collect();
        prepended.extend(keys.iter().cloned());
        state.sync(&Arc::new(prepended));
        assert!(state.range(&viewport).contains(&700));
        assert_eq!(state.heights.values[699], 400.0);
    }

    #[test]
    fn native_scroll_targets_and_event_anchors_work_with_unmounted_history() {
        use iced::widget::{container, scrollable, space};
        #[derive(Default)]
        struct Probe {
            metrics: super::super::timeline::Metrics,
            origin: f32,
            rows: Vec<(String, f32)>,
            keys: HashMap<widget::Id, String>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                _: Option<&widget::Id>,
                bounds: Rectangle,
                content: Rectangle,
                translation: Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                self.origin = bounds.y;
                self.metrics = super::super::timeline::Metrics {
                    height: bounds.height,
                    content_height: content.height,
                    translation: translation.y,
                };
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                if let Some(key) = id.and_then(|id| self.keys.get(id)) {
                    let y = bounds.y - self.origin - self.metrics.translation;
                    if y + bounds.height > 0.0 && y < self.metrics.height {
                        self.rows.push((key.clone(), y));
                    }
                }
            }
        }
        fn build(
            keys: Arc<Vec<String>>,
            target: Option<usize>,
            row_height: f32,
        ) -> (Element<'static, Message>, HashMap<widget::Id, String>) {
            let mapping: HashMap<_, _> = keys
                .iter()
                .enumerate()
                .map(|(i, key)| (super::super::timeline::row_id(i), key.clone()))
                .collect();
            let viewport = Rc::new(RefCell::new(Viewport::default()));
            let rows = view(keys.clone(), viewport.clone(), move |index| {
                container(space().height(row_height))
                    .id(super::super::timeline::row_id(index))
                    .into()
            });
            let content = scrollable(rows)
                .id("room-timeline")
                .anchor_bottom()
                .height(Length::Fill);
            let widget = super::super::timeline::virtual_history_view(
                1,
                Arc::new(mapping.clone()),
                content.into(),
                (false, 1, true),
                true,
                target.map(|i| keys[i].clone()),
                None,
                viewport,
                target,
                super::super::selection::message_source(Arc::new(Vec::new()), Vec::new()),
            );
            (widget, mapping)
        }
        let keys = Arc::new((0..10_000).map(|i| format!("$row-{i}")).collect::<Vec<_>>());
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let limits = layout::Limits::new(Size::ZERO, Size::new(600.0, 400.0));
        let (mut widget, mapping) = build(keys.clone(), Some(5_000), 40.0);
        let mut tree = Tree::new(&widget);
        let node = widget.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let mut before = Probe {
            keys: mapping,
            ..Default::default()
        };
        widget
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut before);
        assert!(
            before
                .rows
                .iter()
                .any(|(key, y)| key == "$row-5000" && (*y - 180.0).abs() < 1.0)
        );
        let anchor = before.rows.first().unwrap().clone();

        // Prepending changes global row indices while preserving logical identity.
        let mut prepended: Vec<_> = (0..200).map(|i| format!("$older-{i}")).collect();
        prepended.extend(keys.iter().cloned());
        let prepended = Arc::new(prepended);
        let (mut widget, mapping) = build(prepended.clone(), None, 40.0);
        tree.diff(widget.as_widget());
        let node = widget.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let mut after = Probe {
            keys: mapping,
            ..Default::default()
        };
        widget
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut after);
        assert!(
            after
                .rows
                .iter()
                .any(|(key, y)| key == &anchor.0 && (*y - anchor.1).abs() < 1.0)
        );

        // A narrower viewport changes row heights without moving the event anchor.
        let (mut widget, mapping) = build(prepended, None, 80.0);
        tree.diff(widget.as_widget());
        let limits = layout::Limits::new(Size::ZERO, Size::new(300.0, 400.0));
        let node = widget.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let mut resized = Probe {
            keys: mapping,
            ..Default::default()
        };
        widget
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut resized);
        assert!(
            resized
                .rows
                .iter()
                .any(|(key, y)| key == &anchor.0 && (*y - anchor.1).abs() < 1.0)
        );

        // A jump above the mounted window must not persist
        // an offscreen row as if it were the first visible reading anchor.
        widget.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut widget::operation::scrollable::scroll_to::<()>(
                "room-timeline".into(),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(resized.metrics.maximum()),
                },
            ),
        );
        let mut pending_messages = Vec::new();
        let mut pending_shell = Shell::new(&mut pending_messages);
        widget.as_widget_mut().update(
            &mut tree,
            &Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut pending_shell,
            &Rectangle::with_size(node.size()),
        );
        assert!(
            !pending_messages
                .iter()
                .any(|message| matches!(message, Message::ReadingPosition { event: Some(_), .. })),
            "offscreen reading anchor emitted before corrective layout: {pending_messages:?}"
        );

        // An explicit native operation is followed by its invalidated layout.
        widget.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut widget::operation::scrollable::scroll_to::<()>(
                "room-timeline".into(),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(0.0),
                },
            ),
        );
        let node = widget.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let mut newest = Probe {
            keys: resized.keys,
            ..Default::default()
        };
        widget
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut newest);
        assert!(newest.rows.iter().any(|(key, _)| key == "$row-9999"));
        assert!(newest.rows.len() <= 11);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        widget.as_widget_mut().update(
            &mut tree,
            &Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(node.size()),
        );
        assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
        assert!(!shell.is_layout_invalid());
    }
}
