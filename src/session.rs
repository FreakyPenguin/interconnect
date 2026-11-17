//! Persistent accounts backed by the native operating-system credential store.
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
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

mod credentials;
mod presence_cursor;

use credentials::CredentialStore;

/// Resolve the default account directory without accessing the filesystem.
pub fn default_profile_directory() -> anyhow::Result<String> {
    default_profile_path_from(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )?
    .into_os_string()
    .into_string()
    .map_err(|_| anyhow::anyhow!("profile path must be UTF-8"))
}

fn default_profile_path_from(
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let root = home
        .filter(|path| path.is_absolute())
        .context("HOME is unavailable; supply a profile-directory argument")?
        .join("Library/Application Support");
    #[cfg(not(target_os = "macos"))]
    let root = xdg
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|path| path.is_absolute())
                .map(|path| path.join(".local/share"))
        })
        .context("No data directory is available; supply a profile-directory argument")?;
    #[cfg(target_os = "macos")]
    let _ = xdg;
    Ok(root.join("interconnect/profiles/default"))
}

// Never derive Debug: this object contains bearer tokens and an encryption key.
#[derive(Serialize, Deserialize)]
struct SavedSession {
    version: u32,
    homeserver: String,
    store_key: String,
    #[serde(default = "legacy_store_directory")]
    store_directory: String,
    #[serde(default)]
    retained_store_keys: HashMap<String, String>,
    session: Option<MatrixSession>,
    logged_out: bool,
    device_key: Option<String>,
}

fn legacy_store_directory() -> String {
    "store".into()
}

impl SavedSession {
    fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let saved: Self = serde_json::from_slice(bytes).context("invalid saved session record")?;
        ensure!(saved.version == 1, "unsupported session record version");
        ensure!(
            !saved.store_key.is_empty(),
            "session record lacks its store key"
        );
        ensure!(
            saved.store_directory == "store"
                || (saved.store_directory.starts_with("store-")
                    && saved.store_directory[6..].len() == 32
                    && saved.store_directory[6..]
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit())),
            "invalid SDK store directory"
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
                .recursive(true)
                .create(path)
                .context("create profile directory")?;
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
        let id_path = directory.join("keyring-profile");
        let id = match fs::symlink_metadata(&id_path) {
            Ok(meta) => {
                ensure!(
                    meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 65_536,
                    "invalid keyring-profile identity file"
                );
                let id = fs::read_to_string(id_path).context("read keyring-profile identity")?;
                ensure!(
                    Path::new(&id).is_absolute()
                        && !id.contains('\0')
                        && !id.ends_with('\n')
                        && !id.ends_with('\r'),
                    "keyring-profile must contain the original absolute profile path without a trailing newline"
                );
                id
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => directory
                .to_str()
                .context("profile path must be UTF-8")?
                .to_owned(),
            Err(error) => return Err(error.into()),
        };
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

    async fn read(&self, service: &CredentialStore) -> anyhow::Result<Option<SavedSession>> {
        service.read(self).await
    }

    async fn save(&self, service: &CredentialStore, saved: &SavedSession) -> anyhow::Result<()> {
        service.save(self, saved).await
    }

    async fn delete_secret(&self, service: &CredentialStore) -> anyhow::Result<()> {
        service.delete_secret(self).await
    }
}

async fn connect() -> anyhow::Result<CredentialStore> {
    CredentialStore::connect().await
}

async fn client(profile: &Profile, saved: &SavedSession) -> anyhow::Result<Client> {
    // Retain before opening SQLite, including build failures/cancellation.
    profile.retain_lock_until_exit()?;
    if saved.session.is_some() && saved.device_key.is_some() {
        presence_cursor::migrate(
            &profile.directory.join(&saved.store_directory),
            &saved.store_key,
        )
        .await?;
    }
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
        .sqlite_store(
            profile.directory.join(&saved.store_directory),
            Some(&saved.store_key),
        )
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
        store_directory: legacy_store_directory(),
        retained_store_keys: HashMap::new(),
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
    println!(
        "Session saved in the system credential store. Closing this process keeps the device logged in."
    );
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
        profile.directory.join(&saved.store_directory).is_dir(),
        "SDK store missing; refusing to restore a device without its local crypto store"
    );
    let client = client(&profile, &saved).await?;
    client
        .matrix_auth()
        .restore_session(session.clone(), Default::default())
        .await?;
    if logout {
        revoke_session(&client).await?;
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

/// Authentication input has a redacted Debug implementation because UI messages
/// and asynchronous requests must never expose passwords through diagnostics.
#[derive(Clone)]
pub enum LoginMethod {
    Browser,
    Password {
        username: String,
        password: Zeroizing<String>,
    },
}
impl std::fmt::Debug for LoginMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Browser => f.write_str("Browser"),
            Self::Password { .. } => f.write_str("Password(<redacted>)"),
        }
    }
}

