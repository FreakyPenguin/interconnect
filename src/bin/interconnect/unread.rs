//! A visit's read boundary stays fixed while receipts and timeline rows change.
use super::{Message, shell, thread_activity};
use iced::{
    Element, Length,
    widget::{container, row, rule, text},
};
use interconnect::connected::Pane;
use std::collections::HashSet;

pub fn divider_index(pane: &Pane, rows: &[thread_activity::Row<'_>]) -> Option<usize> {
    let boundary = pane.read_boundary.as_deref()?;
    let read = pane
        .rows
        .iter()
        .position(|row| row.event_id.as_deref() == Some(boundary))?;
    // Match the projected row: several replies can collapse into one thread entry.
    // A local echo or our own message alone is not unread incoming content.
    let incoming = || {
        pane.rows[read + 1..]
            .iter()
            .filter(|row| !row.is_own && row.event_id.is_some())
    };
    let unread: HashSet<_> = incoming().map(|row| row.key.as_str()).collect();
    let threads: HashSet<_> = incoming()
        .filter_map(|row| row.thread_root.as_deref())
        .collect();
    rows.iter().position(|row| match row {
        thread_activity::Row::Event(message) => unread.contains(message.key.as_str()),
        thread_activity::Row::Thread { root, .. } => threads.contains(root),
    })
}

pub fn divider<'a>() -> Element<'a, Message> {
    let line = || {
        rule::horizontal(1).style(|theme| rule::Style {
            color: shell::palette(theme).accent,
            ..rule::default(theme)
        })
    };
    container(
        row![
            line(),
            text("New messages").size(11).style(|theme| text::Style {
                color: Some(shell::palette(theme).accent),
            }),
            line()
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding([12, 0])
    .id("unread-divider")
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, tests::demo_app};

    #[test]
    fn boundary_survives_receipts_and_waits_for_loaded_history() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("release".into()));
        let pane = app.model.room.as_mut().unwrap();
        let index = |pane: &Pane| divider_index(pane, &thread_activity::project(&pane.rows, None));
        assert_eq!(index(pane), Some(1));
        pane.read_boundary = None;
        assert_eq!(index(pane), None);
        pane.read_boundary = Some("outside-loaded-history".into());
        assert_eq!(index(pane), None);
        pane.read_boundary = Some("release2".into());
        assert_eq!(index(pane), None);
        pane.read_boundary = Some("release".into());
        let rows = std::sync::Arc::make_mut(&mut pane.rows);
        rows[1].is_own = true;
        assert_eq!(index(pane), None);
        std::sync::Arc::make_mut(&mut pane.rows)[1].is_own = false;
        let generation = pane.generation;
        app.pending_scroll[0] = None;
        let _ = app.update(Message::WindowFocused(true));
        app.read_visible(
            false,
            generation,
            &["release2".into()],
            super::super::timeline::Metrics {
                height: 600.0,
                content_height: 600.0,
                translation: 0.0,
            },
        );
        assert!(app.selection.read[0].is_some());
        assert_eq!(index(app.model.room.as_ref().unwrap()), Some(1));
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(index(app.model.room.as_ref().unwrap()), Some(1));
    }

    #[test]
    fn collapsed_thread_entries_respect_hidden_reply_boundaries() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("release".into()));
        let pane = app.model.room.as_mut().unwrap();
        let root = pane.rows[0].clone();
        let mut first = pane.rows[1].clone();
        first.thread_root = root.event_id.clone();
        let mut last = first.clone();
        last.key = "latest-reply".into();
        last.event_id = Some(last.key.clone());
        pane.read_boundary = first.event_id.clone();
        pane.rows = std::sync::Arc::new(vec![root, first, last]);
        let rows = thread_activity::project(&pane.rows, None);
        assert_eq!(rows.len(), 2);
        assert_eq!(divider_index(pane, &rows), Some(1));
        assert!(matches!(rows[1], thread_activity::Row::Thread { .. }));
        pane.read_boundary = Some("latest-reply".into());
        assert_eq!(divider_index(pane, &rows), None);
        // A newer outgoing reply must not hide an earlier unread incoming reply.
        pane.read_boundary = Some("release".into());
        std::sync::Arc::make_mut(&mut pane.rows)[2].is_own = true;
        let rows = thread_activity::project(&pane.rows, None);
        assert_eq!(divider_index(pane, &rows), Some(1));
    }

    #[test]
    fn pagination_reveals_boundary_without_marking_local_echoes_unread() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("release".into()));
        let pane = app.model.room.as_mut().unwrap();
        let read = std::sync::Arc::make_mut(&mut pane.rows).remove(0);
        let index = |pane: &Pane| divider_index(pane, &thread_activity::project(&pane.rows, None));
        assert_eq!(index(pane), None);
        std::sync::Arc::make_mut(&mut pane.rows).insert(0, read);
        assert_eq!(index(pane), Some(1));
        std::sync::Arc::make_mut(&mut pane.rows)[1].event_id = None;
        assert_eq!(index(pane), None);
    }

    #[test]
    fn divider_is_inside_the_first_unread_row_only_in_room_panes() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe {
            divider: Option<iced::Rectangle>,
            unread_row: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("unread-divider")) {
                    assert!(self.divider.is_none(), "only one divider per room");
                    self.divider = Some(bounds);
                }
                if id == Some(&super::super::timeline::row_id(1)) {
                    self.unread_row = Some(bounds);
                }
            }
        }
        let mut app = demo_app();
        let _ = app.update(Message::Room("release".into()));
        let _ = app.update(Message::Thread("release".into()));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for width in [320.0, 640.0, 1000.0] {
            for thread in [false, true] {
                let size = iced::Size::new(width, 480.0);
                let mut view = app.pane_sized(thread, size);
                let mut tree = widget::Tree::new(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, size),
                );
                let mut probe = Probe::default();
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                if thread {
                    assert!(probe.divider.is_none());
                } else {
                    let divider = probe.divider.unwrap();
                    let row = probe.unread_row.unwrap();
                    assert!(divider.width > 100.0 && divider.height > 0.0);
                    assert!(divider.y >= row.y && divider.y + divider.height <= row.y + row.height);
                }
            }
        }
    }
}
