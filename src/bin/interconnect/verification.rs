//! SAS presentation only. Confirmation carries the displayed flow and numbers.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, mouse_area, opaque, row, scrollable, text},
};
use interconnect::verification::{Action, Comparison, Device, Phase, Snapshot};
use matrix_sdk::encryption::VerificationState;

#[derive(Default)]
pub struct State {
    pub open: bool,
    error: Option<String>,
    confirming: Option<(u64, (u16, u16, u16))>,
    demo: Snapshot,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Action(u64, Action),
    Ignore,
}
fn message(event: Event) -> Message {
    Message::Verification(event)
}
pub fn trust_label(trust: VerificationState) -> &'static str {
    match trust {
        VerificationState::Verified => "Verified",
        VerificationState::Unverified => "Unverified",
        VerificationState::Unknown => "Unknown",
    }
}
impl App {
    fn verification_snapshot(&self) -> &Snapshot {
        if self.demo.is_some() {
            &self.verification.demo
        } else {
            &self.model.verification
        }
    }
    pub fn verification_update(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open
                if !self.closing
                    && !self.stopped
                    && self.settings.draft.is_none()
                    && self.image_view.is_none()
                    && (self.demo.is_some() || self.model.verification.control.is_some()) =>
            {
                self.reaction_picker = None;
                self.composer_picker = None;
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.verification.open = true;
                self.verification.error = None;
                if self.demo.is_some() {
                    self.verification.demo = Snapshot {
                        user: "@jamie:example.org".into(),
                        device: "DEMO".into(),
                        trust: VerificationState::Unverified,
                        phase: Phase::Devices(vec![Device {
                            id: "OTHER-DEMO".into(),
                            name: "Example phone".into(),
                            verified: false,
                        }]),
                        revision: self.verification.demo.revision + 1,
                        ..Snapshot::default()
                    };
                } else if matches!(
                    self.model.verification.phase,
                    Phase::Idle | Phase::Cancelled | Phase::Complete | Phase::Failed(_)
                ) {
                    self.verification.error = self
                        .model
                        .verification
                        .control
                        .as_ref()
                        .unwrap()
                        .send(self.model.verification.revision, Action::Refresh)
                        .err();
                }
            }
            Event::Close => {
                if self.demo.is_none()
                    && let Some(control) = &self.model.verification.control
                {
                    self.verification.error = control
                        .send(self.model.verification.revision, Action::Cancel)
                        .err();
                }
                self.verification.open = false;
                self.verification.confirming = None;
            }
            Event::Action(revision, action) if self.verification.open && !self.closing => {
                let snapshot = self.verification_snapshot();
                if snapshot.revision != revision {
                    return Task::none();
                }
                if let Action::Confirm(numbers) = &action {
                    if !matches!(&snapshot.phase, Phase::Compare { comparison, .. } if comparison.decimals == *numbers)
                        || self.verification.confirming == Some((revision, *numbers))
                    {
                        return Task::none();
                    }
                    self.verification.confirming = Some((revision, *numbers));
                }
                self.verification.error = None;
                if self.demo.is_some() {
                    let snapshot = &mut self.verification.demo;
                    snapshot.phase = match action {
                        Action::Start(device) => {
                            snapshot.revision += 1;
                            Phase::Compare {
                                device,
                                comparison: Comparison {
                                    decimals: (1234, 5678, 9012),
                                    emojis: [
                                        ("🐶", "Dog"),
                                        ("🌳", "Tree"),
                                        ("🚀", "Rocket"),
                                        ("🎁", "Gift"),
                                        ("🎸", "Guitar"),
                                        ("🍎", "Apple"),
                                        ("🔑", "Key"),
                                    ]
                                    .into_iter()
                                    .map(|(symbol, name)| (symbol.into(), name.into()))
                                    .collect(),
                                },
                            }
                        }
                        Action::Confirm(_) => Phase::Complete,
                        Action::Mismatch => Phase::Failed(
                            "The comparison did not match. No keys were confirmed.".into(),
                        ),
                        Action::Cancel => Phase::Cancelled,
                        _ => snapshot.phase.clone(),
                    };
                } else if let Some(control) = &self.model.verification.control {
                    self.verification.error = control.send(revision, action).err();
                    if self.verification.error.is_some() {
                        self.verification.confirming = None;
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    pub fn verification_view(&self) -> Element<'_, Message> {
        iced::widget::responsive(move |size| self.verification_card(size)).into()
    }
    fn verification_card(&self, size: iced::Size) -> Element<'_, Message> {
        let snapshot = self.verification_snapshot();
        let action = |action| message(Event::Action(snapshot.revision, action));
        let mut content = column![
            text(snapshot.user.clone()).size(13),
            text(format!(
                "This device: {} · Trust: {}",
                snapshot.device,
                trust_label(snapshot.trust)
            ))
            .size(12)
            .style(shell::muted_text),
        ]
        .spacing(16);
        if self.demo.is_some() {
            content = content.push(
                text("Design preview · sample comparison only. No device or keys are verified.")
                    .size(12)
                    .style(shell::accent_text),
            );
        }
        if matches!(
            snapshot.phase,
            Phase::Idle | Phase::Loading | Phase::Devices(_)
        ) {
            content = content.push(
                text("Verify this device by comparing emojis or numbers with another Matrix app where you're already signed in. You can also verify later from the account menu.")
                    .size(13),
            );
        }
        if let Some(incoming) = &snapshot.incoming {
            content = content.push(text(format!(
                "Your device {} is requesting verification.",
                incoming.device
            )));
        }
        match &snapshot.phase {
            Phase::Idle | Phase::Loading => content = content.push(text("Loading your devices…")),
            Phase::Devices(devices) => {
                content = content.push(text("Open your other Matrix app, keep it open, and choose that device below."));
                if devices.is_empty() { content = content.push(text("No other devices found. Sign in on another Matrix client, then refresh.")); }
                for device in devices {
                    content = content.push(button(column![
                        text(device.name.clone()).size(15),
                        text(format!("{} · {}", device.id, if device.verified { "Verified" } else { "Unverified" })).size(12),
                    ].spacing(4)).width(Length::Fill).padding(12).style(shell::quiet_button)
                        .on_press(action(Action::Start(device.id.clone()))));
                }
            }
            Phase::Waiting { device } => content = content.push(text(format!("Waiting for {device}. Accept the request on your other client.")))
                .push(text("Verification expires after three minutes. You can cancel at any time.").size(12).style(shell::muted_text)),
            Phase::Compare { device, comparison } => {
                content = content.push(shell::title("Compare with your other device", 19))
                    .push(text(format!("Other device: {device}" )).size(13))
                    .push(text("Check all seven emojis, or all three numbers, on both clients."));
                if comparison.emojis.len() == 7 {
                    let mut emojis = row![].spacing(8);
                    for (symbol, description) in &comparison.emojis {
                        emojis = emojis.push(column![
                            text(symbol.clone()).size(28).font(super::emoji::font().unwrap_or_default()),
                            text(description.clone()).size(10),
                        ].spacing(4).width(Length::Fill).align_x(iced::Alignment::Center));
                    }
                    content = content.push(emojis);
                }
                let (a, b, c) = comparison.decimals;
                content = content.push(container(text(format!("{a}    {b}    {c}")).size(26)).center_x(Length::Fill).padding(12));
            }
            Phase::Confirmed { device } => content = content.push(text(format!("Confirmed here. Waiting for {device} to confirm…"))),
            Phase::Complete => content = content.push(shell::title(if self.demo.is_some() { "Demo comparison complete" } else { "Comparison complete on both devices" }, 19))
                .push(text("Device trust above is reported by Matrix. Historical message keys may still need to be shared by your trusted client.").size(13)),
            Phase::Cancelled => content = content.push(text("Verification cancelled. No automatic confirmation was sent.")),
            Phase::Failed(error) => content = content.push(text(error.clone()).size(13)),
        }
        if let Some(error) = &self.verification.error {
            content = content.push(text(error).size(12));
        }
        let mut buttons = row![].spacing(8).align_y(iced::Alignment::Center);
        if !snapshot.phase.active() && self.demo.is_none() && snapshot.control.is_some() {
            buttons = buttons.push(
                button(text("Refresh devices").size(13))
                    .on_press(action(Action::Refresh))
                    .style(shell::quiet_button),
            );
        }
        buttons = buttons
            .push(
                button(
                    text(if snapshot.phase.active() {
                        "Cancel verification"
                    } else if snapshot.trust != VerificationState::Verified
                        && snapshot.phase != Phase::Complete
                    {
                        "Verify later"
                    } else {
                        "Close"
                    })
                    .size(13),
                )
                .on_press(message(Event::Close))
                .padding([7, 10])
                .style(shell::quiet_button),
            )
            .push(iced::widget::space().width(Length::Fill));
        if let Phase::Compare { comparison, .. } = &snapshot.phase {
            buttons = buttons
                .push(
                    button(text("They don't match").size(13))
                        .on_press(action(Action::Mismatch))
                        .padding([7, 10])
                        .style(shell::quiet_button),
                )
                .push(
                    button(text("They match").size(13))
                        .style(shell::primary_button)
                        .on_press_maybe(
                            (self.verification.confirming
                                != Some((snapshot.revision, comparison.decimals)))
                            .then(|| action(Action::Confirm(comparison.decimals))),
                        )
                        .padding([7, 10]),
                );
        } else if let Some(incoming) = &snapshot.incoming
            && !snapshot.phase.active()
        {
            buttons = buttons.push(
                button(text("Accept request").size(13))
                    .style(shell::primary_button)
                    .on_press(action(Action::Accept(incoming.flow.clone())))
                    .padding([7, 10]),
            );
        }
        let header = container(shell::title("Verify this device", 20))
            .padding([18, 24])
            .width(Length::Fill)
            .id("verification-header");
        let body = scrollable(container(content).padding(24).width(Length::Fill))
            .id("verification-body")
            .direction(shell::scrollbar())
            .style(shell::scroll_style)
            .height(Length::Fill);
        let footer = container(buttons)
            .padding([14, 24])
            .width(Length::Fill)
            .id("verification-actions");
        // Keep short phases compact while allowing a bounded, scrolling device
        // list or comparison. The footer always gets its space before the body.
        let preferred_height = match &snapshot.phase {
            Phase::Compare { .. } => 620.0,
            Phase::Devices(devices) => (350.0 + devices.len() as f32 * 72.0).min(620.0),
            Phase::Waiting { .. } | Phase::Complete => 400.0,
            Phase::Idle | Phase::Loading => 320.0,
            Phase::Confirmed { .. } | Phase::Cancelled | Phase::Failed(_) => 360.0,
        } + if snapshot.incoming.is_some() {
            48.0
        } else {
            0.0
        };
        let card = container(
            column![
                header,
                iced::widget::rule::horizontal(1),
                body,
                iced::widget::rule::horizontal(1),
                footer
            ]
            .spacing(0),
        )
        .width((size.width - 32.0).clamp(1.0, 580.0))
        .height((size.height - 32.0).clamp(1.0, preferred_height))
        .clip(true)
        .style(|theme| container::Style {
            border: iced::Border {
                color: shell::palette(theme).line,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..shell::surface(theme, shell::palette(theme).background)
        });
        opaque(
            mouse_area(
                container(card)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .padding(16)
                    .style(|theme| shell::surface(theme, shell::palette(theme).scrim)),
            )
            .on_press(message(Event::Ignore))
            .on_scroll(|_| message(Event::Ignore)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Draft, DraftKey};

    #[test]
    fn demo_comparison_is_local_and_preserves_conversation_drafts_and_scroll() {
        let mut app = super::super::tests::demo_app();
        let selection = app.selection.clone();
        let key = DraftKey {
            room: selection.room.as_ref().unwrap().1.clone(),
            root: None,
        };
        app.drafts.insert(
            key.clone(),
            Draft {
                content: iced::widget::text_editor::Content::with_text("Unsent draft"),
                ..Draft::default()
            },
        );
        app.scroll_positions.insert(key.clone(), 120.0);
        let _ = app.update(message(Event::Open));
        assert!(app.verification.open);
        assert!(app.verification.demo.control.is_none());
        let revision = app.verification.demo.revision;
        let _ = app.update(message(Event::Action(
            revision,
            Action::Start("OTHER-DEMO".into()),
        )));
        let revision = app.verification.demo.revision;
        let _ = app.update(message(Event::Action(
            revision - 1,
            Action::Confirm((1234, 5678, 9012)),
        )));
        assert!(matches!(app.verification.demo.phase, Phase::Compare { .. }));
        let _ = app.update(message(Event::Action(revision, Action::Confirm((0, 0, 0)))));
        assert!(matches!(app.verification.demo.phase, Phase::Compare { .. }));
        let _ = app.update(message(Event::Action(
            revision,
            Action::Confirm((1234, 5678, 9012)),
        )));
        assert_eq!(app.verification.demo.phase, Phase::Complete);
        assert_eq!(app.verification.demo.trust, VerificationState::Unverified);
        let _ = app.update(Message::Escape);
        assert!(!app.verification.open);
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert_eq!(app.drafts[&key].content.text(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 120.0);
        assert!(app.model.verification.control.is_none());
    }

    #[test]
    fn modal_blocks_navigation_and_settings_and_suppresses_visible_conversation() {
        let mut app = super::super::tests::demo_app();
        let _ = app.update(Message::WindowFocused(true));
        assert!(app.selection.conversation_visible);
        let _ = app.update(message(Event::Open));
        assert!(!app.selection.conversation_visible);
        let selected = app.selection.room.clone();
        let _ = app.update(Message::Room("other-room".into()));
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert_eq!(app.selection.room, selected);
        assert!(app.settings.draft.is_none());
        let _ = app.update(Message::Escape);
        assert!(app.selection.conversation_visible);
    }

    #[test]
    fn all_phases_fit_minimum_window_and_backdrop_captures_input() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = super::super::tests::demo_app();
        let _ = app.update(message(Event::Open));
        let revision = app.verification.demo.revision;
        let _ = app.update(message(Event::Action(
            revision,
            Action::Start("OTHER-DEMO".into()),
        )));
        let comparison = app.verification.demo.phase.clone();
        #[derive(Default)]
        struct Bounds {
            header: Option<iced::Rectangle>,
            body: Option<(iced::Rectangle, iced::Rectangle)>,
            footer: Option<iced::Rectangle>,
            actions: Vec<(String, iced::Rectangle)>,
        }
        impl widget::Operation for Bounds {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("verification-header")) {
                    self.header = Some(bounds);
                }
                if id == Some(&widget::Id::new("verification-actions")) {
                    self.footer = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                _: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("verification-body")) {
                    self.body = Some((bounds, content));
                }
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, value: &str) {
                if matches!(
                    value,
                    "Close"
                        | "Verify later"
                        | "Cancel verification"
                        | "They match"
                        | "They don't match"
                        | "Accept request"
                ) {
                    self.actions.push((value.into(), bounds));
                }
            }
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for phase in [
            Phase::Loading,
            Phase::Devices(Vec::new()),
            Phase::Waiting {
                device: "OTHER-DEMO".into(),
            },
            comparison,
            Phase::Confirmed {
                device: "OTHER-DEMO".into(),
            },
            Phase::Complete,
            Phase::Cancelled,
            Phase::Failed("Verification timed out. Try again.".into()),
        ] {
            app.verification.demo.incoming =
                matches!(phase, Phase::Devices(_)).then(|| interconnect::verification::Incoming {
                    flow: "sample-flow".into(),
                    device: "OTHER-DEMO".into(),
                });
            app.verification.demo.phase = phase;
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let size = iced::Size::new(640.0, 480.0);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert_eq!(node.size(), size);
            let mut bounds = Bounds::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut bounds,
            );
            let header = bounds.header.unwrap();
            let (body, body_content) = bounds.body.unwrap();
            let footer = bounds.footer.unwrap();
            assert!(header.y >= 16.0 && header.y + header.height <= body.y);
            assert!(body.y + body.height <= footer.y);
            assert!(footer.y + footer.height <= size.height - 16.0);
            assert!(!bounds.actions.is_empty());
            for (_, action) in &bounds.actions {
                assert!(action.x >= footer.x && action.x + action.width <= footer.x + footer.width);
                assert!(
                    action.y >= footer.y && action.y + action.height <= footer.y + footer.height
                );
            }
            if app.verification.demo.incoming.is_some() {
                assert!(
                    bounds
                        .actions
                        .iter()
                        .any(|(name, _)| name == "Accept request")
                );
            }
            if matches!(app.verification.demo.phase, Phase::Compare { .. }) {
                assert!(
                    body_content.height > body.height,
                    "comparison body must scroll in a short window"
                );
                assert!(bounds.actions.iter().any(|(name, _)| name == "They match"));
                assert!(
                    bounds
                        .actions
                        .iter()
                        .any(|(name, _)| name == "They don't match")
                );
            }
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut widget::operation::scrollable::scroll_to::<()>(
                    widget::Id::new("verification-body"),
                    iced::widget::operation::AbsoluteOffset {
                        x: None,
                        y: Some(1000.0),
                    },
                ),
            );
            let mut scrolled = Bounds::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut scrolled,
            );
            assert_eq!(
                scrolled.actions, bounds.actions,
                "scrolling must leave actions fixed"
            );
            let event = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &event,
                layout::Layout::new(&node),
                mouse::Cursor::Available(iced::Point::new(5.0, 40.0)),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert!(shell.is_event_captured());
            assert!(
                messages
                    .iter()
                    .all(|message| matches!(message, Message::Verification(Event::Ignore)))
            );
        }
    }
}
