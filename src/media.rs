//! Account-scoped, viewport-driven avatar loading. No polling or animation.
use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, ensure};
use iced::{
    futures::{StreamExt, stream::FuturesUnordered},
    widget::image::Handle,
};
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, MediaRetentionPolicy, MediaThumbnailSettings},
    ruma::{OwnedMxcUri, events::room::MediaSource, uint},
};
use tokio::sync::{Semaphore, watch};
use tokio_util::sync::CancellationToken;

pub const MAX_REQUESTED: usize = 256;
const CONCURRENT_LOADS: usize = 4;
const MEMORY_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENCODED_BYTES: usize = 2 * 1024 * 1024;
const AVATAR_SIZE: u32 = 64;

pub type Images = HashMap<OwnedMxcUri, Handle>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Demand {
    pub uris: Vec<OwnedMxcUri>,
    /// Retry failed visible avatars when sync reconnects, without a timer.
    pub retry_epoch: u64,
}

struct Decoded {
    handle: Handle,
    bytes: usize,
}
struct Entry {
    image: Decoded,
    used: u64,
}
struct Cache {
    entries: HashMap<OwnedMxcUri, Entry>,
    bytes: usize,
    clock: u64,
    limit: usize,
}
impl Cache {
    fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            clock: 0,
            limit,
        }
    }
    fn get(&mut self, uri: &OwnedMxcUri) -> Option<Handle> {
        let entry = self.entries.get_mut(uri)?;
        self.clock += 1;
        entry.used = self.clock;
        Some(entry.image.handle.clone())
    }
    fn insert(&mut self, uri: OwnedMxcUri, image: Decoded) {
        if image.bytes > self.limit {
            return;
        }
        if let Some(previous) = self.entries.remove(&uri) {
            self.bytes -= previous.image.bytes;
        }
        while self.bytes + image.bytes > self.limit {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(uri, _)| uri.clone())
                .expect("nonempty cache exceeds budget");
            self.bytes -= self.entries.remove(&oldest).unwrap().image.bytes;
        }
        self.clock += 1;
        self.bytes += image.bytes;
        self.entries.insert(
            uri,
            Entry {
                image,
                used: self.clock,
            },
        );
    }
}

fn request(uri: OwnedMxcUri) -> MediaRequestParameters {
    // All avatar sizes share one static 64px thumbnail variant in this service.
    MediaRequestParameters {
        source: MediaSource::Plain(uri),
        format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(uint!(64), uint!(64))),
    }
}

