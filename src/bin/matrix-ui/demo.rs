//! Deterministic, memory-only fixtures. Demo mode never creates an SDK client.
use super::DraftKey;
use chatclient::connected::{History, MessageRow, Pane, RoomEntry, RowKind, SpaceEntry};
use std::collections::HashMap;

pub struct Room {
    pub id: &'static str,
    pub name: &'static str,
    pub space: usize,
    pub section: &'static str,
    pub unread: usize,
}
pub const ROOMS: &[Room] = &[
    Room {
        id: "studio",
        name: "Design studio",
        space: 1,
        section: "Favorites",
        unread: 0,
    },
    Room {
        id: "general",
        name: "General",
        space: 1,
        section: "Favorites",
        unread: 0,
    },
    Room {
        id: "release",
        name: "Release notes",
        space: 1,
        section: "Unread",
        unread: 3,
    },
    Room {
        id: "engineering",
        name: "Engineering",
        space: 1,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "long",
        name: "Research, references & tiny details",
        space: 1,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "empty",
        name: "Off the clock",
        space: 1,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "activity",
        name: "Room activity",
        space: 1,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "maya",
        name: "Maya Chen",
        space: 1,
        section: "Direct messages",
        unread: 0,
    },
    Room {
        id: "community",
        name: "Welcome",
        space: 2,
        section: "Favorites",
        unread: 0,
    },
    Room {
        id: "busy",
        name: "The commons",
        space: 2,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "loading",
        name: "Loading preview",
        space: 2,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "error",
        name: "Unavailable preview",
        space: 2,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "home",
        name: "Weekend plans",
        space: 3,
        section: "Favorites",
        unread: 0,
    },
    Room {
        id: "alex",
        name: "Alex Rivera",
        space: 3,
        section: "Direct messages",
        unread: 0,
    },
];
pub fn rooms() -> Vec<RoomEntry> {
    ROOMS
        .iter()
        .map(|r| RoomEntry {
            id: r.id.into(),
            name: r.name.into(),
            favorite: r.section == "Favorites",
            direct: r.section == "Direct messages",
            unread: r.unread > 0,
            unread_count: r.unread as u64,
        })
        .collect()
}
pub fn spaces() -> Vec<SpaceEntry> {
    [
        ("studio-space", "Studio"),
        ("community-space", "Community"),
        ("home-space", "Home"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (id, name))| SpaceEntry {
        id: id.into(),
        name: name.into(),
        children: ROOMS
            .iter()
            .filter(|r| r.space == index + 1)
            .map(|r| r.id.into())
            .collect(),
    })
    .collect()
}
#[derive(Default)]
pub struct Demo {
    conversations: HashMap<DraftKey, Vec<MessageRow>>,
    next_message: u64,
}
fn message(id: &str, sender: &str, body: &str) -> MessageRow {
    MessageRow {
        kind: RowKind::Message,
        event_id: Some(id.into()),
        sender: sender.into(),
        display_name: sender.into(),
        timestamp_ms: Some(chrono::Local::now().timestamp_millis()),
        thread_replies: if id == "review" { 2 } else { 0 },
        body: body.into(),
        send_state: String::new(),
        retry: None,
    }
}
fn activity(id: &str, sender: &str, body: &str) -> MessageRow {
    MessageRow {
        kind: RowKind::Activity,
        ..message(id, sender, body)
    }
}
impl Demo {
    fn seed(&self, key: &DraftKey) -> Vec<MessageRow> {
        if let Some(root) = &key.root {
            let source = self
                .conversations
                .get(&DraftKey {
                    room: key.room.clone(),
                    root: None,
                })
                .and_then(|rows| rows.iter().find(|r| r.event_id.as_ref() == Some(root)))
                .cloned();
            let reply_count = source.as_ref().map_or(0, |row| row.thread_replies);
            return source
                .into_iter()
                .chain(
                    [
                        message(
                            "reply-1",
                            "Alex Rivera",
                            "Yes — a little more breathing room makes a big difference.",
                        ),
                        message(
                            "reply-2",
                            "Maya Chen",
                            "Agreed. Let’s keep the details quiet and the conversation in focus.",
                        ),
                    ]
                    .into_iter()
                    .take(reply_count as usize),
                )
                .collect();
        }
        match key.room.as_str() {
            "empty" | "loading" | "error" => vec![],
            "studio" => vec![
                message("welcome", "Maya Chen", "Morning! I’ve been thinking about what makes a chat app feel calm. Less chrome, a little more space, and a clear place for everything."),
                message("direction", "Alex Rivera", "This is the direction I like. Spaces for the bigger picture, sections to keep each room list manageable."),
                message("details", "Maya Chen", "And threads that feel like a conversation on the side. Easy to open, easy to return from."),
                message("proposal", "Jamie Davis", "A small proposal for the first pass:\n\n• Keep the room list compact, but comfortable.\n• Let typography do most of the work.\n• Use color only where it helps you find your way."),
                message("proposal-followup", "Jamie Davis", "I’d also keep the timestamps quiet, but easy to find."),
                message("review", "Alex Rivera", "That sounds right. Try opening a thread here — we can work through the details together."),
            ],
            "long" => vec![message("long-body", "Maya Chen", &format!("Some deliberately difficult content to try at narrow widths.\n\n{}\n\nhttps://example.org/a/very/long/reference/with-many-details-and-a-descriptive-name\n\n日本語のメッセージ · Bonjour · Grüß dich", "Good reading rhythm depends on line length, spacing, and a stable layout. ".repeat(12)))],
            "activity" => vec![
                activity("activity-create", "Maya Chen", "Created the room"),
                activity("activity-join", "Alex Rivera", "Joined the room"),
                activity("activity-invite", "Maya Chen", "Invited Jamie Davis (@jamie:example.org) to the room"),
                message("activity-hello", "Jamie Davis", "Thanks for the invitation! I’ll keep the room topic up to date."),
                activity("activity-topic", "Jamie Davis", "Changed the room topic to A quiet place for design notes, longer discussions, useful references, and plans for the next release."),
                activity("activity-avatar", "Jamie Davis", "Changed avatar"),
                message("activity-followup", "Jamie Davis", "The latest notes are ready to review."),
                activity("activity-leave", "Alex Rivera", "Left the room"),
            ],
            "busy" => (0..150).map(|i| message(&format!("busy-{i}"), ["Maya Chen", "Alex Rivera", "Jamie Davis"][i % 3], &format!("Conversation note {}. A longer history to exercise scrolling, selection, and opening threads.", i + 1))).collect(),
            "home" => vec![message("weekend", "Alex Rivera", "A slow Saturday? Coffee, a walk, and somewhere good for lunch."), message("coffee", "Jamie Davis", "Perfect. I’ll bring the camera.")],
            "release" => vec![message("release", "Maya Chen", "The new preview is ready to explore. Room navigation, a quieter timeline, and space for threads."), message("release2", "Alex Rivera", "I’ll try it with a longer conversation this afternoon.")],
            _ => vec![message("hello", "Maya Chen", "A fresh space for ideas, questions, and the occasional good link."), message("hello2", "Jamie Davis", "Glad to be here. What are we working on today?")],
        }
    }
    pub fn pane(&mut self, key: DraftKey, generation: u64) -> Pane {
        if !self.conversations.contains_key(&key) {
            let mut rows = self.seed(&key);
            let start = chrono::Local::now().timestamp_millis() - rows.len() as i64 * 60_000;
            for (index, row) in rows.iter_mut().enumerate() {
                row.timestamp_ms = Some(start + index as i64 * 60_000);
            }
            self.conversations.insert(key.clone(), rows);
        }
        Pane {
            generation,
            rows: self.conversations[&key].clone().into(),
            error: (key.room == "error").then(|| {
                "This sample room is unavailable. Choose another room to continue.".into()
            }),
            history: if key.room == "loading" {
                History::Loading
            } else {
                History::End
            },
            load_older: None,
            history_revision: 0,
            composer: None,
        }
    }
    pub fn send(&mut self, key: &DraftKey, body: String) {
        if let Some(root) = &key.root
            && let Some(row) = self
                .conversations
                .get_mut(&DraftKey {
                    room: key.room.clone(),
                    root: None,
                })
                .and_then(|rows| {
                    rows.iter_mut()
                        .find(|row| row.event_id.as_ref() == Some(root))
                })
        {
            row.thread_replies += 1;
        }
        self.next_message += 1;
        self.conversations
            .entry(key.clone())
            .or_default()
            .push(message(
                &format!("local-{}", self.next_message),
                "Jamie Davis",
                &body,
            ));
    }
}
