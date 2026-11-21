//! Explicit session-owned sign-out; no credentials or SDK handles reach the UI.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, text},
};
use interconnect::connected::{Model, SignOut};

#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Recovery,
    Confirm,
    Cancel,
    Ignore,
}

impl App {
    pub(super) fn sign_out_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Recovery
                if self.sign_out_open
                    && matches!(
                        self.model.sign_out,
                        SignOut::Idle | SignOut::Failed { revoked: false, .. }
                    )
                    && (self.demo.is_some() || self.model.recovery.control.is_some()) =>
            {
                let _ = self.sign_out_event(Event::Cancel);
                return self.update(Message::Verification(
                    super::verification::Event::OpenRecovery,
                ));
            }
            Event::Open if self.context_menu_allowed() && self.pending_ops == 0 => {
                self.context_menu = None;
                let save = self.save_drafts();
                self.sign_out_open = true;
                self.stop_typing();
                self.publish_notification_context();
                return Task::batch([iced::widget::operation::focus("sign-out-dialog"), save]);
            }
            Event::Confirm
                if self.sign_out_open
                    && self.pending_ops == 0
                    && !self.closing
                    && !self.draft_persistence.blocks_sign_out()
                    && !matches!(self.model.sign_out, SignOut::Running) =>
            {
                if self.demo.is_some() {
                    self.sign_out_open = false;
                    return Task::none();
                }
                self.generation += 1;
                self.selection.sign_out = self.generation;
                self.model.sign_out = SignOut::Running;
                self.publish_notification_context();
            }
            Event::Cancel
                if self.sign_out_open
                    && matches!(
                        self.model.sign_out,
                        SignOut::Idle | SignOut::Failed { revoked: false, .. }
                    ) =>
            {
                self.sign_out_open = false;
                self.selection.sign_out = 0;
                self.publish_notification_context();
                return iced::widget::operation::focus("room-composer");
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn signed_out(&mut self, state: Model) {
        // Destroy every account-scoped view, draft, media cache and pending picker.
        // Keep only application settings, the profile and the session command channel.
        let mut clean = Self::new();
        clean.profile = self.profile.clone();
        clean.settings = std::mem::take(&mut self.settings);
        clean.activation = std::mem::take(&mut self.activation);
        if let Some(handle) = &clean.activation.handle {
            handle.clear();
            handle.update(None, "Not signed in", !self.closing);
        }
        clean.settings.profile.reset_account();
        clean.login.sender = self.login.sender.take();
        clean.login.homeserver = std::mem::take(&mut self.login.homeserver);
        clean.control = self.control.take();
        clean.generation = self.generation + 1;
        clean.closing = self.closing;
        clean.selection.shutdown = self.closing;
        clean.model = state;
        if let Some(control) = &clean.control {
            control.send_replace(clean.selection.clone());
        }
        *self = clean;
    }

    pub(super) fn sign_out_view(&self) -> Element<'_, Message> {
        let modal = iced::widget::responsive(|size| {
            let busy = matches!(self.model.sign_out, SignOut::Running);
            let revoked = matches!(self.model.sign_out, SignOut::Failed { revoked: true, .. });
            let available = !busy && !revoked;
            let mut content = column![
                text("This signs out this device. You may need another verified device or your recovery key to read encrypted history after signing in again.").size(13),
                text("Unsent drafts on this device will be removed after sign-out.").size(13),
            ].spacing(14).width(Length::Fill);
            if self.demo.is_some() {
                content = content.push(
                    text("Demo only · no account is signed out.")
                        .size(12)
                        .style(shell::muted_text),
                );
            }
            let recovery_available = self.demo.is_some() || self.model.recovery.control.is_some();
            if available && recovery_available {
                if self.model.recovery.recovery
                    != matrix_sdk::encryption::recovery::RecoveryState::Enabled
                {
                    content = content.push(container(column![
                        shell::title("Check your recovery access", 14),
                        text("Recovery is not ready on this device. Set it up or check your existing key before signing out.").size(13),
                    ].spacing(8)).padding(12).width(Length::Fill)
                        .style(|theme| shell::surface(theme, shell::palette(theme).error_surface)));
                }
                content = content.push(
                    button(text("Check key backup & recovery…").size(13))
                        .keyboard_navigation(true)
                        .padding([7, 10])
                        .on_press(Message::SignOut(Event::Recovery))
                        .style(shell::quiet_button),
                );
            }
            let mut footer = column![].spacing(8).width(Length::Fill);
            if let Some(error) = &self.draft_persistence.error {
                footer =
                    footer
                        .push(text(error).size(12).style(shell::danger_text))
                        .push(super::tooltips::icon_button(
                            super::icons::Kind::Refresh,
                            "Retry saving drafts before sign out",
                            self.draft_persistence.can_retry().then_some(
                                Message::DraftPersistence(super::draft_persistence::Event::Retry),
                            ),
                        ));
            }
            if let SignOut::Failed { error, .. } = &self.model.sign_out {
                footer = footer.push(text(error).size(13).style(shell::danger_text));
            }
            let cancel = super::tooltips::icon_button(
                super::icons::Kind::Close,
                "Cancel sign out",
                available.then_some(Message::SignOut(Event::Cancel)),
            );
            let confirm = button(
                text(if busy {
                    "Signing out…"
                } else if revoked {
                    "Retry cleanup"
                } else {
                    "Sign out"
                })
                .size(13),
            )
            .keyboard_navigation(true)
            .padding([7, 14])
            .style(button::danger)
            .on_press_maybe(
                (!busy
                    && self.pending_ops == 0
                    && !self.closing
                    && !self.draft_persistence.blocks_sign_out())
                .then_some(Message::SignOut(Event::Confirm)),
            );
            footer = footer
                .push(row![iced::widget::space().width(Length::Fill), cancel, confirm].spacing(8));
            let header = row![
                shell::title("Sign out of Interconnect?", 20),
                iced::widget::space().width(Length::Fill),
                button(super::icons::icon(super::icons::Kind::Close))
                    .keyboard_navigation(true)
                    .padding(6)
                    .style(shell::icon_button)
                    .on_press_maybe(available.then_some(Message::SignOut(Event::Cancel))),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            let preferred_height = if recovery_available { 500.0 } else { 380.0 };
            super::dialog::modal(
                super::dialog::frame(
                    size,
                    560.0,
                    preferred_height,
                    "sign-out",
                    header,
                    content,
                    footer,
                ),
                Message::SignOut(Event::Ignore),
            )
        });
        super::focus_scope::dismissible(modal, "sign_out-focus", Message::Escape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    #[test]
    fn sign_out_waits_for_writes_blocks_input_and_cancel_keeps_drafts() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unfinished");
        app.pending_ops = 1;
        let _ = app.update(Message::SignOut(Event::Open));
        assert!(!app.sign_out_open);
        app.pending_ops = 0;
        let _ = app.update(Message::SignOut(Event::Open));
        assert!(app.sign_out_open);
        let room = app.selection.room.clone();
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(Message::Send(key.clone()));
        assert_eq!(app.selection.room, room);
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::SignOut(Event::Cancel));
        assert!(!app.sign_out_open);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "unfinished");
    }