/// Owns the profile throughout restoration and all login attempts.
/// Failed attempts retain their encrypted stores; a retry always uses fresh keys.
pub struct DesktopSession {
    profile: Profile,
    service: Option<CredentialStore>,
    pub client: Option<Client>,
    revoked: bool,
}

impl DesktopSession {
    pub async fn open(directory: &str) -> anyhow::Result<Self> {
        let exists = Path::new(directory).try_exists()?;
        let profile = Profile::open(Path::new(directory), true)?;
        // A new profile can show the form without a network or keyring request.
        if !exists {
            return Ok(Self {
                profile,
                service: None,
                client: None,
                revoked: false,
            });
        }
        let service = connect().await?;
        let saved = profile.read(&service).await?;
        let client = match saved.filter(|s| !s.logged_out && s.session.is_some()) {
            Some(saved) => Some(restore_client(&profile, &saved).await?),
            None => None,
        };
        Ok(Self {
            profile,
            service: Some(service),
            client,
            revoked: false,
        })
    }

    /// Sync and view workers must be stopped before calling this method.
    /// Failed server requests retain both credentials and the live client. Once
    /// revoked, retries only finish local cleanup, even if saving the tombstone failed.
    pub async fn sign_out(&mut self) -> anyhow::Result<()> {
        let service = self
            .service
            .as_ref()
            .context("Credential store is unavailable")?;
        let mut saved = self
            .profile
            .read(service)
            .await?
            .context("No saved session")?;
        if !self.revoked && !saved.logged_out {
            let client = self.client.as_ref().context("Session is unavailable")?;
            revoke_session(client).await?;
        }
        self.revoked = true;
        self.client = None;
        saved.logged_out = true;
        saved.session = None;
        self.profile.save(service, &saved).await
            .context("Signed out on the server; could not mark local credentials revoked. Retry sign-out before closing")?;
        self.profile
            .delete_secret(service)
            .await
            .context("Signed out on the server; local credential cleanup failed. Retry sign-out")?;
        Ok(())
    }

    pub async fn sign_in(
        &mut self,
        homeserver: &str,
        method: LoginMethod,
        browser_url: impl FnOnce(String) + Send,
    ) -> anyhow::Result<()> {
        // Discover before reserving keys or creating a device.
        let discovered = Client::builder()
            .homeserver_url(homeserver)
            .sliding_sync_version_builder(VersionBuilder::DiscoverNative)
            .build()
            .await?;
        if self.service.is_none() {
            self.service = Some(connect().await?);
        }
        let service = self.service.as_ref().expect("keyring connected");
        let key: [u8; 32] = rand::random();
        let mut retained_store_keys = HashMap::new();
        if let Some(previous) = self.profile.read(service).await? {
            retained_store_keys = previous.retained_store_keys;
            retained_store_keys.insert(previous.store_directory, previous.store_key);
        }
        let mut saved = SavedSession {
            version: 1,
            homeserver: discovered.homeserver().to_string(),
            store_key: key.iter().map(|b| format!("{b:02x}")).collect(),
            store_directory: format!("store-{:032x}", rand::random::<u128>()),
            retained_store_keys,
            session: None,
            logged_out: false,
            device_key: None,
        };
        // Persist the encryption key before opening SQLite or browser login.
        // Never reuse a retained store for a new device.
        self.profile.save(service, &saved).await?;
        let client = client(&self.profile, &saved).await?;
        authenticate(&client, method, browser_url).await?;
        let auth = client.matrix_auth();
        saved.device_key = Some(device_key(&client).await?);
        saved.session = Some(auth.session().context("SDK did not return a session")?);
        // Publish a complete restorable record atomically, before starting sync.
        self.profile.save(service, &saved).await
            .context("Could not save login; remove the Interconnect device in another client before retrying")?;
        self.client = Some(client);
        self.revoked = false;
        Ok(())
    }
}

