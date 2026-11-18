//! Shared presentation for control hints and contextual details.
use iced::{
    Border, Element,
    widget::{container, text, tooltip as iced_tooltip},
};

pub(super) fn tooltip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl text::IntoFragment<'a>,
    position: iced_tooltip::Position,
) -> iced_tooltip::Tooltip<'a, Message> {
    iced_tooltip(
        content,
        container(
            text(label)
                .size(11)
                .style(|theme| iced::widget::text::Style {
                    color: Some(super::shell::palette(theme).tooltip_text),
                }),
        )
        .padding([6, 9])
        .max_width(360),
        position,
    )
    .padding(0)
    .gap(6)
    .style(|theme| container::Style {
        background: Some(super::shell::palette(theme).tooltip_background.into()),
        border: Border {
            color: super::shell::palette(theme).tooltip_border,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
}
