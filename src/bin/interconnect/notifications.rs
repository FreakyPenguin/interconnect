//! Native activation uses the same draft and scroll ownership as room navigation.
use super::{App, Message};
use iced::Task;
use interconnect::notifications::Target;

impl App {
    pub(super) fn activate_notification(&mut self) -> Task<Message> {
        let Some(activation) = self.pending_notification.as_ref() else {
            return Task::none();
        };
        if !activation.is_active() || self.closing || self.stopped || self.selection.sign_out != 0 {
            self.pending_notification = None;
            return Task::none();
        }
        // Preserve modal edits and pending writes. Apply after the dialog closes.
        if !self.context_menu_allowed() {
            return Task::none();
        }
        let activation = self.pending_notification.take().unwrap();
        if !self
            .model
            .rooms
            .iter()
            .any(|room| room.id == activation.target.room)
        {
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

    pub(super) fn open_notification(&mut self, target: &Target) -> bool {
        if !self.model.rooms.iter().any(|room| room.id == target.room) {
            return false;
        }
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
            .is_some_and(|(_, room)| room == &target.room);
        if !same_room {
            self.select_room(target.room.clone());
        }
        if let Some(root) = &target.thread {
            if self
                .selection
                .thread
                .as_ref()
                .is_none_or(|(_, thread)| thread != root)
            {
                self.select_thread(root.clone());
            }
        } else if self.selection.thread.take().is_some() {
            self.selection.event_focus[1] = None;
            self.visible_rows[1].clear();
            self.pending_scroll[1] = None;
            self.pending_scroll[0] = Some(self.saved_scroll(false));
        }
        if target.thread.is_none() {
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
        let target = Target {
            room: "threads".into(),
            thread: Some("thread-plan".into()),
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
        assert!(app.open_notification(&Target {
            room: "studio".into(),
            thread: None
        }));
        assert!(app.selection.thread.is_none());
        assert_eq!(app.saved_scroll(false), 210.0);
        assert_eq!(app.drafts[&original].content.text(), "unfinished");
        assert_eq!(app.drafts[&key].content.text(), "thread draft");
        assert_eq!(app.shell.space, scope);
        assert!(app.open_notification(&target));
        assert_eq!(app.pending_scroll[1], Some(90.0));
        let selected = app.selection.clone();
        assert!(!app.open_notification(&Target {
            room: "left-room".into(),
            thread: None
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
        let target = Target {
            room: "threads".into(),
            thread: Some("thread-plan".into()),
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
        assert_eq!(backend.borrow().room.as_ref().unwrap().1, target.room);
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