/// A rejected/expired token is already signed out; other failures must retain it.
async fn revoke_session(client: &Client) -> anyhow::Result<()> {
    match tokio::time::timeout(Duration::from_secs(30), client.matrix_auth().logout()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error))
            if error.client_api_error_kind().is_some_and(|kind| {
                matches!(
                    kind,
                    matrix_sdk::ruma::api::error::ErrorKind::UnknownToken(_)
                )
            }) =>
        {
            Ok(())
        }
        Ok(Err(error)) => {
            Err(anyhow::Error::new(error).context("Sign-out failed; credentials retained"))
        }
        Err(_) => bail!("Sign-out timed out; credentials retained. Retry when connected"),
    }
}

async fn authenticate(
    client: &Client,
    method: LoginMethod,
    browser_url: impl FnOnce(String) + Send,
) -> anyhow::Result<()> {
    let auth = client.matrix_auth();
    match method {
        LoginMethod::Browser => {
            auth.login_sso(|url| async move {
                browser_url(url);
                Ok(())
            })
            .server_builder(
                LocalServerBuilder::new()
                    .port_range(8765..8766)
                    .bind_tries(1),
            )
            .initial_device_display_name("Interconnect")
            .send()
            .await?;
        }
        LoginMethod::Password { username, password } => {
            auth.login_username(&username, password.as_str())
                .initial_device_display_name("Interconnect")
                .send()
                .await?;
        }
    }
    Ok(())
}

