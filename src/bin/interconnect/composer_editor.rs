//! Native editor scrolling with a quiet scrollbar and an event-driven resize grip.
use super::{DraftKey, Message, formatted, shell};
use iced::{
    Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree},
    },
    widget::text_editor,
};
use interconnect::composition::NamedMention;

pub(super) const MIN_HEIGHT: f32 = 56.0;
pub(super) const MAX_HEIGHT: f32 = 320.0;
pub(super) const GRIP_HEIGHT: f32 = 8.0;

pub(super) struct Options {
    pub height: f32,
    pub max_height: f32,
    pub enabled: bool,
    pub padding: iced::Padding,
    pub grip_height: f32,
}

pub(super) fn view<'a>(
    editor: Element<'a, Message>,
    content: &'a text_editor::Content,
    mentions: &'a [NamedMention],
    key: &DraftKey,
    generation: u64,
    options: Options,
) -> Element<'a, Message> {
    Element::new(Editor {
        editor,
        content,
        mentions,
        key: key.clone(),
        generation,
        height: options.height.clamp(MIN_HEIGHT, options.max_height),
        max_height: options.max_height,
        enabled: options.enabled,
        padding: options.padding,
        grip_height: options.grip_height,
    })
}
struct Editor<'a> {
    editor: Element<'a, Message>,
    content: &'a text_editor::Content,
    mentions: &'a [NamedMention],
    key: DraftKey,
    generation: u64,
    height: f32,
    max_height: f32,
    enabled: bool,
    padding: iced::Padding,
    grip_height: f32,
}
#[derive(Clone, Copy, Debug, Default)]
struct Metrics {
    offset: f32,
    total: f32,
    visible: f32,
    line: f32,
}
impl Metrics {
    fn maximum(self) -> f32 {
        (self.total - self.visible).max(0.0)
    }
    fn thumb(self, rail: Rectangle) -> Option<Rectangle> {
        let maximum = self.maximum();
        if maximum <= 0.5 || rail.height <= 0.0 {
            return None;
        }
        let height =
            (rail.height * self.visible / self.total).clamp(18.0_f32.min(rail.height), rail.height);
        Some(Rectangle {
            y: rail.y + (rail.height - height) * (self.offset / maximum).clamp(0.0, 1.0),
            height,
            ..rail
        })
    }
}
#[derive(Clone, Copy)]
enum Drag {
    Resize { y: f32, height: f32 },
    Scroll { grab: f32 },
}
#[derive(Default)]
struct State {
    key: Option<DraftKey>,
    generation: u64,
    drag: Option<Drag>,
    metrics: Metrics,
    line_offsets: Vec<usize>,
    last_height: Option<f32>,
    hovered: bool,
}
impl Editor<'_> {
    fn rail(&self, bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x + bounds.width - 9.0,
            y: bounds.y + self.grip_height + self.padding.top,
            width: 4.0,
            height: (bounds.height - self.grip_height - (self.padding.top + self.padding.bottom))
                .max(0.0),
        }
    }
    fn scroll_to(
        &self,
        state: &State,
        rail: Rectangle,
        y: f32,
        grab: f32,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(thumb) = state.metrics.thumb(rail) else {
            return;
        };
        let travel = rail.height - thumb.height;
        if travel <= 0.0 {
            return;
        }
        let fraction = ((y - rail.y - grab) / travel).clamp(0.0, 1.0);
        if (fraction <= 0.0 && state.metrics.offset <= 0.5)
            || (fraction >= 1.0 && state.metrics.offset >= state.metrics.maximum() - 0.5)
        {
            return;
        }
        let lines = if fraction >= 1.0 {
            i32::MAX / 2
        } else if fraction <= 0.0 {
            i32::MIN / 2
        } else {
            ((fraction * state.metrics.maximum() - state.metrics.offset) / state.metrics.line)
                .round() as i32
        };
        if lines != 0 {
            shell.publish(Message::Edit(
                self.key.clone(),
                text_editor::Action::Scroll { lines },
            ));
        }
    }
}
/// Native glyph clusters determine the decorations, including partial ligatures,
/// wrapping, scroll offsets, emoji and disjoint visual segments in bidi text.
fn mention_regions(
    buffer: &iced::advanced::graphics::text::cosmic_text::Buffer,
    line_offsets: &[usize],
    mentions: &[NamedMention],
) -> Vec<(usize, Rectangle)> {
    let mut regions = Vec::new();
    for line in buffer.layout_runs() {
        let Some(start) = line_offsets.get(line.line_i) else {
            continue;
        };
        let mut segments = Vec::new();
        for glyph in line.glyphs {
            if glyph.w <= 0.0 {
                continue;
            }
            let from = start + glyph.start;
            let to = start + glyph.end;
            let first = mentions.partition_point(|mention| mention.range.end <= from);
            for (index, mention) in mentions
                .iter()
                .enumerate()
                .skip(first)
                .take_while(|(_, mention)| mention.range.start < to)
            {
                if mention.range.end <= from {
                    continue;
                }
                let (left, width) = formatted::highlight_fraction(
                    &line.text[glyph.start..glyph.end],
                    from,
                    &mention.range,
                    glyph.level.is_rtl(),
                );
                if width > 0.0 {
                    segments.push((
                        index,
                        Rectangle::new(
                            Point::new(glyph.x + left * glyph.w, line.line_top),
                            Size::new(width * glyph.w, line.line_height),
                        ),
                    ));
                }
            }
        }
        segments.sort_by(|(a, ar), (b, br)| a.cmp(b).then(ar.x.total_cmp(&br.x)));
        let mut current: Option<(usize, Rectangle)> = None;
        for (index, segment) in segments {
            if let Some((previous_index, previous)) = current.as_mut()
                && *previous_index == index
                && segment.x <= previous.x + previous.width + 0.5
            {
                previous.width = previous.width.max(segment.x + segment.width - previous.x);
            } else {
                if let Some(previous) = current.take() {
                    regions.push(previous);
                }
                current = Some((index, segment));
            }
        }
        if let Some(previous) = current {
            regions.push(previous);
        }
    }
    regions
}

