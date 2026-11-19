//! Shared room/space avatar preparation and explicit, conflict-checked writes.
use iced::widget::image::Handle;
use matrix_sdk::{
    Room, RoomState,
    ruma::{
        OwnedMxcUri,
        api::client::state::{get_state_event_for_key, send_state_event},
        api::error::ErrorKind,
        events::{
            EmptyStateKey, StateEventType,
            room::{
                avatar::{ImageInfo, RoomAvatarEventContent},
                power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent},
            },
        },
    },
};
use std::{
    io::{Cursor, Read},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

const MAX_BYTES: u64 = 10 * 1024 * 1024;
#[derive(Clone, Debug, Default)]
pub struct Details {
    pub url: Option<OwnedMxcUri>,
    pub editable: bool,
}
#[derive(Clone)]
pub struct Upload(Arc<Data>);
struct Data {
    bytes: Vec<u8>,
    preview: Handle,
    width: u32,
    height: u32,
}
impl std::fmt::Debug for Upload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AvatarUpload")
    }
}
impl Upload {
    pub fn preview(&self) -> &Handle {
        &self.0.preview
    }
}

pub async fn read_image(path: PathBuf) -> Result<Upload, String> {
    let cancel = tokio_util::sync::CancellationToken::new();
    let _guard = cancel.clone().drop_guard();
    tokio::task::spawn_blocking(move || {
        if !std::fs::metadata(&path)
            .map_err(|_| "Could not inspect the image.")?
            .is_file()
        {
            return Err("Choose a regular image file.".into());
        }
        let file = std::fs::File::open(path).map_err(|_| "Could not open the image.")?;
        if !file
            .metadata()
            .map_err(|_| "Could not inspect the image.")?
            .is_file()
        {
            return Err("Choose a regular image file.".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read the image.")?;
        if cancel.is_cancelled() {
            return Err("Avatar preparation cancelled.".into());
        }
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Avatar images must be 10 MiB or smaller.".into());
        }
        prepare(bytes, || cancel.is_cancelled())
    })
    .await
    .map_err(|_| "Avatar preparation failed.".to_owned())?
}
fn prepare(bytes: Vec<u8>, cancelled: impl Fn() -> bool) -> Result<Upload, String> {
    use image::ImageDecoder;
    let format = image::guess_format(&bytes).map_err(|_| "Choose a PNG, JPEG or WebP image.")?;
    if !matches!(
        format,
        image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP
    ) {
        return Err("Choose a PNG, JPEG or WebP image.".into());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "The image is damaged or exceeds 4,096 pixels on a side.")?;
    let orientation = decoder
        .orientation()
        .map_err(|_| "Could not read image orientation.")?;
    let mut image = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| "The image is damaged or too large to decode.")?;
    if cancelled() {
        return Err("Avatar preparation cancelled.".into());
    }
    image.apply_orientation(orientation);
    let image = if image.width() > 512 || image.height() > 512 {
        image.thumbnail(512, 512)
    } else {
        image
    }
    .into_rgba8();
    let (width, height) = image.dimensions();
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|_| "Could not prepare the avatar image.")?;
    if cancelled() {
        return Err("Avatar preparation cancelled.".into());
    }
    Ok(Upload(Arc::new(Data {
        bytes: png.into_inner(),
        preview: Handle::from_rgba(width, height, image.into_raw()),
        width,
        height,
    })))
}
pub(crate) async fn current(room: &Room) -> Result<Option<OwnedMxcUri>, String> {
    let request = get_state_event_for_key::v3::Request::new(
        room.room_id().to_owned(),
        StateEventType::RoomAvatar,
        String::new(),
    );
    match room.client().send(request).await {
        Ok(response) => {
            let content: RoomAvatarEventContent =
                serde_json::from_str(response.event_or_content.get())
                    .map_err(|_| "Could not read the avatar state.")?;
            if content.url.as_ref().is_some_and(|url| !url.is_valid()) {
                return Err("The current avatar URL is invalid.".into());
            }
            Ok(content.url)
        }
        Err(error) if error.client_api_error_kind() == Some(&ErrorKind::NotFound) => Ok(None),
        Err(_) => Err("Could not read the current avatar. Try Reload.".into()),
    }
}
async fn check(room: &Room, baseline: &Option<OwnedMxcUri>) -> Result<(), String> {
    if room.state() != RoomState::Joined {
        return Err("You are no longer joined to this room or space.".into());
    }
    let request = get_state_event_for_key::v3::Request::new(
        room.room_id().to_owned(),
        StateEventType::RoomPowerLevels,
        String::new(),
    );
    let content: Option<RoomPowerLevelsEventContent> = match room.client().send(request).await {
        Ok(response) => Some(
            serde_json::from_str(response.event_or_content.get())
                .map_err(|_| "Could not read avatar permissions.")?,
        ),
        Err(error) if error.client_api_error_kind() == Some(&ErrorKind::NotFound) => None,
        Err(_) => return Err("Could not check avatar permissions.".into()),
    };
    let rules = room.clone_info().room_version_rules_or_default();
    let levels = RoomPowerLevels::new(
        content.into(),
        &rules.authorization,
        room.creators().ok_or("Unknown room creators.")?,
    );
    if !levels.user_can_send_state(room.own_user_id(), StateEventType::RoomAvatar) {
        return Err("You no longer have permission to change this avatar.".into());
    }
    if current(room).await? != *baseline {
        return Err("The avatar changed. Reload before saving again.".into());
    }
    Ok(())
}
pub(crate) async fn save(
    room: &Room,
    baseline: Option<OwnedMxcUri>,
    upload: Option<Upload>,
) -> Result<Option<OwnedMxcUri>, String> {
    let operation = async {
        check(room, &baseline).await?;
        let mut content = RoomAvatarEventContent::new();
        if let Some(upload) = upload {
            let response = room
                .client()
                .media()
                .upload(&mime_guess::mime::IMAGE_PNG, upload.0.bytes.clone(), None)
                .await
                .map_err(|_| "Could not confirm avatar upload. Reload before retrying.")?;
            if !response.content_uri.is_valid() {
                return Err("The server returned an invalid avatar URL.".into());
            }
            // A remote edit or permission change during upload must prevent publishing.
            check(room, &baseline).await?;
            let mut info = ImageInfo::default();
            info.width = Some(upload.0.width.into());
            info.height = Some(upload.0.height.into());
            info.size = Some(
                upload
                    .0
                    .bytes
                    .len()
                    .try_into()
                    .map_err(|_| "Avatar image is too large.")?,
            );
            info.mimetype = Some("image/png".into());
            info.blurhash = response.blurhash;
            content.url = Some(response.content_uri);
            content.info = Some(Box::new(info));
        }
        let url = content.url.clone();
        let request =
            send_state_event::v3::Request::new(room.room_id().to_owned(), &EmptyStateKey, &content)
                .map_err(|_| "Could not prepare the avatar change.")?;
        let config = matrix_sdk::config::RequestConfig::default()
            .disable_retry()
            .timeout(Duration::from_secs(20));
        room.client().send(request).with_request_config(config).await
            .map_err(|_| "Could not confirm saving the avatar. Reload before retrying; an uploaded image may remain on the server.")?;
        Ok(url)
    };
    tokio::time::timeout(Duration::from_secs(60), operation)
        .await
        .map_err(|_| {
            "Avatar saving timed out and may have succeeded. Reload before retrying.".to_owned()
        })?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(width, height)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }
    #[test]
    fn normalization_preserves_aspect_ratio_and_bounds_upload_dimensions() {
        let upload = prepare(png(1024, 256), || false).unwrap();
        assert_eq!((upload.0.width, upload.0.height), (512, 128));
        let decoded = image::load_from_memory(&upload.0.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (512, 128));
        assert_eq!(
            image::guess_format(&upload.0.bytes).unwrap(),
            image::ImageFormat::Png
        );
    }
    #[test]
    fn invalid_oversized_and_cancelled_images_do_not_produce_uploads() {
        assert!(prepare(b"not an image".to_vec(), || false).is_err());
        assert!(prepare(png(4097, 1), || false).is_err());
        assert!(
            prepare(png(2, 2), || true)
                .unwrap_err()
                .contains("cancelled")
        );
    }
    #[tokio::test]
    async fn only_bounded_regular_files_are_read() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_image(dir.path().to_owned()).await.is_err());
        let path = dir.path().join("large.png");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(read_image(path).await.unwrap_err().contains("10 MiB"));
    }
}
