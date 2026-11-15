//! Deterministic, memory-only fixtures. Demo mode never creates an SDK client.
use super::DraftKey;
use interconnect::connected::{
    History, MessageRow, Pane, Reaction, RoomEntry, RowKind, SpaceEntry,
};
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
        id: "images",
        name: "Images",
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
        id: "emoji",
        name: "Emoji preview",
        space: 1,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "threads",
        name: "Thread activity",
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
            topic: if r.id == "studio" {
                "A place for design notes and plans".into()
            } else {
                String::new()
            },
            member_count: if r.section == "Direct messages" { 2 } else { 5 },
            can_edit_topic: r.section != "Direct messages" && r.id != "release",
            can_invite: r.id != "release",
            can_pin: r.section != "Direct messages" && r.id != "release",
            pinned_events: if r.id == "studio" {
                vec!["direction".into(), "proposal".into()]
            } else {
                Vec::new()
            },
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
    pub(super) invitations: HashMap<String, Vec<interconnect::connected::Member>>,
    conversations: HashMap<DraftKey, Vec<MessageRow>>,
    next_message: u64,
}
pub fn attachment(missing: bool) -> interconnect::image_media::Attachment {
    let source = matrix_sdk::ruma::events::room::MediaSource::Plain(
        if missing {
            "mxc://demo.invalid/missing"
        } else {
            "mxc://demo.invalid/sample"
        }
        .into(),
    );
    interconnect::image_media::Attachment::new(&source, None)
}
pub fn gif_attachment() -> interconnect::image_media::Attachment {
    let source =
        matrix_sdk::ruma::events::room::MediaSource::Plain("mxc://demo.invalid/gif".into());
    interconnect::image_media::Attachment::new(&source, None)
}
pub fn images() -> interconnect::media::Snapshot<interconnect::image_media::Key> {
    // A deterministic pixel fixture, generated once at demo startup. No files,
    // decoding, account access, or downloads are involved.
    let pixels: Vec<u8> = (0..180)
        .flat_map(|y| {
            (0..320).flat_map(move |x| {
                if (x / 40 + y / 30) % 2 == 0 {
                    [175, 199, 218, 255]
                } else {
                    [226, 235, 242, 255]
                }
            })
        })
        .collect();
    let handle = iced::widget::image::Handle::from_rgba(320, 180, pixels);
    let sample = attachment(false);
    let missing = attachment(true);
    let gif = gif_attachment();
    let animation = std::sync::Arc::new(interconnect::media::Animation {
        frames: (0..8)
            .map(|frame| {
                let pixels = (0..180)
                    .flat_map(|y| {
                        (0..320).flat_map(move |x| {
                            if (x / 40 + y / 30 + frame) % 8 == 0 {
                                [80, 130, 180, 255]
                            } else {
                                [226, 235, 242, 255]
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                (
                    iced::widget::image::Handle::from_rgba(320, 180, pixels),
                    std::time::Duration::from_millis(150),
                )
            })
            .collect(),
    });
    let first = animation.frames[0].0.clone();
    interconnect::media::Snapshot {
        ready: [
            (sample.preview.unwrap(), handle.clone()),
            (sample.original, handle),
            (gif.preview.unwrap(), first.clone()),
            (gif.original.clone(), first),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>()
        .into(),
        animations: [(gif.original, Ok(animation))]
            .into_iter()
            .collect::<HashMap<_, _>>()
            .into(),
        failed: [missing.preview.unwrap(), missing.original]
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .into(),
    }
}
fn message(id: &str, sender: &str, body: &str) -> MessageRow {
    MessageRow {
        key: id.into(),
        kind: RowKind::Message,
        event_id: Some(id.into()),
        sender: sender.into(),
        display_name: sender.into(),
        avatar_url: None,
        image: None,
        reactions: Vec::new(),
        can_react: true,
        timestamp_ms: Some(chrono::Local::now().timestamp_millis()),
        thread_replies: if id == "review" { 2 } else { 0 },
        thread_root: None,
        is_own: false,
        is_live: false,
        body: body.into(),
        send_state: String::new(),
        retry: None,
    }
}
fn activity(id: &str, sender: &str, body: &str) -> MessageRow {
    MessageRow {
        kind: RowKind::Activity,
        can_react: false,
        ..message(id, sender, body)
    }
}
fn reply(id: &str, root: &str, sender: &str, body: &str) -> MessageRow {
    MessageRow {
        thread_root: Some(root.into()),
        ..message(id, sender, body)
    }
}
fn root(id: &str, sender: &str, body: &str, replies: u32) -> MessageRow {
    MessageRow {
        thread_replies: replies,
        ..message(id, sender, body)
    }
}
fn reacted(mut message: MessageRow, reactions: &[(&str, usize, bool)]) -> MessageRow {
    message.reactions = reactions
        .iter()
        .map(|(key, count, is_own)| Reaction {
            key: (*key).into(),
            count: *count,
            is_own: *is_own,
        })
        .collect();
    message
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
            let replies: Vec<_> = self
                .conversations
                .get(&DraftKey {
                    room: key.room.clone(),
                    root: None,
                })
                .into_iter()
                .flatten()
                .filter(|row| row.thread_root.as_ref() == Some(root))
                .cloned()
                .collect();
            if !replies.is_empty() {
                return source.into_iter().chain(replies).collect();
            }
            return source
                .into_iter()
                .chain(
                    [
                        reply(
                            "reply-1",
                            root,
                            "Alex Rivera",
                            "Yes — a little more breathing room makes a big difference.",
                        ),
                        reply(
                            "reply-2",
                            root,
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
            "images" => {
                let mut sample = message("sample-image", "Maya Chen", "A static image preview, entirely in memory.");
                sample.image = Some(attachment(false));
                sample.thread_replies = 2;
                let mut missing = message("missing-image", "Alex Rivera", "A missing preview keeps its place in the conversation.");
                missing.image = Some(attachment(true));
                let mut gif = message("sample-gif", "Jamie Davis", "A click-to-play animation. Open it, then click Play GIF. Change playback in Settings → GIF playback.");
                gif.image = Some(gif_attachment());
                gif.thread_replies = 2;
                vec![sample, missing, gif]
            },
            "studio" => vec![
                reacted(message("welcome", "Maya Chen", "Morning! I’ve been thinking about what makes a chat app feel calm. Less chrome, a little more space, and a clear place for everything."), &[("❤️", 3, true), ("👍🏽", 2, false)]),
                message("direction", "Alex Rivera", "This is the direction I like. Spaces for the bigger picture, sections to keep each room list manageable."),
                message("details", "Maya Chen", "And threads that feel like a conversation on the side. Easy to open, easy to return from."),
                message("proposal", "Jamie Davis", "A small proposal for the first pass:\n\n• Keep the room list compact, but comfortable.\n• Let typography do most of the work.\n• Use color only where it helps you find your way."),
                message("proposal-followup", "Jamie Davis", "I’d also keep the timestamps quiet, but easy to find."),
                reacted(message("review", "Alex Rivera", "That sounds right. Try opening a thread here — we can work through the details together."), &[("👩🏽‍💻", 2, false), ("👍", 1, true)]),
                reacted(reply("review-reply-1", "review", "Maya Chen", "Yes — a little more breathing room makes a big difference."), &[("❤️", 1, false)]),
                reply("review-reply-2", "review", "Jamie Davis", "Agreed. Let’s keep the details quiet and the conversation in focus."),
            ],
            "emoji" => vec![
                root("emoji-original", "Maya Chen", "Faces and symbols: 😀 😂 ☺ ❤ ❤️ ♥ ☀️ ✈️ ☎️ ☕", 1),
                reply("emoji-reply", "emoji-original", "Alex Rivera", "Joined sequences: 👍🏽 🇩🇪 👩🏽‍💻 👨‍👩‍👧‍👦 1️⃣ #️⃣"),
                message("emoji-text", "Jamie Davis", "Explicit text presentation: ☺\u{fe0e} ❤\u{fe0e}\nOrdinary text: café # * 123"),
                message("emoji-wrap", "Maya Chen", "Try selecting and copying this whole message at narrow widths: café ☺ ❤ ❤️ 👍🏽 🇩🇪 👩🏽‍💻 👨‍👩‍👧‍👦 1️⃣. Paste it into the composer to compare."),
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
            "threads" => vec![
                root("thread-plan", "Alex Rivera", "Release checklist: what should we finish before the next preview?", 3),
                root("thread-design", "Maya Chen", "A few thoughts on the sidebar spacing and navigation.", 2),
                reply("plan-1", "thread-plan", "Maya Chen", "The thread activity should be easy to find."),
                message("thread-room-note", "Jamie Davis", "I’ve put the notes in the shared folder."),
                reply("design-1", "thread-design", "Alex Rivera", "I like the compact spacing."),
                reply("plan-2", "thread-plan", "Jamie Davis", "Let’s keep one summary at the latest reply."),
                reply("plan-3", "thread-plan", "Maya Chen", "And make it easy to jump straight into the conversation."),
                reply("design-2", "thread-design", "Jamie Davis", "The new layout leaves more room for the conversation."),
                reply("old-root-reply", "older-thread", "Alex Rivera", "A reply to an older thread whose original is outside this loaded history."),
            ],
            "busy" => (0..150).map(|i| message(&format!("busy-{i}"), ["Maya Chen", "Alex Rivera", "Jamie Davis"][i % 3], &format!("Conversation note {}. A longer history to exercise scrolling, selection, and opening threads.", i + 1))).collect(),
            "home" => vec![message("weekend", "Alex Rivera", "A slow Saturday? Coffee, a walk, and somewhere good for lunch."), message("coffee", "Jamie Davis", "Perfect. I’ll bring the camera.")],
            "release" => vec![message("release", "Maya Chen", "The new preview is ready to explore. Room navigation, a quieter timeline, and space for threads."), message("release2", "Alex Rivera", "I’ll try it with a longer conversation this afternoon.")],
            _ => vec![message("hello", "Maya Chen", "A fresh space for ideas, questions, and the occasional good link."), message("hello2", "Jamie Davis", "Glad to be here. What are we working on today?")],
        }
    }
    pub fn pinned_rows(&self, room: &str, ids: &[String]) -> Vec<MessageRow> {
        ids.iter()
            .filter_map(|id| {
                self.conversations
                    .iter()
                    .filter(|(key, _)| key.room == room)
                    .flat_map(|(_, rows)| rows)
                    .find(|row| row.event_id.as_ref() == Some(id))
                    .cloned()
            })
            .collect()
    }
    pub fn pane(&mut self, key: DraftKey, generation: u64) -> Pane {
        if !self.conversations.contains_key(&key) {
            let mut rows = self.seed(&key);
            let start = chrono::Local::now().timestamp_millis() - rows.len() as i64 * 60_000;
            // Thread fixtures reuse the original room events and timestamps.
            if key.root.is_none() {
                for (index, row) in rows.iter_mut().enumerate() {
                    row.timestamp_ms = Some(start + index as i64 * 60_000);
                }
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
    pub fn send_attachment_or_text(
        &mut self,
        key: &DraftKey,
        body: String,
        attachment: Option<interconnect::attachment::Attachment>,
    ) {
        let Some(attachment) = attachment else {
            return self.send(key, body);
        };
        let label = if body.trim().is_empty() {
            attachment.filename
        } else {
            format!("{}\n{}", attachment.filename, body)
        };
        self.send(key, label);
        // Demo sends stay entirely in memory; never upload or read file contents.
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
        let mut outgoing = message(
            &format!("local-{}", self.next_message),
            "Jamie Davis",
            &body,
        );
        outgoing.is_own = true;
        outgoing.thread_root = key.root.clone();
        if key.root.is_some() {
            self.conversations
                .entry(DraftKey {
                    room: key.room.clone(),
                    root: None,
                })
                .or_default()
                .push(outgoing.clone());
        }
        self.conversations
            .entry(key.clone())
            .or_default()
            .push(outgoing);
    }
    pub fn add_reaction(&mut self, room: &str, event: &str, key: &str) {
        // Update all loaded copies, including a pinned original and its room row.
        for (conversation, rows) in &mut self.conversations {
            if conversation.room != room {
                continue;
            }
            for row in rows
                .iter_mut()
                .filter(|row| row.event_id.as_deref() == Some(event))
            {
                if let Some(reaction) = row
                    .reactions
                    .iter_mut()
                    .find(|reaction| reaction.key == key)
                {
                    if !reaction.is_own {
                        reaction.count += 1;
                        reaction.is_own = true;
                    }
                } else {
                    row.reactions.push(Reaction {
                        key: key.into(),
                        count: 1,
                        is_own: true,
                    });
                    row.reactions.sort_unstable_by(|a, b| a.key.cmp(&b.key));
                }
            }
        }
    }
    pub fn set_reaction(&mut self, room: &str, event: &str, key: &str, selected: bool) {
        if selected {
            self.add_reaction(room, event, key);
            return;
        }
        for (conversation, rows) in &mut self.conversations {
            if conversation.room != room {
                continue;
            }
            for row in rows
                .iter_mut()
                .filter(|row| row.event_id.as_deref() == Some(event))
            {
                if let Some(reaction) = row
                    .reactions
                    .iter_mut()
                    .find(|reaction| reaction.key == key)
                    && reaction.is_own
                {
                    reaction.count = reaction.count.saturating_sub(1);
                    reaction.is_own = false;
                }
                row.reactions.retain(|reaction| reaction.count > 0);
            }
        }
    }
    pub fn incoming(&mut self) {
        self.next_message += 1;
        let mut incoming = reply(
            &format!("incoming-{}", self.next_message),
            "thread-plan",
            "Maya Chen",
            &format!(
                "New reply {}: the latest update stays in one compact thread summary.",
                self.next_message
            ),
        );
        incoming.is_live = true;
        let room = self
            .conversations
            .entry(DraftKey {
                room: "threads".into(),
                root: None,
            })
            .or_default();
        if let Some(root) = room
            .iter_mut()
            .find(|row| row.event_id.as_deref() == Some("thread-plan"))
        {
            root.thread_replies += 1;
        }
        room.push(incoming.clone());
        if let Some(thread) = self.conversations.get_mut(&DraftKey {
            room: "threads".into(),
            root: Some("thread-plan".into()),
        }) {
            thread.push(incoming);
        }
    }
}
