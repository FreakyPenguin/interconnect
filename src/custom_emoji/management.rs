//! Explicit, serialized image-pack edits. Unknown JSON fields survive edits.
use super::{GLOBAL_TYPES, ROOM_TYPES, shortcode_valid};
use crate::connected::RuntimeOwned;
use matrix_sdk::{
    Client, RoomState,
    ruma::{
        OwnedRoomId,
        api::{client::state::get_state_event_for_key, error::ErrorKind},
        serde::Raw,
    },
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

const PERSONAL: &str = "im.ponies.user_emotes";
const LIMIT: usize = 48 * 1024;
const CONFLICT: &str = "This pack changed in another client. Reload before editing again.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Personal,
    Room {
        room: OwnedRoomId,
        key: String,
        kind: String,
    },
}
#[derive(Clone, PartialEq, Eq)]
pub struct Pack {
    pub source: Source,
    pub content: Option<Value>,
    pub name: String,
    pub writable: bool,
    pub global: bool,
}
impl std::fmt::Debug for Pack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EmojiPack")
    }
}
#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub packs: Vec<Pack>,
    pub can_create: bool,
}
#[derive(Clone)]
pub struct Upload(Arc<UploadData>);
struct UploadData {
    bytes: Vec<u8>,
    mime: mime_guess::mime::Mime,
    preview: iced::widget::image::Handle,
    info: Value,
}
impl Upload {
    pub fn preview(&self) -> &iced::widget::image::Handle {
        &self.0.preview
    }
}
impl std::fmt::Debug for Upload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EmojiUpload")
    }
}
pub async fn read_image(path: PathBuf) -> Result<Upload, String> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|_| "Could not open the image.")?;
        if !file
            .metadata()
            .map_err(|_| "Could not inspect the image.")?
            .is_file()
        {
            return Err("Choose an image file.".into());
        }
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read the image.")?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("Emoji images must be 2 MiB or smaller.".into());
        }
        let format =
            image::guess_format(&bytes).map_err(|_| "Choose a PNG, JPEG, WebP or GIF image.")?;
        if !matches!(
            format,
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::WebP
                | image::ImageFormat::Gif
        ) {
            return Err("Choose a PNG, JPEG, WebP or GIF image.".into());
        }
        let (width, height) = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
            .into_dimensions()
            .map_err(|_| "Could not read image dimensions.")?;
        let preview = crate::media::decode_with(
            bytes.clone(),
            crate::media::DecodeLimits {
                encoded: 2 * 1024 * 1024,
                dimension: 4096,
                allocation: 128 * 1024 * 1024,
                target: (96, 96),
                crop: false,
            },
        )
        .map_err(|_| "The image is damaged or too large to decode.")?
        .handle;
        let info =
            json!({"w":width,"h":height,"size":bytes.len(),"mimetype":format.to_mime_type()});
        Ok(Upload(Arc::new(UploadData {
            bytes,
            mime: format.to_mime_type().parse().expect("image MIME"),
            preview,
            info,
        })))
    })
    .await
    .map_err(|_| "Image preparation failed.".to_owned())?
}

