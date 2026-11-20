//! Recovery shares the verification modal so existing focus/input guards apply.
use super::{App, Message, shell, verification};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, text, text_input},
};
use interconnect::recovery::{Action, Secret, Snapshot};
use matrix_sdk::encryption::{backups::BackupState, recovery::RecoveryState};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct State {
    pub open: bool,
    input: Zeroizing<String>,
    pending: Option<u64>,
    replace: bool,
    error: Option<String>,
    demo: Snapshot,
}

#[derive(Clone, Debug)]
pub enum Event {
    Input(u64, Secret),
    Create(u64),
    Recover(u64),
    Replace(u64),
    ConfirmReplace(u64),
    CancelReplace,
    Saved(u64),
    Copy(u64),
    Upload(u64),
}
fn message(event: Event) -> Message {
    Message::Verification(verification::Event::Recovery(event))
}

impl App {
    fn recovery_snapshot(&self) -> &Snapshot {
        if self.demo.is_some() {
            &self.verification.recovery.demo
        } else {
            &self.model.recovery
        }
    }
    pub(super) fn recovery_can_leave(&self) -> bool {
        let snapshot = self.recovery_snapshot();
        !snapshot.busy
            && snapshot.key.is_none()
            && self.verification.recovery.pending != Some(snapshot.revision)
    }
    pub(super) fn recovery_update(&mut self, event: Event) -> Task<Message> {
        if !self.verification.open
            || !self.verification.recovery.open
            || self.closing
            || self.stopped
        {
            return Task::none();
        }
        let snapshot = self.recovery_snapshot();
        let revision = snapshot.revision;
        let ready = !snapshot.busy && self.verification.recovery.pending != Some(revision);
        let mut action = None;
        match event {
            Event::Input(expected, value) if expected == revision && ready => {
                self.verification.recovery.input = Zeroizing::new(value.expose().to_owned());
            }
            Event::Copy(expected) if expected == revision => {
                if let Some(key) = &snapshot.key {
                    return iced::clipboard::write(key.expose().to_owned());
                }
            }
            Event::Create(expected)
                if expected == revision
                    && ready
                    && matches!(
                        snapshot.recovery,
                        RecoveryState::Unknown | RecoveryState::Disabled
                    )
                    && snapshot.key.is_none() =>
            {
                action = Some(Action::Create)
            }
            Event::Recover(expected)
                if expected == revision
                    && ready
                    && snapshot.key.is_none()
                    && !self.verification.recovery.input.trim().is_empty() =>
            {
                let input = std::mem::take(&mut self.verification.recovery.input);
                action = Some(Action::Recover(Secret::new(input.to_string())));
            }
            Event::Replace(expected)
                if expected == revision
                    && ready
                    && snapshot.recovery == RecoveryState::Enabled
                    && snapshot.key.is_none() =>
            {
                self.verification.recovery.replace = true
            }
            Event::ConfirmReplace(expected)
                if expected == revision
                    && ready
                    && self.verification.recovery.replace
                    && snapshot.recovery == RecoveryState::Enabled
                    && snapshot.key.is_none() =>
            {
                self.verification.recovery.replace = false;
                action = Some(Action::Replace);
            }
            Event::CancelReplace => self.verification.recovery.replace = false,
            Event::Saved(expected) if expected == revision && ready && snapshot.key.is_some() => {
                action = Some(Action::Acknowledge)
            }
            Event::Upload(expected)
                if expected == revision
                    && ready
                    && snapshot.backup == BackupState::Enabled
                    && snapshot.key.is_none() =>
            {
                action = Some(Action::Upload)
            }
            _ => {}
        }
        if let Some(action) = action {
            self.verification.recovery.error = None;
            if self.demo.is_some() {
                let snapshot = &mut self.verification.recovery.demo;
                snapshot.revision += 1;
                match action {
                    Action::Create | Action::Replace => {
                        snapshot.recovery = RecoveryState::Enabled;
                        snapshot.backup = BackupState::Enabled;
                        snapshot.key =
                            Some(Secret::new("DEMO ONLY · this is not a recovery key".into()));
                    }
                    Action::Acknowledge => snapshot.key = None,
                    Action::Recover(_) => {
                        snapshot.notice = Some("Demo only · no secrets imported.".into())
                    }
                    Action::Upload => {
                        snapshot.notice = Some("Demo only · no keys uploaded.".into())
                    }
                }
            } else if let Some(control) = &self.model.recovery.control {
                match control.send(revision, action) {
                    Ok(()) => self.verification.recovery.pending = Some(revision),
                    Err(error) => self.verification.recovery.error = Some(error),
                }
            }
        }
        Task::none()
    }

