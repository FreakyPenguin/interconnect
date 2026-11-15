//! A bounded strip above the room timeline, with permission-gated removal.
use super::{App, Message, emoji, presentation, shell};
use iced::{
    Element, Length,
    widget::{button, column, container, row, rule, scrollable, text, tooltip},
};

impl App {
    pub(super) fn pinned_view(&self, generation: u64) -> Option<Element<'_, Message>> {
        let pins = self
            .model
            .pinned
            .as_ref()
            .filter(|pins| pins.generation == generation)?;
        if pins.total == 0 {
            return None;
        }
        let mut entries = column![].spacing(10);
        for message in pins.rows.iter() {
            let timestamp = tooltip(
                text(presentation::time_label(message))
                    .size(11)
                    .color(shell::MUTED),
                text(presentation::time_detail(message)).size(12),
                tooltip::Position::Bottom,
            );
            let icon: Element<'_, Message> =
                if let Some(unpin) = self.pinned_unpin_action(message, generation) {
                    tooltip(
                        container(
                            button(emoji::preview("📌".into()).size(14))
                                .padding(0)
                                .width(20)
                                .style(shell::icon_button)
                                .on_press(unpin),
                        )
                        .id(format!("pinned-unpin:{}", message.key)),
                        text("Unpin message").size(12),
                        tooltip::Position::Bottom,
                    )
                    .into()
                } else {
                    container(emoji::preview("📌".into()).size(14))
                        .width(20)
                        .into()
                };
            let body = row![
                icon,
                column![
                    row![shell::title(&message.display_name, 12), timestamp].spacing(10),
                    emoji::preview(message.body.clone()).size(13),
                ]
                .spacing(4)
                .width(Length::Fill),
            ]
            .spacing(8);
            entries = entries.push(self.pin_feedback(body.into(), message, false, generation));
        }
        if let Some(error) = &pins.error {
            entries = entries.push(text(error).size(12).color(shell::MUTED));
        } else if pins.loading {
            entries = entries.push(
                text("Loading pinned messages…")
                    .size(12)
                    .color(shell::MUTED),
            );
        } else if pins.rows.len() < pins.total {
            entries = entries.push(
                text("Some pinned messages are unavailable or exceed the display limit.")
                    .size(12)
                    .color(shell::MUTED),
            );
        }
        let content = container(
            scrollable(entries)
                .id("room-pins")
                .direction(shell::scrollbar())
                .style(shell::scroll_style),
        )
        .max_height(144);
        Some(
            column![
                container(content).padding([12, 24]).width(Length::Fill),
                rule::horizontal(1)
            ]
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DraftKey, tests::demo_app};
    use iced::advanced::{layout, widget};

