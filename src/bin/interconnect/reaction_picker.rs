//! Bind a reusable picker to a captured conversation/event; send off the UI thread.
use super::{App, MAX_PENDING_OPERATIONS, Message, current_pane, emoji_picker, shell};
use iced::{
    Element, Task,
    widget::{column, text},
};
use interconnect::connected::{Composer, MessageRow};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub room: String,
    pub thread: bool,
    pub generation: u64,
    pub event: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sending {
    room: String,
    event: String,
    key: String,
}
pub struct State {
    target: Target,
    picker: emoji_picker::State,
    sending: Option<String>,
    error: Option<String>,
}
impl State {
    pub fn dismiss(&self) -> Message {
        message(Event::Picker {
            target: self.target.clone(),
            event: emoji_picker::Event::Close,
        })
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(Target),
    Chip {
        target: Target,
        key: String,
        selected: bool,
    },
    ChipSent {
        target: Target,
        key: String,
        result: Result<(), String>,
    },
    Picker {
        target: Target,
        event: emoji_picker::Event,
    },
    Sent {
        target: Target,
        key: String,
        result: Result<(), String>,
    },
}
fn message(event: Event) -> Message {
    Message::Reaction(event)
}
fn focus_composer(thread: bool) -> Task<Message> {
    iced::widget::operation::focus(if thread {
        "thread-composer"
    } else {
        "room-composer"
    })
}

impl App {
    pub(super) fn reaction_chips<'a>(
        &'a self,
        row: &'a MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Element<'a, Message>> {
        let target = match self.reaction_action(row, thread, generation) {
            Some(Message::Reaction(Event::Open(target))) => Some(target),
            _ => None,
        };
        let badges = super::reactions::view(&row.reactions, |reaction| {
            let target = target.as_ref()?;
            if self.reaction_sends.contains(&Sending {
                room: target.room.clone(),
                event: target.event.clone(),
                key: reaction.key.clone(),
            }) {
                return None;
            }
            Some(message(Event::Chip {
                target: target.clone(),
                key: reaction.key.clone(),
                selected: !reaction.is_own,
            }))
        });
        let error = self
            .reaction_error
            .as_ref()
            .filter(|(failed, _)| target.as_ref() == Some(failed));
        match (badges, error) {
            (None, None) => None,
            (Some(badges), None) => Some(badges),
            (badges, Some((_, error))) => {
                let mut content = column![].spacing(4);
                if let Some(badges) = badges {
                    content = content.push(badges);
                }
                Some(content.push(text(error).size(12)).into())
            }
        }
    }
    pub(super) fn reaction_action(
        &self,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Message> {
        if !row.can_react
            || self.closing
            || self.stopped
            || self.pending_ops >= MAX_PENDING_OPERATIONS
        {
            return None;
        }
        let (_, room) = self.selection.room.as_ref()?;
        Some(message(Event::Open(Target {
            room: room.clone(),
            thread,
            generation,
            event: row.event_id.clone()?,
        })))
    }
    fn reaction_row(&self, target: &Target) -> Option<&MessageRow> {
        if self.selection.room.as_ref()?.1 != target.room {
            return None;
        }
        let selected = if target.thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        if selected.as_ref()?.0 != target.generation {
            return None;
        }
        if target.thread
            && let Some(row) = self
                .thread_original_in_room()
                .filter(|row| row.event_id.as_ref() == Some(&target.event))
        {
            return row.can_react.then_some(row);
        }
        let pane = if target.thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        current_pane(pane, target.generation)?
            .rows
            .iter()
            .find(|row| row.event_id.as_ref() == Some(&target.event))
            .or_else(|| {
                target
                    .thread
                    .then(|| self.thread_original())
                    .flatten()
                    .filter(|row| row.event_id.as_ref() == Some(&target.event))
            })
            .filter(|row| row.can_react)
    }
    fn reaction_composer(&self, target: &Target) -> Option<Composer> {
        // Root reactions live in the room timeline when the root is loaded there.
        if target.thread
            && let Some(row) = self.thread_original_in_room()
            && row.event_id.as_ref() == Some(&target.event)
            && let Some((generation, _)) = &self.selection.room
        {
            return current_pane(&self.model.room, *generation)?
                .composer
                .clone();
        }
        let pane = if target.thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        current_pane(pane, target.generation)?.composer.clone()
    }
    pub(super) fn validate_reaction_picker(&mut self) {
        if self
            .reaction_error
            .as_ref()
            .is_some_and(|(target, _)| self.reaction_row(target).is_none())
        {
            self.reaction_error = None;
        }
        if self.reaction_picker.as_ref().is_some_and(|state| {
            self.closing
                || self.stopped
                || self.reaction_row(&state.target).is_none()
                || (!state.target.thread
                    && self
                        .window_width
                        .is_some_and(|width| self.thread_replaces_room(width)))
        }) {
            self.reaction_picker = None;
        }
    }
    pub(super) fn reaction_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Chip {
                target,
                key,
                selected,
            } if !self.closing
                && !self.stopped
                && self.pending_ops < MAX_PENDING_OPERATIONS
                && self.settings.draft.is_none()
                && self.image_view.is_none()
                && self.composer_picker.is_none()
                && self.reaction_picker.is_none() =>
            {
                if !self
                    .reaction_row(&target)
                    .is_some_and(|row| row.reactions.iter().any(|reaction| reaction.key == key))
                {
                    return Task::none();
                }
                let sending = Sending {
                    room: target.room.clone(),
                    event: target.event.clone(),
                    key: key.clone(),
                };
                if self.reaction_sends.contains(&sending) {
                    return Task::none();
                }
                self.reaction_error = None;
                if let Some(demo) = &mut self.demo {
                    demo.set_reaction(&target.room, &target.event, &key, selected);
                    self.refresh_demo();
                } else if let Some(composer) = self.reaction_composer(&target) {
                    self.reaction_sends.insert(sending);
                    self.pending_ops += 1;
                    return Task::perform(
                        composer.set_reaction(target.event.clone(), key.clone(), selected),
                        move |result| {
                            message(Event::ChipSent {
                                target: target.clone(),
                                key: key.clone(),
                                result,
                            })
                        },
                    );
                } else {
                    self.reaction_error = Some((
                        target,
                        "Conversation is not ready. Click the reaction to retry shortly.".into(),
                    ));
                }
            }
            Event::ChipSent {
                target,
                key,
                result,
            } => {
                if !self.reaction_sends.remove(&Sending {
                    room: target.room.clone(),
                    event: target.event.clone(),
                    key,
                }) {
                    return Task::none();
                }
                self.pending_ops -= 1;
                if !self.closing
                    && !self.stopped
                    && self.reaction_row(&target).is_some()
                    && let Err(error) = result
                {
                    self.reaction_error = Some((
                        target,
                        format!("Could not update reaction: {error}. Click the reaction to retry."),
                    ));
                }
            }
            Event::Open(target)
                if !self.closing
                    && !self.stopped
                    && self.settings.draft.is_none()
                    && self.image_view.is_none()
                    && self.composer_picker.is_none()
                    && self.reaction_picker.is_none() =>
            {
                if self.reaction_row(&target).is_some() {
                    self.reaction_picker = Some(State {
                        target,
                        picker: emoji_picker::State::new(emoji_picker::unicode()),
                        sending: None,
                        error: None,
                    });
                    return iced::widget::operation::focus(emoji_picker::SEARCH_ID);
                }
            }
            Event::Picker {
                target,
                event: emoji_picker::Event::Close,
            } if self
                .reaction_picker
                .as_ref()
                .is_some_and(|state| state.target == target) =>
            {
                if let Some(state) = self.reaction_picker.take() {
                    return focus_composer(state.target.thread);
                }
            }
            Event::Picker { target, event }
                if !self.closing
                    && !self.stopped
                    && self.pending_ops < MAX_PENDING_OPERATIONS
                    && self
                        .reaction_picker
                        .as_ref()
                        .is_some_and(|state| state.target == target) =>
            {
                let Some(state) = &mut self.reaction_picker else {
                    return Task::none();
                };
                if state.sending.is_some() {
                    return Task::none();
                }
                if self.composing && matches!(event, emoji_picker::Event::Select(_)) {
                    return Task::none();
                }
                let Some(key) = state.picker.update(emoji_picker::unicode(), event) else {
                    return Task::none();
                };
                let target = state.target.clone();
                let Some(row) = self.reaction_row(&target) else {
                    self.reaction_picker = None;
                    return Task::none();
                };
                if row
                    .reactions
                    .iter()
                    .any(|reaction| reaction.key == key && reaction.is_own)
                {
                    self.reaction_picker = None;
                    return focus_composer(target.thread);
                }
                if let Some(demo) = &mut self.demo {
                    demo.add_reaction(&target.room, &target.event, &key);
                    self.refresh_demo();
                    self.reaction_picker = None;
                    return focus_composer(target.thread);
                }
                let sending = Sending {
                    room: target.room.clone(),
                    event: target.event.clone(),
                    key: key.clone(),
                };
                if self.reaction_sends.contains(&sending) {
                    self.reaction_picker.as_mut().unwrap().error =
                        Some("This reaction is already being sent".into());
                    return Task::none();
                }
                if let Some(composer) = self.reaction_composer(&target) {
                    self.reaction_sends.insert(sending);
                    self.pending_ops += 1;
                    let state = self.reaction_picker.as_mut().unwrap();
                    state.sending = Some(key.clone());
                    state.error = None;
                    return Task::perform(
                        composer.add_reaction(target.event.clone(), key.clone()),
                        move |result| {
                            message(Event::Sent {
                                target: target.clone(),
                                key: key.clone(),
                                result,
                            })
                        },
                    );
                }
                self.reaction_picker.as_mut().unwrap().error =
                    Some("Conversation is not ready. Try again shortly.".into());
            }
            Event::Sent {
                target,
                key,
                result,
            } => {
                if !self.reaction_sends.remove(&Sending {
                    room: target.room.clone(),
                    event: target.event.clone(),
                    key: key.clone(),
                }) {
                    return Task::none();
                }
                self.pending_ops -= 1;
                if let Some(state) = &mut self.reaction_picker
                    && state.target == target
                    && state.sending.as_ref() == Some(&key)
                {
                    state.sending = None;
                    match result {
                        Ok(()) => {
                            self.reaction_picker = None;
                            return focus_composer(target.thread);
                        }
                        Err(error) => {
                            state.error = Some(format!(
                                "Could not add reaction: {error}. Choose an emoji to retry."
                            ))
                        }
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub(super) fn reaction_popup(
        &self,
        thread: bool,
        generation: u64,
        event: &str,
    ) -> Option<(Element<'_, Message>, Message)> {
        let state = self.reaction_picker.as_ref().filter(|state| {
            state.target.thread == thread
                && state.target.generation == generation
                && state.target.event == event
        })?;
        let mut content = column![emoji_picker::view(
            &state.picker,
            emoji_picker::unicode(),
            state.sending.is_none(),
            |event| message(Event::Picker {
                target: state.target.clone(),
                event
            })
        )]
        .spacing(8);
        if state.sending.is_some() {
            content = content.push(text("Sending reaction…").size(12).color(shell::MUTED));
        }
        if let Some(error) = &state.error {
            content = content.push(text(error).size(12));
        }
        Some((
            emoji_picker::panel(content),
            message(Event::Picker {
                target: state.target.clone(),
                event: emoji_picker::Event::Close,
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn open(app: &mut App, thread: bool, event: &str) -> Target {
        let generation = if thread {
            app.selection.thread.as_ref().unwrap().0
        } else {
            app.selection.room.as_ref().unwrap().0
        };
        let target = Target {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            thread,
            generation,
            event: event.into(),
        };
        let _ = app.update(message(Event::Open(target.clone())));
        assert!(app.reaction_picker.is_some());
        target
    }
    fn pick(app: &mut App, target: &Target, key: &str) {
        let _ = app.update(message(Event::Picker {
            target: target.clone(),
            event: emoji_picker::Event::Select(key.into()),
        }));
    }
    fn pending(app: &mut App, target: &Target, key: &str) {
        app.reaction_picker.as_mut().unwrap().sending = Some(key.into());
        app.reaction_sends.insert(Sending {
            room: target.room.clone(),
            event: target.event.clone(),
            key: key.into(),
        });
        app.pending_ops += 1;
    }

    #[test]
    fn chip_clicks_toggle_room_thread_original_and_reply_without_changing_drafts() {
        use iced::advanced::{
            Layout, Shell, layout, mouse,
            widget::{self, Tree},
        };
        struct Probe {
            key: String,
            bounds: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::from(format!("reaction-chip:{}", self.key))) {
                    self.bounds = Some(bounds);
                }
            }
        }
        fn click(app: &mut App, thread: bool, event: &str, key: &str) -> Message {
            let generation = if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            };
            let row = if thread && event == "review" {
                app.thread_original_in_room().unwrap()
            } else {
                let pane = if thread {
                    &app.model.thread
                } else {
                    &app.model.room
                };
                pane.as_ref()
                    .unwrap()
                    .rows
                    .iter()
                    .find(|row| row.key == event)
                    .unwrap()
            };
            let mut chips = app.reaction_chips(row, thread, generation).unwrap();
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
            let mut tree = Tree::new(&chips);
            let size = iced::Size::new(300.0, 200.0);
            let node = chips.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
            );
            let mut probe = Probe {
                key: key.into(),
                bounds: None,
            };
            chips
                .as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            let cursor = mouse::Cursor::Available(probe.bounds.unwrap().center());
            let mut messages = Vec::new();
            for event in [
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ] {
                let mut shell = Shell::new(&mut messages);
                chips.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                assert!(!matches!(
                    shell.redraw_request(),
                    iced::window::RedrawRequest::At(_)
                ));
            }
            assert_eq!(messages.len(), 1);
            messages.remove(0)
        }
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        for thread in [false, true] {
            let key = app.draft_key(thread).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("keep this draft");
        }
        let selection = app.selection.clone();
        let scroll = app.scroll_positions.clone();
        for thread in [false, true] {
            for (count, own) in [(3, true), (2, false)] {
                let message = click(&mut app, thread, "review", "👩🏽‍💻");
                let replay = message.clone();
                let _ = app.update(message);
                if !own {
                    let _ = app.update(replay);
                }
                for row in [
                    app.thread_original().unwrap(),
                    app.thread_original_in_room().unwrap(),
                ] {
                    let reaction = row
                        .reactions
                        .iter()
                        .find(|reaction| reaction.key == "👩🏽‍💻")
                        .unwrap();
                    assert_eq!((reaction.count, reaction.is_own), (count, own));
                }
            }
        }
        let message = click(&mut app, true, "review", "👍");
        let replay = message.clone();
        let _ = app.update(message);
        let _ = app.update(replay);
        assert!(
            !app.thread_original()
                .unwrap()
                .reactions
                .iter()
                .any(|reaction| reaction.key == "👍"),
            "last own reaction disappears and a stale remove cannot add it back"
        );
        for (count, own) in [(2, true), (1, false)] {
            let message = click(&mut app, true, "review-reply-1", "❤️");
            let _ = app.update(message);
            let reaction = app
                .model
                .thread
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.key == "review-reply-1")
                .unwrap()
                .reactions
                .iter()
                .find(|reaction| reaction.key == "❤️")
                .unwrap();
            assert_eq!((reaction.count, reaction.is_own), (count, own));
        }
        assert_eq!(app.selection, selection);
        assert_eq!(app.scroll_positions, scroll);
        for thread in [false, true] {
            assert_eq!(
                app.drafts[&app.draft_key(thread).unwrap()].content.text(),
                "keep this draft"
            );
        }
    }

    #[test]
    fn chip_sends_reject_duplicates_and_stale_errors_and_finish_before_shutdown() {
        let mut app = crate::tests::demo_app();
        let target = open(&mut app, false, "review");
        let _ = app.update(Message::Escape);
        let sending = Sending {
            room: target.room.clone(),
            event: target.event.clone(),
            key: "👍".into(),
        };
        app.reaction_sends.insert(sending.clone());
        app.pending_ops = 1;
        let _ = app.update(message(Event::Chip {
            target: target.clone(),
            key: "👍".into(),
            selected: false,
        }));
        assert_eq!(app.pending_ops, 1);
        assert!(
            app.model
                .room
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.key == "review")
                .unwrap()
                .reactions
                .iter()
                .any(|reaction| reaction.key == "👍" && reaction.is_own)
        );
        let _ = app.update(message(Event::ChipSent {
            target: target.clone(),
            key: "👍".into(),
            result: Err("denied".into()),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.reaction_error.as_ref().unwrap().1.contains("denied"));
        let _ = app.update(message(Event::Chip {
            target: target.clone(),
            key: "👍".into(),
            selected: false,
        }));
        assert!(app.reaction_error.is_none(), "retry clears the prior error");
        app.reaction_sends.insert(sending.clone());
        app.pending_ops = 1;
        let _ = app.update(Message::Room("home".into()));
        let _ = app.update(message(Event::ChipSent {
            target: target.clone(),
            key: "👍".into(),
            result: Err("old error".into()),
        }));
        assert!(app.reaction_error.is_none());
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Chip {
            target: target.clone(),
            key: "👍".into(),
            selected: false,
        }));
        assert_eq!(app.pending_ops, 0, "stale target cannot send");
        app.reaction_sends.insert(sending);
        app.pending_ops = 1;
        app.demo = None;
        let _ = app.update(Message::Close);
        assert!(!app.selection.shutdown);
        let _ = app.update(message(Event::ChipSent {
            target,
            key: "👍".into(),
            result: Ok(()),
        }));
        assert!(app.selection.shutdown);
        assert_eq!(app.pending_ops, 0);
    }

    #[test]
    fn demo_adds_once_to_room_thread_original_and_reply_without_touching_drafts() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let room_key = app.draft_key(false).unwrap();
        let thread_key = app.draft_key(true).unwrap();
        for key in [&room_key, &thread_key] {
            app.drafts.get_mut(key).unwrap().content =
                iced::widget::text_editor::Content::with_text("keep this draft");
        }
        let scroll = app.scroll_positions.clone();
        let target = open(&mut app, true, "review");
        pick(&mut app, &target, "👩🏽‍💻");
        assert!(app.reaction_picker.is_none());
        for row in [
            app.thread_original().unwrap(),
            app.thread_original_in_room().unwrap(),
        ] {
            let reaction = row
                .reactions
                .iter()
                .find(|reaction| reaction.key == "👩🏽‍💻")
                .unwrap();
            assert!(reaction.is_own);
            assert_eq!(reaction.count, 3);
        }
        let target = open(&mut app, false, "review");
        pick(&mut app, &target, "👩🏽‍💻");
        assert_eq!(
            app.thread_original_in_room()
                .unwrap()
                .reactions
                .iter()
                .find(|reaction| reaction.key == "👩🏽‍💻")
                .unwrap()
                .count,
            3
        );
        let target = open(&mut app, true, "review-reply-1");
        pick(&mut app, &target, "🚀");
        let reply = app
            .model
            .thread
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.key == "review-reply-1")
            .unwrap();
        assert!(
            reply
                .reactions
                .iter()
                .any(|reaction| reaction.key == "🚀" && reaction.count == 1 && reaction.is_own)
        );
        for key in [&room_key, &thread_key] {
            assert_eq!(app.drafts[key].content.text(), "keep this draft");
        }
        assert_eq!(app.scroll_positions, scroll);
        assert_eq!(app.pending_ops, 0);
    }

    #[test]
    fn picker_rejects_stale_events_and_removed_targets_and_blocks_background_actions() {
        let mut app = crate::tests::demo_app();
        let old = open(&mut app, false, "review");
        let room = app.selection.room.clone();
        let _ = app.update(Message::Room("home".into()));
        assert_eq!(
            app.selection.room, room,
            "background shortcuts cannot switch the target"
        );
        let _ = app.update(Message::Escape);
        let _ = app.update(Message::Room("home".into()));
        let current = open(&mut app, false, "weekend");
        pick(&mut app, &old, "🚀");
        let _ = app.update(message(Event::Picker {
            target: old,
            event: emoji_picker::Event::Close,
        }));
        assert_eq!(app.reaction_picker.as_ref().unwrap().target, current);
        let mut model = app.model.clone();
        Arc::make_mut(&mut model.room.as_mut().unwrap().rows)
            .iter_mut()
            .find(|row| row.key == "weekend")
            .unwrap()
            .can_react = false;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.reaction_picker.is_none());
        let _ = app.update(message(Event::Open(current)));
        assert!(app.reaction_picker.is_none());
    }

