//! Inline assets and native embedded-object paragraphs. Pixels never determine
//! explicitly declared layout dimensions, and no decoding happens on the UI thread.
use iced::advanced::{
    graphics::text::{self as graphics, cosmic_text as cosmic},
    text::{self, Paragraph as _},
};
use iced::{Font, Point, Rectangle, Size};
use interconnect::{image_media::Key, media::Snapshot, message_document::ImageSource};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub thread: bool,
    pub generation: u64,
}
pub type Demand =
    HashMap<(bool, u64, String), (Arc<interconnect::message_document::Document>, Vec<Key>)>;
pub type LocalImages = Arc<HashMap<u64, iced::widget::image::Handle>>;
#[derive(Clone, Debug, Default)]
pub struct Assets {
    pub remote: Snapshot<Key>,
    pub local: LocalImages,
    pub links: super::link_preview::Metadata,
}
impl Assets {
    pub fn handle<'a>(
        &'a self,
        source: &'a ImageSource,
    ) -> Option<&'a iced::widget::image::Handle> {
        let handle = match source {
            ImageSource::Matrix(uri) => self.remote.ready.get(&Key::inline(uri.clone())),
            ImageSource::Local(id) => self.local.get(id),
            ImageSource::Formula(image) => Some(&image.handle),
        }?;
        // An asset producer must decode first. Never let image layout read a file
        // or decode arbitrary bytes through Iced's synchronous image path.
        match handle {
            iced::widget::image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            } if *width > 0
                && *height > 0
                && *width <= 4096
                && *height <= 4096
                && u64::from(*width) * u64::from(*height) * 4 == pixels.len() as u64 =>
            {
                Some(handle)
            }
            _ => None,
        }
    }
    pub fn intrinsic(&self, source: &ImageSource) -> Option<Size> {
        match self.handle(source)? {
            iced::widget::image::Handle::Rgba { width, height, .. }
                if *width > 0 && *height > 0 =>
            {
                Some(Size::new(*width as f32, *height as f32))
            }
            _ => None,
        }
    }
    pub fn failed(&self, source: &ImageSource) -> bool {
        match source {
            ImageSource::Matrix(uri) => self.remote.failed.contains(&Key::inline(uri.clone())),
            ImageSource::Local(_) | ImageSource::Formula(_) => false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub size: Size,
    pub descent: f32,
}
impl Metrics {
    pub fn resolve(
        width: Option<u32>,
        height: Option<u32>,
        descent: u32,
        intrinsic: Option<Size>,
        available: f32,
    ) -> Self {
        let w = width.filter(|w| *w > 0).map(|w| w as f32);
        let h = height.filter(|h| *h > 0).map(|h| h as f32);
        let intrinsic = intrinsic.unwrap_or(Size::new(24.0, 24.0));
        let (width, height) = match (w, h) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * intrinsic.height / intrinsic.width),
            (None, Some(h)) => (h * intrinsic.width / intrinsic.height, h),
            (None, None) => (intrinsic.width, intrinsic.height),
        };
        let scale = (available.max(1.0) / width)
            .min(2048.0 / width)
            .min(2048.0 / height)
            .min(1.0);
        Self {
            size: Size::new((width * scale).max(1.0), (height * scale).max(1.0)),
            descent: ((descent as f32).min(height) * scale).min(height * scale),
        }
    }
}
#[derive(Clone, Copy)]
pub struct Object {
    pub span: usize,
    pub metrics: Metrics,
}
#[derive(Debug)]
pub enum Paragraph {
    Text(graphics::Paragraph),
    Embedded {
        buffer: Arc<cosmic::Buffer>,
        size: Size,
    },
}
impl Paragraph {
    pub fn with_wrapping(
        spans: &[text::Span<'_, (), Font>],
        width: f32,
        size: f32,
        objects: &[Object],
        wrapping: text::Wrapping,
        alignment: text::Alignment,
    ) -> Self {
        if objects.is_empty() && alignment == text::Alignment::Default {
            return Self::Text(graphics::Paragraph::with_spans(text::Text {
                content: spans,
                bounds: Size::new(width, f32::INFINITY),
                size: iced::Pixels(size),
                line_height: text::LineHeight::Relative(1.45),
                font: Font::with_name("Inter"),
                align_x: alignment,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping,
            }));
        }
        let mut system = graphics::font_system().write().expect("Write font system");
        let fonts = system.raw();
        let mut buffer = cosmic::Buffer::new(fonts, cosmic::Metrics::new(size, size * 1.45));
        buffer.set_size(fonts, Some(width), None);
        buffer.set_wrap(fonts, cosmic::Wrap::WordOrGlyph);
        buffer.set_rich_text(
            fonts,
            spans.iter().enumerate().map(|(index, span)| {
                let mut attrs =
                    graphics::to_attributes(span.font.unwrap_or(Font::with_name("Inter")))
                        .metadata(index);
                if let Some(color) = span.color {
                    attrs = attrs.color(cosmic::Color::rgba(
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        (color.a * 255.0) as u8,
                    ));
                }
                (span.text.as_ref(), attrs)
            }),
            &graphics::to_attributes(Font::with_name("Inter")),
            cosmic::Shaping::Advanced,
            match alignment {
                text::Alignment::Center => Some(cosmic::Align::Center),
                text::Alignment::Right => Some(cosmic::Align::Right),
                text::Alignment::Left => Some(cosmic::Align::Left),
                _ => None,
            },
        );
        for line in &mut buffer.lines {
            line.customize_shape(fonts, 8, |shape| {
                for span in &mut shape.spans {
                    for word in &mut span.words {
                        let mut seen = std::collections::HashSet::new();
                        word.glyphs.retain_mut(|glyph| {
                            let Some(object) =
                                objects.iter().find(|object| object.span == glyph.metadata)
                            else {
                                return true;
                            };
                            if !seen.insert(glyph.metadata) {
                                return false;
                            }
                            let metrics = object.metrics;
                            glyph.x_advance = metrics.size.width;
                            glyph.y_advance = 0.0;
                            glyph.x_offset = 0.0;
                            glyph.y_offset = 0.0;
                            glyph.ascent = metrics.size.height - metrics.descent;
                            glyph.descent = metrics.descent;
                            glyph.metrics_opt =
                                Some(cosmic::Metrics::new(1.0, metrics.size.height));
                            glyph.font_monospace_em_width = None;
                            glyph.color_opt = Some(cosmic::Color::rgba(0, 0, 0, 0));
                            true
                        });
                    }
                }
            });
        }
        buffer.shape_until_scroll(fonts, false);
        for line in &mut buffer.lines {
            line.customize_layout(|lines| {
                for line in lines {
                    if line
                        .glyphs
                        .iter()
                        .any(|g| objects.iter().any(|o| o.span == g.metadata))
                    {
                        line.line_height_opt = Some(
                            line.line_height_opt
                                .unwrap_or(size * 1.45)
                                .max(line.max_ascent + line.max_descent + 4.0),
                        );
                    }
                }
            });
        }
        let measured = graphics::measure(&buffer).0;
        Self::Embedded {
            buffer: Arc::new(buffer),
            size: measured,
        }
    }
    pub fn buffer(&self) -> &cosmic::Buffer {
        match self {
            Self::Text(p) => p.buffer(),
            Self::Embedded { buffer, .. } => buffer,
        }
    }
    pub fn min_height(&self) -> f32 {
        match self {
            Self::Text(p) => p.min_height(),
            Self::Embedded { size, .. } => size.height,
        }
    }
    pub fn span_bounds(&self, index: usize) -> Vec<Rectangle> {
        let mut bounds = Vec::new();
        for line in self.buffer().layout_runs() {
            let mut regions: Vec<_> = line
                .glyphs
                .iter()
                .filter(|glyph| glyph.metadata == index && glyph.w > 0.0)
                .map(|glyph| {
                    Rectangle::new(
                        Point::new(glyph.x, line.line_top),
                        Size::new(glyph.w, line.line_height),
                    )
                })
                .collect();
            // Logical spans can occupy separate visual segments in bidi text.
            regions.sort_by(|a, b| a.x.total_cmp(&b.x));
            let mut current: Option<Rectangle> = None;
            for region in regions {
                if let Some(previous) = current.as_mut()
                    && region.x <= previous.x + previous.width + 0.5
                {
                    previous.width = (region.x + region.width - previous.x).max(previous.width);
                } else {
                    bounds.extend(current.take());
                    current = Some(region);
                }
            }
            bounds.extend(current);
        }
        bounds
    }
    pub fn hit_span(&self, point: Point) -> Option<usize> {
        // Resolve the glyph under the pointer directly. Cursor affinity can
        // otherwise select an adjacent span at wrapped lines or RTL boundaries.
        let line = self
            .buffer()
            .layout_runs()
            .find(|line| point.y >= line.line_top && point.y < line.line_top + line.line_height)?;
        line.glyphs
            .iter()
            .find(|glyph| point.x >= glyph.x && point.x < glyph.x + glyph.w)
            .map(|glyph| glyph.metadata)
    }
    pub fn draw(
        &self,
        renderer: &mut iced::Renderer,
        position: Point,
        clip: Rectangle,
        color: iced::Color,
    ) {
        use iced::advanced::{graphics::text::Renderer as _, text::Renderer as _};
        match self {
            Self::Text(p) => renderer.fill_paragraph(p, position, color, clip),
            Self::Embedded { buffer, .. } => renderer.fill_raw(graphics::Raw {
                buffer: Arc::downgrade(buffer),
                position,
                color,
                clip_bounds: clip,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_are_bounded_proportional_and_independent_when_declared() {
        assert_eq!(
            Metrics::resolve(Some(80), Some(40), 0, Some(Size::new(200.0, 200.0)), 300.0).size,
            Size::new(80.0, 40.0)
        );
        assert_eq!(
            Metrics::resolve(Some(80), Some(40), 10, None, 40.0),
            Metrics {
                size: Size::new(40.0, 20.0),
                descent: 5.0
            }
        );
        assert_eq!(
            Metrics::resolve(None, Some(20), 0, Some(Size::new(100.0, 50.0)), 300.0).size,
            Size::new(40.0, 20.0)
        );
        for (w, h) in [(0, 0), (u32::MAX, 1), (1, u32::MAX), (u32::MAX, u32::MAX)] {
            let metrics = Metrics::resolve(Some(w), Some(h), u32::MAX, None, 80.0);
            assert!(metrics.size.width.is_finite() && metrics.size.height.is_finite());
            assert!((1.0..=80.0).contains(&metrics.size.width));
            assert!((1.0..=2048.0).contains(&metrics.size.height));
            assert!(metrics.descent <= metrics.size.height);
        }
    }
}
