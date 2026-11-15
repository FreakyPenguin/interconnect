use super::{App, DraftKey};
use interconnect::connected::TypingActivity;

impl App {
    pub(super) fn stop_typing(&mut self) {
        if self.selection.typing.take().is_some()
            && let Some(control) = &self.control
        {
            control.send_if_modified(|selection| selection.typing.take().is_some());
        }
    }

    pub(super) fn typing_edited(&mut self, key: &DraftKey) {
        if self.demo.is_some() || self.closing || self.stopped || !self.window_focused {
            return;
        }
        if self.draft_key(key.root.is_some()).as_ref() != Some(key) {
            return;
        }
        if self
            .drafts
            .get(key)
            .is_none_or(|draft| draft.pending || draft.content.text().trim().is_empty())
        {
            self.stop_typing();
            return;
        }
        if let Some((generation, _)) = &self.selection.room {
            let activity = TypingActivity {
                generation: *generation,
                at: std::time::Instant::now(),
            };
            self.selection.typing = Some(activity.clone());
            if let Some(control) = &self.control {
                control.send_modify(|selection| selection.typing = Some(activity));
            }
        }
    }

    pub(super) fn typing_label(&self) -> String {
        let Some(typing) = &self.model.typing else {
            return String::new();
        };
        if self
            .selection
            .room
            .as_ref()
            .is_none_or(|(generation, _)| *generation != typing.generation)
        {
            return String::new();
        }
        match typing.names.as_slice() {
            [] => String::new(),
            [name] => format!("{name} is typing…"),
            [first, second] => format!("{first} and {second} are typing…"),
            names => format!(
                "{} and {} are typing…",
                names[..names.len() - 1].join(", "),
                names.last().unwrap()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Message;
    use iced::widget::text_editor::{Action, Edit};

    #[test]
    fn typing_tracks_edits_and_stops_without_disturbing_drafts_or_space_selection() {
        let mut app = crate::tests::demo_app();
        app.demo = None;
        app.window_focused = true;
        let key = app.draft_key(false).unwrap();
        let (control, receiver) = tokio::sync::watch::channel(app.selection.clone());
        app.control = Some(control);
        let _ = app.update(Message::Edit(key.clone(), Action::Edit(Edit::Insert('x'))));
        assert!(receiver.borrow().typing.is_some());
        let activity = app.selection.typing.clone();
        let _ = app.update(Message::Edit(key.clone(), Action::SelectAll));
        assert_eq!(app.selection.typing, activity);
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.selection.typing, activity);
        let _ = app.update(Message::WindowFocused(false));
        assert!(receiver.borrow().typing.is_none());
        assert_eq!(app.drafts[&key].content.text(), "x");
    }

    #[test]
    fn thread_edits_notify_the_room_and_empty_text_send_and_navigation_stop() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("$sample-root".into()));
        app.demo = None;
        app.window_focused = true;
        let key = app.draft_key(true).unwrap();
        let room_generation = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::Edit(key.clone(), Action::Edit(Edit::Insert('x'))));
        assert_eq!(
            app.selection.typing.as_ref().unwrap().generation,
            room_generation
        );
        let _ = app.update(Message::Edit(key.clone(), Action::SelectAll));
        let _ = app.update(Message::Edit(key.clone(), Action::Edit(Edit::Backspace)));
        assert!(app.selection.typing.is_none());
        let _ = app.update(Message::Edit(key.clone(), Action::Edit(Edit::Insert('y'))));
        assert!(app.selection.typing.is_some());
        let _ = app.update(Message::Send(key.clone()));
        assert!(app.selection.typing.is_none());
        // No SDK composer is installed in this fixture, so the send retains text.
        assert_eq!(app.drafts[&key].content.text(), "y");
        let _ = app.update(Message::Edit(key.clone(), Action::Edit(Edit::Insert('z'))));
        assert!(app.selection.typing.is_some());
        let _ = app.update(Message::Room("general".into()));
        assert!(app.selection.typing.is_none());
        assert_eq!(app.drafts[&key].content.text(), "yz");
    }

    #[test]
    fn demo_and_restored_drafts_never_publish_outgoing_activity() {
        let mut app = crate::tests::demo_app();
        app.window_focused = true;
        let key = app.draft_key(false).unwrap();
        let _ = app.update(Message::Edit(key.clone(), Action::Edit(Edit::Insert('x'))));
        assert!(app.selection.typing.is_none());
        assert!(!app.typing_label().is_empty());
        app.demo = None;
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(Message::Room(key.room.clone()));
        assert_eq!(app.drafts[&key].content.text(), "x");
        assert!(app.selection.typing.is_none());
    }

    #[test]
    fn incoming_labels_reject_old_rooms_and_distinguish_multiple_people() {
        let mut app = crate::tests::demo_app();
        let generation = app.selection.room.as_ref().unwrap().0;
        for (names, expected) in [
            (vec![], ""),
            (vec!["Alex"], "Alex is typing…"),
            (vec!["Alex", "Sam"], "Alex and Sam are typing…"),
            (vec!["Alex", "Sam", "Jo"], "Alex, Sam and Jo are typing…"),
        ] {
            app.model.typing = Some(interconnect::connected::Typing {
                generation,
                names: names.into_iter().map(str::to_owned).collect(),
            });
            assert_eq!(app.typing_label(), expected);
        }
        app.model.typing.as_mut().unwrap().generation += 1;
        assert!(app.typing_label().is_empty());
    }
}
