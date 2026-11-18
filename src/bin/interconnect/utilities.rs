//! Routine account utilities live in the avatar menu; only actionable status
//! interrupts the conversation. No status polling or animation is needed.
use super::{App, Message, shell, verification};
use iced::{
    Element, Length,
    widget::{button, column, container, row, text},
};

impl App {
    pub(super) fn account_label(&self) -> &str {
        if self.demo.is_some() {
            "Jamie Davis"
        } else {
            &self.model.account_name
        }
    }
    pub(super) fn utilities_need_attention(&self) -> bool {
        self.draft_persistence.error.is_some()
            || self.model.verification.incoming.is_some()
            || self.settings.error.is_some()
            || self.model.notification_error.is_some()
            || self.model.media_error.is_some()
            || (self.demo.is_none()
                && (self.model.status != "Connected" || self.settings.graphics_restart_required()))
    }
    pub(super) fn account_menu_heading(&self) -> Element<'_, Message> {
        let mut content = column![
            shell::title(self.account_label(), 13),
            text(&self.model.status).size(11).style(shell::muted_text),
        ]
        .spacing(5);
        if self.demo.is_none() && self.settings.graphics_restart_required() {
            content = content.push(
                text("Graphics changes require a restart")
                    .size(11)
                    .style(shell::muted_text),
            );
        }
        container(content)
            .padding([8, 10])
            .width(Length::Fill)
            .into()
    }
    pub(super) fn utility_banner(&self) -> Option<Element<'_, Message>> {
        let mut items = column![].spacing(3).width(Length::Fill);
        let connection = self.demo.is_none() && self.model.status != "Connected";
        if connection || self.stopped {
            let mut status = row![super::tooltips::tooltip(
                super::single_line::label(&self.model.status, 11).style(shell::muted_text),
                &self.model.status,
                iced::widget::tooltip::Position::Top,
            )]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            if self.stopped {
                status = status.push(
                    button(text("Exit").size(11))
                        .style(shell::quiet_button)
                        .on_press(Message::Exit),
                );
            }
            items = items.push(status);
        }
        let verification = self.model.verification.incoming.is_some()
            && (self.demo.is_some() || self.model.verification.control.is_some());
        if verification {
            items = items.push(
                button(text("Verification requested · Review").size(11))
                    .style(shell::quiet_button)
                    .on_press(Message::Verification(verification::Event::Open)),
            );
        }
        if let Some(error) = &self.draft_persistence.error {
            items = items.push(
                row![
                    super::tooltips::tooltip(
                        text("Drafts need attention")
                            .size(11)
                            .style(shell::muted_text),
                        error,
                        iced::widget::tooltip::Position::Top
                    ),
                    button(text("Retry draft save").size(11))
                        .style(shell::quiet_button)
                        .on_press_maybe(self.draft_persistence.can_retry().then_some(
                            Message::DraftPersistence(super::draft_persistence::Event::Retry)
                        ))
                ]
                .spacing(8),
            );
        }
        let settings_error =
            self.settings.error.is_some() || self.model.notification_error.is_some();
        if settings_error {
            items = items.push(
                button(text("Settings need attention").size(11))
                    .style(shell::quiet_button)
                    .on_press(Message::Settings(super::settings::Event::Open)),
            );
        }
        if let Some(error) = &self.model.media_error {
            items = items.push(super::tooltips::tooltip(
                text("Media unavailable").size(11).style(shell::muted_text),
                error,
                iced::widget::tooltip::Position::Top,
            ));
        }
        if self.draft_persistence.error.is_none()
            && !connection
            && !self.stopped
            && !verification
            && !settings_error
            && self.model.media_error.is_none()
        {
            return None;
        }
        Some(
            column![
                iced::widget::rule::horizontal(1),
                container(items)
                    .id("utility-banner")
                    .padding([5, 14])
                    .width(Length::Fill)
                    .style(|theme| shell::surface(theme, shell::palette(theme).sidebar))
            ]
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_menu::{Event, Target};
    use iced::{
        Point, Rectangle, Size,
        advanced::{layout, widget},
    };

    fn open(app: &mut App) {
        let _ = app.update(Message::ContextMenu(Event::Open(
            Target::Account,
            Point::new(60.0, 12.0),
        )));
        assert!(app.context_menu.is_some());
    }
    fn choose(app: &mut App, label: &str) {
        let _ = app.update(Message::ContextMenu(Event::Choose(
            Target::Account,
            label.into(),
        )));
    }

    #[test]
    fn account_utilities_dispatch_existing_actions_and_preserve_conversation() {
        let mut app = crate::tests::demo_app();
        let room = app.draft_key(false).unwrap();
        app.drafts.get_mut(&room).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(room.clone(), 80.0);
        let selection = app.selection.clone();
        open(&mut app);
        choose(&mut app, "Settings…");
        assert!(app.settings.draft.is_some());
        assert!(app.context_menu.is_none());
        // An obsolete account-menu action cannot open a second modal.
        choose(&mut app, "Verify device…");
        assert!(!app.verification.open);
        let _ = app.update(Message::Settings(crate::settings::Event::Cancel));
        app.shell.navigation = false;
        open(&mut app);
        choose(&mut app, "Find room…");
        assert!(app.shell.navigation);
        assert!(app.context_menu.is_none());
        open(&mut app);
        let _ = app.update(Message::Escape);
        assert!(app.context_menu.is_none());
        open(&mut app);
        choose(&mut app, "Verify device…");
        assert!(app.verification.open);
        assert!(app.context_menu.is_none());
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.drafts[&room].content.text().trim_end(), "unsent");
        assert_eq!(app.scroll_positions[&room], 80.0);
    }

    #[test]
    fn ordinary_status_is_quiet_but_requests_errors_and_shutdown_remain_visible() {
        let mut app = crate::tests::demo_app();
        assert!(app.utility_banner().is_none());
        assert!(!app.utilities_need_attention());
        app.model.verification.incoming = Some(interconnect::verification::Incoming {
            flow: "sample".into(),
            device: "sample".into(),
        });
        assert!(app.utility_banner().is_some());
        assert!(app.utilities_need_attention());
        app.model.verification.incoming = None;
        for kind in 0..3 {
            let error = match kind {
                0 => &mut app.settings.error,
                1 => &mut app.model.notification_error,
                _ => &mut app.model.media_error,
            };
            *error = Some("fixture error".into());
            assert!(app.utility_banner().is_some());
            assert!(app.utilities_need_attention());
            app.settings.error = None;
            app.model.notification_error = None;
            app.model.media_error = None;
        }
        app.demo = None;
        app.model.status = "Connected".into();
        assert!(app.utility_banner().is_none());
        assert!(!app.utilities_need_attention());
        app.model.status = "Offline — reconnecting automatically".into();
        assert!(app.utility_banner().is_some());
        app.model.status = "Connected".into();
        assert!(app.utility_banner().is_none());
        app.stopped = true;
        assert!(app.utility_banner().is_some());
    }

    #[test]
    fn reconnect_status_text_keeps_banner_and_conversation_bounds_stable() {
        #[derive(Default)]
        struct Probe {
            banner: Option<Rectangle>,
            timeline: Option<Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                if id == Some(&widget::Id::new("utility-banner")) {
                    self.banner = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: Rectangle,
                _: Rectangle,
                _: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.timeline = Some(bounds);
                }
            }
        }
        let mut app = crate::tests::demo_app();
        app.demo = None;
        app.model.authentication = None;
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = widget::Tree::empty();
        // Long translated/error text must not make the whole conversation resize.
        for width in [640.0, 1280.0] {
            let size = Size::new(width, 480.0);
            let mut previous = None;
            for status in [
                "Connecting… Messages and drafts are retained.",
                "Offline — reconnecting automatically. You can still compose and queue messages.",
                "Synchronization interrupted. Session retained.",
                &"A long connection error. ".repeat(60),
                "Connecting… Messages and drafts are retained.",
            ] {
                app.model.status = status.into();
                let mut view = app.view();
                tree.diff(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                let mut probe = Probe::default();
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                let bounds = (probe.banner.unwrap(), probe.timeline.unwrap());
                if let Some(previous) = previous {
                    assert_eq!(bounds, previous);
                }
                previous = Some(bounds);
            }
        }
    }

    #[test]
    fn status_arrival_and_recovery_preserve_the_live_scroll_widget() {
        #[derive(Default)]
        struct Read(Option<(f32, f32)>);
        impl widget::Operation for Read {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: Rectangle,
                content: Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.0 = Some((
                        translation.y,
                        (content.height - bounds.height).max(0.0) - translation.y,
                    ));
                }
            }
        }
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Room("busy".into()));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(640.0, 480.0);
        let mut tree = widget::Tree::empty();
        let mut reading_position = None;
        for step in 0..3 {
            app.model.notification_error = (step == 1).then(|| "fixture error".into());
            let mut view = app.view();
            tree.diff(view.as_widget());
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            if step == 0 {
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        widget::Id::new("room-timeline"),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(120.0),
                        },
                    ),
                );
            }
            let mut read = Read::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut read,
            );
            let (position, from_bottom) = read.0.unwrap();
            if step == 0 {
                assert!((from_bottom - 120.0).abs() < 1.0);
                reading_position = Some(position);
            } else {
                assert!(
                    (position - reading_position.unwrap()).abs() < 1.0,
                    "the visible reading position must survive the banner height change"
                );
            }
        }
    }
}
