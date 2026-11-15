//! One-account persistent-session feasibility harness, backed by Secret Service.
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use anyhow::{Context, bail, ensure};
use matrix_sdk::{
    Client, authentication::matrix::MatrixSession, sliding_sync::VersionBuilder,
    utils::local_server::LocalServerBuilder,
};
use secret_service::{EncryptionType, SecretService};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

// Keep the original keyring namespace so existing saved accounts remain readable.
const APPLICATION: &str = "chatclient-milestone-0";

// Never derive Debug: this object contains bearer tokens and an encryption key.
#[derive(Serialize, Deserialize)]
struct SavedSession {
    version: u32,
    homeserver: String,
    store_key: String,
    session: Option<MatrixSession>,
    logged_out: bool,
    device_key: Option<String>,
}

impl SavedSession {
    fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let saved: Self =
            serde_json::from_slice(bytes).context("invalid Secret Service session record")?;
        ensure!(saved.version == 1, "unsupported session record version");
        ensure!(
            !saved.store_key.is_empty(),
            "session record lacks its store key"
        );
        Ok(saved)
    }
}

struct Profile {
    directory: PathBuf,
    id: String,
    _lock: File,
}

impl Profile {
    fn open(path: &Path, create: bool) -> anyhow::Result<Self> {
        if create && !path.exists() {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(path)
                .context("create profile directory (parent must already exist)")?;
        }
        let meta = fs::symlink_metadata(path).context("profile directory does not exist")?;
        ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "profile must be a real directory"
        );
        ensure!(
            meta.permissions().mode() & 0o077 == 0,
            "profile directory must have mode 0700"
        );
        let directory = path.canonicalize()?;
        let lock_path = directory.join("session.lock");
        if let Ok(meta) = fs::symlink_metadata(&lock_path) {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "invalid profile lock file"
            );
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(lock_path)?;
        lock.try_lock()
            .context("profile is already in use by another process")?;
        let id = directory
            .to_str()
            .context("profile path must be UTF-8")?
            .to_owned();
        Ok(Self {
            directory,
            id,
            _lock: lock,
        })
    }

    /// Persistent stores use SDK SingleProcess mode. Keep the OS lock until
    /// process exit, including late UI handles and timed-out runtime cleanup.
    /// This prototype opens one account per process; reopening requires restart.
    fn retain_lock_until_exit(&self) -> anyhow::Result<()> {
        static LOCKS: OnceLock<Mutex<HashMap<PathBuf, File>>> = OnceLock::new();
        let mut locks = LOCKS.get_or_init(Mutex::default).lock().unwrap();
        if !locks.contains_key(&self.directory) {
            locks.insert(self.directory.clone(), self._lock.try_clone()?);
        }
        // Static values are not dropped at shutdown. The kernel releases the
        // duplicated descriptor when the process exits; never unlock it early.
        Ok(())
    }

    fn attributes(&self) -> HashMap<&str, &str> {
        HashMap::from([("application", APPLICATION), ("profile", self.id.as_str())])
    }

    async fn read(&self, service: &SecretService<'_>) -> anyhow::Result<Option<SavedSession>> {
        let found = service.search_items(self.attributes()).await?;
        let mut items = found.unlocked;
        items.extend(found.locked);
        ensure!(
            items.len() <= 1,
            "multiple Secret Service records match this profile; refusing to choose"
        );
        let Some(item) = items.first() else {
            return Ok(None);
        };
        item.unlock().await.context("unlock session keyring item")?;
        let bytes = Zeroizing::new(item.get_secret().await?);
        Ok(Some(SavedSession::decode(&bytes)?))
    }

    async fn save(&self, service: &SecretService<'_>, saved: &SavedSession) -> anyhow::Result<()> {
        let collection = service
            .get_default_collection()
            .await
            .context("Secret Service needs a default collection")?;
        collection
            .unlock()
            .await
            .context("unlock the host keyring")?;
        let bytes = Zeroizing::new(serde_json::to_vec(saved)?);
        collection
            .create_item(
                "interconnect milestone 0 session",
                self.attributes(),
                &bytes,
                true,
                "application/json",
            )
            .await?;
        Ok(())
    }

    async fn delete_secret(&self, service: &SecretService<'_>) -> anyhow::Result<()> {
        let found = service.search_items(self.attributes()).await?;
        for item in found.unlocked.iter().chain(found.locked.iter()) {
            item.unlock().await?;
            item.delete().await?;
        }
        Ok(())
    }
}

async fn connect() -> anyhow::Result<SecretService<'static>> {
    SecretService::connect(EncryptionType::Dh).await.context(
        "Secret Service unavailable: expose the host session bus and unlock its keyring; no plaintext fallback is used"
    )
}

