//! Encrypted, device-local text drafts. All encryption and filesystem work runs
//! on blocking workers; the UI owns a debounced, serialized writer.
use crate::{composition::Format, custom_emoji::Selection};
use anyhow::{Context, ensure};
use matrix_sdk_store_encryption::StoreCipher;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const FILE: &str = "drafts.json";
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_NOTIFICATIONS: usize = crate::notifications::HISTORY_LIMIT;
const MAX_NOTIFICATION_ID_BYTES: usize = 2048;
const MAX_NOTIFICATION_TITLE_BYTES: usize = 4096;
const MAX_NOTIFICATION_BODY_BYTES: usize = 8192;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll: Option<crate::polls::Creation>,
    pub room: String,
    pub root: Option<String>,
    #[serde(default)]
    pub reply: Option<String>,
    pub text: String,
    pub format: Format,
    pub emoji: Vec<Selection>,
    #[serde(default)]
    pub mentions: Vec<matrix_sdk::ruma::OwnedUserId>,
    #[serde(default)]
    pub named_mentions: Vec<crate::composition::NamedMention>,
}
/// A stable event at the viewport's top, with its signed pixel displacement.
/// Newest-following conversations have no saved entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingPosition {
    pub room: String,
    pub root: Option<String>,
    pub event: String,
    pub offset: i32,
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub reading_positions: Vec<ReadingPosition>,
    #[serde(default)]
    pub notifications: Vec<crate::notifications::Record>,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drafts")
            .field("count", &self.entries.len())
            .field("notifications", &self.notifications.len())
            .finish()
    }
}
impl Snapshot {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(self.entries.len() <= 4096, "Too many drafts to save");
        let mut keys = HashSet::new();
        let mut bytes = 0_usize;
        for entry in &self.entries {
            ensure!(
                entry.room.parse::<matrix_sdk::ruma::OwnedRoomId>().is_ok(),
                "Invalid draft room"
            );
            ensure!(
                entry
                    .root
                    .as_ref()
                    .is_none_or(|root| root.parse::<matrix_sdk::ruma::OwnedEventId>().is_ok()),
                "Invalid draft thread"
            );
            ensure!(
                entry
                    .reply
                    .as_ref()
                    .is_none_or(|id| id.parse::<matrix_sdk::ruma::OwnedEventId>().is_ok()),
                "Invalid draft reply"
            );
            ensure!(
                keys.insert((&entry.room, &entry.root)),
                "Duplicate draft conversation"
            );
            ensure!(
                entry.emoji.len() <= 256
                    && entry
                        .emoji
                        .iter()
                        .all(|choice| !choice.token.is_empty() && choice.emoji.uri.is_valid()),
                "Invalid draft emoji"
            );
            ensure!(entry.mentions.len() <= 256, "Too many draft mentions");
            ensure!(
                entry.named_mentions.len() <= 256
                    && entry
                        .named_mentions
                        .iter()
                        .all(|mention| mention.valid(&entry.text))
                    && entry
                        .named_mentions
                        .windows(2)
                        .all(|pair| pair[0].range.end <= pair[1].range.start),
                "Invalid named draft mentions"
            );
            if let Some(poll) = &entry.poll {
                ensure!(
                    (2..=20).contains(&poll.answers.len())
                        && (1..=poll.answers.len() as u64).contains(&poll.max_selections),
                    "Invalid poll draft options"
                );
                bytes = bytes.saturating_add(poll.question.len());
                for answer in &poll.answers {
                    bytes = bytes.saturating_add(answer.len());
                }
            }
            bytes = bytes.saturating_add(entry.text.len());
            ensure!(
                bytes <= MAX_TEXT_BYTES,
                "Drafts exceed the 8 MiB text storage limit"
            );
        }
        ensure!(
            self.reading_positions.len() <= 4096,
            "Too many reading positions"
        );
        let mut positions = HashSet::new();
        for position in &self.reading_positions {
            ensure!(
                position
                    .room
                    .parse::<matrix_sdk::ruma::OwnedRoomId>()
                    .is_ok()
                    && position
                        .root
                        .as_ref()
                        .is_none_or(|root| root.parse::<matrix_sdk::ruma::OwnedEventId>().is_ok())
                    && position
                        .event
                        .parse::<matrix_sdk::ruma::OwnedEventId>()
                        .is_ok()
                    && position.offset.unsigned_abs() <= 1_000_000,
                "Invalid reading position"
            );
            ensure!(
                positions.insert((&position.room, &position.root)),
                "Duplicate reading position"
            );
        }
        ensure!(
            self.notifications.len() <= MAX_NOTIFICATIONS,
            "Too many saved notifications"
        );
        let mut notifications = HashSet::new();
        for record in &self.notifications {
            let entry = &record.entry;
            ensure!(valid_notification(entry), "Invalid saved notification");
            ensure!(
                notifications.insert(&entry.id),
                "Duplicate saved notification"
            );
        }
        Ok(())
    }
}
/// Validate an individual inbox entry before accepting it into local history.
/// Producers can discard malformed remote metadata without blocking draft saves.
pub fn valid_notification(entry: &crate::notifications::Entry) -> bool {
    if entry.id.is_empty()
        || entry.id.len() > MAX_NOTIFICATION_ID_BYTES
        || entry.id.chars().any(char::is_control)
        || entry.title.len() > MAX_NOTIFICATION_TITLE_BYTES
        || entry.body.len() > MAX_NOTIFICATION_BODY_BYTES
    {
        return false;
    }
    use crate::notifications::Target;
    match &entry.target {
        Target::Conversation {
            room,
            thread,
            event,
        } => {
            room.len() <= 1024
                && room.parse::<matrix_sdk::ruma::OwnedRoomId>().is_ok()
                && thread.as_ref().is_none_or(|thread| {
                    thread.len() <= 1024 && thread.parse::<matrix_sdk::ruma::OwnedEventId>().is_ok()
                })
                && event.as_ref().is_none_or(|event| {
                    event.len() <= 1024
                        && !event.chars().any(char::is_control)
                        && event.parse::<matrix_sdk::ruma::OwnedEventId>().is_ok()
                })
        }
        Target::Invitation { room } => {
            room.len() <= 1024 && room.parse::<matrix_sdk::ruma::OwnedRoomId>().is_ok()
        }
        Target::Verification { flow, device } => [flow, device].into_iter().all(|id| {
            !id.trim().is_empty() && id.len() <= 1024 && !id.chars().any(char::is_control)
        }),
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    account: String,
    drafts: Snapshot,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    key_id: String,
    cipher: Vec<u8>,
    value: Vec<u8>,
}

pub struct Store {
    id: u64,
    path: PathBuf,
    account: String,
    key_id: String,
    exported: Vec<u8>,
    cipher: StoreCipher,
}
impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DraftStore")
    }
}
#[derive(Clone, Debug)]
pub struct Loaded {
    pub store: Arc<Store>,
    pub saved: Arc<Snapshot>,
}
impl Store {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub async fn open(
        directory: PathBuf,
        account: String,
        current_key: String,
        keys: HashMap<String, String>,
    ) -> Result<Loaded, String> {
        tokio::task::spawn_blocking(move || {
            Self::open_blocking(directory, account, current_key, keys)
        })
        .await
        .map_err(|_| "Draft restoration worker failed.".to_owned())?
        .map_err(|error| {
            format!("Could not restore saved drafts: {error:#}. The saved file was retained.")
        })
    }
    fn open_blocking(
        directory: PathBuf,
        account: String,
        current_key: String,
        keys: HashMap<String, String>,
    ) -> anyhow::Result<Loaded> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = directory.join(FILE);
        let mut saved = Snapshot::default();
        if let Some(bytes) = read(&path)? {
            let envelope: Envelope =
                serde_json::from_slice(&bytes).context("Invalid draft file")?;
            ensure!(envelope.version == 1, "Unsupported draft file version");
            let passphrase = keys
                .get(&envelope.key_id)
                .context("Draft encryption key is unavailable")?;
            let cipher = StoreCipher::import(passphrase, &envelope.cipher)
                .context("Could not unlock drafts")?;
            let record: Record = cipher
                .decrypt_value(&envelope.value)
                .context("Could not decrypt drafts")?;
            record.drafts.validate()?;
            // A profile may later sign in as another user. Never expose their drafts.
            if record.account == account {
                saved = record.drafts;
            }
        }
        // Rewrap with the current device key; retained keys allow same-account
        // reauthentication to recover drafts from an earlier encrypted store.
        let cipher = StoreCipher::new()?;
        let exported = cipher.export(
            keys.get(&current_key)
                .context("Draft encryption key is unavailable")?,
        )?;
        Ok(Loaded {
            store: Arc::new(Self {
                id: NEXT.fetch_add(1, Ordering::Relaxed),
                path,
                account,
                key_id: current_key,
                exported,
                cipher,
            }),
            saved: Arc::new(saved),
        })
    }
    pub async fn save(self: Arc<Self>, drafts: Arc<Snapshot>) -> Result<(), String> {
        tokio::task::spawn_blocking(move || self.save_blocking(&drafts))
            .await
            .map_err(|_| "Draft saving worker failed.".to_owned())?
            .map_err(|error| format!("Could not save drafts: {error:#}"))
    }
    fn save_blocking(&self, drafts: &Snapshot) -> anyhow::Result<()> {
        drafts.validate()?;
        let value = self.cipher.encrypt_value(&Record {
            account: self.account.clone(),
            drafts: drafts.clone(),
        })?;
        let bytes = serde_json::to_vec(&Envelope {
            version: 1,
            key_id: self.key_id.clone(),
            cipher: self.exported.clone(),
            value,
        })?;
        ensure!(
            bytes.len() as u64 <= MAX_FILE_BYTES,
            "Draft file exceeds the storage limit"
        );
        let directory = self
            .path
            .parent()
            .context("Draft directory is unavailable")?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".drafts-")
            .tempfile_in(directory)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        fs::File::open(directory)?.sync_all()?;
        Ok(())
    }
}
fn read(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) => ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.permissions().mode() & 0o077 == 0
                && metadata.len() <= MAX_FILE_BYTES,
            "Invalid draft file or permissions"
        ),
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "Draft file exceeds the storage limit"
    );
    Ok(Some(bytes))
}

