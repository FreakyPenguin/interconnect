//! Deterministic, memory-only fixtures. Demo mode never creates an SDK client.
use super::DraftKey;
use chatclient::connected::{History, MessageRow, Pane, RoomEntry};
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
        event_id: Some(id.into()),
        sender: sender.into(),
        body: body.into(),
        send_state: String::new(),
        retry: None,
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
            return source
                .into_iter()
                .chain([
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
                ])
                .collect();
        }
        match key.room.as_str() {
            "empty" | "loading" | "error" => vec![],
            "studio" => vec![
                message("welcome", "Maya Chen", "Morning! I’ve been thinking about what makes a chat app feel calm. Less chrome, a little more space, and a clear place for everything."),
                message("direction", "Alex Rivera", "This is the direction I like. Spaces for the bigger picture, sections to keep each room list manageable."),
                message("details", "Maya Chen", "And threads that feel like a conversation on the side. Easy to open, easy to return from."),
                message("proposal", "Jamie Davis", "A small proposal for the first pass:\n\n• Keep the room list compact, but comfortable.\n• Let typography do most of the work.\n• Use color only where it helps you find your way."),
                message("review", "Alex Rivera", "That sounds right. Try opening a thread here — we can work through the details together."),
            ],
            "long" => vec![message("long-body", "Maya Chen", &format!("Some deliberately difficult content to try at narrow widths.\n\n{}\n\nhttps://example.org/a/very/long/reference/with-many-details-and-a-descriptive-name\n\n日本語のメッセージ · Bonjour · Grüß dich", "Good reading rhythm depends on line length, spacing, and a stable layout. ".repeat(12)))],
            "busy" => (0..150).map(|i| message(&format!("busy-{i}"), ["Maya Chen", "Alex Rivera", "Jamie Davis"][i % 3], &format!("Conversation note {}. A longer history to exercise scrolling, selection, and opening threads.", i + 1))).collect(),
            "home" => vec![message("weekend", "Alex Rivera", "A slow Saturday? Coffee, a walk, and somewhere good for lunch."), message("coffee", "Jamie Davis", "Perfect. I’ll bring the camera.")],
            "release" => vec![message("release", "Maya Chen", "The new preview is ready to explore. Room navigation, a quieter timeline, and space for threads."), message("release2", "Alex Rivera", "I’ll try it with a longer conversation this afternoon.")],
            _ => vec![message("hello", "Maya Chen", "A fresh space for ideas, questions, and the occasional good link."), message("hello2", "Jamie Davis", "Glad to be here. What are we working on today?")],
        }
    }
    pub fn pane(&mut self, key: DraftKey, generation: u64) -> Pane {
        if !self.conversations.contains_key(&key) {
            let rows = self.seed(&key);
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
            composer: None,
        }
    }
    pub fn send(&mut self, key: &DraftKey, body: String) {
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