#[derive(Clone, Debug)]
pub enum Edit {
    Rename(String),
    Image {
        previous: Option<String>,
        shortcode: String,
        description: String,
        uri: String,
        upload: Option<Upload>,
    },
    RemoveImage(String),
    Delete,
}
/// Pure edit validation is shared with the in-memory demo.
pub fn edited(content: Option<&Value>, edit: &Edit) -> Result<Value, String> {
    let mut next = content
        .cloned()
        .unwrap_or_else(|| json!({"images":{},"pack":{"usage":["emoticon"]}}));
    if !next.is_object() {
        return Err("This pack has invalid metadata.".into());
    }
    if matches!(edit, Edit::Delete) {
        return Ok(json!({}));
    }
    match edit {
        Edit::Rename(name) => {
            if name.trim().is_empty() || name.len() > 128 {
                return Err("Enter a pack name of 1–128 bytes.".into());
            }
            if next.get("pack").is_none() {
                next["pack"] = json!({});
            }
            let pack = next["pack"]
                .as_object_mut()
                .ok_or("This pack has invalid metadata.")?;
            pack.insert("display_name".into(), Value::String(name.trim().into()));
            if next.get("images").is_none() {
                next["images"] = json!({});
            }
        }
        Edit::Image {
            previous,
            shortcode,
            description,
            uri,
            upload,
        } => {
            if !shortcode_valid(shortcode) {
                return Err(
                    "Shortcodes use 1–100 ASCII letters, digits, underscores or hyphens.".into(),
                );
            }
            if description.len() > 1024 {
                return Err("Descriptions must be 1024 bytes or shorter.".into());
            }
            if upload.is_none() && !matrix_sdk::ruma::OwnedMxcUri::from(uri.as_str()).is_valid() {
                return Err("Choose an image or enter a valid mxc:// image URL.".into());
            }
            if next.get("images").is_none() {
                next["images"] = json!({});
            }
            let images = next["images"]
                .as_object_mut()
                .ok_or("This pack has invalid image metadata.")?;
            if previous.as_deref() != Some(shortcode) && images.contains_key(shortcode) {
                return Err("That shortcode already exists in this pack.".into());
            }
            if previous.is_none() && images.len() >= super::MAX_EMOJI {
                return Err("This pack already has 1024 images.".into());
            }
            let mut image = match previous {
                Some(key) => images
                    .remove(key)
                    .ok_or("This emoji is no longer in the pack.")?,
                None => json!({}),
            };
            let object = image
                .as_object_mut()
                .ok_or("This emoji has invalid metadata.")?;
            if upload.is_none() && object.get("url").and_then(Value::as_str) != Some(uri) {
                object.remove("info");
            }
            object.insert("url".into(), uri.clone().into());
            object.insert("body".into(), description.clone().into());
            // Adding an emoticon to a sticker-only pack must still surface it.
            if previous.is_none() {
                object.insert("usage".into(), json!(["emoticon"]));
            }
            if let Some(upload) = upload {
                object.insert("info".into(), upload.0.info.clone());
            }
            images.insert(shortcode.clone(), image);
        }
        Edit::RemoveImage(key) => {
            next["images"]
                .as_object_mut()
                .ok_or("Invalid image metadata.")?
                .remove(key)
                .ok_or("This emoji is no longer in the pack.")?;
        }
        Edit::Delete => unreachable!(),
    }
    if serde_json::to_vec(&next)
        .map_err(|_| "Could not serialize this pack.")?
        .len()
        > LIMIT
    {
        return Err("This pack is too large to save (48 KiB limit).".into());
    }
    Ok(next)
}
#[derive(Clone, Debug)]
pub struct Service(Arc<RuntimeOwned<Client>>, Arc<tokio::sync::Mutex<()>>);
impl Service {
    pub fn new(client: Client) -> Self {
        Self(RuntimeOwned::new(client), Arc::default())
    }
    async fn account(&self, kind: &str) -> Result<Option<Value>, String> {
        self.0
            .account()
            .fetch_account_data(kind.into())
            .await
            .map_err(|_| "Could not load account emoji packs.")?
            .map(|raw| {
                serde_json::from_str(raw.json().get())
                    .map_err(|_| "Invalid account emoji pack.".into())
            })
            .transpose()
    }
    async fn state(
        &self,
        id: &OwnedRoomId,
        key: &str,
        kind: &str,
    ) -> Result<Option<Value>, String> {
        match self
            .0
            .send(get_state_event_for_key::v3::Request::new(
                id.clone(),
                kind.into(),
                key.into(),
            ))
            .await
        {
            Ok(response) => serde_json::from_str(response.event_or_content.get())
                .map(Some)
                .map_err(|_| "Invalid emoji pack state.".into()),
            Err(error) if error.client_api_error_kind() == Some(&ErrorKind::NotFound) => Ok(None),
            Err(_) => Err("Could not load room emoji packs.".into()),
        }
    }
    async fn current(&self, source: &Source) -> Result<Option<Value>, String> {
        match source {
            Source::Personal => self.account(PERSONAL).await,
            Source::Room { room, key, kind } => self.state(room, key, kind).await,
        }
    }
    async fn unchanged(&self, baseline: &Pack) -> Result<bool, String> {
        if let Source::Room { room, key, kind } = &baseline.source
            && kind == ROOM_TYPES[1]
            && self.state(room, key, ROOM_TYPES[0]).await?.is_some()
        {
            return Ok(false);
        }
        Ok(self.current(&baseline.source).await? == baseline.content)
    }
    async fn allowed(&self, source: &Source) -> bool {
        match source {
            Source::Personal => true,
            Source::Room { room, kind, .. } => {
                let Some(room) = self
                    .0
                    .get_room(room)
                    .filter(|r| r.state() == RoomState::Joined)
                else {
                    return false;
                };
                room.power_levels().await.is_ok_and(|levels| {
                    levels.user_can_send_state(room.own_user_id(), kind.as_str().into())
                })
            }
        }
    }
    pub async fn load(self, room: String) -> Result<Listing, String> {
        tokio::time::timeout(Duration::from_secs(30), self.load_inner(room))
            .await
            .map_err(|_| "Loading emoji packs timed out.".to_owned())?
    }
    async fn load_inner(&self, room: String) -> Result<Listing, String> {
        let _serial = self.1.lock().await;
        let personal = self.current(&Source::Personal).await?;
        let mut listing = Listing {
            packs: vec![pack(
                Source::Personal,
                personal,
                "Personal emoji",
                true,
                true,
            )],
            can_create: false,
        };
        let (_, global) = self.global().await?;
        let snapshot = super::snapshot(&self.0, &room)
            .await
            .map_err(|_| "Could not read cached pack sources.")?;
        for id in &snapshot.sources {
            let Some(joined) = self
                .0
                .get_room(id)
                .filter(|r| r.state() == RoomState::Joined)
            else {
                continue;
            };
            if id.as_str() == room {
                listing.can_create = self
                    .allowed(&Source::Room {
                        room: id.clone(),
                        key: String::new(),
                        kind: ROOM_TYPES[0].into(),
                    })
                    .await;
            }
            let cached = super::packs(&joined)
                .await
                .map_err(|_| "Could not read cached packs.")?;
            for key in cached.keys().take(64) {
                let mut source = Source::Room {
                    room: id.clone(),
                    key: key.clone(),
                    kind: ROOM_TYPES[0].into(),
                };
                let mut content = self.current(&source).await?;
                if content.is_none() {
                    source = Source::Room {
                        room: id.clone(),
                        key: key.clone(),
                        kind: ROOM_TYPES[1].into(),
                    };
                    content = self.current(&source).await?;
                }
                if content.as_ref().is_none_or(|v| v.get("images").is_none()) {
                    continue;
                }
                let enabled = global
                    .as_ref()
                    .is_some_and(|g| g["rooms"][id.as_str()].get(key).is_some());
                let writable = self.allowed(&source).await;
                listing.packs.push(pack(
                    source,
                    content,
                    &joined.name().unwrap_or_else(|| "Room emoji".into()),
                    writable,
                    enabled,
                ));
                if listing.packs.len() >= 128 {
                    return Ok(listing);
                }
            }
        }
        Ok(listing)
    }
    async fn global(&self) -> Result<(String, Option<Value>), String> {
        for kind in GLOBAL_TYPES {
            if let Some(content) = self.account(kind).await? {
                return Ok((kind.into(), Some(content)));
            }
        }
        Ok((GLOBAL_TYPES[0].into(), None))
    }
    pub async fn save(self, baseline: Pack, edit: Edit) -> Result<Pack, String> {
        tokio::time::timeout(Duration::from_secs(60), self.save_inner(baseline, edit))
            .await
            .map_err(|_| {
                "Saving timed out. Reload to check whether it succeeded before retrying.".to_owned()
            })?
    }
    async fn save_inner(&self, baseline: Pack, mut edit: Edit) -> Result<Pack, String> {
        let _serial = self.1.lock().await;
        let mut replacement = edited(baseline.content.as_ref(), &edit)?;
        if !self.allowed(&baseline.source).await {
            return Err("You do not have permission to edit this pack.".into());
        }
        if !self.unchanged(&baseline).await? {
            return Err(CONFLICT.into());
        }
        if let Edit::Image {
            uri,
            upload: Some(upload),
            ..
        } = &mut edit
        {
            *uri = self
                .0
                .media()
                .upload(&upload.0.mime, upload.0.bytes.clone(), None)
                .await
                .map_err(|_| "Could not upload the emoji image.")?
                .content_uri
                .to_string();
            replacement = edited(baseline.content.as_ref(), &edit)?;
            if !self.unchanged(&baseline).await? {
                return Err(CONFLICT.into());
            }
        }
        if !self.allowed(&baseline.source).await {
            return Err("You no longer have permission to edit this pack.".into());
        }
        match &baseline.source {
            Source::Personal => {
                self.0
                    .account()
                    .set_account_data_raw(
                        PERSONAL.into(),
                        Raw::new(&replacement)
                            .map_err(|_| "Could not serialize emoji pack.")?
                            .cast_unchecked(),
                    )
                    .await
                    .map_err(|_| "Could not save personal emoji. Reload before retrying.")?;
            }
            Source::Room { room, key, kind } => {
                self.0
                    .get_room(room)
                    .ok_or("Room is no longer available.")?
                    .send_state_event_raw(
                        kind,
                        key,
                        Raw::new(&replacement)
                            .map_err(|_| "Could not serialize emoji pack.")?
                            .cast_unchecked(),
                    )
                    .await
                    .map_err(|_| "Could not save this room pack. Reload before retrying.")?;
            }
        }
        Ok(pack(
            baseline.source,
            Some(replacement),
            &baseline.name,
            true,
            baseline.global,
        ))
    }
    pub async fn enable(self, baseline: Pack, enabled: bool) -> Result<Pack, String> {
        tokio::time::timeout(Duration::from_secs(30), async {
            let _serial = self.1.lock().await;
            let Source::Room { room, key, .. } = &baseline.source else {
                return Err("Personal emoji are already global.".into());
            };
            let (kind, current) = self.global().await?;
            let mut next = current.clone().unwrap_or_else(|| json!({"rooms":{}}));
            if !next.is_object() {
                return Err("Invalid global pack references. No changes were made.".into());
            }
            if next.get("rooms").is_none() {
                next["rooms"] = json!({});
            }
            let rooms = next["rooms"]
                .as_object_mut()
                .ok_or("Invalid global pack references.")?;
            if enabled {
                let keys = rooms
                    .entry(room.to_string())
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .ok_or("Invalid global pack references.")?;
                keys.entry(key.clone()).or_insert_with(|| json!({}));
            } else if let Some(keys) = rooms.get_mut(room.as_str()).and_then(Value::as_object_mut) {
                keys.remove(key);
                if keys.is_empty() {
                    rooms.remove(room.as_str());
                }
            }
            if current.as_ref() == Some(&next) || (current.is_none() && !enabled) {
                return Ok(Pack {
                    global: enabled,
                    ..baseline
                });
            }
            self.0
                .account()
                .set_account_data_raw(
                    kind.into(),
                    Raw::new(&next)
                        .map_err(|_| "Could not prepare global packs.")?
                        .cast_unchecked(),
                )
                .await
                .map_err(|_| "Could not update global packs. Reload before retrying.")?;
            Ok(Pack {
                global: enabled,
                ..baseline
            })
        })
        .await
        .map_err(|_| "Updating global packs timed out. Reload before retrying.".to_owned())?
    }
}
pub fn pack(
    source: Source,
    content: Option<Value>,
    fallback: &str,
    writable: bool,
    global: bool,
) -> Pack {
    let name = content
        .as_ref()
        .and_then(|v| v["pack"]["display_name"].as_str())
        .unwrap_or(fallback)
        .to_owned();
    Pack {
        source,
        content,
        name,
        writable,
        global,
    }
}
pub fn project(listing: &Listing, room: &str, demo_room: Option<OwnedRoomId>) -> super::Snapshot {
    let mut snapshot = super::Snapshot {
        room: room.into(),
        ..Default::default()
    };
    for pack in &listing.packs {
        let identity = match &pack.source {
            Source::Personal => "user".into(),
            Source::Room { room: id, key, .. } => {
                if !pack.global && id.as_str() != room && Some(id) != demo_room.as_ref() {
                    continue;
                }
                serde_json::to_string(&(id.as_str(), key)).expect("pack identity")
            }
        };
        if let Some(content) = &pack.content {
            super::append_pack(&mut snapshot.entries, content, &identity, &pack.name);
        }
    }
    snapshot
}

#[cfg(test)]
mod tests;
