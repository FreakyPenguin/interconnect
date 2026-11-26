//! Merge bounded worker snapshots with encrypted, device-local read state.
use super::{App, DraftKey, current_pane, thread_activity, timeline};
use interconnect::notifications::{Entry, HISTORY_LIMIT, Record, Target};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct State {
    pub dirty: bool,
    seen: Option<Arc<Vec<Entry>>>,
    previous: HashMap<String, Entry>,
    visible: [Option<Visible>; 2],
    read_events: HashSet<(DraftKey, String)>,
    read_event_order: VecDeque<(DraftKey, String)>,
    read_through: VecDeque<(DraftKey, u64)>,
    resolved_invitations: HashSet<(String, u64)>,
}
struct Visible {
    generation: u64,
    keys: HashSet<String>,
    metrics: timeline::Metrics,
}
impl State {
    fn remember_reads(
        &mut self,
        conversation: DraftKey,
        events: &HashSet<String>,
        timestamp: Option<u64>,
    ) {
        for event in events {
            let key = (conversation.clone(), event.clone());
            if self.read_events.insert(key.clone()) {
                self.read_event_order.push_back(key);
                if self.read_event_order.len() > HISTORY_LIMIT
                    && let Some(oldest) = self.read_event_order.pop_front()
                {
                    self.read_events.remove(&oldest);
                }
            }
        }
        if let Some(mut timestamp) = timestamp {
            self.read_through.retain(|(key, previous)| {
                if key == &conversation {
                    timestamp = timestamp.max(*previous);
                    false
                } else {
                    true
                }
            });
            self.read_through.push_front((conversation, timestamp));
            self.read_through.truncate(HISTORY_LIMIT);
        }
    }

    fn was_read(&self, entry: &Entry) -> bool {
        let Target::Conversation {
            room,
            thread,
            event,
        } = entry.destination()
        else {
            return false;
        };
        let key = DraftKey { room, root: thread };
        event.is_some_and(|event| self.read_events.contains(&(key.clone(), event)))
            || self.read_through.iter().any(|(conversation, timestamp)| {
                conversation == &key && entry.timestamp <= *timestamp
            })
    }
}
impl App {
    pub(super) fn observe_notification_visibility(
        &mut self,
        thread: bool,
        generation: u64,
        keys: &[String],
        metrics: timeline::Metrics,
    ) {
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        if self.selection.conversation_visible
            && self.pending_scroll[usize::from(thread)].is_none()
            && selected
                .as_ref()
                .is_some_and(|(view, _)| *view == generation)
            && metrics.height.is_finite()
            && metrics.height > 0.0
            && metrics.content_height.is_finite()
            && metrics.translation.is_finite()
        {
            self.notification_history.visible[usize::from(thread)] = Some(Visible {
                generation,
                keys: keys.iter().cloned().collect(),
                metrics,
            });
        }
    }

