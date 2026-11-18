//! Per-occurrence identities for editable, visible mention labels.
use matrix_sdk::ruma::{OwnedRoomOrAliasId, OwnedServerName, OwnedUserId};
use std::ops::Range;

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum MentionTarget {
    /// Explicit room-wide notification, serialized distinctly from room links.
    RoomWide {
        #[serde(with = "room_wide_marker")]
        room_wide: (),
    },
    User {
        user_id: OwnedUserId,
    },
    Room {
        room_id: OwnedRoomOrAliasId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        via: Vec<OwnedServerName>,
    },
}
// Keep the persisted marker explicit and reject false/missing markers. Using
// a custom codec also preserves the existing flattened user/room draft schema.
mod room_wide_marker {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(_: &(), serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(true)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
        if bool::deserialize(deserializer)? {
            Ok(())
        } else {
            Err(D::Error::custom("room-wide mention marker must be true"))
        }
    }
}
impl MentionTarget {
    pub fn user_id(&self) -> Option<&OwnedUserId> {
        match self {
            Self::User { user_id } => Some(user_id),
            Self::Room { .. } | Self::RoomWide { .. } => None,
        }
    }
    pub fn room_id(&self) -> Option<&OwnedRoomOrAliasId> {
        match self {
            Self::Room { room_id, .. } => Some(room_id),
            Self::User { .. } | Self::RoomWide { .. } => None,
        }
    }
    pub fn is_room_wide(&self) -> bool {
        matches!(self, Self::RoomWide { .. })
    }
    pub fn uri(&self) -> Option<String> {
        Some(match self {
            Self::RoomWide { .. } => return None,
            Self::User { user_id } => user_id.matrix_to_uri().to_string(),
            Self::Room { room_id, via } => {
                match <&matrix_sdk::ruma::RoomId>::try_from(&**room_id) {
                    Ok(id) => id.matrix_to_uri_via(via.clone()).to_string(),
                    Err(alias) => alias.matrix_to_uri().to_string(),
                }
            }
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NamedMention {
    pub range: Range<usize>,
    #[serde(flatten)]
    pub target: MentionTarget,
}
impl NamedMention {
    pub fn valid(&self, source: &str) -> bool {
        self.range.start < self.range.end
            && source.get(self.range.clone()).is_some_and(|label| {
                !label.trim().is_empty()
                    && (!self.target.is_room_wide()
                        || crate::message_document::room_wide_token(source, self.range.clone()))
            })
    }
}
/// Changes are exact editor replacements in descending source order. Identity
/// follows surviving label characters; replacing/deleting an entire name drops it.
pub fn update_named_mentions(
    mentions: &mut Vec<NamedMention>,
    source: &str,
    changes: &[(Range<usize>, usize)],
) {
    for (edit, inserted) in changes {
        let shift = |offset: usize| offset.saturating_sub(edit.end - edit.start) + inserted;
        mentions.retain_mut(|mention| {
            let range = &mut mention.range;
            if edit.end <= range.start {
                range.start = shift(range.start);
                range.end = shift(range.end);
            } else if edit.start >= range.end {
                // Appending body text at a mention boundary does not extend it.
            } else if edit.start <= range.start && edit.end >= range.end {
                return false;
            } else {
                if edit.end < range.end {
                    range.end = shift(range.end);
                } else {
                    range.end = edit.start + if edit.end == range.end { *inserted } else { 0 };
                }
                if edit.start < range.start {
                    range.start = edit.start + inserted;
                }
            }
            range.start < range.end
        });
    }
    mentions.retain_mut(|mention| {
        let Some(label) = source.get(mention.range.clone()) else {
            return false;
        };
        let trimmed = label.trim();
        mention.range.start += label.len() - label.trim_start().len();
        mention.range.end = mention.range.start + trimmed.len();
        !trimmed.is_empty()
    });
    mentions.retain(|mention| mention.valid(source));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mention(range: Range<usize>, user: &str) -> NamedMention {
        NamedMention {
            range,
            target: MentionTarget::User {
                user_id: user.parse().unwrap(),
            },
        }
    }
    #[test]
    fn room_wide_binding_roundtrips_and_any_token_change_removes_it() {
        let mention = NamedMention {
            range: 3..8,
            target: MentionTarget::RoomWide { room_wide: () },
        };
        assert!(mention.valid("Hi @room!"));
        for invalid in [
            r#"{"range":{"start":3,"end":8}}"#,
            r#"{"range":{"start":3,"end":8},"room_wide":false}"#,
        ] {
            assert!(serde_json::from_str::<NamedMention>(invalid).is_err());
        }

        assert!(mention.target.uri().is_none());
        assert_eq!(
            serde_json::from_str::<NamedMention>(&serde_json::to_string(&mention).unwrap())
                .unwrap(),
            mention
        );
        for (body, change) in [
            ("Hi @roo!", (7..8, 0)),
            ("Hi !", (3..8, 0)),
            ("Hi @roommate!", (8..8, 4)),
        ] {
            let mut mentions = vec![mention.clone()];
            update_named_mentions(&mut mentions, body, &[change]);
            assert!(mentions.is_empty(), "{body}");
        }
    }
    #[test]
    fn persisted_targets_accept_legacy_users_and_roundtrip_room_routing() {
        let legacy = r#"{"range":{"start":0,"end":4},"user_id":"@maya:example.org"}"#;
        let user: NamedMention = serde_json::from_str(legacy).unwrap();
        assert_eq!(user.target.user_id().unwrap().as_str(), "@maya:example.org");
        let room = NamedMention {
            range: 0..7,
            target: MentionTarget::Room {
                room_id: "!opaque:example.org".parse().unwrap(),
                via: vec!["route.example.org".parse().unwrap()],
            },
        };
        assert_eq!(
            serde_json::from_str::<NamedMention>(&serde_json::to_string(&room).unwrap()).unwrap(),
            room
        );
        assert!(room.target.uri().unwrap().contains("via=route.example.org"));
        assert!(serde_json::from_str::<NamedMention>(r#"{"range":{"start":0,"end":4},"user_id":"@maya:example.org","room_id":"!room:example.org"}"#).is_err());
        let mut rooms = vec![room];
        update_named_mentions(&mut rooms, "#Room", &[(5..7, 0)]);
        assert_eq!(rooms[0].range, 0..5);
        update_named_mentions(&mut rooms, "", &[(0..5, 0)]);
        assert!(rooms.is_empty());
    }
    #[test]
    fn shortening_names_preserves_identity_but_whole_replacements_remove_it() {
        let mut names = vec![mention(3..12, "@maya:example.org")];
        update_named_mentions(&mut names, "Hi Maya!", &[(7..12, 0)]);
        assert_eq!(names[0].range, 3..7);
        update_named_mentions(&mut names, "Hi Alex!", &[(3..7, 4)]);
        assert!(names.is_empty());
    }
    #[test]
    fn exact_ranges_keep_duplicate_names_and_utf8_identities_separate() {
        let mut names = vec![
            mention(0..4, "@one:example.org"),
            mention(5..9, "@two:example.org"),
        ];
        update_named_mentions(&mut names, "Maya", &[(0..5, 0)]);
        assert_eq!(names, vec![mention(0..4, "@two:example.org")]);
        update_named_mentions(&mut names, "👋 Maya", &[(0..0, 5)]);
        assert_eq!(names[0].range, 5..9);
        update_named_mentions(&mut names, "👋 Maya and more", &[(9..9, 9)]);
        assert_eq!(names[0].range, 5..9);
    }
}
