//! SAS presentation only. Confirmation carries the displayed flow and numbers.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, checkbox, column, container, row, text},
};
use interconnect::verification::{Action, Comparison, Device, Phase, Snapshot};
use matrix_sdk::encryption::VerificationState;

pub struct State {
    pub open: bool,
    pub recovery: super::recovery::State,
    error: Option<String>,
    confirming: Option<(u64, (u16, u16, u16))>,
    demo: Snapshot,
    prefetch_keys: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            recovery: super::recovery::State::default(),
            error: None,
            confirming: None,
            demo: Snapshot::default(),
            prefetch_keys: true,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    OpenRecovery,
    Recovery(super::recovery::Event),
    Close,
    Action(u64, Action),
    PrefetchKeys(u64, bool),
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
            Event::Recovery(event) => return self.recovery_update(event),
            Event::OpenRecovery
                if !self.closing
                    && !self.stopped
                    && self.settings.draft.is_none()
                    && self.image_view.is_none()
                    && (!self.verification.open
                        || !self.verification_snapshot().phase.active())
                    && (self.demo.is_some() || self.model.recovery.control.is_some()) =>
            {
                if self.verification_snapshot().phase == Phase::Complete
                    && let Some(control) = &self.model.verification.control
                    && let Err(error) = control.send(
                        self.model.verification.revision,
                        Action::Finish {
                            prefetch_keys: self.verification.prefetch_keys,
                        },
                    )
                {
                    self.verification.error = Some(error);
                    return Task::none();
                }
                self.reaction_picker = None;
                self.composer_picker = None;
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.verification.open = true;
                self.open_recovery();
            }
            Event::Open
                if !self.closing
                    && !self.stopped
                    && self.settings.draft.is_none()
                    && self.image_view.is_none()
                    && (!self.verification.recovery.open || self.recovery_can_leave())
                    && (self.demo.is_some() || self.model.verification.control.is_some()) =>
            {
                self.reaction_picker = None;
                self.composer_picker = None;
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.verification.open = true;
                self.verification.recovery.open = false;
                self.clear_recovery_input();
                self.verification.error = None;
                self.verification.prefetch_keys = true;
                if self.demo.is_some() {
                    self.verification.demo = Snapshot {
                        user: "@jamie:example.org".into(),
                        device: "DEMO".into(),
                        trust: VerificationState::Unverified,
                        incoming: self.model.verification.incoming.clone(),
                        phase: if self.model.verification.incoming.is_some() {
                            Phase::Idle
                        } else {
                            Phase::Devices(vec![Device {
                                id: "OTHER-DEMO".into(),
                                name: "Example phone".into(),
                                verified: false,
                            }])
                        },
                        revision: self.verification.demo.revision + 1,
                        ..Snapshot::default()
                    };
                } else if self.model.verification.incoming.is_none()
                    && matches!(
                        self.model.verification.phase,
                        Phase::Idle | Phase::Cancelled | Phase::Complete | Phase::Failed(_)
                    )
                {
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
                if self.verification.recovery.open {
                    if !self.recovery_can_leave() {
                        return Task::none();
                    }
                    self.verification.recovery.open = false;
                    self.clear_recovery_input();
                    self.verification.open = false;
                    return Task::none();
                }
                let complete = self.verification_snapshot().phase == Phase::Complete;
                if self.demo.is_none()
                    && let Some(control) = &self.model.verification.control
                {
                    let action = if complete {
                        Action::Finish {
                            prefetch_keys: self.verification.prefetch_keys,
                        }
                    } else {
                        Action::Cancel
                    };
                    self.verification.error =
                        control.send(self.model.verification.revision, action).err();
                    if complete && self.verification.error.is_some() {
                        return Task::none();
                    }
                }
                self.verification.open = false;
                self.verification.confirming = None;
            }
            Event::PrefetchKeys(revision, value)
                if self.verification.open
                    && !self.closing
                    && self.verification_snapshot().revision == revision
                    && self.verification_snapshot().phase == Phase::Complete =>
            {
                self.verification.prefetch_keys = value;
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
        if self.verification.recovery.open {
            return self.recovery_view();
        }
        super::focus_scope::dismissible(
            iced::widget::responsive(move |size| self.verification_card(size)),
            "verification-focus",
            Message::Escape,
        )
    }
    fn verification_card(&self, size: iced::Size) -> Element<'_, Message> {
        let snapshot = self.verification_snapshot();
        let action = |action| message(Event::Action(snapshot.revision, action));
        let compact_comparison =
            size.height < 640.0 && matches!(snapshot.phase, Phase::Compare { .. });
        let mut content = column![].spacing(if compact_comparison { 10 } else { 16 });
        if !compact_comparison {
            content = content.push(text(snapshot.user.clone()).size(13)).push(
                text(format!(
                    "This device: {} · Trust: {}",
                    snapshot.device,
                    trust_label(snapshot.trust)
                ))
                .size(12)
                .style(shell::muted_text),
            );
            if self.demo.is_some() {
                content = content.push(
                    text(
                        "Design preview · sample comparison only. No device or keys are verified.",
                    )
                    .size(12)
                    .style(shell::accent_text),
                );
            }
        }
        if snapshot.incoming.is_none()
            && matches!(
                snapshot.phase,
                Phase::Idle | Phase::Loading | Phase::Devices(_)
            )
        {
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
            Phase::Idle | Phase::Loading => {
                if snapshot.incoming.is_none() { content = content.push(text("Loading your devices…")); }
            }
            Phase::Devices(devices) => {
                if snapshot.incoming.is_none() {
                    content = content.push(text("Open your other Matrix app, keep it open, and choose that device below."));
                    if devices.is_empty() { content = content.push(text("No other devices found. Sign in on another Matrix client, then refresh.")); }
                    for device in devices {
                        content = content.push(button(column![
                            text(device.name.clone()).size(15),
                            text(format!("{} · {}", device.id, if device.verified { "Verified" } else { "Unverified" })).size(12),
                        ].spacing(4)).keyboard_navigation(true).width(Length::Fill).padding(12).style(shell::quiet_button)
                            .on_press(action(Action::Start(device.id.clone()))));
                    }
                }
            }
            Phase::Waiting { device } => content = content.push(text(format!("Waiting for {device}. Accept the request on your other client.")))
                .push(text("Verification expires after three minutes. You can cancel at any time.").size(12).style(shell::muted_text)),
            Phase::Compare { device, comparison } => {
                if !compact_comparison {
                    content = content.push(shell::title("Compare with your other device", 19));
                }
                content = content.push(text(format!("Other device: {device}" )).size(13))
                    .push(text("Check all seven emojis or all three numbers on both devices.").size(13));
                if comparison.emojis.len() == 7 {
                    let mut emojis = row![].spacing(8);
                    for (symbol, description) in &comparison.emojis {
                        emojis = emojis.push(column![
                            text(symbol.clone()).size(if compact_comparison { 26 } else { 28 }).font(super::emoji::font().unwrap_or_default()),
                            text(description.clone()).size(10),
                        ].spacing(4).width(Length::Fill).align_x(iced::Alignment::Center));
                    }
                    content = content.push(emojis);
                }
                let (a, b, c) = comparison.decimals;
                content = content.push(container(text(format!("{a}    {b}    {c}")).size(if compact_comparison { 24 } else { 26 })).center_x(Length::Fill).padding(if compact_comparison { 6 } else { 12 }));
            }
            Phase::Confirmed { device } => content = content.push(text(format!("Confirmed here. Waiting for {device} to confirm…"))),
            Phase::Complete => content = content.push(shell::title(if self.demo.is_some() { "Demo comparison complete" } else { "Comparison complete on both devices" }, 19))
                .push(text("Device trust above is reported by Matrix. Historical message keys may still need to be shared by your trusted client.").size(13))
                .push(checkbox(self.verification.prefetch_keys).keyboard_navigation(true)
                    .label("Request room keys in the background")
                    .size(16).text_size(13).spacing(9)
                    .on_toggle(move |value| message(Event::PrefetchKeys(snapshot.revision, value))))
                .push(text("After closing, prepare recent encrypted messages across your rooms. Keep your other trusted client online.").size(12).style(shell::muted_text)),
            Phase::Cancelled => content = content.push(text("Verification cancelled. No automatic confirmation was sent.")),
            Phase::Failed(error) => content = content.push(text(error.clone()).size(13).style(shell::danger_text)),
        }
        if let Some(error) = &self.verification.error {
            content = content.push(text(error).size(12).style(shell::danger_text));
        }
        let mut buttons = row![].spacing(8).align_y(iced::Alignment::Center);
        if !snapshot.phase.active()
            && snapshot.incoming.is_none()
            && (self.demo.is_some() || self.model.recovery.control.is_some())
        {
            buttons = buttons.push(
                button(text("Recovery…").size(13))
                    .keyboard_navigation(true)
                    .on_press(message(Event::OpenRecovery))
                    .style(shell::quiet_button),
            );
        }
        if !snapshot.phase.active()
            && snapshot.incoming.is_none()
            && snapshot.phase != Phase::Complete
            && self.demo.is_none()
            && snapshot.control.is_some()
        {
            buttons = buttons.push(super::tooltips::icon_button(
                super::icons::Kind::Refresh,
                "Refresh verification devices",
                Some(action(Action::Refresh)),
            ));
        }
        let dismiss: Element<'_, Message> = if !snapshot.phase.active()
            && snapshot.trust != VerificationState::Verified
            && snapshot.phase != Phase::Complete
        {
            // Deferring verification is a security choice, so keep its wording.
            button(text("Verify later").size(13))
                .keyboard_navigation(true)
                .on_press(message(Event::Close))
                .padding([7, 10])
                .style(shell::quiet_button)
                .into()
        } else {
            super::tooltips::icon_button(
                super::icons::Kind::Close,
                if snapshot.phase.active() {
                    "Cancel verification"
                } else {
                    "Close verification"
                },
                Some(message(Event::Close)),
            )
            .into()
        };
        buttons = buttons
            .push(container(dismiss).id("verification-dismiss"))
            .push(iced::widget::space().width(Length::Fill));
        if let Phase::Compare { comparison, .. } = &snapshot.phase {
            buttons = buttons
                .push(
                    container(
                        button(text("They don't match").size(13))
                            .keyboard_navigation(true)
                            .on_press(action(Action::Mismatch))
                            .padding([7, 10])
                            .style(shell::quiet_button),
                    )
                    .id("verification-mismatch"),
                )
                .push(
                    container(
                        button(text("They match").size(13))
                            .keyboard_navigation(true)
                            .style(shell::primary_button)
                            .on_press_maybe(
                                (self.verification.confirming
                                    != Some((snapshot.revision, comparison.decimals)))
                                .then(|| action(Action::Confirm(comparison.decimals))),
                            )
                            .padding([7, 10]),
                    )
                    .id("verification-match"),
                );
        } else if let Some(incoming) = &snapshot.incoming
            && !snapshot.phase.active()
        {
            buttons = buttons.push(
                container(
                    button(text("Accept request").size(13))
                        .keyboard_navigation(true)
                        .style(shell::primary_button)
                        .on_press(action(Action::Accept(incoming.flow.clone())))
                        .padding([7, 10]),
                )
                .id("verification-accept"),
            );
        }
        let mut heading = column![shell::title(
            if compact_comparison {
                "Compare your devices"
            } else {
                "Verify this device"
            },
            20
        )]
        .spacing(6);
        if compact_comparison {
            heading = heading.push(
                text(format!(
                    "{} · This device: {}{}",
                    snapshot.user,
                    snapshot.device,
                    if self.demo.is_some() {
                        " · Demo only"
                    } else {
                        ""
                    }
                ))
                .size(11)
                .style(shell::muted_text),
            );
        }
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
        super::dialog::modal(
            super::dialog::frame(
                size,
                580.0,
                preferred_height,
                "verification",
                heading,
                content,
                buttons,
            ),
            message(Event::Ignore),
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
        assert!(app.verification.prefetch_keys);
        let _ = app.update(message(Event::PrefetchKeys(revision - 1, false)));
        assert!(
            app.verification.prefetch_keys,
            "stale checkbox events must be rejected"
        );
        let _ = app.update(message(Event::PrefetchKeys(revision, false)));
        assert!(!app.verification.prefetch_keys);
        let _ = app.update(Message::Escape);
        assert!(!app.verification.open);
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert_eq!(app.drafts[&key].content.text(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 120.0);
        assert!(app.model.verification.control.is_none());
        let _ = app.update(message(Event::PrefetchKeys(revision, true)));
        assert!(
            !app.verification.prefetch_keys,
            "closed dialogs reject late toggles"
        );
        let _ = app.update(message(Event::Open));
        assert!(
            app.verification.prefetch_keys,
            "each dialog defaults to checked"
        );
        let revision = app.verification.demo.revision;
        let _ = app.update(message(Event::PrefetchKeys(revision, false)));
        assert!(
            app.verification.prefetch_keys,
            "only the completion dialog accepts toggles"
        );
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

    #[tokio::test]
    async fn reviewing_an_incoming_request_does_not_refresh_or_change_its_revision() {
        let client = matrix_sdk::Client::builder()
            .homeserver_url("https://example.org")
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .build()
            .await
            .unwrap();
        let (state, snapshots) =
            tokio::sync::watch::channel(interconnect::connected::Model::default());
        let stop = tokio_util::sync::CancellationToken::new();
        let mut worker = Box::pin(
            interconnect::verification::Service::new(client).run(state.clone(), stop.clone()),
        );
        assert!(iced::futures::poll!(&mut worker).is_pending());
        let incoming = interconnect::verification::Incoming {
            flow: "existing-transaction".into(),
            device: "OTHER".into(),
        };
        state.send_modify(|s| s.verification.incoming = Some(incoming.clone()));
        let mut app = super::super::tests::demo_app();
        app.demo = None;
        app.model = snapshots.borrow().clone();
        let revision = app.model.verification.revision;
        let _ = app.update(message(Event::Open));
        assert!(app.verification.open);
        assert!(iced::futures::poll!(&mut worker).is_pending());
        let snapshot = snapshots.borrow().verification.clone();
        assert_eq!(snapshot.revision, revision);
        assert_eq!(snapshot.phase, Phase::Idle);
        assert_eq!(snapshot.incoming, Some(incoming));
        stop.cancel();
        worker.await;
    }

    #[tokio::test]
    async fn closing_the_final_dialog_submits_the_checkbox_choice() {
        for choice in [false, true] {
            // Only exercise the local command channel. SAS completion itself is
            // covered by the backend's two-device crypto fixtures.
            let client = matrix_sdk::Client::builder()
                .homeserver_url("https://example.org")
                .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
                .build()
                .await
                .unwrap();
            let (state, snapshots) =
                tokio::sync::watch::channel(interconnect::connected::Model::default());
            let stop = tokio_util::sync::CancellationToken::new();
            let mut worker = Box::pin(
                interconnect::verification::Service::new(client).run(state.clone(), stop.clone()),
            );
            assert!(iced::futures::poll!(&mut worker).is_pending());
            state.send_modify(|s| s.verification.phase = Phase::Complete);

            let mut app = super::super::tests::demo_app();
            app.demo = None;
            app.model = snapshots.borrow().clone();
            app.verification.open = true;
            let revision = app.model.verification.revision;
            let _ = app.update(message(Event::PrefetchKeys(revision, choice)));
            // Escape on completion has exactly the same semantics as Close.
            let _ = app.update(if choice {
                message(Event::Close)
            } else {
                Message::Escape
            });
            assert!(!app.verification.open);
            assert!(iced::futures::poll!(&mut worker).is_pending());
            assert_eq!(snapshots.borrow().verification.prefetch_keys, Some(choice));
            stop.cancel();
            worker.await;
        }
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
            focusable: Vec<iced::Rectangle>,
            prefetch: Option<iced::Rectangle>,
            body_translation: iced::Vector,
            device_visible: bool,
            comparison_numbers: Option<iced::Rectangle>,
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
                for name in [
                    "verification-dismiss",
                    "verification-match",
                    "verification-mismatch",
                    "verification-accept",
                ] {
                    if id == Some(&widget::Id::new(name)) {
                        self.actions.push((name.into(), bounds));
                    }
                }
            }
            fn focusable(
                &mut self,
                _: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: &mut dyn widget::operation::Focusable,
            ) {
                self.focusable.push(bounds);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("verification-body")) {
                    self.body = Some((bounds, content));
                    self.body_translation = translation;
                }
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, value: &str) {
                if value == "1234    5678    9012" {
                    self.comparison_numbers = Some(bounds);
                }
                if value == "Example phone" {
                    self.device_visible = true;
                }
                if value == "Request room keys in the background" {
                    self.prefetch = Some(bounds);
                }
            }
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for phase in [
            Phase::Loading,
            Phase::Devices(vec![Device {
                id: "OTHER-DEMO".into(),
                name: "Example phone".into(),
                verified: false,
            }]),
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
            assert!(
                footer.y + footer.height <= size.height - 16.0 + 0.1,
                "phase {:?}: footer {footer:?}, header {header:?}, body {body:?}",
                app.verification.demo.phase
            );
            assert!(!bounds.actions.is_empty());
            for (_, action) in &bounds.actions {
                assert!(
                    bounds.focusable.iter().any(|button| button == action),
                    "footer action must expose an actual focusable control: {action:?}"
                );
                assert!(action.x >= footer.x && action.x + action.width <= footer.x + footer.width);
                assert!(
                    action.y >= footer.y && action.y + action.height <= footer.y + footer.height
                );
            }
            if app.verification.demo.incoming.is_some() {
                assert!(
                    !bounds.device_visible,
                    "incoming review must hide outgoing device buttons"
                );
                let (_, accept) = bounds
                    .actions
                    .iter()
                    .find(|(name, _)| name == "verification-accept")
                    .unwrap();
                let mut messages = Vec::new();
                for event in [
                    mouse::Event::ButtonPressed(mouse::Button::Left),
                    mouse::Event::ButtonReleased(mouse::Button::Left),
                ] {
                    view.as_widget_mut().update(
                        &mut tree,
                        &iced::Event::Mouse(event),
                        layout::Layout::new(&node),
                        mouse::Cursor::Available(accept.center()),
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut Shell::new(&mut messages),
                        &iced::Rectangle::with_size(size),
                    );
                }
                assert!(messages.iter().any(|message| matches!(message,
                    Message::Verification(Event::Action(_, Action::Accept(flow))) if flow == "sample-flow"
                )), "Accept request must retain the incoming transaction");
                assert!(!messages.iter().any(|message| matches!(
                    message,
                    Message::Verification(Event::Action(_, Action::Start(_)))
                )));
            }
            if matches!(app.verification.demo.phase, Phase::Compare { .. }) {
                assert!(
                    body_content.height <= body.height + 1.0,
                    "the complete comparison fits without scrolling in a short window"
                );
                let numbers = bounds.comparison_numbers.expect("comparison numbers");
                assert!(
                    numbers.y >= body.y && numbers.y + numbers.height <= body.y + body.height,
                    "all three numbers must be visible before scrolling"
                );
                assert!(
                    bounds
                        .actions
                        .iter()
                        .any(|(name, _)| name == "verification-match")
                );
                assert!(
                    bounds
                        .actions
                        .iter()
                        .any(|(name, _)| name == "verification-mismatch")
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
            assert_eq!(
                bounds.prefetch.is_some(),
                app.verification.demo.phase == Phase::Complete
            );
            if let Some(checkbox) = scrolled.prefetch {
                let point = checkbox.center() - scrolled.body_translation;
                assert!(
                    body.contains(point),
                    "checkbox must be visible after scrolling"
                );
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    layout::Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                assert!(shell.is_event_captured());
                assert!(
                    messages.iter().any(|message| matches!(
                        message,
                        Message::Verification(Event::PrefetchKeys(_, false))
                    )),
                    "the checked checkbox must be reachable and toggle off at minimum size"
                );
            }
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
