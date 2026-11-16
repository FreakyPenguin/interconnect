//! Give a timeline's widget state the same lifetime as its selected SDK view.
//! Preserve a visible message across relayouts, or follow new content at the bottom.
//! Input, clipping, and drawing remain Iced's responsibility.
use super::Message;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{self, Tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};
use std::collections::HashMap;

#[cfg(test)]
pub fn view(
    generation: u64,
    keys: Vec<Option<String>>,
    content: Element<'_, Message>,
) -> Element<'_, Message> {
    make_view(generation, keys, content, None, false)
}

#[cfg(test)]
pub fn history_view(
    generation: u64,
    keys: Vec<Option<String>>,
    content: Element<'_, Message>,
    thread: bool,
    revision: u64,
    ready: bool,
) -> Element<'_, Message> {
    make_view(
        generation,
        keys,
        content,
        Some((thread, revision, ready)),
        false,
    )
}

pub fn room_history_view(
    generation: u64,
    keys: Vec<Option<String>>,
    content: Element<'_, Message>,
    revision: u64,
    ready: bool,
    observe_visible: bool,
) -> Element<'_, Message> {
    make_view(
        generation,
        keys,
        content,
        Some((false, revision, ready)),
        observe_visible,
    )
}

pub fn thread_history_view(
    generation: u64,
    keys: Vec<Option<String>>,
    content: Element<'_, Message>,
    revision: u64,
    ready: bool,
    observe_visible: bool,
) -> Element<'_, Message> {
    make_view(
        generation,
        keys,
        content,
        Some((true, revision, ready)),
        observe_visible,
    )
}

fn make_view(
    generation: u64,
    keys: Vec<Option<String>>,
    content: Element<'_, Message>,
    history: Option<(bool, u64, bool)>,
    observe_visible: bool,
) -> Element<'_, Message> {
    Element::new(Timeline {
        generation,
        history,
        observe_visible,
        keys: keys
            .into_iter()
            .enumerate()
            .filter_map(|(index, key)| key.map(|key| (row_id(index), key)))
            .collect(),
        content,
    })
}
struct Timeline<'a> {
    generation: u64,
    history: Option<(bool, u64, bool)>,
    observe_visible: bool,
    keys: HashMap<widget::Id, String>,
    content: Element<'a, Message>,
}
pub fn row_id(index: usize) -> widget::Id {
    widget::Id::from(format!("timeline-row-{index}"))
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub height: f32,
    pub content_height: f32,
    pub translation: f32,
}
impl Metrics {
    pub fn maximum(&self) -> f32 {
        (self.content_height - self.height).max(0.0)
    }
    fn distance_from_bottom(&self) -> f32 {
        self.maximum() - self.translation
    }
    pub fn buffer(&self) -> f32 {
        (self.height * 3.0).max(1200.0)
    }
}
#[derive(Clone, PartialEq)]
struct Row {
    key: String,
    y: f32,
    height: f32,
}
#[derive(Clone)]
struct Snapshot {
    metrics: Metrics,
    rows: Vec<Row>,
}
struct Identity {
    selection: super::selection::Selection,
    generation: u64,
    snapshot: Option<Snapshot>,
    reported: Option<(Metrics, u64, bool)>,
    visible: Option<(Metrics, Vec<String>)>,
    media_visible: Option<(Metrics, Vec<String>)>,
}

struct Measure<'a> {
    keys: Option<&'a HashMap<widget::Id, String>>,
    origin: f32,
    snapshot: Snapshot,
}
impl<'a> Measure<'a> {
    fn new(keys: Option<&'a HashMap<widget::Id, String>>) -> Self {
        Self {
            keys,
            origin: 0.0,
            snapshot: Snapshot {
                metrics: Metrics::default(),
                rows: Vec::new(),
            },
        }
    }
}
impl widget::Operation for Measure<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
        if self.keys.is_some() {
            operate(self);
        }
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
        self.snapshot.metrics = Metrics {
            height: bounds.height,
            content_height: content.height,
            translation: translation.y,
        };
    }
    fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
        if let Some(key) = id.and_then(|id| self.keys?.get(id)) {
            self.snapshot.rows.push(Row {
                key: key.clone(),
                y: bounds.y - self.origin,
                height: bounds.height,
            });
        }
    }
}
impl Timeline<'_> {
    // Scroll input and explicit operations refresh only scroll metrics. This
    // avoids a row traversal on every pointer event and never schedules work.
    fn remember_scroll(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) {
        let mut measure = Measure::new(None);
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut measure);
        if let Some(snapshot) = &mut tree.state.downcast_mut::<Identity>().snapshot {
            snapshot.metrics = measure.snapshot.metrics;
        }
    }
}

