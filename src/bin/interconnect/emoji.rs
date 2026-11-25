//! Font attributes for complete emoji graphemes; message text stays untouched.
use iced::Font;
use iced::advanced::{graphics::text::font_system, text::highlighter};
use std::{ops::Range, sync::OnceLock};
use unicode_segmentation::UnicodeSegmentation;

/// Resolve installed fonts once, including for both renderers and composers.
pub fn font() -> Option<Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    *FONT.get_or_init(|| {
        let mut system = font_system().write().expect("Write font system");
        let database = system.raw().db();
        let names = if cfg!(target_os = "macos") {
            ["Apple Color Emoji", "Noto Color Emoji", "Segoe UI Emoji"]
        } else if cfg!(target_os = "windows") {
            ["Segoe UI Emoji", "Noto Color Emoji", "Apple Color Emoji"]
        } else {
            ["Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji"]
        };
        names.into_iter().find_map(|name| {
            database
                .faces()
                .any(|face| face.families.iter().any(|(family, _)| family == name))
                .then(|| Font::with_name(name))
        })
    })
}

#[derive(Debug)]
pub struct Highlighter {
    font: Option<Font>,
    line: usize,
}

/// Thread excerpts use rich text because their whole row is a button.
pub fn preview<Message: 'static>(body: String) -> iced::widget::text::Rich<'static, (), Message> {
    use highlighter::Highlighter as _;
    use iced::widget::{rich_text, span};
    let mut highlighter = Highlighter::new(&font());
    let mut spans = Vec::new();
    let mut end = 0;
    for (range, font) in highlighter.highlight_line(&body) {
        if end < range.start {
            spans.push(span(body[end..range.start].to_owned()));
        }
        spans.push(span(body[range.clone()].to_owned()).font(font));
        end = range.end;
    }
    spans.push(span(body[end..].to_owned()));
    rich_text(spans)
}

