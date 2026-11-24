//! Opt-in, local-only timeline measurements. No SDK, accounts, or window required.
use super::{App, DraftKey, MessageRow, demo};
use iced::{
    Color, Font, Rectangle, Size,
    advanced::{Layout, Renderer as _, layout, mouse, renderer, widget},
};
use iced_tiny_skia::graphics::Viewport;
use std::{sync::Arc, time::Instant};

fn rss_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .and_then(|value| value.split_whitespace().next()?.parse().ok())
        })
}

fn rows(count: usize) -> Vec<MessageRow> {
    let mut source = demo::Demo::default();
    let room = |name: &str| DraftKey {
        room: name.into(),
        root: None,
    };
    let plain = source.pane(room("studio"), 0).rows[0].clone();
    let media = source
        .pane(room("images"), 0)
        .rows
        .iter()
        .find(|row| row.image.is_some())
        .expect("demo image fixture")
        .clone();
    (0..count).map(|index| {
        let mut row = if index % 10 == 9 { media.clone() } else { plain.clone() };
        row.key = format!("history-{index}");
        row.event_id = Some(format!("$history-{index}"));
        row.sender = format!("@person{}:example.org", index % 4);
        row.display_name = format!("Person {}", index % 4);
        row.timestamp_ms = Some(1_700_000_000_000 + index as i64 * 60_000);
        row.thread_root = None;
        row.thread_replies = 0;
        row.reply = None;
        row.reactions.clear();
        row.read_by.clear();
        row.is_own = false;
        row.is_live = false;
        row.edit_source = None;
        row.body = format!("Message {index}: A local timeline sample with wrapping text, café, 日本語 and שלום. Keep the reading position through new arrivals and window resizing.");
        row.formatted = match index % 10 {
            6 => interconnect::message_document::Document::parse(&format!("<p>Message {index}: <strong>Planning notes</strong> and a <a href=\"https://example.org/notes\">reference</a>.</p><ul><li>Keep drafts</li><li>Preserve reading positions</li></ul>")),
            7 => interconnect::message_document::Document::parse(&format!("<blockquote>Message {index}: café, 日本語 and שלום; a longer quoted paragraph to exercise wrapping.</blockquote><p>Follow-up with <em>emphasis</em> and <code>inline_code()</code>.</p>")),
            8 => interconnect::message_document::Document::parse(&format!("<pre><code class=\"language-rust\">// Message {index}\nfn main() {{\n    let position = 42;\n    println!(\"{{position}}\");\n}}</code></pre>")),
            _ => None,
        }.map(Arc::new);
        row
    }).collect()
}

/// Keep single-message workloads separate from mixed-history scaling.
fn workload(name: &str, count: usize) -> Vec<MessageRow> {
    if name == "mixed" {
        return rows(count);
    }
    let mut row = rows(1).remove(0);
    row.body = match name {
        "long-text" => {
            "Technical notes: café, 日本語, שלום. Preserve selection and the reading position. "
                .repeat(count)
        }
        "many-line-code" => (0..count)
            .map(|i| format!("let value_{i} = calculate({i}); // café 👩🏽‍💻\n"))
            .collect(),
        "wide-code" => format!("// {}\n", "column_0123456789 ".repeat(count)),
        _ => panic!("unknown workload: {name}"),
    };
    row.formatted = if name == "long-text" {
        None
    } else {
        let mut document = interconnect::message_document::Document::parse(
            "<pre><code class=\"language-rust\">fixture</code></pre>",
        )
        .unwrap();
        // Synthetic renderer stress deliberately bypasses the incoming HTML byte
        // limit, so oversized code cannot silently become a plain-text fallback.
        document.blocks[0].runs[0].content =
            interconnect::message_document::Inline::Text(row.body.clone());
        Some(Arc::new(document))
    };
    vec![row]
}

#[test]
fn individual_message_workloads_are_distinct_and_local() {
    for name in ["long-text", "many-line-code", "wide-code"] {
        let rows = workload(name, 1000);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].body.len() > 10_000);
        assert_eq!(rows[0].formatted.is_some(), name != "long-text");
        assert_eq!(
            rows[0].body.lines().count(),
            if name == "many-line-code" { 1000 } else { 1 }
        );
    }
}