fn decode(bytes: Vec<u8>) -> anyhow::Result<Decoded> {
    ensure!(
        bytes.len() <= MAX_ENCODED_BYTES,
        "Avatar response is too large"
    );
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(8 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()?
        .resize_to_fill(
            AVATAR_SIZE,
            AVATAR_SIZE,
            image::imageops::FilterType::Triangle,
        )
        .into_rgba8();
    let bytes = image.into_raw();
    let cost = bytes.len();
    Ok(Decoded {
        handle: Handle::from_rgba(AVATAR_SIZE, AVATAR_SIZE, bytes),
        bytes: cost,
    })
}

async fn load(client: Client, uri: OwnedMxcUri, slots: Arc<Semaphore>) -> anyhow::Result<Decoded> {
    let permit = slots.acquire_owned().await?;
    // The SDK owns authentication, disk caching, and thumbnail request identity.
    let bytes = tokio::time::timeout(
        Duration::from_secs(20),
        client.media().get_media_content(&request(uri), true),
    )
    .await
    .context("Avatar download timed out")??;
    tokio::task::spawn_blocking(move || {
        // Cancellation cannot interrupt a running decoder. Keep its slot until
        // it finishes so rapid navigation cannot create unbounded blocking work.
        let _permit = permit;
        decode(bytes)
    })
    .await?
}

/// One service per connected account. Dropping it cancels network futures;
/// bounded blocking decoders finish under the application's runtime cleanup.
pub async fn run(
    client: Client,
    mut requests: watch::Receiver<Demand>,
    output: watch::Sender<Arc<Images>>,
) -> anyhow::Result<()> {
    // SDK cleanup is checked on media API activity, not on a background timer.
    // Its defaults are 400 MiB total, 20 MiB per file, and 60-day expiry.
    client
        .media()
        .set_media_retention_policy(MediaRetentionPolicy::default())
        .await?;
    let slots = Arc::new(Semaphore::new(CONCURRENT_LOADS));
    let mut cache = Cache::new(MEMORY_BYTES);
    let mut current = Demand::default();
    let mut failed = HashSet::new();
    let mut active: HashMap<OwnedMxcUri, (u64, CancellationToken)> = HashMap::new();
    let mut jobs = FuturesUnordered::new();
    let mut serial = 0_u64;
    loop {
        let next = requests.borrow_and_update().clone();
        let wanted: HashSet<_> = next
            .uris
            .iter()
            .filter(|uri| uri.is_valid())
            .take(MAX_REQUESTED)
            .cloned()
            .collect();
        if current.retry_epoch != next.retry_epoch {
            failed.clear();
        }
        failed.retain(|uri| wanted.contains(uri));
        active.retain(|uri, (_, cancel)| {
            if wanted.contains(uri) {
                true
            } else {
                cancel.cancel();
                false
            }
        });
        current = next;
        let mut images = Images::new();
        for uri in current.uris.iter().filter(|uri| wanted.contains(*uri)) {
            if let Some(handle) = cache.get(uri) {
                images.insert(uri.clone(), handle);
            } else if jobs.len() < CONCURRENT_LOADS
                && !active.contains_key(uri)
                && !failed.contains(uri)
            {
                serial += 1;
                let id = serial;
                let cancel = CancellationToken::new();
                active.insert(uri.clone(), (id, cancel.clone()));
                let client = client.clone();
                let slots = slots.clone();
                let uri = uri.clone();
                let epoch = current.retry_epoch;
                jobs.push(async move {
                    let result = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => None,
                        result = load(client, uri.clone(), slots) => Some(result),
                    };
                    (uri, id, epoch, result)
                });
            }
        }
        output.send_if_modified(|old| {
            if old.len() == images.len()
                && images
                    .iter()
                    .all(|(uri, handle)| old.get(uri) == Some(handle))
            {
                false
            } else {
                *old = Arc::new(images);
                true
            }
        });
        tokio::select! {
            changed = requests.changed() => if changed.is_err() { return Ok(()); },
            Some((uri, id, epoch, result)) = jobs.next(), if !jobs.is_empty() => {
                // A URI can leave and re-enter the viewport before its previous
                // cancelled future completes. Only its current request may win.
                if active.get(&uri).is_some_and(|(current, _)| *current == id) {
                    active.remove(&uri);
                    if let Some(Ok(image)) = result { cache.insert(uri, image); }
                    else if epoch == requests.borrow().retry_epoch { failed.insert(uri); }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, sliding_sync::VersionBuilder};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex, query_param},
    };

    fn uri(name: &str) -> OwnedMxcUri {
        format!("mxc://example.org/{name}").into()
    }
    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([20, 80, 160, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }
    async fn client(server: &MockServer, directory: &std::path::Path) -> Client {
        let client = Client::builder()
            .cross_process_store_config(
                matrix_sdk_common::cross_process_lock::CrossProcessLockConfig::SingleProcess,
            )
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(VersionBuilder::Native)
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
            .sqlite_store(directory, Some("test-only-media-passphrase"))
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(json!({
                    "user_id":"@test:example.org", "device_id":"TEST", "access_token":"fake"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        client
    }
    async fn versions(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(server)
            .await;
    }
    async fn wait_image(images: &mut watch::Receiver<Arc<Images>>, key: &OwnedMxcUri) -> Handle {
        tokio::time::timeout(
            Duration::from_secs(5),
            images.wait_for(|images| images.contains_key(key)),
        )
        .await
        .unwrap()
        .unwrap()
        .get(key)
        .unwrap()
        .clone()
    }

    #[test]
    fn decode_limits_and_lru_account_for_decoded_bytes() {
        assert!(decode(vec![0; MAX_ENCODED_BYTES + 1]).is_err());
        assert!(decode(b"invalid image".to_vec()).is_err());
        assert!(decode(png(1025, 1)).is_err());
        let cost = (AVATAR_SIZE * AVATAR_SIZE * 4) as usize;
        let mut cache = Cache::new(cost * 2);
        let image = decode(png(128, 32)).unwrap();
        assert_eq!(image.bytes, cost);
        cache.insert(uri("a"), image);
        cache.insert(uri("b"), decode(png(1, 1)).unwrap());
        assert!(cache.get(&uri("a")).is_some());
        cache.insert(uri("c"), decode(png(1, 1)).unwrap());
        assert!(cache.get(&uri("b")).is_none());
        assert!(cache.get(&uri("a")).is_some());
        assert_eq!(cache.bytes, cost * 2);
        cache.insert(uri("a"), decode(png(1, 1)).unwrap());
        assert_eq!(
            cache.bytes,
            cost * 2,
            "replacement must not double-count pixels"
        );
    }

    #[tokio::test]
    async fn avatars_deduplicate_reuse_memory_and_reopen_from_sqlite_offline() {
        let directory = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        versions(&server).await;
        Mock::given(method("GET"))
            .and(path_regex(".*/thumbnail/example.org/avatar"))
            .and(query_param("width", "64"))
            .and(query_param("height", "64"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png(16, 16)))
            .expect(1)
            .mount(&server)
            .await;
        let first = client(&server, directory.path()).await;
        let key = first
            .encryption()
            .get_own_device()
            .await
            .unwrap()
            .unwrap()
            .ed25519_key();
        let avatar = uri("avatar");
        let (control, requests) = watch::channel(Demand {
            uris: vec![avatar.clone(), avatar.clone()],
            ..Default::default()
        });
        let (output, mut images) = watch::channel(Arc::default());
        let worker = tokio::spawn(run(first.clone(), requests, output));
        let handle = wait_image(&mut images, &avatar).await;
        control.send_replace(Demand::default());
        tokio::time::timeout(
            Duration::from_secs(2),
            images.wait_for(|images| images.is_empty()),
        )
        .await
        .unwrap()
        .unwrap();
        control.send_replace(Demand {
            uris: vec![avatar.clone()],
            ..Default::default()
        });
        assert_eq!(wait_image(&mut images, &avatar).await, handle);
        drop(control);
        worker.await.unwrap().unwrap();
        drop(first);
        // Remove every HTTP mock. The new service must use the account's disk
        // cache, with no media request or surviving decoded-image cache.
        server.verify().await;
        server.reset().await;
        let restored = client(&server, directory.path()).await;
        assert_eq!(
            restored
                .encryption()
                .get_own_device()
                .await
                .unwrap()
                .unwrap()
                .ed25519_key(),
            key
        );
        let (control, requests) = watch::channel(Demand {
            uris: vec![avatar.clone()],
            ..Default::default()
        });
        let (output, mut images) = watch::channel(Arc::default());
        let worker = tokio::spawn(run(restored, requests, output));
        wait_image(&mut images, &avatar).await;
        let received = server.received_requests().await.unwrap();
        assert!(
            !received.iter().any(
                |r| r.url.path().contains("/thumbnail/") || r.url.path().contains("/download/")
            ),
            "unexpected offline requests: {:?}",
            received.iter().map(|r| r.url.path()).collect::<Vec<_>>()
        );
        drop(control);
        worker.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn viewport_changes_cancel_downloads_and_reconnect_retries_failed_avatars() {
        let directory = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        versions(&server).await;
        let (seen, mut calls) = tokio::sync::mpsc::unbounded_channel();
        let seen_failures = seen.clone();
        Mock::given(method("GET"))
            .and(path_regex(".*/thumbnail/.*"))
            .respond_with(move |request: &wiremock::Request| {
                let name = request.url.path().rsplit('/').next().unwrap().to_owned();
                seen.send(name.clone()).unwrap();
                if name == "fresh" {
                    ResponseTemplate::new(200).set_body_bytes(png(16, 16))
                } else {
                    ResponseTemplate::new(200)
                        .set_body_bytes(png(16, 16))
                        .set_delay(Duration::from_secs(60))
                }
            })
            .mount(&server)
            .await;
        let failed_once = std::sync::atomic::AtomicBool::new(false);
        Mock::given(method("GET"))
            .and(path_regex(".*/thumbnail/example.org/retry"))
            .respond_with(move |_: &wiremock::Request| {
                seen_failures.send("retry".into()).unwrap();
                if failed_once.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    ResponseTemplate::new(200).set_body_bytes(png(16, 16))
                } else {
                    ResponseTemplate::new(404)
                        .set_body_json(json!({"errcode":"M_NOT_FOUND", "error":"missing fixture"}))
                }
            })
            .with_priority(1)
            .mount(&server)
            .await;
        let client = client(&server, directory.path()).await;
        let (control, requests) = watch::channel(Demand {
            uris: (0..12).map(|i| uri(&format!("slow{i}"))).collect(),
            ..Default::default()
        });
        let (output, mut images) = watch::channel(Arc::default());
        let worker = tokio::spawn(run(client, requests, output));
        for _ in 0..CONCURRENT_LOADS {
            tokio::time::timeout(Duration::from_secs(5), calls.recv())
                .await
                .unwrap()
                .unwrap();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), calls.recv())
                .await
                .is_err(),
            "downloads must respect the concurrency bound"
        );
        let fresh = uri("fresh");
        let retry = uri("retry");
        control.send_replace(Demand {
            uris: vec![fresh.clone(), retry.clone()],
            ..Default::default()
        });
        wait_image(&mut images, &fresh).await;
        assert!(images.borrow().keys().all(|key| key == &fresh));
        // Ensure the failed request was actually attempted before reconnecting.
        tokio::time::timeout(Duration::from_secs(5), async {
            while calls.recv().await.unwrap() != "retry" {}
        })
        .await
        .unwrap();
        control.send_replace(Demand {
            uris: vec![fresh.clone(), retry.clone()],
            retry_epoch: 1,
        });
        wait_image(&mut images, &retry).await;
        assert_eq!(images.borrow().len(), 2);
        drop(control);
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
