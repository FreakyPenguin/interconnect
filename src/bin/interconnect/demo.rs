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
        section: "Rooms",
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
        id: "inline-images",
        name: "Inline images",
        space: 1,
        section: "Rooms",
        unread: 0,
    },
    Room {
        id: "formatting",
        name: "Message formatting",
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
            canonical_alias: Some(format!("#{}:example.org", r.id)),
            tombstone: None,
            topic: if r.id == "studio" {
                "A place for design notes and plans".into()
            } else {
                String::new()
            },
            member_count: if r.section == "Direct messages" { 2 } else { 5 },
            can_edit_topic: r.section != "Direct messages" && r.id != "release",
            can_invite: r.id != "release",
            can_notify_room: r.id != "release",
            can_redact_own: r.id != "release",
            can_redact_other: false,
            can_pin: r.section != "Direct messages" && r.id != "release",
            pinned_events: if r.id == "studio" {
                vec!["direction".into(), "proposal".into()]
            } else {
                Vec::new()
            },
            tags: if matches!(r.id, "engineering" | "long") {
                vec!["element.io.section.demo-design".into()]
            } else {
                Vec::new()
            },
            favorite: r.section == "Favorites",
            direct: r.section == "Direct messages",
            direct_user: match r.id {
                "maya" => Some("@maya:example.org".into()),
                "alex" => Some("@alex:example.org".into()),
                _ => None,
            },
            unread: r.unread > 0,
            unread_count: r.unread as u64,
        })
        .collect()
}
pub fn presence() -> interconnect::connected::PresenceUsers {
    use interconnect::connected::Presence;
    [
        ("@maya:example.org", Presence::Online),
        ("@alex:example.org", Presence::Away),
        ("@jamie:example.org", Presence::Offline),
    ]
    .into_iter()
    .map(|(user, status)| (user.into(), status))
    .collect()
}

