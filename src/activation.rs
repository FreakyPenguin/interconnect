//! Same-user, event-driven desktop activation. No account store is opened here.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fmt,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Notify,
};

pub const MAX_LINK_BYTES: usize = 8192;
const MAX_FRAME: usize = 16_384;
const MAX_PENDING: usize = 16;
const MAX_SEEN: usize = 256;
const IO_TIMEOUT: Duration = Duration::from_millis(800);

/// Deliberately redacted: destinations can contain private room/event IDs.
#[derive(Clone, Serialize, Deserialize)]
pub struct Link(String);
impl Link {
    pub fn parse(input: &str) -> Result<Self, &'static str> {
        use matrix_sdk::ruma::{MatrixToUri, MatrixUri, matrix_uri::MatrixId};
        if input.len() > MAX_LINK_BYTES || input.chars().any(char::is_control) {
            return Err("The Matrix link is too long or contains invalid characters.");
        }
        let normalized;
        let input = if let Some((scheme, rest)) = input.split_once(':')
            && scheme.eq_ignore_ascii_case("matrix")
        {
            normalized = format!("matrix:{rest}");
            normalized.as_str()
        } else {
            input
        };
        let (id, canonical) = if input.starts_with("matrix:") {
            (
                MatrixUri::parse(input).ok().map(|uri| uri.id().clone()),
                input.to_owned(),
            )
        } else {
            let url = url::Url::parse(input).map_err(|_| "Enter a valid Matrix link.")?;
            if url.scheme() != "https"
                || url.host_str() != Some("matrix.to")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port().is_some()
            {
                return Err("Only matrix: and https://matrix.to links are supported.");
            }
            (
                MatrixToUri::parse(url.as_str())
                    .ok()
                    .map(|uri| uri.id().clone()),
                url.to_string(),
            )
        };
        if !matches!(
            id,
            Some(
                MatrixId::User(_)
                    | MatrixId::Room(_)
                    | MatrixId::RoomAlias(_)
                    | MatrixId::Event(_, _)
            )
        ) {
            return Err("This Matrix link is invalid or unsupported.");
        }
        Ok(Self(canonical))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MatrixLink(<redacted>)")
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Request {
    id: String,
    link: Option<Link>,
}
impl Request {
    pub fn new(link: Option<Link>) -> Self {
        Self {
            id: format!("{:032x}", rand::random::<u128>()),
            link,
        }
    }
    pub fn has_link(&self) -> bool {
        self.link.is_some()
    }
    fn valid(&self) -> bool {
        self.id.len() == 32
            && self.id.bytes().all(|b| b.is_ascii_hexdigit())
            && self
                .link
                .as_ref()
                .is_none_or(|link| Link::parse(link.as_str()).is_ok())
    }
}
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationRequest(<redacted>)")
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Target {
    pub key: String,
    pub instance: String,
    pub label: String,
    pub account_name: String,
    pub user: Option<String>,
    profile: String,
    accepting: bool,
}
impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActiveProfile(<redacted>)")
    }
}
impl Target {
    pub fn profile_directory(&self) -> &str {
        &self.profile
    }
}

#[derive(Serialize, Deserialize)]
enum Command {
    Inspect,
    Deliver { target: Target, request: Request },
}
#[derive(Serialize, Deserialize)]
enum Response {
    Profile(Target),
    Accepted,
    Busy,
    Changed,
    Unavailable,
    Invalid,
}