fn anchored_offset(previous: &Snapshot, current: &Snapshot) -> f32 {
    if previous.metrics.distance_from_bottom() <= 1.0 {
        return 0.0;
    }
    let top = previous
        .rows
        .iter()
        .position(|r| r.y + r.height > previous.metrics.translation)
        .unwrap_or(previous.rows.len());
    let current_rows: HashMap<_, _> = current.rows.iter().map(|r| (r.key.as_str(), r)).collect();
    // If the visible message disappears, keep the nearest surviving successor,
    // then predecessor. If no IDs survive, preserve the old top pixel offset.
    let translation = previous.rows[top..]
        .iter()
        .chain(previous.rows[..top].iter().rev())
        .find_map(|old| {
            current_rows
                .get(old.key.as_str())
                .map(|new| new.y - (old.y - previous.metrics.translation))
        })
        .unwrap_or(previous.metrics.translation);
    (current.metrics.maximum() - translation).clamp(0.0, current.metrics.maximum())
}

impl Widget<Message, Theme, Renderer> for Timeline<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<Identity>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(Identity {
            generation: self.generation,
            snapshot: None,
            reported: None,
            visible: None,
            media_visible: None,
            selection: Default::default(),
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let identity = tree.state.downcast_mut::<Identity>();
        if identity.generation != self.generation {
            identity.generation = self.generation;
            identity.snapshot = None;
            identity.reported = None;
            identity.visible = None;
            identity.media_visible = None;
            identity.selection = Default::default();
            tree.children = self.children();
        } else {
            if !self.observe_visible {
                identity.visible = None;
            }
            tree.children[0].diff(self.content.as_widget());
        }
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
        let mut measure = Measure::new(Some(&self.keys));
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            Layout::new(&node),
            renderer,
            &mut measure,
        );
        let mut current = measure.snapshot;
        if let Some(previous) = &tree.state.downcast_ref::<Identity>().snapshot {
            let offset = anchored_offset(previous, &current);
            struct Restore(f32);
            impl widget::Operation for Restore {
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
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                Layout::new(&node),
                renderer,
                &mut Restore(offset),
            );
            current.metrics.translation = (current.metrics.maximum() - offset).round();
        }
        let identity = tree.state.downcast_mut::<Identity>();
        if identity
            .snapshot
            .as_ref()
            .is_some_and(|previous| previous.rows != current.rows)
        {
            identity.media_visible = None;
        }
        identity.snapshot = Some(current);
        identity.selection.restore(
            self.content.as_widget_mut(),
            &mut tree.children[0],
            Layout::new(&node),
            renderer,
        );
        node
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
        self.remember_scroll(tree, layout, renderer);
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
        tree.state.downcast_mut::<Identity>().selection.update(
            self.content.as_widget_mut(),
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
            self.generation,
        );
        self.remember_scroll(tree, layout, renderer);
        if let Some((thread, _, _)) = self.history {
            let identity = tree.state.downcast_mut::<Identity>();
            if let Some(snapshot) = &identity.snapshot {
                let metrics = snapshot.metrics;
                if identity
                    .media_visible
                    .as_ref()
                    .is_none_or(|(previous, _)| *previous != metrics)
                {
                    let keys: Vec<_> = snapshot
                        .rows
                        .iter()
                        .filter(|row| {
                            row.y + row.height > metrics.translation + 1.0
                                && row.y < metrics.translation + metrics.height - 1.0
                        })
                        .map(|row| row.key.clone())
                        .collect();
                    if (identity.media_visible.is_some() || !keys.is_empty())
                        && identity
                            .media_visible
                            .as_ref()
                            .map(|(_, previous)| previous)
                            != Some(&keys)
                    {
                        shell.publish(Message::RowsVisible {
                            thread,
                            generation: self.generation,
                            keys: keys.clone(),
                        });
                    }
                    identity.media_visible = Some((metrics, keys));
                }
            }
        }
        if self.observe_visible && !matches!(event, Event::Window(iced::window::Event::Unfocused)) {
            let identity = tree.state.downcast_mut::<Identity>();
            if let Some(snapshot) = &identity.snapshot {
                let metrics = snapshot.metrics;
                let visible: Vec<_> = snapshot
                    .rows
                    .iter()
                    .filter(|row| {
                        row.y + row.height > metrics.translation + 1.0
                            && row.y < metrics.translation + metrics.height - 1.0
                    })
                    .map(|row| row.key.clone())
                    .collect();
                let report = (metrics, visible);
                if identity.visible.as_ref() != Some(&report) {
                    shell.publish(Message::TimelineVisible {
                        thread: self.history.is_some_and(|(thread, _, _)| thread),
                        generation: self.generation,
                        keys: report.1.clone(),
                        metrics,
                    });
                    identity.visible = Some(report);
                }
            }
        }
        if let Some((thread, revision, ready)) = self.history {
            let identity = tree.state.downcast_mut::<Identity>();
            if let Some(snapshot) = &identity.snapshot {
                let metrics = snapshot.metrics;
                let report = (metrics, revision, ready);
                if metrics.height > 0.0 && identity.reported != Some(report) {
                    identity.reported = Some(report);
                    shell.publish(Message::TimelineMeasured {
                        thread,
                        generation: self.generation,
                        revision,
                        metrics,
                    });
                }
            }
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
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
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
    use iced::widget::{column, container, scrollable, space};

    #[test]
    fn viewport_reports_layout_and_page_changes_once_without_timers() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = Tree::empty();
        for (revision, ready, height, expected) in [
            (1, true, 400.0, true),
            (1, true, 400.0, false),
            (1, false, 400.0, true),
            (2, true, 400.0, true), // A page with no visible rows still advances demand.
            (2, true, 400.0, false),
            (2, true, 600.0, true),
            (2, true, 600.0, false),
        ] {
            let mut timeline = history_view(
                7,
                vec![],
                scrollable(space().height(1000))
                    .height(Length::Fill)
                    .anchor_bottom()
                    .into(),
                false,
                revision,
                ready,
            );
            tree.diff(timeline.as_widget());
            let node = timeline.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(300.0, height)),
            );
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            timeline.as_widget_mut().update(
                &mut tree,
                &Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, height)),
            );
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
            assert_eq!(messages.len(), usize::from(expected));
            if expected {
                assert!(matches!(&messages[0], Message::TimelineMeasured {
                    thread: false, generation: 7, revision: current, metrics,
                } if *current == revision && metrics.height == height));
            }
        }
    }

    #[test]
    fn avatar_visibility_follows_room_and_thread_viewports_without_repeated_reports() {
        use iced::widget::{column, container, scrollable, space};
        for thread in [false, true] {
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut tree = Tree::empty();
            for (offset, expected) in [
                (0.0, Some(vec!["b", "c"])),
                (0.0, None),
                (40.0, Some(vec!["a", "b"])),
            ] {
                let rows =
                    column((0..3).map(|index| {
                        Element::from(container(space().height(40)).id(row_id(index)))
                    }));
                let mut view = history_view(
                    9,
                    ["a", "b", "c"].map(|key| Some(key.to_owned())).into(),
                    scrollable(rows)
                        .id("avatar-viewport")
                        .height(Length::Fill)
                        .anchor_bottom()
                        .into(),
                    thread,
                    1,
                    true,
                );
                tree.diff(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(300.0, 80.0)),
                );
                view.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        "avatar-viewport".into(),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(offset),
                        },
                    ),
                );
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &Event::Window(iced::window::Event::RedrawRequested(
                        iced::time::Instant::now(),
                    )),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &Rectangle::with_size(Size::new(300.0, 80.0)),
                );
                assert!(!matches!(
                    shell.redraw_request(),
                    iced::window::RedrawRequest::At(_)
                ));
                let reports: Vec<_> = messages
                    .into_iter()
                    .filter_map(|message| match message {
                        Message::RowsVisible {
                            thread: current,
                            generation: 9,
                            keys,
                        } if current == thread => Some(keys),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    reports,
                    expected
                        .map(|keys| keys.into_iter().map(str::to_owned).collect::<Vec<_>>())
                        .into_iter()
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    struct Harness {
        tree: Tree,
        renderer: Renderer,
    }
    impl Harness {
        fn new() -> Self {
            Self {
                tree: Tree::empty(),
                renderer: crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0)),
            }
        }
        fn render(
            &mut self,
            rows: &[(String, f32)],
            generation: u64,
            height: f32,
            offset: Option<f32>,
        ) -> Snapshot {
            let messages = column(rows.iter().enumerate().map(|(index, (_, height))| {
                Element::from(container(space().height(*height)).id(row_id(index)))
            }));
            let scroll = scrollable(messages)
                .id("test-timeline")
                .anchor_bottom()
                .height(Length::Fill);
            let mut timeline = view(
                generation,
                rows.iter().map(|(key, _)| Some(key.clone())).collect(),
                scroll.into(),
            );
            self.tree.diff(timeline.as_widget());
            let node = timeline.as_widget_mut().layout(
                &mut self.tree,
                &self.renderer,
                &layout::Limits::new(Size::ZERO, Size::new(300.0, height)),
            );
            if let Some(offset) = offset {
                timeline.as_widget_mut().operate(
                    &mut self.tree,
                    Layout::new(&node),
                    &self.renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        "test-timeline".into(),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(offset),
                        },
                    ),
                );
            }
            self.tree
                .state
                .downcast_ref::<Identity>()
                .snapshot
                .as_ref()
                .unwrap()
                .clone()
        }
    }
    fn screen_y(snapshot: &Snapshot, key: &str) -> f32 {
        snapshot.rows.iter().find(|row| row.key == key).unwrap().y - snapshot.metrics.translation
    }

    #[test]
    fn visible_thread_reports_follow_actual_rows_and_focus_without_timers() {
        let mut harness = Harness::new();
        let mut rows: Vec<_> = (0..6).map(|index| format!("reply-{index}")).collect();
        let frame = |harness: &mut Harness, rows: &[String], enabled: bool, offset: Option<f32>| {
            let content =
                column(rows.iter().enumerate().map(|(index, _)| {
                    Element::from(container(space().height(40)).id(row_id(index)))
                }));
            let mut view = thread_history_view(
                7,
                rows.iter().cloned().map(Some).collect(),
                scrollable(content)
                    .id("thread-timeline")
                    .anchor_bottom()
                    .height(Length::Fill)
                    .into(),
                1,
                true,
                enabled,
            );
            harness.tree.diff(view.as_widget());
            let node = view.as_widget_mut().layout(
                &mut harness.tree,
                &harness.renderer,
                &layout::Limits::new(Size::ZERO, Size::new(300.0, 80.0)),
            );
            if let Some(offset) = offset {
                view.as_widget_mut().operate(
                    &mut harness.tree,
                    Layout::new(&node),
                    &harness.renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        "thread-timeline".into(),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(offset),
                        },
                    ),
                );
            }
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut harness.tree,
                &Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &harness.renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, 80.0)),
            );
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
            messages
                .into_iter()
                .filter_map(|message| match message {
                    Message::TimelineVisible {
                        thread: true,
                        generation: 7,
                        keys,
                        ..
                    } => Some(keys),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            frame(&mut harness, &rows, true, None),
            vec![vec!["reply-4", "reply-5"]]
        );
        assert!(frame(&mut harness, &rows, true, None).is_empty());
        assert!(frame(&mut harness, &rows, false, None).is_empty());
        assert_eq!(
            frame(&mut harness, &rows, true, None),
            vec![vec!["reply-4", "reply-5"]]
        );
        rows[5] = "new-reply".into();
        assert_eq!(
            frame(&mut harness, &rows, true, None),
            vec![vec!["reply-4", "new-reply"]],
            "same geometry still reports new visible IDs"
        );
        assert_eq!(
            frame(&mut harness, &rows, true, Some(80.0)),
            vec![vec!["reply-2", "reply-3"]]
        );
        assert!(frame(&mut harness, &rows, true, None).is_empty());
    }

    #[test]
    fn moving_thread_summary_keeps_nearest_surviving_reading_anchor() {
        let mut harness = Harness::new();
        let mut rows: Vec<_> = (0..10)
            .map(|index| (format!("event-{index}"), 40.0))
            .collect();
        rows.insert(2, ("thread:$root:$old-reply".into(), 40.0));
        let before = harness.render(&rows, 1, 200.0, Some(160.0));
        assert_near(screen_y(&before, "thread:$root:$old-reply"), 0.0);
        let successor = screen_y(&before, "event-2");
        rows.remove(2);
        rows.push(("thread:$root:$new-reply".into(), 40.0));
        let after = harness.render(&rows, 1, 200.0, None);
        assert_near(screen_y(&after, "event-2"), successor);
        assert!(after.metrics.distance_from_bottom() > 1.0);
    }
    fn assert_near(actual: f32, expected: f32) {
        assert!((actual - expected).abs() <= 1.0, "{actual} != {expected}");
    }

    #[test]
    fn arrivals_follow_only_at_bottom_and_explicit_jump_resumes_following() {
        let mut harness = Harness::new();
        let mut rows: Vec<_> = (0..10).map(|i| (format!("event-{i}"), 40.0)).collect();
        assert_near(
            harness
                .render(&rows, 1, 200.0, None)
                .metrics
                .distance_from_bottom(),
            0.0,
        );
        rows.push(("incoming-1".into(), 90.0));
        assert_near(
            harness
                .render(&rows, 1, 200.0, None)
                .metrics
                .distance_from_bottom(),
            0.0,
        );
        let reading = harness.render(&rows, 1, 200.0, Some(180.0));
        let top = screen_y(&reading, "event-2");
        rows.push(("incoming-2".into(), 130.0));
        let arrived = harness.render(&rows, 1, 200.0, None);
        assert_near(screen_y(&arrived, "event-2"), top);
        assert_near(
            arrived.metrics.distance_from_bottom(),
            reading.metrics.distance_from_bottom() + 130.0,
        );
        harness.render(&rows, 1, 200.0, Some(0.0));
        rows.push(("incoming-3".into(), 70.0));
        assert_near(
            harness
                .render(&rows, 1, 200.0, None)
                .metrics
                .distance_from_bottom(),
            0.0,
        );
        harness.render(&rows, 1, 200.0, Some(100.0));
        assert_near(
            harness
                .render(&rows, 2, 200.0, None)
                .metrics
                .distance_from_bottom(),
            0.0,
        );
    }

    #[test]
    fn pagination_edits_resize_and_anchor_removal_preserve_reading_position() {
        let mut harness = Harness::new();
        let mut rows: Vec<_> = (0..10).map(|i| (format!("event-{i}"), 40.0)).collect();
        let reading = harness.render(&rows, 1, 200.0, Some(90.0));
        let top = screen_y(&reading, "event-2");
        rows.insert(0, ("older".into(), 60.0));
        assert_near(
            screen_y(&harness.render(&rows, 1, 200.0, None), "event-2"),
            top,
        );
        rows[0].1 += 45.0;
        assert_near(
            screen_y(&harness.render(&rows, 1, 200.0, None), "event-2"),
            top,
        );
        let resized = harness.render(&rows, 1, 240.0, None);
        assert_near(screen_y(&resized, "event-2"), top);
        let next_y = screen_y(&resized, "event-3");
        rows.retain(|(key, _)| key != "event-2");
        assert_near(
            screen_y(&harness.render(&rows, 1, 240.0, None), "event-3"),
            next_y,
        );
        for _ in 0..5 {
            assert_near(
                screen_y(&harness.render(&rows, 1, 240.0, None), "event-3"),
                next_y,
            );
        }
    }
}
