//! Cached, grapheme-safe single-line labels. Re-shape only when text or layout changes.
use iced::{
    Element, Font, Length, Pixels, Rectangle, Size,
    advanced::{
        self, layout, mouse, renderer,
        text::{self, Paragraph as _, paragraph},
        widget,
    },
};
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

type TextStyle<'a> = Box<dyn Fn(&iced::Theme) -> iced::widget::text::Style + 'a>;

type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;

pub(super) fn label<'a>(value: impl Into<Cow<'a, str>>, size: u32) -> Label<'a> {
    Label {
        value: value.into(),
        size: size as f32,
        font: None,
        style: None,
        width: Length::Fill,
    }
}

pub(super) struct Label<'a> {
    value: Cow<'a, str>,
    size: f32,
    font: Option<Font>,
    style: Option<TextStyle<'a>>,
    width: Length,
}
impl<'a> Label<'a> {
    pub(super) fn shrink(mut self) -> Self {
        self.width = Length::Shrink;
        self
    }
    pub(super) fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }
    pub(super) fn style(
        mut self,
        style: impl Fn(&iced::Theme) -> iced::widget::text::Style + 'a,
    ) -> Self {
        self.style = Some(Box::new(style));
        self
    }
}
#[derive(Default)]
struct State {
    source: Option<String>,
    width: f32,
    paragraph: paragraph::Plain<Paragraph>,
    display: String,
}
impl<Message> advanced::Widget<Message, iced::Theme, iced::Renderer> for Label<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(self.size * 1.3))
    }
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        use text::Renderer as _;
        // Shape against available bounds before resolving intrinsic width. A
        // shrink parent compresses Fill to zero until that width is measured.
        let size = Size::new(limits.max().width, self.size * 1.3);
        let state = tree.state.downcast_mut::<State>();
        let font = self.font.unwrap_or_else(|| renderer.default_font());
        if state.source.as_deref() != Some(self.value.as_ref())
            || state.width != size.width
            || state.paragraph.raw().font() != font
            || state.paragraph.raw().size() != Pixels(self.size)
        {
            let source = self.value.replace(['\n', '\r'], " ");
            let format = text::Text {
                content: "",
                bounds: Size::new(f32::INFINITY, self.size * 1.3),
                size: Pixels(self.size),
                line_height: text::LineHeight::Relative(1.3),
                font,
                align_x: text::Alignment::Default,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            };
            let mut measure = |value: &str| {
                let _ = state.paragraph.update(format.with_content(value));
                state.paragraph.min_width()
            };
            let display = if measure(&source) <= size.width {
                source
            } else if measure("…") > size.width {
                String::new()
            } else {
                let ends: Vec<_> = source
                    .grapheme_indices(true)
                    .map(|(offset, grapheme)| offset + grapheme.len())
                    .collect();
                let (mut low, mut high) = (0, ends.len());
                while low < high {
                    let mid = (low + high).div_ceil(2);
                    let candidate = format!("{}…", &source[..ends[mid - 1]]);
                    if measure(&candidate) <= size.width {
                        low = mid;
                    } else {
                        high = mid - 1;
                    }
                }
                if low == 0 {
                    "…".into()
                } else {
                    format!("{}…", &source[..ends[low - 1]])
                }
            };
            let _ = state
                .paragraph
                .update(format.with_content(display.as_str()));
            state.source = Some(self.value.to_string());
            state.width = size.width;
            state.display = display;
        }
        layout::Node::new(limits.resolve(self.width, self.size * 1.3, state.paragraph.min_bounds()))
    }
    fn operate(
        &mut self,
        _: &mut widget::Tree,
        layout: advanced::Layout<'_>,
        _: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.text(None, layout.bounds(), self.value.as_ref());
    }
    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: advanced::Layout<'_>,
        _: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use text::Renderer as _;
        if let Some(clip) = layout.bounds().intersection(viewport) {
            renderer.fill_paragraph(
                tree.state.downcast_ref::<State>().paragraph.raw(),
                layout.position(),
                self.style
                    .as_ref()
                    .and_then(|f| f(theme).color)
                    .unwrap_or(style.text_color),
                clip,
            );
        }
    }
}
impl<'a, Message: 'a> From<Label<'a>> for Element<'a, Message> {
    fn from(label: Label<'a>) -> Self {
        Element::new(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    #[test]
    fn ellipsis_preserves_graphemes_and_updates_after_resize_or_rename() {
        let renderer = crate::test_renderer(Font::DEFAULT, Pixels(13.0));
        let source = "Café 👩🏽‍💻 🇯🇵 e\u{301} — Research and references";
        let mut view: Element<'_, Message> = label(source, 13).into();
        let mut tree = widget::Tree::new(&view);
        for width in [0.0, 5.0, 20.0, 60.0, 130.0, 500.0, 40.0, 500.0] {
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(width, 100.0)),
            );
            let state = tree.state.downcast_ref::<State>();
            assert_eq!(node.size().height, 13.0 * 1.3);
            assert!(state.paragraph.min_width() <= width + 0.01);
            if width == 500.0 {
                assert_eq!(state.display, source);
            } else if !state.display.is_empty() {
                let prefix = state.display.strip_suffix('…').unwrap();
                assert!(source.starts_with(prefix));
                assert!(
                    source
                        .grapheme_indices(true)
                        .any(|(offset, _)| offset == prefix.len())
                );
            }
        }
        let mut renamed: Element<'_, Message> = label("New\nname", 13).into();
        tree.diff(renamed.as_widget());
        renamed.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(500.0, 100.0)),
        );
        assert_eq!(tree.state.downcast_ref::<State>().display, "New name");
    }
}
