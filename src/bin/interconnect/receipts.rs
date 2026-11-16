use super::{App, History, current_pane, thread_activity, timeline};
use interconnect::connected::ReadTarget;

impl App {
    pub(super) fn read_visible(
        &mut self,
        thread: bool,
        generation: u64,
        keys: &[String],
        metrics: timeline::Metrics,
    ) {
        let index = usize::from(thread);
        self.update_notification_context();
        if !self.selection.conversation_visible
            || self.pending_scroll[index].is_some()
            || !metrics.height.is_finite()
            || metrics.height <= 0.0
            || !metrics.content_height.is_finite()
            || !metrics.translation.is_finite()
        {
            return;
        }
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let Some((current, id)) = selected else {
            return;
        };
        if *current != generation {
            return;
        }
        let offset = metrics.maximum() - metrics.translation;
        if thread {
            if self.thread_scroll_target.is_some_and(|(view, target)| {
                view != generation || (offset - target.min(metrics.maximum())).abs() > 1.0
            }) {
                return;
            }
        } else if offset.abs() > 1.0 || self.saved_scroll(false).min(metrics.maximum()) > 1.0 {
            // Reading older history must not clear the room's unread badge.
            return;
        }
        let pane = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        let Some(pane) = current_pane(pane, generation) else {
            return;
        };
        if pane.error.is_some() || matches!(pane.history, History::Loading) {
            return;
        }
        let rows = thread_activity::project(&pane.rows, thread.then_some(id.as_str()));
        let target = rows
            .iter()
            .rev()
            .find(|row| row.message().event_id.is_some() && keys.contains(&row.key()))
            .and_then(|row| row.message().event_id.as_ref());
        if target.is_none() && (thread || !pane.rows.is_empty()) {
            return;
        }
        self.selection.read[index] = Some(ReadTarget {
            generation,
            event: target.cloned(),
        });
        if !thread
            && self.demo.is_some()
            && let Some((_, room)) = &self.selection.room
            && let Some(entry) = std::sync::Arc::make_mut(&mut self.model.rooms)
                .iter_mut()
                .find(|entry| entry.id == *room)
        {
            entry.unread = false;
            entry.unread_count = 0;
        }
    }

    pub(super) fn validate_read_targets(&mut self) {
        for (index, selected) in [&self.selection.room, &self.selection.thread]
            .into_iter()
            .enumerate()
        {
            if !self.selection.conversation_visible
                || self.selection.read[index].as_ref().is_some_and(|target| {
                    selected
                        .as_ref()
                        .is_none_or(|(generation, _)| *generation != target.generation)
                })
            {
                self.selection.read[index] = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, settings, tests::demo_app};

    fn visible(app: &App, thread: bool, offset: f32) -> Message {
        let selected = if thread {
            &app.selection.thread
        } else {
            &app.selection.room
        };
        let (generation, id) = selected.as_ref().unwrap();
        let pane = if thread {
            &app.model.thread
        } else {
            &app.model.room
        };
        let rows = thread_activity::project(&pane.as_ref().unwrap().rows, thread.then_some(id));
        Message::TimelineVisible {
            thread,
            generation: *generation,
            keys: rows.iter().map(|row| row.key()).collect(),
            metrics: timeline::Metrics {
                height: 100.0,
                content_height: 600.0,
                translation: 500.0 - offset,
            },
        }
    }

    #[test]
    fn room_unread_clears_only_after_focused_current_bottom_is_visible() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("studio".into()));
        // Give this fixture an unread badge independently of demo navigation sections.
        let room = app.selection.room.as_ref().unwrap().1.clone();
        let entry = std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|entry| entry.id == room)
            .unwrap();
        entry.unread = true;
        entry.unread_count = 3;
        let message = visible(&app, false, 0.0);
        let _ = app.update(message.clone());
        assert!(app.selection.read[0].is_none());
        let _ = app.update(Message::WindowFocused(true));
        let mut stale = message.clone();
        if let Message::TimelineVisible { generation, .. } = &mut stale {
            *generation -= 1;
        }
        let _ = app.update(stale);
        assert!(app.selection.read[0].is_none());
        app.pending_scroll[0] = Some(300.0);
        app.read_visible(
            false,
            app.selection.room.as_ref().unwrap().0,
            &["ignored".into()],
            timeline::Metrics {
                height: 100.0,
                content_height: 600.0,
                translation: 500.0,
            },
        );
        assert!(app.selection.read[0].is_none());
        app.pending_scroll[0] = None;
        let _ = app.update(visible(&app, false, 100.0));
        assert!(app.selection.read[0].is_none());
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unfinished draft");
        app.scroll_positions.insert(key.clone(), 300.0);
        let _ = app.update(message.clone());
        assert!(
            app.selection.read[0].is_none(),
            "wait for the saved position even if a transient layout reports bottom"
        );
        assert_eq!(app.scroll_positions[&key], 300.0);
        app.scroll_positions.insert(key.clone(), 0.0);
        let _ = app.update(message);
        assert!(app.selection.read[0].is_some());
        assert_eq!(app.drafts[&key].content.text().trim(), "unfinished draft");
        assert!(
            app.model
                .rooms
                .iter()
                .filter(|entry| entry.id == room)
                .all(|entry| !entry.unread && entry.unread_count == 0)
        );
        let _ = app.update(Message::WindowFocused(false));
        assert!(app.selection.read[0].is_none());
    }

    #[test]
    fn overlays_and_navigation_reject_receipts() {
        let mut app = demo_app();
        let _ = app.update(Message::WindowFocused(true));
        let message = visible(&app, false, 0.0);
        let _ = app.update(Message::Settings(settings::Event::Open));
        let _ = app.update(message.clone());
        assert!(app.selection.read[0].is_none());
        let _ = app.update(Message::Settings(settings::Event::Cancel));
        let _ = app.update(message.clone());
        assert!(app.selection.read[0].is_some());
        let _ = app.update(Message::Room("home".into()));
        assert!(app.selection.read[0].is_none());
        let _ = app.update(message);
        assert!(app.selection.read[0].is_none());
    }

    #[test]
    fn empty_room_can_clear_manual_unread_and_thread_uses_its_visible_reply() {
        let mut app = demo_app();
        let _ = app.update(Message::Room("empty".into()));
        let _ = app.update(Message::WindowFocused(true));
        let _ = app.update(visible(&app, false, 0.0));
        assert_eq!(app.selection.read[0].as_ref().unwrap().event, None);
        let _ = app.update(Message::Room("threads".into()));
        let _ = app.update(Message::OpenThreadLatest("thread-plan".into()));
        let _ = app.update(visible(&app, true, 0.0));
        assert!(app.selection.read[1].as_ref().unwrap().event.is_some());
        assert!(app.selection.read[0].is_none());
        let _ = app.update(Message::CloseThread);
        assert!(app.selection.read[1].is_none());
    }
}