/// Called after remote revocation and before deleting credential keys.
pub(crate) async fn remove(directory: PathBuf) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || match fs::remove_file(directory.join(FILE)) {
        Ok(()) => fs::File::open(directory)?
            .sync_all()
            .context("Flush draft removal"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::Error::new(error).context("Remove saved drafts")),
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn keys() -> HashMap<String, String> {
        HashMap::from([("store".into(), "fixture-only-store-key".into())])
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            notifications: vec![],
            reading_positions: vec![ReadingPosition {
                room: "!room:example.org".into(),
                root: Some("$thread".into()),
                event: "$reading".into(),
                offset: -27,
            }],
            entries: vec![
                Entry {
                    poll: None,
                    room: "!room:example.org".into(),
                    root: None,
                    reply: Some("$original".into()),
                    text: "unfinished private fixture 🌱\nsecond line".into(),
                    format: Format::Plain,
                    emoji: vec![],
                    mentions: vec![],
                    named_mentions: vec![],
                },
                Entry {
                    poll: None,
                    room: "!room:example.org".into(),
                    root: Some("$thread".into()),
                    reply: None,
                    text: "thread draft".into(),
                    format: Format::Markdown,
                    emoji: vec![],
                    mentions: vec![],
                    named_mentions: vec![],
                },
            ],
        }
    }
    fn notification_records() -> Vec<crate::notifications::Record> {
        use crate::notifications::{Entry, Record, Target};
        [
            Target::Conversation {
                room: "!private-inbox-fixture:example.org".into(),
                thread: Some("$private-inbox-thread-fixture".into()),
                event: Some("$private-inbox-event-fixture".into()),
            },
            Target::Invitation {
                room: "!private-invitation-fixture:example.org".into(),
            },
            Target::Verification {
                flow: "private-verification-flow-fixture".into(),
                device: "PRIVATE_DEVICE_FIXTURE".into(),
            },
        ]
        .into_iter()
        .enumerate()
        .map(|(index, target)| Record {
            entry: Entry {
                id: format!("private-notification-id-fixture-{index}"),
                target,
                timestamp: 1_799_999_000_000 + index as u64,
                title: "Private inbox title fixture 🌱".into(),
                body: "Private notification body fixture\nsecond line".into(),
            },
            read: index % 2 == 0,
        })
        .collect()
    }
    #[test]
    fn earlier_snapshot_formats_default_to_no_notification_history() {
        let oldest: Snapshot = serde_json::from_value(serde_json::json!({"entries":[]})).unwrap();
        assert!(oldest.notifications.is_empty());
        let mut current = serde_json::to_value(snapshot()).unwrap();
        current.as_object_mut().unwrap().remove("notifications");
        let earlier: Snapshot = serde_json::from_value(current).unwrap();
        assert!(earlier.notifications.is_empty());
        assert_eq!(earlier.reading_positions, snapshot().reading_positions);
        assert!(earlier.validate().is_ok());
    }
    #[test]
    fn older_notification_targets_default_to_conversation_navigation() {
        use crate::notifications::Target;
        let mut saved = snapshot();
        saved.notifications = notification_records();
        let mut old = serde_json::to_value(&saved).unwrap();
        old["notifications"][0]["entry"]["target"]
            .as_object_mut()
            .unwrap()
            .remove("event");
        let restored: Snapshot = serde_json::from_value(old).unwrap();
        assert!(restored.validate().is_ok());
        assert!(matches!(
            &restored.notifications[0].entry.target,
            Target::Conversation { event: None, .. }
        ));
        assert_eq!(restored.notifications[0].read, saved.notifications[0].read);
        let serialized = serde_json::to_value(&restored).unwrap();
        assert!(
            serialized["notifications"][0]["entry"]["target"]
                .get("event")
                .is_none()
        );
        let serialized = serde_json::to_value(&saved).unwrap();
        assert_eq!(
            serialized["notifications"][0]["entry"]["target"]["event"],
            "$private-inbox-event-fixture"
        );
    }
    #[test]
    fn notification_history_bounds_and_duplicate_ids_are_enforced() {
        let mut saved = snapshot();
        let record = notification_records().remove(0);
        saved.notifications = (0..MAX_NOTIFICATIONS)
            .map(|index| {
                let mut record = record.clone();
                record.entry.id = format!("bounded-notification-{index}");
                record
            })
            .collect();
        assert!(saved.validate().is_ok());
        saved.notifications.push(record.clone());
        assert!(saved.validate().is_err());
        saved.notifications = vec![record.clone(), record.clone()];
        assert!(saved.validate().is_err());
        for (field, limit) in [
            ("id", MAX_NOTIFICATION_ID_BYTES),
            ("title", MAX_NOTIFICATION_TITLE_BYTES),
            ("body", MAX_NOTIFICATION_BODY_BYTES),
        ] {
            saved.notifications = vec![record.clone()];
            let entry = &mut saved.notifications[0].entry;
            match field {
                "id" => entry.id = "x".repeat(limit + 1),
                "title" => entry.title = "x".repeat(limit + 1),
                _ => entry.body = "x".repeat(limit + 1),
            }
            assert!(saved.validate().is_err(), "oversized {field}");
        }
        for invalid in ["", "invalid\nidentity"] {
            saved.notifications = vec![record.clone()];
            saved.notifications[0].entry.id = invalid.into();
            assert!(saved.validate().is_err());
        }
    }
    #[test]
    fn notification_targets_validate_rooms_threads_and_verification_identities() {
        use crate::notifications::Target;
        let mut saved = snapshot();
        let valid = notification_records();
        saved.notifications = valid.clone();
        assert!(saved.validate().is_ok());
        for invalid in [
            Target::Conversation {
                room: "invalid".into(),
                thread: None,
                event: None,
            },
            Target::Conversation {
                room: "!room:example.org".into(),
                thread: Some("invalid".into()),
                event: None,
            },
            Target::Conversation {
                room: "!room:example.org".into(),
                thread: None,
                event: Some("invalid".into()),
            },
            Target::Conversation {
                room: "!room:example.org".into(),
                thread: None,
                event: Some(format!("${}", "x".repeat(1024))),
            },
            Target::Conversation {
                room: "!room:example.org".into(),
                thread: None,
                event: Some("$event\n".into()),
            },
            Target::Invitation {
                room: "invalid".into(),
            },
            Target::Verification {
                flow: "".into(),
                device: "DEVICE".into(),
            },
            Target::Verification {
                flow: "flow".into(),
                device: "".into(),
            },
            Target::Verification {
                flow: "f".repeat(1025),
                device: "DEVICE".into(),
            },
            Target::Verification {
                flow: "flow".into(),
                device: "d".repeat(1025),
            },
            Target::Verification {
                flow: "flow\n".into(),
                device: "DEVICE".into(),
            },
        ] {
            saved.notifications = vec![valid[0].clone()];
            saved.notifications[0].entry.target = invalid;
            assert!(saved.validate().is_err());
        }
    }
    #[tokio::test]
    async fn inbox_history_and_read_state_are_encrypted_and_account_isolated_across_restart() {
        let directory = tempfile::tempdir().unwrap();
        let mut saved = snapshot();
        saved.notifications = notification_records();
        open(directory.path(), "@me:example.org")
            .await
            .unwrap()
            .store
            .save(Arc::new(saved.clone()))
            .await
            .unwrap();
        let bytes = fs::read(directory.path().join(FILE)).unwrap();
        for private in [
            "private-notification-id-fixture",
            "Private inbox title fixture",
            "Private notification body fixture",
            "!private-inbox-fixture:example.org",
            "$private-inbox-thread-fixture",
            "private-verification-flow-fixture",
            "PRIVATE_DEVICE_FIXTURE",
        ] {
            assert!(
                !bytes
                    .windows(private.len())
                    .any(|window| window == private.as_bytes()),
                "inbox fields must be encrypted"
            );
        }
        let restored = open(directory.path(), "@me:example.org").await.unwrap();
        assert_eq!(*restored.saved, saved);
        assert_eq!(
            restored
                .saved
                .notifications
                .iter()
                .map(|record| record.read)
                .collect::<Vec<_>>(),
            [true, false, true]
        );
        let other = open(directory.path(), "@other:example.org").await.unwrap();
        assert!(other.saved.notifications.is_empty());
        saved.notifications[1].read = true;
        restored.store.save(Arc::new(saved.clone())).await.unwrap();
        assert_eq!(
            *open(directory.path(), "@me:example.org")
                .await
                .unwrap()
                .saved,
            saved
        );
        let mut invalid = saved.clone();
        invalid.notifications[0].entry.body = "x".repeat(MAX_NOTIFICATION_BODY_BYTES + 1);
        let store = open(directory.path(), "@me:example.org")
            .await
            .unwrap()
            .store;
        assert!(store.save(Arc::new(invalid)).await.is_err());
        assert_eq!(
            *open(directory.path(), "@me:example.org")
                .await
                .unwrap()
                .saved,
            saved,
            "invalid histories must preserve the previous saved file"
        );
        let diagnostics = format!(
            "{:?}",
            open(directory.path(), "@me:example.org")
                .await
                .unwrap()
                .saved
        );
        assert!(!diagnostics.contains("Private"));
    }
    #[test]
    fn old_snapshots_default_to_no_reading_positions_and_invalid_anchors_are_rejected() {
        let old: Snapshot = serde_json::from_value(serde_json::json!({"entries": []})).unwrap();
        assert!(old.reading_positions.is_empty());
        for invalid in ["event", ""] {
            let mut saved = snapshot();
            saved.reading_positions[0].event = invalid.into();
            assert!(saved.validate().is_err());
        }
        let mut saved = snapshot();
        saved.reading_positions[0].offset = i32::MIN;
        assert!(saved.validate().is_err());
        let mut saved = snapshot();
        saved
            .reading_positions
            .push(saved.reading_positions[0].clone());
        assert!(saved.validate().is_err());
    }
    #[test]
    fn invalid_reply_ids_are_rejected() {
        let mut saved = snapshot();
        saved.entries[0].reply = Some("invalid".into());
        assert!(saved.validate().is_err());
    }
    #[test]
    fn old_drafts_default_to_no_mentions_and_new_mentions_are_bounded() {
        let old: Entry = serde_json::from_value(serde_json::json!({
            "room":"!room:example.org", "root":null, "text":"@maya:example.org", "format":"Plain", "emoji":[]
        })).unwrap();
        assert!(old.mentions.is_empty());
        assert!(old.reply.is_none());
        let mut saved = Snapshot {
            entries: vec![old],
            ..Default::default()
        };
        saved.entries[0].mentions = vec!["@maya:example.org".parse().unwrap(); 257];
        assert!(saved.validate().is_err());
    }
    async fn open(path: &Path, account: &str) -> Result<Loaded, String> {
        Store::open(path.to_owned(), account.into(), "store".into(), keys()).await
    }

    #[tokio::test]
    async fn encrypted_drafts_round_trip_across_restart_without_plaintext_or_idle_files() {
        let directory = tempfile::tempdir().unwrap();
        let loaded = open(directory.path(), "@me:example.org").await.unwrap();
        assert!(!directory.path().join(FILE).exists());
        loaded.store.save(Arc::new(snapshot())).await.unwrap();
        let bytes = fs::read(directory.path().join(FILE)).unwrap();
        assert!(
            !bytes
                .windows(b"unfinished private fixture".len())
                .any(|w| w == b"unfinished private fixture")
        );
        assert!(
            !bytes
                .windows(b"$reading".len())
                .any(|window| window == b"$reading")
        );
        assert_eq!(
            fs::metadata(directory.path().join(FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let restored = open(directory.path(), "@me:example.org").await.unwrap();
        assert_eq!(*restored.saved, snapshot());
        let other = open(directory.path(), "@other:example.org").await.unwrap();
        assert!(other.saved.entries.is_empty());
        restored
            .store
            .save(Arc::new(Snapshot::default()))
            .await
            .unwrap();
        assert!(
            open(directory.path(), "@me:example.org")
                .await
                .unwrap()
                .saved
                .entries
                .is_empty()
        );
    }

    #[tokio::test]
    async fn retained_keys_recover_same_account_drafts_after_reauthentication() {
        let directory = tempfile::tempdir().unwrap();
        open(directory.path(), "@me:example.org")
            .await
            .unwrap()
            .store
            .save(Arc::new(snapshot()))
            .await
            .unwrap();
        let mut retained = keys();
        retained.insert("store-new".into(), "new-fixture-key".into());
        let loaded = Store::open(
            directory.path().to_owned(),
            "@me:example.org".into(),
            "store-new".into(),
            retained,
        )
        .await
        .unwrap();
        assert_eq!(*loaded.saved, snapshot());
        loaded.store.save(loaded.saved).await.unwrap();
        let restored = Store::open(
            directory.path().to_owned(),
            "@me:example.org".into(),
            "store-new".into(),
            HashMap::from([("store-new".into(), "new-fixture-key".into())]),
        )
        .await
        .unwrap();
        assert_eq!(*restored.saved, snapshot());
    }

    #[tokio::test]
    async fn invalid_or_unreadable_files_are_retained_instead_of_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        open(directory.path(), "@me:example.org")
            .await
            .unwrap()
            .store
            .save(Arc::new(snapshot()))
            .await
            .unwrap();
        let path = directory.path().join(FILE);
        let original = fs::read(&path).unwrap();
        assert!(
            Store::open(
                directory.path().to_owned(),
                "@me:example.org".into(),
                "store".into(),
                HashMap::from([("store".into(), "wrong-key".into())])
            )
            .await
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(open(directory.path(), "@me:example.org").await.is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&path, b"damaged draft envelope").unwrap();
        assert!(open(directory.path(), "@me:example.org").await.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"damaged draft envelope");
    }

    #[tokio::test]
    async fn failed_saves_keep_previous_drafts_and_cleanup_removes_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let loaded = open(directory.path(), "@me:example.org").await.unwrap();
        loaded
            .store
            .clone()
            .save(Arc::new(snapshot()))
            .await
            .unwrap();
        let mut invalid = snapshot();
        invalid.entries[0].room = "invalid".into();
        assert!(loaded.store.save(Arc::new(invalid)).await.is_err());
        assert_eq!(
            *open(directory.path(), "@me:example.org")
                .await
                .unwrap()
                .saved,
            snapshot()
        );
        remove(directory.path().to_owned()).await.unwrap();
        assert!(!directory.path().join(FILE).exists());
        remove(directory.path().to_owned()).await.unwrap();
    }
}