    #[test]
    fn panel_icon_unpins_events_outside_history_and_stays_static_without_permission() {
        use iced::advanced::{Layout, Shell, mouse};
        #[derive(Default)]
        struct Probe(Option<iced::Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("pinned-unpin:direction")) {
                    self.0 = Some(bounds);
                }
            }
        }
        let mut app = demo_app();
        let generation = app.selection.room.as_ref().unwrap().0;
        // This pin is older than the loaded history; removal must use the pin snapshot.
        app.model.room.as_mut().unwrap().rows = std::sync::Arc::default();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(360.0, 200.0);
        let mut tree = widget::Tree::empty();
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .can_pin = false;
        {
            let mut view = app.pinned_view(generation).unwrap();
            tree.diff(view.as_widget());
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
            );
            let mut probe = Probe::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            assert!(probe.0.is_none(), "no unpin button without permission");
        }
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .can_pin = true;
        let mut view = app.pinned_view(generation).unwrap();
        tree.diff(view.as_widget());
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        let mut messages = Vec::new();
        for event in [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(probe.0.unwrap().center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        drop(view);
        let unpin = messages
            .into_iter()
            .find(|message| {
                matches!(
                    message,
                    Message::Pin(super::super::pin_action::Event::Set(_, false))
                )
            })
            .expect("the icon dispatches unpin");
        let _ = app.update(unpin.clone());
        assert_eq!(app.model.pinned.as_ref().unwrap().total, 1);
        assert_eq!(app.model.pinned.as_ref().unwrap().rows[0].key, "proposal");
        let _ = app.update(unpin);
        assert_eq!(app.model.pinned.as_ref().unwrap().total, 1);
    }

    #[test]
    fn pins_are_room_scoped_and_space_filters_keep_drafts_and_scroll() {
        let mut app = demo_app();
        let generation = app.selection.room.as_ref().unwrap().0;
        assert_eq!(app.model.pinned.as_ref().unwrap().rows.len(), 2);
        assert!(app.pinned_view(generation).is_some());
        assert!(app.pinned_view(generation + 1).is_none());
        let key = DraftKey {
            room: "studio".into(),
            root: None,
        };
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent draft");
        app.scroll_positions.insert(key.clone(), 80.0);
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert!(app.pinned_view(generation).is_some());
        let _ = app.update(Message::Thread("review".into()));
        assert!(
            app.pinned_view(generation).is_some(),
            "thread navigation keeps room pins"
        );
        let _ = app.update(Message::Room("general".into()));
        assert!(app.model.pinned.is_none());
        assert_eq!(app.drafts[&key].content.text(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 80.0);
    }

    #[test]
    fn live_pin_arrival_and_removal_preserve_the_timeline_reading_position() {
        #[derive(Default)]
        struct Probe(Option<f32>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                _: iced::Rectangle,
                _: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.0 = Some(translation.y);
                }
            }
        }
        fn inspect(app: &App, tree: &mut widget::Tree, set: bool) -> f32 {
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut view = app.view();
            tree.diff(view.as_widget());
            let size = iced::Size::new(1240.0, 820.0);
            let node =
                view.as_widget_mut()
                    .layout(tree, &renderer, &layout::Limits::new(size, size));
            if set {
                view.as_widget_mut().operate(
                    tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        widget::Id::new("room-timeline"),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(500.0),
                        },
                    ),
                );
            }
            let mut probe = Probe::default();
            view.as_widget_mut()
                .operate(tree, layout::Layout::new(&node), &renderer, &mut probe);
            probe.0.unwrap()
        }
        let mut app = demo_app();
        let mut pins = app.model.pinned.clone().unwrap();
        let _ = app.update(Message::Room("busy".into()));
        pins.generation = app.selection.room.as_ref().unwrap().0;
        let mut tree = widget::Tree::empty();
        let before = inspect(&app, &mut tree, true);
        assert!(before > 0.0);
        app.model.pinned = Some(pins);
        assert!((inspect(&app, &mut tree, false) - before).abs() < 1.0);
        app.model.pinned = None;
        assert!((inspect(&app, &mut tree, false) - before).abs() < 1.0);
    }

    #[test]
    fn long_pins_scroll_within_a_bounded_strip() {
        #[derive(Default)]
        struct Probe {
            viewport: Option<iced::Rectangle>,
            content: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                _: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-pins")) {
                    self.viewport = Some(bounds);
                    self.content = Some(content);
                }
            }
        }
        let mut app = demo_app();
        let pins = app.model.pinned.as_mut().unwrap();
        std::sync::Arc::make_mut(&mut pins.rows)[0].body =
            "A long pinned note with many lines.\n".repeat(30);
        let generation = app.selection.room.as_ref().unwrap().0;
        let mut view = app.pinned_view(generation).unwrap();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(320.0, 480.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
        assert!(node.size().height <= 200.0);
        assert!(probe.viewport.unwrap().height <= 144.0);
        assert!(probe.content.unwrap().height > probe.viewport.unwrap().height);
        assert!(node.size().width <= 320.0);
    }
}
