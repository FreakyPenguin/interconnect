//! Bounded, session-local conversation history. Spaces remain sidebar filters.
use super::{App, DraftKey, Message, icons, shell, tooltips};
use iced::{
    Element, Task,
    widget::{button, row},
};
use interconnect::drafts::ReadingPosition;
use std::collections::VecDeque;

const CAPACITY: usize = 64;

#[derive(Clone, Debug)]
pub(super) struct Place {
    room: String,
    root: Option<String>,
    events: [Option<String>; 2],
    offsets: [f32; 2],
    anchors: [Option<ReadingPosition>; 2],
}
impl Place {
    fn destination_eq(&self, other: &Self) -> bool {
        self.room == other.room
            && self.root == other.root
            && self.events == other.events
            && self.offsets == other.offsets
            && self.anchors == other.anchors
    }
}

#[derive(Default)]
pub(super) struct State {
    back: VecDeque<Place>,
    forward: VecDeque<Place>,
    pub depth: usize,
}
impl State {
    pub(super) fn record(&mut self, before: Option<Place>, after: Option<Place>) {
        let (Some(before), Some(after)) = (before, after) else {
            return;
        };
        if !before.destination_eq(&after) {
            push(&mut self.back, before);
            self.forward.clear();
        }
    }
}
fn push(stack: &mut VecDeque<Place>, place: Place) {
    if stack.len() == CAPACITY {
        stack.pop_front();
    }
    stack.push_back(place);
}

pub(super) fn tracks(message: &Message) -> bool {
    matches!(
        message,
        Message::Room(_)
            | Message::Thread(_)
            | Message::CloseThread
            | Message::OpenThreadLatest(_)
            | Message::OpenLastThread
            | Message::AdjacentRoom(_)
            | Message::UnreadRoom { .. }
            | Message::SearchOpen
            | Message::QuickRoom(_)
            | Message::Replying(_)
            | Message::OpenMessageLink(_)
            | Message::Membership(_)
            | Message::Pinned(_)
            | Message::Bookmark(_)
            | Message::GlobalSearch(_)
            | Message::Notification(_)
            | Message::FollowReplacement(_)
            | Message::NewChat(_)
            | Message::CreateRoom(_)
            | Message::ContextMenu(_)
            | Message::Escape
            | Message::JumpToNewest { .. }
            | Message::SpaceSettings(_)
            | Message::Members(_)
    )
}