async fn client(profile: &Profile, saved: &SavedSession) -> anyhow::Result<Client> {
    // Retain before opening SQLite, including build failures/cancellation.
    profile.retain_lock_until_exit()?;
    Ok(Client::builder()
        .cross_process_store_config(
            matrix_sdk_common::cross_process_lock::CrossProcessLockConfig::SingleProcess,
        )
        .homeserver_url(&saved.homeserver)
        // Restoration must open the same local store even when offline.
        // Native support is verified by the login discovery step.
        .sliding_sync_version_builder(VersionBuilder::Native)
        .with_threading_support(matrix_sdk::ThreadingSupport::Enabled {
            with_subscriptions: false,
        })
        .sqlite_store(profile.directory.join("store"), Some(&saved.store_key))
        .build()
        .await?)
}

pub async fn login(homeserver: &str, directory: &str) -> anyhow::Result<()> {
    let service = connect().await?;
    let profile = Profile::open(Path::new(directory), true)?;
    ensure!(
        profile.read(&service).await?.is_none(),
        "profile already has a keyring record; use session-resume"
    );
    ensure!(
        !profile.directory.join("store").exists(),
        "existing SDK store has no session record; use a new profile directory rather than replacing its keys"
    );
    // Validate capabilities before creating a persistent record or device.
    Client::builder()
        .homeserver_url(homeserver)
        .sliding_sync_version_builder(VersionBuilder::DiscoverNative)
        .build()
        .await?;
    let key: [u8; 32] = rand::random();
    let store_key = key.iter().map(|b| format!("{b:02x}")).collect();
    let mut saved = SavedSession {
        version: 1,
        homeserver: homeserver.to_owned(),
        store_key,
        session: None,
        logged_out: false,
        device_key: None,
    };
    // Prove keyring writes work and persist the store key BEFORE browser login.
    profile.save(&service, &saved).await?;
    let client = client(&profile, &saved).await?;
    println!("Persistent login: forward localhost port 8765 when running in a container.");
    let auth = client.matrix_auth();
    let login = auth
        .login_sso(|url| async move {
            println!("Open this URL in your browser:\n{url}");
            Ok(())
        })
        .server_builder(
            LocalServerBuilder::new()
                .port_range(8765..8766)
                .bind_tries(1),
        )
        .initial_device_display_name("interconnect milestone-0 persistent session")
        .send();
    tokio::time::timeout(Duration::from_secs(180), login)
        .await
        .context(
            "SSO timed out; profile remains reserved; use a new directory for another attempt",
        )??;
    saved.session = Some(auth.session().context("SDK did not return a session")?);
    saved.homeserver = client.homeserver().to_string();
    // Save before any sync work, so later network failure cannot discard login.
    profile.save(&service, &saved).await.context("could not save login; remove the named device in your other client before retrying with a new profile")?;
    saved.device_key = Some(device_key(&client).await?);
    profile.save(&service, &saved).await?;
    println!("Session saved in Secret Service. Closing this process keeps the device logged in.");
    verify_identity(&client, saved.session.as_ref().expect("session set")).await?;
    crate::sso::initial_sync(client).await
}

pub async fn resume(directory: &str, logout: bool) -> anyhow::Result<()> {
    resume_inner(directory, logout, false).await
}

pub async fn live(directory: &str) -> anyhow::Result<()> {
    resume_inner(directory, false, true).await
}

async fn resume_inner(directory: &str, logout: bool, live: bool) -> anyhow::Result<()> {
    let service = connect().await?;
    let profile = Profile::open(Path::new(directory), false)?;
    let mut saved = profile
        .read(&service)
        .await?
        .context("no saved session for this profile")?;
    if saved.logged_out {
        ensure!(
            logout,
            "session was logged out; rerun session-logout to finish local cleanup"
        );
        profile.delete_secret(&service).await?;
        println!("Local credential cleanup completed; SDK store retained.");
        return Ok(());
    }
    let session = saved.session.clone().context("login was interrupted before session persistence; use a new profile and remove any abandoned interconnect device in the other client")?;
    ensure!(
        profile.directory.join("store").is_dir(),
        "SDK store missing; refusing to restore a device without its local crypto store"
    );
    let client = client(&profile, &saved).await?;
    client
        .matrix_auth()
        .restore_session(session.clone(), Default::default())
        .await?;
    if logout {
        // Retain credentials on network/server errors, so logout can be retried.
        tokio::time::timeout(Duration::from_secs(30), client.matrix_auth().logout())
            .await
            .context("logout timed out; credentials retained")?
            .context("logout failed; credentials retained")?;
        saved.logged_out = true;
        saved.session = None;
        profile
            .save(&service, &saved)
            .await
            .context("server logout succeeded but local tombstone write failed")?;
        profile
            .delete_secret(&service)
            .await
            .context("server logout succeeded; rerun session-logout to finish keyring cleanup")?;
        println!(
            "Server session revoked and keyring record deleted. SDK store retained; use a new profile for a future login."
        );
        return Ok(());
    }
    validate_restored(&client, &saved, &session).await?;
    println!(
        "Restored the same account and device without browser login. Trust and historical keys remain separate checks."
    );
    if live {
        crate::live::run(client).await
    } else {
        crate::sso::initial_sync(client).await
    }
}

