//! Merge bounded worker snapshots with encrypted, device-local read state.
use super::{App, current_pane};
use interconnect::notifications::{Entry, HISTORY_LIMIT, Record, Target};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct State {
    pub dirty: bool,
    seen: Option<Arc<Vec<Entry>>>,
    previous: HashMap<String, Entry>,
}
impl App {
    pub(super) fn merge_notification_history(&mut self) -> bool {
        // Restore durable read state before importing the session's first events.
        if self.demo.is_none() && !self.draft_persistence.belongs_to(&self.account_id) {
            return false;
        }
        let history = self.model.notification_history.clone();
        if self
            .notification_history
            .seen
            .as_ref()
            .is_some_and(|seen| Arc::ptr_eq(seen, &history))
        {
            return false;
        }
        self.notification_history.seen = Some(history.clone());
        let before = self.notification_center.records.clone();
        let mut ids: HashSet<_> = self
            .notification_center
            .records
            .iter()
            .map(|record| record.entry.id.clone())
            .collect();
        for entry in history.iter() {
            if self.demo.is_none() && !interconnect::drafts::valid_notification(entry) {
                continue;
            }
            if ids.insert(entry.id.clone()) {
                self.notification_center.records.push(Record {
                    entry: entry.clone(),
                    read: false,
                });
            } else if self
                .notification_history
                .previous
                .get(&entry.id)
                .is_some_and(|previous| previous.timestamp != entry.timestamp)
                && let Some(record) = self
                    .notification_center
                    .records
                    .iter_mut()
                    .find(|record| record.entry.id == entry.id)
            {
                // Preserve saved read state on baseline import after restart;
                // a later occurrence in this worker is a fresh notification.
                record.entry = entry.clone();
                record.read = false;
            }
        }
        self.notification_history.previous = history
            .iter()
            .map(|entry| (entry.id.clone(), entry.clone()))
            .collect();
        self.notification_center.records.sort_by(|a, b| {
            b.entry
                .timestamp
                .cmp(&a.entry.timestamp)
                .then_with(|| a.entry.id.cmp(&b.entry.id))
        });
        self.notification_center.records.truncate(HISTORY_LIMIT);
        self.notification_center.records != before
    }

    pub(super) fn mark_notification_target_read(&mut self, target: &Target) {
        for record in &mut self.notification_center.records {
            if &record.entry.target == target && !record.read {
                record.read = true;
                self.notification_history.dirty = true;
            }
        }
    }

