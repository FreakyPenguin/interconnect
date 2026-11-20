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