pub struct Incoming {
    pub link: Option<Link>,
    pub user: Option<String>,
}
struct Inbox {
    info: Target,
    pending: VecDeque<Incoming>,
    seen: VecDeque<String>,
}
#[derive(Clone)]
pub struct Handle {
    inner: Arc<Mutex<Inbox>>,
    wake: Arc<Notify>,
}
impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationHandle(<redacted>)")
    }
}
impl Handle {
    /// A bounded local queue, also used by the native macOS callback and demo.
    pub fn local() -> Self {
        let info = Target {
            key: String::new(),
            instance: format!("{:032x}", rand::random::<u128>()),
            label: String::new(),
            account_name: String::new(),
            user: None,
            profile: String::new(),
            accepting: true,
        };
        Self {
            inner: Arc::new(Mutex::new(Inbox {
                info,
                pending: VecDeque::new(),
                seen: VecDeque::new(),
            })),
            wake: Arc::new(Notify::new()),
        }
    }
    /// Native callbacks may not wait behind another thread or accept work after
    /// the launcher has atomically decided to exit.
    pub fn native_link(&self, input: &str) -> bool {
        let Ok(link) = Link::parse(input) else {
            return false;
        };
        let Ok(mut inbox) = self.inner.try_lock() else {
            return false;
        };
        if !inbox.info.accepting || inbox.pending.len() >= MAX_PENDING {
            return false;
        }
        inbox.pending.push_back(Incoming {
            link: Some(link),
            user: None,
        });
        drop(inbox);
        self.wake.notify_one();
        true
    }
    pub fn close_if_empty(&self) -> bool {
        let mut inbox = self.inner.lock().unwrap();
        if !inbox.pending.is_empty() {
            return false;
        }
        inbox.info.accepting = false;
        true
    }
    pub fn update(&self, user: Option<&str>, name: &str, accepting: bool) {
        let mut inbox = self.inner.lock().unwrap();
        if inbox.info.user.as_deref() != user {
            inbox.info.user = user.map(str::to_owned);
        }
        if inbox.info.account_name != name {
            inbox.info.account_name = name.chars().take(128).collect();
        }
        inbox.info.accepting = accepting;
    }
    pub async fn notified(&self) {
        self.wake.notified().await;
    }
    pub fn pop(&self) -> Option<Incoming> {
        self.inner.lock().unwrap().pending.pop_front()
    }
    pub fn pending(&self) -> usize {
        self.inner.lock().unwrap().pending.len()
    }
    pub fn pop_focus(&self) -> bool {
        let mut inbox = self.inner.lock().unwrap();
        if inbox
            .pending
            .front()
            .is_some_and(|incoming| incoming.link.is_none())
        {
            inbox.pending.pop_front();
            true
        } else {
            false
        }
    }
    pub fn clear(&self) {
        self.inner.lock().unwrap().pending.clear();
    }
    pub fn cancel(&self) {
        let mut inbox = self.inner.lock().unwrap();
        inbox.info.accepting = false;
        inbox.pending.clear();
    }
    pub fn target(&self) -> Target {
        self.inner.lock().unwrap().info.clone()
    }
    pub fn enqueue(&self, request: Request) -> Result<(), &'static str> {
        match self.deliver(&self.target(), request) {
            Response::Accepted => Ok(()),
            Response::Busy => {
                Err("Too many links are waiting. Close the blocking dialog and try again.")
            }
            _ => Err("This profile is no longer accepting links."),
        }
    }
    fn deliver(&self, target: &Target, request: Request) -> Response {
        if !request.valid() {
            return Response::Invalid;
        }
        let mut inbox = self.inner.lock().unwrap();
        if inbox.info.instance != target.instance
            || inbox.info.user != target.user
            || inbox.info.key != target.key
            || inbox.info.profile != target.profile
        {
            return Response::Changed;
        }
        if !inbox.info.accepting {
            return Response::Unavailable;
        }
        if inbox.seen.contains(&request.id) {
            return Response::Accepted;
        }
        if inbox.pending.len() >= MAX_PENDING {
            return Response::Busy;
        }
        let user = inbox.info.user.clone();
        inbox.pending.push_back(Incoming {
            link: request.link,
            user,
        });
        inbox.seen.push_back(request.id);
        if inbox.seen.len() > MAX_SEEN {
            inbox.seen.pop_front();
        }
        drop(inbox);
        self.wake.notify_one();
        Response::Accepted
    }
}