    fn visible_notification_events(&self, thread: bool) -> HashSet<String> {
        let index = usize::from(thread);
        let Some(visible) = &self.notification_history.visible[index] else {
            return HashSet::new();
        };
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let Some((generation, id)) = selected else {
            return HashSet::new();
        };
        let offset = visible.metrics.maximum() - visible.metrics.translation;
        if !self.selection.conversation_visible
            || *generation != visible.generation
            || self.pending_scroll[index].is_some()
            || (!thread
                && (self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width))
                    || (offset - self.saved_scroll(false).min(visible.metrics.maximum())).abs()
                        > 1.0))
            || (thread
                && self.thread_scroll_target.is_some_and(|(view, target)| {
                    view != *generation
                        || (offset - target.min(visible.metrics.maximum())).abs() > 1.0
                }))
        {
            return HashSet::new();
        }
        let Some(pane) = current_pane(
            if thread {
                &self.model.thread
            } else {
                &self.model.room
            },
            *generation,
        ) else {
            return HashSet::new();
        };
        if pane.error.is_some() || matches!(pane.history, super::History::Loading) {
            return HashSet::new();
        }
        thread_activity::project(&pane.rows, thread.then_some(id.as_str()))
            .into_iter()
            // A room's collapsed thread summary does not expose the reply itself.
            .filter(|row| matches!(row, thread_activity::Row::Event(_)))
            .filter(|row| visible.keys.contains(&row.key()))
            .filter_map(|row| row.message().event_id.clone())
            .collect()
    }

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
                    // Collection and layout arrive independently. A newly
                    // imported event may already have been read in this session.
                    read: self.notification_history.was_read(entry),
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
        // Keep the last viewport so history delivered after layout is reconciled too.
        let visible = [false, true].map(|thread| self.visible_notification_events(thread));
        let latest = [false, true].map(|thread| {
            let index = usize::from(thread);
            let viewport = self.notification_history.visible[index].as_ref()?;
            if visible[index].is_empty()
                || viewport.metrics.maximum() - viewport.metrics.translation > 1.0
                || self.saved_scroll(thread).min(viewport.metrics.maximum()) > 1.0
            {
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
                .filter(|row| {
                    row.event_id
                        .as_ref()
                        .is_some_and(|event| visible[index].contains(event))
                })
                .filter_map(|row| row.timestamp_ms)
                .max()?;
            Some((id.clone(), u64::try_from(timestamp).ok()?))
        });
        for (index, events) in visible.iter().enumerate() {
            if let Some(conversation) = self.draft_key(index == 1) {
                self.notification_history.remember_reads(
                    conversation,
                    events,
                    latest[index].as_ref().map(|(_, timestamp)| *timestamp),
                );
            }
        }
        let joined: HashSet<_> = self
            .model
            .rooms
            .iter()
            .map(|room| room.id.as_str())
            .chain(self.model.spaces.iter().map(|space| space.id.as_str()))
            .collect();
        let invitations: HashSet<_> = self
            .model
            .invitations
            .iter()
            .map(|invite| invite.id.as_str())
            .collect();
        let mut resolved_invitations = HashSet::new();
        let mut changed = false;
        for record in &mut self.notification_center.records {
            let destination = record.entry.destination();
            let newly_resolved = if matches!(&destination, Target::Invitation { room }
                if joined.contains(room.as_str()) && !invitations.contains(room.as_str()))
            {
                let key = (record.entry.id.clone(), record.entry.timestamp);
                let newly_resolved = !self
                    .notification_history
                    .resolved_invitations
                    .contains(&key);
                resolved_invitations.insert(key);
                newly_resolved
            } else {
                false
            };
            if record.read {
                continue;
            }
            let read = match &destination {
                Target::Conversation {
                    room,
                    thread,
                    event,
                } => {
                    self.selection
                        .room
                        .as_ref()
                        .is_some_and(|(_, selected)| selected == room)
                        && (event.as_ref().is_some_and(|event| {
                            let index = usize::from(thread.is_some());
                            let selected = if thread.is_some() {
                                &self.selection.thread
                            } else {
                                &self.selection.room
                            };
                            selected.as_ref().is_some_and(|(_, id)| {
                                id == thread.as_ref().unwrap_or(room)
                                    && visible[index].contains(event)
                            })
                        }) || latest[usize::from(thread.is_some())].as_ref().is_some_and(
                            |(id, timestamp)| {
                                id == thread.as_ref().unwrap_or(room)
                                    && record.entry.timestamp <= *timestamp
                            },
                        ))
                }
                Target::Invitation { room } => {
                    newly_resolved
                        || (self.membership.open
                            && self.membership.focus_invitation.as_ref() == Some(room))
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
        // Track each occurrence once, including already-read records, so later
        // manual unread choices survive. Retain at most the bounded inbox's IDs.
        self.notification_history.resolved_invitations = resolved_invitations;
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

    fn message_entry(id: &str, room: &str, thread: Option<&str>, event: Option<&str>) -> Entry {
        Entry {
            target: Target::Conversation {
                room: room.into(),
                thread: thread.map(str::to_owned),
                event: event.map(str::to_owned),
            },
            // Exact visibility must work independently of timestamp ordering.
            ..entry(id, u64::MAX)
        }
    }

    fn older_view(app: &mut App, thread: bool, keys: &[&str]) -> Message {
        let index = usize::from(thread);
        app.pending_scroll[index] = None;
        app.thread_scroll_target = None;
        let key = app.draft_key(thread).unwrap();
        app.scroll_positions.insert(key, 200.0);
        Message::TimelineVisible {
            thread,
            generation: if thread {
                &app.selection.thread
            } else {
                &app.selection.room
            }
            .as_ref()
            .unwrap()
            .0,
            keys: keys.iter().map(|key| (*key).into()).collect(),
            metrics: timeline::Metrics {
                height: 100.0,
                content_height: 600.0,
                translation: 300.0,
            },
        }
    }

    #[test]
    fn direct_message_layout_clears_visible_notifications() {
        use iced::advanced::{Layout, Shell, clipboard, layout, mouse, widget};
        let mut app = app();
        let _ = app.update(Message::Room("alex".into()));
        let _ = app.update(Message::WindowFocused(true));
        let events: Vec<_> = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .filter_map(|row| row.event_id.clone())
            .collect();
        publish(
            &mut app,
            events
                .iter()
                .map(|event| message_entry(event, "alex", None, Some(event)))
                .collect(),
        );
        assert_eq!(app.notification_center.unread_count(), events.len());
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let size = iced::Size::new(1000.0, 1000.0);
        let mut messages = Vec::new();
        {
            let mut view = app.pane_sized(false, size);
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        assert!(messages.iter().any(
            |message| matches!(message, Message::TimelineVisible { keys, .. } if !keys.is_empty())
        ));
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.notification_center.unread_count(), 0);
    }

    #[test]
    fn notifications_delivered_after_scrolling_and_leaving_a_conversation_stay_read() {
        let mut app = app();
        let _ = app.update(Message::Room("busy".into()));
        let _ = app.update(Message::WindowFocused(true));
        for event in ["busy-20", "busy-21", "busy-22", "busy-23", "busy-24"] {
            let view = older_view(&mut app, false, &[event]);
            let _ = app.update(view);
        }
        let _ = app.update(Message::Room("general".into()));
        publish(
            &mut app,
            (20..=25)
                .map(|index| {
                    let event = format!("busy-{index}");
                    message_entry(&event, "busy", None, Some(&event))
                })
                .collect(),
        );
        assert_eq!(app.notification_center.unread_count(), 1);
        assert!(
            app.notification_center
                .records
                .iter()
                .all(|record| { record.read == (record.entry.id != "busy-25") })
        );
    }

    #[test]
    fn newest_read_boundary_survives_navigation_without_consuming_unseen_arrivals() {
        let mut app = app();
        let _ = app.update(Message::Room("alex".into()));
        let _ = app.update(Message::WindowFocused(true));
        let row = Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
            .last_mut()
            .unwrap();
        row.timestamp_ms = Some(100);
        let event = row.event_id.clone().unwrap();
        let mut view = older_view(&mut app, false, &[&event]);
        app.scroll_positions
            .insert(app.draft_key(false).unwrap(), 0.0);
        if let Message::TimelineVisible { metrics, .. } = &mut view {
            metrics.translation = metrics.maximum();
        }
        let _ = app.update(view);
        let mut unseen = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .last()
            .unwrap()
            .clone();
        unseen.key = "$unseen".into();
        unseen.event_id = Some(unseen.key.clone());
        unseen.timestamp_ms = Some(200);
        let mut model = app.model.clone();
        Arc::make_mut(&mut model.room.as_mut().unwrap().rows).push(unseen);
        let _ = app.update(Message::State(Box::new(model)));
        let _ = app.update(Message::Room("general".into()));
        let mut read = message_entry("older", "alex", None, Some("$older"));
        read.timestamp = 90;
        let mut future = message_entry("future", "alex", None, Some("$unseen"));
        future.timestamp = 200;
        let mut other_room = read.clone();
        other_room.id = "other-room".into();
        other_room.target = Target::Conversation {
            room: "general".into(),
            thread: None,
            event: Some("$older".into()),
        };
        let mut other_thread = read.clone();
        other_thread.id = "other-thread".into();
        other_thread.target = Target::Conversation {
            room: "alex".into(),
            thread: Some("$thread".into()),
            event: Some("$older".into()),
        };
        publish(
            &mut app,
            vec![read.clone(), future, other_room, other_thread],
        );
        assert_eq!(app.notification_center.unread_count(), 3);
        assert!(
            app.notification_center
                .records
                .iter()
                .find(|record| record.entry.id == "older")
                .unwrap()
                .read
        );
        // A saved/manual unread choice wins over a duplicate worker snapshot.
        app.notification_center
            .records
            .iter_mut()
            .find(|record| record.entry.id == "older")
            .unwrap()
            .read = false;
        publish(&mut app, vec![read]);
        assert_eq!(app.notification_center.unread_count(), 4);
    }

    #[test]
    fn remembered_reads_are_bounded_and_cleared_on_account_replacement() {
        let mut app = app();
        for index in 0..HISTORY_LIMIT + 1 {
            app.notification_history.remember_reads(
                DraftKey {
                    room: format!("room-{index}"),
                    root: None,
                },
                &HashSet::from([format!("$event-{index}")]),
                Some(100),
            );
        }
        assert_eq!(app.notification_history.read_events.len(), HISTORY_LIMIT);
        assert_eq!(
            app.notification_history.read_event_order.len(),
            HISTORY_LIMIT
        );
        assert_eq!(app.notification_history.read_through.len(), HISTORY_LIMIT);
        assert!(!app.notification_history.was_read(&message_entry(
            "expired",
            "room-0",
            None,
            Some("$event-0")
        )));
        assert!(app.notification_history.was_read(&message_entry(
            "recent",
            "room-256",
            None,
            Some("$event-256")
        )));
        app.account_id = Some("@first:example.org".into());
        let mut model = app.model.clone();
        model.account_id = Some("@replacement:example.org".into());
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.notification_history.read_events.is_empty());
        assert!(app.notification_history.read_event_order.is_empty());
        assert!(app.notification_history.read_through.is_empty());
    }

    #[test]
    fn reading_an_older_message_clears_only_its_notification_and_preserves_room_state() {
        let mut app = app();
        let _ = app.update(Message::Room("busy".into()));
        let _ = app.update(Message::WindowFocused(true));
        let view = older_view(&mut app, false, &["busy-20"]);
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unfinished");
        let room = Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "busy")
            .unwrap();
        room.unread = true;
        room.unread_count = 3;
        publish(
            &mut app,
            vec![
                message_entry("seen", "busy", None, Some("busy-20")),
                message_entry("offscreen", "busy", None, Some("busy-21")),
                message_entry("other-room", "general", None, Some("busy-20")),
                message_entry("thread", "busy", Some("root"), Some("busy-20")),
                message_entry("legacy-room-only", "busy", None, None),
            ],
        );
        let _ = app.update(view);
        assert_eq!(app.notification_center.unread_count(), 4);
        assert!(
            app.notification_center
                .records
                .iter()
                .all(|record| record.read == (record.entry.id == "seen"))
        );
        assert!(app.selection.read[0].is_none());
        let room = app
            .model
            .rooms
            .iter()
            .find(|room| room.id == "busy")
            .unwrap();
        assert!(room.unread);
        assert_eq!(room.unread_count, 3);
        assert_eq!(app.saved_scroll(false), 200.0);
        assert_eq!(app.drafts[&key].content.text(), "unfinished");
    }

    #[test]
    fn history_arriving_after_visibility_clears_the_exact_event_including_legacy_identity() {
        let mut app = app();
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(Message::WindowFocused(true));
        let view = older_view(&mut app, false, &["$preview-message"]);
        let _ = app.update(view);
        let legacy = format!(
            "message:{}",
            serde_json::to_string(&("general", "$preview-message")).unwrap()
        );
        publish(
            &mut app,
            vec![message_entry(&legacy, "general", None, None)],
        );
        assert_eq!(app.notification_center.unread_count(), 0);
    }

    #[test]
    fn thread_summary_stays_unread_until_the_reply_is_visible_in_its_thread() {
        let mut app = app();
        let _ = app.update(Message::Room("studio".into()));
        let _ = app.update(Message::WindowFocused(true));
        publish(
            &mut app,
            vec![
                message_entry("seen", "studio", Some("review"), Some("review-reply-2")),
                message_entry(
                    "offscreen",
                    "studio",
                    Some("review"),
                    Some("review-reply-1"),
                ),
                message_entry(
                    "other-thread",
                    "studio",
                    Some("other"),
                    Some("review-reply-2"),
                ),
            ],
        );
        let summary = "thread:review:review-reply-2";
        let view = older_view(&mut app, false, &[summary]);
        let _ = app.update(view);
        assert_eq!(app.notification_center.unread_count(), 3);
        let _ = app.update(Message::Thread("review".into()));
        let view = older_view(&mut app, true, &["review-reply-2"]);
        let _ = app.update(view);
        assert_eq!(app.notification_center.unread_count(), 2);
        assert!(
            app.notification_center
                .records
                .iter()
                .all(|record| record.read == (record.entry.id == "seen"))
        );
    }

    #[test]
    fn stale_hidden_loading_and_unsettled_viewports_do_not_clear_notifications() {
        for state in [
            "stale",
            "unfocused",
            "covered",
            "pending",
            "restoring",
            "loading",
            "invalid",
        ] {
            let mut app = app();
            let _ = app.update(Message::Room("busy".into()));
            let _ = app.update(Message::WindowFocused(true));
            let mut view = older_view(&mut app, false, &["busy-20"]);
            publish(
                &mut app,
                vec![message_entry("seen", "busy", None, Some("busy-20"))],
            );
            match state {
                "stale" => {
                    if let Message::TimelineVisible { generation, .. } = &mut view {
                        *generation -= 1;
                    }
                }
                "unfocused" => {
                    let _ = app.update(Message::WindowFocused(false));
                }
                "covered" => {
                    let _ = app.update(Message::NotificationCenter(
                        notification_center::Event::Open,
                    ));
                }
                "pending" => app.pending_scroll[0] = Some(200.0),
                "restoring" => {
                    app.scroll_positions
                        .insert(app.draft_key(false).unwrap(), 400.0);
                }
                "loading" => {
                    app.model.room.as_mut().unwrap().history = super::super::History::Loading
                }
                "invalid" => {
                    if let Message::TimelineVisible { metrics, .. } = &mut view {
                        metrics.translation = f32::NAN;
                    }
                }
                _ => unreachable!(),
            }
            let _ = app.update(view);
            assert_eq!(app.notification_center.unread_count(), 1, "{state}");
        }
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
    fn joined_invites_clear_on_sync_or_import_and_preserve_later_manual_unread() {
        use interconnect::room_membership::Joined;
        for space in [false, true] {
            for import_after_join in [false, true] {
                let mut app = app();
                let joined = Joined {
                    id: "!invite:example.org".into(),
                    name: "Joined".into(),
                    space,
                };
                let mut notice = entry("invite", 10);
                notice.target = Target::Invitation {
                    room: joined.id.clone(),
                };
                if !import_after_join {
                    publish(&mut app, vec![notice.clone()]);
                    assert!(!app.notification_center.records[0].read);
                }
                let mut model = app.model.clone();
                if space {
                    Arc::make_mut(&mut model.spaces).push(joined.space());
                } else {
                    Arc::make_mut(&mut model.rooms).push(joined.room());
                }
                let _ = app.update(Message::State(Box::new(model)));
                if import_after_join {
                    publish(&mut app, vec![notice.clone()]);
                }
                assert!(app.notification_center.records[0].read);
                // This is the same read flag set by the inbox's manual control.
                app.notification_center.records[0].read = false;
                let model = app.model.clone();
                let _ = app.update(Message::State(Box::new(model)));
                assert!(!app.notification_center.records[0].read);

                // A new invitation occurrence must stay unread even if the
                // room-list snapshot still includes the previous membership.
                notice.timestamp = 20;
                let mut model = app.model.clone();
                model.invitations = vec![interconnect::room_membership::Invitation {
                    id: joined.id.clone(),
                    name: joined.name.clone(),
                    inviter: None,
                    space,
                }]
                .into();
                model.notification_history = vec![notice].into();
                let _ = app.update(Message::State(Box::new(model)));
                assert!(!app.notification_center.records[0].read);
                let mut model = app.model.clone();
                model.invitations = Arc::default();
                let _ = app.update(Message::State(Box::new(model)));
                assert!(app.notification_center.records[0].read);
            }
        }
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
