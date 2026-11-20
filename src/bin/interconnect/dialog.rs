//! Shared, bounded dialog frame. Only the body scrolls; no polling or timers.
use super::shell;
use iced::{
    Element, Length, Size,
    widget::{column, container, mouse_area, opaque, rule, scrollable},
};

pub(super) fn frame<'a, Message: 'a>(
    size: Size,
    max_width: f32,
    preferred_height: f32,
    id: &'static str,
    header: impl Into<Element<'a, Message>>,
    body: impl Into<Element<'a, Message>>,
    footer: impl Into<Element<'a, Message>>,
) -> iced::widget::Container<'a, Message> {
    container(column![
        container(header)
            .padding([18, 24])
            .width(Length::Fill)
            .id(format!("{id}-header")),
        rule::horizontal(1),
        scrollable(container(body).padding(24).width(Length::Fill))
            .id(format!("{id}-body"))
            .direction(shell::scrollbar())
            .style(shell::scroll_style)
            .height(Length::Fill),
        rule::horizontal(1),
        container(footer)
            .padding([14, 24])
            .width(Length::Fill)
            .id(format!("{id}-actions")),
    ])
    .id(format!("{id}-dialog"))
    .width((size.width - 32.0).clamp(1.0, max_width))
    .height((size.height - 32.0).clamp(1.0, preferred_height))
    .clip(true)
    .style(|theme| container::Style {
        border: iced::Border {
            color: shell::palette(theme).line,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..shell::surface(theme, shell::palette(theme).background)
    })
}

pub(super) fn modal<'a, Message: Clone + 'a>(
    card: impl Into<Element<'a, Message>>,
    ignore: Message,
) -> Element<'a, Message> {
    let scroll_ignore = ignore.clone();
    opaque(
        mouse_area(
            container(card)
                .padding(16)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|theme| shell::surface(theme, shell::palette(theme).scrim)),
        )
        .on_press(ignore)
        .on_scroll(move |_| scroll_ignore.clone()),
    )
}

/// Keep the shell in one stable widget slot while a modal covers it. Covered
/// widgets still paint, but cannot receive input, publish viewport demand, or
/// participate in the active dialog's focus cycle.
pub(super) fn underlay<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    blocked: bool,
) -> Element<'a, Message> {
    Element::new(Underlay {
        content: content.into(),
        blocked,
    })
}
struct Underlay<'a, Message> {
    content: Element<'a, Message>,
    blocked: bool,
}
impl<Message> iced::advanced::Widget<Message, iced::Theme, iced::Renderer>
    for Underlay<'_, Message>
{
    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> iced::Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        if !self.blocked {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        }
    }
    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        if !self.blocked {
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
        }
    }
    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            if self.blocked {
                iced::advanced::mouse::Cursor::Unavailable
            } else {
                cursor
            },
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &iced::Renderer,
    ) -> iced::advanced::mouse::Interaction {
        if self.blocked {
            iced::advanced::mouse::Interaction::None
        } else {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &iced::Rectangle,
        translation: iced::Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        if self.blocked {
            None
        } else {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{Shell, layout, mouse, widget};

    #[test]
    fn covering_the_shell_blocks_input_and_focus_without_resetting_scroll() {
        fn view(blocked: bool) -> Element<'static, ()> {
            underlay(
                scrollable(iced::widget::column![
                    iced::widget::button("Covered action")
                        .keyboard_navigation(true)
                        .on_press(()),
                    iced::widget::space().height(600),
                ])
                .id("covered-scroll")
                .height(Length::Fill),
                blocked,
            )
        }
        #[derive(Default)]
        struct Probe {
            offset: Option<f32>,
            controls: usize,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn focusable(
                &mut self,
                _: Option<&widget::Id>,
                _: iced::Rectangle,
                _: &mut dyn widget::operation::Focusable,
            ) {
                self.controls += 1;
            }
            fn scrollable(
                &mut self,
                _: Option<&widget::Id>,
                _: iced::Rectangle,
                _: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                self.offset = Some(translation.y);
            }
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(300.0, 200.0);
        let mut active = view(false);
        let mut tree = widget::Tree::new(&active);
        let node =
            active
                .as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        active.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut widget::operation::scrollable::scroll_to::<()>(
                "covered-scroll".into(),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(120.0),
                },
            ),
        );
        let mut before = Probe::default();
        active.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut before,
        );
        assert!(before.offset.unwrap() >= 119.0);
        assert_eq!(before.controls, 1);
        let mut covered = view(true);
        tree.diff(covered.as_widget());
        let node =
            covered
                .as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut hidden = Probe::default();
        covered.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut hidden,
        );
        assert!(hidden.offset.is_none());
        assert_eq!(hidden.controls, 0);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        covered.as_widget_mut().update(
            &mut tree,
            &iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            layout::Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
        assert!(messages.is_empty());
        let mut active = view(false);
        tree.diff(active.as_widget());
        let node =
            active
                .as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut after = Probe::default();
        active.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut after,
        );
        assert_eq!(after.offset, before.offset);
        assert_eq!(after.controls, before.controls);
    }
}