impl Widget<Message, Theme, Renderer> for Editor<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            key: Some(self.key.clone()),
            generation: self.generation,
            ..Default::default()
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.editor)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.key.as_ref() != Some(&self.key)
            || state.generation != self.generation
            || !self.enabled
        {
            state.drag = None;
            state.last_height = None;
            state.key = Some(self.key.clone());
            state.generation = self.generation;
        }
        tree.diff_children(std::slice::from_ref(&self.editor));
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.height + self.grip_height))
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits
            .shrink(Size::new(0.0, self.grip_height))
            .height(self.height);
        let node = self
            .editor
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits);
        let size = Size::new(node.size().width, node.size().height + self.grip_height);
        let (metrics, line_offsets) = self.content.with_editor(|editor| {
            let buffer = editor.buffer();
            let line = buffer.metrics().line_height;
            let scroll = buffer.scroll();
            let mut total = 0.0;
            let mut source_offset = 0;
            let mut line_offsets = Vec::with_capacity(if self.mentions.is_empty() {
                0
            } else {
                buffer.lines.len()
            });
            let mut offset = scroll.vertical;
            for (index, paragraph) in buffer.lines.iter().enumerate() {
                if !self.mentions.is_empty() {
                    line_offsets.push(source_offset);
                    source_offset += paragraph.text().len() + paragraph.ending().as_str().len();
                }
                // Offscreen paragraphs are lazy; refine their extent when visited.
                let height = paragraph.layout_opt().map_or(line, |rows| {
                    rows.iter()
                        .map(|row| row.line_height_opt.unwrap_or(line))
                        .sum()
                });
                total += height;
                if index < scroll.line {
                    offset += height;
                }
            }
            (
                Metrics {
                    offset,
                    total,
                    visible: (size.height
                        - self.grip_height
                        - (self.padding.top + self.padding.bottom))
                        .max(0.0),
                    line,
                },
                line_offsets,
            )
        });
        let state = tree.state.downcast_mut::<State>();
        state.metrics = metrics;
        state.line_offsets = line_offsets;
        layout::Node::with_children(size, vec![node.move_to(Point::new(0.0, self.grip_height))])
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.container(
            Some(&widget::Id::new("composer-editor-surface")),
            layout.bounds(),
        );
        self.editor.as_widget_mut().operate(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
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
        let state = tree.state.downcast_mut::<State>();
        let hovered = cursor.is_over(layout.bounds()) && cursor.is_over(*viewport);
        if hovered != state.hovered {
            state.hovered = hovered;
            shell.request_redraw();
        }
        if (!self.enabled
            || matches!(
                event,
                Event::Mouse(mouse::Event::CursorLeft)
                    | Event::Window(
                        iced::window::Event::Unfocused | iced::window::Event::Resized(_)
                    )
            ))
            && state.drag.take().is_some()
        {
            shell.request_redraw();
        }
        let bounds = layout.bounds();
        let rail = self.rail(bounds);
        if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
            match state.drag {
                Some(Drag::Resize { y, height }) => {
                    let height = (height + y - position.y).clamp(MIN_HEIGHT, self.max_height);
                    if state.last_height != Some(height) {
                        state.last_height = Some(height);
                        shell.publish(Message::ResizeComposer {
                            key: self.key.clone(),
                            generation: self.generation,
                            height,
                        });
                    }
                    shell.capture_event();
                    return;
                }
                Some(Drag::Scroll { grab }) => {
                    self.scroll_to(state, rail, position.y, grab, shell);
                    shell.capture_event();
                    return;
                }
                None => {}
            }
        }
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) && state.drag.take().is_some()
        {
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        if self.enabled
            && cursor.is_over(*viewport)
            && cursor.is_over(bounds)
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
        {
            let point = cursor.position().unwrap();
            if point.y <= bounds.y + self.grip_height {
                state.drag = Some(Drag::Resize {
                    y: point.y,
                    height: self.height,
                });
                state.last_height = Some(self.height);
                shell.capture_event();
                shell.request_redraw();
                return;
            }
            if let Some(thumb) = state.metrics.thumb(rail)
                && point.x >= rail.x - 4.0
                && point.y >= rail.y
                && point.y <= rail.y + rail.height
            {
                let grab = if thumb.contains(point) {
                    point.y - thumb.y
                } else {
                    thumb.height / 2.0
                };
                state.drag = Some(Drag::Scroll { grab });
                self.scroll_to(state, rail, point.y, grab, shell);
                shell.capture_event();
                shell.request_redraw();
                return;
            }
        }
        self.editor.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
        use iced::advanced::Renderer as _;
        let native_layout = layout.children().next().unwrap();
        let text_bounds = native_layout.bounds().shrink(self.padding);
        if !self.mentions.is_empty()
            && let Some(clip) = text_bounds.intersection(viewport)
        {
            let state = tree.state.downcast_ref::<State>();
            self.content.with_editor(|editor| {
                let regions = mention_regions(editor.buffer(), &state.line_offsets, self.mentions);
                renderer.with_layer(clip, |renderer| {
                    for (index, region) in regions {
                        let accent = if self.mentions[index].target.is_room_wide() {
                            shell::palette(theme).room_wide_accent
                        } else if self.mentions[index].target.room_id().is_some() {
                            shell::palette(theme).room_accent
                        } else {
                            shell::palette(theme).accent
                        };
                        let bounds = region + (text_bounds.position() - Point::ORIGIN);
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds,
                                border: iced::Border::default().rounded(4.0),
                                ..Default::default()
                            },
                            if self.mentions[index].target.room_id().is_some()
                                || self.mentions[index].target.is_room_wide()
                            {
                                accent.scale_alpha(0.12)
                            } else {
                                shell::palette(theme).mention
                            },
                        );
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    Point::new(bounds.x, bounds.y + bounds.height - 1.5),
                                    Size::new(bounds.width, 1.0),
                                ),
                                ..Default::default()
                            },
                            accent.scale_alpha(0.65),
                        );
                    }
                });
            });
        }
        self.editor.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
            viewport,
        );
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let over = cursor.is_over(bounds) && cursor.is_over(*viewport);
        let grip = Rectangle {
            x: bounds.center_x() - 14.0,
            y: bounds.y + (self.grip_height - 2.0) / 2.0,
            width: 28.0,
            height: 2.0,
        };
        if let Some(bounds) = grip.intersection(viewport) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border::default().rounded(1),
                    ..Default::default()
                },
                if over || state.drag.is_some() {
                    shell::palette(theme).muted.scale_alpha(0.65)
                } else {
                    shell::palette(theme).line
                },
            );
        }
        if let Some(thumb) = state.metrics.thumb(self.rail(bounds))
            && let Some(bounds) = thumb.intersection(viewport)
        {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border::default().rounded(2),
                    ..Default::default()
                },
                if over || state.drag.is_some() {
                    shell::palette(theme).scroller
                } else {
                    shell::palette(theme).line
                },
            );
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
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        if self.enabled
            && (matches!(state.drag, Some(Drag::Resize { .. }))
                || (cursor.is_over(*viewport)
                    && cursor.is_over(bounds)
                    && cursor
                        .position()
                        .is_some_and(|point| point.y <= bounds.y + self.grip_height)))
        {
            return if matches!(state.drag, Some(Drag::Resize { .. })) {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::Grab
            };
        }
        let rail = self.rail(bounds);
        if self.enabled
            && (matches!(state.drag, Some(Drag::Scroll { .. }))
                || (state.metrics.thumb(rail).is_some()
                    && cursor.is_over(*viewport)
                    && cursor.position().is_some_and(|p| {
                        p.x >= rail.x - 4.0
                            && p.x <= bounds.x + bounds.width
                            && p.y >= rail.y
                            && p.y <= rail.y + rail.height
                    })))
        {
            return mouse::Interaction::Pointer;
        }
        self.editor.as_widget().mouse_interaction(
            &tree.children[0],
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.editor.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{Renderer as _, Widget};

    fn editor<'a>(content: &'a text_editor::Content) -> Editor<'a> {
        let key = DraftKey {
            room: "room".into(),
            root: None,
        };
        let target = key.clone();
        let native = text_editor(content)
            .height(Length::Fill)
            .size(14)
            .padding(iced::Padding {
                top: 12.0,
                right: 18.0,
                bottom: 8.0,
                left: 12.0,
            })
            .cursor_blinking(false)
            .style(shell::composer_editor_style)
            .on_action(move |action| Message::Edit(target.clone(), action));
        Editor {
            editor: native.into(),
            content,
            mentions: &[],
            key,
            generation: 7,
            height: 56.0,
            max_height: 200.0,
            enabled: true,
            grip_height: GRIP_HEIGHT,
            padding: iced::Padding {
                top: 12.0,
                right: 18.0,
                bottom: 8.0,
                left: 12.0,
            },
        }
    }
    fn dispatch(
        editor: &mut Editor<'_>,
        tree: &mut Tree,
        renderer: &Renderer,
        node: &layout::Node,
        event: Event,
        point: Point,
    ) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        editor.update(
            tree,
            &event,
            Layout::new(node),
            mouse::Cursor::Available(point),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(Size::new(300.0, 400.0)),
        );
        assert!(!matches!(
            shell.redraw_request(),
            iced::window::RedrawRequest::At(_)
        ));
        messages
    }
    fn mention(source: &str, label: &str) -> NamedMention {
        let start = source.find(label).unwrap();
        NamedMention {
            range: start..start + label.len(),
            target: interconnect::composition::MentionTarget::User {
                user_id: "@maya:example.org".parse().unwrap(),
            },
        }
    }
    fn render(
        editor: &Editor<'_>,
        tree: &Tree,
        node: &layout::Node,
        renderer: &mut Renderer,
        viewport: Rectangle,
    ) -> tiny_skia::Pixmap {
        renderer.reset(viewport);
        editor.draw(
            tree,
            renderer,
            &shell::theme(),
            &renderer::Style {
                text_color: crate::appearance::LIGHT.ink,
            },
            Layout::new(node),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        let Renderer::Secondary(software) = renderer else {
            panic!("software renderer");
        };
        let mut pixels =
            tiny_skia::Pixmap::new(viewport.width as u32, viewport.height as u32).unwrap();
        let mut mask = tiny_skia::Mask::new(viewport.width as u32, viewport.height as u32).unwrap();
        software.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &iced::advanced::graphics::Viewport::with_physical_size(
                Size::new(viewport.width as u32, viewport.height as u32),
                1.0,
            ),
            &[viewport],
            iced::Color::WHITE,
        );
        pixels
    }
    #[test]
    fn mention_background_is_visible_and_removing_binding_preserves_layout_and_text() {
        let source = "Hi Maya Chen, welcome!";
        let content = text_editor::Content::with_text(source);
        let mentions = [mention(source, "Maya Chen")];
        let mut view = editor(&content);
        view.mentions = &mentions;
        let mut renderer = crate::test_renderer(iced::Font::with_name("Inter"), iced::Pixels(14.0));
        let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
        let limits = layout::Limits::new(Size::ZERO, Size::new(300.0, 100.0));
        let node = view.layout(&mut tree, &renderer, &limits);
        let cursor = content.cursor();
        let viewport = Rectangle::with_size(Size::new(300.0, 100.0));
        let marked = render(&view, &tree, &node, &mut renderer, viewport);
        view.mentions = &[];
        view.diff(&mut tree);
        let plain_node = view.layout(&mut tree, &renderer, &limits);
        let plain = render(&view, &tree, &plain_node, &mut renderer, viewport);
        assert_eq!(node.bounds(), plain_node.bounds());
        assert_eq!(content.cursor(), cursor);
        assert_eq!(content.text(), source);
        // The software framebuffer is BGRA; its first channel contains blue.
        let changed = marked
            .pixels()
            .iter()
            .zip(plain.pixels())
            .filter(|(marked, plain)| {
                marked != plain && marked.red() > marked.blue().saturating_add(8)
            })
            .count();
        assert!(
            changed > 200,
            "mention background and underline must be visible: {changed}"
        );
    }
    #[test]
    fn mention_regions_follow_wrapped_unicode_and_rtl_glyphs() {
        let source = "Before Maya Chen after.\nלפני מיה אחרי.\n👋 Zoë returns.";
        let content = text_editor::Content::with_text(source);
        let mut mentions = [
            mention(source, "Maya Chen"),
            mention(source, "מיה"),
            mention(source, "👋 Zoë"),
        ];
        mentions.sort_by_key(|mention| mention.range.start);
        let mut view = editor(&content);
        view.height = 200.0;
        view.mentions = &mentions;
        let renderer = crate::test_renderer(iced::Font::with_name("Inter"), iced::Pixels(14.0));
        let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
        let _node = view.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(130.0, 240.0)),
        );
        let offsets = &tree.state.downcast_ref::<State>().line_offsets;
        content.with_editor(|editor| {
            let regions = mention_regions(editor.buffer(), offsets, &mentions);
            assert!(regions.len() >= 3);
            for line in editor.buffer().layout_runs() {
                for glyph in line.glyphs {
                    let start = offsets[line.line_i] + glyph.start;
                    let end = offsets[line.line_i] + glyph.end;
                    if glyph.w > 0.0
                        && mentions
                            .iter()
                            .any(|mention| mention.range.start <= start && mention.range.end >= end)
                    {
                        let center = Point::new(
                            glyph.x + glyph.w / 2.0,
                            line.line_top + line.line_height / 2.0,
                        );
                        assert!(
                            regions.iter().any(|(_, region)| region.contains(center)),
                            "mention background must cover its shaped glyph at {center:?}"
                        );
                    }
                }
            }
        });
    }
    #[test]
    fn scrolled_mention_uses_absolute_source_offsets_and_clips_to_the_editor() {
        let source = format!(
            "{}Maya Chen",
            (0..30)
                .map(|i| format!("Paragraph {i}\n"))
                .collect::<String>()
        );
        let mentions = [mention(&source, "Maya Chen")];
        let mut content = text_editor::Content::with_text(&source);
        content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        content.perform(text_editor::Action::Scroll {
            lines: i32::MAX / 2,
        });
        let mut view = editor(&content);
        view.mentions = &mentions;
        let mut renderer = crate::test_renderer(iced::Font::with_name("Inter"), iced::Pixels(14.0));
        let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
        let node = view.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 100.0)),
        );
        let offsets = &tree.state.downcast_ref::<State>().line_offsets;
        content.with_editor(|editor| {
            assert!(editor.buffer().scroll().line > 0);
            assert!(!mention_regions(editor.buffer(), offsets, &mentions).is_empty());
        });
        let viewport = Rectangle::with_size(Size::new(300.0, 100.0));
        let pixels = render(&view, &tree, &node, &mut renderer, viewport);
        let text = Layout::new(&node)
            .children()
            .next()
            .unwrap()
            .bounds()
            .shrink(view.padding);
        for (index, pixel) in pixels.pixels().iter().enumerate() {
            if pixel.red() > pixel.blue().saturating_add(20) && pixel.green() > 100 {
                let point = Point::new((index % 300) as f32 + 0.5, (index / 300) as f32 + 0.5);
                assert!(
                    text.contains(point),
                    "mention decoration must not paint outside the text viewport"
                );
            }
        }
    }

    #[test]
    fn resize_is_clamped_deduplicated_and_canceled_on_release_focus_loss_or_new_draft() {
        let content = text_editor::Content::new();
        let mut editor = editor(&content);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&editor as &dyn Widget<Message, Theme, Renderer>);
        let node = editor.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 400.0)),
        );
        let grip = Point::new(150.0, 3.0);
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            press.clone(),
            grip,
        );
        for (y, expected) in [
            (-97.0, Some(156.0)),
            (-900.0, Some(200.0)),
            (-900.0, None),
            (900.0, Some(56.0)),
        ] {
            let point = Point::new(150.0, y);
            let messages = dispatch(
                &mut editor,
                &mut tree,
                &renderer,
                &node,
                Event::Mouse(mouse::Event::CursorMoved { position: point }),
                point,
            );
            let height = messages.iter().find_map(|message| match message {
                Message::ResizeComposer {
                    generation: 7,
                    height,
                    ..
                } => Some(*height),
                _ => None,
            });
            assert_eq!(height, expected);
            editor.diff(&mut tree); // A new app view must not cancel an ongoing drag.
        }
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Point::new(500.0, 500.0),
        );
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            press.clone(),
            grip,
        );
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Window(iced::window::Event::Unfocused),
            grip,
        );
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
        dispatch(&mut editor, &mut tree, &renderer, &node, press, grip);
        editor.key.room = "another-room".into();
        editor.diff(&mut tree);
        assert!(tree.state.downcast_ref::<State>().drag.is_none());
    }
    #[test]
    fn scrollbar_tracks_wrapping_and_scrolls_without_changing_text_or_selection() {
        let source =
            "A paragraph with enough words to wrap across several visual lines.\n".repeat(40);
        let mut content = text_editor::Content::with_text(&source);
        content.move_to(iced::advanced::text::editor::Cursor {
            position: iced::advanced::text::editor::Position { line: 0, column: 1 },
            selection: Some(iced::advanced::text::editor::Position { line: 0, column: 0 }),
        });
        let cursor = content.cursor();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut editor = editor(&content);
        let mut tree = Tree::new(&editor as &dyn Widget<Message, Theme, Renderer>);
        let limits = layout::Limits::new(Size::ZERO, Size::new(300.0, 400.0));
        let node = editor.layout(&mut tree, &renderer, &limits);
        let rail = editor.rail(Rectangle::with_size(node.size()));
        let metrics = tree.state.downcast_ref::<State>().metrics;
        assert!(metrics.thumb(rail).is_some());
        let bottom = Point::new(rail.center_x(), rail.y + rail.height);
        let messages = dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bottom,
        );
        drop(editor);
        for message in messages {
            if let Message::Edit(_, action) = message {
                content.perform(action);
            }
        }
        assert_eq!(content.cursor(), cursor);
        assert_eq!(content.text(), source);
        let mut editor = self::editor(&content);
        let node = editor.layout(&mut tree, &renderer, &limits);
        let metrics = tree.state.downcast_ref::<State>().metrics;
        assert!(metrics.offset > 0.0);
        assert!(metrics.offset >= metrics.maximum() - metrics.line);
        let thumb = metrics
            .thumb(editor.rail(Rectangle::with_size(node.size())))
            .unwrap();
        assert!(thumb.y + thumb.height <= rail.y + rail.height + 0.1);
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            bottom,
        );
        dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            thumb.center(),
        );
        let point = Point::new(rail.center_x(), rail.y);
        let messages = dispatch(
            &mut editor,
            &mut tree,
            &renderer,
            &node,
            Event::Mouse(mouse::Event::CursorMoved { position: point }),
            point,
        );
        drop(editor);
        for message in messages {
            if let Message::Edit(_, action) = message {
                content.perform(action);
            }
        }
        let mut editor = self::editor(&content);
        editor.layout(&mut tree, &renderer, &limits);
        assert!(tree.state.downcast_ref::<State>().metrics.offset < 0.5);
        assert_eq!(content.cursor(), cursor);
        assert_eq!(content.text(), source);
        let short = text_editor::Content::with_text("A short draft");
        let mut short_editor = self::editor(&short);
        let mut short_tree = Tree::new(&short_editor as &dyn Widget<Message, Theme, Renderer>);
        short_editor.layout(&mut short_tree, &renderer, &limits);
        assert!(
            short_tree
                .state
                .downcast_ref::<State>()
                .metrics
                .thumb(rail)
                .is_none()
        );
    }
    #[test]
    fn resize_grip_routes_through_the_full_app_view() {
        #[derive(Default)]
        struct Probe(Option<Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                if id == Some(&widget::Id::new("composer-editor-surface")) {
                    self.0 = Some(bounds);
                }
            }
        }
        let app = crate::tests::demo_app();
        let mut view = app.view();
        let mut tree = Tree::new(&view);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(1280.0, 900.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        let bounds = probe.0.unwrap();
        let grip = Point::new(bounds.center_x(), bounds.y + 3.0);
        let target = grip - Vector::new(0.0, 120.0);
        let mut messages = Vec::new();
        for (event, point) in [
            (
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                grip,
            ),
            (
                Event::Mouse(mouse::Event::CursorMoved { position: target }),
                target,
            ),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::ResizeComposer { height, .. } if *height == 176.0)),
            "grip must emit resize: {bounds:?} {messages:?}"
        );
    }
    #[test]
    fn sizes_are_per_draft_and_stale_or_modal_resize_messages_are_ignored() {
        let mut app = crate::tests::demo_app();
        let room = app.draft_key(false).unwrap();
        let generation = app.selection.room.as_ref().unwrap().0;
        let resize = |key, generation, height| Message::ResizeComposer {
            key,
            generation,
            height,
        };
        let _ = app.update(resize(room.clone(), generation, 200.0));
        let _ = app.update(Message::Thread("review".into()));
        let thread = app.draft_key(true).unwrap();
        let _ = app.update(resize(
            thread.clone(),
            app.selection.thread.as_ref().unwrap().0,
            130.0,
        ));
        assert_eq!(app.drafts[&room].composer_height, Some(200.0));
        assert_eq!(app.drafts[&thread].composer_height, Some(130.0));
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.drafts[&room].composer_height, Some(200.0));
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(resize(room.clone(), generation, 300.0));
        let _ = app.update(Message::Room(room.room.clone()));
        let _ = app.update(resize(room.clone(), generation, 300.0));
        let current = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(resize(room.clone(), current, f32::NAN));
        let _ = app.settings_update(crate::settings::Event::Open);
        let _ = app.update(resize(room.clone(), current, 300.0));
        assert_eq!(app.drafts[&room].composer_height, Some(200.0));
    }
}
