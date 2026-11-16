//! Project SDK replies into one recent activity row per thread. Unread state is
//! local to this process: no receipts, timers, extra timelines, or persisted text.
use interconnect::connected::{History, MessageRow};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
pub enum Row<'a> {
    Event(&'a MessageRow),
    Thread {
        root: &'a str,
        latest: &'a MessageRow,
        original: Option<&'a MessageRow>,
        replies: usize,
    },
}
impl<'a> Row<'a> {
    pub fn message(self) -> &'a MessageRow {
        match self {
            Self::Event(message)
            | Self::Thread {
                latest: message, ..
            } => message,
        }
    }
    pub fn key(self) -> String {
        match self {
            Self::Event(message) => message
                .event_id
                .clone()
                .unwrap_or_else(|| message.key.clone()),
            // A replaced summary disappears at its old position. Let the scroll
            // anchor fall back to a surviving message instead of following the
            // moving summary down to the latest reply.
            Self::Thread { root, latest, .. } => format!("thread:{root}:{}", latest.key),
        }
    }
}

pub fn project<'a>(rows: &'a [MessageRow], thread: Option<&str>) -> Vec<Row<'a>> {
    if let Some(root) = thread {
        return rows
            .iter()
            .filter(|row| row.event_id.as_deref() != Some(root))
            .map(Row::Event)
            .collect();
    }
    let originals: HashMap<_, _> = rows
        .iter()
        .filter(|row| row.thread_root.is_none())
        .filter_map(|row| row.event_id.as_deref().map(|id| (id, row)))
        .collect();
    let mut threads = HashMap::new();
    let mut unique = HashSet::new();
    for (index, row) in rows.iter().enumerate() {
        if let Some(root) = row.thread_root.as_deref()
            && unique.insert(&row.key)
        {
            let entry = threads.entry(root).or_insert((index, 0));
            entry.0 = index;
            entry.1 += 1;
        }
    }
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let Some(root) = row.thread_root.as_deref() else {
                return Some(Row::Event(row));
            };
            let (last, count) = threads[root];
            (last == index).then(|| Row::Thread {
                root,
                latest: row,
                original: originals.get(root).copied(),
                replies: count,
            })
        })
        .collect()
}

pub fn excerpt(body: &str, limit: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    let mut graphemes = body
        .split_whitespace()
        .flat_map(|word| std::iter::once(" ").chain(word.graphemes(true)))
        .skip(1);
    let mut result: String = graphemes.by_ref().take(limit).collect();
    if graphemes.next().is_some() {
        result.push('…');
    }
    result
}

