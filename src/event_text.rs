//! Plain-text timeline presentation shared by the desktop and feasibility console.
//! The SDK computes event changes; wording stays here and needs no extra requests.
use matrix_sdk::ruma::{
    UserId,
    events::{RedactContent, StateEventContentChange, StaticStateEventContent},
};
use matrix_sdk_ui::timeline::{
    AnyOtherStateEventContentChange as State, MemberProfileChange, MembershipChange, MsgLikeKind,
    RoomMembershipChange, RoomPinnedEventsChange, TimelineItemContent,
};

/// The sender is already shown by the timeline row. Include affected users when
/// an action targets someone else, particularly invites, kicks, and bans.
pub(crate) fn body(content: &TimelineItemContent, sender: &UserId) -> String {
    match content {
        TimelineItemContent::MsgLike(content) => match &content.kind {
            MsgLikeKind::Message(message) => message.body().to_owned(),
            MsgLikeKind::Sticker(sticker) => described("Sticker", &sticker.content().body),
            MsgLikeKind::Poll(poll) => poll
                .fallback_text()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| described("Poll", &poll.results().question)),
            MsgLikeKind::Redacted => "Message removed".into(),
            MsgLikeKind::UnableToDecrypt(_) => "Unable to decrypt — keys unavailable".into(),
            MsgLikeKind::Other(other) => format!("Unsupported event ({})", other.event_type()),
            // Do not schedule a timer to update this text as a session expires.
            MsgLikeKind::LiveLocation(location) => {
                described("Live location", location.description().unwrap_or_default())
            }
        },
        TimelineItemContent::MembershipChange(change) => membership(change, sender),
        TimelineItemContent::ProfileChange(change) => profile(change, sender),
        TimelineItemContent::OtherState(state) => match state.content() {
            State::RoomName(change) => original(change).map_or_else(removed_state, |content| {
                if content.name.trim().is_empty() {
                    "Removed the room name".into()
                } else {
                    format!("Changed the room name to {}", content.name)
                }
            }),
            State::RoomTopic(change) => original(change).map_or_else(removed_state, |content| {
                if content.topic.trim().is_empty() {
                    "Removed the room topic".into()
                } else {
                    format!("Changed the room topic to {}", content.topic)
                }
            }),
            State::RoomAvatar(change) => state_text(change, "Changed the room avatar"),
            State::RoomCanonicalAlias(change) => state_text(change, "Changed the room address"),
            State::RoomCreate(change) => state_text(change, "Created the room"),
            State::RoomEncryption(change) => state_text(change, "Enabled room encryption"),
            State::RoomGuestAccess(change) => state_text(change, "Changed guest access"),
            State::RoomHistoryVisibility(change) => {
                state_text(change, "Changed who can read room history")
            }
            State::RoomJoinRules(change) => state_text(change, "Changed who can join the room"),
            State::RoomPinnedEvents(change) => {
                let text = match RoomPinnedEventsChange::from(change) {
                    RoomPinnedEventsChange::Added => "Pinned messages",
                    RoomPinnedEventsChange::Removed => "Unpinned messages",
                    RoomPinnedEventsChange::Changed => "Changed pinned messages",
                };
                state_text(change, text)
            }
            State::RoomPowerLevels(change) => state_text(change, "Changed room permissions"),
            State::RoomServerAcl(change) => state_text(change, "Changed server access rules"),
            State::RoomThirdPartyInvite(change) => {
                state_text(change, "Created a third-party invitation")
            }
            State::RoomTombstone(change) => {
                original(change).map_or_else(removed_state, |content| {
                    described(
                        &format!("Replaced this room with {}", content.replacement_room),
                        &content.body,
                    )
                })
            }
            State::SpaceChild(change) => state_text(change, "Changed a space room link"),
            State::SpaceParent(change) => state_text(change, "Changed a parent space link"),
            State::PolicyRuleRoom(change) => state_text(change, "Changed a room moderation rule"),
            State::PolicyRuleServer(change) => {
                state_text(change, "Changed a server moderation rule")
            }
            State::PolicyRuleUser(change) => state_text(change, "Changed a user moderation rule"),
            State::_Custom { event_type } => format!("Room settings changed ({event_type})"),
        },
        TimelineItemContent::FailedToParseMessageLike { event_type, .. } => {
            format!("Could not read event ({event_type})")
        }
        TimelineItemContent::FailedToParseState { event_type, .. } => {
            format!("Could not read room event ({event_type})")
        }
        TimelineItemContent::CallInvite => "Started a call".into(),
        TimelineItemContent::RtcNotification { .. } => "Call notification".into(),
    }
}