/// Session owner; persistent-client construction also retains the OS lock until
/// process exit, so SDK handles may safely outlive this owner.
pub struct RestoredSession {
    pub client: Client,
    _profile: Profile,
}

/// Restore the desktop session using local credentials and device keys only.
/// The ongoing sync service authenticates with the server and reports revoked
/// tokens; an unavailable network must not prevent reading cached conversations.
pub async fn restore(directory: &str) -> anyhow::Result<RestoredSession> {
    let service = connect().await?;
    let profile = Profile::open(Path::new(directory), false)?;
    let saved = profile
        .read(&service)
        .await?
        .context("no saved session for this profile")?;
    let client = restore_client(&profile, &saved).await?;
    Ok(RestoredSession {
        client,
        _profile: profile,
    })
}

async fn restore_client(profile: &Profile, saved: &SavedSession) -> anyhow::Result<Client> {
    ensure!(
        !saved.logged_out,
        "session was logged out; finish cleanup with session-logout"
    );
    let session = saved.session.clone().context("saved login is incomplete")?;
    ensure!(
        profile.directory.join("store").is_dir(),
        "SDK store missing; refusing replacement crypto state"
    );
    let client = client(profile, saved).await?;
    client
        .matrix_auth()
        .restore_session(session.clone(), Default::default())
        .await?;
    validate_local_device(&client, saved).await?;
    Ok(client)
}

async fn validate_restored(
    client: &Client,
    saved: &SavedSession,
    session: &MatrixSession,
) -> anyhow::Result<()> {
    validate_local_device(client, saved).await?;
    verify_identity(client, session).await.context("session check failed; saved credentials retained (no automatic relogin or device replacement)")?;
    Ok(())
}

async fn validate_local_device(client: &Client, saved: &SavedSession) -> anyhow::Result<()> {
    let expected_key = saved
        .device_key
        .as_ref()
        .context("saved device fingerprint is missing; refusing unverified store restoration")?;
    ensure!(
        &device_key(client).await? == expected_key,
        "local device key changed; refusing to use a replacement crypto store"
    );
    Ok(())
}

async fn device_key(client: &Client) -> anyhow::Result<String> {
    let device = client
        .encryption()
        .get_own_device()
        .await?
        .context("local device keys unavailable")?;
    Ok(device
        .ed25519_key()
        .context("local device signing key unavailable")?
        .to_base64())
}