impl highlighter::Highlighter for Highlighter {
    type Settings = Option<Font>;
    type Highlight = Font;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Font)>;

    fn new(font: &Self::Settings) -> Self {
        Self {
            font: *font,
            line: 0,
        }
    }

    fn update(&mut self, font: &Self::Settings) {
        self.font = *font;
        self.line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.line = self.line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.line += 1;
        let mut ranges = Vec::new();
        if let Some(font) = self.font {
            for (start, grapheme) in line.grapheme_indices(true) {
                // VS15 explicitly requests text presentation. Look up whole
                // graphemes to keep modifiers, flags, keycaps, and ZWJ intact.
                if !grapheme.is_ascii()
                    && !grapheme.contains('\u{fe0e}')
                    && emojis::get(grapheme).is_some()
                {
                    ranges.push((start..start + grapheme.len(), font));
                }
            }
        }
        ranges.into_iter()
    }

    fn current_line(&self) -> usize {
        self.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use highlighter::Highlighter as _;

    #[test]
    fn font_ranges_preserve_complete_sequences_and_text_presentation() {
        let font = Font::with_name("Noto Color Emoji");
        let mut highlighter = Highlighter::new(&Some(font));
        let text = "café 😀 ☺ ❤ ❤️ 👍🏽 🇩🇪 👩🏽‍💻 👨‍👩‍👧‍👦 1️⃣ #️⃣ ☺\u{fe0e} ❤\u{fe0e} # * 123";
        let ranges: Vec<_> = highlighter.highlight_line(text).collect();
        assert_eq!(
            ranges
                .iter()
                .map(|(range, _)| &text[range.clone()])
                .collect::<Vec<_>>(),
            ["😀", "☺", "❤", "❤️", "👍🏽", "🇩🇪", "👩🏽‍💻", "👨‍👩‍👧‍👦", "1️⃣", "#️⃣"]
        );
        assert!(ranges.iter().all(|(_, selected)| *selected == font));
        assert_eq!(highlighter.current_line(), 1);
        highlighter.change_line(0);
        assert_eq!(highlighter.current_line(), 0);
        highlighter.update(&None);
        assert_eq!(highlighter.highlight_line(text).count(), 0);
    }

    #[test]
    fn emoji_glyphs_are_colored_and_sequences_shape_together() {
        use iced::advanced::{graphics::text, text::Editor as _};
        let Some(font) = font() else {
            eprintln!("Skipping raster check: no supported color emoji font installed");
            return;
        };
        for emoji in [
            "☺",
            "❤",
            "♥",
            "☀️",
            "✈️",
            "☎️",
            "😀",
            "👍🏽",
            "🇩🇪",
            "👩🏽‍💻",
            "👨‍👩‍👧‍👦",
            "1️⃣",
            // Exercise an unconsumed default-ignorable even with fonts that
            // consume VS16 into the preceding emoji glyph.
            "😀\u{200b}",
        ] {
            let mut editor = text::Editor::with_text(emoji);
            let mut highlighter = Highlighter::new(&Some(font));
            editor.update(
                iced::Size::new(300.0, 100.0),
                Font::with_name("Inter"),
                iced::Pixels(14.0),
                iced::advanced::text::LineHeight::default(),
                iced::advanced::text::Wrapping::WordOrGlyph,
                &mut highlighter,
            );
            editor.highlight(Font::with_name("Inter"), &mut highlighter, |font| {
                highlighter::Format {
                    font: Some(*font),
                    color: None,
                }
            });
            let glyphs: Vec<_> = editor
                .buffer()
                .layout_runs()
                .flat_map(|run| run.glyphs)
                .collect();
            let mut system = font_system().write().unwrap();
            let mut cache = text::cosmic_text::SwashCache::new();
            let mut visible_glyphs = 0;
            for glyph in glyphs {
                assert_ne!(glyph.glyph_id, 0, "{emoji} must not use a missing glyph");
                let key = glyph.physical((0.0, 0.0), 1.0).cache_key;
                let image = cache.get_image_uncached(system.raw(), key);
                let visible = image.as_ref().is_some_and(|image| match image.content {
                    text::cosmic_text::SwashContent::Color => image
                        .data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel[3] != 0),
                    _ => image.data.iter().any(|byte| *byte != 0),
                });
                // Shaping may retain VS16 (e.g. with Apple Color Emoji) as
                // an invisible space glyph. It must neither draw nor advance.
                if glyph.w == 0.0 {
                    assert!(!visible, "{emoji} has a visible zero-width glyph");
                    continue;
                }
                let image = image.unwrap_or_else(|| panic!("{emoji} must rasterize"));
                assert!(visible, "{emoji} must have visible pixels");
                assert_eq!(
                    image.content,
                    text::cosmic_text::SwashContent::Color,
                    "{emoji}"
                );
                visible_glyphs += 1;
            }
            assert_eq!(visible_glyphs, 1, "{emoji} must stay one visible glyph");
        }
    }

    #[test]
    fn editing_recomputes_font_spans_and_clears_removed_emoji() {
        use iced::advanced::{
            graphics::text,
            text::Editor as _,
            text::editor::{Action, Edit},
        };
        let mut editor = text::Editor::with_text("");
        let mut highlighter = Highlighter::new(&Some(Font::with_name("Noto Color Emoji")));
        for body in ["❤", "ordinary # * 123", "👍🏽\n👩🏽‍💻", "text ❤\u{fe0e}"] {
            editor.perform(Action::SelectAll);
            editor.perform(Action::Edit(Edit::Paste(std::sync::Arc::new(body.into()))));
            editor.update(
                iced::Size::new(300.0, 100.0),
                Font::with_name("Inter"),
                iced::Pixels(14.0),
                iced::advanced::text::LineHeight::default(),
                iced::advanced::text::Wrapping::WordOrGlyph,
                &mut highlighter,
            );
            editor.highlight(Font::with_name("Inter"), &mut highlighter, |font| {
                highlighter::Format {
                    font: Some(*font),
                    color: None,
                }
            });
            let spans: Vec<_> = editor
                .buffer()
                .lines
                .iter()
                .flat_map(|line| {
                    line.attrs_list()
                        .spans_iter()
                        .map(|(range, _)| &line.text()[range.clone()])
                })
                .collect();
            let expected = match body {
                "❤" => vec!["❤"],
                "👍🏽\n👩🏽‍💻" => vec!["👍🏽", "👩🏽‍💻"],
                _ => vec![],
            };
            assert_eq!(spans, expected);
            editor.perform(Action::SelectAll);
            assert_eq!(editor.copy().as_deref(), Some(body));
        }
    }

    #[test]
    fn first_message_layout_matches_draw_and_copy_keeps_original_unicode() {
        use crate::{selectable, shell};
        use iced::advanced::{Renderer as _, graphics, layout, mouse, widget::Tree};
        let Some(_) = font() else {
            eprintln!("Skipping layout/raster check: no supported color emoji font installed");
            return;
        };
        font_system().write().unwrap().load_font(
            include_bytes!("../../../assets/fonts/Inter-Regular.otf")
                .as_slice()
                .into(),
        );
        let body = "café ☺ ❤ ❤️ 👍🏽 🇩🇪 👩🏽‍💻 👨‍👩‍👧‍👦 1️⃣\ntext ☺\u{fe0e} ❤\u{fe0e}";
        for width in [75.0, 300.0] {
            let mut view = selectable::view("emoji", body);
            let mut tree = Tree::new(&view);
            let size = iced::Size::new(width, 500.0);
            let limits = layout::Limits::new(iced::Size::ZERO, size);
            let mut renderer = crate::test_renderer(Font::with_name("Inter"), iced::Pixels(14.0));
            let first = view.as_widget_mut().layout(&mut tree, &renderer, &limits);
            renderer.reset(iced::Rectangle::with_size(size));
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &shell::theme(),
                &iced::advanced::renderer::Style::default(),
                layout::Layout::new(&first),
                mouse::Cursor::Unavailable,
                &iced::Rectangle::with_size(size),
            );
            let iced::Renderer::Secondary(software) = &mut renderer else {
                panic!("software test renderer");
            };
            let mut pixels = tiny_skia::Pixmap::new(width as u32, 500).unwrap();
            let mut mask = tiny_skia::Mask::new(width as u32, 500).unwrap();
            software.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &graphics::Viewport::with_physical_size(iced::Size::new(width as u32, 500), 1.0),
                &[iced::Rectangle::with_size(size)],
                iced::Color::WHITE,
            );
            assert!(
                pixels.pixels().iter().any(|pixel| {
                    let rgb = [pixel.red(), pixel.green(), pixel.blue()];
                    rgb.iter().max().unwrap() - rgb.iter().min().unwrap() > 50
                }),
                "message must contain colored pixels"
            );
            let second = view.as_widget_mut().layout(&mut tree, &renderer, &limits);
            assert_eq!(
                first.bounds(),
                second.bounds(),
                "draw must not change wrapping"
            );
            assert!(first.size().height > 2.0 * 14.0);
            let state = tree.state.downcast_mut::<selectable::State>();
            state
                .content
                .perform(iced::widget::text_editor::Action::SelectAll);
            assert_eq!(state.content.selection().as_deref(), Some(body));
            assert_eq!(state.content.text(), body);
        }
    }
}
