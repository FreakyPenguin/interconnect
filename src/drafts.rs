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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
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
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drafts")
            .field("count", &self.entries.len())
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
            bytes = bytes.saturating_add(entry.text.len());
            ensure!(
                bytes <= MAX_TEXT_BYTES,
                "Drafts exceed the 8 MiB text storage limit"
            );
        }
        Ok(())
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
            entries: vec![
                Entry {
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
        let mut saved = Snapshot { entries: vec![old] };
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
