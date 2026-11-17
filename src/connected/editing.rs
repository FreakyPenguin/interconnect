//! Message replacement admission and bounded authoring metadata projection.
use super::*;
use crate::composition::{Format, Prepared};

#[derive(Clone, Debug)]
pub struct EditSource {
    pub body: String,
    pub format: Format,
    pub emoji: Vec<crate::custom_emoji::Selection>,
}
impl EditSource {
    pub fn plain(body: String) -> Self {
        Self {
            body,
            format: Format::Plain,
            emoji: Vec::new(),
        }
    }
    pub fn from_prepared(prepared: &Prepared) -> Self {
        Self::new(prepared.source.clone(), prepared.html.as_deref())
    }
    pub(super) fn from_message(
        message: &matrix_sdk::ruma::events::room::message::MessageType,
    ) -> Self {
        Self::new(
            message.body().to_owned(),
            crate::message_document::Document::html(message),
        )
    }
    fn new(body: String, html: Option<&str>) -> Self {
        let mut source = Self::plain(body);
        if let Some(html) = html {
            source.format = Format::Markdown;
            // Projection already runs on the SDK worker. Parse only bounded HTML;
            // never discover or fetch media while constructing an edit draft.
            if html.len() <= 64 * 1024 {
                let html = matrix_sdk::ruma::html::Html::parse(html);
                let mut nodes: Vec<_> = html.children().collect();
                nodes.reverse();
                let mut visited = 0;
                while let Some(node) = nodes.pop() {
                    visited += 1;
                    if visited > 4096 || source.emoji.len() >= 256 {
                        break;
                    }
                    nodes.extend(node.children().collect::<Vec<_>>().into_iter().rev());
                    let Some(element) = node.as_element() else {
                        continue;
                    };
                    if element.name.local.as_ref() != "img" {
                        continue;
                    }
                    let attrs = element.attrs.borrow();
                    let attr = |name: &str| {
                        attrs
                            .iter()
                            .find(|attr| attr.name.local.as_ref() == name)
                            .map(|attr| attr.value.to_string())
                    };
                    // SDK sanitization strips data-mx-emoticon; the title and
                    // matching source shortcode still bind the authored image.
                    let (Some(uri), Some(shortcode)) = (attr("src"), attr("title")) else {
                        continue;
                    };
                    let uri = matrix_sdk::ruma::OwnedMxcUri::from(uri);
                    if !uri.is_valid() || !crate::custom_emoji::shortcode_valid(&shortcode) {
                        continue;
                    }
                    let prefix = format!(":{shortcode}");
                    for (offset, _) in source.body.match_indices(&prefix) {
                        let suffix = &source.body[offset + prefix.len()..];
                        let length = if suffix.starts_with(':') {
                            1
                        } else if let Some(suffix) = suffix.strip_prefix('/') {
                            let Some(end) = suffix.find(':') else {
                                continue;
                            };
                            if suffix[..end].is_empty()
                                || !suffix[..end].bytes().all(|ch| ch.is_ascii_digit())
                            {
                                continue;
                            }
                            end + 2
                        } else {
                            continue;
                        };
                        let token = source.body[offset..offset + prefix.len() + length].to_owned();
                        if let Some(choice) =
                            source.emoji.iter().find(|choice| choice.token == token)
                        {
                            if choice.emoji.uri == uri {
                                break;
                            }
                            continue;
                        }
                        source.emoji.push(crate::custom_emoji::Selection {
                            token,
                            emoji: crate::custom_emoji::Emoji {
                                id: uri.to_string(),
                                shortcode: shortcode.clone(),
                                name: attr("alt").unwrap_or_else(|| shortcode.clone()),
                                pack: String::new(),
                                uri: uri.clone(),
                            },
                        });
                        break;
                    }
                }
            }
        }
        source
    }
}

impl Composer {
    /// Queue a replacement against the captured timeline. Recheck ownership and
    /// content after asynchronous formatting, before admitting the write.
    pub async fn edit_message(
        self,
        event: String,
        body: String,
        format: crate::composition::Format,
        emoji: Vec<crate::custom_emoji::Selection>,
    ) -> Result<(), String> {
        use matrix_sdk::ruma::events::room::message::{
            MessageType, RoomMessageEventContentWithoutRelation,
        };
        let prepared = crate::composition::prepare_with_emoji(body, format, emoji).await?;
        let update = async {
            let event: matrix_sdk::ruma::OwnedEventId = event
                .parse()
                .map_err(|error: matrix_sdk::ruma::IdParseError| error.to_string())?;
            let item =
                self.0.item_by_event_id(&event).await.ok_or_else(|| {
                    "This message is no longer available in the timeline.".to_owned()
                })?;
            if !item.is_editable() || self.0.room().state() != matrix_sdk::RoomState::Joined {
                return Err("This message can no longer be edited.".into());
            }
            let TimelineItemContent::MsgLike(content) = item.content() else {
                return Err("This message cannot be edited.".into());
            };
            let matrix_sdk_ui::timeline::MsgLikeKind::Message(message) = &content.kind else {
                return Err("This message cannot be edited.".into());
            };
            let mut msgtype = message.msgtype().clone();
            let text = prepared.text_content();
            match &mut msgtype {
                MessageType::Text(content) => {
                    content.body = text.body;
                    content.formatted = text.formatted;
                }
                MessageType::Emote(content) => {
                    content.body = text.body;
                    content.formatted = text.formatted;
                }
                _ => return Err("Only text messages and emotes can be edited.".into()),
            }
            let mut replacement = RoomMessageEventContentWithoutRelation::new(msgtype);
            replacement.mentions = message.mentions().cloned();
            Box::pin(self.0.edit(
                &item.identifier(),
                matrix_sdk::room::edit::EditedContent::RoomMessage(replacement),
            ))
            .await
            .map_err(|error| error.to_string())
        };
        tokio::time::timeout(Duration::from_secs(30), update)
            .await
            .map_err(|_| "Editing timed out; check the message before retrying.".to_owned())?
    }
}
