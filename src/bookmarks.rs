//! MSC4482 adapter. The SDK owns encryption, history, gaps and redecryption;
//! this module owns the experimental wire format and bookmark projection.
use crate::connected::{Model, RuntimeOwned};
use anyhow::{Context, ensure};
use iced::futures::StreamExt;
use matrix_sdk::{
    Client, Room,
    ruma::{
        OwnedRoomId, OwnedTransactionId, TransactionId, api::client::room::create_room,
        events::AnyGlobalAccountDataEvent, serde::Raw,
    },
};
use matrix_sdk_ui::timeline::{TimelineBuilder, TimelineFocus};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::{Mutex, Notify, watch};

// MSC4482 has not assigned unstable names yet. Write a single experimental
// namespace; accept the proposal's future stable names as well. Keep migration
// confined to this adapter. m.text is also included for extensible-event clients.
pub const ROOM_TYPE: &str = "org.matrix.msc4482.bookmarks";
const EVENT_TYPE: &str = "org.matrix.msc4482.bookmark";
const ACCOUNT_TYPE: &str = "org.matrix.msc4482.bookmarks_room";
const POINTER: &str = "org.matrix.msc4482.pointer";

pub fn is_bookmarks_room(room: &Room) -> bool {
    room.room_type()
        .is_some_and(|kind| matches!(kind.as_str(), ROOM_TYPE | "m.bookmarks"))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pointer {
    pub room_id: String,
    pub event_id: String,
    #[serde(default)]
    pub via: Vec<String>,
}
impl Pointer {
    pub fn matches(&self, room: &str, event: &str) -> bool {
        self.room_id == room && self.event_id == event
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub pointer: Pointer,
    pub bookmark_ids: Vec<String>,
    pub saved_at: i64,
    pub fallback: String,
}
#[derive(Clone, Debug)]
pub struct Preview {
    pub pointer: Pointer,
    pub room_name: String,
    pub sender: String,
    pub body: String,
    pub thread_root: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    /// False until all storage-room history has been loaded and decrypted.
    pub complete: bool,
    pub loading: bool,
    pub error: Option<String>,
}
impl Snapshot {
    pub fn contains(&self, room: &str, event: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.pointer.matches(room, event))
    }
}

#[derive(Default)]
struct Writes {
    // Retain a created room if publishing account data fails, so retry cannot
    // create another room. Never silently replace an inaccessible remote room.
    created: Option<OwnedRoomId>,
    indexed_room: Option<OwnedRoomId>,
    transactions: HashMap<String, OwnedTransactionId>,
    confirmed: HashMap<(OwnedRoomId, String), Value>,
    removed: HashSet<(OwnedRoomId, String)>,
}
struct Inner {
    client: Client,
    room_list: OnceLock<Arc<matrix_sdk_ui::room_list_service::RoomListService>>,
    subscription: std::sync::Mutex<Option<OwnedRoomId>>,
    writes: Mutex<Writes>,
    changes: Notify,
    snapshots: watch::Sender<Snapshot>,
}
#[derive(Clone)]
pub struct Service(Arc<RuntimeOwned<Inner>>);
impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bookmarks")
    }
}
impl Service {
    pub(crate) fn new(client: Client) -> Self {
        Self(RuntimeOwned::new(Inner {
            client,
            room_list: OnceLock::new(),
            subscription: std::sync::Mutex::new(None),
            writes: Mutex::new(Writes::default()),
            changes: Notify::new(),
            snapshots: watch::channel(Snapshot {
                loading: true,
                ..Default::default()
            })
            .0,
        }))
    }
    pub fn refresh(&self) {
        self.0.changes.notify_one();
    }
    /// Each displayed entry owns a cancellable SDK timeline. Setups are bounded;
    /// settled previews wait on SDK updates without timers or repeat fetches.
    pub fn previews(
        self,
        pointers: Vec<Pointer>,
    ) -> iced::futures::stream::BoxStream<'static, Preview> {
        use iced::futures::SinkExt;
        let slots = Arc::new(tokio::sync::Semaphore::new(4));
        let streams = pointers.into_iter().map(|pointer| {
            let service = self.clone();
            let slots = slots.clone();
            iced::stream::channel(1, move |mut output: iced::futures::channel::mpsc::Sender<Preview>| async move {
                let setup = async {
                    let _slot = slots.acquire().await?;
                    let id = pointer.room_id.parse::<OwnedRoomId>()?;
                    let mut metadata = service.0.client.subscribe_to_all_room_updates();
                    let room = loop {
                        if let Some(room) = service.0.client.get_room(&id) { break room; }
                        // The visible-page subscription can arrive after this
                        // preview starts in a cold session. Wait for its state.
                        match metadata.recv().await {
                            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {},
                            Err(_) => anyhow::bail!("Room unavailable"),
                        }
                    };
                    let timeline = TimelineBuilder::new(&room)
                        .with_focus(TimelineFocus::Event {
                            target: pointer.event_id.parse()?, num_context_events: 0,
                            thread_mode: matrix_sdk_ui::timeline::TimelineEventFocusThreadMode::Automatic { hide_threaded_events: false },
                        })
                        .track_read_marker_and_receipts(matrix_sdk_ui::timeline::TimelineReadReceiptTracking::Disabled)
                        .build().await?;
                    Ok::<_, anyhow::Error>((room, timeline))
                };
                let ready = tokio::time::timeout(Duration::from_secs(25), setup).await;
                let Ok(Ok((room, timeline))) = ready else {
                    let _ = output.send(Preview { room_name: pointer.room_id.clone(), pointer, sender: String::new(), body: "Message unavailable — access or room keys may be missing.".into(), thread_root: None }).await;
                    return;
                };
                let (_, updates) = timeline.subscribe().await;
                tokio::pin!(updates);
                loop {
                    let rows = crate::connected::timeline_rows(&timeline).await;
                    let row = rows.iter().find(|row| row.event_id.as_deref() == Some(&pointer.event_id));
                    let preview = Preview {
                        pointer: pointer.clone(), room_name: room.name().unwrap_or_else(|| pointer.room_id.clone()),
                        sender: row.map_or(String::new(), |row| row.display_name.clone()),
                        body: row.map_or_else(|| "Message unavailable".into(), |row| row.body.clone()),
                        thread_root: row.and_then(|row| row.thread_root.clone()),
                    };
                    if output.send(preview).await.is_err() || updates.next().await.is_none() { return; }
                }
            }).boxed()
        }).collect::<Vec<_>>();
        iced::futures::stream::select_all(streams).boxed()
    }
    pub async fn set(self, pointer: Pointer, saved: bool) -> Result<(), String> {
        // Writes finish against this account even after UI navigation; the UI
        // counts them toward shutdown. A retry reuses uncertain transaction IDs.
        let result =
            tokio::time::timeout(Duration::from_secs(45), self.write(pointer, saved)).await;
        self.refresh();
        match result {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(_) => Err("Saving timed out. Refresh saved messages before retrying.".into()),
        }
    }
    async fn write(&self, mut pointer: Pointer, saved: bool) -> anyhow::Result<()> {
        let source_id: OwnedRoomId = pointer.room_id.parse()?;
        let event_id: matrix_sdk::ruma::OwnedEventId = pointer.event_id.parse()?;
        let mut writes = self.0.writes.lock().await;
        let snapshot = self.0.snapshots.borrow().clone();
        ensure!(
            snapshot.complete,
            "Bookmarks are still loading or unavailable. Refresh saved messages first."
        );
        let room = self
            .storage_room(&mut writes, saved)
            .await?
            .context("Bookmarks room is unavailable")?;
        validate_room(&room).await?;
        ensure!(
            writes.indexed_room.as_deref() == Some(room.room_id()),
            "The active bookmarks room changed. Refresh saved messages and retry once it finishes loading."
        );
        let existing = snapshot
            .entries
            .iter()
            .find(|entry| entry.pointer.matches(&pointer.room_id, &pointer.event_id));
        if saved && existing.is_some() || !saved && existing.is_none() {
            return Ok(());
        }
        if saved {
            let source = self
                .0
                .client
                .get_room(&source_id)
                .context("Source room is unavailable")?;
            // Timeline rows expose the original ID for edits. Confirm it again
            // when callers supply a raw replacement event ID.
            let original = source.event(&event_id, None).await?;
            let raw: Value = serde_json::from_str(original.raw().json().get())?;
            if raw
                .pointer("/content/m.relates_to/rel_type")
                .and_then(Value::as_str)
                == Some("m.replace")
            {
                pointer.event_id = raw
                    .pointer("/content/m.relates_to/event_id")
                    .and_then(Value::as_str)
                    .context("Invalid edited message")?
                    .to_owned();
            }
            if snapshot.contains(&pointer.room_id, &pointer.event_id) {
                return Ok(());
            }
            let original_id: matrix_sdk::ruma::OwnedEventId = pointer.event_id.parse()?;
            // IDs alone are not routable. Include the account server as a
            // conservative route when a room's own server name is unavailable.
            pointer.via = vec![
                self.0
                    .client
                    .user_id()
                    .context("Signed out")?
                    .server_name()
                    .to_string(),
            ];
            let uri = source_id.matrix_event_uri_via(
                original_id,
                [self.0.client.user_id().context("Signed out")?.server_name()],
            );
            let name = source.name().unwrap_or_else(|| pointer.room_id.clone());
            let fallback = format!("Saved a message in {name}: {uri}");
            let content = json!({POINTER: pointer, "m.text":[{"body":fallback}], "org.matrix.msc1767.text":[{"body":fallback}]});
            let key = format!(
                "save:{}:{}:{}",
                room.room_id(),
                pointer.room_id,
                pointer.event_id
            );
            let transaction = writes
                .transactions
                .entry(key.clone())
                .or_insert_with(TransactionId::new)
                .clone();
            let response = room
                .send_raw(EVENT_TYPE, content.clone())
                .with_transaction_id(&transaction)
                .await?;
            writes.transactions.remove(&key);
            let saved_at = chrono::Utc::now().timestamp_millis();
            self.0.snapshots.send_modify(|snapshot| {
                snapshot.entries.insert(
                    0,
                    Entry {
                        pointer: pointer.clone(),
                        bookmark_ids: vec![response.response.event_id.to_string()],
                        saved_at,
                        fallback: fallback.clone(),
                    },
                )
            });
            writes.confirmed.insert((room.room_id().to_owned(), response.response.event_id.to_string()), json!({
                "type":EVENT_TYPE, "event_id":response.response.event_id, "sender":self.0.client.user_id(),
                "origin_server_ts":chrono::Utc::now().timestamp_millis(), "content":content
            }));
        } else if let Some(existing) = existing {
            for id in &existing.bookmark_ids {
                if writes
                    .removed
                    .contains(&(room.room_id().to_owned(), id.clone()))
                {
                    continue;
                }
                let key = format!("remove:{}:{id}", room.room_id());
                let transaction = writes
                    .transactions
                    .entry(key.clone())
                    .or_insert_with(TransactionId::new)
                    .clone();
                room.redact(
                    &id.parse::<matrix_sdk::ruma::OwnedEventId>()?,
                    None,
                    Some(transaction),
                )
                .await?;
                writes.transactions.remove(&key);
                writes
                    .removed
                    .insert((room.room_id().to_owned(), id.clone()));
                writes
                    .confirmed
                    .remove(&(room.room_id().to_owned(), id.clone()));
            }
            self.0.snapshots.send_modify(|snapshot| {
                snapshot
                    .entries
                    .retain(|entry| !entry.pointer.matches(&pointer.room_id, &pointer.event_id))
            });
        }
        Ok(())
    }
    async fn storage_room(
        &self,
        writes: &mut Writes,
        create: bool,
    ) -> anyhow::Result<Option<Room>> {
        if let Some(id) = fetch_room_id(&self.0.client).await? {
            let mut updates = self.0.client.subscribe_to_all_room_updates();
            self.subscribe_storage(&id).await;
            loop {
                if let Some(room) = self.0.client.get_room(&id) {
                    return Ok(Some(room));
                }
                ensure!(
                    self.0.room_list.get().is_some(),
                    "The saved messages room is unavailable. It has not been replaced."
                );
                // A cold session may not have this room in its room-list range.
                // The explicit subscription supplies metadata independently.
                match updates.recv().await {
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => anyhow::bail!("The saved messages room is unavailable"),
                }
            }
        }
        if !create {
            self.unsubscribe_storage();
            return Ok(None);
        }
        let room = if let Some(id) = &writes.created {
            self.0
                .client
                .get_room(id)
                .context("The newly created bookmarks room is unavailable")?
        } else {
            // Recover a prior interrupted creation rather than creating duplicates.
            let candidates: Vec<_> = self
                .0
                .client
                .joined_rooms()
                .into_iter()
                .filter(is_bookmarks_room)
                .collect();
            ensure!(
                candidates.len() <= 1,
                "Multiple bookmarks rooms exist. Set the active room in another compatible client."
            );
            if let Some(room) = candidates.into_iter().next() {
                room
            } else {
                let mut request = create_room::v3::Request::new();
                request.name = Some("Bookmarks".into());
                request.preset = Some(create_room::v3::RoomPreset::PrivateChat);
                request.creation_content = Some(Raw::from_json_string(
                    json!({"type":ROOM_TYPE}).to_string(),
                )?);
                request.initial_state = vec![Raw::from_json_string(json!({
                    "type":"m.room.encryption", "state_key":"", "content":{"algorithm":"m.megolm.v1.aes-sha2"}
                }).to_string())?];
                let room = self.0.client.create_room(request).await?;
                // A room we just created has no earlier bookmarks to index.
                writes.indexed_room = Some(room.room_id().to_owned());
                room
            }
        };
        writes.created = Some(room.room_id().to_owned());
        // create_room returns before /sync supplies the new room's state.
        // Subscribe explicitly and wait for metadata, without polling or
        // assuming the room is inside the current room-list range.
        self.subscribe_storage(room.room_id()).await;
        let mut updates = self.0.client.subscribe_to_all_room_updates();
        while !is_bookmarks_room(&room) || room.join_rule().is_none() {
            updates
                .recv()
                .await
                .context("Waiting for the new bookmarks room failed")?;
        }
        validate_room(&room).await?;
        // Recheck after creation: another device may have won the race.
        if let Some(id) = fetch_room_id(&self.0.client).await? {
            return self
                .0
                .client
                .get_room(&id)
                .map(Some)
                .context("Another device selected an unavailable bookmarks room");
        }
        self.0
            .client
            .account()
            .set_account_data_raw(
                ACCOUNT_TYPE.into(),
                Raw::from_json_string(
                    json!({"active_bookmarks_room_id":room.room_id()}).to_string(),
                )?,
            )
            .await?;
        Ok(Some(room))
    }
    async fn subscribe_storage(&self, id: &matrix_sdk::ruma::RoomId) {
        if let Some(room_list) = self.0.room_list.get() {
            let previous = {
                let mut current = self
                    .0
                    .subscription
                    .lock()
                    .expect("storage subscription lock");
                if current.as_deref() == Some(id) {
                    return;
                }
                current.replace(id.to_owned())
            };
            if let Some(previous) = previous {
                room_list.remove_room_subscriptions(&[&previous]);
            }
            room_list.set_room_subscriptions(&[id]).await;
        }
    }
    fn unsubscribe_storage(&self) {
        let previous = self
            .0
            .subscription
            .lock()
            .expect("storage subscription lock")
            .take();
        if let (Some(room_list), Some(previous)) = (self.0.room_list.get(), previous) {
            room_list.remove_room_subscriptions(&[&previous]);
        }
    }
}