    pub(super) fn reconcile_notification_reads(&mut self) -> bool {
        self.update_notification_context();
        let latest = [false, true].map(|thread| {
            if !self.selection.conversation_visible || self.saved_scroll(thread) > 1.0 {
                return None;
            }
            if !thread
                && self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width))
            {
                return None;
            }
            let (generation, id) = if thread {
                self.selection.thread.as_ref()?
            } else {
                self.selection.room.as_ref()?
            };
            if self.pending_scroll[usize::from(thread)].is_some() {
                return None;
            }
            // An explicit event jump may load a live timeline before its first
            // centered layout. Do not treat the previous zero offset as reading
            // the newest messages while that destination is still settling.
            if self.selection.event_focus[usize::from(thread)].is_some()
                && self.timeline_metrics[usize::from(thread)].is_none_or(|(view, _, metrics)| {
                    view != *generation || metrics.maximum() - metrics.translation > 1.0
                })
            {
                return None;
            }
            let pane = current_pane(
                if thread {
                    &self.model.thread
                } else {
                    &self.model.room
                },
                *generation,
            )?;
            if pane.error.is_some() || matches!(pane.history, super::History::Loading) {
                return None;
            }
            let timestamp = pane
                .rows
                .iter()
                .filter(|row| row.is_live || row.timestamp_ms.is_some())
                .filter_map(|row| row.timestamp_ms)
                .max()?;
            Some((id.clone(), u64::try_from(timestamp).ok()?))
        });
        let mut changed = false;
        for record in &mut self.notification_center.records {
            if record.read {
                continue;
            }
            let read = match &record.entry.target {
                Target::Conversation { room, thread, .. } => {
                    self.selection
                        .room
                        .as_ref()
                        .is_some_and(|(_, selected)| selected == room)
                        && latest[usize::from(thread.is_some())].as_ref().is_some_and(
                            |(id, timestamp)| {
                                id == thread.as_ref().unwrap_or(room)
                                    && record.entry.timestamp <= *timestamp
                            },
                        )
                }
                Target::Invitation { room } => {
                    self.membership.open && self.membership.focus_invitation.as_ref() == Some(room)
                }
                Target::Verification { flow, device } => {
                    self.verification.open
                        && self
                            .model
                            .verification
                            .incoming
                            .as_ref()
                            .is_some_and(|incoming| {
                                &incoming.flow == flow && &incoming.device == device
                            })
                }
            };
            if read {
                record.read = true;
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, notification_center, tests::demo_app};

    fn app() -> App {
        let mut app = demo_app();
        app.notification_center.reset();
        app.notification_history = Default::default();
        app.model.notification_history = Arc::default();
        app
    }
    fn entry(id: &str, timestamp: u64) -> Entry {
        Entry {
            id: id.into(),
            target: Target::Conversation {
                room: "general".into(),
                thread: None,
                event: None,
            },
            timestamp,
            title: "Example room".into(),
            body: "Example notification".into(),
        }
    }
    fn publish(app: &mut App, entries: Vec<Entry>) {
        let mut model = app.model.clone();
        model.notification_history = entries.into();
        let _ = app.update(Message::State(Box::new(model)));
    }
    #[test]
    fn merges_replaceable_snapshots_without_replaying_read_notifications() {
        let mut app = app();
        publish(&mut app, vec![entry("one", 10)]);
        assert_eq!(app.notification_center.unread_count(), 1);
        let _ = app.update(Message::NotificationCenter(
            notification_center::Event::Open,
        ));
        let _ = app.update(Message::NotificationCenter(
            notification_center::Event::ReadAll(app.account_id.clone()),
        ));
        publish(&mut app, vec![entry("one", 10), entry("two", 11)]);
        assert_eq!(app.notification_center.records.len(), 2);
        assert_eq!(app.notification_center.unread_count(), 1);
        assert!(
            app.notification_center
                .records
                .iter()
                .find(|record| record.entry.id == "one")
                .unwrap()
                .read
        );
        assert_eq!(app.notification_center.records[0].entry.id, "two");
    }
    #[test]
    fn baseline_preserves_saved_read_state_but_live_reinvitation_is_unread() {
        let mut app = app();
        let mut saved = entry("invite", 10);
        saved.target = Target::Invitation {
            room: "!invitation:example.org".into(),
        };
        app.notification_center.records.push(Record {
            entry: saved.clone(),
            read: true,
        });
        let mut baseline = saved;
        baseline.timestamp = 20;
        publish(&mut app, vec![baseline.clone()]);
        assert!(app.notification_center.records[0].read);
        assert_eq!(app.notification_center.records[0].entry.timestamp, 10);
        baseline.timestamp = 30;
        publish(&mut app, vec![baseline]);
        assert!(!app.notification_center.records[0].read);
        assert_eq!(app.notification_center.records[0].entry.timestamp, 30);
    }
    #[test]
    fn bounded_history_retains_newest_entries_and_resets_for_another_account() {
        let mut app = app();
        publish(
            &mut app,
            (0..300).map(|i| entry(&format!("event-{i}"), i)).collect(),
        );
        assert_eq!(app.notification_center.records.len(), HISTORY_LIMIT);
        assert_eq!(app.notification_center.records[0].entry.timestamp, 299);
        let mut model = app.model.clone();
        app.account_id = Some("@first:example.org".into());
        model.account_id = Some("@other:example.org".into());
        model.notification_history = Arc::default();
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.notification_center.records.is_empty());
    }
    #[test]
    fn opening_the_center_keeps_the_live_timeline_offset_and_draft() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe(Option<f32>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.0 = Some((content.height - bounds.height).max(0.0) - translation.y);
                }
            }
        }
        fn inspect(app: &App, tree: &mut widget::Tree, set: bool) -> Option<f32> {
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut view = app.view();
            tree.diff(view.as_widget());
            let size = iced::Size::new(1240.0, 820.0);
            let node =
                view.as_widget_mut()
                    .layout(tree, &renderer, &layout::Limits::new(size, size));
            if set {
                view.as_widget_mut().operate(
                    tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        "room-timeline".into(),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(120.0),
                        },
                    ),
                );
            }
            let mut probe = Probe::default();
            view.as_widget_mut()
                .operate(tree, layout::Layout::new(&node), &renderer, &mut probe);
            probe.0
        }
        let mut app = demo_app();
        let _ = app.update(Message::Room("busy".into()));
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unfinished");
        let selection = app.selection.clone();
        let mut tree = widget::Tree::empty();
        assert!((inspect(&app, &mut tree, true).unwrap() - 120.0).abs() < 1.0);
        let _ = app.update(Message::NotificationCenter(
            notification_center::Event::Open,
        ));
        assert!(inspect(&app, &mut tree, false).is_none());
        let _ = app.update(Message::NotificationCenter(
            notification_center::Event::Close,
        ));
        assert!((inspect(&app, &mut tree, false).unwrap() - 120.0).abs() < 1.0);
        assert_eq!(app.drafts[&key].content.text(), "unfinished");
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
    }

    #[test]
    fn covered_conversation_does_not_clear_unread_entries() {
        let mut app = app();
        let _ = app.update(Message::WindowFocused(true));
        let _ = app.update(Message::NotificationCenter(
            notification_center::Event::Open,
        ));
        let mut notice = entry("current", 1);
        notice.target = Target::Conversation {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            thread: None,
            event: None,
        };
        publish(&mut app, vec![notice]);
        assert_eq!(app.notification_center.unread_count(), 1);
        assert!(!app.selection.conversation_visible);
        assert_eq!(app.selection.read, [None, None]);
    }
}