/// Lifetime ownership is separate from the session's authoritative store lock.
/// A conflicting store lock still rejects startup (e.g. an older app version).
pub struct Server {
    handle: Handle,
    task: tokio::task::JoinHandle<()>,
    socket: PathBuf,
    _lock: File,
}
impl Server {
    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.handle.update(None, "", false);
        self.task.abort();
        let _ = fs::remove_file(&self.socket);
        // Keep the lock file inode: unlinking it would allow two owners.
    }
}
pub enum Claim {
    Owner(Server),
    Existing(Target),
}

#[derive(Clone)]
pub struct Registry {
    directory: PathBuf,
}
impl Registry {
    pub fn system() -> anyhow::Result<Self> {
        // A short stable path also fits macOS's Unix socket path limit. Nothing
        // confidential is persisted here; metadata is served over the socket.
        #[cfg(target_os = "linux")]
        if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
            let meta = fs::symlink_metadata(&path).context("Invalid runtime directory")?;
            ensure!(
                path.is_absolute()
                    && meta.is_dir()
                    && !meta.file_type().is_symlink()
                    && meta.uid() == uid()
                    && meta.permissions().mode() & 0o077 == 0,
                "Runtime directory must be private and owned by this user"
            );
            if path.as_os_str().len() < 50 {
                return Self::open(path.join("interconnect-activation"));
            }
        }
        Self::open(Path::new("/tmp").join(format!("interconnect-activation-{}", uid())))
    }
    pub fn open(directory: PathBuf) -> anyhow::Result<Self> {
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).context("Could not create activation directory"),
        }
        let meta = fs::symlink_metadata(&directory)?;
        ensure!(
            meta.is_dir()
                && !meta.file_type().is_symlink()
                && meta.uid() == uid()
                && meta.permissions().mode() & 0o077 == 0,
            "The activation directory must be a private directory owned by this user"
        );
        ensure!(
            directory.as_os_str().len() < 75,
            "Activation directory path is too long"
        );
        Ok(Self { directory })
    }
    fn socket(&self, key: &str) -> PathBuf {
        self.directory.join(format!("{key}.sock"))
    }
    pub async fn claim(&self, profile: &str) -> anyhow::Result<Claim> {
        let key = profile_key(profile);
        let lock_path = self.directory.join(format!("{key}.lock"));
        if let Ok(meta) = fs::symlink_metadata(&lock_path) {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink() && meta.uid() == uid(),
                "Invalid activation lock file"
            );
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(lock_path)?;
        if let Err(error) = lock.try_lock() {
            ensure!(
                matches!(error, std::fs::TryLockError::WouldBlock),
                "Could not lock activation endpoint"
            );
            // A concurrent launcher may hold its lock just before binding. This
            // is a bounded startup retry, never an idle subscription timer.
            for _ in 0..10 {
                if let Ok(target) = self.inspect(&key).await {
                    ensure!(
                        target.profile == profile,
                        "Activation profile identity conflict"
                    );
                    return Ok(Claim::Existing(target));
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            anyhow::bail!("This profile is starting or stopping. Try opening the link again.");
        }
        let socket = self.socket(&key);
        if let Ok(meta) = fs::symlink_metadata(&socket) {
            use std::os::unix::fs::FileTypeExt;
            ensure!(
                meta.file_type().is_socket() && meta.uid() == uid(),
                "Invalid activation socket"
            );
            fs::remove_file(&socket)?;
        }
        let listener = UnixListener::bind(&socket).context("Could not bind activation socket")?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
        let info = Target {
            key,
            instance: format!("{:032x}", rand::random::<u128>()),
            label: Path::new(profile)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Profile")
                .chars()
                .take(128)
                .collect(),
            account_name: "Starting…".into(),
            user: None,
            profile: profile.into(),
            accepting: true,
        };
        let handle = Handle {
            inner: Arc::new(Mutex::new(Inbox {
                info,
                pending: VecDeque::new(),
                seen: VecDeque::new(),
            })),
            wake: Arc::new(Notify::new()),
        };
        let service = handle.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                // Sequential bounded frames prevent unbounded connection tasks.
                let _ = tokio::time::timeout(IO_TIMEOUT, async {
                    let response = match read::<Command>(&mut stream).await {
                        Ok(Command::Inspect) => Response::Profile(service.target()),
                        Ok(Command::Deliver { target, request }) => {
                            service.deliver(&target, request)
                        }
                        Err(_) => Response::Invalid,
                    };
                    write(&mut stream, &response).await
                })
                .await;
            }
        });
        Ok(Claim::Owner(Server {
            handle,
            task,
            socket,
            _lock: lock,
        }))
    }
    async fn exchange(&self, key: &str, command: Command) -> Result<Response, &'static str> {
        if key.len() != 16 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid activation target.");
        }
        tokio::time::timeout(IO_TIMEOUT, async {
            let mut stream = UnixStream::connect(self.socket(key))
                .await
                .map_err(|_| "The profile closed. Refresh the list and choose again.")?;
            write(&mut stream, &command)
                .await
                .map_err(|_| "Could not deliver the link. Refresh and try again.")?;
            read(&mut stream)
                .await
                .map_err(|_| "Could not confirm delivery. Retry the same request.")
        })
        .await
        .map_err(|_| "The profile did not respond. Refresh and try again.")?
    }
    pub async fn inspect(&self, key: &str) -> Result<Target, &'static str> {
        match self.exchange(key, Command::Inspect).await? {
            Response::Profile(target) if target.key == key => Ok(target),
            _ => Err("The profile is unavailable."),
        }
    }
    pub async fn discover(&self) -> anyhow::Result<Vec<Target>> {
        let mut keys = Vec::new();
        for (index, entry) in fs::read_dir(&self.directory)?.enumerate() {
            ensure!(
                index < 4096,
                "Too many activation records. Choose an explicit profile."
            );
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "sock")
            {
                let key = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|key| {
                        key.len() == 16 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                    .context("Invalid activation endpoint")?;
                keys.push(key.to_owned());
                ensure!(
                    keys.len() <= 128,
                    "Too many activation endpoints. Choose an explicit profile."
                );
            }
        }
        keys.sort();
        ensure!(
            keys.len() <= 128,
            "Too many activation endpoints. Choose an explicit profile."
        );
        let mut active = Vec::new();
        let results =
            iced::futures::future::join_all(keys.iter().map(|key| self.inspect(key))).await;
        for (key, result) in keys.iter().zip(results) {
            match result {
                Ok(target) if target.accepting => active.push(target),
                Ok(_) => {}
                Err(_) => {
                    // A failed probe is not proof that an owner is gone. Do not
                    // route into a different account when a live owner is busy.
                    let lock_path = self.directory.join(format!("{key}.lock"));
                    let meta = fs::symlink_metadata(&lock_path)
                        .context("Could not verify activation owner")?;
                    ensure!(
                        meta.is_file() && !meta.file_type().is_symlink() && meta.uid() == uid(),
                        "Invalid activation owner"
                    );
                    let lock = OpenOptions::new().read(true).write(true).open(lock_path)?;
                    ensure!(
                        lock.try_lock().is_ok(),
                        "An active profile is not responding. Retry or choose an explicit profile."
                    );
                }
            }
        }
        active.sort_by_cached_key(|target| (target.label.to_lowercase(), target.user.clone()));
        Ok(active)
    }
    pub async fn send(&self, target: &Target, request: &Request) -> Result<(), &'static str> {
        // Retry an ambiguous acknowledgement with the same ID. The receiver
        // records IDs atomically with queue admission, so this cannot duplicate.
        let mut response = self
            .exchange(
                &target.key,
                Command::Deliver {
                    target: target.clone(),
                    request: request.clone(),
                },
            )
            .await;
        if response.is_err() {
            response = self
                .exchange(
                    &target.key,
                    Command::Deliver {
                        target: target.clone(),
                        request: request.clone(),
                    },
                )
                .await;
        }
        match response? {
            Response::Accepted => Ok(()),
            Response::Busy => {
                Err("This profile has too many pending links. Close its dialog and try again.")
            }
            Response::Changed => {
                Err("The selected profile or account changed. Refresh and choose again.")
            }
            _ => Err("The profile is closing or unavailable. Refresh and choose again."),
        }
    }
}
impl Hash for Handle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

