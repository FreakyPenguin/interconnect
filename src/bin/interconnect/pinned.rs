//! A compact room-scoped disclosure, with bounded previews and permission-gated removal.
use super::{
    App, Message,
    icons::{Kind, icon},
    presentation, selectable, shell,
};
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
        let expanded = self.expanded_pins == Some(generation);
        let label = if pins.total == 1 {
            "1 pinned message".to_owned()
        } else {
            format!("{} pinned messages", pins.total)
        };
        let mut summary = row![
            icon(Kind::Pin),
            text(label).size(12),
            iced::widget::space().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if pins.error.is_some() {
            summary = summary.push(text("Some unavailable").size(11).color(shell::MUTED));
        } else if pins.loading {
            summary = summary.push(text("Loading…").size(11).color(shell::MUTED));
        }
        summary = summary.push(icon(if expanded {
            Kind::ChevronUp
        } else {
            Kind::ChevronDown
        }));
        let header = container(
            button(summary)
                .padding([6, 8])
                .style(shell::quiet_button)
                .on_press_maybe(
                    (!self.closing && !self.stopped).then_some(Message::TogglePins(generation)),
                ),
        )
        .id("pinned-toggle")
        .padding([2, 16]);
        if !expanded {
            return Some(column![header, rule::horizontal(1)].spacing(0).into());
        }
        let mut entries = column![].spacing(10);
        for message in pins.rows.iter() {
            let timestamp = super::tooltips::tooltip(
                text(presentation::time_label(message))
                    .size(11)
                    .color(shell::MUTED),
                presentation::time_detail(message),
                tooltip::Position::Bottom,
            );
            let icon: Element<'_, Message> =
                if let Some(unpin) = self.pinned_unpin_action(message, generation) {
                    super::tooltips::tooltip(
                        container(
                            button(icon(Kind::Unpin))
                                .padding(0)
                                .width(20)
                                .style(shell::icon_button)
                                .on_press(unpin),
                        )
                        .id(format!("pinned-unpin:{}", message.key)),
                        "Unpin message",
                        tooltip::Position::Bottom,
                    )
                    .into()
                } else {
                    container(icon(Kind::Pin)).width(20).into()
                };
            let body = row![
                icon,
                column![
                    row![shell::title(&message.display_name, 12), timestamp].spacing(10),
                    selectable::message_view_with_assets(message, self.inline_assets(), None),
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
                header,
                container(content).padding([8, 24]).width(Length::Fill),
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
    fn expanded_pins_render_rich_messages_and_activate_links() {
        use iced::advanced::{Layout, Shell, mouse};
        let mut app = demo_app();
        let generation = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::TogglePins(generation));
        let pins = app.model.pinned.as_mut().unwrap();
        let message = &mut std::sync::Arc::make_mut(&mut pins.rows)[0];
        message.body = "Read guide carefully. Secret".into();
        message.formatted = Some(std::sync::Arc::new(
            interconnect::message_document::Document::parse(
                "<p><a href='https://example.org/guide'><strong>Read guide</strong></a> <em>carefully.</em></p><p><span data-mx-spoiler>Secret</span></p>",
            ).unwrap(),
        ));
        #[derive(Default)]
        struct Probe(Option<iced::Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn custom(
                &mut self,
                _: Option<&widget::Id>,
                bounds: iced::Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(state) = state.downcast_ref::<selectable::State>()
                    && state.id == "direction"
                {
                    assert!(
                        state.rich.is_some(),
                        "pins use the formatted message renderer"
                    );
                    assert!(!state.body.contains("Secret"), "spoilers stay concealed");
                    self.0 = Some(bounds);
                }
            }
        }
        let mut view = app.pinned_view(generation).unwrap();
        let mut tree = widget::Tree::new(&view);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(400.0, 480.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        let point = probe.0.unwrap().position() + iced::Vector::new(8.0, 8.0);
        let mut messages = Vec::new();
        for event in [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        assert!(messages.iter().any(|message| matches!(message,
            Message::OpenMessageLink(url) if url == "https://example.org/guide")));
    }

    #[test]
    fn disclosure_starts_compact_and_is_scoped_to_the_active_room() {
        let mut app = demo_app();
        let generation = app.selection.room.as_ref().unwrap().0;
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let height = |app: &App| {
            let mut view = app.pinned_view(generation).unwrap();
            let mut tree = widget::Tree::new(&view);
            view.as_widget_mut()
                .layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(320.0, 480.0)),
                )
                .size()
                .height
        };
        assert!(height(&app) <= 40.0);
        let _ = app.update(Message::TogglePins(generation + 1));
        assert_eq!(app.expanded_pins, None);
        let _ = app.update(Message::TogglePins(generation));
        assert_eq!(app.expanded_pins, Some(generation));
        assert!(height(&app) > 40.0);
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.expanded_pins, Some(generation));
        let _ = app.settings_update(super::super::settings::Event::Open);
        let _ = app.update(Message::TogglePins(generation));
        assert_eq!(
            app.expanded_pins,
            Some(generation),
            "modal blocks hidden disclosure"
        );
        let _ = app.settings_update(super::super::settings::Event::Cancel);
        let _ = app.update(Message::TogglePins(generation));
        assert!(height(&app) <= 40.0);
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(Message::TogglePins(generation));
        let _ = app.update(Message::Room("studio".into()));
        assert_ne!(
            app.expanded_pins,
            app.selection.room.as_ref().map(|(current, _)| *current)
        );
    }

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
        let _ = app.update(Message::TogglePins(generation));
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
        let generation = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::TogglePins(generation));
        assert!((inspect(&app, &mut tree, false) - before).abs() < 1.0);
        let _ = app.update(Message::TogglePins(generation));
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
        let _ = app.update(Message::TogglePins(generation));
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
        assert!(node.size().height <= 220.0);
        assert!(probe.viewport.unwrap().height <= 144.0);
        assert!(probe.content.unwrap().height > probe.viewport.unwrap().height);
        assert!(node.size().width <= 320.0);
    }
}