pub fn spaces() -> Vec<SpaceEntry> {
    let mut spaces: Vec<_> = [
        ("studio-space", "Studio"),
        ("community-space", "Community"),
        ("home-space", "Personal"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (id, name))| SpaceEntry {
        id: id.into(),
        name: name.into(),
        avatar_url: None,
        can_manage_rooms: true,
        children: ROOMS
            .iter()
            .filter(|r| r.space == index + 1)
            .map(|r| r.id.into())
            .collect(),
    })
    .collect();
    // Shared links let the original room examples remain directly available
    // while demonstrating a three-level hierarchy and deduplication.
    spaces[0].children.push("product-space".into());
    spaces.extend(
        [
            (
                "product-space",
                "Product",
                vec!["design-space", "engineering"],
            ),
            (
                "design-space",
                "Design",
                vec!["studio", "long", "mobile-space"],
            ),
            ("mobile-space", "Mobile", vec!["images", "inline-images"]),
        ]
        .into_iter()
        .map(|(id, name, children)| SpaceEntry {
            id: id.into(),
            name: name.into(),
            avatar_url: None,
            can_manage_rooms: true,
            children: children.into_iter().map(String::from).collect(),
        }),
    );
    spaces
}
#[derive(Default)]
pub struct Demo {
    pub(super) emoji_packs: Option<interconnect::custom_emoji::management::Listing>,
    pub(super) emoji_uploads: std::collections::VecDeque<interconnect::image_media::Key>,
    pub(super) managed_members: HashMap<String, Vec<interconnect::connected::Member>>,
    pub(super) invitations: HashMap<String, Vec<interconnect::connected::Member>>,
    conversations: HashMap<DraftKey, Vec<MessageRow>>,
    code: Option<std::sync::Arc<interconnect::message_document::Document>>,
    next_message: u64,
}
impl Demo {
    pub(super) fn members(
        &self,
        generation: u64,
        room: &str,
        direct: bool,
    ) -> interconnect::connected::Members {
        let invitations = self.invitations.get(room).map_or(&[][..], Vec::as_slice);
        let mut snapshot = super::members::demo(generation, direct, invitations);
        if let Some(rows) = self.managed_members.get(room) {
            let mut rows = rows.clone();
            for invite in invitations {
                if !rows.iter().any(|m| m.user_id == invite.user_id) {
                    rows.push(invite.clone());
                }
            }
            rows.sort_by_cached_key(|m| (m.invited, m.name.to_lowercase(), m.user_id.clone()));
            snapshot.rows = rows.into();
        }
        snapshot
    }
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
    let mut attachment = interconnect::image_media::Attachment::new(&source, None);
    attachment.gif = true;
    attachment
}
pub fn custom_emoji(room: &str) -> interconnect::custom_emoji::Snapshot {
    interconnect::custom_emoji::Snapshot {
        room: room.into(),
        sources: Vec::new(),
        entries: [
            ("wave", "Demo shapes", "A blue checker", "sample"),
            ("wave", "Other shapes", "A missing checker", "missing"),
        ]
        .into_iter()
        .enumerate()
        .map(
            |(index, (shortcode, pack, name, media))| interconnect::custom_emoji::Emoji {
                id: format!("demo-emoji-{index}"),
                shortcode: shortcode.into(),
                pack: pack.into(),
                name: name.into(),
                uri: format!("mxc://demo.invalid/{media}").into(),
            },
        )
        .collect(),
    }
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
            (
                interconnect::image_media::Key::inline("mxc://demo.invalid/sample".into()),
                handle.clone(),
            ),
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
        failed: [
            missing.preview.unwrap(),
            missing.original,
            interconnect::image_media::Key::inline("mxc://demo.invalid/missing".into()),
        ]
        .into_iter()
        .collect::<std::collections::HashSet<_>>()
        .into(),
    }
}
pub fn inline_assets() -> super::inline_image::LocalImages {
    let pixels: Vec<u8> = (0..40)
        .flat_map(|y| {
            (0..120).flat_map(move |x| {
                if (8..112).contains(&x)
                    && ((17..21).contains(&y) || (x % 24 < 4 && (5..34).contains(&y)))
                {
                    [38, 65, 115, 255]
                } else {
                    [0, 0, 0, 0]
                }
            })
        })
        .collect();
    [(1, iced::widget::image::Handle::from_rgba(120, 40, pixels))]
        .into_iter()
        .collect::<HashMap<_, _>>()
        .into()
}
fn local_inline_fixture() -> MessageRow {
    let mut row = formatted(
        "inline-local",
        "<p>Local asset: <img alt='local generated graphic' width='180' height='60'> aligned with this baseline.</p>",
    );
    let document = std::sync::Arc::make_mut(row.formatted.as_mut().unwrap());
    for run in document.blocks.iter_mut().flat_map(|block| &mut block.runs) {
        if let interconnect::message_document::Inline::Image {
            source, descent, ..
        } = &mut run.content
        {
            *source = Some(interconnect::message_document::ImageSource::Local(1));
            *descent = 12;
        }
    }
    row
}
fn message(id: &str, sender: &str, body: &str) -> MessageRow {
    MessageRow {
        key: id.into(),
        kind: RowKind::Message,
        event_id: Some(id.into()),
        sender: if sender.starts_with('@') {
            sender.into()
        } else {
            format!(
                "@{}:example.org",
                sender
                    .split_whitespace()
                    .next()
                    .unwrap_or("user")
                    .to_lowercase()
            )
        },
        display_name: sender.into(),
        avatar_url: None,
        image: None,
        download: None,
        reactions: Vec::new(),
        can_react: true,
        redactable: true,
        edit_source: None,
        edited: false,
        reply: None,
        read_by: Vec::new(),
        timestamp_ms: Some(chrono::Local::now().timestamp_millis()),
        thread_replies: if id == "review" { 2 } else { 0 },
        thread_root: None,
        is_own: false,
        is_live: false,
        body: body.into(),
        formatted: interconnect::message_document::Document::plain_links(body)
            .map(std::sync::Arc::new),
        send_state: String::new(),
        retry: None,
    }
}
fn formatted(id: &str, html: &str) -> MessageRow {
    let mut row = message(id, "Maya Chen", "Formatted message fallback");
    row.formatted = interconnect::message_document::Document::parse(html).map(std::sync::Arc::new);
    row.thread_replies = 1;
    row
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
pub(super) const CODE_SAMPLE: &str = r#"<pre><code class='language-rust'>fn main() {
    println!("hello 👋");
}</code></pre><pre><code class='language-typescript'>const answer: number = 42;</code></pre><pre><code class='language-toml'>[package]
name = "interconnect"</code></pre><pre><code class='language-dockerfile'>FROM debian:stable
RUN true</code></pre><pre><code class='language-unknown'>Unknown languages stay plain.</code></pre><p>An image placeholder: <img src='mxc://example.org/diagram' alt='Architecture diagram' width='320' height='160'></p>"#;