async fn read<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> std::io::Result<T> {
    let len = stream.read_u32().await? as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::other("Oversized activation frame"));
    }
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| std::io::Error::other("Invalid activation frame"))
}
async fn write(stream: &mut UnixStream, value: &impl Serialize) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(std::io::Error::other("Oversized activation frame"));
    }
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await
}
fn profile_key(profile: &str) -> String {
    // Stable across compiler releases; this is only a compact socket filename,
    // never an authentication secret. Full canonical identity is rechecked.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in profile.as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
fn uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: POSIX geteuid has no preconditions and returns the calling uid.
    unsafe { geteuid() }
}
pub fn canonical_profile(profile: &str) -> anyhow::Result<String> {
    let path = std::path::absolute(profile)?;
    if let Ok(meta) = fs::symlink_metadata(&path) {
        ensure!(
            !meta.file_type().is_symlink(),
            "Profile must not be a symlink"
        );
    }
    let mut ancestor = path.as_path();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .context("Choose a profile directory without unresolved parent components")?
                .to_owned(),
        );
        ancestor = ancestor.parent().context("Invalid profile directory")?;
    }
    let mut canonical = ancestor.canonicalize()?;
    for part in missing.into_iter().rev() {
        canonical.push(part);
    }
    canonical
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("Profile path must be UTF-8"))
}
pub fn same_profile(target: &Target, canonical: &str) -> bool {
    target.profile == canonical
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> (tempfile::TempDir, Registry) {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::open(directory.path().join("ipc")).unwrap();
        (directory, registry)
    }
    async fn owner(registry: &Registry, profile: &str) -> Server {
        let Claim::Owner(server) = registry.claim(profile).await.unwrap() else {
            panic!("expected owner");
        };
        server
            .handle()
            .update(Some("@fixture:example.org"), "Fixture account", true);
        server
    }
    fn request() -> Request {
        Request::new(Some(
            Link::parse("matrix:roomid/fixture:example.org/e/event").unwrap(),
        ))
    }

    #[test]
    fn links_validate_normalize_and_redact_before_any_profile_access() {
        for input in [
            "MATRIX:roomid/fixture:example.org",
            "HTTPS://MATRIX.TO/#/!fixture:example.org",
            "matrix:u/fixture:example.org",
        ] {
            let link = Link::parse(input).unwrap();
            assert!(!format!("{link:?}").contains("fixture"));
            assert!(!link.as_str().starts_with("MATRIX:"));
        }
        for input in [
            "http://matrix.to/#/!fixture:example.org",
            "https://matrix.to.evil/#/!fixture:example.org",
            "https://person@matrix.to/#/!fixture:example.org",
            "matrix:invalid",
            "matrix:roomid/fixture:example.org\n",
        ] {
            assert!(Link::parse(input).is_err(), "{input}");
        }
        assert!(Link::parse(&"x".repeat(MAX_LINK_BYTES + 1)).is_err());
        assert!(!format!("{:?}", request()).contains("fixture"));
    }

    #[tokio::test]
    async fn real_socket_forwarding_is_atomic_bounded_and_deduplicated() {
        let (_directory, registry) = registry();
        let server = owner(&registry, "/fixture/personal").await;
        let target = registry.discover().await.unwrap().pop().unwrap();
        let first = request();
        registry.send(&target, &first).await.unwrap();
        registry.send(&target, &first).await.unwrap();
        let incoming = server.handle().pop().unwrap();
        assert_eq!(incoming.user.as_deref(), Some("@fixture:example.org"));
        assert!(incoming.link.unwrap().as_str().ends_with("/e/event"));
        assert!(server.handle().pop().is_none());
        registry.send(&target, &first).await.unwrap();
        assert!(server.handle().pop().is_none());
        for _ in 0..MAX_PENDING {
            registry.send(&target, &request()).await.unwrap();
        }
        let blocked = request();
        assert!(
            registry
                .send(&target, &blocked)
                .await
                .unwrap_err()
                .contains("pending")
        );
        server.handle().pop().unwrap();
        registry.send(&target, &blocked).await.unwrap();
        assert_eq!(
            server.handle().inner.lock().unwrap().pending.len(),
            MAX_PENDING
        );
    }

    #[tokio::test]
    async fn profile_owner_handoff_and_stale_incarnations_never_open_another_account() {
        let (_directory, registry) = registry();
        let server = owner(&registry, "/fixture/work").await;
        let old = server.handle().target();
        assert!(matches!(
            registry.claim("/fixture/work").await.unwrap(),
            Claim::Existing(_)
        ));
        server
            .handle()
            .update(Some("@different:example.org"), "Different account", true);
        assert!(
            registry
                .send(&old, &request())
                .await
                .unwrap_err()
                .contains("changed")
        );
        assert!(server.handle().pop().is_none());
        let fresh = server.handle().target();
        let mut wrong = fresh.clone();
        wrong.profile = "/other/profile".into();
        assert!(registry.send(&wrong, &request()).await.is_err());
        drop(server);
        let replacement = owner(&registry, "/fixture/work").await;
        assert_ne!(replacement.handle().target().instance, fresh.instance);
        assert!(registry.send(&fresh, &request()).await.is_err());
        assert!(replacement.handle().pop().is_none());
    }

    #[tokio::test]
    async fn discovery_distinguishes_busy_live_owner_from_conclusively_stale_socket() {
        let (_directory, registry) = registry();
        let key = profile_key("/fixture/stalled");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(registry.directory.join(format!("{key}.lock")))
            .unwrap();
        lock.try_lock().unwrap();
        let socket = registry.socket(&key);
        let stale = UnixListener::bind(&socket).unwrap();
        assert!(
            registry.discover().await.is_err(),
            "a stalled live owner must not mean zero active profiles"
        );
        drop(lock);
        drop(stale);
        assert!(registry.discover().await.unwrap().is_empty());
        let server = owner(&registry, "/fixture/stalled").await;
        assert_eq!(registry.discover().await.unwrap().len(), 1);
        drop(server);
        assert!(!socket.exists());
        assert!(registry.directory.join(format!("{key}.lock")).exists());
    }

    #[tokio::test]
    async fn malformed_or_oversized_frames_and_links_do_not_enter_the_inbox() {
        let (_directory, registry) = registry();
        let server = owner(&registry, "/fixture/limits").await;
        let target = server.handle().target();
        let mut stream = UnixStream::connect(registry.socket(&target.key))
            .await
            .unwrap();
        stream.write_u32((MAX_FRAME + 1) as u32).await.unwrap();
        assert!(matches!(
            read::<Response>(&mut stream).await.unwrap(),
            Response::Invalid
        ));
        let invalid = Request {
            id: "0".repeat(32),
            link: Some(Link("https://evil.example.org".into())),
        };
        assert!(registry.send(&target, &invalid).await.is_err());
        assert!(server.handle().pop().is_none());
        server
            .handle()
            .update(Some("@fixture:example.org"), "", false);
        assert!(registry.discover().await.unwrap().is_empty());
        assert!(registry.send(&target, &request()).await.is_err());
    }

    #[test]
    fn native_queue_admission_and_exit_are_bounded_and_atomic() {
        let queue = Handle::local();
        assert!(queue.native_link("matrix:roomid/fixture:example.org"));
        assert!(!queue.close_if_empty());
        for _ in 1..MAX_PENDING {
            assert!(queue.native_link("matrix:roomid/fixture:example.org"));
        }
        assert!(!queue.native_link("matrix:roomid/fixture:example.org"));
        for _ in 0..MAX_PENDING {
            queue.pop().unwrap();
        }
        assert!(queue.close_if_empty());
        assert!(!queue.native_link("matrix:roomid/fixture:example.org"));
        let cancelled = Handle::local();
        assert!(cancelled.native_link("matrix:roomid/fixture:example.org"));
        cancelled.cancel();
        assert_eq!(cancelled.pending(), 0);
        assert!(!cancelled.native_link("matrix:roomid/fixture:example.org"));
    }

    #[tokio::test]
    async fn private_directory_and_symlink_checks_prevent_endpoint_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let public = directory.path().join("public");
        fs::create_dir(&public).unwrap();
        fs::set_permissions(&public, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Registry::open(public).is_err());
        let symlink = directory.path().join("alias");
        std::os::unix::fs::symlink(directory.path(), &symlink).unwrap();
        assert!(Registry::open(symlink.clone()).is_err());
        assert!(canonical_profile(symlink.to_str().unwrap()).is_err());
        let registry = Registry::open(directory.path().join("ipc")).unwrap();
        let destination = directory.path().join("untouched");
        fs::write(&destination, "fixture").unwrap();
        std::os::unix::fs::symlink(
            &destination,
            registry
                .directory
                .join(format!("{}.lock", profile_key("/fixture/work"))),
        )
        .unwrap();
        assert!(registry.claim("/fixture/work").await.is_err());
        assert_eq!(fs::read_to_string(destination).unwrap(), "fixture");
    }

    /// Test-only subprocess fixture for native desktop validation. Its registry
    /// is supplied explicitly and never registers fake profiles system-wide.
    #[test]
    fn fixture_process() {
        let Some(directory) = std::env::var_os("INTERCONNECT_ACTIVATION_FIXTURE") else {
            return;
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let root = PathBuf::from(directory);
            let registry = Registry::open(root.join("interconnect-activation")).unwrap();
            let only = std::env::var("INTERCONNECT_ACTIVATION_FIXTURE_SINGLE").ok();
            let mut servers = Vec::new();
            for (label, user, name) in [("personal", "@jamie:example.org", "Jamie"), ("work", "@jamie:work.example.org", "Jamie at work")] {
                if only.as_deref().is_some_and(|only| only != label) { continue; }
                let server = owner(&registry, root.join(label).to_str().unwrap()).await;
                server.handle().update(Some(user), name, true);
                servers.push((label, server));
            }
            assert!(!servers.is_empty());
            fs::write(root.join("ready.json"), serde_json::to_vec(&registry.discover().await.unwrap()).unwrap()).unwrap();
            loop {
                let waits = servers.iter().map(|(_, server)| {
                    let handle = server.handle();
                    Box::pin(async move { handle.notified().await })
                }).collect::<Vec<_>>();
                iced::futures::future::select_all(waits).await;
                for (label, server) in &servers {
                    let handle = server.handle();
                    while let Some(incoming) = handle.pop() {
                        use std::io::Write;
                        let mut log = OpenOptions::new().create(true).append(true).mode(0o600).open(root.join("received.jsonl")).unwrap();
                        writeln!(log, "{}", serde_json::json!({"profile":label,"link":incoming.link.map(|link|link.0)})).unwrap();
                    }
                }
            }
        });
    }
}