#[derive(Default)]
pub struct State {
    rooms: HashMap<String, Room>,
}
#[derive(Default)]
struct Room {
    initialized: bool,
    known: HashSet<String>,
    incoming: HashSet<String>,
    viewed: HashSet<String>,
    unread: HashMap<String, HashSet<String>>,
}
impl State {
    pub fn observe(&mut self, room_id: &str, rows: &[MessageRow], history: &History) {
        let room = self.rooms.entry(room_id.into()).or_default();
        for row in rows {
            // Record all events, including UTD placeholders. A newly received
            // event can become a thread reply after decryption without being
            // mistaken for a second incoming message.
            if !room.known.contains(&row.key) {
                room.known.insert(row.key.clone());
                if room.initialized && row.is_live && !row.is_own {
                    room.incoming.insert(row.key.clone());
                }
            }
            if let Some(root) = &row.thread_root
                && room.incoming.contains(&row.key)
                && !room.viewed.contains(&row.key)
                && !row.is_own
            {
                room.unread
                    .entry(root.clone())
                    .or_default()
                    .insert(row.key.clone());
            }
        }
        if !rows.is_empty() || !matches!(history, History::Loading) {
            room.initialized = true;
        }
    }
    pub fn unread(&self, room: &str, root: &str) -> usize {
        self.rooms
            .get(room)
            .and_then(|room| room.unread.get(root))
            .map_or(0, HashSet::len)
    }
    pub fn viewed(&mut self, room: &str, root: &str, keys: &[String]) {
        let room = self.rooms.entry(room.into()).or_default();
        for key in keys {
            room.viewed.insert(key.clone());
            room.incoming.remove(key);
            if let Some(unread) = room.unread.get_mut(root) {
                unread.remove(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interconnect::connected::RowKind;
    fn event(id: &str, root: Option<&str>, live: bool) -> MessageRow {
        MessageRow {
            key: id.into(),
            kind: RowKind::Message,
            event_id: Some(id.into()),
            sender: "@other:example.org".into(),
            display_name: "Other".into(),
            avatar_url: None,
            image: None,
            download: None,
            reactions: Vec::new(),
            can_react: true,
            timestamp_ms: Some(1),
            thread_replies: 0,
            thread_root: root.map(str::to_owned),
            is_own: false,
            is_live: live,
            body: id.into(),
            formatted: None,
            send_state: String::new(),
            retry: None,
        }
    }
    #[test]
    fn summaries_coalesce_interleaved_replies_at_latest_position_and_thread_keeps_all() {
        let rows = vec![
            event("$root", None, false),
            event("$a", Some("$root"), false),
            event("$ordinary", None, false),
            event("$other", Some("$outside"), false),
            event("$b", Some("$root"), false),
        ];
        let projected = project(&rows, None);
        assert_eq!(
            projected
                .iter()
                .map(|row| row.message().key.as_str())
                .collect::<Vec<_>>(),
            ["$root", "$ordinary", "$other", "$b"]
        );
        assert!(matches!(
            projected[2],
            Row::Thread {
                root: "$outside",
                original: None,
                replies: 1,
                ..
            }
        ));
        assert!(matches!(
            projected[3],
            Row::Thread {
                root: "$root",
                original: Some(_),
                replies: 2,
                ..
            }
        ));
        assert_eq!(project(&rows, Some("$root")).len(), 4);
        let mut updated = rows.clone();
        updated.push(event("$c", Some("$root"), true));
        assert_ne!(
            projected[3].key(),
            project(&updated, None).last().unwrap().key(),
            "a moving summary must not drag the reading anchor"
        );
        updated.last_mut().unwrap().body = "edited latest".into();
        assert_eq!(
            project(&updated, None).last().unwrap().message().body,
            "edited latest"
        );
        updated.last_mut().unwrap().body = "Message removed".into();
        assert_eq!(
            project(&updated, None).last().unwrap().message().body,
            "Message removed"
        );
    }
    #[test]
    fn unread_counts_only_new_incoming_sync_replies_and_survives_navigation() {
        let mut state = State::default();
        let mut rows = vec![event("$old", Some("$root"), true)];
        state.observe("room", &[], &History::Loading);
        state.observe("room", &rows, &History::Ready);
        assert_eq!(state.unread("room", "$root"), 0);
        rows.insert(0, event("$history", Some("$root"), false));
        rows.push(event("$one", Some("$root"), true));
        rows.push(event("$two", Some("$root"), true));
        let mut own = event("$own", Some("$root"), true);
        own.is_own = true;
        rows.push(own);
        for _ in 0..3 {
            state.observe("room", &rows, &History::Ready);
        }
        assert_eq!(state.unread("room", "$root"), 2);
        state.observe("another-room", &rows, &History::Ready);
        assert_eq!(state.unread("another-room", "$root"), 0);
        state.viewed("room", "$root", &["$one".into()]);
        state.observe("room", &rows, &History::Loading);
        assert_eq!(state.unread("room", "$root"), 1);
        state.viewed("room", "$root", &["$two".into()]);
        state.observe("room", &rows, &History::End);
        assert_eq!(state.unread("room", "$root"), 0);
    }
    #[test]
    fn decryption_and_thread_view_arrival_order_do_not_double_count_or_resurrect_unread() {
        let mut state = State::default();
        state.observe("room", &[event("$baseline", None, false)], &History::Ready);
        let mut pending = event("$encrypted", None, true);
        state.observe("room", &[pending.clone()], &History::Ready);
        pending.thread_root = Some("$root".into());
        state.observe("room", &[pending.clone()], &History::Ready);
        assert_eq!(state.unread("room", "$root"), 1);
        state.viewed(
            "room",
            "$root",
            &["$encrypted".into(), "$view-first".into()],
        );
        state.observe(
            "room",
            &[pending, event("$view-first", Some("$root"), true)],
            &History::Ready,
        );
        assert_eq!(state.unread("room", "$root"), 0);
    }
    #[test]
    fn excerpts_normalize_whitespace_and_truncate_on_unicode_boundaries() {
        assert_eq!(excerpt("  Hello\n\tworld  ", 30), "Hello world");
        assert_eq!(excerpt("é🙂日本語", 3), "é🙂日…");
        assert_eq!(excerpt("👨‍👩‍👧‍👦🇩🇪👍🏽 next", 2), "👨‍👩‍👧‍👦🇩🇪…");
        assert_eq!(excerpt("   ", 10), "");
    }
}