impl Demo {
    pub(super) fn install_code(
        &mut self,
        code: std::sync::Arc<interconnect::message_document::Document>,
    ) {
        for rows in self.conversations.values_mut() {
            for row in rows
                .iter_mut()
                .filter(|row| row.event_id.as_deref() == Some("format-code"))
            {
                row.formatted = Some(code.clone());
            }
        }
        self.code = Some(code);
    }

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
            "general" => vec![
                message("hello", "Maya Chen", "A fresh space for ideas, questions, and the occasional good link."),
                message("hello2", "Jamie Davis", "Glad to be here. What are we working on today?"),
                message("$preview-message", "Alex Rivera", "This is the destination of the semantic message link in Message formatting."),
            ],
            "images" => {
                let mut sample = message("sample-image", "Maya Chen", "A static image preview, entirely in memory.");
                sample.image = Some(attachment(false));
                sample.thread_replies = 2;
                let mut missing = message("missing-image", "Alex Rivera", "A missing preview keeps its place in the conversation.");
                missing.image = Some(attachment(true));
                let mut gif = message("sample-gif", "Jamie Davis", "A click-to-play animation. Click Play GIF on the preview, or open it full screen. Change playback in Settings → GIF playback.");
                gif.image = Some(gif_attachment());
                gif.thread_replies = 2;
                for (row, filename) in [(&mut sample, "sample.png"), (&mut missing, "missing.png"), (&mut gif, "animation-preview.png")] {
                    row.download = Some(interconnect::download::Attachment { source: row.image.as_ref().unwrap().original.clone(), filename: filename.into() });
                }
                let mut file = message("sample-file", "Maya Chen", "notes.txt");
                file.download = Some(interconnect::download::Attachment { source: interconnect::image_media::Key::file(&matrix_sdk::ruma::events::room::MediaSource::Plain("mxc://demo.invalid/notes".into())), filename: "notes.txt".into() });
                vec![sample, missing, gif, file]
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
            "inline-images" => vec![
                formatted("inline-mixed", "<p>Small <img src='mxc://demo.invalid/sample' alt='small diagram' width='40' height='24'> and tall <img src='mxc://demo.invalid/sample' alt='tall diagram' width='70' height='80'> images flow with <b>styled text</b>.</p>"),
                formatted("inline-wrap", "<p>Resize this pane to wrap around <img src='mxc://demo.invalid/sample' alt='wide diagram' width='260' height='100'> and keep reading after the image.</p>"),
                local_inline_fixture(),
                formatted("inline-states", "<p>Loading <img src='mxc://demo.invalid/waiting' alt='pending image' width='60' height='40'> and failed <img src='mxc://demo.invalid/missing' alt='missing image' width='60' height='40'> keep their declared dimensions.</p><p><span data-mx-spoiler><img src='mxc://demo.invalid/sample' alt='hidden diagram' width='160' height='60'></span></p>"),
                formatted("inline-bidi", "<p>قبل <img src='mxc://demo.invalid/sample' alt='diagram' width='80' height='30'> אחרי</p>"),
            ],
            "formatting" => vec![
                formatted("format-intro", "<h2>Formatting preview</h2><p>Try <b>bold <i>and italic</i></b>, <u>underline</u>, <del>strikethrough</del>, and <code>inline code</code>. Emoji stay intact: café 👩🏽‍💻 ❤️ 🇩🇪.</p><p>Select across paragraphs and into the next message. العربية עברית 日本語</p>"),
                formatted("format-blocks", "<blockquote><p>A quiet quotation.</p><blockquote>Nested quotation.</blockquote></blockquote><ol start='3'><li>Third item<ul><li>Nested item</li></ul></li><li>Fourth item</li></ol>"),
                {
                    let mut row = formatted("format-code", CODE_SAMPLE);
                    if let Some(code) = &self.code {
                        row.formatted = Some(code.clone());
                    }
                    row
                },
                formatted("format-mentions", "<p>Thanks <a href='https://matrix.to/#/@maya:example.org'>Maya Chen</a>! Could <a href='matrix:u/jules:example.org'>Jules 👋</a> take a look?</p><p>A longer mention wraps naturally: <a href='https://matrix.to/#/%40alex%3Aexample.org'>Alex with a deliberately long display name</a>. Plain @maya and @room stay literal.</p><p>قبل <a href='https://matrix.to/#/@maya:example.org'>مايا</a> אחרי. <span data-mx-spoiler><a href='https://matrix.to/#/@jules:example.org'>Hidden mention</a></span></p>"),
                {
                    let mut row = formatted("format-room-wide", "<p>Attention @room! This is a room-wide notification.</p><p>قبل @room אחרי. <code>@room</code> stays code. <span data-mx-spoiler>Hidden @room</span></p>");
                    row.formatted = row.formatted.map(|doc| std::sync::Arc::new((*doc).clone().with_room_wide_mention(true)));
                    row
                },
                formatted("format-room-links", "<p>Continue in <a href='https://matrix.to/#/!demo-general:example.org?via=example.org'>#General</a> or use its <a href='https://matrix.to/#/%23general%3Aexample.org'>#general alias</a>.</p><p>An unfamiliar room: <a href='matrix:roomid/unknown:example.org?via=example.org'>#Another room</a>. قبل <a href='https://matrix.to/#/!demo-general:example.org'>#General</a> אחרי. <span data-mx-spoiler><a href='https://matrix.to/#/!demo-general:example.org'>#Hidden room</a></span></p>"),
                root("matrix-link-previews", "Maya Chen", "Matrix destinations: https://matrix.to/#/!demo-general:example.org/$preview-message https://matrix.to/#/%23general%3Aexample.org https://matrix.to/#/@maya:example.org", 1),
                formatted("matrix-link-preview-reply", "<p>A linked message: <a href='https://matrix.to/#/!demo-general:example.org/$preview-message'><b>read </b>the original</a>. A hidden destination: <span data-mx-spoiler><a href='https://matrix.to/#/!demo-general:example.org/$preview-message'>secret message</a></span></p>"),
                formatted("format-links", "<p><a href='https://spec.matrix.org/'>Matrix specification</a> — hover to inspect the destination; drag to select.</p><p>The ending is <span data-mx-spoiler>hidden <b>until revealed</b></span>. Select and copy before and after revealing.</p>"),
                root("bare-links", "Alex Rivera", "Bare links: https://example.org/docs?q=café&lang=en, (https://en.wikipedia.org/wiki/Matrix_(protocol)).\nTry www.matrix.org too; drag to select without opening.", 1),
                reply("bare-link-reply", "bare-links", "Maya Chen", "Thread links also work: https://spec.matrix.org/"),
                message("format-plain", "Alex Rivera", "Plain messages stay literal: **bold?** <b>HTML?</b>"),
            ],
            "emoji" => vec![
                { let mut row = formatted("custom-emoji", "<p>Custom emoji: <img data-mx-emoticon src='mxc://demo.invalid/sample' alt='A blue checker' title='wave' height='32'> missing: <img data-mx-emoticon src='mxc://demo.invalid/missing' alt='Missing checker' height='32'></p>"); row.reactions = vec![Reaction { key: "mxc://demo.invalid/sample".into(), count: 2, is_own: false }]; row },
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
            room if room.starts_with("demo-dm:") || room.starts_with("demo-room:") => Vec::new(),
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
            if key.room == "studio"
                && key.root.is_none()
                && let Some(row) = rows.last_mut()
            {
                row.read_by = vec!["@alex:demo.invalid".into(), "@maya:demo.invalid".into()];
            }
            for row in &mut rows {
                if !row.is_own && row.sender == "@jamie:example.org" {
                    row.is_own = true;
                    if row.redactable
                        && row.kind == RowKind::Message
                        && row.image.is_none()
                        && row.download.is_none()
                    {
                        row.edit_source =
                            Some(interconnect::connected::EditSource::plain(row.body.clone()));
                    }
                }
            }
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
            read_boundary: (key.room == "release" && key.root.is_none()).then(|| "release".into()),
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
        attachments: Vec<interconnect::attachment::Attachment>,
        reply: Option<String>,
    ) {
        if attachments.is_empty() {
            return self.send(key, body);
        }
        for (index, attachment) in attachments.into_iter().enumerate() {
            let body = if index == 0 { body.as_str() } else { "" };
            let label = if body.trim().is_empty() {
                attachment.filename
            } else {
                format!("{}\n{}", attachment.filename, body)
            };
            self.send(key, label);
            for target in [
                key.clone(),
                DraftKey {
                    room: key.room.clone(),
                    root: None,
                },
            ] {
                if let Some(row) = self
                    .conversations
                    .get_mut(&target)
                    .and_then(|rows| rows.last_mut())
                {
                    row.edit_source = None;
                }
            }
            self.set_sent_reply(key, reply.clone());
        }
        // Demo sends stay entirely in memory; never upload or read file contents.
    }
    pub fn set_sent_reply(&mut self, key: &DraftKey, event: Option<String>) {
        let preview = event.and_then(|event| {
            self.conversations
                .get(key)?
                .iter()
                .find(|row| row.event_id.as_ref() == Some(&event))
                .and_then(interconnect::connected::ReplyPreview::from_row)
        });
        for target in [
            key.clone(),
            DraftKey {
                room: key.room.clone(),
                root: None,
            },
        ] {
            if let Some(row) = self
                .conversations
                .get_mut(&target)
                .and_then(|rows| rows.last_mut())
            {
                row.reply = preview.clone();
            }
        }
    }
    pub fn send_prepared(&mut self, key: &DraftKey, prepared: interconnect::composition::Prepared) {
        self.send(key, prepared.source.clone());
        for key in [
            key.clone(),
            DraftKey {
                room: key.room.clone(),
                root: None,
            },
        ] {
            if let Some(row) = self
                .conversations
                .get_mut(&key)
                .and_then(|rows| rows.last_mut())
            {
                row.edit_source = Some(interconnect::connected::EditSource::from_prepared(
                    &prepared,
                ));
                row.formatted = prepared.document.clone().or_else(|| {
                    interconnect::message_document::Document::plain_links(&row.body)
                        .map(std::sync::Arc::new)
                });
            }
        }
    }
    pub fn delete_message(&mut self, room: &str, event: &str) {
        for (key, rows) in &mut self.conversations {
            if key.room == room {
                for row in rows
                    .iter_mut()
                    .filter(|row| row.event_id.as_deref() == Some(event))
                {
                    row.body = "Message removed".into();
                    row.formatted = None;
                    row.image = None;
                    row.download = None;
                    row.reactions.clear();
                    row.can_react = false;
                    row.redactable = false;
                    row.edit_source = None;
                    row.edited = false;
                    row.retry = None;
                    row.send_state.clear();
                }
            }
        }
    }
    pub fn edit_message(
        &mut self,
        room: &str,
        event: &str,
        prepared: &interconnect::composition::Prepared,
    ) {
        for (key, rows) in &mut self.conversations {
            if key.room == room {
                for row in rows
                    .iter_mut()
                    .filter(|row| row.event_id.as_deref() == Some(event))
                {
                    row.body = prepared.source.clone();
                    row.edit_source =
                        Some(interconnect::connected::EditSource::from_prepared(prepared));
                    row.formatted = prepared.document.clone().or_else(|| {
                        interconnect::message_document::Document::plain_links(&row.body)
                            .map(std::sync::Arc::new)
                    });
                    row.edited = true;
                }
            }
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
        let mut outgoing = message(
            &format!("local-{}", self.next_message),
            "Jamie Davis",
            &body,
        );
        outgoing.is_own = true;
        outgoing.edit_source = Some(interconnect::connected::EditSource::plain(body.clone()));
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

/// Isolated Element-format fixtures; never read account settings in demo mode.
pub fn sections() -> Vec<interconnect::navigation::SectionEntry> {
    interconnect::navigation::sections_from_settings(&serde_json::json!({
        "RoomList.CustomSectionData": {
            "element.io.section.demo-design": {"tag":"element.io.section.demo-design", "name":"Design"}
        },
        "RoomList.OrderedCustomSections": ["element.io.section.demo-design", "chats"]
    }))
}
