//! Native formatted text. A single paragraph layout supplies drawing, hit-testing,
//! decorations and selection geometry; the editor stores only logical positions.
use super::{Message, emoji, icons, inline_image, link_preview, shell};
use iced::advanced::{
    Renderer as _, layout, renderer,
    text::{self, Paragraph as _, Renderer as _},
};
use iced::{Font, Point, Rectangle, Size, Vector, font};
use interconnect::message_document::{
    self as document, Block, BlockKind, Document, ImageSource, Inline,
};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    ops::Range,
    sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;

type Paragraph = iced::advanced::graphics::text::Paragraph;
#[derive(Debug)]
struct LaidBlock {
    paragraph: inline_image::Paragraph,
    offsets: Vec<Offset>,
    images: Vec<LaidImage>,
    links: Vec<LaidLink>,
    bounds: Rectangle,
    range: Range<usize>,
    block: Block,
    /// Each shaping span maps back to its semantic run (including emoji font spans).
    runs: Vec<usize>,
    lines: Vec<usize>,
    size: f32,
    code: Option<CodeLayout>,
    table_bounds: Option<Rectangle>,
}
const CODE_LINES: usize = 12;
const CODE_BYTES: usize = 2048;
const CODE_TOP: f32 = 24.0;
const CODE_BUTTON: f32 = 28.0;
const CODE_GUTTER: f32 = 20.0;
const CODE_SCROLLBAR: f32 = 16.0;
const CODE_OVERLAY: f32 = 48.0;
/// Keep the message hover toolbar beside the code toolbar without changing text width.
pub(super) fn code_corner_space(document: Option<&Document>) -> f32 {
    if document
        .and_then(|document| document.blocks.first())
        .is_some_and(|block| matches!(block.kind, BlockKind::Code { .. }))
    {
        CODE_BUTTON * 3.0 + 16.0
    } else {
        0.0
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct CodeView {
    pub expanded: bool,
    pub x: f32,
}
#[derive(Debug)]
struct CodeLayout {
    index: usize,
    truncated: bool,
    controls: Vec<CodeControl>,
    max_x: f32,
    expand_label: Option<Paragraph>,
}
#[derive(Debug)]
struct CodeControl {
    bounds: Rectangle,
    action: CodeAction,
    icon: icons::Kind,
    tooltip: &'static str,
    drawing: icons::Drawing,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct CodeScrollbar {
    pub track: Rectangle,
    pub thumb: Rectangle,
    max_x: f32,
}
#[derive(Debug)]
struct CodeDrag {
    index: usize,
    /// Fraction of thumb width, retained across Iced's intermediate measurements.
    grab: f32,
}
impl LaidBlock {
    fn code_footer(&self) -> f32 {
        if self.code.as_ref().is_some_and(|code| code.max_x > 0.0) {
            CODE_SCROLLBAR
        } else {
            0.0
        }
    }
    fn expand_overlay(&self) -> Option<Rectangle> {
        self.code.as_ref()?.expand_label.as_ref()?;
        Some(Rectangle::new(
            Point::new(
                self.bounds.x - 8.0,
                self.bounds.y + self.bounds.height - CODE_OVERLAY,
            ),
            Size::new(self.bounds.width + 16.0, CODE_OVERLAY),
        ))
    }
    fn paint_bounds(&self) -> Rectangle {
        if let Some(code) = &self.code {
            let top = code
                .controls
                .first()
                .map_or(self.bounds.y, |control| control.bounds.y - 2.0);
            Rectangle::new(
                Point::new(self.bounds.x - 8.0, top),
                Size::new(
                    self.bounds.width + 16.0,
                    self.bounds.y + self.bounds.height + 8.0 + self.code_footer() - top,
                ),
            )
        } else {
            self.table_bounds.unwrap_or(self.bounds)
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CodeAction {
    Toggle,
    Copy,
    Save,
    Scroll,
}
fn preview_end(text: &str) -> usize {
    let mut end = text.len().min(CODE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end]
        .match_indices('\n')
        .nth(CODE_LINES - 1)
        .map_or(end, |(i, _)| i)
}
fn truncate_code(block: &mut Block, mut remaining: usize) {
    for run in &mut block.runs {
        let length = run.content.text().len().min(remaining);
        let text = run.content.text()[..length].to_owned();
        remaining -= length;
        run.content = Inline::Text(text);
    }
}
#[derive(Debug)]
struct Offset {
    shaped: Range<usize>,
    canonical: Range<usize>,
    atomic: bool,
}
#[derive(Debug)]
struct LaidImage {
    source: Option<ImageSource>,
    bounds: Rectangle,
    run: usize,
    canonical: Range<usize>,
    placeholder: Paragraph,
}
#[derive(Debug)]
struct LaidLink {
    chip: link_preview::Chip,
    bounds: Rectangle,
    run: usize,
    canonical: Range<usize>,
}
impl LaidBlock {
    fn canonical(&self, offset: usize) -> usize {
        for mapping in &self.offsets {
            if offset <= mapping.shaped.end {
                if offset <= mapping.shaped.start {
                    return mapping.canonical.start;
                }
                return if mapping.atomic {
                    mapping.canonical.end
                } else {
                    mapping.canonical.start + offset - mapping.shaped.start
                };
            }
        }
        self.range.end - self.range.start
    }
}
#[derive(Debug)]
pub struct State {
    pub document: Arc<Document>,
    revealed: BTreeSet<usize>,
    pub(super) code_views: HashMap<usize, CodeView>,
    code_drag: Option<CodeDrag>,
    pub(super) reflow_requested: bool,
    pub(super) semantics_restored: bool,
    blocks: Vec<LaidBlock>,
    width: Option<f32>,
    text_size: interconnect::settings::MessageTextSize,
    mode: interconnect::settings::ThemeMode,
    image_metrics: Vec<inline_image::Metrics>,
    link_metadata: Option<link_preview::Metadata>,
    summaries: HashMap<String, link_preview::Summary>,
    pub assets: inline_image::Assets,
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
            code_views: HashMap::new(),
            code_drag: None,
            reflow_requested: false,
            semantics_restored: false,
            blocks: Vec::new(),
            width: None,
            text_size: Default::default(),
            mode: interconnect::settings::ThemeMode::Light,
            image_metrics: Vec::new(),
            link_metadata: None,
            summaries: HashMap::new(),
            assets: inline_image::Assets::default(),
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
    /// Lightweight interaction state survives eviction of paragraph/layout state.
    pub(super) fn revealed(&self) -> &BTreeSet<usize> {
        &self.revealed
    }
    pub(super) fn restore_revealed(&mut self, revealed: &BTreeSet<usize>) -> bool {
        if &self.revealed == revealed {
            return false;
        }
        self.revealed.clone_from(revealed);
        self.width = None;
        true
    }
    pub(super) fn restore_code_views(&mut self, views: &HashMap<usize, CodeView>) -> bool {
        if &self.code_views == views {
            return false;
        }
        self.code_views.clone_from(views);
        self.width = None;
        true
    }
    pub fn set_theme(&mut self, mode: interconnect::settings::ThemeMode) {
        if self.mode != mode {
            self.mode = mode;
            self.width = None;
        }
    }
    pub fn layout(&mut self, width: f32) -> layout::Node {
        if self.text_size != self.assets.message_text_size {
            self.text_size = self.assets.message_text_size;
            self.width = None;
        }

        if self
            .link_metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.same(&self.assets.links))
        {
            let summaries: HashMap<_, _> = self
                .document
                .blocks
                .iter()
                .flat_map(|block| &block.runs)
                .filter_map(|run| run.style.link.as_ref())
                .filter_map(|link| {
                    self.assets
                        .links
                        .summary(link)
                        .map(|summary| (link.clone(), summary))
                })
                .collect();
            if self.summaries != summaries {
                self.width = None;
            }
            self.summaries = summaries;
            self.link_metadata = Some(self.assets.links.clone());
        }
        let active_theme = super::appearance::theme(self.mode);
        let theme = &active_theme;
        let mut table_columns: HashMap<usize, usize> = HashMap::new();
        for block in &self.document.blocks {
            if let BlockKind::TableCell(cell) = &block.kind {
                table_columns
                    .entry(cell.table)
                    .and_modify(|count| *count = (*count).max(cell.column + 1))
                    .or_insert(cell.column + 1);
            }
        }
        let mut image_metrics = Vec::new();
        for block in &self.document.blocks {
            let inset = ((block.quote_depth + block.list_depth.saturating_sub(1)).min(8) as f32
                * 14.0)
                .min(width / 3.0);
            let padding = if matches!(block.kind, BlockKind::Code { .. }) {
                16.0
            } else {
                0.0
            };
            for run in &block.runs {
                if run
                    .style
                    .spoilers
                    .iter()
                    .all(|id| self.revealed.contains(id))
                    && let Inline::Image {
                        source,
                        width: w,
                        height: h,
                        descent,
                        ..
                    } = &run.content
                {
                    image_metrics.push(inline_image::Metrics::resolve(
                        *w,
                        *h,
                        *descent,
                        source
                            .as_ref()
                            .and_then(|source| self.assets.intrinsic(source)),
                        (if let BlockKind::TableCell(cell) = &block.kind {
                            (width - inset) / table_columns[&cell.table] as f32 - 16.0
                        } else {
                            width - inset - padding
                        })
                        .max(1.0),
                    ));
                }
            }
        }
        if self.width != Some(width) || self.image_metrics != image_metrics {
            self.image_metrics = image_metrics;
            self.blocks.clear();
            let mut image_index = 0;
            let mut visible = self.document.visible_blocks(&self.revealed);
            // A styled anchor can contain several text runs. Its summary is one
            // object, while its canonical copy text remains the entire anchor label.
            for block in &mut visible {
                let mut merged: Vec<document::Run> = Vec::new();
                for run in std::mem::take(&mut block.runs) {
                    let summary = run
                        .style
                        .link
                        .as_ref()
                        .and_then(|link| self.summaries.get(link));
                    if summary.is_some_and(|summary| summary.replaces(&run))
                        && let Some(previous) = merged.last_mut()
                        && previous.style.link == run.style.link
                        && previous.style.spoilers == run.style.spoilers
                        && summary.is_some_and(|summary| summary.replaces(previous))
                        && let Inline::Text(text) = &mut previous.content
                    {
                        text.push_str(run.content.text());
                    } else {
                        merged.push(run);
                    }
                }
                block.runs = merged;
            }
            let mut y = 0.0;
            for (block_index, (original, range)) in visible
                .iter()
                .zip(document::block_ranges(&visible))
                .enumerate()
            {
                let mut preview_block = original.clone();
                let is_code = matches!(original.kind, BlockKind::Code { .. });
                let source: String = if is_code {
                    original.runs.iter().map(|run| run.content.text()).collect()
                } else {
                    String::new()
                };
                let end = preview_end(&source);
                let truncated = end < source.len();
                let expanded = self
                    .code_views
                    .get(&block_index)
                    .is_some_and(|view| view.expanded);
                if is_code && truncated && !expanded {
                    truncate_code(&mut preview_block, end);
                }
                let block = &preview_block;
                let inset =
                    ((block.quote_depth + block.list_depth.saturating_sub(1)).min(8) as f32 * 14.0)
                        .min(width / 3.0);
                let code = matches!(block.kind, BlockKind::Code { .. });
                let cell = if let BlockKind::TableCell(cell) = &block.kind {
                    Some(*cell)
                } else {
                    None
                };
                let padding = if code || cell.is_some() { 8.0 } else { 0.0 };
                let cell_width =
                    cell.map(|cell| (width - inset) / table_columns[&cell.table] as f32);
                let mut x = inset
                    + padding
                    + cell
                        .zip(cell_width)
                        .map_or(0.0, |(cell, width)| cell.column as f32 * width);
                let mut paragraph_width = cell_width
                    .map_or(width - x - padding, |width| width - 2.0 * padding)
                    .max(1.0);
                if matches!(block.kind, BlockKind::Math)
                    && block
                        .runs
                        .iter()
                        .any(|run| matches!(run.content, Inline::Image { .. }))
                    && let Some(metrics) = self.image_metrics.get(image_index)
                {
                    x += (paragraph_width - metrics.size.width).max(0.0) / 2.0;
                    paragraph_width = metrics.size.width;
                }
                let size = match block.kind {
                    BlockKind::Heading(level) => 23.0 - f32::from(level.min(6)),
                    _ => 14.0,
                } * f32::from(self.text_size.points())
                    / 14.0;
                let mut spans: Vec<text::Span<'_, (), Font>> = Vec::new();
                let mut runs = Vec::new();
                let mut objects = Vec::new();
                let mut object_runs = Vec::new();
                let mut offsets = Vec::new();
                let mut chips = HashMap::new();
                let mut shaped_offset = 0;
                let mut canonical_offset = 0;
                for (i, run) in block.runs.iter().enumerate() {
                    let text_len = run.content.text().len();
                    let image = matches!(run.content, Inline::Image { .. });
                    let chip = run
                        .style
                        .link
                        .as_ref()
                        .and_then(|link| self.summaries.get(link))
                        .filter(|summary| summary.replaces(run))
                        .filter(|_| {
                            run.style
                                .spoilers
                                .iter()
                                .all(|id| self.revealed.contains(id))
                        })
                        .map(|summary| link_preview::Chip::new(summary, paragraph_width));
                    let atomic = image || chip.is_some();
                    let shaped_len = if atomic { "\u{fffc}".len() } else { text_len };
                    offsets.push(Offset {
                        shaped: shaped_offset..shaped_offset + shaped_len,
                        canonical: canonical_offset..canonical_offset + text_len,
                        atomic,
                    });
                    shaped_offset += shaped_len;
                    canonical_offset += text_len;
                    if let Some(chip) = chip {
                        objects.push(inline_image::Object {
                            span: spans.len(),
                            metrics: chip.metrics,
                        });
                        object_runs.push(i);
                        chips.insert(i, chip);
                        spans.push(text::Span::new("\u{fffc}"));
                        runs.push(i);
                        continue;
                    }
                    if image {
                        let metrics = self.image_metrics[image_index];
                        image_index += 1;
                        objects.push(inline_image::Object {
                            span: spans.len(),
                            metrics,
                        });
                        object_runs.push(i);
                        spans.push(text::Span::new("\u{fffc}"));
                        runs.push(i);
                        continue;
                    }
                    let style = &run.style;
                    let base = Font {
                        weight: if style.bold {
                            font::Weight::Bold
                        } else if (style.mention.is_some()
                            || style.room_mention.is_some()
                            || style.room_wide)
                            && !style.code
                        {
                            font::Weight::Semibold
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
                    let color = if style.room_wide {
                        shell::palette(theme).room_wide_accent
                    } else if style.room_mention.is_some() {
                        shell::palette(theme).room_accent
                    } else if style.link.is_some()
                        || style.spoilers.iter().any(|id| !self.revealed.contains(id))
                    {
                        shell::palette(theme).accent
                    } else {
                        shell::palette(theme).ink
                    };
                    // Split at grapheme boundaries so syntax colors never break emoji.
                    let text = run.content.text();
                    let syntax = block.syntax.as_ref().map(|syntax| match self.mode {
                        interconnect::settings::ThemeMode::Light
                        | interconnect::settings::ThemeMode::System => &syntax.light,
                        interconnect::settings::ThemeMode::Dark => &syntax.dark,
                    });
                    let run_start = canonical_offset - text_len;
                    let mut start = 0;
                    let mut previous = None;
                    for (offset, grapheme) in text.grapheme_indices(true) {
                        let foreground = syntax
                            .and_then(|spans| {
                                let position = run_start + offset;
                                let index =
                                    spans.partition_point(|span| span.range.end <= position);
                                spans
                                    .get(index)
                                    .filter(|span| span.range.contains(&position))
                            })
                            .map_or(color, |span| {
                                iced::Color::from_rgb8(span.rgb[0], span.rgb[1], span.rgb[2])
                            });
                        let font =
                            if !grapheme.contains('\u{fe0e}') && emojis::get(grapheme).is_some() {
                                emoji::font().unwrap_or(base)
                            } else {
                                base
                            };
                        if let Some((old_font, old_color)) = previous
                            && (font, foreground) != (old_font, old_color)
                        {
                            spans.push(
                                text::Span::new(&text[start..offset])
                                    .font(old_font)
                                    .color(old_color),
                            );
                            runs.push(i);
                            start = offset;
                        }
                        previous = Some((font, foreground));
                    }
                    if let Some((font, foreground)) = previous {
                        spans.push(text::Span::new(&text[start..]).font(font).color(foreground));
                        runs.push(i);
                    }
                }
                if spans.is_empty() {
                    spans.push(text::Span::new(""));
                    runs.push(0);
                }
                let paragraph = inline_image::Paragraph::with_wrapping(
                    &spans,
                    paragraph_width,
                    size,
                    &objects,
                    if code {
                        text::Wrapping::None
                    } else {
                        text::Wrapping::WordOrGlyph
                    },
                    match cell.map(|cell| cell.alignment) {
                        Some(document::TableAlignment::Center) => text::Alignment::Center,
                        Some(document::TableAlignment::Right) => text::Alignment::Right,
                        Some(document::TableAlignment::Left) => text::Alignment::Left,
                        _ => text::Alignment::Default,
                    },
                );
                let mut images = Vec::new();
                let mut links = Vec::new();
                for (object, run_index) in objects.iter().zip(object_runs) {
                    for line in paragraph.buffer().layout_runs() {
                        if let Some(glyph) = line
                            .glyphs
                            .iter()
                            .find(|glyph| glyph.metadata == object.span)
                        {
                            if let Some(chip) = chips.remove(&run_index) {
                                links.push(LaidLink {
                                    bounds: Rectangle::new(
                                        Point::new(
                                            glyph.x,
                                            line.line_y - object.metrics.size.height
                                                + object.metrics.descent,
                                        ),
                                        object.metrics.size,
                                    ),
                                    chip,
                                    run: run_index,
                                    canonical: offsets[run_index].canonical.clone(),
                                });
                                continue;
                            }
                            let Inline::Image { source, .. } = &block.runs[run_index].content
                            else {
                                continue;
                            };
                            images.push(LaidImage {
                                source: source.clone(),
                                bounds: Rectangle::new(
                                    Point::new(
                                        glyph.x,
                                        line.line_y - object.metrics.size.height
                                            + object.metrics.descent,
                                    ),
                                    object.metrics.size,
                                ),
                                run: run_index,
                                canonical: offsets[run_index].canonical.clone(),
                                placeholder: Paragraph::with_text(text::Text {
                                    content: block.runs[run_index].content.text(),
                                    bounds: object.metrics.size,
                                    size: iced::Pixels(11.0),
                                    line_height: text::LineHeight::default(),
                                    font: Font::with_name("Inter"),
                                    align_x: text::Alignment::Default,
                                    align_y: iced::alignment::Vertical::Top,
                                    shaping: text::Shaping::Advanced,
                                    wrapping: text::Wrapping::WordOrGlyph,
                                }),
                            });
                        }
                    }
                }
                let height = paragraph
                    .min_height()
                    .max(size * 1.45)
                    .max(if code && truncated {
                        CODE_OVERLAY + size * 1.45
                    } else {
                        0.0
                    });
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
                let code_layout = if code {
                    let max_x = (paragraph
                        .buffer()
                        .layout_runs()
                        .map(|line| line.line_w)
                        .fold(0.0_f32, f32::max)
                        - paragraph_width)
                        .max(0.0);
                    if let Some(view) = self.code_views.get_mut(&block_index) {
                        view.x = view.x.clamp(0.0, max_x);
                    }
                    let actions = [
                        truncated.then_some((
                            CodeAction::Toggle,
                            if expanded {
                                icons::Kind::ChevronUp
                            } else {
                                icons::Kind::ChevronDown
                            },
                            if expanded {
                                "Collapse code"
                            } else {
                                "Expand code"
                            },
                        )),
                        Some((CodeAction::Copy, icons::Kind::Copy, "Copy code")),
                        Some((CodeAction::Save, icons::Kind::Download, "Save code")),
                    ];
                    let count = actions.iter().flatten().count();
                    let button_width = CODE_BUTTON.min(paragraph_width / count as f32);
                    let mut control_x = x + paragraph_width - button_width * count as f32;
                    let controls = actions
                        .into_iter()
                        .flatten()
                        .map(|(action, icon, tooltip)| {
                            let bounds = Rectangle::new(
                                Point::new(control_x, y + 2.0),
                                Size::new(button_width, CODE_BUTTON),
                            );
                            control_x += button_width;
                            CodeControl {
                                bounds,
                                action,
                                icon,
                                tooltip,
                                drawing: icons::Drawing::default(),
                            }
                        })
                        .collect();
                    let expand_label = (truncated && !expanded).then(|| {
                        Paragraph::with_text(text::Text {
                            content: "Expand code",
                            bounds: Size::new(paragraph_width, 24.0),
                            size: iced::Pixels(12.0),
                            line_height: text::LineHeight::default(),
                            font: Font {
                                weight: font::Weight::Semibold,
                                ..Font::with_name("Inter")
                            },
                            align_x: text::Alignment::Default,
                            align_y: iced::alignment::Vertical::Top,
                            shaping: text::Shaping::Advanced,
                            wrapping: text::Wrapping::None,
                        })
                    });
                    Some(CodeLayout {
                        expand_label,
                        index: block_index,
                        truncated,
                        controls,
                        max_x,
                    })
                } else {
                    None
                };
                self.blocks.push(LaidBlock {
                    paragraph,
                    offsets,
                    images,
                    links,
                    bounds: Rectangle::new(
                        Point::new(x, y + padding + if code { CODE_TOP } else { 0.0 }),
                        Size::new(paragraph_width, height),
                    ),
                    range,
                    block: block.clone(),
                    runs,
                    lines,
                    size,
                    code: code_layout,
                    table_bounds: None,
                });
                y += height + padding * 2.0 + 8.0 + if code { CODE_TOP } else { 0.0 };
            }
            let mut top = 0.0;
            let mut index = 0;
            while index < self.blocks.len() {
                if let BlockKind::TableCell(cell) = self.blocks[index].block.kind {
                    let end = (index + 1..self.blocks.len()).find(|&next| !matches!(self.blocks[next].block.kind, BlockKind::TableCell(next_cell) if next_cell.table == cell.table && next_cell.row == cell.row)).unwrap_or(self.blocks.len());
                    let row_height = self.blocks[index..end]
                        .iter()
                        .map(|block| block.bounds.height + 16.0)
                        .fold(0.0_f32, f32::max);
                    for block in &mut self.blocks[index..end] {
                        block.bounds.y = top + 8.0;
                        block.table_bounds = Some(Rectangle::new(
                            Point::new(block.bounds.x - 8.0, top),
                            Size::new(block.bounds.width + 16.0, row_height),
                        ));
                    }
                    top += row_height;
                    if !matches!(self.blocks.get(end).map(|block| &block.block.kind), Some(BlockKind::TableCell(next)) if next.table == cell.table)
                    {
                        top += 8.0;
                    }
                    index = end;
                } else {
                    let block = &mut self.blocks[index];
                    let padding = if block.code.is_some() { 8.0 } else { 0.0 };
                    block.bounds.y =
                        top + padding + if block.code.is_some() { CODE_TOP } else { 0.0 };
                    if let Some(code) = &mut block.code {
                        for control in &mut code.controls {
                            control.bounds.y = top + 2.0;
                        }
                    }
                    top =
                        block.bounds.y + block.bounds.height + padding + block.code_footer() + 8.0;
                    index += 1;
                }
            }
            self.width = Some(width);
        }
        let height = self
            .blocks
            .iter()
            .map(|block| {
                block.table_bounds.map_or(
                    block.bounds.y
                        + block.bounds.height
                        + if block.code.is_some() { 8.0 } else { 0.0 }
                        + block.code_footer(),
                    |bounds| bounds.y + bounds.height,
                )
            })
            .fold(0.0_f32, f32::max);
        layout::Node::new(Size::new(width, height))
    }
    pub fn hit(&self, point: Point) -> usize {
        let Some(block) = self
            .blocks
            .iter()
            .find(|b| {
                b.table_bounds
                    .map_or(point.y < b.bounds.y + b.bounds.height, |bounds| {
                        bounds.contains(point)
                    })
            })
            .or(self.blocks.last())
        else {
            return 0;
        };
        if point.y < block.bounds.y {
            return block.range.start;
        }
        let image_point = Point::ORIGIN + (point - block.bounds.position());
        let object = block
            .images
            .iter()
            .map(|image| (image.bounds, image.run, &image.canonical))
            .chain(
                block
                    .links
                    .iter()
                    .map(|link| (link.bounds, link.run, &link.canonical)),
            )
            .find(|(bounds, _, _)| bounds.contains(image_point));
        if let Some((bounds, run, canonical)) = object {
            let rtl = block
                .paragraph
                .buffer()
                .layout_runs()
                .flat_map(|line| line.glyphs)
                .find(|g| block.runs.get(g.metadata) == Some(&run))
                .is_some_and(|g| g.level.is_rtl());
            let after = image_point.x >= bounds.center_x();
            return block.range.start
                + if after != rtl {
                    canonical.end
                } else {
                    canonical.start
                };
        }
        let mut local = point - block.bounds.position();
        if let Some(code) = &block.code {
            local.x += self.code_views.get(&code.index).map_or(0.0, |view| view.x);
        }
        block
            .paragraph
            .buffer()
            .hit(local.x.max(0.0), local.y.max(0.0))
            .map_or(block.range.end, |hit| {
                block.range.start
                    + block.canonical(block.lines.get(hit.line).copied().unwrap_or(0) + hit.index)
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
            let base = block.lines[line.line_i];
            let start = block.range.start
                + block.canonical(line.glyphs.iter().map(|g| g.start).min().unwrap_or(0) + base);
            let end = block.range.start
                + block.canonical(line.glyphs.iter().map(|g| g.end).max().unwrap_or(0) + base);
            start <= offset && offset <= end
        });
        let Some(current) = current else {
            return offset;
        };
        let (block, line) = &lines[current];
        let base = block.lines[line.line_i];
        let x = line
            .glyphs
            .iter()
            .find(|g| {
                block.range.start + block.canonical(base + g.start) <= offset
                    && offset <= block.range.start + block.canonical(base + g.end)
            })
            .map_or(block.bounds.x, |g| {
                let after = offset == block.range.start + block.canonical(base + g.end);
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
        let run = *block.runs.get(span)?;
        if matches!(block.block.runs.get(run)?.content, Inline::Image { .. })
            && !block
                .images
                .iter()
                .any(|image| image.run == run && image.bounds.contains(local))
        {
            return None;
        }
        if block
            .links
            .iter()
            .any(|link| link.run == run && !link.bounds.contains(local))
        {
            return None;
        }
        block.block.runs.get(run)
    }
    pub fn image_requests(
        &self,
        origin: Point,
        viewport: Rectangle,
    ) -> Vec<interconnect::image_media::Key> {
        let mut keys = Vec::new();
        for block in &self.blocks {
            for image in &block.images {
                if let Some(ImageSource::Matrix(uri)) = &image.source
                    && (image.bounds
                        + (origin - Point::ORIGIN)
                        + (block.bounds.position() - Point::ORIGIN))
                        .intersects(&viewport)
                {
                    let key = interconnect::image_media::Key::inline(uri.clone());
                    if !keys.contains(&key)
                        && keys.len() < interconnect::image_media::MAX_INLINE_IMAGES
                    {
                        keys.push(key);
                    }
                }
            }
        }
        keys
    }
    pub fn link_at(&self, point: Point) -> Option<&str> {
        let run = self.run_at(point)?;
        run.style
            .spoilers
            .iter()
            .all(|id| self.revealed.contains(id))
            .then_some(run.style.link.as_deref())
            .flatten()
    }
    pub(super) fn code_control(&self, point: Point) -> Option<(usize, CodeAction)> {
        self.blocks.iter().find_map(|block| {
            let code = block.code.as_ref()?;
            if self
                .scrollbar(block)
                .is_some_and(|bar| bar.track.contains(point))
            {
                return Some((code.index, CodeAction::Scroll));
            }
            if block
                .expand_overlay()
                .is_some_and(|bounds| bounds.contains(point))
            {
                return Some((code.index, CodeAction::Toggle));
            }
            code.controls
                .iter()
                .find(|control| control.bounds.contains(point))
                .map(|control| (code.index, control.action))
        })
    }
    fn code_tooltip(&self, point: Point) -> Option<&'static str> {
        self.blocks
            .iter()
            .filter_map(|block| block.code.as_ref())
            .flat_map(|code| &code.controls)
            .find(|control| control.bounds.contains(point))
            .map(|control| control.tooltip)
    }
    #[cfg(test)]
    pub(super) fn code_button(&self, index: usize, action: CodeAction) -> Option<Rectangle> {
        self.blocks.iter().find_map(|block| {
            let code = block.code.as_ref().filter(|code| code.index == index)?;
            code.controls
                .iter()
                .find(|control| control.action == action)
                .map(|control| control.bounds)
        })
    }
    pub(super) fn code_scrollbar(&self, index: usize) -> Option<CodeScrollbar> {
        let block = self
            .blocks
            .iter()
            .find(|block| block.code.as_ref().is_some_and(|code| code.index == index))?;
        self.scrollbar(block)
    }
    fn scrollbar(&self, block: &LaidBlock) -> Option<CodeScrollbar> {
        let code = block.code.as_ref()?;
        let index = code.index;
        if code.max_x <= 0.0 {
            return None;
        }
        let track = Rectangle::new(
            Point::new(block.bounds.x, block.bounds.y + block.bounds.height),
            Size::new(block.bounds.width, CODE_SCROLLBAR),
        );
        let thumb_width = (track.width * track.width / (track.width + code.max_x))
            .clamp(24.0_f32.min(track.width), track.width);
        let offset = self
            .code_views
            .get(&index)
            .map_or(0.0, |view| view.x)
            .clamp(0.0, code.max_x);
        let thumb = Rectangle::new(
            Point::new(
                track.x + (track.width - thumb_width) * offset / code.max_x,
                track.y,
            ),
            Size::new(thumb_width, track.height),
        );
        Some(CodeScrollbar {
            track,
            thumb,
            max_x: code.max_x,
        })
    }
    pub(super) fn begin_code_scroll(&mut self, index: usize, point: Point) {
        let Some(bar) = self.code_scrollbar(index) else {
            return;
        };
        let grab = if bar.thumb.contains(point) {
            ((point.x - bar.thumb.x) / bar.thumb.width).clamp(0.0, 1.0)
        } else {
            0.5
        };
        self.code_drag = Some(CodeDrag { index, grab });
        self.drag_code_scroll(point);
    }
    pub(super) fn drag_code_scroll(&mut self, point: Point) -> bool {
        let Some(drag) = &self.code_drag else {
            return false;
        };
        let index = drag.index;
        let Some(bar) = self.code_scrollbar(index) else {
            self.code_drag = None;
            return false;
        };
        let travel = bar.track.width - bar.thumb.width;
        let offset = if travel > 0.0 {
            ((point.x - bar.track.x - drag.grab * bar.thumb.width) / travel).clamp(0.0, 1.0)
                * bar.max_x
        } else {
            0.0
        };
        self.code_views.entry(index).or_default().x = offset;
        true
    }
    pub(super) fn finish_code_scroll(&mut self) -> bool {
        self.code_drag.take().is_some()
    }
    pub(super) fn dragging_code(&self) -> bool {
        self.code_drag
            .as_ref()
            .is_some_and(|drag| self.code_scrollbar(drag.index).is_some())
    }
    pub(super) fn code_mouse_interaction(
        &self,
        point: Point,
    ) -> Option<iced::advanced::mouse::Interaction> {
        use iced::advanced::mouse::Interaction;
        if self.dragging_code() {
            return Some(Interaction::Grabbing);
        }
        let (index, action) = self.code_control(point)?;
        Some(
            if matches!(action, CodeAction::Scroll)
                && self
                    .code_scrollbar(index)
                    .is_some_and(|bar| bar.thumb.contains(point))
            {
                Interaction::Grab
            } else {
                Interaction::Pointer
            },
        )
    }
    pub(super) fn code_text(&self, index: usize) -> String {
        self.document
            .visible_blocks(&self.revealed)
            .get(index)
            .map(|block| block.runs.iter().map(|run| run.content.text()).collect())
            .unwrap_or_default()
    }
    pub(super) fn code_action(&mut self, index: usize, action: CodeAction) {
        let Some(code) = self
            .blocks
            .iter()
            .filter_map(|block| block.code.as_ref())
            .find(|code| code.index == index)
        else {
            return;
        };
        let view = self.code_views.entry(index).or_default();
        match action {
            CodeAction::Toggle if code.truncated => {
                view.expanded = !view.expanded;
                self.width = None;
                self.reflow_requested = true;
            }
            _ => {}
        }
    }
    pub(super) fn scroll_code(&mut self, point: Point, delta: f32) -> bool {
        let Some(code) = self
            .blocks
            .iter()
            .find(|block| {
                block.bounds.contains(point)
                    || self
                        .scrollbar(block)
                        .is_some_and(|bar| bar.track.contains(point))
            })
            .and_then(|block| block.code.as_ref())
        else {
            return false;
        };
        if code.max_x <= 0.0 || delta == 0.0 {
            return false;
        }
        let view = self.code_views.entry(code.index).or_default();
        view.x = (view.x - delta).clamp(0.0, code.max_x);
        true
    }
    pub fn interactive(&self, point: Point) -> bool {
        if self.code_control(point).is_some() {
            return true;
        }
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
        if let Some(user) = &run.style.mention {
            publish(Message::NewChat(crate::new_chat::Event::Mention(
                user.clone(),
            )));
        } else if let Some(room) = &run.style.room_mention {
            publish(Message::Membership(crate::membership::Event::Link(
                room.clone(),
            )));
        } else if let Some(link) = &run.style.link {
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
        let label = self
            .code_tooltip(point)
            .or_else(|| self.run_at(point)?.style.link.as_deref())?;
        let width = (viewport.width - 16.0).clamp(1.0, 500.0);
        let mut preview = self.preview.borrow_mut();
        if preview
            .as_ref()
            .is_none_or(|(cached, cached_width, _)| cached != label || *cached_width != width)
        {
            let paragraph = Paragraph::with_text(text::Text {
                content: label,
                bounds: Size::new(width, f32::INFINITY),
                size: iced::Pixels(12.0),
                line_height: text::LineHeight::default(),
                font: Font::with_name("Inter"),
                align_x: text::Alignment::Default,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::WordOrGlyph,
            });
            *preview = Some((label.to_owned(), width, paragraph));
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
        search: Option<&str>,
    ) {
        let active_theme = super::appearance::theme(self.mode);
        let theme = &active_theme;
        let highlights = search
            .map(|query| super::room_search::matching_ranges(&self.text(), query))
            .unwrap_or_default();
        for block in &self.blocks {
            let translation = origin - Point::ORIGIN;
            let bounds = block.bounds + translation;
            if !(block.paint_bounds() + translation).intersects(&viewport) {
                continue;
            }
            let code = matches!(block.block.kind, BlockKind::Code { .. });
            if code {
                quad(
                    renderer,
                    Rectangle::new(
                        Point::new(origin.x, bounds.y - CODE_GUTTER),
                        Size::new(
                            self.width.unwrap_or(bounds.width),
                            bounds.height + CODE_GUTTER + 8.0 + block.code_footer(),
                        ),
                    ),
                    shell::palette(theme).code,
                );
            }
            if let Some(code) = &block.code
                && let (Some(first), Some(last)) = (code.controls.first(), code.controls.last())
            {
                let palette = shell::palette(theme);
                let panel = Rectangle::new(
                    Point::new(first.bounds.x - 2.0, first.bounds.y - 2.0),
                    Size::new(
                        last.bounds.x + last.bounds.width - first.bounds.x + 4.0,
                        CODE_BUTTON + 4.0,
                    ),
                ) + translation;
                if let Some(clip) = panel.intersection(&viewport) {
                    renderer.with_layer(clip, |renderer| {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: panel,
                                border: iced::Border {
                                    color: palette.muted.scale_alpha(0.18),
                                    width: 1.0,
                                    radius: 6.0.into(),
                                },
                                ..Default::default()
                            },
                            palette.rail.scale_alpha(0.96),
                        );
                        for control in &code.controls {
                            let bounds = control.bounds + translation;
                            let hovered = self
                                .hover
                                .is_some_and(|point| control.bounds.contains(point));
                            if hovered {
                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds,
                                        border: iced::Border::default().rounded(4.0),
                                        ..Default::default()
                                    },
                                    palette.accent.scale_alpha(0.10),
                                );
                            }
                            let size = 16.0_f32.min((bounds.width - 6.0).max(1.0));
                            let icon = Rectangle::new(
                                Point::new(
                                    bounds.center_x() - size / 2.0,
                                    bounds.center_y() - size / 2.0,
                                ),
                                Size::new(size, size),
                            );
                            control.drawing.draw(
                                renderer,
                                control.icon,
                                icon,
                                if hovered {
                                    palette.accent
                                } else {
                                    palette.muted
                                },
                            );
                        }
                    });
                }
            }
            if let Some(cell_bounds) = block.table_bounds {
                let cell_bounds = cell_bounds + translation;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: cell_bounds,
                        border: iced::Border {
                            color: shell::palette(theme).muted.scale_alpha(0.25),
                            width: 1.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    if matches!(block.block.kind, BlockKind::TableCell(cell) if cell.header) {
                        shell::palette(theme).code
                    } else {
                        iced::Color::TRANSPARENT
                    },
                );
            }
            let clip = bounds
                .intersection(&viewport)
                .unwrap_or(Rectangle::with_size(Size::ZERO));
            let bounds = if let Some(code) = &block.code {
                bounds
                    - Vector::new(
                        self.code_views.get(&code.index).map_or(0.0, |view| view.x),
                        0.0,
                    )
            } else {
                bounds
            };
            renderer.with_layer(clip, |renderer| {
                if block.block.quote_depth > 0 {
                    quad(
                        renderer,
                        Rectangle::new(
                            Point::new(bounds.x - 9.0, bounds.y),
                            Size::new(2.0, bounds.height),
                        ),
                        shell::palette(theme).muted,
                    );
                }
                for (index, run_index) in block.runs.iter().enumerate() {
                    let Some(run) = block.block.runs.get(*run_index) else {
                        continue;
                    };
                    if matches!(run.content, Inline::Image { .. })
                        || block.links.iter().any(|link| link.run == *run_index)
                    {
                        continue;
                    }
                    let style = &run.style;
                    for region in block.paragraph.span_bounds(index) {
                        let region = region + (bounds.position() - Point::ORIGIN);
                        if style.code && !code {
                            quad(renderer, region, shell::palette(theme).inline_code);
                        }
                        // Keep the background inside the shaped span so wrapping, bidi,
                        // selection and hit testing all use exactly the same geometry.
                        let mention = (style.mention.is_some()
                            || style.room_mention.is_some()
                            || style.room_wide)
                            && !style.code;
                        if mention {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: region,
                                    border: iced::Border::default().rounded(4.0),
                                    ..renderer::Quad::default()
                                },
                                if style.room_wide {
                                    shell::palette(theme).room_wide_accent.scale_alpha(0.12)
                                } else if style.room_mention.is_some() {
                                    shell::palette(theme).room_accent.scale_alpha(0.12)
                                } else {
                                    shell::palette(theme).mention
                                },
                            );
                        }
                        if style.underline || (style.link.is_some() && !mention) {
                            quad(
                                renderer,
                                Rectangle::new(
                                    Point::new(region.x, region.y + block.size * 1.2),
                                    Size::new(region.width, 1.0),
                                ),
                                if style.link.is_some() {
                                    shell::palette(theme).accent
                                } else {
                                    shell::palette(theme).ink
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
                                shell::palette(theme).ink,
                            );
                        }
                    }
                }
                // Glyph clusters supply byte ranges and visual rectangles, including bidi runs.
                for line in block.paragraph.buffer().layout_runs() {
                    let line_start = block.lines[line.line_i];
                    for glyph in line.glyphs {
                        if block.runs.get(glyph.metadata).is_some_and(|run| {
                            matches!(block.block.runs[*run].content, Inline::Image { .. })
                                || block.links.iter().any(|link| link.run == *run)
                        }) {
                            continue;
                        }
                        let glyph_start =
                            block.range.start + block.canonical(line_start + glyph.start);
                        let glyph_end = block.range.start + block.canonical(line_start + glyph.end);
                        // Logical order can differ from visual order in bidi text.
                        let first = highlights.partition_point(|range| range.end <= glyph_start);
                        for (selected, color) in highlights[first..]
                            .iter()
                            .take_while(|range| range.start < glyph_end)
                            .map(|range| (range, shell::palette(theme).search))
                            .chain(
                                selection
                                    .iter()
                                    .filter(|range| {
                                        range.start < glyph_end && range.end > glyph_start
                                    })
                                    .map(|range| (range, shell::palette(theme).selection)),
                            )
                        {
                            let cluster = &line.text[glyph.start..glyph.end];
                            let (left, width) = highlight_fraction(
                                cluster,
                                glyph_start,
                                selected,
                                glyph.level.is_rtl(),
                            );
                            quad(
                                renderer,
                                Rectangle::new(
                                    Point::new(
                                        bounds.x + glyph.x + left * glyph.w,
                                        bounds.y + line.line_top,
                                    ),
                                    Size::new(width * glyph.w, line.line_height),
                                ),
                                color,
                            );
                        }
                    }
                }
                block.paragraph.draw(
                    renderer,
                    bounds.position(),
                    viewport,
                    shell::palette(theme).ink,
                );
                for link in &block.links {
                    let selected = selection.iter().chain(&highlights).any(|range| {
                        block.range.start + link.canonical.start < range.end
                            && block.range.start + link.canonical.end > range.start
                    });
                    link.chip.draw(
                        renderer,
                        link.bounds + (bounds.position() - Point::ORIGIN),
                        viewport,
                        theme,
                        selected,
                    );
                }
                for image in &block.images {
                    let image_bounds = image.bounds + (bounds.position() - Point::ORIGIN);
                    if !image_bounds.intersects(&viewport) {
                        continue;
                    }
                    let handle = image
                        .source
                        .as_ref()
                        .and_then(|source| self.assets.handle(source));
                    if let Some(handle) = handle {
                        // Formula bitmaps have their own ink and may contain authored colors.
                        // Keep a light backing instead of modifying image pixels on the UI thread.
                        if matches!(image.source, Some(ImageSource::Formula(_))) {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: image_bounds,
                                    border: iced::Border::default().rounded(3),
                                    ..Default::default()
                                },
                                shell::palette(theme).formula_surface,
                            );
                        }
                        use iced::advanced::image::Renderer as _;
                        let mut drawing = iced::advanced::image::Image::new(handle.clone());
                        drawing.snap = false;
                        renderer.draw_image(drawing, image_bounds, viewport);
                    } else {
                        let failed = image
                            .source
                            .as_ref()
                            .is_none_or(|source| self.assets.failed(source));
                        quad(
                            renderer,
                            image_bounds,
                            if failed {
                                shell::palette(theme).error_surface
                            } else {
                                shell::palette(theme).rail
                            },
                        );
                        if let Some(clip) = image_bounds.intersection(&viewport) {
                            renderer.fill_paragraph(
                                &image.placeholder,
                                image_bounds.position(),
                                shell::palette(theme).muted,
                                clip,
                            );
                        }
                    }
                    if selection.as_ref().is_some_and(|range| {
                        block.range.start + image.canonical.start < range.end
                            && block.range.start + image.canonical.end > range.start
                    }) {
                        quad(renderer, image_bounds, shell::palette(theme).selection);
                    }
                }
            });
            if let Some(code) = &block.code {
                if let Some(overlay) = block.expand_overlay() {
                    let overlay = overlay + translation;
                    if let Some(clip) = overlay.intersection(&viewport) {
                        renderer.with_layer(clip, |renderer| {
                            let palette = shell::palette(theme);
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: overlay,
                                    ..Default::default()
                                },
                                iced::gradient::Linear::new(iced::Radians::PI / 2.0)
                                    .add_stop(0.0, palette.code.scale_alpha(0.0))
                                    .add_stop(0.45, palette.code.scale_alpha(0.85))
                                    .add_stop(1.0, palette.code.scale_alpha(0.98)),
                            );
                            let width = 112.0_f32.min((overlay.width - 12.0).max(1.0));
                            let pill = Rectangle::new(
                                Point::new(
                                    overlay.center_x() - width / 2.0,
                                    overlay.y + overlay.height - 30.0,
                                ),
                                Size::new(width, 24.0),
                            );
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: pill,
                                    border: iced::Border {
                                        color: palette.accent.scale_alpha(0.22),
                                        width: 1.0,
                                        radius: 12.0.into(),
                                    },
                                    ..Default::default()
                                },
                                palette.code.scale_alpha(0.92),
                            );
                            if let Some(label) = &code.expand_label {
                                renderer.fill_paragraph(
                                    label,
                                    Point::new(
                                        pill.center_x() - label.min_width() / 2.0,
                                        pill.y + 4.0,
                                    ),
                                    palette.accent,
                                    clip,
                                );
                            }
                        });
                    }
                }
                if let Some(bar) = self.scrollbar(block)
                    && let Some(clip) = (bar.track + translation).intersection(&viewport)
                {
                    renderer.with_layer(clip, |renderer| {
                        let rail = Rectangle::new(
                            Point::new(bar.track.x, bar.track.y + 5.0),
                            Size::new(bar.track.width, 6.0),
                        ) + translation;
                        let thumb = Rectangle::new(
                            Point::new(bar.thumb.x, bar.thumb.y + 4.0),
                            Size::new(bar.thumb.width, 8.0),
                        ) + translation;
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: rail,
                                border: iced::Border::default().rounded(3.0),
                                ..Default::default()
                            },
                            shell::palette(theme).muted.scale_alpha(0.15),
                        );
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: thumb,
                                border: iced::Border::default().rounded(4.0),
                                ..Default::default()
                            },
                            shell::palette(theme).muted.scale_alpha(0.65),
                        );
                    });
                }
            }
        }
    }
}
/// Preserve partial ligatures, complete graphemes, and RTL visual direction.
pub(super) fn highlight_fraction(
    cluster: &str,
    start: usize,
    selected: &Range<usize>,
    rtl: bool,
) -> (f32, f32) {
    let graphemes: Vec<_> = cluster.grapheme_indices(true).collect();
    let count = graphemes.len().max(1) as f32;
    let from = graphemes
        .iter()
        .take_while(|(i, g)| start + i + g.len() <= selected.start)
        .count() as f32
        / count;
    let to = graphemes
        .iter()
        .take_while(|(i, _)| start + i < selected.end)
        .count() as f32
        / count;
    (if rtl { 1.0 - to } else { from }, to - from)
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
        theme: &iced::Theme,
        _: &renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _: iced::advanced::mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        quad(renderer, bounds, shell::palette(theme).rail);
        renderer.fill_paragraph(
            &self.paragraph,
            bounds.position() + Vector::new(6.0, 4.0),
            shell::palette(theme).ink,
            bounds,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_corner_icons_have_tooltips_and_do_not_cover_text_or_change_copy() {
        let document = Arc::new(
            Document::parse(&format!(
                "<pre><code>short</code></pre><pre><code>{}</code></pre>",
                "let café = 42;\n".repeat(30)
            ))
            .unwrap(),
        );
        let mut state = State::new(document);
        let text = state.text();
        for width in [110.0, 260.0, 600.0] {
            state.layout(width);
            assert_eq!(state.blocks[0].code.as_ref().unwrap().controls.len(), 2);
            assert_eq!(state.blocks[1].code.as_ref().unwrap().controls.len(), 3);
            assert!(state.code_button(0, CodeAction::Toggle).is_none());
            for block in &state.blocks {
                let code = block.code.as_ref().unwrap();
                let last = code.controls.last().unwrap();
                assert!(
                    (last.bounds.x + last.bounds.width - block.bounds.x - block.bounds.width).abs()
                        < 0.1
                );
                for control in &code.controls {
                    assert!(control.bounds.x >= block.bounds.x);
                    assert!(control.bounds.y + control.bounds.height <= block.bounds.y);
                    assert_eq!(
                        state.code_control(control.bounds.center()),
                        Some((code.index, control.action))
                    );
                    assert_eq!(
                        state.code_tooltip(control.bounds.center()),
                        Some(control.tooltip)
                    );
                }
            }
            let point = state.code_button(1, CodeAction::Copy).unwrap().center();
            state.hover = Some(point);
            assert!(
                state
                    .overlay(
                        Point::ORIGIN,
                        Rectangle::with_size(Size::new(width, 1000.0))
                    )
                    .is_some()
            );
            assert_eq!(state.text(), text);
        }
        let point = state.code_button(1, CodeAction::Toggle).unwrap().center();
        assert_eq!(state.code_tooltip(point), Some("Expand code"));
        state.code_action(1, CodeAction::Toggle);
        state.layout(600.0);
        let point = state.code_button(1, CodeAction::Toggle).unwrap().center();
        assert_eq!(state.code_tooltip(point), Some("Collapse code"));
        assert_eq!(state.text(), text);
    }
    #[test]
    fn code_scrollbars_track_overflow_resize_and_expansion_without_changing_source() {
        let source = format!("{}{}", "short\n".repeat(12), "wide_column ".repeat(300));
        let mut state = State::new(Arc::new(
            Document::parse(&format!("<pre><code>{source}</code></pre>")).unwrap(),
        ));
        state.layout(180.0);
        assert!(
            state.code_scrollbar(0).is_none(),
            "collapsed visible lines fit"
        );
        let overlay = state.blocks[0].expand_overlay().unwrap();
        assert!(matches!(
            state.code_control(overlay.center()),
            Some((0, CodeAction::Toggle))
        ));
        state.code_action(0, CodeAction::Toggle);
        let expanded = state.layout(180.0).bounds();
        assert!(state.blocks[0].expand_overlay().is_none());
        let bar = state.code_scrollbar(0).unwrap();
        assert!(bar.thumb.width >= 24.0 && bar.thumb.width < bar.track.width);
        assert_eq!(bar.thumb.x, bar.track.x);
        assert!(expanded.height >= bar.track.y + bar.track.height);
        assert!(matches!(
            state.code_control(bar.thumb.center()),
            Some((0, CodeAction::Scroll))
        ));
        state.begin_code_scroll(0, bar.thumb.center());
        state.drag_code_scroll(Point::new(
            bar.track.x + bar.track.width + 100.0,
            bar.track.y,
        ));
        assert_eq!(state.code_views[&0].x, bar.max_x);
        let right = state.code_scrollbar(0).unwrap();
        assert!(
            (right.thumb.x + right.thumb.width - right.track.x - right.track.width).abs() < 0.1
        );
        state.drag_code_scroll(Point::new(bar.track.x - 100.0, bar.track.y));
        assert_eq!(state.code_views[&0].x, 0.0);
        assert!(state.finish_code_scroll());
        assert!(!state.drag_code_scroll(bar.thumb.center()));
        state.begin_code_scroll(
            0,
            Point::new(bar.track.x + bar.track.width / 2.0, bar.track.center_y()),
        );
        assert!(state.code_views[&0].x > 0.0, "track presses move the thumb");
        // Iced may measure at several widths before committing a row layout.
        // Preserve a captured grab while recomputing its current thumb geometry.
        state.layout(250.0);
        assert!(state.dragging_code());
        let resized = state.code_scrollbar(0).unwrap();
        state.drag_code_scroll(Point::new(
            resized.track.x + resized.track.width + 100.0,
            resized.track.y,
        ));
        assert_eq!(state.code_views[&0].x, resized.max_x);
        state.layout(100_000.0);
        assert!(!state.dragging_code(), "resizing cancels captured geometry");
        assert!(
            state.code_scrollbar(0).is_none(),
            "fitting code has no scrollbar"
        );
        assert_eq!(state.code_views[&0].x, 0.0);
        assert_eq!(state.code_text(0), source);
        state.code_action(0, CodeAction::Toggle);
        state.layout(180.0);
        assert!(state.blocks[0].expand_overlay().is_some());
        assert!(state.code_scrollbar(0).is_none());
        assert_eq!(state.text(), source);
        let mut short = State::new(Arc::new(
            Document::parse("<pre><code>short</code></pre>").unwrap(),
        ));
        short.layout(180.0);
        assert!(short.blocks[0].expand_overlay().is_none());
        assert!(short.code_scrollbar(0).is_none());
    }
    #[test]
    fn table_grid_wraps_cells_shares_row_heights_and_keeps_unicode_hit_offsets() {
        let document = Arc::new(Document::parse("<p>Before</p><table><tr><th>Name</th><th align='right'>Count</th></tr><tr><td>A long café 👩🏽‍💻 label that wraps across lines</td><td align='right'><a href='https://example.org'>42</a></td></tr><tr><td></td><td>done</td></tr></table><p>After</p>").unwrap());
        let mut state = State::new(document);
        let canonical = state.text();
        for width in [640.0, 280.0, 160.0] {
            state.layout(width);
            let cells: Vec<_> = state
                .blocks
                .iter()
                .filter(|block| block.table_bounds.is_some())
                .collect();
            assert_eq!(cells.len(), 6);
            for pair in cells.chunks(2) {
                assert_eq!(
                    pair[0].table_bounds.unwrap().height,
                    pair[1].table_bounds.unwrap().height
                );
                assert_eq!(pair[0].bounds.y, pair[1].bounds.y);
                assert!((pair[0].table_bounds.unwrap().width - width / 2.0).abs() < 0.1);
                assert!(
                    pair[0].table_bounds.unwrap().x + pair[0].table_bounds.unwrap().width
                        <= pair[1].table_bounds.unwrap().x + 0.1
                );
            }
            for cell in &cells {
                for line in cell.paragraph.buffer().layout_runs() {
                    if matches!(cell.block.kind, BlockKind::TableCell(metadata) if metadata.alignment == document::TableAlignment::Right)
                        && let Some(glyph) = line.glyphs.last()
                    {
                        assert!(
                            (glyph.x + glyph.w - cell.bounds.width).abs() < 1.0,
                            "right alignment must use the full cell width"
                        );
                    }
                    for glyph in line.glyphs {
                        let point = cell.bounds.position()
                            + Vector::new(
                                glyph.x + glyph.w / 2.0,
                                line.line_top + line.line_height / 2.0,
                            );
                        let hit = state.hit(point);
                        assert!(canonical.is_char_boundary(hit));
                        assert!(cell.range.start <= hit && hit <= cell.range.end);
                    }
                }
            }
            assert!(
                state.blocks.last().unwrap().bounds.y
                    >= cells.last().unwrap().table_bounds.unwrap().y
                        + cells.last().unwrap().table_bounds.unwrap().height
            );
            assert_eq!(state.text(), canonical);
            let mut renderer = crate::test_renderer(Font::DEFAULT, iced::Pixels(14.0));
            state.draw(
                &mut renderer,
                Point::ORIGIN,
                Rectangle::with_size(Size::new(width, 600.0)),
                Some(0..canonical.len()),
                None,
            );
        }
    }
    #[test]
    fn code_preview_bounds_shaping_preserves_full_copy_and_releases_expanded_layout() {
        for source in [
            "let café = 1; // 👩🏽‍💻\n".repeat(1000),
            "wide_café ".repeat(3000),
        ] {
            let document = Arc::new(
                Document::parse(&format!("<pre><code>{source}</code></pre><p>After</p>")).unwrap(),
            );
            let mut state = State::new(document);
            let collapsed = state.layout(280.0).bounds();
            let block = &state.blocks[0];
            assert!(
                block
                    .paragraph
                    .buffer()
                    .lines
                    .iter()
                    .map(|line| line.text().len())
                    .sum::<usize>()
                    <= CODE_BYTES
            );
            assert!(block.paragraph.buffer().layout_runs().count() <= CODE_LINES);
            assert_eq!(state.code_text(0), source);
            let canonical = state.text();
            assert_eq!(state.blocks[1].range.start, source.len() + 2);
            state.code_action(0, CodeAction::Toggle);
            state.layout(280.0);
            assert_eq!(state.code_text(0), source);
            assert_eq!(state.text(), canonical);
            assert_eq!(state.blocks[1].range.start, source.len() + 2);
            state.code_action(0, CodeAction::Toggle);
            assert_eq!(state.layout(280.0).bounds(), collapsed);
            assert!(
                state.blocks[0]
                    .paragraph
                    .buffer()
                    .lines
                    .iter()
                    .map(|line| line.text().len())
                    .sum::<usize>()
                    <= CODE_BYTES
            );
        }
    }
    #[test]
    fn wide_code_scrolls_without_wrapping_and_hit_testing_uses_the_scroll_offset() {
        let mut state = State::new(Arc::new(
            Document::parse("<pre><code>0123456789abcdefghijklmnopqrstuvwxyz</code></pre>")
                .unwrap(),
        ));
        state.layout(120.0);
        let block = &state.blocks[0];
        assert_eq!(block.paragraph.buffer().layout_runs().count(), 1);
        let point = block.bounds.position() + Vector::new(20.0, 8.0);
        let before = state.hit(point);
        assert!(state.scroll_code(point, -80.0));
        assert!(state.hit(point) > before);
        assert!(!state.scroll_code(point, 0.0));
        assert_eq!(state.code_text(0), "0123456789abcdefghijklmnopqrstuvwxyz");
    }
    #[test]
    fn matrix_link_chips_preserve_source_copy_hit_targets_spoilers_and_metadata_updates() {
        let event = "https://matrix.to/#/!design:example.org/$message?via=example.org";
        let room = "https://matrix.to/#/%23design%3Aexample.org";
        let user = "https://matrix.to/#/@maya:example.org";
        let document = Arc::new(Document::parse(&format!(
            "<p>Read <a href='{event}'><b>the </b>message</a> please.</p><p><a href='{room}'>{room}</a> and <a href='{user}'>{user}</a></p><p><span data-mx-spoiler><a href='{event}'>secret</a></span></p><pre><code>{event}</code></pre>"
        )).unwrap());
        for width in [100.0, 320.0] {
            let mut state = State::new(document.clone());
            state.assets.links.rooms = Arc::new(vec![interconnect::navigation::RoomEntry {
                id: "!design:example.org".into(),
                name: "Design notes".into(),
                canonical_alias: Some("#design:example.org".into()),
                ..Default::default()
            }]);
            let canonical = state.text();
            let first = state.layout(width).bounds();
            assert_eq!(
                state
                    .blocks
                    .iter()
                    .map(|block| block.links.len())
                    .sum::<usize>(),
                3
            );
            assert!(state.blocks[2].links.is_empty());
            assert!(state.blocks[3].links.is_empty());
            for block in &state.blocks {
                for link in &block.links {
                    assert!(link.bounds.width <= width);
                    let point = link.bounds.center() + (block.bounds.position() - Point::ORIGIN);
                    let target = state.link_at(point).unwrap();
                    assert!([event, room, user].contains(&target));
                    let hit = state.hit(point);
                    assert!(
                        hit == block.range.start + link.canonical.start
                            || hit == block.range.start + link.canonical.end
                    );
                }
            }
            let chip = &state.blocks[0].links[0];
            let point = chip.bounds.center() + (state.blocks[0].bounds.position() - Point::ORIGIN);
            let mut messages = Vec::new();
            assert!(!state.activate(point, &mut |message| messages.push(message)));
            assert!(matches!(&messages[..], [Message::OpenMessageLink(url)] if url == event));
            let buffer = state.blocks[0].paragraph.buffer() as *const _;
            assert_eq!(state.layout(width).bounds(), first);
            assert_eq!(
                state.blocks[0].paragraph.buffer() as *const _,
                buffer,
                "unchanged metadata does not reshape"
            );
            assert_eq!(state.text(), canonical);
            Arc::make_mut(&mut state.assets.links.rooms)[0].name = "Renamed café 👩🏽‍💻 العربية".into();
            state.layout(width);
            assert_eq!(
                state.summaries[event].label,
                "Message in Renamed café 👩🏽‍💻 العربية"
            );
            assert_eq!(state.text(), canonical);
            state.set_theme(interconnect::settings::ThemeMode::Dark);
            let size = Size::new(width, state.layout(width).bounds().height.ceil());
            let mut renderer = crate::test_renderer(Font::with_name("Inter"), iced::Pixels(14.0));
            renderer.reset(Rectangle::with_size(size));
            state.draw(
                &mut renderer,
                Point::ORIGIN,
                Rectangle::with_size(size),
                Some(0..canonical.len()),
                None,
            );
            assert_eq!(
                state.layout(width).bounds().height.ceil(),
                size.height,
                "draw keeps layout stable"
            );
            if let Ok(path) = std::env::var("INTERCONNECT_LINK_PREVIEW")
                && width == 320.0
            {
                renderer.reset(Rectangle::with_size(size));
                state.draw(
                    &mut renderer,
                    Point::ORIGIN,
                    Rectangle::with_size(size),
                    None,
                    None,
                );
                let iced::Renderer::Secondary(software) = &mut renderer else {
                    panic!("software renderer");
                };
                let mut pixels = tiny_skia::Pixmap::new(width as u32, size.height as u32).unwrap();
                let mut mask = tiny_skia::Mask::new(width as u32, size.height as u32).unwrap();
                software.draw(
                    &mut pixels.as_mut(),
                    &mut mask,
                    &iced::advanced::graphics::Viewport::with_physical_size(
                        Size::new(width as u32, size.height as u32),
                        1.0,
                    ),
                    &[Rectangle::with_size(size)],
                    iced::Color::from_rgb8(24, 26, 28),
                );
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
            let spoiler = &state.blocks[2];
            let point = spoiler.paragraph.span_bounds(0)[0].center()
                + (spoiler.bounds.position() - Point::ORIGIN);
            assert!(state.link_at(point).is_none());
            assert!(state.activate(point, &mut |_| panic!("revealing must not navigate")));
            state.layout(width);
            assert_eq!(state.blocks[2].links.len(), 1);
            let chip = &state.blocks[2].links[0];
            let point = chip.bounds.center() + (state.blocks[2].bounds.position() - Point::ORIGIN);
            assert_eq!(state.link_at(point), Some(event));
        }
    }

    #[test]
    fn syntax_colors_preserve_unicode_copy_offsets_and_theme_cache() {
        use interconnect::settings::ThemeMode;
        let html = "<pre><code class='language-rust'>/* comment\ncontinuation */\nlet <b>café</b> = \"👩🏽‍💻\";\n</code></pre>";
        let document = Arc::new(Document::parse_with_formulas(html).unwrap());
        let syntax = document.blocks[0].syntax.as_ref().unwrap().clone();
        let expected = "/* comment\ncontinuation */\nlet café = \"👩🏽‍💻\";\n";
        let mut state = State::new(document.clone());
        let mut plain = State::new(Arc::new(Document::parse(html).unwrap()));
        let colors = |state: &State| {
            state.blocks[0]
                .paragraph
                .buffer()
                .layout_runs()
                .flat_map(|line| line.glyphs.iter().map(|glyph| glyph.color_opt))
                .collect::<Vec<_>>()
        };
        for width in [400.0, 110.0] {
            let light_bounds = state.layout(width).bounds();
            assert_eq!(light_bounds, plain.layout(width).bounds());
            assert_eq!(state.text(), expected);
            let light_colors = colors(&state);
            assert!(light_colors.windows(2).any(|pair| pair[0] != pair[1]));
            // Hit-testing maps each syntax span back to the original semantic run.
            let block = &state.blocks[0];
            for (index, run) in block.runs.iter().enumerate() {
                for region in block.paragraph.span_bounds(index) {
                    let point = block.bounds.position() + (region.center() - Point::ORIGIN);
                    assert!(expected.is_char_boundary(state.hit(point)));
                    if block.bounds.contains(point) {
                        assert_eq!(state.run_at(point).unwrap(), &block.block.runs[*run]);
                    }
                }
            }
            state.set_theme(ThemeMode::Dark);
            assert_eq!(state.layout(width).bounds(), light_bounds);
            assert_ne!(colors(&state), light_colors);
            assert_eq!(state.text(), expected);
            state.set_theme(ThemeMode::Light);
            state.layout(width);
            assert_eq!(colors(&state), light_colors);
            assert!(Arc::ptr_eq(
                document.blocks[0].syntax.as_ref().unwrap(),
                &syntax
            ));
        }
    }
    #[test]
    fn theme_switch_refreshes_span_colors_without_changing_geometry_or_interaction() {
        use interconnect::settings::ThemeMode;
        let document = Arc::new(
            Document::parse(
                "<p>Hello <a href='https://example.org'>link</a> 👋</p><pre>code</pre>",
            )
            .unwrap(),
        );
        let mut state = State::new(document);
        state.focused = true;
        state.hover = Some(Point::new(10.0, 8.0));
        state.revealed.insert(7);
        let initial = state.layout(240.0).bounds();
        let text = state.text();
        let hit = state.hit(Point::new(10.0, 8.0));
        let color = state.blocks[0]
            .paragraph
            .buffer()
            .layout_runs()
            .next()
            .unwrap()
            .glyphs[0]
            .color_opt;
        state.set_theme(ThemeMode::Dark);
        assert!(state.width.is_none());
        assert_eq!(state.layout(240.0).bounds(), initial);
        assert_eq!(state.text(), text);
        assert_eq!(state.hit(Point::new(10.0, 8.0)), hit);
        assert!(state.focused && state.revealed.contains(&7));
        assert_eq!(state.hover, Some(Point::new(10.0, 8.0)));
        assert_ne!(
            state.blocks[0]
                .paragraph
                .buffer()
                .layout_runs()
                .next()
                .unwrap()
                .glyphs[0]
                .color_opt,
            color
        );
        state.set_theme(ThemeMode::Dark);
        assert_eq!(
            state.width,
            Some(240.0),
            "unchanged theme retains shaping cache"
        );
        state.set_theme(ThemeMode::Light);
        assert_eq!(state.layout(240.0).bounds(), initial);
        assert_eq!(
            state.blocks[0]
                .paragraph
                .buffer()
                .layout_runs()
                .next()
                .unwrap()
                .glyphs[0]
                .color_opt,
            color
        );
    }

    #[test]
    fn room_wide_mentions_wrap_copy_and_do_not_open_links_or_chats() {
        let document = Arc::new(
            Document::parse("<p>Hi @room!</p>")
                .unwrap()
                .with_room_wide_mention(true),
        );
        for width in [50.0, 300.0] {
            let mut state = State::new(document.clone());
            state.layout(width);
            assert_eq!(state.text(), "Hi @room!");
            let mut points = Vec::new();
            for block in &state.blocks {
                for (index, run_index) in block.runs.iter().enumerate() {
                    if block.block.runs[*run_index].style.room_wide {
                        points.extend(block.paragraph.span_bounds(index).iter().map(|region| {
                            region.center() + (block.bounds.position() - Point::ORIGIN)
                        }));
                    }
                }
            }
            assert!(!points.is_empty());
            for point in points {
                let mut messages = Vec::new();
                assert!(!state.activate(point, &mut |message| messages.push(message)));
                assert!(messages.is_empty());
            }
        }
    }
    #[test]
    fn inline_images_reserve_exact_boxes_and_keep_alt_copy_offsets_across_loading() {
        let alt = "a long alternative café 👋";
        let doc = Arc::new(Document::parse(&format!("<p>before <img src='mxc://example.org/inline' alt='{alt}' width='80' height='40'> after</p>")).unwrap());
        let mut state = State::new(doc);
        let initial = state.layout(300.0).bounds();
        assert_eq!(state.blocks[0].images.len(), 1);
        let bounds = state.blocks[0].images[0].bounds;
        assert_eq!(bounds.size(), Size::new(80.0, 40.0));
        assert!(bounds.y >= 0.0);
        assert!(bounds.y + bounds.height <= state.blocks[0].bounds.height);
        let text = state.text();
        let start = text.find(alt).unwrap();
        let origin = state.blocks[0].bounds.position();
        assert_eq!(
            state.hit(origin + (bounds.position() - Point::ORIGIN) + Vector::new(2.0, 2.0)),
            start
        );
        assert_eq!(
            state.hit(origin + (bounds.position() - Point::ORIGIN) + Vector::new(78.0, 2.0)),
            start + alt.len()
        );
        assert_eq!(
            state
                .image_requests(Point::ORIGIN, Rectangle::with_size(Size::new(300.0, 200.0)))
                .len(),
            1
        );
        assert!(
            state
                .image_requests(
                    Point::new(0.0, 500.0),
                    Rectangle::with_size(Size::new(300.0, 200.0))
                )
                .is_empty()
        );
        let key = interconnect::image_media::Key::inline("mxc://example.org/inline".into());
        state.assets.remote.ready = Arc::new(
            [(
                key.clone(),
                iced::widget::image::Handle::from_rgba(
                    3,
                    1,
                    vec![20, 180, 80, 255, 20, 180, 80, 255, 20, 180, 80, 255],
                ),
            )]
            .into_iter()
            .collect(),
        );
        assert_eq!(
            state.layout(300.0).bounds(),
            initial,
            "explicit dimensions must survive decoded aspect ratio changes"
        );
        let mut renderer = crate::test_renderer(Font::with_name("Inter"), iced::Pixels(14.0));
        let viewport = Rectangle::with_size(Size::new(300.0, 200.0));
        renderer.reset(viewport);
        state.draw(&mut renderer, Point::ORIGIN, viewport, None, None);
        let iced::Renderer::Secondary(software) = &mut renderer else {
            panic!("software renderer");
        };
        let mut pixels = tiny_skia::Pixmap::new(300, 200).unwrap();
        let mut mask = tiny_skia::Mask::new(300, 200).unwrap();
        software.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(300, 200), 1.0),
            &[viewport],
            iced::Color::WHITE,
        );
        let green = pixels
            .pixels()
            .iter()
            .filter(|p| {
                p.green() > p.red().saturating_add(50) && p.green() > p.blue().saturating_add(50)
            })
            .count();
        assert!(
            green >= 3000,
            "image pixels must be drawn inside the 80x40 reserved box: {green}"
        );
        state.assets.remote.ready = Arc::default();
        state.assets.remote.failed = Arc::new([key].into_iter().collect());
        assert_eq!(
            state.layout(300.0).bounds(),
            initial,
            "failures keep their reserved box"
        );
        assert_eq!(state.text(), text);
    }
    #[test]
    fn local_objects_wrap_and_reserve_baseline_descent_in_bidi_text() {
        let mut doc = Document::parse("<p>قبل <img alt='formula' width='120' height='48'> אחרי <img alt='second' width='120' height='24'></p>").unwrap();
        for run in &mut doc.blocks[0].runs {
            if let Inline::Image {
                source, descent, ..
            } = &mut run.content
            {
                *source = Some(ImageSource::Local(7));
                *descent = 8;
            }
        }
        let mut state = State::new(Arc::new(doc));
        for width in [300.0, 150.0, 50.0] {
            state.layout(width);
            let block = &state.blocks[0];
            assert_eq!(block.images.len(), 2);
            for image in &block.images {
                assert!(image.bounds.width <= width);
                let run = block
                    .paragraph
                    .buffer()
                    .layout_runs()
                    .find(|line| {
                        line.glyphs
                            .iter()
                            .any(|glyph| block.runs[glyph.metadata] == image.run)
                    })
                    .unwrap();
                assert!(image.bounds.y >= run.line_top - 0.01);
                assert!(
                    image.bounds.y + image.bounds.height <= run.line_top + run.line_height + 0.01
                );
                let expected_descent = 8.0 * image.bounds.width / 120.0;
                assert!(
                    (image.bounds.y + image.bounds.height - run.line_y - expected_descent).abs()
                        < 0.01
                );
                let hit =
                    state.hit(block.bounds.position() + (image.bounds.center() - Point::ORIGIN));
                assert!(state.text().is_char_boundary(hit));
            }
            assert!(
                state
                    .image_requests(
                        Point::ORIGIN,
                        Rectangle::with_size(Size::new(300.0, 1000.0))
                    )
                    .is_empty(),
                "local assets never enter Matrix demand"
            );
        }
    }
    #[test]
    fn matrix_formulas_use_native_object_layout_copy_source_and_center_display_math() {
        let doc = Document::parse_with_formulas(r#"<p>Before <span data-mx-maths="\frac{a}{b}"><code>fallback</code></span> after.</p><div data-mx-maths="\sum_{n=1}^{10}n"><code>sum</code></div><span data-mx-spoiler data-mx-maths="x^2">hidden</span>"#).unwrap();
        let mut state = State::new(Arc::new(doc));
        assert_eq!(
            state.text(),
            "Before \\frac{a}{b} after.\n\n\\sum_{n=1}^{10}n\n\n[Spoiler — click to reveal]"
        );
        for width in [400.0, 150.0, 30.0] {
            state.layout(width);
            assert_eq!(state.blocks[0].images.len(), 1);
            assert_eq!(state.blocks[1].images.len(), 1);
            assert!(state.blocks[2].images.is_empty());
            let display = &state.blocks[1];
            let image = &display.images[0];
            let center = display.bounds.x + image.bounds.center_x();
            assert!((center - width / 2.0).abs() < 0.01);
            for block in &state.blocks[..2] {
                for image in &block.images {
                    assert!(image.bounds.width <= width);
                    let handle = state.assets.handle(image.source.as_ref().unwrap()).unwrap();
                    assert!(matches!(handle, iced::widget::image::Handle::Rgba { .. }));
                    let point = block.bounds.position() + (image.bounds.center() - Point::ORIGIN);
                    let hit = state.hit(point);
                    let start = block.range.start + image.canonical.start;
                    let end = block.range.start + image.canonical.end;
                    assert!(hit == start || hit == end, "formula hit is atomic");
                }
            }
            assert!(
                state
                    .image_requests(
                        Point::ORIGIN,
                        Rectangle::with_size(Size::new(width, 1000.0))
                    )
                    .is_empty()
            );
        }
        let point = state.blocks[2].bounds.position() + Vector::new(2.0, 5.0);
        assert!(state.activate(point, &mut |_| {}));
        state.layout(400.0);
        assert_eq!(state.blocks[2].images.len(), 1);
        assert!(state.text().ends_with("x^2"));
        assert!(!state.text().contains('\u{fffc}'));
    }
    #[test]
    fn missing_dimensions_relayout_once_from_intrinsics_and_spoilers_do_not_fetch() {
        let mut state = State::new(Arc::new(Document::parse("<p><img src='mxc://example.org/inline' alt='image' height='20'></p><p><span data-mx-spoiler><img src='mxc://example.org/hidden' alt='secret' width='40' height='30'></span></p>").unwrap()));
        state.layout(300.0);
        assert_eq!(
            state.blocks[0].images[0].bounds.size(),
            Size::new(20.0, 20.0)
        );
        let key = interconnect::image_media::Key::inline("mxc://example.org/inline".into());
        state.assets.remote.ready = Arc::new(
            [(
                key,
                iced::widget::image::Handle::from_rgba(4, 1, vec![0; 16]),
            )]
            .into_iter()
            .collect(),
        );
        state.layout(300.0);
        assert_eq!(
            state.blocks[0].images[0].bounds.size(),
            Size::new(80.0, 20.0)
        );
        let keys = state.image_requests(
            Point::ORIGIN,
            Rectangle::with_size(Size::new(300.0, 1000.0)),
        );
        assert_eq!(keys.len(), 1);
        assert!(!state.text().contains("secret"));
        state.revealed.insert(1);
        state.width = None;
        state.layout(300.0);
        assert_eq!(
            state
                .image_requests(
                    Point::ORIGIN,
                    Rectangle::with_size(Size::new(300.0, 1000.0))
                )
                .len(),
            2
        );
    }
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
                None,
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
    fn mentions_wrap_select_and_activate_with_their_displayed_text() {
        let link = "https://matrix.to/#/@maya:example.org";
        let label = "Maya 👋 with a long display name";
        let document = Arc::new(Document::parse(&format!(
            "<p>Hello <a href='{link}'>{label}</a>!</p><p>قبل <a href='{link}'>مايا</a> אחרי</p>"
        )).unwrap());
        for width in [95.0, 300.0] {
            let mut state = State::new(document.clone());
            let size = state.layout(width).size();
            assert_eq!(state.text(), format!("Hello {label}!\n\nقبل مايا אחרי"));
            let mut points = Vec::new();
            for block in &state.blocks {
                for (index, run_index) in block.runs.iter().enumerate() {
                    if block.block.runs[*run_index].style.mention.is_none() {
                        continue;
                    }
                    for region in block.paragraph.span_bounds(index) {
                        let point = region.center() + (block.bounds.position() - Point::ORIGIN);
                        points.push(point);
                    }
                }
            }
            assert!(points.len() >= 2);
            for point in points {
                assert!(state.text().is_char_boundary(state.hit(point)));
                let mut messages = Vec::new();
                assert!(!state.activate(point, &mut |message| messages.push(message)));
                assert!(
                    matches!(&messages[..], [Message::NewChat(crate::new_chat::Event::Mention(user))] if user.as_str() == "@maya:example.org"),
                    "width {width}, point {point:?}, run {:?}",
                    state.run_at(point)
                );
            }
            let mut renderer = crate::test_renderer(Font::with_name("Inter"), iced::Pixels(14.0));
            let viewport = Rectangle::with_size(size);
            renderer.reset(viewport);
            state.draw(
                &mut renderer,
                Point::ORIGIN,
                viewport,
                Some(0..state.text().len()),
                None,
            );
            assert_eq!(state.layout(width).size(), size);
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
        assert_eq!(state.link_at(point), None);
        let mut messages = vec![];
        assert!(state.activate(point, &mut |m| messages.push(m)));
        assert!(messages.is_empty());
        assert_eq!(state.text(), "secret");
        state.layout(300.0);
        let point = state.blocks[0].paragraph.span_bounds(0)[0].center();
        assert_eq!(state.link_at(point), Some("https://example.org/"));
        assert!(!state.activate(point, &mut |m| messages.push(m)));
        assert!(
            matches!(&messages[0], Message::OpenMessageLink(url) if url == "https://example.org/")
        );
    }
}
