//! Shared presentation for control hints and contextual details.
use iced::{
    Border, Color, Element, Shadow, Vector,
    widget::{container, text, tooltip as iced_tooltip},
};

pub(super) fn tooltip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl text::IntoFragment<'a>,
    position: iced_tooltip::Position,
) -> iced_tooltip::Tooltip<'a, Message> {
    iced_tooltip(
        content,
        container(text(label).size(11).color(Color::from_rgb8(247, 248, 250)))
            .padding([6, 9])
            .max_width(360),
        position,
    )
    .padding(0)
    .gap(6)
    .style(|_| container::Style {
        background: Some(Color::from_rgb8(43, 48, 57).into()),
        border: Border {
            color: Color::WHITE.scale_alpha(0.12),
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.18),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        ..Default::default()
    })
}