fn described(label: &str, description: &str) -> String {
    if description.trim().is_empty() {
        label.into()
    } else {
        format!("{label}: {description}")
    }
}

fn original<C: StaticStateEventContent + RedactContent>(
    change: &StateEventContentChange<C>,
) -> Option<&C> {
    match change {
        StateEventContentChange::Original { content, .. } => Some(content),
        StateEventContentChange::Redacted(_) => None,
    }
}

fn removed_state() -> String {
    "Room event removed".into()
}

fn state_text<C: StaticStateEventContent + RedactContent>(
    change: &StateEventContentChange<C>,
    text: &str,
) -> String {
    original(change).map_or_else(removed_state, |_| text.into())
}

fn membership(change: &RoomMembershipChange, sender: &UserId) -> String {
    let Some(content) = original(change.content()) else {
        return "Membership event removed".into();
    };
    let target = change
        .display_name()
        .filter(|name| !name.trim().is_empty())
        .map(|name| format!("{name} ({})", change.user_id()))
        .unwrap_or_else(|| change.user_id().to_string());
    let text = match change.change() {
        Some(MembershipChange::Joined) => {
            if change.user_id() == sender {
                "Joined the room".into()
            } else {
                format!("{target} joined the room")
            }
        }
        Some(MembershipChange::Left) => "Left the room".into(),
        Some(MembershipChange::Banned) => format!("Banned {target} from the room"),
        Some(MembershipChange::Unbanned) => format!("Unbanned {target}"),
        Some(MembershipChange::Kicked) => format!("Removed {target} from the room"),
        Some(MembershipChange::Invited) => format!("Invited {target} to the room"),
        Some(MembershipChange::KickedAndBanned) => {
            format!("Removed and banned {target} from the room")
        }
        Some(MembershipChange::InvitationAccepted) => {
            "Accepted the invitation and joined the room".into()
        }
        Some(MembershipChange::InvitationRejected) => "Declined the invitation".into(),
        Some(MembershipChange::InvitationRevoked) => format!("Revoked the invitation for {target}"),
        Some(MembershipChange::Knocked) => "Requested to join the room".into(),
        Some(MembershipChange::KnockAccepted) => format!("Accepted {target}'s request to join"),
        Some(MembershipChange::KnockRetracted) => "Withdrew the request to join".into(),
        Some(MembershipChange::KnockDenied) => format!("Denied {target}'s request to join"),
        _ => format!("Updated room membership for {target}"),
    };
    match content
        .reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty())
    {
        Some(reason) => format!("{text} — {reason}"),
        None => text,
    }
}

