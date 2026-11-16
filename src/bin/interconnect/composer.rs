//! Shared composer surface. Focus follows the real editor without app messages or timers.
use super::{Message, shell};
use iced::{
    Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree},
    },
};

pub(super) fn frame<'a>(
    content: Element<'a, Message>,
    editor: &'static str,
) -> Element<'a, Message> {
    Element::new(Frame {
        content,
        editor: widget::Id::new(editor),
    })
}
struct Frame<'a> {
    content: Element<'a, Message>,
    editor: widget::Id,
}
#[derive(Default)]
struct State {
    focused: bool,
}
struct Focus<'a> {
    id: &'a widget::Id,
    focused: bool,
}
impl widget::Operation for Focus<'_> {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
        f(self);
    }
    fn focusable(
        &mut self,
        id: Option<&widget::Id>,
        _: Rectangle,
        state: &mut dyn widget::operation::Focusable,
    ) {
        if id == Some(self.id) {
            self.focused = state.is_focused();
        }
    }
}
impl Frame<'_> {
    fn sync_focus(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) -> bool {
        let mut focus = Focus {
            id: &self.editor,
            focused: false,
        };
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut focus);
        let state = tree.state.downcast_mut::<State>();
        let changed = state.focused != focus.focused;
        state.focused = focus.focused;
        changed
    }
}
impl Widget<Message, Theme, Renderer> for Frame<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
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
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        self.sync_focus(tree, layout, renderer);
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if self.sync_focus(tree, layout, renderer) {
            shell.request_redraw();
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
        use iced::advanced::Renderer as _;
        if !layout.bounds().intersects(viewport) {
            return;
        }
        let focused = tree.state.downcast_ref::<State>().focused;
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: Border {
                    color: if focused { shell::ACCENT } else { shell::LINE },
                    width: if focused { 1.5 } else { 1.0 },
                    radius: 10.0.into(),
                },
                ..Default::default()
            },
            Color::WHITE,
        );
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
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
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
    use iced::widget::{column, text, text_editor};

    #[test]
    fn surface_tracks_editor_focus_and_settles_without_timers() {
        let content = text_editor::Content::with_text("A draft\nwith several\nlines");
        let editor = text_editor(&content)
            .id("room-composer")
            .height(56)
            .cursor_blinking(false)
            .style(shell::composer_editor_style)
            .on_action(|_| Message::Escape);
        let mut view = frame(column![editor, text("Toolbar")].into(), "room-composer");
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view);
        let size = Size::new(300.0, 200.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        for focused in [true, false, true] {
            let mut operation = if focused {
                widget::operation::focusable::focus(widget::Id::new("room-composer"))
            } else {
                widget::operation::focusable::focus(widget::Id::new("another-field"))
            };
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut operation);
            assert_eq!(tree.state.downcast_ref::<State>().focused, focused);
            for milliseconds in [0, 750, 1500] {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &Event::Window(iced::window::Event::RedrawRequested(
                        iced::time::Instant::now() + std::time::Duration::from_millis(milliseconds),
                    )),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &Rectangle::with_size(size),
                );
                assert!(!matches!(
                    shell.redraw_request(),
                    iced::window::RedrawRequest::At(_)
                ));
                if milliseconds > 0 {
                    assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                }
            }
        }
        assert_eq!(content.text(), "A draft\nwith several\nlines");
    }
}