async fn fetch_room_id(client: &Client) -> anyhow::Result<Option<OwnedRoomId>> {
    for kind in [ACCOUNT_TYPE, "m.bookmarks_room"] {
        if let Some(raw) = client.account().fetch_account_data(kind.into()).await? {
            let content: Value = serde_json::from_str(raw.json().get())?;
            return Ok(Some(
                content
                    .get("active_bookmarks_room_id")
                    .and_then(Value::as_str)
                    .context("Invalid bookmarks room account data")?
                    .parse()?,
            ));
        }
    }
    Ok(None)
}
async fn validate_room(room: &Room) -> anyhow::Result<()> {
    ensure!(
        room.state() == matrix_sdk::RoomState::Joined && is_bookmarks_room(room),
        "The bookmarks room is not a joined bookmarks room"
    );
    ensure!(
        room.latest_encryption_state().await?.is_encrypted(),
        "The bookmarks room is not encrypted"
    );
    ensure!(
        room.join_rule() == Some(matrix_sdk::ruma::events::room::join_rules::JoinRule::Invite),
        "The bookmarks room must be invite-only to keep bookmarks private"
    );
    let members = room.members(matrix_sdk::RoomMemberships::ACTIVE).await?;
    ensure!(
        members.len() == 1 && members[0].user_id() == room.own_user_id(),
        "The bookmarks room contains other people. Saving is disabled to keep bookmarks private."
    );
    ensure!(
        !room.is_tombstoned(),
        "The bookmarks room was upgraded. Select its replacement in a compatible client first."
    );
    Ok(())
}