pub(super) fn shortcut(
    key: &iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Option<bool> {
    use iced::keyboard::{Key, Modifiers, key::Named};
    if cfg!(target_os = "macos") {
        if modifiers == Modifiers::LOGO {
            match key.as_ref() {
                Key::Character("[") => Some(true),
                Key::Character("]") => Some(false),
                _ => None,
            }
        } else {
            None
        }
    } else if modifiers == Modifiers::ALT {
        match key {
            Key::Named(Named::ArrowLeft) => Some(true),
            Key::Named(Named::ArrowRight) => Some(false),
            _ => None,
        }
    } else {
        None
    }
}

impl App {
    pub(super) fn history_place(&self) -> Option<Place> {
        let room = self.selection.room.as_ref()?.1.clone();
        let root = self.selection.thread.as_ref().map(|(_, root)| root.clone());
        let keys = [
            DraftKey {
                room: room.clone(),
                root: None,
            },
            DraftKey {
                room: room.clone(),
                root: root.clone(),
            },
        ];
        Some(Place {
            room,
            root,
            events: std::array::from_fn(|index| {
                let selected = if index == 1 {
                    &self.selection.thread
                } else {
                    &self.selection.room
                };
                // A completed jump's focus remains in Selection after scrolling.
                // Once layout has settled, an explicit scroll to newest wins over
                // that stale target. Before the first matching layout, retain the
                // requested event so Back can resume an interrupted jump.
                let newest = self
                    .scroll_positions
                    .get(&keys[index])
                    .is_some_and(|offset| *offset <= 1.0)
                    && self.reading_anchor(index == 1).is_none()
                    && self.pending_scroll[index].is_none()
                    && selected.as_ref().is_some_and(|(generation, _)| {
                        self.timeline_metrics[index].is_some_and(|(view, _, _)| view == *generation)
                    });
                self.selection.event_focus[index]
                    .as_ref()
                    .filter(|_| !newest)
                    .map(|(_, event)| event.clone())
            }),
            offsets: keys
                .each_ref()
                .map(|key| self.scroll_positions.get(key).copied().unwrap_or(0.0)),
            anchors: std::array::from_fn(|index| {
                self.reading_anchor(index == 1).or_else(|| {
                    (self
                        .scroll_positions
                        .get(&keys[index])
                        .copied()
                        .unwrap_or(0.0)
                        > 1.0)
                        .then(|| self.reading_positions.positions.get(&keys[index]).cloned())
                        .flatten()
                })
            }),
        })
    }

    pub(super) fn navigate_history(&mut self, backwards: bool) -> Task<Message> {
        if !self.context_menu_allowed()
            || self.composing
            || self.space_settings.is_open()
            || self.emoji_management.is_some()
            || self.section_dragging()
        {
            return Task::none();
        }
        let Some(current) = self.history_place() else {
            return Task::none();
        };
        let stack = if backwards {
            &mut self.conversation_history.back
        } else {
            &mut self.conversation_history.forward
        };
        let target = loop {
            let Some(place) = stack.pop_back() else {
                return Task::none();
            };
            if self.model.rooms.iter().any(|room| room.id == place.room) {
                break place;
            }
        };
        push(
            if backwards {
                &mut self.conversation_history.forward
            } else {
                &mut self.conversation_history.back
            },
            current,
        );
        self.context_menu = None;
        self.select_room(target.room.clone());
        if let Some(root) = target.root {
            self.select_thread(root);
        }
        for thread in [false, true] {
            let index = usize::from(thread);
            let Some(key) = self.draft_key(thread) else {
                continue;
            };
            self.scroll_positions.insert(key, target.offsets[index]);
            self.pending_scroll[index] = Some(target.offsets[index]);
            if let Some(anchor) = target.anchors[index].clone() {
                self.restore_reading_anchor(thread, anchor);
            } else if let Some(event) = target.events[index].clone() {
                let generation = if thread {
                    self.selection.thread.as_ref().unwrap().0
                } else {
                    self.selection.room.as_ref().unwrap().0
                };
                self.selection.event_focus[index] = Some((generation, event));
                self.pending_scroll[index] = None;
            } else {
                self.selection.event_focus[index] = None;
            }
        }
        self.pending_room_focus = self
            .selection
            .thread
            .is_none()
            .then(|| {
                self.selection
                    .room
                    .as_ref()
                    .map(|(generation, _)| *generation)
            })
            .flatten();
        iced::widget::operation::focus(if self.selection.thread.is_some() {
            "thread-composer"
        } else {
            "room-composer"
        })
    }

    pub(super) fn history_controls(&self) -> Element<'_, Message> {
        let enabled = self.context_menu_allowed() && !self.composing;
        let control = |backwards, available| {
            tooltips::tooltip(
                button(icons::icon(if backwards {
                    icons::Kind::ChevronLeft
                } else {
                    icons::Kind::ChevronRight
                }))
                .keyboard_navigation(true)
                .padding(5)
                .style(shell::icon_button)
                .on_press_maybe(
                    (enabled && available).then_some(Message::ConversationHistory(backwards)),
                ),
                if backwards {
                    super::shortcuts::label("Back (Cmd+[)", "Back (Alt+Left)")
                } else {
                    super::shortcuts::label("Forward (Cmd+])", "Forward (Alt+Right)")
                },
                iced::widget::tooltip::Position::Bottom,
            )
        };
        row![
            control(true, !self.conversation_history.back.is_empty()),
            control(false, !self.conversation_history.forward.is_empty())
        ]
        .spacing(2)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn navigate(app: &mut App, room: &str) {
        let _ = app.update(Message::Room(room.into()));
    }
    fn step(app: &mut App, backwards: bool) {
        let _ = app.update(Message::ConversationHistory(backwards));
    }
    fn room(app: &App) -> &str {
        &app.selection.room.as_ref().unwrap().1
    }

    #[test]
    fn back_forward_preserves_thread_drafts_offsets_and_space_filter() {
        let mut app = demo_app();
        navigate(&mut app, "threads");
        let _ = app.update(Message::Thread("review".into()));
        let room_key = app.draft_key(false).unwrap();
        let thread_key = app.draft_key(true).unwrap();
        app.scroll_positions.insert(room_key.clone(), 250.0);
        app.scroll_positions.insert(thread_key.clone(), 120.0);
        app.drafts.get_mut(&thread_key).unwrap().content =
            iced::widget::text_editor::Content::with_text("keep draft");
        let space = app.shell.space.clone();
        navigate(&mut app, "general");
        step(&mut app, true);
        assert_eq!(room(&app), "threads");
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "review");
        assert_eq!(app.scroll_positions[&room_key], 250.0);
        assert_eq!(app.scroll_positions[&thread_key], 120.0);
        assert_eq!(app.drafts[&thread_key].content.text().trim(), "keep draft");
        assert_eq!(app.shell.space, space);
        assert_eq!(
            app.model.room.as_ref().unwrap().generation,
            app.selection.room.as_ref().unwrap().0
        );
        step(&mut app, false);
        assert_eq!(room(&app), "general");
        assert!(app.selection.thread.is_none());
    }

    #[test]
    fn nested_navigation_records_once_and_new_navigation_discards_forward() {
        let mut app = demo_app();
        app.conversation_history = State::default();
        let _ = app.update(Message::AdjacentRoom(false));
        assert_eq!(app.conversation_history.back.len(), 1);
        step(&mut app, true);
        assert_eq!(app.conversation_history.forward.len(), 1);
        navigate(&mut app, "general");
        assert!(app.conversation_history.forward.is_empty());
        let selected = room(&app).to_owned();
        step(&mut app, false);
        assert_eq!(room(&app), selected);
    }

    #[test]
    fn reply_jumps_in_the_same_room_have_a_return_path() {
        let mut app = demo_app();
        let before = app.history_place().unwrap();
        let generation = app.selection.room.as_ref().unwrap().0;
        app.selection.event_focus[0] = Some((generation, "$original".into()));
        let after = app.history_place();
        app.conversation_history.record(Some(before), after);
        step(&mut app, true);
        assert!(app.selection.event_focus[0].is_none());
        step(&mut app, false);
        assert_eq!(
            app.selection.event_focus[0].as_ref().unwrap().1,
            "$original"
        );
    }

    #[test]
    fn manually_scrolling_to_newest_after_a_jump_returns_to_newest_through_history() {
        for thread in [false, true] {
            let mut app = demo_app();
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let index = usize::from(thread);
            let generation = if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            };
            let key = app.draft_key(thread).unwrap();
            app.focus_message_event(thread, "$jumped-event".into());
            app.scroll_positions.insert(key.clone(), 240.0);
            // The scroll event can precede the next geometry report. A matching
            // generation proves the jump was laid out, not its last translation.
            app.timeline_metrics[index] = Some((
                generation,
                1,
                super::super::timeline::Metrics {
                    height: 400.0,
                    content_height: 1200.0,
                    translation: 560.0,
                },
            ));
            let _ = app.update(Message::Scrolled {
                thread,
                generation,
                offset: 0.0,
            });
            assert!(app.selection.event_focus[index].is_some());
            navigate(&mut app, "general");
            step(&mut app, true);
            assert_eq!(room(&app), "studio");
            assert_eq!(
                app.selection.thread.as_ref().map(|(_, root)| root.as_str()),
                thread.then_some("review")
            );
            assert!(
                app.selection.event_focus[index].is_none(),
                "settled newest position must beat an earlier jump"
            );
            assert_eq!(app.scroll_positions[&key], 0.0);
        }
    }
    #[test]
    fn leaving_a_jump_before_its_first_layout_retains_the_requested_event() {
        for thread in [false, true] {
            let mut app = demo_app();
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let index = usize::from(thread);
            let generation = if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            };
            let key = app.draft_key(thread).unwrap();
            app.scroll_positions.insert(key, 0.0);
            app.focus_message_event(thread, "$pending-event".into());
            // Metrics from the previous selected view cannot make a requested
            // event look like a completed jump followed by scrolling to newest.
            app.timeline_metrics[index] = Some((
                generation.wrapping_sub(1),
                1,
                super::super::timeline::Metrics::default(),
            ));
            navigate(&mut app, "general");
            step(&mut app, true);
            assert_eq!(
                app.selection.event_focus[index]
                    .as_ref()
                    .map(|(_, event)| event.as_str()),
                Some("$pending-event")
            );
            assert!(app.pending_scroll[index].is_none());
        }
    }

    #[test]
    fn unavailable_rooms_are_skipped_and_dialogs_do_not_consume_history() {
        let mut app = demo_app();
        navigate(&mut app, "general");
        navigate(&mut app, "threads");
        let count = app.conversation_history.back.len();
        let _ = app.update(Message::Membership(super::super::membership::Event::Join));
        step(&mut app, true);
        assert_eq!(app.conversation_history.back.len(), count);
        assert_eq!(room(&app), "threads");
        let _ = app.update(Message::Membership(super::super::membership::Event::Close));
        std::sync::Arc::make_mut(&mut app.model.rooms).retain(|room| room.id != "general");
        step(&mut app, true);
        assert_eq!(room(&app), "studio");
    }

    #[test]
    fn history_is_bounded_and_account_changes_clear_both_stacks() {
        let mut app = demo_app();
        for index in 0..100 {
            navigate(&mut app, if index % 2 == 0 { "general" } else { "threads" });
        }
        assert_eq!(app.conversation_history.back.len(), CAPACITY);
        step(&mut app, true);
        let mut model = app.model.clone();
        model.account_id = Some("@other:example.org".into());
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.conversation_history.back.is_empty());
        assert!(app.conversation_history.forward.is_empty());
    }

    #[test]
    fn navigating_away_during_startup_restore_keeps_the_event_offset() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let position = ReadingPosition {
            room: key.room.clone(),
            root: None,
            event: "$older".into(),
            offset: -27,
        };
        app.restore_reading_anchor(false, position.clone());
        assert!(!app.scroll_positions.contains_key(&key));
        navigate(&mut app, "general");
        step(&mut app, true);
        assert_eq!(app.reading_anchor(false), Some(position));
    }

    #[test]
    fn menu_navigation_escape_and_jump_to_newest_record_actual_dispatches() {
        let mut app = demo_app();
        let target = super::super::context_menu::Target::Room("general".into());
        let _ = app.update(Message::ContextMenu(
            super::super::context_menu::Event::Open(target.clone(), iced::Point::ORIGIN),
        ));
        let _ = app.update(Message::ContextMenu(
            super::super::context_menu::Event::Choose(target, "Open room".into()),
        ));
        assert_eq!(room(&app), "general");
        step(&mut app, true);
        assert_eq!(room(&app), "studio");
        step(&mut app, false);
        let _ = app.update(Message::Thread("review".into()));
        let _ = app.update(Message::Escape);
        assert!(app.selection.thread.is_none());
        step(&mut app, true);
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "review");
        let _ = app.update(Message::CloseThread);
        let key = app.draft_key(false).unwrap();
        app.scroll_positions.insert(key.clone(), 200.0);
        let generation = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::JumpToNewest {
            thread: false,
            generation,
        });
        assert_eq!(app.scroll_positions[&key], 0.0);
        step(&mut app, true);
        assert_eq!(app.scroll_positions[&key], 200.0);
    }

    #[test]
    fn signing_out_resets_history_without_underflowing_update_depth() {
        let mut app = demo_app();
        navigate(&mut app, "general");
        let mut model = app.model.clone();
        model.sign_out = interconnect::connected::SignOut::Complete;
        let _ = app.update(Message::State(Box::new(model)));
        assert_eq!(app.conversation_history.depth, 0);
        assert!(app.conversation_history.back.is_empty());
        assert!(app.conversation_history.forward.is_empty());
    }

    #[test]
    fn shortcuts_leave_word_navigation_alone() {
        use iced::keyboard::{Key, Modifiers, key::Named};
        if cfg!(target_os = "macos") {
            assert_eq!(
                shortcut(&Key::Character("[".into()), Modifiers::LOGO),
                Some(true)
            );
            assert_eq!(
                shortcut(&Key::Character("]".into()), Modifiers::LOGO),
                Some(false)
            );
            assert_eq!(
                shortcut(&Key::Named(Named::ArrowLeft), Modifiers::ALT),
                None
            );
        } else {
            assert_eq!(
                shortcut(&Key::Named(Named::ArrowLeft), Modifiers::ALT),
                Some(true)
            );
            assert_eq!(
                shortcut(&Key::Named(Named::ArrowRight), Modifiers::ALT),
                Some(false)
            );
        }
        assert_eq!(
            shortcut(&Key::Named(Named::ArrowLeft), Modifiers::CTRL),
            None
        );
        assert_eq!(
            shortcut(
                &Key::Named(Named::ArrowLeft),
                Modifiers::ALT | Modifiers::SHIFT
            ),
            None
        );
    }
}