    #[test]
    fn popup_escape_has_no_second_global_shortcut_to_close_the_thread() {
        use iced::advanced::subscription::{self, Event as RuntimeEvent};
        use iced::futures::{FutureExt, StreamExt};
        fn has_global_escape(app: &App) -> bool {
            subscription::into_recipes(app.subscription())
                .into_iter()
                .any(|recipe| {
                    let input = RuntimeEvent::Interaction {
                        window: iced::window::Id::unique(),
                        status: iced::event::Status::Captured,
                        event: iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                            modified_key: iced::keyboard::Key::Named(
                                iced::keyboard::key::Named::Escape,
                            ),
                            physical_key: iced::keyboard::key::Physical::Unidentified(
                                iced::keyboard::key::NativeCode::Unidentified,
                            ),
                            location: iced::keyboard::Location::Standard,
                            modifiers: iced::keyboard::Modifiers::default(),
                            text: None,
                            repeat: false,
                        }),
                    };
                    matches!(
                        recipe
                            .stream(Box::pin(iced::futures::stream::iter([input])))
                            .next()
                            .now_or_never(),
                        Some(Some(Message::Escape))
                    )
                })
        }
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        assert!(has_global_escape(&app));
        let _ = open(&mut app, true, "review");
        assert!(!has_global_escape(&app));
        let dismiss = app.reaction_picker.as_ref().unwrap().dismiss();
        let _ = app.update(dismiss);
        assert!(app.reaction_picker.is_none());
        assert!(app.selection.thread.is_some());
    }

    #[test]
    fn resizing_away_from_the_anchor_closes_the_picker_and_ime_cannot_submit() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let _ = app.update(Message::WindowResized(1400.0));
        let _ = open(&mut app, false, "review");
        let _ = app.update(Message::WindowResized(640.0));
        assert!(
            app.reaction_picker.is_none(),
            "narrow layout hides the room anchor"
        );
        let target = open(&mut app, true, "review");
        let _ = app.update(Message::Composing(true));
        pick(&mut app, &target, "🔥");
        assert!(app.reaction_picker.is_some());
        let _ = app.update(Message::Composing(false));
        pick(&mut app, &target, "🔥");
        assert!(app.reaction_picker.is_none());
    }

    #[test]
    fn failures_allow_retry_and_stale_completions_do_not_change_a_new_picker_or_shutdown() {
        let mut app = crate::tests::demo_app();
        let old = open(&mut app, false, "review");
        pending(&mut app, &old, "🚀");
        pick(&mut app, &old, "🔥");
        assert_eq!(app.pending_ops, 1, "busy picker rejects a second selection");
        let _ = app.update(message(Event::Sent {
            target: old.clone(),
            key: "🚀".into(),
            result: Err("denied".into()),
        }));
        let state = app.reaction_picker.as_ref().unwrap();
        assert!(state.sending.is_none());
        assert!(state.error.as_ref().unwrap().contains("denied"));
        pending(&mut app, &old, "🚀");
        let _ = app.update(Message::Escape);
        let _ = app.update(Message::Room("home".into()));
        let current = open(&mut app, false, "weekend");
        let _ = app.update(message(Event::Sent {
            target: old,
            key: "🚀".into(),
            result: Ok(()),
        }));
        assert_eq!(app.reaction_picker.as_ref().unwrap().target, current);
        assert!(app.reaction_picker.as_ref().unwrap().error.is_none());
        assert_eq!(app.pending_ops, 0);
        pending(&mut app, &current, "🔥");
        app.demo = None;
        let _ = app.update(Message::Close);
        assert!(app.closing && !app.selection.shutdown);
        assert!(app.reaction_picker.is_none());
        let _ = app.update(message(Event::Sent {
            target: current.clone(),
            key: "🔥".into(),
            result: Err("offline".into()),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
        let _ = app.update(message(Event::Sent {
            target: current,
            key: "🔥".into(),
            result: Ok(()),
        }));
        assert_eq!(app.pending_ops, 0, "duplicate completion is harmless");
    }

    #[test]
    fn picker_opens_over_thread_original_without_moving_off_the_action_button() {
        use iced::advanced::{
            Layout, Shell, layout, mouse,
            widget::{self, Tree},
        };
        #[derive(Default)]
        struct Probe {
            add: Option<iced::Rectangle>,
            choice: bool,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("add-reaction")) && self.add.is_none() {
                    self.add = Some(bounds);
                }
                if id == Some(&widget::Id::new("emoji-choice:🔥")) {
                    self.choice = true;
                }
            }
        }
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::empty();
        let size = iced::Size::new(360.0, 640.0);
        let mut view = app.pane(true);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        // Hover before clicking: this also opens the timestamp tooltip beneath
        // the pinned original's action panel at the same coordinates.
        let position = probe.add.expect("pinned original action").center();
        let cursor = mouse::Cursor::Available(position);
        let mut messages = Vec::new();
        for event in [
            iced::Event::Mouse(mouse::Event::CursorMoved { position }),
            iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        drop(view);
        let open = messages.into_iter().find(|message| matches!(message, Message::Reaction(Event::Open(target)) if target.event == "review")).expect("click opens original's picker");
        let _ = app.update(open);
        let mut view = app.pane(true);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        // No cursor motion or synthetic mouse-leave between click and overlay.
        let mut overlay = view
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &iced::Rectangle::with_size(size),
                iced::Vector::ZERO,
            )
            .expect("picker overlay");
        let overlay = overlay.as_overlay_mut();
        let node = overlay.layout(&renderer, size);
        let mut probe = Probe::default();
        overlay.operate(Layout::new(&node), &renderer, &mut probe);
        assert!(
            probe.choice,
            "picker choices must be visible while the pointer stays on Add reaction"
        );
    }

    #[test]
    fn hover_button_opens_an_interactive_popup_and_selects_into_its_message() {
        use iced::advanced::{
            Layout, Shell, layout, mouse,
            widget::{self, Tree},
        };
        #[derive(Default)]
        struct Probe {
            add: Option<iced::Rectangle>,
            reply: Option<iced::Rectangle>,
            choice: Option<iced::Rectangle>,
            translation: iced::Vector,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
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
                for (name, destination) in [
                    ("add-reaction", &mut self.add),
                    ("reply-in-thread", &mut self.reply),
                    ("emoji-choice:🔥", &mut self.choice),
                ] {
                    if id == Some(&widget::Id::new(name)) {
                        *destination = Some(bounds - self.translation);
                    }
                }
            }
        }
        let mut app = crate::tests::demo_app();
        let pane = app.model.room.as_mut().unwrap();
        let row = pane
            .rows
            .iter()
            .find(|row| row.key == "review")
            .unwrap()
            .clone();
        pane.rows = Arc::new(vec![row]);
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(
                include_bytes!("../../../assets/fonts/Inter-Regular.otf")
                    .as_slice()
                    .into(),
            );
        let renderer = crate::test_renderer(iced::Font::with_name("Inter"), iced::Pixels(14.0));
        let mut tree = Tree::empty();
        let size = iced::Size::new(360.0, 640.0);
        let mut view = app.pane(false);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        assert!(
            probe.add.unwrap().x < probe.reply.unwrap().x,
            "the buttons share the hover panel"
        );
        let mut messages = Vec::new();
        let cursor = mouse::Cursor::Available(probe.add.unwrap().center());
        for event in [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        drop(view);
        let open = messages
            .into_iter()
            .find(|message| matches!(message, Message::Reaction(Event::Open(_))))
            .expect("button opens picker");
        let _ = app.update(open);
        let mut view = app.pane(false);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut overlay_element = view
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &iced::Rectangle::with_size(size),
                iced::Vector::ZERO,
            )
            .expect("picker is an overlay");
        let overlay = overlay_element.as_overlay_mut();
        let node = overlay.layout(&renderer, size);
        let mut probe = Probe::default();
        overlay.operate(Layout::new(&node), &renderer, &mut probe);
        let mut messages = Vec::new();
        let cursor = mouse::Cursor::Available(probe.choice.expect("visible emoji choice").center());
        for event in [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            overlay.update(
                &event,
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
            );
        }
        drop(overlay_element);
        drop(view);
        for event in messages {
            let _ = app.update(event);
        }
        assert!(app.reaction_picker.is_none());
        assert!(
            app.model
                .room
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.key == "review")
                .unwrap()
                .reactions
                .iter()
                .any(|reaction| reaction.key == "🔥" && reaction.is_own)
        );
    }
}
