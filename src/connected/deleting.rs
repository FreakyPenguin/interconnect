//! Confirmed redactions await the server against a runtime-owned room capability.
use super::*;

pub(super) fn redactable(content: &TimelineItemContent) -> bool {
    matches!(content, TimelineItemContent::MsgLike(content)
        if !matches!(content.kind, matrix_sdk_ui::timeline::MsgLikeKind::Redacted))
}

impl Composer {
    /// Recheck current membership, permissions, and content before sending. A
    /// captured transaction makes an uncertain timeout safe to retry explicitly.
    pub async fn delete_message(
        self,
        event: String,
        transaction: matrix_sdk::ruma::OwnedTransactionId,
    ) -> Result<(), String> {
        self.delete_message_with_reason(event, transaction, String::new())
            .await
    }
    pub async fn delete_message_with_reason(
        self,
        event: String,
        transaction: matrix_sdk::ruma::OwnedTransactionId,
        input: String,
    ) -> Result<(), String> {
        let reason = crate::moderation::reason(&input)?;
        let delete = async {
            let event: matrix_sdk::ruma::OwnedEventId = event
                .parse()
                .map_err(|error: matrix_sdk::ruma::IdParseError| error.to_string())?;
            let room = self.0.room();
            let levels = room
                .power_levels()
                .await
                .map_err(|error| error.to_string())?;
            if room.state() != matrix_sdk::RoomState::Joined {
                return Err("You are no longer a member of this room.".into());
            }
            let item =
                self.0.item_by_event_id(&event).await.ok_or_else(|| {
                    "This message is no longer available in the timeline.".to_owned()
                })?;
            if !redactable(item.content()) {
                return Err("This message has already been deleted or cannot be deleted.".into());
            }
            let permitted = if item.is_own() {
                levels.user_can_redact_own_event(room.own_user_id())
            } else {
                levels.user_can_redact_event_of_other(room.own_user_id())
            };
            if !permitted {
                return Err("You no longer have permission to delete this message.".into());
            }
            room.redact(&event, reason.as_deref(), Some(transaction))
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        };
        tokio::time::timeout(Duration::from_secs(30), delete)
            .await
            .map_err(|_| {
                "Deleting timed out and may have succeeded. Check the message before retrying."
                    .to_owned()
            })?
    }
}
