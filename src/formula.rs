//! Bounded, offline RaTeX rendering. Call only from a blocking worker.
use iced::widget::image::Handle;
use ratex_types::{color::Color, math_style::MathStyle};
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

pub const MAX_FORMULAS: usize = 16;
pub const MAX_DOCUMENT_PIXELS: usize = 512 * 1024;
const MAX_SOURCE: usize = 2048;
const FONT_SIZE: f32 = 14.0;
const PADDING: f32 = 1.0;
const DENSITY: f32 = 2.0;

/// A decoded local asset, retained only as long as its document. Equality and
/// hashing use asset identity, without scanning private source or pixel buffers.
#[derive(Clone)]
pub struct Image {
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
    pub descent: u32,
}
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        self.handle.id() == other.handle.id()
    }
}
impl Eq for Image {}
impl Hash for Image {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.handle.id().hash(state);
    }
}

/// No source or renderer errors are logged: both can contain private messages.
pub fn render(source: &str, display: bool, remaining_pixels: &mut usize) -> Option<Arc<Image>> {
    if source.trim().is_empty() || source.len() > MAX_SOURCE || *remaining_pixels == 0 {
        return None;
    }
    let nodes = ratex_parser::parse(source).ok()?;
    let options = ratex_layout::LayoutOptions {
        style: if display {
            MathStyle::Display
        } else {
            MathStyle::Text
        },
        // Keep in sync with the native message text color (shell::INK).
        color: Color::new(39.0 / 255.0, 46.0 / 255.0, 56.0 / 255.0, 1.0),
        ..Default::default()
    };
    let layout = ratex_layout::layout(&nodes, &options);
    ratex_layout::validate_layout(&layout).ok()?;
    let list = ratex_layout::to_display_list(&layout);
    if list.items.is_empty() {
        return None;
    }
    let width = (list.width * f64::from(FONT_SIZE) + f64::from(PADDING * 2.0)) * f64::from(DENSITY);
    let height = ((list.height + list.depth) * f64::from(FONT_SIZE) + f64::from(PADDING * 2.0))
        * f64::from(DENSITY);
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || width > 2048.0
        || height > 2048.0
    {
        return None;
    }
    let pixels = width.ceil() as usize * height.ceil() as usize;
    if pixels > 256 * 1024 || pixels > *remaining_pixels {
        return None;
    }
    let png = ratex_render::render_to_png(
        &list,
        &ratex_render::RenderOptions {
            font_size: FONT_SIZE,
            padding: PADDING,
            background_color: Color::new(0.0, 0.0, 0.0, 0.0),
            device_pixel_ratio: DENSITY,
            ..Default::default()
        },
    )
    .ok()?;
    let rgba = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (pixel_width, pixel_height) = rgba.dimensions();
    // Account for the rasterizer's f32 rounding rather than just our f64 estimate.
    let actual_pixels = pixel_width as usize * pixel_height as usize;
    if pixel_width > 2048
        || pixel_height > 2048
        || actual_pixels > 256 * 1024
        || actual_pixels > *remaining_pixels
    {
        return None;
    }
    let width = (pixel_width as f32 / DENSITY).ceil() as u32;
    let height = (pixel_height as f32 / DENSITY).ceil() as u32;
    let baseline = list.height * f64::from(FONT_SIZE) + f64::from(PADDING);
    let descent = (f64::from(height) - baseline)
        .round()
        .clamp(0.0, f64::from(height)) as u32;
    *remaining_pixels -= actual_pixels;
    Some(Arc::new(Image {
        handle: Handle::from_rgba(pixel_width, pixel_height, rgba.into_raw()),
        width,
        height,
        descent,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_render_common_element_formulas_with_transparent_pixels() {
        for source in [
            r"E=mc^2",
            r"\frac{a}{b}",
            r"\sqrt{x^2+y^2}",
            r"\int_0^\infty e^{-x^2}\,dx",
            r"\begin{pmatrix}a&b\\c&d\end{pmatrix}",
            r"\begin{aligned}a&=b+c\\d&=e\end{aligned}",
            r"\ce{H2O}",
        ] {
            for display in [false, true] {
                let mut budget = MAX_DOCUMENT_PIXELS;
                let image = render(source, display, &mut budget).expect(source);
                assert!(image.width > 0 && image.height > 0 && image.descent <= image.height);
                assert!(budget < MAX_DOCUMENT_PIXELS);
                let Handle::Rgba {
                    width,
                    height,
                    pixels,
                    ..
                } = &image.handle
                else {
                    panic!("decoded pixels");
                };
                assert_eq!(pixels.len(), (width * height * 4) as usize);
                assert!(
                    pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                    "visible ink: {source}"
                );
                assert!(
                    pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0),
                    "transparent background"
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_recursive_oversized_and_over_budget_formulas() {
        for source in [
            r"\unknowncommand{x}".to_owned(),
            r"\frac{".into(),
            r"\def\a{\a}\a".into(),
            format!("{}x{}", "{".repeat(100), "}".repeat(100)),
            "x".repeat(MAX_SOURCE + 1),
            r"\rule{1000000em}{1000000em}".into(),
        ] {
            let mut budget = MAX_DOCUMENT_PIXELS;
            assert!(render(&source, false, &mut budget).is_none());
            assert_eq!(budget, MAX_DOCUMENT_PIXELS);
        }
        assert!(render("x", false, &mut 1).is_none());
    }
}
