//! Native formatted text. A single paragraph layout supplies drawing, hit-testing,
//! decorations and selection geometry; the editor stores only logical positions.
use super::{Message, emoji, shell};
use iced::advanced::{
    Renderer as _, layout, renderer,
    text::{self, Paragraph as _, Renderer as _},
};
use iced::{Font, Point, Rectangle, Size, Vector, font};
use interconnect::message_document::{self as document, Block, BlockKind, Document};
use std::{cell::RefCell, collections::BTreeSet, ops::Range, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;

type Paragraph = iced::advanced::graphics::text::Paragraph;
#[derive(Debug)]
struct LaidBlock {
    paragraph: Paragraph,
    bounds: Rectangle,
    range: Range<usize>,
    block: Block,
    /// Each shaping span maps back to its semantic run (including emoji font spans).
    runs: Vec<usize>,
    lines: Vec<usize>,
    size: f32,
}
#[derive(Debug)]
pub struct State {
    pub document: Arc<Document>,
    revealed: BTreeSet<usize>,
    blocks: Vec<LaidBlock>,
    width: Option<f32>,
    pub pressed: Option<Point>,
    pub focused: bool,
    pub click: Option<iced::advanced::mouse::Click>,
    pub hover: Option<Point>,
    preview: RefCell<Option<(String, f32, Paragraph)>>,
}
impl State {
    pub fn new(document: Arc<Document>) -> Self {
        Self {
            document,
            revealed: BTreeSet::new(),
            blocks: Vec::new(),
            width: None,
            pressed: None,
            focused: false,
            click: None,
            hover: None,
            preview: RefCell::new(None),
        }
    }
    pub fn text(&self) -> String {
        self.document.text(&self.revealed)
    }
    pub fn layout(&mut self, width: f32) -> layout::Node {
        if self.width != Some(width) {
            self.blocks.clear();
            let visible = self.document.visible_blocks(&self.revealed);
            let mut y = 0.0;
            for (block, range) in visible.iter().zip(document::block_ranges(&visible)) {
                let inset =
                    ((block.quote_depth + block.list_depth.saturating_sub(1)).min(8) as f32 * 14.0)
                        .min(width / 3.0);
                let code = matches!(block.kind, BlockKind::Code { .. });
                let padding = if code { 8.0 } else { 0.0 };
                let x = inset + padding;
                let size = match block.kind {
                    BlockKind::Heading(level) => 23.0 - f32::from(level.min(6)),
                    _ => 14.0,
                };
                let mut spans: Vec<text::Span<'_, (), Font>> = Vec::new();
                let mut runs = Vec::new();
                for (i, run) in block.runs.iter().enumerate() {
                    let style = &run.style;
                    let base = Font {
                        weight: if style.bold {
                            font::Weight::Bold
                        } else {
                            font::Weight::Normal
                        },
                        style: if style.italic {
                            font::Style::Italic
                        } else {
                            font::Style::Normal
                        },
                        ..if style.code {
                            Font::MONOSPACE
                        } else {
                            Font::with_name("Inter")
                        }
                    };
                    let color = if style.link.is_some()
                        || style.spoilers.iter().any(|id| !self.revealed.contains(id))
                    {
                        shell::ACCENT
                    } else {
                        shell::INK
                    };
                    // Combine ordinary text into spans; keep complete emoji graphemes in their font.
                    let text = run.content.text();
                    let mut start = 0;
                    for (offset, grapheme) in text.grapheme_indices(true) {
                        if !grapheme.contains('\u{fe0e}')
                            && emojis::get(grapheme).is_some()
                            && let Some(font) = emoji::font()
                        {
                            if start < offset {
                                spans.push(
                                    text::Span::new(&text[start..offset])
                                        .font(base)
                                        .color(color),
                                );
                                runs.push(i);
                            }
                            spans.push(text::Span::new(grapheme).font(font).color(color));
                            runs.push(i);
                            start = offset + grapheme.len();
                        }
                    }
                    if start < text.len() {
                        spans.push(text::Span::new(&text[start..]).font(base).color(color));
                        runs.push(i);
                    }
                }
                if spans.is_empty() {
                    spans.push(text::Span::new(""));
                    runs.push(0);
                }
                let paragraph = Paragraph::with_spans(text::Text {
                    content: &spans,
                    bounds: Size::new((width - x - padding).max(1.0), f32::INFINITY),
                    size: iced::Pixels(size),
                    line_height: text::LineHeight::Relative(1.45),
                    font: Font::with_name("Inter"),
                    align_x: text::Alignment::Default,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::WordOrGlyph,
                });
                let height = paragraph.min_height().max(size * 1.45);
                let mut offset = 0;
                let lines = paragraph
                    .buffer()
                    .lines
                    .iter()
                    .map(|line| {
                        let start = offset;
                        offset += line.text().len() + line.ending().as_str().len();
                        start
                    })
                    .collect();
                self.blocks.push(LaidBlock {
                    paragraph,
                    bounds: Rectangle::new(
                        Point::new(x, y + padding),
                        Size::new((width - x - padding).max(1.0), height),
                    ),
                    range,
                    block: block.clone(),
                    runs,
                    lines,
                    size,
                });
                y += height + padding * 2.0 + 8.0;
            }
            self.width = Some(width);
        }
        let height = self.blocks.last().map_or(0.0, |b| {
            b.bounds.y
                + b.bounds.height
                + if matches!(b.block.kind, BlockKind::Code { .. }) {
                    8.0
                } else {
                    0.0
                }
        });
        layout::Node::new(Size::new(width, height))
    }
    pub fn hit(&self, point: Point) -> usize {
        let Some(block) = self
            .blocks
            .iter()
            .find(|b| point.y < b.bounds.y + b.bounds.height)
            .or(self.blocks.last())
        else {
            return 0;
        };
        if point.y < block.bounds.y {
            return block.range.start;
        }
        let local = point - block.bounds.position();
        block
            .paragraph
            .buffer()
            .hit(local.x.max(0.0), local.y.max(0.0))
            .map_or(block.range.end, |hit| {
                block.range.start + block.lines.get(hit.line).copied().unwrap_or(0) + hit.index
            })
    }
    pub fn vertical(&self, offset: usize, down: bool) -> usize {
        let lines: Vec<_> = self
            .blocks
            .iter()
            .flat_map(|block| {
                block
                    .paragraph
                    .buffer()
                    .layout_runs()
                    .map(move |line| (block, line))
            })
            .collect();
        let current = lines.iter().position(|(block, line)| {
            let base = block.range.start + block.lines[line.line_i];
            let start = line.glyphs.iter().map(|g| g.start).min().unwrap_or(0) + base;
            let end = line.glyphs.iter().map(|g| g.end).max().unwrap_or(0) + base;
            start <= offset && offset <= end
        });
        let Some(current) = current else {
            return offset;
        };
        let (block, line) = &lines[current];
        let base = block.range.start + block.lines[line.line_i];
        let x = line
            .glyphs
            .iter()
            .find(|g| base + g.start <= offset && offset <= base + g.end)
            .map_or(block.bounds.x, |g| {
                let after = offset == base + g.end;
                block.bounds.x + g.x + if after != g.level.is_rtl() { g.w } else { 0.0 }
            });
        let target = if down {
            current.checked_add(1).filter(|i| *i < lines.len())
        } else {
            current.checked_sub(1)
        };
        let Some(target) = target else {
            return if down {
                self.blocks.last().map_or(0, |b| b.range.end)
            } else {
                0
            };
        };
        let (block, line) = &lines[target];
        self.hit(Point::new(
            x,
            block.bounds.y + line.line_top + line.line_height / 2.0,
        ))
    }
    fn run_at(&self, point: Point) -> Option<&document::Run> {
        let block = self.blocks.iter().find(|b| b.bounds.contains(point))?;
        let span = block
            .paragraph
            .hit_span(Point::ORIGIN + (point - block.bounds.position()))?;
        let regions = block.paragraph.span_bounds(span);
        let local = Point::ORIGIN + (point - block.bounds.position());
        if !regions.iter().any(|r| r.contains(local)) {
            return None;
        }
        block.block.runs.get(*block.runs.get(span)?)
    }
    pub fn interactive(&self, point: Point) -> bool {
        self.run_at(point).is_some_and(|run| {
            run.style.link.is_some()
                || run
                    .style
                    .spoilers
                    .iter()
                    .any(|id| !self.revealed.contains(id))
        })
    }
    /// Returns true when revealing a spoiler changes canonical text and layout.
    pub fn activate(&mut self, point: Point, publish: &mut impl FnMut(Message)) -> bool {
        let Some(run) = self.run_at(point) else {
            return false;
        };
        if let Some(id) = run
            .style
            .spoilers
            .iter()
            .find(|id| !self.revealed.contains(id))
            .copied()
        {
            self.revealed.insert(id);
            self.width = None;
            return true;
        }
        if let Some(link) = &run.style.link {
            publish(Message::OpenMessageLink(link.clone()));
        }
        false
    }
    pub fn overlay(
        &self,
        origin: Point,
        viewport: Rectangle,
    ) -> Option<iced::advanced::overlay::Element<'static, Message, iced::Theme, iced::Renderer>>
    {
        if self.pressed.is_some() {
            return None;
        }
        let point = self.hover?;
        let position = origin + (point - Point::ORIGIN);
        if !viewport.contains(position) {
            return None;
        }
        let link = self.run_at(point)?.style.link.as_ref()?;
        let width = (viewport.width - 16.0).clamp(1.0, 500.0);
        let mut preview = self.preview.borrow_mut();
        if preview
            .as_ref()
            .is_none_or(|(cached, cached_width, _)| cached != link || *cached_width != width)
        {
            let paragraph = Paragraph::with_text(text::Text {
                content: link,
                bounds: Size::new(width, f32::INFINITY),
                size: iced::Pixels(12.0),
                line_height: text::LineHeight::default(),
                font: Font::with_name("Inter"),
                align_x: text::Alignment::Default,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::WordOrGlyph,
            });
            *preview = Some((link.clone(), width, paragraph));
        }
        // Iced recreates overlay values between layout and drawing. Keep shaped
        // text in the widget state, so both instances use the same paragraph.
        let paragraph = preview.as_ref()?.2.clone();
        Some(iced::advanced::overlay::Element::new(Box::new(
            LinkPreview {
                position,
                paragraph,
            },
        )))
    }
    pub fn draw(
        &self,
        renderer: &mut iced::Renderer,
        origin: Point,
        viewport: Rectangle,
        selection: Option<Range<usize>>,
    ) {
        for block in &self.blocks {
            let translation = origin - Point::ORIGIN;
            let bounds = block.bounds + translation;
            if !bounds.intersects(&viewport) {
                continue;
            }
            let code = matches!(block.block.kind, BlockKind::Code { .. });
            if code {
                quad(
                    renderer,
                    Rectangle::new(
                        Point::new(origin.x, bounds.y - 8.0),
                        Size::new(self.width.unwrap_or(bounds.width), bounds.height + 16.0),
                    ),
                    iced::Color::from_rgb8(245, 246, 248),
                );
            }
            if block.block.quote_depth > 0 {
                quad(
                    renderer,
                    Rectangle::new(
                        Point::new(bounds.x - 9.0, bounds.y),
                        Size::new(2.0, bounds.height),
                    ),
                    shell::MUTED,
                );
            }
            for (index, run_index) in block.runs.iter().enumerate() {
                let Some(run) = block.block.runs.get(*run_index) else {
                    continue;
                };
                let style = &run.style;
                for region in block.paragraph.span_bounds(index) {
                    let region = region + (bounds.position() - Point::ORIGIN);
                    if style.code && !code {
                        quad(renderer, region, iced::Color::from_rgb8(241, 243, 246));
                    }
                    if style.underline || style.link.is_some() {
                        quad(
                            renderer,
                            Rectangle::new(
                                Point::new(region.x, region.y + block.size * 1.2),
                                Size::new(region.width, 1.0),
                            ),
                            if style.link.is_some() {
                                shell::ACCENT
                            } else {
                                shell::INK
                            },
                        );
                    }
                    if style.strike {
                        quad(
                            renderer,
                            Rectangle::new(
                                Point::new(region.x, region.y + block.size * 0.75),
                                Size::new(region.width, 1.0),
                            ),
                            shell::INK,
                        );
                    }
                }
            }
            // Glyph clusters supply byte ranges and visual rectangles, including bidi runs.
            if let Some(selected) = &selection {
                for line in block.paragraph.buffer().layout_runs() {
                    let line_start = block.range.start + block.lines[line.line_i];
                    for glyph in line.glyphs {
                        if line_start + glyph.start < selected.end
                            && line_start + glyph.end > selected.start
                        {
                            let cluster = &line.text[glyph.start..glyph.end];
                            let graphemes: Vec<_> = cluster.grapheme_indices(true).collect();
                            let start = line_start + glyph.start;
                            let from = graphemes
                                .iter()
                                .take_while(|(i, g)| start + i + g.len() <= selected.start)
                                .count() as f32
                                / graphemes.len().max(1) as f32;
                            let to = graphemes
                                .iter()
                                .take_while(|(i, _)| start + i < selected.end)
                                .count() as f32
                                / graphemes.len().max(1) as f32;
                            let left = if glyph.level.is_rtl() { 1.0 - to } else { from };
                            quad(
                                renderer,
                                Rectangle::new(
                                    Point::new(
                                        bounds.x + glyph.x + left * glyph.w,
                                        bounds.y + line.line_top,
                                    ),
                                    Size::new((to - from) * glyph.w, line.line_height),
                                ),
                                iced::Color::from_rgba8(73, 121, 211, 0.25),
                            );
                        }
                    }
                }
            }
            renderer.fill_paragraph(&block.paragraph, bounds.position(), shell::INK, viewport);
        }
    }
}
fn quad(renderer: &mut iced::Renderer, bounds: Rectangle, color: iced::Color) {
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            ..renderer::Quad::default()
        },
        color,
    );
}

