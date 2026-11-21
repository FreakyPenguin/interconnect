//! Cached, lightweight row identities. No paragraph/editor/widget state is retained.
use super::{thread_activity, unread};
use interconnect::connected::{MessageRow, Pane};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::{Arc, Weak},
};

#[derive(Clone, Copy)]
enum Item {
    Event(usize),
    Thread {
        latest: usize,
        original: Option<usize>,
        replies: usize,
    },
}
pub(super) struct Projection {
    items: Vec<Item>,
    pub keys: Arc<Vec<String>>,
    pub selection_source: Arc<dyn super::selection::Source>,
    pub observed_keys: Arc<HashMap<iced::advanced::widget::Id, String>>,
    pub indices: HashMap<String, usize>,
    source_events: HashMap<String, usize>,
    pub preview_roots: HashSet<String>,
    pub unread_index: Option<usize>,
}
impl Projection {
    pub fn row<'a>(&self, source: &'a [MessageRow], index: usize) -> thread_activity::Row<'a> {
        match self.items[index] {
            Item::Event(i) => thread_activity::Row::Event(&source[i]),
            Item::Thread {
                latest,
                original,
                replies,
            } => thread_activity::Row::Thread {
                root: source[latest].thread_root.as_deref().unwrap(),
                latest: &source[latest],
                original: original.map(|i| &source[i]),
                replies,
            },
        }
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    fn new(pane: &Pane, root: Option<&str>, expanded: bool) -> Self {
        let source = pane.rows.as_slice();
        let rows = if expanded {
            source.iter().map(thread_activity::Row::Event).collect()
        } else {
            thread_activity::project(source, root)
        };
        let indices: HashMap<*const MessageRow, usize> = source
            .iter()
            .enumerate()
            .map(|(i, row)| (row as *const _, i))
            .collect();
        let index = |row: &MessageRow| indices[&(row as *const _)];
        let items: Vec<Item> = rows
            .iter()
            .map(|row| match row {
                thread_activity::Row::Event(row) => Item::Event(index(row)),
                thread_activity::Row::Thread {
                    latest,
                    original,
                    replies,
                    ..
                } => Item::Thread {
                    latest: index(latest),
                    original: original.map(index),
                    replies: *replies,
                },
            })
            .collect();
        let keys: Vec<_> = rows.iter().map(|row| row.key()).collect();
        let selectable = items
            .iter()
            .filter_map(|item| match item {
                Item::Event(i)
                    if !source[*i]
                        .download
                        .as_ref()
                        .is_some_and(|file| source[*i].body == file.filename) =>
                {
                    Some(*i)
                }
                _ => None,
            })
            .collect();
        let mut source_events = HashMap::new();
        for (index, row) in source.iter().enumerate() {
            if let Some(event) = &row.event_id {
                source_events.entry(event.clone()).or_insert(index);
            }
        }
        Self {
            source_events,
            selection_source: super::selection::message_source(pane.rows.clone(), selectable),
            observed_keys: Arc::new(
                keys.iter()
                    .enumerate()
                    .map(|(i, key)| (super::timeline::row_id(i), key.clone()))
                    .collect(),
            ),
            indices: keys
                .iter()
                .enumerate()
                .map(|(i, key)| (key.clone(), i))
                .collect(),
            keys: Arc::new(keys),
            items,
            preview_roots: rows
                .iter()
                .filter_map(|row| match row {
                    thread_activity::Row::Thread { root, .. } => Some((*root).to_owned()),
                    _ => None,
                })
                .collect(),
            unread_index: root
                .is_none()
                .then(|| unread::divider_index(pane, &rows))
                .flatten(),
        }
    }
}
struct Entry {
    source: Weak<Vec<MessageRow>>,
    generation: u64,
    root: Option<String>,
    expanded: bool,
    boundary: Option<String>,
    projection: Arc<Projection>,
}
#[derive(Default)]
pub(super) struct Cache(RefCell<Option<Entry>>);
impl Cache {
    pub fn virtualized(&self, generation: u64) -> bool {
        self.0.borrow().as_ref().is_some_and(|entry| {
            entry.generation == generation && entry.projection.keys.len() > 192
        })
    }
    pub fn clear(&self) {
        self.0.borrow_mut().take();
    }
    /// Reuse the complete source-event index for hot row actions/reply previews.
    /// Before a matching projection exists, retain the ordinary guarded lookup.
    pub fn row<'a>(&self, pane: &'a Pane, event: &str) -> Option<&'a MessageRow> {
        let cached = self
            .0
            .borrow()
            .as_ref()
            .filter(|entry| {
                entry.generation == pane.generation
                    && Weak::ptr_eq(&entry.source, &Arc::downgrade(&pane.rows))
            })
            .map(|entry| entry.projection.source_events.get(event).copied());
        match cached {
            Some(index) => index.and_then(|index| pane.rows.get(index)),
            None => pane
                .rows
                .iter()
                .find(|row| row.event_id.as_deref() == Some(event)),
        }
    }
    pub fn get(&self, pane: &Pane, root: Option<&str>, expanded: bool) -> Arc<Projection> {
        let mut cache = self.0.borrow_mut();
        if let Some(entry) = cache.as_ref()
            && Weak::ptr_eq(&entry.source, &Arc::downgrade(&pane.rows))
            && entry.generation == pane.generation
            && entry.root.as_deref() == root
            && entry.expanded == expanded
            && entry.boundary == pane.read_boundary
        {
            return entry.projection.clone();
        }
        let projection = Arc::new(Projection::new(pane, root, expanded));
        *cache = Some(Entry {
            source: Arc::downgrade(&pane.rows),
            generation: pane.generation,
            root: root.map(str::to_owned),
            expanded,
            boundary: pane.read_boundary.clone(),
            projection: projection.clone(),
        });
        projection
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_cache_reuses_metadata_without_retaining_sdk_snapshots() {
        let app = crate::tests::demo_app();
        let mut pane = app.model.room.as_ref().unwrap().clone();
        pane.rows = Arc::new(pane.rows.as_ref().clone());
        let weak = Arc::downgrade(&pane.rows);
        let cache = Cache::default();
        let first = cache.get(&pane, None, false);
        assert!(Arc::ptr_eq(&first, &cache.get(&pane, None, false)));
        let expanded = cache.get(&pane, None, true);
        assert!(!Arc::ptr_eq(&first, &expanded));
        drop(first);
        drop(expanded);
        drop(pane);
        assert!(
            weak.upgrade().is_none(),
            "cached projection must not retain SDK handles"
        );
        cache.clear();
    }

    #[test]
    fn event_lookup_uses_current_source_and_falls_back_after_snapshot_changes() {
        let app = crate::tests::demo_app();
        let mut pane = app.model.room.as_ref().unwrap().clone();
        let cache = Cache::default();
        let event = pane.rows[1].event_id.clone().unwrap();
        assert_eq!(cache.row(&pane, &event).unwrap().key, pane.rows[1].key);
        let _ = cache.get(&pane, None, false);
        assert_eq!(cache.row(&pane, &event).unwrap().key, pane.rows[1].key);
        assert!(cache.row(&pane, "$missing").is_none());
        let mut inserted = pane.rows[0].clone();
        inserted.event_id = Some("$inserted".into());
        inserted.key = "inserted".into();
        Arc::make_mut(&mut pane.rows).insert(0, inserted);
        assert_eq!(cache.row(&pane, &event).unwrap().key, pane.rows[2].key);
        let _ = cache.get(&pane, None, false);
        assert_eq!(cache.row(&pane, "$inserted").unwrap().key, "inserted");
        assert_eq!(cache.row(&pane, &event).unwrap().key, pane.rows[2].key);
    }
}
