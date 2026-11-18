//! Pin or unpin a captured message, with one write per room in flight and scoped feedback.
use super::{App, MAX_PENDING_OPERATIONS, Message, reaction_picker::Target, shell};
use iced::{
    Element, Task,
    widget::{column, text},
};
use interconnect::connected::MessageRow;

#[derive(Clone, Debug)]
pub enum Event {
    Set(Target, bool),
    Saved(Target, bool, Result<(), String>),
}
fn message(event: Event) -> Message {
    Message::Pin(event)
}
impl App {
    fn pin_available(&self, target: &Target) -> bool {
        !self.closing
            && !self.stopped
            && self.pending_ops < MAX_PENDING_OPERATIONS
            && !self
                .pinning
                .iter()
                .any(|pending| pending.room == target.room)
            && self
                .model
                .rooms
                .iter()
                .any(|room| room.id == target.room && room.can_pin)
    }
    pub(super) fn pin_action(
        &self,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Message> {
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        if selected.as_ref()?.0 != generation {
            return None;
        }
        let target = Target {
            room: self.selection.room.as_ref()?.1.clone(),
            thread,
            generation,
            event: row.event_id.clone()?,
        };
        let pinned = !self
            .model
            .rooms
            .iter()
            .any(|room| room.id == target.room && room.pinned_events.contains(&target.event));
        (self.pin_available(&target) && (!pinned || row.can_react))
            .then(|| message(Event::Set(target, pinned)))
    }
    pub(super) fn pinned_unpin_action(&self, row: &MessageRow, generation: u64) -> Option<Message> {
        let (current, room) = self.selection.room.as_ref()?;
        if *current != generation {
            return None;
        }
        let target = Target {
            room: room.clone(),
            thread: false,
            generation,
            event: row.event_id.clone()?,
        };
        (self.pin_available(&target)
            && (self.demo.is_some() || self.reaction_composer(&target).is_some()))
        .then(|| message(Event::Set(target, false)))
    }
    fn pin_target_row(&self, target: &Target) -> Option<&MessageRow> {
        self.message_row(target).or_else(|| {
            let (generation, room) = self.selection.room.as_ref()?;
            if target.thread || *generation != target.generation || room != &target.room {
                return None;
            }
            self.model
                .pinned
                .as_ref()
                .filter(|pins| pins.generation == target.generation)?
                .rows
                .iter()
                .find(|row| row.event_id.as_ref() == Some(&target.event))
        })
    }
    pub(super) fn validate_pin_error(&mut self) {
        if self
            .pin_error
            .as_ref()
            .is_some_and(|(target, _)| self.pin_target_row(target).is_none())
        {
            self.pin_error = None;
        }
    }
    pub(super) fn pin_feedback<'a>(
        &'a self,
        body: Element<'a, Message>,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Element<'a, Message> {
        if let Some((target, error)) = &self.pin_error
            && target.thread == thread
            && target.generation == generation
            && row.event_id.as_ref() == Some(&target.event)
        {
            column![body, text(error).size(12).style(shell::muted_text)]
                .spacing(4)
                .into()
        } else {
            body
        }
    }
    pub(super) fn pin_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Set(target, pinned) => {
                if !self.pin_available(&target)
                    || !self
                        .pin_target_row(&target)
                        .is_some_and(|row| !pinned || row.can_react)
                    || self.settings.draft.is_some()
                    || self.verification.open
                    || self.image_view.is_some()
                    || self.reaction_picker.is_some()
                    || self.composer_picker.is_some()
                {
                    return Task::none();
                }
                self.pin_error = None;
                if self.demo.is_some() {
                    let room = std::sync::Arc::make_mut(&mut self.model.rooms)
                        .iter_mut()
                        .find(|room| room.id == target.room)
                        .unwrap();
                    if pinned {
                        if !room.pinned_events.contains(&target.event) {
                            room.pinned_events.push(target.event);
                        }
                    } else {
                        room.pinned_events.retain(|event| event != &target.event);
                    }
                    self.refresh_demo();
                } else if let Some(composer) = self.reaction_composer(&target) {
                    self.pinning.insert(target.clone());
                    self.pending_ops += 1;
                    return Task::perform(
                        composer.set_message_pinned(target.event.clone(), pinned),
                        move |result| message(Event::Saved(target.clone(), pinned, result)),
                    );
                }
            }
            Event::Saved(target, pinned, result) => {
                if !self.pinning.remove(&target) {
                    return Task::none();
                }
                self.pending_ops -= 1;
                if !self.closing
                    && !self.stopped
                    && self.pin_target_row(&target).is_some()
                    && let Err(error) = result
                {
                    self.pin_error = Some((
                        target,
                        format!(
                            "Could not {} message: {error}",
                            if pinned { "pin" } else { "unpin" }
                        ),
                    ));
                }
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn action(app: &App, thread: bool, key: &str) -> Option<Message> {
        let generation = if thread {
            app.selection.thread.as_ref()?.0
        } else {
            app.selection.room.as_ref()?.0
        };
        let row = if thread {
            app.model.thread.as_ref()?
        } else {
            app.model.room.as_ref()?
        }
        .rows
        .iter()
        .find(|row| row.key == key)?;
        app.pin_action(row, thread, generation)
    }
    #[test]
    fn hover_panel_pin_button_is_after_reply_and_click_pins_the_message() {
        use iced::advanced::{Layout, Shell, layout, mouse, widget};
        #[derive(Default)]
        struct Probe {
            pin: Option<iced::Rectangle>,
            unpin: Option<iced::Rectangle>,
            reply: Option<iced::Rectangle>,
            reaction: Option<iced::Rectangle>,
            translation: iced::Vector,
        }
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
                    self.translation = translation;
                }
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                for (name, dest) in [
                    ("pin-message", &mut self.pin),
                    ("unpin-message", &mut self.unpin),
                    ("reply-in-thread", &mut self.reply),
                    ("add-reaction", &mut self.reaction),
                ] {
                    if id == Some(&widget::Id::new(name)) {
                        *dest = Some(bounds - self.translation);
                    }
                }
            }
        }
        let mut app = demo_app();
        let row = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.key == "review")
            .unwrap()
            .clone();
        app.model.room.as_mut().unwrap().rows = std::sync::Arc::new(vec![row]);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = widget::Tree::empty();
        let size = iced::Size::new(360.0, 640.0);
        let mut view = app.pane(false);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        assert!(probe.reaction.unwrap().x < probe.reply.unwrap().x);
        assert!(probe.reply.unwrap().x < probe.pin.unwrap().x);
        let mut messages = Vec::new();
        for event in [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(probe.pin.unwrap().center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        drop(view);
        let pin = messages
            .into_iter()
            .find(|message| matches!(message, Message::Pin(Event::Set(_, true))))
            .expect("Pin button dispatches the captured message");
        let _ = app.update(pin);
        assert!(
            app.model
                .pinned
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.key == "review")
        );
        let review = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.key == "review")
            .unwrap()
            .clone();
        app.model.room.as_mut().unwrap().rows = std::sync::Arc::new(vec![review]);
        let mut view = app.pane(false);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        assert!(probe.pin.is_none());
        let mut messages = Vec::new();
        for event in [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(probe.unpin.unwrap().center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        drop(view);
        let unpin = messages
            .into_iter()
            .find(|message| matches!(message, Message::Pin(Event::Set(_, false))))
            .expect("Unpin button dispatches removal rather than a toggle");
        let _ = app.update(unpin);
        assert!(
            !app.model
                .pinned
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.key == "review")
        );
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .can_pin = false;
        let mut view = app.pane(false);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        assert!(probe.pin.is_none() && probe.unpin.is_none());
        assert!(probe.reply.is_some() && probe.reaction.is_some());
    }

    #[test]
    fn demo_pin_and_unpin_require_permission_and_reject_stale_targets() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 70.0);
        let pin = action(&app, false, "review").unwrap();
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .can_pin = false;
        assert!(action(&app, false, "review").is_none());
        let _ = app.update(pin.clone());
        assert_eq!(app.model.pinned.as_ref().unwrap().total, 2);
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .can_pin = true;
        let _ = app.update(pin.clone());
        assert_eq!(app.model.pinned.as_ref().unwrap().total, 3);
        assert!(
            app.model
                .pinned
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.key == "review")
        );
        assert!(matches!(
            action(&app, false, "review"),
            Some(Message::Pin(Event::Set(_, false)))
        ));
        let _ = app.update(pin);
        assert_eq!(app.model.pinned.as_ref().unwrap().total, 3);
        let unpin = action(&app, false, "review").unwrap();
        assert!(matches!(unpin, Message::Pin(Event::Set(_, false))));
        let _ = app.update(unpin.clone());
        let _ = app.update(unpin);
        assert_eq!(app.model.pinned.as_ref().unwrap().total, 2);
        assert!(matches!(
            action(&app, false, "review"),
            Some(Message::Pin(Event::Set(_, true)))
        ));
        assert_eq!(app.drafts[&key].content.text(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 70.0);
        let old = action(&app, false, "welcome").unwrap();
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(old);
        assert!(app.model.pinned.is_none());
        let _ = app.update(Message::Room("release".into()));
        assert!(action(&app, false, "release").is_none());
    }
    #[test]
    fn panel_unpin_feedback_survives_missing_history_and_rejects_old_views() {
        let mut app = demo_app();
        let generation = app.selection.room.as_ref().unwrap().0;
        let row = app.model.pinned.as_ref().unwrap().rows[0].clone();
        app.model.room.as_mut().unwrap().rows = std::sync::Arc::default();
        let Message::Pin(Event::Set(target, false)) =
            app.pinned_unpin_action(&row, generation).unwrap()
        else {
            panic!()
        };
        assert!(app.pinned_unpin_action(&row, generation + 1).is_none());
        app.pinning.insert(target.clone());
        app.pending_ops = 1;
        assert!(app.pinned_unpin_action(&row, generation).is_none());
        let _ = app.update(message(Event::Saved(
            target.clone(),
            false,
            Err("Denied".into()),
        )));
        assert!(
            app.pin_error
                .as_ref()
                .unwrap()
                .1
                .contains("Could not unpin")
        );
        assert!(app.pinned_unpin_action(&row, generation).is_some());
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(message(Event::Set(target, false)));
        assert!(app.pin_error.is_none());
        assert!(
            app.model
                .rooms
                .iter()
                .find(|room| room.id == "studio")
                .unwrap()
                .pinned_events
                .contains(&row.key)
        );
    }

    #[test]
    fn a_redacted_pinned_message_can_still_be_unpinned() {
        let mut app = demo_app();
        let row = std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
            .iter_mut()
            .find(|row| row.key == "direction")
            .unwrap();
        row.can_react = false;
        row.body = "Message removed".into();
        let unpin = action(&app, false, "direction").unwrap();
        assert!(matches!(unpin, Message::Pin(Event::Set(_, false))));
        let _ = app.update(unpin);
        assert!(
            !app.model
                .rooms
                .iter()
                .find(|room| room.id == "studio")
                .unwrap()
                .pinned_events
                .contains(&"direction".to_owned())
        );
    }
    #[test]
    fn pinning_thread_messages_updates_room_pins() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let original = app.thread_original().unwrap();
        assert!(
            app.pin_action(original, true, app.selection.thread.as_ref().unwrap().0)
                .is_some()
        );
        let pin = action(&app, true, "review-reply-1").unwrap();
        let _ = app.update(pin);
        assert!(
            app.model
                .pinned
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.key == "review-reply-1")
        );
        let unpin = action(&app, true, "review-reply-1").unwrap();
        assert!(matches!(unpin, Message::Pin(Event::Set(_, false))));
        let _ = app.update(unpin.clone());
        let _ = app.update(unpin);
        assert!(
            !app.model
                .pinned
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.key == "review-reply-1")
        );
    }
    #[test]
    fn pending_sends_serialize_per_room_and_late_failures_do_not_follow_navigation() {
        let mut app = demo_app();
        let Message::Pin(Event::Set(target, true)) = action(&app, false, "review").unwrap() else {
            panic!()
        };
        app.pinning.insert(target.clone());
        app.pending_ops = 1;
        assert!(action(&app, false, "welcome").is_none());
        let _ = app.update(message(Event::Saved(
            target.clone(),
            true,
            Err("Denied".into()),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(app.pin_error.is_some());
        app.pinning.insert(target.clone());
        app.pending_ops = 1;
        let _ = app.update(Message::Room("general".into()));
        assert!(app.pin_error.is_none());
        let _ = app.update(message(Event::Saved(
            target.clone(),
            true,
            Err("Late".into()),
        )));
        assert!(app.pin_error.is_none());
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Saved(target, true, Ok(()))));
        assert_eq!(app.pending_ops, 0);
        let Message::Pin(Event::Set(target, true)) = action(&app, false, "hello").unwrap() else {
            panic!()
        };
        app.pinning.insert(target.clone());
        app.pending_ops = 1;
        app.demo = None;
        let _ = app.update(Message::Close);
        assert!(app.closing && !app.selection.shutdown);
        let _ = app.update(message(Event::Saved(target, true, Err("Offline".into()))));
        assert!(app.selection.shutdown);
        assert_eq!(app.pending_ops, 0);
        assert!(app.pin_error.is_none());
    }
}
