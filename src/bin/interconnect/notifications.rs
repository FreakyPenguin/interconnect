//! Native and inbox activation share guarded, account-bound navigation.
use super::{App, Message};
use iced::Task;
use interconnect::notifications::Target;

impl App {
    pub(super) fn notification_target_available(&self, target: &Target) -> bool {
        if self.closing || self.stopped || self.selection.sign_out != 0 {
            return false;
        }
        match target {
            Target::Conversation {
                room,
                thread,
                event,
            } => {
                self.model.rooms.iter().any(|entry| &entry.id == room)
                    && thread.as_ref().is_none_or(|thread| {
                        self.demo.is_some()
                            || thread.parse::<matrix_sdk::ruma::OwnedEventId>().is_ok()
                    })
                    && event.as_ref().is_none_or(|event| {
                        self.demo.is_some()
                            || event.parse::<matrix_sdk::ruma::OwnedEventId>().is_ok()
                    })
            }
            Target::Invitation { room } => {
                self.model.invitations.iter().any(|entry| &entry.id == room)
            }
            Target::Verification { flow, device } => {
                (self.demo.is_some() || self.model.verification.control.is_some())
                    && self
                        .model
                        .verification
                        .incoming
                        .as_ref()
                        .is_some_and(|incoming| {
                            &incoming.flow == flow && &incoming.device == device
                        })
            }
        }
    }
    pub(super) fn activate_notification(&mut self) -> Task<Message> {
        let Some(activation) = self.pending_notification.as_ref() else {
            return Task::none();
        };
        if !activation.is_active() || self.closing || self.stopped || self.selection.sign_out != 0 {
            self.pending_notification = None;
            return Task::none();
        }
        // Native clicks defer until the covered form can safely relinquish input.
        if !self.context_menu_allowed()
            || self.space_settings.is_open()
            || self.emoji_management.is_some()
            || self.composing
        {
            return Task::none();
        }
        let activation = self.pending_notification.take().unwrap();
        if !self.notification_target_available(&activation.target) {
            return Task::none();
        }
        let navigate = self.update(Message::Notification(activation.target.clone()));
        let focus = iced::window::latest().then(|window| {
            window.map_or_else(Task::none, |id| {
                iced::window::minimize(id, false).chain(iced::window::gain_focus(id))
            })
        });
        Task::batch([navigate, focus])
    }
    pub(super) fn notification_task(&mut self, target: &Target) -> Task<Message> {
        if !self.context_menu_allowed()
            || !self.notification_target_available(target)
            || self.composing
        {
            return Task::none();
        }
        self.notification_center.close();
        self.stop_image_playback();
        self.mark_notification_target_read(target);
        match target {
            Target::Conversation { .. } => {
                self.open_notification(target);
                Task::none()
            }
            Target::Invitation { room } => {
                let task = self.membership_event(super::membership::Event::Open);
                if self.membership.open {
                    self.membership.focus_invitation = Some(room.clone());
                }
                task
            }
            Target::Verification { .. } => {
                self.verification_update(super::verification::Event::Open)
            }
        }
    }
    pub(super) fn open_notification(&mut self, target: &Target) -> bool {
        let Target::Conversation {
            room,
            thread,
            event,
        } = target
        else {
            return false;
        };
        if !self.model.rooms.iter().any(|entry| &entry.id == room) {
            return false;
        }
        self.notification_center.close();
        self.stop_typing();
        self.context_menu = None;
        self.shell.spaces.menu = None;
        self.shell.narrow_navigation = false;
        self.bookmarks.close();
        self.global_search.close();
        self.room_search = Default::default();
        let same_room = self
            .selection
            .room
            .as_ref()
            .is_some_and(|(_, selected)| selected == room);
        if !same_room || (event.is_some() && thread.is_none()) {
            self.select_room(room.clone());
        }
        if let Some(root) = thread {
            if event.is_some()
                || self
                    .selection
                    .thread
                    .as_ref()
                    .is_none_or(|(_, open)| open != root)
            {
                self.select_thread(root.clone());
            }
        } else if self.selection.thread.take().is_some() {
            self.selection.event_focus[1] = None;
            self.visible_rows[1].clear();
            self.pending_scroll[1] = None;
            self.pending_scroll[0] = Some(self.saved_scroll(false));
        }
        if let Some(event) = event {
            self.focus_message_event(thread.is_some(), event.clone());
        }
        if thread.is_none() {
            self.pending_room_focus = self
                .selection
                .room
                .as_ref()
                .map(|(generation, _)| *generation);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor;

    #[test]
    fn opens_room_and_thread_without_losing_drafts_scroll_or_space() {
        let mut app = super::super::tests::demo_app();
        let scope = app.shell.space.clone();
        let original = app.draft_key(false).unwrap();
        app.drafts.get_mut(&original).unwrap().content =
            text_editor::Content::with_text("unfinished");
        app.scroll_positions.insert(original.clone(), 210.0);
        let target = Target::Conversation {
            room: "threads".into(),
            thread: Some("thread-plan".into()),
            event: None,
        };
        assert!(app.open_notification(&target));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "threads");
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "thread-plan");
        let key = app.draft_key(true).unwrap();
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::with_text("thread draft");
        app.scroll_positions.insert(key.clone(), 90.0);
        let generation = app.generation;
        assert!(app.open_notification(&target));
        assert_eq!(
            app.generation, generation,
            "the same conversation keeps its timeline"
        );
        assert_eq!(app.saved_scroll(true), 90.0);
        assert!(app.open_notification(&Target::Conversation {
            room: "studio".into(),
            thread: None,
            event: None,
        }));
        assert!(app.selection.thread.is_none());
        assert_eq!(app.saved_scroll(false), 210.0);
        assert_eq!(app.drafts[&original].content.text(), "unfinished");
        assert_eq!(app.drafts[&key].content.text(), "thread draft");
        assert_eq!(app.shell.space, scope);
        assert!(app.open_notification(&target));
        assert_eq!(app.pending_scroll[1], Some(90.0));
        let selected = app.selection.clone();
        assert!(!app.open_notification(&Target::Conversation {
            room: "left-room".into(),
            thread: None,
            event: None,
        }));
        assert_eq!(app.selection, selected);
    }
}