struct LinkPreview {
    position: Point,
    paragraph: Paragraph,
}
impl iced::advanced::Overlay<Message, iced::Theme, iced::Renderer> for LinkPreview {
    fn layout(&mut self, _: &iced::Renderer, bounds: Size) -> layout::Node {
        let size = self.paragraph.min_bounds() + Size::new(12.0, 8.0);
        let position = Point::new(
            self.position
                .x
                .min(bounds.width - size.width - 4.0)
                .max(0.0),
            (self.position.y + 20.0)
                .min(bounds.height - size.height)
                .max(0.0),
        );
        layout::Node::new(size).move_to(position)
    }
    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        _: &iced::Theme,
        _: &renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _: iced::advanced::mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        quad(renderer, bounds, iced::Color::from_rgb8(235, 239, 245));
        renderer.fill_paragraph(
            &self.paragraph,
            bounds.position() + Vector::new(6.0, 4.0),
            shell::INK,
            bounds,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_hit_testing_and_copy_offsets_share_wrapped_unicode_text() {
        let document = Arc::new(Document::parse("<h2>Title</h2><p>café <b>bold 👩🏽‍💻 text</b> that wraps over several lines. العربية עברית</p><pre><code class='language-rust'><b>let</b> x = 1;\n  x</code></pre><p><img src='mxc://example/image' alt='diagram'></p>").unwrap());
        for width in [100.0, 300.0] {
            let mut state = State::new(document.clone());
            let first = state.layout(width).bounds();
            let canonical = state.text();
            assert_eq!(state.blocks.len(), 4);
            for block in &state.blocks {
                assert_eq!(
                    block.range.end - block.range.start,
                    block
                        .block
                        .runs
                        .iter()
                        .map(|run| run.content.text().len())
                        .sum::<usize>()
                );
                for line in block.paragraph.buffer().layout_runs() {
                    for glyph in line.glyphs {
                        let point = Point::new(
                            block.bounds.x + glyph.x + glyph.w / 2.0,
                            block.bounds.y + line.line_top + line.line_height / 2.0,
                        );
                        let hit = state.hit(point);
                        assert!(canonical.is_char_boundary(hit), "{hit} {canonical:?}");
                        assert!(block.range.contains(&hit) || hit == block.range.end);
                    }
                }
            }
            let code = &state.blocks[2];
            assert!(
                matches!(&code.block.kind, BlockKind::Code { language: Some(language) } if language == "rust")
            );
            assert!(code.block.runs.iter().any(|r| r.style.bold && r.style.code));
            assert!(canonical.ends_with("diagram"));
            let size = Size::new(width, first.height.ceil());
            let mut renderer = crate::test_renderer(Font::with_name("Inter"), iced::Pixels(14.0));
            renderer.reset(Rectangle::with_size(size));
            state.draw(
                &mut renderer,
                Point::ORIGIN,
                Rectangle::with_size(size),
                Some(0..canonical.len()),
            );
            assert_eq!(
                state.layout(width).bounds(),
                first,
                "drawing must not alter layout"
            );
            if let Ok(path) = std::env::var("INTERCONNECT_FORMATTING_PREVIEW")
                && width == 300.0
            {
                let iced::Renderer::Secondary(software) = &mut renderer else {
                    panic!("software renderer");
                };
                let mut pixels = tiny_skia::Pixmap::new(width as u32, size.height as u32).unwrap();
                let mut mask = tiny_skia::Mask::new(width as u32, size.height as u32).unwrap();
                software.draw(
                    &mut pixels.as_mut(),
                    &mut mask,
                    &iced::advanced::graphics::Viewport::with_physical_size(
                        iced::Size::new(width as u32, size.height as u32),
                        1.0,
                    ),
                    &[Rectangle::with_size(size)],
                    iced::Color::WHITE,
                );
                // Iced's software renderer writes BGRA for the window surface.
                let rgba: Vec<u8> = pixels
                    .data()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[2], p[1], p[0], p[3]])
                    .collect();
                image::save_buffer(
                    path,
                    &rgba,
                    width as u32,
                    size.height as u32,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
    }
    #[test]
    fn recreated_link_overlays_keep_their_shaped_destination() {
        let mut state = State::new(Arc::new(
            Document::parse("<a href='https://example.org/'>link</a>").unwrap(),
        ));
        state.layout(300.0);
        state.hover = Some(Point::new(5.0, 10.0));
        let viewport = Rectangle::with_size(Size::new(300.0, 200.0));
        let mut first = state.overlay(Point::ORIGIN, viewport).unwrap();
        let renderer = crate::test_renderer(Font::DEFAULT, iced::Pixels(14.0));
        let layout = first.as_overlay_mut().layout(&renderer, viewport.size());
        drop(first);
        let second = state.overlay(Point::ORIGIN, viewport).unwrap();
        let preview = state.preview.borrow();
        let (link, _, paragraph) = preview.as_ref().unwrap();
        assert_eq!(link, "https://example.org/");
        assert!(paragraph.min_width() > 0.0);
        assert_eq!(paragraph.buffer().lines[0].text(), link);
        assert!(layout.size().width > paragraph.min_width());
        drop(second);
    }
    #[test]
    fn vertical_cursor_movement_follows_visual_blocks() {
        let mut state = State::new(Arc::new(
            Document::parse("<p>one two</p><p>three four</p>").unwrap(),
        ));
        state.layout(300.0);
        assert_eq!(state.vertical(0, true), 9);
        assert_eq!(state.vertical(9, false), 0);
    }
    #[test]
    fn hidden_spoiler_cannot_copy_or_activate_a_nested_link() {
        let mut state = State::new(Arc::new(
            Document::parse(
                "<p><span data-mx-spoiler><a href='https://example.org/'>secret</a></span></p>",
            )
            .unwrap(),
        ));
        state.layout(300.0);
        assert!(!state.text().contains("secret"));
        let region = state.blocks[0].paragraph.span_bounds(0)[0];
        let point = region.center();
        let mut messages = vec![];
        assert!(state.activate(point, &mut |m| messages.push(m)));
        assert!(messages.is_empty());
        assert_eq!(state.text(), "secret");
        state.layout(300.0);
        let point = state.blocks[0].paragraph.span_bounds(0)[0].center();
        assert!(!state.activate(point, &mut |m| messages.push(m)));
        assert!(
            matches!(&messages[0], Message::OpenMessageLink(url) if url == "https://example.org/")
        );
    }
}
