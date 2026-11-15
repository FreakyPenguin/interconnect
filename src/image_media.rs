//! Image-message metadata and requests. Encryption remains owned by the SDK.
use std::sync::Arc;

use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    ruma::{
        events::room::{MediaSource, message::MessageType},
        uint,
    },
};
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineItemContent};

use crate::media::{DecodeLimits, Decoded, ImageRequest};

pub const MAX_PREVIEWS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Variant {
    Thumbnail,
    ThumbnailFile,
    Original,
}

/// Account-scoped identity includes the complete encryption descriptor and
/// requested variant. Never include its serialized key material in Debug.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Key {
    source: Arc<str>,
    variant: Variant,
}
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageKey")
            .field("variant", &self.variant)
            .finish_non_exhaustive()
    }
}
impl Key {
    fn new(source: &MediaSource, variant: Variant) -> Self {
        Self {
            source: serde_json::to_string(source)
                .expect("media source serialization")
                .into(),
            variant,
        }
    }
    fn source(&self) -> MediaSource {
        serde_json::from_str(&self.source).expect("internally serialized media source")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub preview: Option<Key>,
    pub original: Key,
}
impl Attachment {
    pub fn from_content(content: &TimelineItemContent) -> Option<Self> {
        let TimelineItemContent::MsgLike(content) = content else {
            return None;
        };
        let MsgLikeKind::Message(message) = &content.kind else {
            return None;
        };
        let MessageType::Image(image) = message.msgtype() else {
            return None;
        };
        let attachment = Self::new(
            &image.source,
            image
                .info
                .as_ref()
                .and_then(|info| info.thumbnail_source.as_ref()),
        );
        attachment
            .original
            .parameters()
            .uri()
            .is_valid()
            .then_some(attachment)
    }
    pub fn new(source: &MediaSource, thumbnail: Option<&MediaSource>) -> Self {
        let preview = match thumbnail {
            Some(source @ MediaSource::Encrypted(_)) => {
                Some(Key::new(source, Variant::ThumbnailFile))
            }
            Some(source @ MediaSource::Plain(_)) => Some(Key::new(source, Variant::Thumbnail)),
            None if matches!(source, MediaSource::Plain(_)) => {
                Some(Key::new(source, Variant::Thumbnail))
            }
            // The SDK ignores thumbnail dimensions for encrypted sources and
            // downloads their original. Require a click if no thumbnail exists.
            None => None,
        };
        Self {
            preview: preview.filter(|key| key.parameters().uri().is_valid()),
            original: Key::new(source, Variant::Original),
        }
    }
}
impl ImageRequest for Key {
    const MEMORY_BYTES: usize = 96 * 1024 * 1024;
    // One original (<=16 MiB), GIF frames (<=64 MiB), and previews (<=12 MiB).
    const MAX_REQUESTED: usize = MAX_PREVIEWS + 1;
    const RETAIN_READY: bool = true;
    fn parameters(&self) -> MediaRequestParameters {
        MediaRequestParameters {
            source: self.source(),
            format: match self.variant {
                Variant::Thumbnail => {
                    MediaFormat::Thumbnail(MediaThumbnailSettings::new(uint!(512), uint!(384)))
                }
                Variant::ThumbnailFile | Variant::Original => MediaFormat::File,
            },
        }
    }
    fn decode(&self, bytes: Vec<u8>) -> anyhow::Result<Decoded> {
        let original = self.variant == Variant::Original;
        let limits = DecodeLimits {
            encoded: if original {
                20 * 1024 * 1024
            } else {
                2 * 1024 * 1024
            },
            dimension: 4096,
            allocation: 128 * 1024 * 1024,
            target: if original { (2048, 2048) } else { (512, 384) },
            crop: false,
        };
        if original && matches!(image::guess_format(&bytes), Ok(image::ImageFormat::Gif)) {
            crate::media::decode_gif(bytes, limits)
        } else {
            crate::media::decode_with(bytes, limits)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{Demand, Snapshot};
    use matrix_sdk::{Client, authentication::matrix::MatrixSession, sliding_sync::VersionBuilder};
    use serde_json::json;
    use std::{
        io::{Cursor, Read},
        time::Duration,
    };
    use tokio::sync::{Semaphore, watch};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex, query_param},
    };