    #[test]
    fn retry_and_server_revocation_keep_the_correct_controls() {
        let mut app = demo_app();
        app.demo = None;
        app.model.authentication = None;
        let _ = app.update(Message::SignOut(Event::Open));
        let _ = app.update(Message::SignOut(Event::Confirm));
        let request = app.selection.sign_out;
        assert!(request > 0);
        let _ = app.update(Message::SignOut(Event::Cancel));
        assert!(app.sign_out_open);
        app.model.sign_out = SignOut::Failed {
            error: "cleanup failed".into(),
            revoked: true,
        };
        let _ = app.update(Message::SignOut(Event::Cancel));
        assert!(app.sign_out_open);
        let _ = app.update(Message::SignOut(Event::Confirm));
        assert!(app.selection.sign_out > request);
        app.model.sign_out = SignOut::Failed {
            error: "offline".into(),
            revoked: false,
        };
        let _ = app.update(Message::SignOut(Event::Cancel));
        assert!(!app.sign_out_open);
        assert_eq!(app.selection.sign_out, 0);
    }

    #[test]
    fn success_clears_private_ui_state_and_returns_to_login() {
        let mut app = demo_app();
        app.account_id = Some("@old:example.org".into());
        app.profile = "/profile".into();
        app.login.username = "old".into();
        let (control, received) = tokio::sync::watch::channel(app.selection.clone());
        app.control = Some(control);
        let _ = app.update(Message::State(Box::new(Model {
            sign_out: SignOut::Complete,
            authentication: Some(interconnect::connected::Authentication::SignIn { error: None }),
            ..Model::default()
        })));
        assert!(app.drafts.is_empty());
        assert!(app.scroll_positions.is_empty());
        assert!(app.account_id.is_none());
        assert!(app.login.username.is_empty());
        assert_eq!(app.profile, "/profile");
        assert_eq!(*received.borrow(), Default::default());
        assert!(!app.stopped);
    }
}
