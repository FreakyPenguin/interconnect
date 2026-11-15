//! Account-scoped, viewport-driven image loading. No polling.
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
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

pub type Images<K = OwnedMxcUri> = HashMap<K, Handle>;
pub type Animations<K> = HashMap<K, Result<Arc<Animation>, &'static str>>;

/// Composited frames decoded on a blocking worker. Playback is owned by the UI.
#[derive(Clone, Debug)]
pub struct Animation {
    pub frames: Vec<(Handle, Duration)>,
}

#[derive(Clone, Debug)]
pub struct Snapshot<K = OwnedMxcUri> {
    pub ready: Arc<Images<K>>,
    pub failed: Arc<HashSet<K>>,
    pub animations: Arc<Animations<K>>,
}
impl<K> Default for Snapshot<K> {
    fn default() -> Self {
        Self {
            ready: Arc::default(),
            failed: Arc::default(),
            animations: Arc::default(),
        }
    }
}

pub(crate) trait ImageRequest: Clone + Eq + Hash + Send + Sync + 'static {
    const MEMORY_BYTES: usize;
    const MAX_REQUESTED: usize;
    const RETAIN_READY: bool = false;
    fn parameters(&self) -> MediaRequestParameters;
    fn decode(&self, bytes: Vec<u8>) -> anyhow::Result<Decoded>;
}
impl ImageRequest for OwnedMxcUri {
    const MEMORY_BYTES: usize = MEMORY_BYTES;
    const MAX_REQUESTED: usize = MAX_REQUESTED;
    fn parameters(&self) -> MediaRequestParameters {
        request(self.clone())
    }
    fn decode(&self, bytes: Vec<u8>) -> anyhow::Result<Decoded> {
        decode(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Demand<K = OwnedMxcUri> {
    pub uris: Vec<K>,
    /// Retry failed visible avatars when sync reconnects, without a timer.
    pub retry_epoch: u64,
}
impl<K> Default for Demand<K> {
    fn default() -> Self {
        Self {
            uris: Vec::new(),
            retry_epoch: 0,
        }
    }
}

pub(crate) struct Decoded {
    handle: Handle,
    animation: Option<Result<Arc<Animation>, &'static str>>,
    pub(crate) bytes: usize,
}
struct Entry {
    image: Decoded,
    used: u64,
}
struct Cache<K = OwnedMxcUri> {
    entries: HashMap<K, Entry>,
    bytes: usize,
    clock: u64,
    limit: usize,
}
impl<K: Eq + Hash + Clone> Cache<K> {
    fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            clock: 0,
            limit,
        }
    }
    fn get(&mut self, uri: &K) -> Option<Handle> {
        let entry = self.entries.get_mut(uri)?;
        self.clock += 1;
        entry.used = self.clock;
        Some(entry.image.handle.clone())
    }
    fn insert(&mut self, uri: K, image: Decoded) {
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
    decode_with(
        bytes,
        DecodeLimits {
            encoded: MAX_ENCODED_BYTES,
            dimension: 1024,
            allocation: 8 * 1024 * 1024,
            target: (AVATAR_SIZE, AVATAR_SIZE),
            crop: true,
        },
    )
}

pub(crate) struct DecodeLimits {
    pub encoded: usize,
    pub dimension: u32,
    pub allocation: u64,
    pub target: (u32, u32),
    pub crop: bool,
}
pub(crate) fn decode_with(bytes: Vec<u8>, limits: DecodeLimits) -> anyhow::Result<Decoded> {
    ensure!(bytes.len() <= limits.encoded, "Image response is too large");
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut decoder_limits = image::Limits::default();
    decoder_limits.max_image_width = Some(limits.dimension);
    decoder_limits.max_image_height = Some(limits.dimension);
    decoder_limits.max_alloc = Some(limits.allocation);
    reader.limits(decoder_limits);
    let image = reader.decode()?;
    let (width, height) = limits.target;
    let image = if limits.crop {
        image.resize_to_fill(width, height, image::imageops::FilterType::Triangle)
    } else {
        image.thumbnail(width.min(image.width()), height.min(image.height()))
    }
    .into_rgba8();
    let (width, height) = image.dimensions();
    let bytes = image.into_raw();
    let cost = bytes.len();
    Ok(Decoded {
        handle: Handle::from_rgba(width, height, bytes),
        bytes: cost,
        animation: None,
    })
}

/// Decode animation only for originals explicitly opened by the user. Keep a
/// static fallback if a later frame is malformed or exceeds the playback limits.
pub(crate) fn decode_gif(bytes: Vec<u8>, limits: DecodeLimits) -> anyhow::Result<Decoded> {
    use image::{AnimationDecoder, ImageDecoder};
    ensure!(bytes.len() <= limits.encoded, "Image response is too large");
    let mut decoded = decode_with(bytes.clone(), limits)?;
    let animation = (|| -> anyhow::Result<Option<Arc<Animation>>> {
        let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(128 * 1024 * 1024);
        decoder.set_limits(limits)?;
        let (width, height) = decoder.dimensions();
        let source_pixels = u64::from(width) * u64::from(height);
        let mut frames: Vec<(Handle, Duration)> = Vec::new();
        let mut target = 512;
        let mut cost = 0;
        let mut iterator = decoder.into_frames();
        loop {
            // Bound total decoding work as well as retained pixels. Probe only
            // one extra frame at the boundary to distinguish EOF from overflow.
            let Some(frame) = iterator.next() else { break };
            ensure!(frames.len() < 512, "Too many GIF frames");
            ensure!(
                source_pixels * (frames.len() as u64 + 1) <= 128 * 1024 * 1024,
                "GIF decode work limit exceeded"
            );
            let frame = frame?;
            let (numerator, denominator) = frame.delay().numer_denom_ms();
            let delay =
                Duration::from_secs_f64(f64::from(numerator) / f64::from(denominator) / 1000.0)
                    .max(Duration::from_millis(20));
            let image = image::DynamicImage::ImageRgba8(frame.into_buffer());
            let mut image = image.thumbnail(target.min(image.width()), target.min(image.height()));
            // Reduce resolution uniformly instead of rejecting ordinary longer
            // GIFs just because they were initially decoded at a larger size.
            while cost + image.as_bytes().len() > 64 * 1024 * 1024 {
                target /= 2;
                ensure!(target >= 128, "GIF frame memory limit exceeded");
                cost = 0;
                for (handle, _) in &mut frames {
                    let Handle::Rgba {
                        width,
                        height,
                        pixels,
                        ..
                    } = handle
                    else {
                        unreachable!("decoded RGBA")
                    };
                    let pixels = image::RgbaImage::from_raw(*width, *height, pixels.to_vec())
                        .expect("decoded dimensions");
                    let resized = image::DynamicImage::ImageRgba8(pixels)
                        .thumbnail(target.min(*width), target.min(*height))
                        .into_rgba8();
                    let (width, height) = resized.dimensions();
                    let pixels = resized.into_raw();
                    cost += pixels.len();
                    *handle = Handle::from_rgba(width, height, pixels);
                }
                image = image.thumbnail(target.min(image.width()), target.min(image.height()));
            }
            let image = image.into_rgba8();
            let (width, height) = image.dimensions();
            let pixels = image.into_raw();
            cost += pixels.len();
            frames.push((Handle::from_rgba(width, height, pixels), delay));
        }
        if frames.len() < 2 {
            return Ok(None);
        }
        decoded.bytes += cost;
        Ok(Some(Arc::new(Animation { frames })))
    })();
    decoded.animation = match animation {
        Ok(animation) => animation.map(Ok),
        Err(_) => Some(Err(
            "Animation unavailable: GIF is damaged or exceeds playback limits.",
        )),
    };
    Ok(decoded)
}

async fn load<K: ImageRequest>(
    client: Client,
    key: K,
    slots: Arc<Semaphore>,
) -> anyhow::Result<Decoded> {
    let permit = slots.acquire_owned().await?;
    let bytes = tokio::time::timeout(
        Duration::from_secs(20),
        client.media().get_media_content(&key.parameters(), true),
    )
    .await
    .context("Image download timed out")??;
    tokio::task::spawn_blocking(move || {
        // Keep the slot until a cancelled request's blocking decoder finishes.
        let _permit = permit;
        key.decode(bytes)
    })
    .await?
}

/// One service per connected account. Dropping it cancels network futures;
/// bounded blocking decoders finish under the application's runtime cleanup.
pub async fn run(
    client: Client,
    requests: watch::Receiver<Demand>,
    output: watch::Sender<Snapshot>,
) -> anyhow::Result<()> {
    run_requests(
        client,
        requests,
        output,
        Arc::new(Semaphore::new(CONCURRENT_LOADS)),
    )
    .await
}

pub(crate) async fn run_requests<K: ImageRequest>(
    client: Client,
    mut requests: watch::Receiver<Demand<K>>,
    output: watch::Sender<Snapshot<K>>,
    slots: Arc<Semaphore>,
) -> anyhow::Result<()> {
    // SDK cleanup is checked on media API activity, not on a background timer.
    // Its defaults are 400 MiB total, 20 MiB per file, and 60-day expiry.
    client
        .media()
        .set_media_retention_policy(MediaRetentionPolicy::default())
        .await?;
    let mut cache = Cache::new(K::MEMORY_BYTES);
    let mut current_epoch = 0;
    let mut failed = HashSet::new();
    let mut active: HashMap<K, (u64, CancellationToken)> = HashMap::new();
    let mut jobs = FuturesUnordered::new();
    let mut serial = 0_u64;
    loop {
        let next = requests.borrow_and_update().clone();
        let wanted: HashSet<_> = next
            .uris
            .iter()
            .filter(|uri| uri.parameters().uri().is_valid())
            .take(K::MAX_REQUESTED)
            .cloned()
            .collect();
        if current_epoch != next.retry_epoch {
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
        current_epoch = next.retry_epoch;
        let mut images = Images::new();
        for uri in next.uris.iter().filter(|uri| wanted.contains(*uri)) {
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
                let epoch = current_epoch;
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
        if K::RETAIN_READY {
            // Retain the bounded decoded cache in the presentation snapshot so
            // reopening an original can render immediately, without a loading
            // frame and an asynchronous round trip to rediscover the handle.
            images = cache
                .entries
                .iter()
                .map(|(key, entry)| (key.clone(), entry.image.handle.clone()))
                .collect();
        }
        let animations = cache
            .entries
            .iter()
            .filter(|(key, _)| images.contains_key(*key))
            .filter_map(|(key, entry)| {
                entry
                    .image
                    .animation
                    .clone()
                    .map(|value| (key.clone(), value))
            })
            .collect();
        output.send_if_modified(|old| {
            if old.ready.len() == images.len()
                && images.iter().all(|(uri, handle)| {
                    old.ready
                        .get(uri)
                        .is_some_and(|old| old.id() == handle.id())
                })
                && old.failed.as_ref() == &failed
            {
                false
            } else {
                *old = Snapshot {
                    ready: Arc::new(images),
                    failed: Arc::new(failed.clone()),
                    animations: Arc::new(animations),
                };
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
    async fn wait_image(images: &mut watch::Receiver<Snapshot>, key: &OwnedMxcUri) -> Handle {
        tokio::time::timeout(
            Duration::from_secs(5),
            images.wait_for(|images| images.ready.contains_key(key)),
        )
        .await
        .unwrap()
        .unwrap()
        .ready
        .get(key)
        .unwrap()
        .clone()
    }

    fn gif(count: usize) -> Vec<u8> {
        gif_size(count, 2)
    }
    fn gif_size(count: usize, size: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            encoder
                .set_repeat(image::codecs::gif::Repeat::Infinite)
                .unwrap();
            for i in 0..count {
                let pixels =
                    image::RgbaImage::from_pixel(size, size, image::Rgba([i as u8, 80, 160, 255]));
                encoder
                    .encode_frame(image::Frame::from_parts(
                        pixels,
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(if i == 0 { 0 } else { 70 }, 1),
                    ))
                    .unwrap();
            }
        }
        bytes
    }
    fn gif_limits() -> DecodeLimits {
        DecodeLimits {
            encoded: 1024 * 1024,
            dimension: 4096,
            allocation: 128 * 1024 * 1024,
            target: (2048, 2048),
            crop: false,
        }
    }
    #[test]
    fn gif_frames_preserve_pixels_and_delay_and_count_toward_lru_budget() {
        let decoded = decode_gif(gif(3), gif_limits()).unwrap();
        let frames = &decoded.animation.as_ref().unwrap().as_ref().unwrap().frames;
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].1, Duration::from_millis(20));
        assert_eq!(frames[1].1, Duration::from_millis(70));
        let Handle::Rgba { pixels, .. } = &frames[2].0 else {
            panic!("decoded RGBA")
        };
        assert_eq!(&pixels[..4], &[2, 80, 160, 255]);
        assert_eq!(decoded.bytes, 4 * 2 * 2 * 4); // Still plus three frames.
        let mut cache = Cache::new(decoded.bytes);
        cache.insert(uri("animated"), decoded);
        cache.insert(
            uri("replacement"),
            decode_gif(gif(3), gif_limits()).unwrap(),
        );
        assert!(cache.get(&uri("animated")).is_none());
        assert!(cache.get(&uri("replacement")).is_some());
    }
    #[test]
    fn longer_gifs_shrink_all_frames_to_stay_within_the_pixel_budget() {
        let decoded = decode_gif(gif_size(65, 512), gif_limits()).unwrap();
        let animation = decoded.animation.unwrap().unwrap();
        assert_eq!(animation.frames.len(), 65);
        for (frame, _) in &animation.frames {
            assert!(matches!(
                frame,
                Handle::Rgba {
                    width: 256,
                    height: 256,
                    ..
                }
            ));
        }
        assert_eq!(decoded.bytes, 512 * 512 * 4 + 65 * 256 * 256 * 4);
    }
    #[test]
    fn excessive_or_broken_gif_animation_keeps_a_static_fallback() {
        let decoded = decode_gif(gif(513), gif_limits()).unwrap();
        assert!(decoded.animation.unwrap().is_err());
        assert_eq!(decoded.bytes, 16);
        let mut truncated = gif(3);
        truncated.truncate(truncated.len() - 10);
        let decoded = decode_gif(truncated, gif_limits()).unwrap();
        assert!(decoded.animation.unwrap().is_err());
        assert_eq!(decoded.bytes, 16);
        assert!(
            decode_gif(gif(1), gif_limits())
                .unwrap()
                .animation
                .is_none()
        );
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
        let (output, mut images) = watch::channel(Snapshot::default());
        let worker = tokio::spawn(run(first.clone(), requests, output));
        let handle = wait_image(&mut images, &avatar).await;
        control.send_replace(Demand::default());
        tokio::time::timeout(
            Duration::from_secs(2),
            images.wait_for(|images| images.ready.is_empty()),
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
        let (output, mut images) = watch::channel(Snapshot::default());
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
        let (output, mut images) = watch::channel(Snapshot::default());
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
        assert!(images.borrow().ready.keys().all(|key| key == &fresh));
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
        assert_eq!(images.borrow().ready.len(), 2);
        drop(control);
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