    fn png() -> Vec<u8> {
        let pixels = image::RgbaImage::from_pixel(800, 400, image::Rgba([30, 80, 160, 255]));
        let mut bytes = Cursor::new(Vec::new());
        pixels
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    fn encrypted(name: &str, bytes: Vec<u8>) -> (MediaSource, Vec<u8>) {
        let mut cursor = Cursor::new(bytes);
        let mut encryptor = matrix_sdk_crypto::AttachmentEncryptor::new(&mut cursor);
        let mut ciphertext = Vec::new();
        encryptor.read_to_end(&mut ciphertext).unwrap();
        let mut file = serde_json::to_value(encryptor.finish()).unwrap();
        file["url"] = json!(format!("mxc://example.org/{name}"));
        let source = serde_json::from_value(json!({"file":file})).unwrap();
        (source, ciphertext)
    }
    async fn client(server: &MockServer, directory: &std::path::Path) -> Client {
        let client = Client::builder()
            .homeserver_url(server.uri())
            .cross_process_store_config(
                matrix_sdk_common::cross_process_lock::CrossProcessLockConfig::SingleProcess,
            )
            .sliding_sync_version_builder(VersionBuilder::Native)
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
            .sqlite_store(directory, Some("test-only-image-passphrase"))
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
    async fn ready(images: &mut watch::Receiver<Snapshot<Key>>, key: &Key) {
        tokio::time::timeout(
            Duration::from_secs(5),
            images.wait_for(|images| images.ready.contains_key(key)),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[test]
    fn encrypted_original_requires_a_click_and_key_identity_includes_encryption_info() {
        let (source, _) = encrypted("original", png());
        let attachment = Attachment::new(&source, None);
        assert!(attachment.preview.is_none());
        assert!(matches!(
            attachment.original.parameters().format,
            MediaFormat::File
        ));
        let (different_key, _) = encrypted("original", png());
        assert_ne!(
            attachment.original,
            Attachment::new(&different_key, None).original
        );
        let debug = format!("{:?}", attachment.original);
        assert!(!debug.contains("example.org") && !debug.contains("key"));
        let preview = Attachment::new(&source, Some(&different_key))
            .preview
            .unwrap();
        assert!(matches!(preview.parameters().format, MediaFormat::File));
        let decoded = preview.decode(png()).unwrap();
        assert!(decoded.bytes <= 512 * 384 * 4);
        assert!(preview.decode(vec![0; 2 * 1024 * 1024 + 1]).is_err());
    }

    #[tokio::test]
    async fn plain_preview_defers_original_and_both_reopen_from_disk() {
        cache_fixture(false, false).await;
    }
    #[tokio::test]
    async fn encrypted_thumbnail_and_original_decrypt_and_reopen_from_disk() {
        cache_fixture(true, false).await;
    }
    #[tokio::test]
    async fn plain_and_encrypted_gifs_keep_previews_static_and_reopen_animations_offline() {
        cache_fixture(false, true).await;
        cache_fixture(true, true).await;
    }
    fn gif() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [30, 60] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        image::RgbaImage::from_pixel(8, 8, image::Rgba([color, 80, 160, 255])),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(100, 1),
                    ))
                    .unwrap();
            }
        }
        bytes
    }
    #[tokio::test]
    async fn invalid_encrypted_content_is_reported_as_failed() {
        let directory = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let (source, mut ciphertext) = encrypted("corrupt", png());
        ciphertext[0] ^= 1;
        Mock::given(method("GET"))
            .and(path_regex(".*/download/example.org/corrupt"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(ciphertext))
            .mount(&server)
            .await;
        let key = Attachment::new(&source, None).original;
        let client = client(&server, directory.path()).await;
        let (control, requests) = watch::channel(Demand {
            uris: vec![key.clone()],
            ..Default::default()
        });
        let (output, mut images) = watch::channel(Snapshot::default());
        let worker = tokio::spawn(crate::media::run_requests(
            client,
            requests,
            output,
            Arc::new(Semaphore::new(4)),
        ));
        tokio::time::timeout(
            Duration::from_secs(5),
            images.wait_for(|images| images.failed.contains(&key)),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(images.borrow().ready.is_empty());
        drop(control);
        worker.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn avatars_and_attachments_share_the_four_load_slots() {
        let directory = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let (seen, mut requests_seen) = tokio::sync::mpsc::unbounded_channel();
        Mock::given(method("GET"))
            .and(path_regex(".*/thumbnail/.*"))
            .respond_with(move |_: &wiremock::Request| {
                seen.send(()).unwrap();
                ResponseTemplate::new(200)
                    .set_body_bytes(vec![0])
                    .set_delay(Duration::from_secs(60))
            })
            .mount(&server)
            .await;
        let client = client(&server, directory.path()).await;
        let slots = Arc::new(Semaphore::new(4));
        let (avatars, requests) = watch::channel(Demand::<matrix_sdk::ruma::OwnedMxcUri> {
            uris: (0..8)
                .map(|i| format!("mxc://example.org/avatar{i}").into())
                .collect(),
            ..Default::default()
        });
        let (output, _avatar_updates) = watch::channel(Snapshot::default());
        let avatar_worker = tokio::spawn(crate::media::run_requests(
            client.clone(),
            requests,
            output,
            slots.clone(),
        ));
        let (images, requests) = watch::channel(Demand {
            uris: (0..8)
                .map(|i| {
                    Attachment::new(
                        &MediaSource::Plain(format!("mxc://example.org/image{i}").into()),
                        None,
                    )
                    .preview
                    .unwrap()
                })
                .collect(),
            ..Default::default()
        });
        let (output, _image_updates) = watch::channel(Snapshot::default());
        let image_worker =
            tokio::spawn(crate::media::run_requests(client, requests, output, slots));
        for _ in 0..4 {
            tokio::time::timeout(Duration::from_secs(5), requests_seen.recv())
                .await
                .unwrap()
                .unwrap();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), requests_seen.recv())
                .await
                .is_err()
        );
        drop(avatars);
        drop(images);
        tokio::time::timeout(Duration::from_secs(2), avatar_worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), image_worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    async fn cache_fixture(encryption: bool, animated: bool) {
        let directory = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let pixels = || if animated { gif() } else { png() };
        let (source, original_bytes) = if encryption {
            encrypted("original", pixels())
        } else {
            (
                MediaSource::Plain("mxc://example.org/original".into()),
                pixels(),
            )
        };
        let (thumbnail, thumbnail_bytes) = if encryption {
            encrypted("preview", pixels())
        } else {
            (
                MediaSource::Plain("mxc://example.org/original".into()),
                pixels(),
            )
        };
        let attachment = Attachment::new(&source, encryption.then_some(&thumbnail));
        let preview = attachment.preview.clone().unwrap();
        let original = attachment.original;
        if encryption {
            Mock::given(method("GET"))
                .and(path_regex(".*/download/example.org/preview"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(thumbnail_bytes))
                .expect(1)
                .mount(&server)
                .await;
        } else {
            Mock::given(method("GET"))
                .and(path_regex(".*/thumbnail/example.org/original"))
                .and(query_param("width", "512"))
                .and(query_param("height", "384"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(thumbnail_bytes))
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path_regex(".*/download/example.org/original"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(original_bytes))
            .expect(1)
            .mount(&server)
            .await;
        let first = client(&server, directory.path()).await;
        let (control, requests) = watch::channel(Demand {
            uris: vec![preview.clone(), preview.clone()],
            ..Default::default()
        });
        let (output, mut images) = watch::channel(Snapshot::default());
        let worker = tokio::spawn(crate::media::run_requests(
            first.clone(),
            requests,
            output,
            Arc::new(Semaphore::new(4)),
        ));
        ready(&mut images, &preview).await;
        assert!(images.borrow().animations.is_empty());
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|r| r.url.path().ends_with("/download/example.org/original")),
            "preview must not fetch the original"
        );
        control.send_replace(Demand {
            uris: vec![original.clone(), preview.clone()],
            ..Default::default()
        });
        ready(&mut images, &original).await;
        assert!(images.borrow().failed.is_empty());
        assert_eq!(images.borrow().animations.contains_key(&original), animated);
        if animated {
            assert_eq!(
                images.borrow().animations[&original]
                    .as_ref()
                    .unwrap()
                    .frames
                    .len(),
                2
            );
        }
        assert!(!images.borrow().animations.contains_key(&preview));
        let cached_id = images.borrow().ready[&original].id();
        // Closing and reopening must keep the same decoded handle available
        // synchronously, rather than sending empty/loading/ready snapshots.
        for _ in 0..3 {
            images.borrow_and_update();
            control.send_replace(Demand {
                uris: vec![preview.clone()],
                ..Default::default()
            });
            assert!(
                tokio::time::timeout(Duration::from_millis(50), images.changed())
                    .await
                    .is_err()
            );
            assert_eq!(images.borrow().ready[&original].id(), cached_id);
            control.send_replace(Demand {
                uris: vec![original.clone()],
                ..Default::default()
            });
            assert!(
                tokio::time::timeout(Duration::from_millis(50), images.changed())
                    .await
                    .is_err()
            );
            assert_eq!(images.borrow().ready[&original].id(), cached_id);
        }
        drop(control);
        worker.await.unwrap().unwrap();
        drop(first);
        server.verify().await;
        server.reset().await;
        let restored = client(&server, directory.path()).await;
        let (control, requests) = watch::channel(Demand {
            uris: vec![original.clone(), preview.clone()],
            ..Default::default()
        });
        let (output, mut images) = watch::channel(Snapshot::default());
        let worker = tokio::spawn(crate::media::run_requests(
            restored,
            requests,
            output,
            Arc::new(Semaphore::new(4)),
        ));
        ready(&mut images, &original).await;
        ready(&mut images, &preview).await;
        assert_eq!(images.borrow().animations.contains_key(&original), animated);
        assert!(
            !server.received_requests().await.unwrap().iter().any(|r| r
                .url
                .path()
                .contains("/download/")
                || r.url.path().contains("/thumbnail/"))
        );
        drop(control);
        worker.await.unwrap().unwrap();
    }
}
