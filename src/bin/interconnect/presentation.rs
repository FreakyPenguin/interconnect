use super::{Message, shell, single_line, tooltips};
use chrono::{DateTime, Local, NaiveDate};
use iced::{
    Element, Font, Length, font,
    widget::{button, container, responsive, row, text, tooltip},
};
use interconnect::connected::{MessageRow, RowKind};

/// Keep readable message lines on wide windows, without moving the scrollbar.
pub(super) const MESSAGE_WIDTH: f32 = 840.0;

pub(super) fn heading(message: &MessageRow, action: Option<Message>) -> Element<'_, Message> {
    responsive(move |bounds| {
        let label = time_label(message);
        let reserved = if label.is_empty() { 0.0 } else { 44.0 };
        let name = sender_name(
            container(
                single_line::label(&message.display_name, 13)
                    .shrink()
                    .font(Font {
                        weight: font::Weight::Semibold,
                        ..Font::with_name("Inter")
                    }),
            )
            .max_width((bounds.width - reserved).max(0.0)),
            &message.sender,
            action.clone(),
        );
        let mut heading = row![name].spacing(8).align_y(iced::Alignment::Center);
        if !label.is_empty() {
            heading = heading.push(tooltips::tooltip(
                text(label).size(11).style(shell::muted_text),
                time_detail(message),
                tooltip::Position::Top,
            ));
        }
        heading.into()
    })
    .width(Length::Fill)
    .height(18)
    .into()
}

pub(super) fn sender_name<'a>(
    content: impl Into<Element<'a, Message>>,
    user: &'a str,
    action: Option<Message>,
) -> Element<'a, Message> {
    let content = content.into();
    let hint = if action.is_some() {
        format!("{user} · Open direct chat")
    } else {
        user.to_owned()
    };
    let name: Element<'a, Message> = if let Some(action) = action {
        button(content)
            .padding(0)
            .style(|theme, status| iced::widget::button::Style {
                text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    shell::palette(theme).accent
                } else {
                    shell::palette(theme).ink
                },
                ..Default::default()
            })
            .on_press(action)
            .into()
    } else {
        content
    };
    tooltips::tooltip(name, hint, tooltip::Position::Top).into()
}

pub fn local_time(message: &MessageRow) -> Option<DateTime<Local>> {
    DateTime::from_timestamp_millis(message.timestamp_ms?).map(|time| time.with_timezone(&Local))
}
pub fn date(message: &MessageRow) -> Option<NaiveDate> {
    local_time(message).map(|time| time.date_naive())
}
pub fn grouped(previous: Option<&MessageRow>, current: &MessageRow) -> bool {
    previous.is_some_and(|previous| {
        previous.kind == RowKind::Message
            && current.kind == RowKind::Message
            && previous.sender == current.sender
            && previous.display_name == current.display_name
            && date(previous).is_some()
            && date(previous) == date(current)
            && previous
                .timestamp_ms
                .zip(current.timestamp_ms)
                .and_then(|(a, b)| b.checked_sub(a))
                .is_some_and(|gap| (0..=5 * 60_000).contains(&gap))
    })
}
pub fn time_label(message: &MessageRow) -> String {
    local_time(message)
        .map(|time| time.format("%H:%M").to_string())
        .unwrap_or_default()
}
pub fn time_detail(message: &MessageRow) -> String {
    local_time(message)
        .map(|time| time.format("%a, %d %b %Y · %H:%M:%S %:z").to_string())
        .unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn author_and_time_stay_adjacent_and_fit_with_long_names() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe(Vec<(String, iced::Rectangle)>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, value: &str) {
                self.0.push((value.to_owned(), bounds));
            }
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for name in [
            "Maya Chen".to_owned(),
            "A long display name 👩🏽‍💻 with many details ".repeat(8),
        ] {
            let mut message = message("@maya:example.org", Some(120_000));
            message.display_name = name;
            for width in [120.0, 300.0, 900.0] {
                let mut view = heading(
                    &message,
                    Some(Message::NewChat(super::super::new_chat::Event::Ignore)),
                );
                let mut tree = widget::Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 40.0)),
                );
                let mut probe = Probe::default();
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                let author = probe
                    .0
                    .iter()
                    .find(|(value, _)| value == &message.display_name)
                    .unwrap()
                    .1;
                let time = probe
                    .0
                    .iter()
                    .find(|(value, _)| value == &time_label(&message))
                    .unwrap()
                    .1;
                assert!(
                    author.width > 20.0,
                    "author must remain visible: {author:?}"
                );
                assert!((time.x - author.x - author.width - 8.0).abs() < 0.1);
                assert!(time.x + time.width <= width);
                assert_eq!(node.size().height, 18.0);
            }
        }
    }
    fn message(sender: &str, timestamp_ms: Option<i64>) -> MessageRow {
        MessageRow {
            key: "fixture".into(),
            kind: RowKind::Message,
            event_id: None,
            sender: sender.into(),
            display_name: "Same name".into(),
            avatar_url: None,
            image: None,
            download: None,
            reactions: Vec::new(),
            can_react: true,
            redactable: true,
            edit_source: None,
            edited: false,
            reply: None,
            read_by: Vec::new(),
            timestamp_ms,
            thread_replies: 0,
            thread_root: None,
            is_own: false,
            is_live: false,
            body: String::new(),
            formatted: None,
            send_state: String::new(),
            retry: None,
            cancel_send: None,
        }
    }
    #[test]
    fn grouping_requires_same_identity_and_nearby_ordered_known_times() {
        let now = Local::now()
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp_millis();
        let a = message("@a:example.org", Some(now));
        assert!(grouped(Some(&a), &message(&a.sender, Some(now + 60_000))));
        assert!(!grouped(Some(&a), &message("@b:example.org", Some(now))));
        assert!(!grouped(Some(&a), &message(&a.sender, Some(now + 300_001))));
        assert!(!grouped(Some(&a), &message(&a.sender, Some(now - 1))));
        assert!(!grouped(Some(&a), &message(&a.sender, None)));
        assert!(!grouped(None, &a));
        let mut activity = a.clone();
        activity.kind = RowKind::Activity;
        assert!(!grouped(Some(&a), &activity));
        assert!(!grouped(Some(&activity), &a));
        assert!(!grouped(Some(&activity), &activity));
    }
}