async fn restore_client(profile: &Profile, saved: &SavedSession) -> anyhow::Result<Client> {
    ensure!(
        !saved.logged_out,
        "session was logged out; finish cleanup with session-logout"
    );
    let session = saved.session.clone().context("saved login is incomplete")?;
    ensure!(
        profile.directory.join(&saved.store_directory).is_dir(),
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
    #[cfg(not(target_os = "macos"))]
    fn default_profile_uses_xdg_data_and_absolute_home_fallback() {
        for (xdg, home, expected) in [
            (Some("/data"), None, "/data/interconnect/profiles/default"),
            (
                None,
                Some("/users/me"),
                "/users/me/.local/share/interconnect/profiles/default",
            ),
            (
                Some(""),
                Some("/users/me"),
                "/users/me/.local/share/interconnect/profiles/default",
            ),
            (
                Some("relative"),
                Some("/users/me"),
                "/users/me/.local/share/interconnect/profiles/default",
            ),
        ] {
            assert_eq!(
                default_profile_path_from(xdg.map(PathBuf::from), home.map(PathBuf::from)).unwrap(),
                PathBuf::from(expected)
            );
        }
        assert!(default_profile_path_from(None, None).is_err());
        assert!(
            default_profile_path_from(Some("relative".into()), Some("relative".into())).is_err()
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn default_profile_uses_application_support_on_macos() {
        assert_eq!(
            default_profile_path_from(Some("/ignored".into()), Some("/users/me".into())).unwrap(),
            PathBuf::from("/users/me/Library/Application Support/interconnect/profiles/default")
        );
        assert!(default_profile_path_from(Some("/ignored".into()), None).is_err());
    }

    #[test]
    fn login_creates_nested_private_profile_directories() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("interconnect/profiles/default");
        assert!(Profile::open(&path, false).is_err());
        assert!(!parent.path().join("interconnect").exists());
        let profile = Profile::open(&path, true).unwrap();
        for directory in [
            "interconnect",
            "interconnect/profiles",
            "interconnect/profiles/default",
        ] {
            assert_eq!(
                fs::metadata(parent.path().join(directory))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert!(Profile::open(&path, false).is_err());
        drop(profile);
        assert!(Profile::open(&path, false).is_ok());
    }

    #[test]
    fn moved_profile_keeps_keyring_identity_navigation_and_exclusive_lock() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join(".matrix-spike");
        let profile = Profile::open(&old, true).unwrap();
        let id = profile.id.clone();
        fs::create_dir(old.join("store")).unwrap();
        fs::write(old.join("store/fixture"), b"test store").unwrap();
        fs::write(old.join("navigation.json"), b"test navigation").unwrap();
        fs::write(old.join("keyring-profile"), &id).unwrap();
        drop(profile);
        let moved = parent.path().join("default");
        fs::rename(&old, &moved).unwrap();
        let profile = Profile::open(&moved, false).unwrap();
        assert_eq!(profile.directory, moved.canonicalize().unwrap());
        assert_eq!(profile.id, id);
        assert_eq!(
            fs::read(moved.join("store/fixture")).unwrap(),
            b"test store"
        );
        assert_eq!(
            fs::read(moved.join("navigation.json")).unwrap(),
            b"test navigation"
        );
        assert!(Profile::open(&moved, false).is_err());
        drop(profile);
        assert!(Profile::open(&moved, false).is_ok());
        assert!(!old.exists());
    }

    #[test]
    fn malformed_or_symlinked_keyring_identity_is_rejected_without_fallback() {
        let parent = tempfile::tempdir().unwrap();
        let directory = parent.path().join("profile");
        drop(Profile::open(&directory, true).unwrap());
        let identity = directory.join("keyring-profile");
        for invalid in [
            "",
            "relative",
            "/old/profile\n",
            "/old/profile\r",
            "/old/\0profile",
        ] {
            fs::write(&identity, invalid).unwrap();
            assert!(Profile::open(&directory, false).is_err());
        }
        fs::write(&identity, vec![b'/'; 65_537]).unwrap();
        assert!(Profile::open(&directory, false).is_err());
        fs::remove_file(&identity).unwrap();
        std::os::unix::fs::symlink("/missing", &identity).unwrap();
        assert!(Profile::open(&directory, false).is_err());
        fs::remove_file(&identity).unwrap();
        fs::create_dir(&identity).unwrap();
        assert!(Profile::open(&directory, false).is_err());
    }

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
                store_directory: legacy_store_directory(),
                retained_store_keys: HashMap::new(),
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
            store_directory: legacy_store_directory(),
            retained_store_keys: HashMap::new(),
            session: Some(session.clone()),
            logged_out: false,
            device_key: None,
        };
        saved.store_directory = "store-0123456789abcdef0123456789abcdef".into();
        fs::create_dir(profile.directory.join("store")).unwrap();
        fs::write(
            profile.directory.join("store/retained-fixture"),
            b"retained",
        )
        .unwrap();
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
        // Opening encrypted SQLite stores can exceed five seconds in debug
        // builds on shared CI runners. Keep a generous hang guard rather than
        // treating local restoration as a performance benchmark.
        let restored =
            tokio::time::timeout(Duration::from_secs(60), restore_client(&profile, &saved))
                .await
                .expect("offline session restoration did not finish within 60 seconds")
                .expect("offline session restoration failed");
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
        assert!(!empty.directory.join(&saved.store_directory).exists());
        assert_eq!(
            fs::read(profile.directory.join("store/retained-fixture")).unwrap(),
            b"retained"
        );
    }

    #[tokio::test]
    async fn password_sign_in_installs_session_without_browser_or_sync() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{body_partial_json, method, path},
        };
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "versions": ["v1.11"], "unstable_features": {"org.matrix.simplified_msc3575": true},
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/_matrix/client/v3/login"))
            .and(body_partial_json(serde_json::json!({
                "type": "m.login.password", "identifier": {"type":"m.id.user", "user":"fixture"},
                "password":"fixture-password", "initial_device_display_name":"Interconnect",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "user_id":"@fixture:example.org", "device_id":"FIXTURE", "access_token":"fixture-token",
            }))).expect(1).mount(&server).await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(VersionBuilder::Native)
            .build()
            .await
            .unwrap();
        authenticate(
            &client,
            LoginMethod::Password {
                username: "fixture".into(),
                password: Zeroizing::new("fixture-password".into()),
            },
            |_| panic!("password login must not open a browser"),
        )
        .await
        .unwrap();
        assert_eq!(client.user_id().unwrap().as_str(), "@fixture:example.org");
        assert_eq!(client.device_id().unwrap().as_str(), "FIXTURE");
        let requests = server.received_requests().await.unwrap();
        assert!(!requests.iter().any(|r| r.url.path().ends_with("/sync")));
    }

    #[tokio::test]
    async fn sign_out_accepts_revoked_tokens_but_preserves_sessions_on_server_failure() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        for (status, body, success) in [
            (200, serde_json::json!({}), true),
            (
                401,
                serde_json::json!({"errcode":"M_UNKNOWN_TOKEN", "error":"expired"}),
                true,
            ),
            (
                500,
                serde_json::json!({"errcode":"M_UNKNOWN", "error":"offline"}),
                false,
            ),
            (
                403,
                serde_json::json!({"errcode":"M_FORBIDDEN", "error":"refused"}),
                false,
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/_matrix/client/versions"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({"versions":["v1.11"]})),
                )
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/logout"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;
            let client = Client::builder()
                .homeserver_url(server.uri())
                .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
                .sliding_sync_version_builder(VersionBuilder::Native)
                .build()
                .await
                .unwrap();
            client.matrix_auth().restore_session(serde_json::from_value::<MatrixSession>(serde_json::json!({
                "user_id":"@fixture:example.org", "device_id":"FIXTURE", "access_token":"fixture-token"
            })).unwrap(), Default::default()).await.unwrap();
            assert_eq!(revoke_session(&client).await.is_ok(), success);
            if !success {
                assert!(client.matrix_auth().session().is_some());
            }
        }
    }

    #[test]
    fn password_request_debug_is_redacted() {
        let method = LoginMethod::Password {
            username: "private-user".into(),
            password: Zeroizing::new("private-password".into()),
        };
        let debug = format!("{method:?}");
        assert!(!debug.contains("private-user"));
        assert!(!debug.contains("private-password"));
    }

    #[test]
    fn session_records_keep_legacy_paths_and_reject_store_path_traversal() {
        let mut record = serde_json::json!({
            "version": 1, "homeserver": "https://matrix.example", "store_key": "fixture",
            "session": null, "logged_out": false, "device_key": null,
        });
        let decode =
            |value: &serde_json::Value| SavedSession::decode(&serde_json::to_vec(value).unwrap());
        assert_eq!(decode(&record).unwrap().store_directory, "store");
        assert!(decode(&record).unwrap().retained_store_keys.is_empty());
        for path in [
            "../store",
            "/tmp/store",
            "store-../outside",
            "store-",
            "store-é",
        ] {
            record["store_directory"] = path.into();
            assert!(decode(&record).is_err(), "accepted {path}");
        }
        record["store_directory"] = "store-0123456789abcdef0123456789abcdef".into();
        record["retained_store_keys"] = serde_json::json!({"store": "previous-fixture-key"});
        let saved = decode(&record).unwrap();
        assert_eq!(saved.retained_store_keys["store"], "previous-fixture-key");
        assert_eq!(
            decode(&serde_json::to_value(saved).unwrap())
                .unwrap()
                .retained_store_keys
                .len(),
            1
        );
    }

    #[test]
    fn reject_unknown_or_damaged_record() {
        assert!(SavedSession::decode(b"not json").is_err());
        let record = SavedSession {
            version: 2,
            homeserver: "https://example.org".into(),
            store_key: "test-key".into(),
            store_directory: legacy_store_directory(),
            retained_store_keys: HashMap::new(),
            session: None,
            logged_out: false,
            device_key: None,
        };
        assert!(SavedSession::decode(&serde_json::to_vec(&record).unwrap()).is_err());
    }
}
