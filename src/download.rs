//! Original media metadata shared by room and thread attachment actions.
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineItemContent};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub source: crate::image_media::Key,
    pub filename: String,
}
impl Attachment {
    pub async fn fetch(&self, client: &matrix_sdk::Client) -> Result<Vec<u8>, String> {
        tokio::time::timeout(
            std::time::Duration::from_secs(120),
            client
                .media()
                .get_media_content(&self.source.file_request(), true),
        )
        .await
        .map_err(|_| "Attachment download timed out.".to_owned())?
        .map_err(|_| {
            "Could not download this attachment. Check your connection and encryption keys."
                .to_owned()
        })
    }

    pub fn from_content(content: &TimelineItemContent) -> Option<Self> {
        let TimelineItemContent::MsgLike(content) = content else {
            return None;
        };
        let MsgLikeKind::Message(message) = &content.kind else {
            return None;
        };
        Self::from_message(message.msgtype())
    }
    pub fn from_message(message: &MessageType) -> Option<Self> {
        let (source, filename, mimetype) = match message {
            MessageType::Image(media) => (
                &media.source,
                media.filename(),
                media
                    .info
                    .as_ref()
                    .and_then(|info| info.mimetype.as_deref()),
            ),
            MessageType::File(media) => (
                &media.source,
                media.filename(),
                media
                    .info
                    .as_ref()
                    .and_then(|info| info.mimetype.as_deref()),
            ),
            MessageType::Video(media) => (
                &media.source,
                media.filename(),
                media
                    .info
                    .as_ref()
                    .and_then(|info| info.mimetype.as_deref()),
            ),
            MessageType::Audio(media) => (
                &media.source,
                media.filename(),
                media
                    .info
                    .as_ref()
                    .and_then(|info| info.mimetype.as_deref()),
            ),
            _ => return None,
        };
        let mut filename = safe_filename(filename);
        if std::path::Path::new(&filename).extension().is_none()
            && let Some(extension) = mimetype
                .and_then(mime_guess::get_mime_extensions_str)
                .and_then(|extensions| extensions.first())
        {
            filename.push('.');
            filename.push_str(extension);
        }
        let source = crate::image_media::Key::file(source);
        source
            .file_request()
            .uri()
            .is_valid()
            .then_some(Self { source, filename })
    }
}

/// Names are remote input. Strip both platform separators and control characters
/// before suggesting a save name or placing a file in a private temporary directory.
pub fn safe_filename(name: &str) -> String {
    let name: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let mut name = name;
    while name.len() > 200 {
        name.pop();
    }
    if name.trim().is_empty() || name == "." || name == ".." {
        "attachment".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_names_stay_inside_the_destination_directory() {
        for name in ["../../report.pdf", "C:\\private\\report.pdf", "report.pdf"] {
            assert_eq!(safe_filename(name), "report.pdf");
        }
        for name in ["", ".", "..", "\n\0"] {
            assert_eq!(safe_filename(name), "attachment");
        }
        assert_eq!(safe_filename("photo\n.png"), "photo.png");
    }
    #[test]
    fn all_media_types_use_original_sources_and_filenames_not_captions() {
        for kind in ["m.image", "m.file", "m.audio", "m.video"] {
            let message: MessageType = serde_json::from_value(serde_json::json!({
                "msgtype": kind, "body": "A caption", "filename": "../original.bin",
                "url": "mxc://example.org/original",
                "info": {"thumbnail_url": "mxc://example.org/thumbnail"}
            }))
            .unwrap();
            let attachment = Attachment::from_message(&message).unwrap();
            assert_eq!(attachment.filename, "original.bin");
            let request = attachment.source.file_request();
            assert_eq!(request.uri().as_str(), "mxc://example.org/original");
            assert!(matches!(
                request.format,
                matrix_sdk::media::MediaFormat::File
            ));
        }
        assert!(
            Attachment::from_message(&MessageType::Text(
                matrix_sdk::ruma::events::room::message::TextMessageEventContent::plain("hello")
            ))
            .is_none()
        );
    }
    #[test]
    fn extensionless_media_names_use_the_declared_type_for_desktop_opening() {
        let message = serde_json::from_value(serde_json::json!({
            "msgtype": "m.image", "body": "image", "url": "mxc://example.org/image",
            "info": {"mimetype": "image/png"}
        }))
        .unwrap();
        assert_eq!(
            Attachment::from_message(&message).unwrap().filename,
            "image.png"
        );
        assert!(safe_filename(&"é".repeat(300)).len() <= 200);
    }
}
