//! Local attachment preparation. All filesystem and image-header work stays off the UI thread.
use std::{io::Read, path::PathBuf, sync::Arc};

use matrix_sdk::attachment::{
    AttachmentInfo, BaseAudioInfo, BaseFileInfo, BaseImageInfo, BaseVideoInfo, Thumbnail,
};

pub const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Attachment {
    source: Source,
    pub filename: String,
    pub size: u64,
}

#[derive(Clone)]
enum Source {
    File(PathBuf),
    Memory(Arc<[u8]>),
}
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::File(_) => "File",
            Self::Memory(_) => "Memory",
        })
    }
}

impl Attachment {
    pub async fn from_path(path: PathBuf) -> Result<Self, String> {
        tokio::task::spawn_blocking(move || {
            let metadata = std::fs::metadata(&path).map_err(|_| "Could not access this file.")?;
            validate_file(&metadata)?;
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .ok_or("The filename is not valid Unicode.")?
                .to_owned();
            Ok(Self {
                source: Source::File(path),
                filename,
                size: metadata.len(),
            })
        })
        .await
        .map_err(|_| "File preparation failed.".to_owned())?
    }

    /// Clipboard images stay in memory with the draft, without temporary files.
    pub(crate) fn from_png(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
            return Err("Attachments must be 50 MiB or smaller.".into());
        }
        Ok(Self {
            size: bytes.len() as u64,
            source: Source::Memory(bytes.into()),
            filename: "clipboard.png".into(),
        })
    }

    pub(crate) async fn read(self) -> Result<Prepared, String> {
        tokio::task::spawn_blocking(move || {
            let bytes = match self.source {
                Source::File(path) => {
                    let metadata =
                        std::fs::metadata(&path).map_err(|_| "Could not access this file.")?;
                    validate_file(&metadata)?;
                    let file =
                        std::fs::File::open(&path).map_err(|_| "Could not open this file.")?;
                    let mut bytes = Vec::new();
                    file.take(MAX_ATTACHMENT_BYTES + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| "Could not read this file.")?;
                    if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
                        return Err("Attachments must be 50 MiB or smaller.".into());
                    }
                    bytes
                }
                Source::Memory(bytes) => bytes.to_vec(),
            };
            let mime = image::guess_format(&bytes)
                .ok()
                .and_then(|format| format.to_mime_type().parse().ok())
                .unwrap_or_else(|| mime_guess::from_path(&self.filename).first_or_octet_stream());
            let size = (bytes.len() as u64).try_into().ok();
            let info = match mime.type_().as_str() {
                "image" => {
                    let dimensions = image::ImageReader::new(std::io::Cursor::new(&bytes))
                        .with_guessed_format()
                        .ok()
                        .and_then(|reader| reader.into_dimensions().ok());
                    AttachmentInfo::Image(BaseImageInfo {
                        size,
                        width: dimensions.map(|(width, _)| width.into()),
                        height: dimensions.map(|(_, height)| height.into()),
                        ..Default::default()
                    })
                }
                "video" => AttachmentInfo::Video(BaseVideoInfo {
                    size,
                    ..Default::default()
                }),
                "audio" => AttachmentInfo::Audio(BaseAudioInfo {
                    size,
                    ..Default::default()
                }),
                _ => AttachmentInfo::File(BaseFileInfo { size }),
            };
            let thumbnail = (mime.type_() == mime_guess::mime::IMAGE)
                .then(|| make_thumbnail(&bytes))
                .flatten();
            Ok(Prepared {
                thumbnail,
                bytes,
                filename: self.filename,
                mime,
                info,
            })
        })
        .await
        .map_err(|_| "File preparation failed.".to_owned())?
    }
}

/// Optional, bounded static preview. Unsupported/oversized/damaged images still
/// send their original bytes; never decompress unbounded images to make a preview.
fn make_thumbnail(bytes: &[u8]) -> Option<Thumbnail> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let thumbnail = image.thumbnail(512.min(image.width()), 384.min(image.height()));
    let mut data = std::io::Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut data, image::ImageFormat::Png)
        .ok()?;
    let data = data.into_inner();
    Some(Thumbnail {
        size: (data.len() as u64).try_into().ok()?,
        data,
        content_type: mime_guess::mime::IMAGE_PNG,
        width: thumbnail.width().into(),
        height: thumbnail.height().into(),
    })
}

fn validate_file(metadata: &std::fs::Metadata) -> Result<(), String> {
    if !metadata.is_file() {
        return Err("Choose a regular file.".into());
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        return Err("Attachments must be 50 MiB or smaller.".into());
    }
    Ok(())
}

pub(crate) struct Prepared {
    pub thumbnail: Option<Thumbnail>,
    pub bytes: Vec<u8>,
    pub filename: String,
    pub mime: mime_guess::mime::Mime,
    pub info: AttachmentInfo,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_directories_oversize_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Attachment::from_path(dir.path().into()).await.is_err());
        let path = dir.path().join("large.bin");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_ATTACHMENT_BYTES + 1).unwrap();
        assert!(Attachment::from_path(path).await.is_err());
        assert!(
            Attachment::from_path(dir.path().join("missing"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn generic_files_use_current_bytes_and_file_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "old").unwrap();
        let attachment = Attachment::from_path(path.clone()).await.unwrap();
        std::fs::write(path, "current").unwrap();
        let prepared = attachment.read().await.unwrap();
        assert!(prepared.thumbnail.is_none());
        assert_eq!(prepared.bytes, b"current");
        assert_eq!(prepared.mime.as_ref(), "text/plain");
        assert!(
            matches!(prepared.info, AttachmentInfo::File(info) if info.size == Some(7u32.into()))
        );
    }

    #[test]
    fn thumbnail_generation_is_bounded_and_optional() {
        assert!(make_thumbnail(b"not an image").is_none());
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(4097, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        assert!(make_thumbnail(encoded.get_ref()).is_none());
        encoded.set_position(0);
        encoded.get_mut().clear();
        image::RgbaImage::new(800, 400)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let preview = make_thumbnail(encoded.get_ref()).unwrap();
        assert_eq!(
            (u64::from(preview.width), u64::from(preview.height)),
            (512, 256)
        );
        assert!(preview.data.len() < 2 * 1024 * 1024);
    }

    #[tokio::test]
    async fn reads_images_and_rechecks_files_at_send_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("picture.bin");
        image::RgbaImage::new(3, 2)
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let attachment = Attachment::from_path(path.clone()).await.unwrap();
        let prepared = attachment.clone().read().await.unwrap();
        let preview = prepared.thumbnail.as_ref().unwrap();
        assert_eq!(
            (u64::from(preview.width), u64::from(preview.height)),
            (3, 2)
        );
        assert_eq!(preview.content_type, mime_guess::mime::IMAGE_PNG);
        assert!(image::load_from_memory(&preview.data).is_ok());
        assert_eq!(prepared.mime.as_ref(), "image/png");
        assert!(
            matches!(prepared.info, AttachmentInfo::Image(info) if info.width == Some(3u32.into()) && info.height == Some(2u32.into()))
        );
        std::fs::remove_file(path).unwrap();
        assert!(attachment.read().await.is_err());
    }
}
