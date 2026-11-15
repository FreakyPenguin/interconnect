//! Small, versioned UI preferences in the already-locked account profile.
use crate::{
    connected::{Model, Selection},
    navigation::Scope,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    pub version: u32,
    pub scope: Scope,
    pub room: Option<String>,
    pub collapsed: Vec<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            scope: Scope::All,
            room: None,
            collapsed: Vec::new(),
        }
    }
}
impl Preferences {
    pub fn load(directory: &Path) -> anyhow::Result<Self> {
        let path = directory.join("navigation.json");
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
            Ok(meta) => anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 262_144,
                "invalid navigation preferences file"
            ),
        }
        let preferences: Self = serde_json::from_slice(&fs::read(path)?)?;
        anyhow::ensure!(
            preferences.version == 1,
            "unsupported navigation preferences version"
        );
        Ok(preferences)
    }
    fn save(&self, directory: &Path) -> anyhow::Result<()> {
        let temporary = directory.join(format!(".navigation-{:016x}.tmp", rand::random::<u64>()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(&serde_json::to_vec_pretty(self)?)?;
            file.sync_all()?;
            fs::rename(&temporary, directory.join("navigation.json"))?;
            fs::File::open(directory)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

/// One owned writer coalesces selections, does no periodic work, and flushes the
/// latest preference value before returning on shutdown. No SDK handles are held.
pub async fn persist(
    directory: PathBuf,
    mut selection: watch::Receiver<Selection>,
    state: watch::Sender<Model>,
    mut last: Preferences,
    stop: CancellationToken,
) {
    loop {
        let next = selection.borrow_and_update().clone();
        if let Some(preferences) = next.preferences
            && preferences != last
        {
            let value = preferences.clone();
            let directory = directory.clone();
            let result = tokio::task::spawn_blocking(move || value.save(&directory)).await;
            let error = if matches!(result, Ok(Ok(()))) {
                last = preferences;
                None
            } else {
                Some(
                    "Could not save navigation. It will be retried on your next navigation change."
                        .to_owned(),
                )
            };
            state.send_if_modified(|s| {
                if s.preferences_error == error {
                    false
                } else {
                    s.preferences_error = error;
                    true
                }
            });
        }
        if selection.has_changed().unwrap_or(false) {
            continue;
        }
        if next.shutdown || stop.is_cancelled() {
            return;
        }
        tokio::select! {
            result = selection.changed() => if result.is_err() { return; },
            _ = stop.cancelled() => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_and_invalid_files_are_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            Preferences::load(directory.path()).unwrap(),
            Preferences::default()
        );
        let prefs = Preferences {
            scope: Scope::Space("!space:example.org".into()),
            room: Some("!room:example.org".into()),
            collapsed: vec!["space/Rooms".into()],
            ..Default::default()
        };
        prefs.save(directory.path()).unwrap();
        assert_eq!(Preferences::load(directory.path()).unwrap(), prefs);
        assert_eq!(
            fs::metadata(directory.path().join("navigation.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::write(directory.path().join("navigation.json"), b"broken json").unwrap();
        assert!(Preferences::load(directory.path()).is_err());
        let mut unknown = prefs;
        unknown.version = 2;
        unknown.save(directory.path()).unwrap();
        assert!(Preferences::load(directory.path()).is_err());
    }
    #[tokio::test]
    async fn writer_flushes_latest_selection_on_shutdown() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, receiver) = watch::channel(Selection::default());
        let (state, _) = watch::channel(Model::default());
        let stop = CancellationToken::new();
        let writer = tokio::spawn(persist(
            directory.path().into(),
            receiver,
            state,
            Preferences::default(),
            stop,
        ));
        for room in ["one", "two", "three"] {
            sender.send_modify(|s| {
                s.preferences = Some(Preferences {
                    room: Some(room.into()),
                    ..Default::default()
                })
            });
        }
        sender.send_modify(|s| s.shutdown = true);
        tokio::time::timeout(std::time::Duration::from_secs(5), writer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            Preferences::load(directory.path()).unwrap().room.as_deref(),
            Some("three")
        );
    }
}
