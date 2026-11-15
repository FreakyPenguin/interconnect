//! Toggleable reaction chips, shared by room messages and thread originals/replies.
use super::{Message, emoji, shell};
use iced::{
    Border, Color, Element,
    widget::{button, container, row},
};
use interconnect::connected::Reaction;

pub fn view<'a>(
    reactions: &'a [Reaction],
    on_toggle: impl Fn(&Reaction) -> Option<Message>,
) -> Option<Element<'a, Message>> {
    if reactions.is_empty() {
        return None;
    }
    let badges = reactions.iter().map(|reaction| {
        let is_own = reaction.is_own;
        container(
            button(emoji::preview(format!("{}  {}", reaction.key, reaction.count)).size(12))
                .padding([3, 8])
                .on_press_maybe(on_toggle(reaction))
                .style(move |_, status| button::Style {
                    background: Some(
                        if is_own {
                            Color::from_rgb8(226, 234, 247)
                        } else if matches!(
                            status,
                            button::Status::Hovered | button::Status::Pressed
                        ) {
                            Color::from_rgb8(237, 240, 244)
                        } else {
                            Color::from_rgb8(246, 247, 249)
                        }
                        .into(),
                    ),
                    text_color: if is_own { shell::ACCENT } else { shell::INK },
                    border: Border {
                        color: if is_own {
                            shell::ACCENT.scale_alpha(0.45)
                        } else {
                            Color::from_rgb8(228, 232, 237)
                        },
                        width: 1.0,
                        radius: 7.0.into(),
                    },
                    ..Default::default()
                }),
        )
        .max_width(180)
        .id(format!("reaction-chip:{}", reaction.key))
        .into()
    });
    Some(
        container(row(badges).spacing(5).wrap())
            .padding([4, 0])
            .id("message-reactions")
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{
        Event, Rectangle, Size,
        advanced::{Layout, Shell, layout, mouse, widget::Tree},
    };

    #[test]
    fn chips_wrap_unicode_and_text_keys_and_stay_disabled_without_actions_or_timers() {
        let reactions: Vec<_> = ["👍🏽", "👩🏽‍💻", "❤️", "approved", "a-long-custom-annotation-key"]
            .into_iter()
            .enumerate()
            .map(|(index, key)| Reaction {
                key: key.into(),
                count: index + 1,
                is_own: index % 2 == 0,
            })
            .collect();
        assert!(view(&[], |_| None).is_none());
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut heights = Vec::new();
        for width in [700.0, 120.0, 64.0] {
            let mut badges = view(&reactions, |_| None).unwrap();
            let mut tree = Tree::new(&badges);
            let size = Size::new(width, 1000.0);
            let node = badges.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            fn fits(layout: Layout<'_>, width: f32) {
                let bounds = layout.bounds();
                assert!(
                    bounds.x >= 0.0 && bounds.x + bounds.width <= width + 0.1,
                    "{bounds:?} exceeds {width}"
                );
                for child in layout.children() {
                    fits(child, width);
                }
            }
            fits(Layout::new(&node), width);
            heights.push(node.size().height);
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            let mut clipboard = iced::advanced::clipboard::Null;
            for event in [
                Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ] {
                badges.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    mouse::Cursor::Available(iced::Point::new(10.0, 10.0)),
                    &renderer,
                    &mut clipboard,
                    &mut shell,
                    &Rectangle::with_size(size),
                );
            }
            assert!(shell.is_empty());
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
        }
        assert!(heights[1] > heights[0]);
        assert!(heights[2] > heights[1]);
    }
}