#[cfg(test)]
mod activation_tests {
    use super::*;
    use interconnect::notifications::Activation;
    use std::sync::Arc;

    #[test]
    fn state_click_waits_for_modal_close_and_is_consumed_once() {
        let mut app = super::super::tests::demo_app();
        let session = Arc::new(());
        let target = Target::Conversation {
            room: "threads".into(),
            thread: Some("thread-plan".into()),
            event: None,
        };
        let activation = Arc::new(Activation::new(target.clone(), &session));
        let (control, backend) = tokio::sync::watch::channel(app.selection.clone());
        app.control = Some(control);
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        let mut state = app.model.clone();
        state.notification_activation = Some(activation.clone());
        let _ = app.update(Message::State(Box::new(state)));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
        assert!(app.pending_notification.is_some());
        // No SDK event or timer is needed after closing a modal.
        let _ = app.update(Message::Settings(super::super::settings::Event::Cancel));
        assert_eq!(
            backend.borrow().room.as_ref().unwrap().1,
            match &target {
                Target::Conversation { room, .. } => room.as_str(),
                _ => unreachable!(),
            }
        );
        assert_eq!(backend.borrow().thread.as_ref().unwrap().1, "thread-plan");
        assert!(app.pending_notification.is_none());
        let _ = app.update(Message::Room("studio".into()));
        let state = app.model.clone();
        let _ = app.update(Message::State(Box::new(state)));
        assert_eq!(
            app.selection.room.as_ref().unwrap().1,
            "studio",
            "unrelated snapshots cannot replay a click"
        );
        app.pending_notification = Some(activation);
        drop(session);
        let _ = app.update(Message::WindowFocused(true));
        assert!(app.pending_notification.is_none());
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
    }
}

#[cfg(test)]
mod category_tests {
    use super::*;
    use crate::{notification_center, tests::demo_app};
    use interconnect::notifications::Activation;
    use std::sync::Arc;

    fn app() -> App {
        let mut app = demo_app();
        app.model.invitations = vec![interconnect::room_membership::Invitation {
            id: "!invitation:example.org".into(),
            name: "Example invitation".into(),
            inviter: Some("@other:example.org".into()),
            space: false,
        }]
        .into();
        app
    }
    #[test]
    fn invitation_click_reviews_that_invite_without_accepting_or_switching_conversation() {
        let mut app = app();
        let selected = app.selection.clone();
        let invitations = app.model.invitations.len();
        let target = Target::Invitation {
            room: "!invitation:example.org".into(),
        };
        let _ = app.update(Message::NotificationCenter(
            notification_center::Event::Open,
        ));
        let _ = app.update(Message::Notification(target));
        assert!(app.membership.open);
        assert_eq!(
            app.membership.focus_invitation.as_deref(),
            Some("!invitation:example.org")
        );
        assert!(!app.notification_center.open);
        assert_eq!(app.model.invitations.len(), invitations);
        assert_eq!(app.selection.room, selected.room);
        assert_eq!(app.selection.thread, selected.thread);
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn verification_click_reviews_only_the_current_request_without_accepting_it() {
        let mut app = demo_app();
        app.model.verification.incoming = Some(interconnect::verification::Incoming {
            flow: "flow".into(),
            device: "PHONE".into(),
        });
        let target = Target::Verification {
            flow: "flow".into(),
            device: "PHONE".into(),
        };
        let original = app.model.verification.revision;
        let _ = app.update(Message::Notification(target));
        assert!(app.verification.open);
        assert_eq!(app.model.verification.revision, original);
        assert_eq!(
            app.model.verification.incoming.as_ref().unwrap().flow,
            "flow"
        );
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::Verification(
            super::super::verification::Event::Close,
        ));
        let _ = app.update(Message::Notification(Target::Verification {
            flow: "expired".into(),
            device: "PHONE".into(),
        }));
        assert!(!app.verification.open);
    }
    #[test]
    fn native_invitation_and_verification_clicks_defer_and_expire_with_the_session() {
        for verification in [false, true] {
            let mut app = app();
            app.model.verification.incoming = Some(interconnect::verification::Incoming {
                flow: "flow".into(),
                device: "PHONE".into(),
            });
            let target = if verification {
                Target::Verification {
                    flow: "flow".into(),
                    device: "PHONE".into(),
                }
            } else {
                Target::Invitation {
                    room: "!invitation:example.org".into(),
                }
            };
            let session = Arc::new(());
            let _ = app.update(Message::Settings(super::super::settings::Event::Open));
            let mut model = app.model.clone();
            model.notification_activation = Some(Arc::new(Activation::new(target, &session)));
            let _ = app.update(Message::State(Box::new(model)));
            assert!(app.pending_notification.is_some());
            assert!(!app.membership.open && !app.verification.open);
            let _ = app.update(Message::Settings(super::super::settings::Event::Cancel));
            assert!(app.pending_notification.is_none());
            assert_eq!(app.membership.open, !verification);
            assert_eq!(app.verification.open, verification);
            assert_eq!(app.pending_ops, 0);
        }
    }
}