fn profile(change: &MemberProfileChange, sender: &UserId) -> String {
    let mut changes = Vec::new();
    if let Some(name) = change.displayname_change() {
        changes.push(
            match name.new.as_deref().filter(|name| !name.trim().is_empty()) {
                Some(new) => format!("Changed display name to {new}"),
                None => "Removed display name".into(),
            },
        );
    }
    if let Some(avatar) = change.avatar_url_change() {
        changes.push(if avatar.new.is_some() {
            "Changed avatar".into()
        } else {
            "Removed avatar".into()
        });
    }
    if changes.is_empty() {
        return "Profile change removed".into();
    }
    let text = changes.join("; ");
    if change.user_id() == sender {
        text
    } else {
        format!("Profile update for {}: {text}", change.user_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{
        Client, Room,
        authentication::matrix::MatrixSession,
        deserialized_responses::TimelineEvent,
        ruma::{serde::Raw, user_id},
    };
    use matrix_sdk_ui::timeline::{EncryptedMessage, MsgLikeContent};
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    async fn fixture() -> (MockServer, Room) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(".*/createRoom"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"room_id":"!fixture:example.org"})),
            )
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .build()
            .await
            .unwrap();
        let session: MatrixSession = serde_json::from_value(json!({
            "user_id":"@alice:example.org", "device_id":"TEST", "access_token":"fake"
        }))
        .unwrap();
        client
            .matrix_auth()
            .restore_session(session, Default::default())
            .await
            .unwrap();
        let room = client.create_room(Default::default()).await.unwrap();
        (server, room)
    }

    async fn rendered(room: &Room, mut event: Value) -> String {
        event["event_id"] = json!("$fixture");
        event["origin_server_ts"] = json!(1);
        let sender = event
            .as_object_mut()
            .unwrap()
            .entry("sender")
            .or_insert(json!("@alice:example.org"));
        let sender = matrix_sdk::ruma::UserId::parse(sender.as_str().unwrap()).unwrap();
        let raw = Raw::from_json_string(event.to_string()).unwrap();
        let content = TimelineItemContent::from_event(room, TimelineEvent::from_plaintext(raw))
            .await
            .expect("SDK should produce timeline content for this fixture");
        body(&content, &sender)
    }

    #[tokio::test]
    async fn sdk_html_document_reuses_sanitized_content_and_reply_fallback_rules() {
        let (_server, room) = fixture().await;
        let event = json!({"type":"m.room.message", "event_id":"$formatted", "sender":"@alice:example.org", "origin_server_ts":1,
            "content":{"msgtype":"m.text", "body":"> <@bob:example.org> old reply\n\nactual", "format":"org.matrix.custom.html", "formatted_body":"<mx-reply><blockquote>old reply</blockquote></mx-reply><p><b>actual</b><script>bad</script></p>", "m.relates_to":{"m.in_reply_to":{"event_id":"$old"}}}});
        let content = TimelineItemContent::from_event(
            &room,
            TimelineEvent::from_plaintext(Raw::from_json_string(event.to_string()).unwrap()),
        )
        .await
        .unwrap();
        let TimelineItemContent::MsgLike(content) = content else {
            panic!("message content");
        };
        let MsgLikeKind::Message(message) = content.kind else {
            panic!("message");
        };
        let doc = crate::message_document::Document::from_message(message.msgtype(), None).unwrap();
        // Ruma strips the script element but retains its inert text content.
        assert_eq!(doc.text(&std::collections::BTreeSet::new()), "actualbad");
        assert!(doc.blocks[0].runs[0].style.bold);
    }
    #[tokio::test]
    async fn sdk_sanitization_preserves_element_math_markup_for_native_rendering() {
        let (_server, room) = fixture().await;
        let event = json!({"type":"m.room.message", "event_id":"$math", "sender":"@alice:example.org", "origin_server_ts":1,
            "content":{"msgtype":"m.text", "body":"Equation: $x^2$", "format":"org.matrix.custom.html", "formatted_body":r#"<p>Equation: <span data-mx-maths="x^2"><code>x^2</code></span></p>"#}});
        let content = TimelineItemContent::from_event(
            &room,
            TimelineEvent::from_plaintext(Raw::from_json_string(event.to_string()).unwrap()),
        )
        .await
        .unwrap();
        let TimelineItemContent::MsgLike(content) = content else {
            panic!("message content");
        };
        let MsgLikeKind::Message(message) = content.kind else {
            panic!("message");
        };
        let doc = crate::message_document::Document::from_message(message.msgtype(), None).unwrap();
        assert!(
            doc.blocks
                .iter()
                .flat_map(|block| &block.runs)
                .any(|run| matches!(
                    run.content,
                    crate::message_document::Inline::Image {
                        source: Some(crate::message_document::ImageSource::Formula(_)),
                        ..
                    }
                ))
        );
        assert_eq!(
            doc.text(&std::collections::BTreeSet::new()),
            "Equation: x^2"
        );
    }
    #[tokio::test]
    async fn sdk_event_kind_drives_activity_style_independently_of_message_text() {
        use crate::connected::RowKind;
        let (_server, room) = fixture().await;
        for (event, expected) in [
            (
                json!({"type":"m.room.member", "state_key":"@alice:example.org", "content":{"membership":"join"}}),
                RowKind::Activity,
            ),
            (
                json!({"type":"m.room.topic", "state_key":"", "content":{"topic":"Plans"}}),
                RowKind::Activity,
            ),
            (
                json!({"type":"m.room.message", "content":{"msgtype":"m.text", "body":"Joined the room"}}),
                RowKind::Message,
            ),
            (
                json!({"type":"m.sticker", "content":{"body":"Hello", "url":"mxc://example.org/sticker", "info":{}}}),
                RowKind::Message,
            ),
        ] {
            let mut event = event;
            event["event_id"] = json!("$fixture");
            event["origin_server_ts"] = json!(1);
            event["sender"] = json!("@alice:example.org");
            let content = TimelineItemContent::from_event(
                &room,
                TimelineEvent::from_plaintext(Raw::from_json_string(event.to_string()).unwrap()),
            )
            .await
            .unwrap();
            assert_eq!(RowKind::from_content(&content), expected, "{event}");
        }
        assert_eq!(
            RowKind::from_content(&TimelineItemContent::CallInvite),
            RowKind::Activity
        );
        assert_eq!(
            RowKind::from_content(&TimelineItemContent::MsgLike(MsgLikeContent::redacted())),
            RowKind::Message
        );
    }

    #[tokio::test]
    async fn sdk_membership_changes_distinguish_self_actions_and_moderation_targets() {
        let (_server, room) = fixture().await;
        for (before, after, own_action, expected) in [
            ("leave", "join", true, "Joined the room"),
            ("join", "leave", true, "Left the room"),
            (
                "leave",
                "invite",
                false,
                "Invited Bob (@bob:example.org) to the room",
            ),
            (
                "invite",
                "join",
                true,
                "Accepted the invitation and joined the room",
            ),
            ("invite", "leave", true, "Declined the invitation"),
            (
                "invite",
                "leave",
                false,
                "Revoked the invitation for Bob (@bob:example.org)",
            ),
            (
                "join",
                "leave",
                false,
                "Removed Bob (@bob:example.org) from the room",
            ),
            (
                "leave",
                "ban",
                false,
                "Banned Bob (@bob:example.org) from the room",
            ),
            (
                "join",
                "ban",
                false,
                "Removed and banned Bob (@bob:example.org) from the room",
            ),
            ("ban", "leave", false, "Unbanned Bob (@bob:example.org)"),
            ("leave", "knock", true, "Requested to join the room"),
            (
                "knock",
                "invite",
                false,
                "Accepted Bob (@bob:example.org)'s request to join",
            ),
            ("knock", "leave", true, "Withdrew the request to join"),
            (
                "knock",
                "leave",
                false,
                "Denied Bob (@bob:example.org)'s request to join",
            ),
        ] {
            let event = json!({
                "type":"m.room.member", "state_key":"@bob:example.org",
                "sender":if own_action { "@bob:example.org" } else { "@alice:example.org" },
                "content":{"membership":after, "displayname":"Bob"},
                "unsigned":{"prev_content":{"membership":before, "displayname":"Bob"}}
            });
            assert_eq!(
                rendered(&room, event).await,
                expected,
                "{before} → {after}, self={own_action}"
            );
        }
        assert_eq!(
            rendered(
                &room,
                json!({
                    "type":"m.room.member", "state_key":"@bob:example.org",
                    "content":{"membership":"leave", "reason":"Room rules"},
                    "unsigned":{"prev_content":{"membership":"join", "displayname":"Bob"}}
                })
            )
            .await,
            "Removed Bob (@bob:example.org) from the room — Room rules"
        );
    }

    #[tokio::test]
    async fn sdk_state_profile_and_message_content_provides_readable_text() {
        let (_server, room) = fixture().await;
        let cases = [
            (
                json!({"type":"m.room.name", "state_key":"", "content":{"name":"Studio"}}),
                "Changed the room name to Studio",
            ),
            (
                json!({"type":"m.room.name", "state_key":"", "content":{"name":""}}),
                "Removed the room name",
            ),
            (
                json!({"type":"m.room.topic", "state_key":"", "content":{"topic":"Plans"}}),
                "Changed the room topic to Plans",
            ),
            (
                json!({"type":"m.room.topic", "state_key":"", "content":{"topic":""}}),
                "Removed the room topic",
            ),
            (
                json!({"type":"m.room.encryption", "state_key":"", "content":{"algorithm":"m.megolm.v1.aes-sha2"}}),
                "Enabled room encryption",
            ),
            (
                json!({"type":"m.room.pinned_events", "state_key":"", "content":{"pinned":["$pinned"]}}),
                "Pinned messages",
            ),
            (
                json!({"type":"m.room.member", "state_key":"@alice:example.org", "content":{"membership":"join", "displayname":"Alice", "avatar_url":"mxc://example.org/new"}, "unsigned":{"prev_content":{"membership":"join", "displayname":"Old name"}}}),
                "Changed display name to Alice; Changed avatar",
            ),
            (
                json!({"type":"m.room.member", "state_key":"@alice:example.org", "content":{"membership":"join"}, "unsigned":{"prev_content":{"membership":"join", "displayname":"Alice", "avatar_url":"mxc://example.org/old"}}}),
                "Removed display name; Removed avatar",
            ),
            (
                json!({"type":"m.sticker", "content":{"body":"Waving hello", "url":"mxc://example.org/sticker", "info":{}}}),
                "Sticker: Waving hello",
            ),
            (
                json!({"type":"org.matrix.msc3381.poll.start", "content":{"org.matrix.msc3381.poll.start":{"question":{"org.matrix.msc1767.text":"Lunch?"}, "answers":[{"id":"a", "org.matrix.msc1767.text":"Soup"}]}}}),
                "Poll: Lunch?",
            ),
            (
                json!({"type":"org.matrix.msc3381.poll.start", "content":{"org.matrix.msc1767.text":"Lunch?\n1. Soup", "org.matrix.msc3381.poll.start":{"question":{"org.matrix.msc1767.text":"Lunch?"}, "answers":[{"id":"a", "org.matrix.msc1767.text":"Soup"}]}}}),
                "Lunch?\n1. Soup",
            ),
            (
                json!({"type":"org.matrix.msc3672.beacon_info", "state_key":"@alice:example.org", "content":{"description":"Afternoon walk", "live":true, "org.matrix.msc3488.ts":1, "timeout":60000}}),
                "Live location: Afternoon walk",
            ),
            (
                json!({"type":"m.room.message", "content":{"msgtype":"m.text", "body":"Hello\nworld"}}),
                "Hello\nworld",
            ),
            (
                json!({"type":"m.room.message", "content":{"msgtype":"m.file", "body":"notes.txt", "url":"mxc://example.org/file"}}),
                "notes.txt",
            ),
            (
                json!({"type":"com.example.custom", "content":{}}),
                "Unsupported event (com.example.custom)",
            ),
            (
                json!({"type":"com.example.setting", "state_key":"", "content":{}}),
                "Room settings changed (com.example.setting)",
            ),
        ];
        for (event, expected) in cases {
            assert_eq!(rendered(&room, event.clone()).await, expected, "{event}");
        }
    }

    #[tokio::test]
    async fn sdk_redacted_events_do_not_claim_a_new_state_change() {
        let (_server, room) = fixture().await;
        for (event_type, state_key, content, expected) in [
            ("m.room.message", None, json!({}), "Message removed"),
            ("m.room.name", Some(""), json!({}), "Room event removed"),
            (
                "m.room.member",
                Some("@bob:example.org"),
                json!({"membership":"join"}),
                "Membership event removed",
            ),
        ] {
            let mut event = json!({"type":event_type, "content":content, "unsigned":{
                "redacted_because":{"type":"m.room.redaction", "event_id":"$redaction", "sender":"@alice:example.org", "origin_server_ts":2, "content":{}}
            }});
            if let Some(key) = state_key {
                event["state_key"] = json!(key);
            }
            assert_eq!(rendered(&room, event).await, expected);
        }
    }

    #[test]
    fn unavailable_and_malformed_content_has_specific_fallbacks() {
        let sender = user_id!("@alice:example.org");
        assert_eq!(
            body(
                &TimelineItemContent::MsgLike(MsgLikeContent::unable_to_decrypt(
                    EncryptedMessage::Unknown,
                )),
                sender
            ),
            "Unable to decrypt — keys unavailable"
        );
        let error = std::sync::Arc::new(serde_json::from_str::<Value>("invalid").unwrap_err());
        assert_eq!(
            body(
                &TimelineItemContent::FailedToParseMessageLike {
                    event_type: "m.room.message".into(),
                    error: error.clone(),
                },
                sender
            ),
            "Could not read event (m.room.message)"
        );
        assert_eq!(
            body(
                &TimelineItemContent::FailedToParseState {
                    event_type: "m.room.name".into(),
                    state_key: String::new(),
                    error,
                },
                sender
            ),
            "Could not read room event (m.room.name)"
        );
        assert_eq!(
            body(&TimelineItemContent::CallInvite, sender),
            "Started a call"
        );
        assert_eq!(
            body(
                &TimelineItemContent::RtcNotification {
                    call_intent: None,
                    declined_by: Vec::new(),
                    active_call_info: None,
                },
                sender
            ),
            "Call notification"
        );
    }
}