struct Frame {
    tree: widget::Tree,
    renderer: iced::Renderer,
    pixels: tiny_skia::Pixmap,
    mask: tiny_skia::Mask,
    peak_mounted: usize,
    loaded: usize,
    code_expanded: Option<bool>,
}
impl Frame {
    fn new() -> Self {
        Self {
            tree: widget::Tree::empty(),
            renderer: super::test_renderer(Font::with_name("Inter"), iced::Pixels(14.0)),
            pixels: tiny_skia::Pixmap::new(1280, 800).unwrap(),
            mask: tiny_skia::Mask::new(1280, 800).unwrap(),
            peak_mounted: 0,
            loaded: 0,
            code_expanded: None,
        }
    }
    /// Measure view construction, diff/layout, optional scroll operation, drawing,
    /// and full software rasterization. Buffers and fixture generation are excluded.
    fn render(&mut self, app: &App, size: Size, scroll: Option<f32>) {
        let bounds = Rectangle::with_size(size);
        self.renderer.reset(bounds);
        let mut view = app.view();
        self.tree.diff(view.as_widget());
        let mut node = view.as_widget_mut().layout(
            &mut self.tree,
            &self.renderer,
            &layout::Limits::new(size, size),
        );
        if let Some(expanded) = self.code_expanded.take() {
            struct Expand(bool);
            impl widget::Operation for Expand {
                fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                    f(self);
                }
                fn custom(
                    &mut self,
                    _: Option<&widget::Id>,
                    _: Rectangle,
                    state: &mut dyn std::any::Any,
                ) {
                    if let Some(state) = state.downcast_mut::<super::selectable::State>()
                        && let Some(rich) = &mut state.rich
                    {
                        for index in 0..rich.document.blocks.len() {
                            if matches!(
                                rich.document.blocks[index].kind,
                                interconnect::message_document::BlockKind::Code { .. }
                            ) && rich
                                .code_views
                                .get(&index)
                                .is_some_and(|view| view.expanded)
                                != self.0
                            {
                                rich.code_action(index, super::formatted::CodeAction::Toggle);
                            }
                        }
                    }
                }
            }
            view.as_widget_mut().operate(
                &mut self.tree,
                Layout::new(&node),
                &self.renderer,
                &mut Expand(expanded),
            );
            node = view.as_widget_mut().layout(
                &mut self.tree,
                &self.renderer,
                &layout::Limits::new(size, size),
            );
        }
        if let Some(offset) = scroll {
            view.as_widget_mut().operate(
                &mut self.tree,
                Layout::new(&node),
                &self.renderer,
                &mut widget::operation::scrollable::scroll_to::<()>(
                    "room-timeline".into(),
                    iced::widget::operation::AbsoluteOffset {
                        x: None,
                        y: Some(offset),
                    },
                ),
            );
        }
        if scroll.is_some() {
            node = view.as_widget_mut().layout(
                &mut self.tree,
                &self.renderer,
                &layout::Limits::new(size, size),
            );
        }
        #[derive(Default)]
        struct Count(super::virtual_rows::Stats);
        impl widget::Operation for Count {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn custom(
                &mut self,
                _: Option<&widget::Id>,
                _: Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(stats) = state.downcast_ref::<super::virtual_rows::Stats>() {
                    self.0.mounted += stats.mounted;
                    self.0.loaded += stats.loaded;
                }
            }
        }
        let mut count = Count::default();
        view.as_widget_mut().operate(
            &mut self.tree,
            Layout::new(&node),
            &self.renderer,
            &mut count,
        );
        self.peak_mounted = self.peak_mounted.max(count.0.mounted);
        self.loaded = count.0.loaded;
        view.as_widget().draw(
            &self.tree,
            &mut self.renderer,
            &super::appearance::theme(app.effective_theme()),
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &bounds,
        );
        let iced::Renderer::Secondary(software) = &mut self.renderer else {
            unreachable!()
        };
        let viewport = Viewport::with_physical_size(Size::new(1280, 800), 1.0);
        software.draw(
            &mut self.pixels.as_mut(),
            &mut self.mask,
            &viewport,
            &[bounds],
            Color::WHITE,
        );
        std::hint::black_box(self.pixels.data());
    }
}

fn summarize(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    serde_json::json!({
        "median_ms": (values[(values.len() - 1) / 2] + values[values.len() / 2]) / 2.0,
        "p95_ms": values[(values.len() * 95).div_ceil(100).saturating_sub(1)],
        "max_ms": values.last().unwrap(),
    })
}