// The SDK cache preserves custom JSON through decryption and redaction.
// Bookmark replacements are projected here, independently of the SDK's
// room-message-specific UI edit aggregator.
fn read_pointer(content: &Value) -> Option<Pointer> {
    let pointer: Pointer = serde_json::from_value(
        content
            .get(POINTER)
            .or_else(|| content.get("m.pointer"))?
            .clone(),
    )
    .ok()?;
    pointer.room_id.parse::<OwnedRoomId>().ok()?;
    pointer
        .event_id
        .parse::<matrix_sdk::ruma::OwnedEventId>()
        .ok()?;
    Some(pointer)
}
fn event_order(event: &Value) -> (i64, &str) {
    (
        event
            .get("origin_server_ts")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        event
            .get("event_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}
fn project(events: &[Value], removed: &HashSet<String>) -> Vec<Entry> {
    // A redaction can arrive before its target has reached the local cache,
    // including a save acknowledged by HTTP but not yet echoed through sync.
    let redacted: HashSet<_> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("m.room.redaction"))
        .filter_map(|event| {
            event
                .pointer("/content/redacts")
                .or_else(|| event.get("redacts"))
                .and_then(Value::as_str)
        })
        .collect();
    let mut roots = BTreeMap::<&str, &Value>::new();
    for event in events {
        if matches!(
            event.get("type").and_then(Value::as_str),
            Some(EVENT_TYPE | "m.bookmark")
        ) && event.pointer("/unsigned/redacted_because").is_none()
            && event
                .pointer("/content/m.relates_to/rel_type")
                .and_then(Value::as_str)
                != Some("m.replace")
            && let Some(id) = event.get("event_id").and_then(Value::as_str)
            && !removed.contains(id)
            && !redacted.contains(id)
        {
            roots.insert(id, event);
        }
    }
    let mut edits = HashMap::<&str, &Value>::new();
    for edit in events {
        if edit
            .pointer("/content/m.relates_to/rel_type")
            .and_then(Value::as_str)
            != Some("m.replace")
        {
            continue;
        }
        let Some(target) = edit
            .pointer("/content/m.relates_to/event_id")
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(root) = roots.get(target) else {
            continue;
        };
        if edit.get("type") != root.get("type")
            || edit.get("sender") != root.get("sender")
            || edit.pointer("/unsigned/redacted_because").is_some()
            || edit
                .pointer("/content/m.new_content")
                .and_then(read_pointer)
                .is_none()
            || redacted.contains(event_order(edit).1)
        {
            continue;
        }
        let current = edits.entry(target).or_insert(edit);
        if event_order(edit) > event_order(current) {
            *current = edit;
        }
    }
    let mut entries: HashMap<(String, String), Entry> = HashMap::new();
    for (id, root) in roots {
        let content = edits
            .get(id)
            .and_then(|edit| edit.pointer("/content/m.new_content"))
            .unwrap_or(&root["content"]);
        let Some(pointer) = read_pointer(content) else {
            continue;
        };
        let key = (pointer.room_id.clone(), pointer.event_id.clone());
        let entry = entries.entry(key).or_insert_with(|| Entry {
            pointer,
            bookmark_ids: Vec::new(),
            saved_at: event_order(root).0,
            fallback: content
                .get("m.text")
                .or_else(|| content.get("org.matrix.msc1767.text"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|block| {
                    block
                        .get("mimetype")
                        .and_then(Value::as_str)
                        .is_none_or(|mime| mime == "text/plain")
                })
                .and_then(|block| block.get("body"))
                .and_then(Value::as_str)
                .unwrap_or("Saved message")
                .into(),
        });
        entry.saved_at = entry.saved_at.max(event_order(root).0);
        entry.bookmark_ids.push(id.into());
    }
    let mut entries: Vec<_> = entries.into_values().collect();
    for entry in &mut entries {
        entry.bookmark_ids.sort();
    }
    entries.sort_by(|a, b| {
        b.saved_at
            .cmp(&a.saved_at)
            .then_with(|| a.bookmark_ids.cmp(&b.bookmark_ids))
    });
    entries
}

impl Service {
    async fn publish(
        &self,
        cache: &matrix_sdk::event_cache::RoomEventCache,
        complete: bool,
        state: &watch::Sender<Model>,
    ) -> anyhow::Result<()> {
        // Read the authoritative cache after pagination. A UI timeline applies
        // cache diffs asynchronously and could briefly report complete history
        // before its last page has reached the presentation controller.
        let items = cache.events().await?;
        let unknown = items.iter().any(|event| event.kind.is_utd());
        let mut events: Vec<Value> = items
            .iter()
            .filter_map(|event| serde_json::from_str(event.raw().json().get()).ok())
            .collect();
        let mut writes = self.0.writes.lock().await;
        // A redacted item has no original_json. Seeing its ID still retires our
        // acknowledged local overlay, so remote redactions cannot resurrect it.
        writes.confirmed.retain(|(room, id), _| {
            room != cache.room_id()
                || !items
                    .iter()
                    .any(|event| event.event_id().is_some_and(|event| event.as_str() == id))
        });
        for ((room, id), event) in &writes.confirmed {
            if room != cache.room_id() {
                continue;
            }
            if !events
                .iter()
                .any(|event| event.get("event_id").and_then(Value::as_str) == Some(id))
            {
                events.push(event.clone());
            }
        }
        events.retain(|event| {
            event.get("sender").and_then(Value::as_str)
                == self.0.client.user_id().map(|user| user.as_str())
        });
        let removed = writes
            .removed
            .iter()
            .filter(|(room, _)| room == cache.room_id())
            .map(|(_, id)| id.clone())
            .collect();
        let snapshot = Snapshot {
            entries: project(&events, &removed), complete: complete && !unknown, loading: !complete,
            error: unknown.then(|| "Some bookmarks could not be decrypted. Verify this device or restore room keys, then refresh.".into()),
        };
        if snapshot.complete {
            writes.indexed_room = Some(cache.room_id().to_owned());
        }
        self.send(snapshot, state);
        Ok(())
    }
    fn send(&self, snapshot: Snapshot, state: &watch::Sender<Model>) {
        self.0.snapshots.send_if_modified(|old| {
            if *old == snapshot {
                false
            } else {
                *old = snapshot.clone();
                true
            }
        });
        state.send_if_modified(|model| {
            if model.bookmarks == snapshot {
                false
            } else {
                model.bookmarks = snapshot;
                true
            }
        });
    }
    pub(crate) async fn run(
        self,
        sync: &matrix_sdk_ui::sync_service::SyncService,
        state: watch::Sender<Model>,
    ) {
        let _ = self.0.room_list.set(sync.room_list_service());
        struct Cleanup(Service);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                self.0.unsubscribe_storage();
            }
        }
        let _cleanup = Cleanup(self.clone());
        let changes = self.0.clone();
        let handler =
            self.0
                .client
                .add_event_handler(move |event: Raw<AnyGlobalAccountDataEvent>| {
                    let changes = changes.clone();
                    async move {
                        if event
                            .get_field::<String>("type")
                            .ok()
                            .flatten()
                            .is_some_and(|kind| {
                                matches!(kind.as_str(), ACCOUNT_TYPE | "m.bookmarks_room")
                            })
                        {
                            changes.changes.notify_one();
                        }
                    }
                });
        let _guard = self.0.client.event_handler_drop_guard(handler);
        let mut current = None;
        let mut initialized = false;
        let mut room_updates = self.0.client.subscribe_to_all_room_updates();
        let mut display = super::connected::inactive();
        loop {
            self.send(
                Snapshot {
                    loading: true,
                    ..Default::default()
                },
                &state,
            );
            let result = tokio::time::timeout(Duration::from_secs(30), async {
                let mut writes = self.0.writes.lock().await;
                self.storage_room(&mut writes, false).await
            })
            .await;
            match result {
                Ok(Ok(room)) => {
                    let id = room.as_ref().map(|room| room.room_id().to_owned());
                    if id != current || !initialized {
                        initialized = true;
                        drop(display);
                        display = super::connected::inactive();
                        self.send(
                            Snapshot {
                                loading: room.is_some(),
                                complete: room.is_none(),
                                ..Default::default()
                            },
                            &state,
                        );
                        current = id;
                        if let Some(room) = room {
                            self.subscribe_storage(room.room_id()).await;
                            let service = self.clone();
                            let state = state.clone();
                            display = Box::pin(async move {
                                if let Err(error) = service.watch_room(room, &state).await {
                                    let mut snapshot = service.0.snapshots.borrow().clone();
                                    snapshot.complete = false;
                                    snapshot.loading = false;
                                    snapshot.error = Some(error.to_string());
                                    service.send(snapshot, &state);
                                }
                            });
                        }
                    }
                }
                _ => self.send(
                    Snapshot {
                        error: Some(
                            "Could not load saved messages. Check your connection and refresh."
                                .into(),
                        ),
                        ..Default::default()
                    },
                    &state,
                ),
            }
            loop {
                tokio::select! {
                    _ = self.0.changes.notified() => {
                        drop(display);
                        display = super::connected::inactive();
                        current = None;
                        initialized = false;
                        break;
                    },
                    update = room_updates.recv(), if !initialized => {
                        if matches!(update, Err(tokio::sync::broadcast::error::RecvError::Closed)) { return; }
                        break;
                    },
                    _ = &mut display => display = super::connected::inactive(),
                }
            }
        }
    }
    async fn watch_room(&self, room: Room, state: &watch::Sender<Model>) -> anyhow::Result<()> {
        validate_room(&room).await?;
        self.0.client.event_cache().subscribe()?;
        let (cache, _handles) = room.event_cache().await?;
        let (_, mut updates) = cache.subscribe().await?;
        loop {
            // Fully hydrate history and any new gaps before exposing binary
            // bookmark state. The cache owns decryption and raw redactions.
            loop {
                self.publish(&cache, false, state).await?;
                let outcome = tokio::time::timeout(
                    Duration::from_secs(30),
                    cache.pagination().run_backwards_once(100),
                )
                .await
                .context("Loading saved messages timed out")??;
                if outcome.reached_start {
                    break;
                }
                tokio::task::yield_now().await;
            }
            self.publish(&cache, true, state).await?;
            loop {
                match updates.recv().await {
                    Ok(matrix_sdk::event_cache::RoomEventCacheUpdate::UpdateTimelineEvents(_))
                    | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        anyhow::bail!("Saved messages subscription ended")
                    }
                    _ => {}
                }
            }
            // The next iteration reads an authoritative snapshot after these
            // notifications. Raw event diffs stay ordered inside the SDK.
            while updates.try_recv().is_ok() {}
        }
    }
}

#[cfg(test)]
mod tests;