async fn verify_identity(client: &Client, saved: &MatrixSession) -> anyhow::Result<()> {
    let current = tokio::time::timeout(Duration::from_secs(30), client.whoami())
        .await
        .context("session identity check timed out")??;
    if current.user_id != saved.meta.user_id
        || current.device_id.as_ref() != Some(&saved.meta.device_id)
    {
        bail!("server identity differs from saved account/device");
    }
    println!(
        "Server authenticated the saved device: {}",
        saved.meta.device_id
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_lock_excludes_concurrent_processes_and_releases() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("profile");
        let profile = Profile::open(&path, true).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(Profile::open(&path, false).is_err());
        drop(profile);
        assert!(Profile::open(&path, false).is_ok());
    }
    #[test]
    fn process_lock_survives_handles_and_runtime_cleanup() {
        use std::io::{BufRead, Read, Write};
        const CHILD_PROFILE: &str = "INTERCONNECT_TEST_LOCK_PROFILE";
        if let Some(path) = std::env::var_os(CHILD_PROFILE) {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let profile = Profile::open(Path::new(&path), false).unwrap();
            let saved = SavedSession {
                version: 1,
                homeserver: "http://127.0.0.1:1".into(),
                store_key: "test-only-key".into(),
                session: None,
                logged_out: false,
                device_key: None,
            };
            let client = runtime.block_on(client(&profile, &saved)).unwrap();
            assert!(matches!(
                client.cross_process_lock_config(),
                matrix_sdk_common::cross_process_lock::CrossProcessLockConfig::SingleProcess
            ));
            let late_handle = client.clone();
            drop(client);
            drop(profile);
            let phase = |name| {
                println!("LOCK_PHASE:{name}");
                std::io::stdout().flush().unwrap();
                std::io::stdin().read_exact(&mut [0]).unwrap();
            };
            phase("late-handle");
            {
                let _entered = runtime.enter();
                drop(late_handle);
            }
            let (release, blocked) = std::sync::mpsc::channel();
            let (started, ready) = std::sync::mpsc::channel();
            runtime.spawn_blocking(move || {
                started.send(()).unwrap();
                let _ = blocked.recv();
            });
            ready.recv().unwrap();
            // Exercise the actual timeout path while a cleanup worker survives.
            assert!(crate::shutdown_runtime(runtime, Duration::from_millis(20)).is_err());
            phase("cleanup-timeout");
            release.send(()).unwrap();
            // Lock retention is deliberately independent of cleanup completion.
            phase("before-process-exit");
            return;
        }
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("profile");
        drop(Profile::open(&path, true).unwrap());
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "session::tests::process_lock_survives_handles_and_runtime_cleanup",
                "--nocapture",
            ])
            .env(CHILD_PROFILE, &path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        for phase in ["late-handle", "cleanup-timeout", "before-process-exit"] {
            loop {
                let mut line = String::new();
                assert_ne!(
                    output.read_line(&mut line).unwrap(),
                    0,
                    "child exited before {phase}"
                );
                if line.contains(&format!("LOCK_PHASE:{phase}")) {
                    break;
                }
            }
            assert!(
                Profile::open(&path, false).is_err(),
                "profile unlocked during {phase}"
            );
            child.stdin.as_mut().unwrap().write_all(b"x").unwrap();
        }
        assert!(child.wait().unwrap().success());
        assert!(
            Profile::open(&path, false).is_ok(),
            "kernel must release lock at process exit"
        );
    }

    #[test]
    fn reject_public_or_symlinked_profile() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("profile");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Profile::open(&path, false).is_err());
        let link = parent.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(Profile::open(&link, false).is_err());
    }
    #[tokio::test]
    async fn offline_restore_preserves_device_keys_and_rejects_damaged_store() {
        let parent = tempfile::tempdir().unwrap();
        let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
        let session: MatrixSession = serde_json::from_value(serde_json::json!({
            "user_id": "@test:example.org", "device_id": "TESTDEVICE",
            "access_token": "test-only-token", "refresh_token": null
        }))
        .unwrap();
        let mut saved = SavedSession {
            version: 1,
            homeserver: "http://127.0.0.1:1".into(),
            store_key: "test-only-store-passphrase".into(),
            session: Some(session.clone()),
            logged_out: false,
            device_key: None,
        };
        let first = client(&profile, &saved).await.unwrap();
        first
            .matrix_auth()
            .restore_session(session.clone(), Default::default())
            .await
            .unwrap();
        let original_key = device_key(&first).await.unwrap();
        saved.device_key = Some(original_key.clone());
        drop(first);
        // Nothing is listening at the saved homeserver. Exercise the same local
        // restoration as the desktop, including the device fingerprint check.
        let restored =
            tokio::time::timeout(Duration::from_secs(5), restore_client(&profile, &saved))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(device_key(&restored).await.unwrap(), original_key);
        let record = Zeroizing::new(serde_json::to_vec(&saved).unwrap());
        let decoded = SavedSession::decode(&record).unwrap();
        assert_eq!(
            decoded.session.unwrap().meta.device_id.as_str(),
            "TESTDEVICE"
        );
        drop(restored);
        saved.device_key = Some("different-device-key".into());
        assert!(
            restore_client(&profile, &saved)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("local device key changed")
        );
        saved.device_key = None;
        assert!(
            restore_client(&profile, &saved)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("device fingerprint is missing")
        );
        saved.device_key = Some(original_key);
        saved.store_key = "wrong-store-passphrase".into();
        assert!(restore_client(&profile, &saved).await.is_err());
        saved.logged_out = true;
        assert!(
            restore_client(&profile, &saved)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("session was logged out")
        );
        saved.logged_out = false;
        let empty = Profile::open(&parent.path().join("empty"), true).unwrap();
        assert!(restore_client(&empty, &saved).await.is_err());
        assert!(!empty.directory.join("store").exists());
    }

    #[test]
    fn reject_unknown_or_damaged_record() {
        assert!(SavedSession::decode(b"not json").is_err());
        let record = SavedSession {
            version: 2,
            homeserver: "https://example.org".into(),
            store_key: "test-key".into(),
            session: None,
            logged_out: false,
            device_key: None,
        };
        assert!(SavedSession::decode(&serde_json::to_vec(&record).unwrap()).is_err());
    }
}