#[test]
#[ignore = "opt-in timeline measurement; run scripts/measure-long-history.py"]
fn measure() {
    let count: usize = std::env::var("INTERCONNECT_HISTORY_EVENTS")
        .unwrap_or_else(|_| "500".into())
        .parse()
        .unwrap();
    let workload_name =
        std::env::var("INTERCONNECT_HISTORY_WORKLOAD").unwrap_or_else(|_| "mixed".into());
    let samples: usize = std::env::var("INTERCONNECT_HISTORY_SAMPLES")
        .unwrap_or_else(|_| "20".into())
        .parse()
        .unwrap();
    assert!((1..=100_000).contains(&count) && (1..=1000).contains(&samples));
    {
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .unwrap();
        fonts.load_font(
            include_bytes!("../../../assets/fonts/Inter-Regular.otf")
                .as_slice()
                .into(),
        );
        fonts.load_font(
            include_bytes!("../../../assets/fonts/Inter-SemiBold.otf")
                .as_slice()
                .into(),
        );
    }
    let mut app = super::tests::demo_app();
    let mut frame = Frame::new();
    // Warm the regular shell, font discovery, and rendering before the baseline.
    let wide = Size::new(1280.0, 800.0);
    let narrow = Size::new(960.0, 800.0);
    frame.render(&app, wide, None);
    let baseline = rss_kib();
    app.model.room.as_mut().unwrap().rows = Arc::new(workload(&workload_name, count));
    let initial_rows = app.model.room.as_ref().unwrap().rows.len();
    let message_bytes: usize = app
        .model
        .room
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .map(|row| row.body.len())
        .sum();
    let fixture = rss_kib();
    let started = Instant::now();
    frame.render(&app, wide, None);
    let initial_ms = started.elapsed().as_secs_f64() * 1000.0;
    let loaded = rss_kib();
    let mut append = Vec::new();
    let mut resize = Vec::new();
    let mut scroll = Vec::new();
    let new_rows = rows(samples);
    for (index, mut row) in new_rows.into_iter().enumerate() {
        row.key = format!("append-{index}");
        row.event_id = Some(format!("$append-{index}"));
        row.timestamp_ms = Some(1_700_000_000_000 + (count + index) as i64 * 60_000);
        let started = Instant::now();
        Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows).push(row);
        frame.render(&app, wide, None);
        append.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    for index in 0..samples {
        let started = Instant::now();
        frame.render(&app, if index % 2 == 0 { narrow } else { wide }, None);
        resize.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    frame.render(&app, wide, Some(0.0));
    for index in 0..samples {
        let started = Instant::now();
        frame.render(&app, wide, Some((index + 1) as f32 * 400.0));
        scroll.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let mut expand = Vec::new();
    let mut collapse = Vec::new();
    if matches!(workload_name.as_str(), "many-line-code" | "wide-code") {
        // Return to the original message; appended rows must not hide the target.
        frame.render(&app, wide, Some(f32::MAX));
        for _ in 0..samples {
            frame.code_expanded = Some(true);
            let started = Instant::now();
            frame.render(&app, wide, None);
            expand.push(started.elapsed().as_secs_f64() * 1000.0);
            frame.code_expanded = Some(false);
            let started = Instant::now();
            frame.render(&app, wide, None);
            collapse.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let settled = rss_kib();
    let mounted = frame.peak_mounted;
    let loaded_rows = frame.loaded;
    assert_eq!(
        loaded_rows,
        initial_rows + samples,
        "rendering must retain all loaded events"
    );
    assert!(
        mounted <= 192,
        "the mixed-history fixture must keep widget state bounded"
    );
    drop(frame);
    drop(app);
    println!(
        "LONG_HISTORY {}",
        serde_json::json!({
            "workload": workload_name, "units": count, "message_bytes": message_bytes, "events": initial_rows, "peak_mounted_rows": mounted, "loaded_rows": loaded_rows, "samples": samples, "optimized": !cfg!(debug_assertions),
            "expand": (!expand.is_empty()).then(|| summarize(expand)), "collapse": (!collapse.is_empty()).then(|| summarize(collapse)),
            "initial_ms": initial_ms, "append": summarize(append), "resize": summarize(resize), "scroll": summarize(scroll),
            "rss_kib": { "baseline": baseline, "fixture": fixture, "loaded": loaded, "settled": settled, "after_drop": rss_kib() },
        })
    );
}
