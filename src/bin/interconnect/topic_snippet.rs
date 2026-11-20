//! A cached rich, grapheme-safe single line. Its disclosure button owns input.
use super::{Message, shell};
use iced::{
    Element, Font, Length, Pixels, Rectangle, Size,
    advanced::{
        self, layout, mouse, renderer,
        text::{self, Paragraph as _},
        widget,
    },
    widget::rich_text,
};
use interconnect::{message_document::Run, settings::ThemeMode};
use unicode_segmentation::UnicodeSegmentation;

type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;
type Span = text::Span<'static, (), Font>;
const SIZE: f32 = 12.0;
const HEIGHT: f32 = SIZE * 1.3;

pub(super) fn view(runs: &[Run], mode: ThemeMode) -> Element<'_, Message> {
    let theme = super::appearance::theme(mode);
    let palette = shell::palette(&theme);
    let spans = runs
        .iter()
        .map(|run| {
            let style = &run.style;
            let font = Font {
                weight: if style.bold {
                    iced::font::Weight::Bold
                } else {
                    iced::font::Weight::Normal
                },
                style: if style.italic {
                    iced::font::Style::Italic
                } else {
                    iced::font::Style::Normal
                },
                ..if style.code {
                    Font::MONOSPACE
                } else {
                    Font::with_name("Inter")
                }
            };
            let mut span = text::Span::new(run.content.text().to_owned())
                .font(font)
                .color(if style.link.is_some() {
                    palette.accent
                } else {
                    palette.muted
                })
                .underline(style.underline || style.link.is_some())
                .strikethrough(style.strike);
            if style.code {
                span = span.background(palette.toolbar);
            }
            span
        })
        .collect();
    Element::new(Snippet {
        spans,
        content: rich_text(Vec::<Span>::new()).into(),
    })
}
struct Snippet<'a> {
    spans: Vec<Span>,
    content: Element<'a, Message>,
}
#[derive(Default)]
struct State {
    source: Vec<Span>,
    display: Vec<Span>,
    width: Option<f32>,
}
fn prefix(spans: &[Span], end: usize, ellipsis: bool) -> Vec<Span> {
    let mut offset = 0;
    let mut result = Vec::new();
    for span in spans {
        if offset >= end {
            break;
        }
        let mut part = span.clone();
        let length = span.text.len().min(end - offset);
        part.text = span.text[..length].to_owned().into();
        result.push(part);
        offset += span.text.len();
    }
    if ellipsis {
        result.push(text::Span::new("…"));
    }
    result
}
fn measure(spans: &[Span]) -> f32 {
    Paragraph::with_spans(text::Text {
        content: spans,
        bounds: Size::new(f32::INFINITY, HEIGHT),
        size: Pixels(SIZE),
        line_height: text::LineHeight::Relative(1.3),
        font: Font::with_name("Inter"),
        align_x: text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    })
    .min_width()
}
impl advanced::Widget<Message, iced::Theme, iced::Renderer> for Snippet<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(HEIGHT))
    }
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.content)]
    }
    fn diff(&self, _tree: &mut widget::Tree) {
        // The child type is fixed. Reconcile it during layout, after restoring
        // the cached displayed spans instead of the constructor's empty text.
    }
    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let state = tree.state.downcast_mut::<State>();
        if state.width != Some(width) || state.source != self.spans {
            state.display = if measure(&self.spans) <= width {
                self.spans.clone()
            } else if measure(&[text::Span::new("…")]) > width {
                Vec::new()
            } else {
                let source: String = self.spans.iter().map(|span| span.text.as_ref()).collect();
                let ends: Vec<_> = source
                    .grapheme_indices(true)
                    .map(|(offset, g)| offset + g.len())
                    .collect();
                let (mut low, mut high) = (0, ends.len());
                while low < high {
                    let mid = (low + high).div_ceil(2);
                    if measure(&prefix(&self.spans, ends[mid - 1], true)) <= width {
                        low = mid;
                    } else {
                        high = mid - 1;
                    }
                }
                prefix(&self.spans, if low == 0 { 0 } else { ends[low - 1] }, true)
            };
            state.source = self.spans.clone();
            state.width = Some(width);
        }
        self.content = rich_text(state.display.clone())
            .size(SIZE)
            .font(Font::with_name("Inter"))
            .line_height(1.3)
            .wrapping(text::Wrapping::None)
            .width(Length::Fill)
            .height(HEIGHT)
            .style(shell::muted_text)
            .into();
        tree.children[0].diff(self.content.as_widget());
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        layout::Node::with_children(
            limits.resolve(Length::Fill, HEIGHT, child.size()),
            vec![child],
        )
    }
    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: advanced::Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }
    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: advanced::Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(clip) = layout.bounds().intersection(viewport) {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout.children().next().unwrap(),
                cursor,
                &clip,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interconnect::message_document::{Inline, Style};
    #[test]
    fn rich_snippet_preserves_styles_and_ellipsizes_at_grapheme_boundaries() {
        let runs = vec![
            Run {
                content: Inline::Text("Café 👩🏽‍💻".into()),
                style: Style {
                    bold: true,
                    ..Default::default()
                },
            },
            Run {
                content: Inline::Text(" 🇯🇵 e\u{301}".into()),
                style: Style {
                    italic: true,
                    ..Default::default()
                },
            },
            Run {
                content: Inline::Text(" reference".into()),
                style: Style {
                    link: Some("https://example.org/".into()),
                    ..Default::default()
                },
            },
            Run {
                content: Inline::Text(" code".into()),
                style: Style {
                    code: true,
                    strike: true,
                    ..Default::default()
                },
            },
        ];
        let source: String = runs.iter().map(|run| run.content.text()).collect();
        let renderer = crate::test_renderer(Font::DEFAULT, Pixels(12.0));
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let mut tree = widget::Tree::new(view(&runs, mode));
            for width in [0.0, 5.0, 20.0, 70.0, 160.0, 700.0, 40.0, 700.0, 700.0] {
                let mut element = view(&runs, mode);
                tree.diff(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(width, 100.0)),
                );
                let state = tree.state.downcast_ref::<State>();
                assert!(measure(&state.display) <= width + 0.01);
                assert!(node.size().height <= HEIGHT);
                let text: String = state
                    .display
                    .iter()
                    .map(|span| span.text.as_ref())
                    .collect();
                if width == 700.0 {
                    assert_eq!(text, source);
                } else if !text.is_empty() {
                    let prefix = text.strip_suffix('…').unwrap();
                    assert!(source.starts_with(prefix));
                    assert!(
                        source
                            .grapheme_indices(true)
                            .any(|(offset, _)| offset == prefix.len())
                    );
                }
                assert_eq!(
                    state.source[0].font.unwrap().weight,
                    iced::font::Weight::Bold
                );
                assert_eq!(
                    state.source[1].font.unwrap().style,
                    iced::font::Style::Italic
                );
                assert!(state.source[2].underline);
                assert!(state.source[3].strikethrough);
                assert!(state.source[3].highlight.is_some());
            }
        }
    }
}