    pub(super) fn open_recovery(&mut self) {
        self.verification.recovery.open = true;
        self.verification.recovery.input = Zeroizing::new(String::new());
        self.verification.recovery.replace = false;
        self.verification.recovery.error = None;
        if self.demo.is_some() && self.verification.recovery.demo.revision == 0 {
            self.verification.recovery.demo.revision = 1;
            self.verification.recovery.demo.recovery = RecoveryState::Disabled;
        }
    }

    pub(super) fn clear_recovery_input(&mut self) {
        self.verification.recovery.input = Zeroizing::new(String::new());
        self.verification.recovery.replace = false;
    }

    pub(super) fn defer_close_for_recovery(&mut self) -> bool {
        if self.verification.open && self.verification.recovery.open && !self.recovery_can_leave() {
            self.verification.recovery.error = Some("Save your recovery key and confirm it is saved before closing. If an operation is running, wait for it to finish.".into());
            return true;
        }
        false
    }

    pub(super) fn recovery_view(&self) -> Element<'_, Message> {
        super::focus_scope::dismissible(
            iced::widget::responsive(move |size| {
                let snapshot = self.recovery_snapshot();
                let revision = snapshot.revision;
                let ready = !snapshot.busy && self.verification.recovery.pending != Some(revision);
                let mut content = column![
                text("Keep access to your encrypted conversations if you lose a device. Your homeserver stores encrypted room keys; your recovery key unlocks your backup and encryption identity.").size(13),
                text(match snapshot.recovery {
                    RecoveryState::Unknown => "Recovery: status not yet available",
                    RecoveryState::Disabled => "Recovery: not set up",
                    RecoveryState::Incomplete => "Recovery: enter your key or verify with another device",
                    RecoveryState::Enabled => "Recovery: ready on this device",
                }).size(14),
                text(match snapshot.backup {
                    BackupState::Enabled => "Key backup: enabled · keys upload automatically",
                    BackupState::Downloading => "Key backup: downloading keys…",
                    BackupState::Creating | BackupState::Enabling | BackupState::Resuming => "Key backup: connecting…",
                    BackupState::Unknown => "Key backup: not connected on this device",
                    BackupState::Disabling => "Key backup: disabling…",
                }).size(13).style(shell::muted_text),
            ].spacing(16);
                if self.demo.is_some() {
                    content = content.push(
                        text("Design preview · no account, backup, or secrets are changed.")
                            .size(12)
                            .style(shell::accent_text),
                    );
                }
                let mut footer = row![].spacing(8);
                if let Some(key) = &snapshot.key {
                    content = content.push(shell::title("Save your recovery key", 18))
                    .push(text("Store this in a password manager or another safe place. Anyone with this key can unlock your encrypted history. This key is shown until you confirm it is saved.").size(13))
                    .push(container(text(key.expose()).font(iced::Font::MONOSPACE).size(16)).padding(16).width(Length::Fill).style(|theme| shell::surface(theme, shell::palette(theme).sidebar)))
                    .push(button("Copy recovery key").keyboard_navigation(true).on_press(message(Event::Copy(revision))).style(shell::quiet_button))
                    .push(text("Copying puts the key on your system clipboard. Clear it after saving.").size(12).style(shell::muted_text));
                    footer = footer.push(
                        button("I've saved my key")
                            .keyboard_navigation(true)
                            .on_press_maybe(ready.then_some(message(Event::Saved(revision))))
                            .style(shell::primary_button),
                    );
                } else if self.verification.recovery.replace {
                    content = content.push(shell::title("Replace your recovery key?", 18))
                    .push(text("The previous key will no longer unlock recovery. Your existing message backup is kept. Save the new key before leaving this screen.").size(13));
                    footer = footer
                        .push(
                            button("Cancel")
                                .keyboard_navigation(true)
                                .on_press(message(Event::CancelReplace))
                                .style(shell::quiet_button),
                        )
                        .push(
                            button("Replace recovery key")
                                .keyboard_navigation(true)
                                .on_press_maybe(
                                    ready.then_some(message(Event::ConfirmReplace(revision))),
                                )
                                .style(button::danger),
                        );
                } else {
                    match snapshot.recovery {
                        RecoveryState::Unknown | RecoveryState::Disabled => {
                            content = content.push(
                                button("Set up recovery")
                                    .keyboard_navigation(true)
                                    .on_press_maybe(
                                        ready.then_some(message(Event::Create(revision))),
                                    )
                                    .style(shell::primary_button),
                            );
                        }
                        RecoveryState::Enabled => {
                            content = content.push(text("Your existing recovery key cannot be displayed here. If you no longer have it, create a replacement while your secrets are available on this device.").size(13))
                            .push(button("Replace recovery key…").keyboard_navigation(true).on_press_maybe(ready.then_some(message(Event::Replace(revision)))).style(shell::quiet_button));
                        }
                        _ => {}
                    }
                    content = content.push(shell::title("Use an existing recovery key", 16));
                    let mut input = text_input(
                        "Recovery key or passphrase",
                        &self.verification.recovery.input,
                    )
                    .id("recovery-key-input")
                    .cursor_blinking(false)
                    .style(shell::search_style)
                    .secure(true)
                    .padding(10);
                    if ready {
                        input = input
                            .on_input(move |value| {
                                message(Event::Input(revision, Secret::new(value)))
                            })
                            .on_submit(message(Event::Recover(revision)));
                    }
                    content = content
                        .push(text("Recovery key or passphrase").size(13))
                        .push(input)
                        .push(
                            button("Recover encrypted history")
                                .keyboard_navigation(true)
                                .on_press_maybe(
                                    (ready && !self.verification.recovery.input.trim().is_empty())
                                        .then_some(message(Event::Recover(revision))),
                                )
                                .style(shell::primary_button),
                        );
                    if snapshot.backup == BackupState::Enabled {
                        content = content.push(
                            button("Back up now")
                                .keyboard_navigation(true)
                                .on_press_maybe(ready.then_some(message(Event::Upload(revision))))
                                .style(shell::quiet_button),
                        );
                    }
                    footer =
                        footer
                            .push(super::tooltips::icon_button(
                                super::icons::Kind::Close,
                                "Close recovery",
                                ready.then_some(Message::Verification(verification::Event::Close)),
                            ))
                            .push(
                                button("Verify with another device…")
                                    .keyboard_navigation(true)
                                    .on_press_maybe(ready.then_some(Message::Verification(
                                        verification::Event::Open,
                                    )))
                                    .style(shell::quiet_button),
                            );
                }
                if !ready {
                    content =
                        content.push(text("Working… Stay signed in while this finishes.").size(13));
                }
                if let Some(notice) = &snapshot.notice {
                    content = content.push(text(notice).size(13));
                }
                if let Some(error) = self
                    .verification
                    .recovery
                    .error
                    .as_ref()
                    .or(snapshot.error.as_ref())
                {
                    content = content.push(text(error).size(13).style(shell::danger_text));
                }
                super::dialog::modal(
                    super::dialog::frame(
                        size,
                        580.0,
                        650.0,
                        "recovery",
                        shell::title("Key backup & recovery", 20),
                        content,
                        footer.wrap(),
                    ),
                    Message::Verification(verification::Event::Ignore),
                )
            }),
            "recovery-focus",
            Message::Escape,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    #[test]
    fn demo_setup_retains_key_until_saved_and_preserves_conversation() {
        let mut app = demo_app();
        let room = app.selection.room.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unfinished draft");
        app.scroll_positions.insert(key.clone(), 144.0);
        let _ = app.update(Message::Verification(verification::Event::OpenRecovery));
        let revision = app.recovery_snapshot().revision;
        let _ = app.update(message(Event::Create(revision)));
        assert!(app.recovery_snapshot().key.is_some());
        let revision = app.recovery_snapshot().revision;
        for event in [
            Message::Escape,
            Message::Close,
            Message::Verification(verification::Event::Open),
            Message::Room("general".into()),
            Message::Send(key.clone()),
        ] {
            let _ = app.update(event);
            assert!(app.verification.open);
            assert!(app.verification.recovery.open);
            assert!(!app.closing);
            assert!(app.recovery_snapshot().key.is_some());
        }
        let _ = app.update(message(Event::Saved(revision - 1)));
        assert!(app.recovery_snapshot().key.is_some());
        let _ = app.update(message(Event::Saved(revision)));
        assert!(app.recovery_snapshot().key.is_none());
        let _ = app.update(Message::Escape);
        assert!(!app.verification.open);
        assert_eq!(app.selection.room, room);
        assert_eq!(app.drafts[&key].content.text(), "unfinished draft");
        assert_eq!(app.scroll_positions[&key], 144.0);
        assert!(app.model.recovery.control.is_none());
    }

    #[test]
    fn replacement_requires_confirmation_and_clears_entered_secret_on_close() {
        let mut app = demo_app();
        let _ = app.update(Message::Verification(verification::Event::OpenRecovery));
        app.verification.recovery.demo.recovery = RecoveryState::Enabled;
        let revision = app.recovery_snapshot().revision;
        let _ = app.update(message(Event::ConfirmReplace(revision)));
        assert!(app.recovery_snapshot().key.is_none());
        let _ = app.update(message(Event::Replace(revision)));
        assert!(app.verification.recovery.replace);
        let _ = app.update(message(Event::CancelReplace));
        let _ = app.update(message(Event::ConfirmReplace(revision)));
        assert!(app.recovery_snapshot().key.is_none());
        let _ = app.update(message(Event::Input(
            revision,
            Secret::new("sensitive-fixture".into()),
        )));
        assert!(!app.verification.recovery.input.is_empty());
        let _ = app.update(Message::Escape);
        assert!(app.verification.recovery.input.is_empty());
    }

    #[test]
    fn modal_footer_fits_short_windows_during_setup_and_key_display() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Buttons(Vec<iced::Rectangle>);
        impl widget::Operation for Buttons {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, value: &str) {
                if matches!(
                    value,
                    "Close"
                        | "Verify with another device…"
                        | "I've saved my key"
                        | "Replace recovery key"
                        | "Cancel"
                ) {
                    self.0.push(bounds);
                }
            }
        }
        let mut app = demo_app();
        let _ = app.update(Message::Verification(verification::Event::OpenRecovery));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for phase in 0..3 {
            if phase == 1 {
                app.verification.recovery.demo.key = Some(Secret::new("DEMO ONLY".into()));
            } else if phase == 2 {
                app.verification.recovery.demo.key = None;
                app.verification.recovery.replace = true;
            }
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let size = iced::Size::new(640.0, 480.0);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut buttons = Buttons::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut buttons,
            );
            assert!(!buttons.0.is_empty());
            for bounds in buttons.0 {
                assert!(bounds.x >= 16.0 && bounds.x + bounds.width <= size.width - 16.0);
                assert!(bounds.y >= 16.0 && bounds.y + bounds.height <= size.height - 16.0);
            }
        }
    }
}
