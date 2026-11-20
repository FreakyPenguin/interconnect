//! Event-driven reading anchors, persisted by the encrypted draft writer.
use super::{App, DraftKey, current_pane};
use interconnect::drafts::ReadingPosition;
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    pub positions: HashMap<DraftKey, ReadingPosition>,
    active: [Option<(u64, ReadingPosition)>; 2],
}
impl State {
    pub fn from_saved(saved: &[ReadingPosition]) -> Self {
        Self {
            positions: saved
                .iter()
                .map(|position| {
                    (
                        DraftKey {
                            room: position.room.clone(),
                            root: position.root.clone(),
                        },
                        position.clone(),
                    )
                })
                .collect(),
            ..Default::default()
        }
    }
    pub fn snapshot(&self) -> Vec<ReadingPosition> {
        let mut positions: Vec<_> = self.positions.values().cloned().collect();
        positions.sort_by(|a, b| (&a.room, &a.root).cmp(&(&b.room, &b.root)));
        positions.truncate(4096);
        positions
    }
}
impl App {
    pub(super) fn restore_reading_anchor(&mut self, thread: bool, position: ReadingPosition) {
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let Some((generation, _)) = selected else {
            return;
        };
        if self.draft_key(thread).as_ref()
            != Some(&DraftKey {
                room: position.room.clone(),
                root: position.root.clone(),
            })
        {
            return;
        }
        let index = usize::from(thread);
        self.selection.event_focus[index] = Some((*generation, position.event.clone()));
        self.pending_scroll[index] = None;
        self.reading_positions.active[index] = Some((*generation, position));
    }
    pub(super) fn restore_saved_reading_anchor(&mut self, thread: bool) {
        let index = usize::from(thread);
        self.reading_positions.active[index] = None;
        if let Some(key) = self.draft_key(thread)
            && !self.scroll_positions.contains_key(&key)
            && let Some(position) = self.reading_positions.positions.get(&key).cloned()
        {
            self.restore_reading_anchor(thread, position);
        }
    }
    pub(super) fn reading_anchor(&self, thread: bool) -> Option<ReadingPosition> {
        let index = usize::from(thread);
        self.reading_positions.active[index]
            .as_ref()
            .filter(|(generation, position)| {
                self.selection.event_focus[index].as_ref()
                    == Some(&(*generation, position.event.clone()))
            })
            .map(|(_, position)| position.clone())
    }
    pub(super) fn remember_reading_anchor(
        &mut self,
        thread: bool,
        generation: u64,
        event: Option<String>,
        offset: i32,
    ) {
        let index = usize::from(thread);
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        if !self.selection.conversation_visible
            || selected
                .as_ref()
                .is_none_or(|(current, _)| *current != generation)
            || self.pending_scroll[index].is_some()
        {
            return;
        }
        let pane = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        if current_pane(pane, generation).is_none_or(|pane| {
            pane.error.is_some() || matches!(pane.history, super::History::Loading)
        }) {
            return;
        }
        let Some(key) = self.draft_key(thread) else {
            return;
        };
        self.reading_positions.active[index] = None;
        if let Some(event) = event {
            if event.parse::<matrix_sdk::ruma::OwnedEventId>().is_err() {
                return;
            }
            self.reading_positions.positions.insert(
                key.clone(),
                ReadingPosition {
                    room: key.room,
                    root: key.root,
                    event,
                    offset: offset.clamp(-1_000_000, 1_000_000),
                },
            );
        } else {
            self.reading_positions.positions.remove(&key);
        }
    }
    /// A deleted/inaccessible anchor or offline history miss must not strand the
    /// room on an error page. Retry the ordinary timeline once, without an anchor.
    pub(super) fn fallback_reading_anchors(&mut self) {
        for thread in [false, true] {
            let index = usize::from(thread);
            let Some((generation, position)) = self.reading_positions.active[index].as_ref() else {
                continue;
            };
            if self.selection.event_focus[index].as_ref()
                != Some(&(*generation, position.event.clone()))
            {
                self.reading_positions.active[index] = None;
                continue;
            }
            let pane = if thread {
                &self.model.thread
            } else {
                &self.model.room
            };
            if current_pane(pane, *generation).is_some_and(|pane| pane.error.is_some()) {
                self.reading_positions.active[index] = None;
                self.selection.event_focus[index] = None;
                self.pending_scroll[index] = Some(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn position(key: &DraftKey) -> ReadingPosition {
        ReadingPosition {
            room: key.room.clone(),
            root: key.root.clone(),
            event: "$saved".into(),
            offset: -12,
        }
    }
    #[test]
    fn startup_restores_room_and_thread_anchors_but_in_session_offsets_win() {
        let mut app = crate::tests::demo_app();
        let key = app.draft_key(false).unwrap();
        let saved = position(&key);
        app.reading_positions = State::from_saved(std::slice::from_ref(&saved));
        app.scroll_positions.clear();
        app.restore_saved_reading_anchor(false);
        assert_eq!(app.reading_anchor(false), Some(saved));
        assert!(app.pending_scroll[0].is_none());
        assert_eq!(app.selection.event_focus[0].as_ref().unwrap().1, "$saved");
        app.scroll_positions.insert(key, 100.0);
        app.restore_saved_reading_anchor(false);
        assert!(app.reading_anchor(false).is_none());
        app.select_thread("$thread".into());
        let key = app.draft_key(true).unwrap();
        let saved = position(&key);
        app.reading_positions.positions.insert(key, saved.clone());
        app.restore_saved_reading_anchor(true);
        assert_eq!(app.reading_anchor(true), Some(saved));
    }
    #[test]
    fn newest_following_removes_saved_anchor_and_loading_does_not_overwrite_it() {
        let mut app = crate::tests::demo_app();
        let key = app.draft_key(false).unwrap();
        let generation = app.selection.room.as_ref().unwrap().0;
        app.selection.conversation_visible = true;
        app.pending_scroll[0] = None;
        app.remember_reading_anchor(false, generation, Some("$reading".into()), -19);
        assert_eq!(app.reading_positions.positions[&key].offset, -19);
        app.model.room.as_mut().unwrap().history = super::super::History::Loading;
        app.remember_reading_anchor(false, generation, None, 0);
        assert!(app.reading_positions.positions.contains_key(&key));
        app.model.room.as_mut().unwrap().history = super::super::History::Ready;
        app.remember_reading_anchor(false, generation, None, 0);
        assert!(!app.reading_positions.positions.contains_key(&key));
    }
    #[test]
    fn failed_anchor_falls_back_once_and_stale_reports_cannot_change_saved_positions() {
        let mut app = crate::tests::demo_app();
        let key = app.draft_key(false).unwrap();
        let saved = position(&key);
        app.restore_reading_anchor(false, saved.clone());
        app.model.room.as_mut().unwrap().error = Some("unavailable".into());
        app.fallback_reading_anchors();
        assert!(app.reading_anchor(false).is_none());
        assert!(app.selection.event_focus[0].is_none());
        assert_eq!(app.pending_scroll[0], Some(0.0));
        app.reading_positions
            .positions
            .insert(key.clone(), saved.clone());
        app.remember_reading_anchor(false, u64::MAX, None, 0);
        assert_eq!(app.reading_positions.positions[&key], saved);
        app.restore_reading_anchor(false, saved);
        let generation = app.selection.room.as_ref().unwrap().0;
        app.selection.event_focus[0] = Some((generation, "$explicit-reply".into()));
        app.fallback_reading_anchors();
        assert_eq!(
            app.selection.event_focus[0],
            Some((generation, "$explicit-reply".into()))
        );
        assert!(app.reading_anchor(false).is_none());
    }
}
