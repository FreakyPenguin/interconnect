//! Account navigation is a projection of SDK metadata, independent of open timelines.
use matrix_sdk::{
    Client,
    ruma::events::{SyncStateEvent, space::child::SpaceChildEventContent},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    #[default]
    All,
    Direct,
    Space(String),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoomEntry {
    pub id: String,
    pub name: String,
    pub topic: String,
    /// Joined members, excluding invitations and departed users.
    pub member_count: u64,
    pub can_edit_topic: bool,
    pub can_pin: bool,
    pub can_redact_own: bool,
    pub can_redact_other: bool,
    pub can_invite: bool,
    pub pinned_events: Vec<String>,
    /// Room m.tag keys, including Element custom section membership.
    pub tags: Vec<String>,
    pub favorite: bool,
    pub direct: bool,
    /// Single Matrix peer from m.direct; ambiguous targets have none.
    pub direct_user: Option<String>,
    pub unread: bool,
    /// SDK client-side unread message count; not a claim about thread unread state.
    pub unread_count: u64,
}
impl RoomEntry {
    /// Explicit tags win over inferred direct grouping. Unknown tags fall back.
    /// Unread is a separate shortcut section, so read state never changes membership.
    pub fn section_in<'a>(&'a self, sections: &'a [SectionEntry]) -> &'a str {
        if self.favorite {
            return "m.favourite";
        }
        sections
            .iter()
            .find(|section| self.tags.contains(&section.id))
            .map_or_else(|| self.section(), |section| section.id.as_str())
    }
    pub fn section(&self) -> &'static str {
        if self.favorite {
            "m.favourite"
        } else if self.direct {
            "Direct messages"
        } else {
            "Rooms"
        }
    }
}
/// Element's account-level section definitions. IDs, rather than names, identify headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionEntry {
    pub id: String,
    pub name: String,
    /// Creation scope controls where an empty custom section is shown.
    pub space_id: Option<String>,
}
impl SectionEntry {
    pub fn show_empty(&self, scope: &Scope) -> bool {
        self.id.starts_with(CUSTOM_PREFIX)
            && match scope {
                Scope::Space(id) => self.space_id.as_ref() == Some(id),
                Scope::All | Scope::Direct => {
                    self.space_id.as_deref().is_none_or(|id| id == "home-space")
                }
            }
    }
}

pub const ELEMENT_SETTINGS: &str = "im.vector.web.settings";
const CUSTOM_PREFIX: &str = "element.io.section.";

/// Read only the interoperable schema; never repair or write another client's settings.
/// Missing/invalid definitions leave tagged rooms in their usual fallback groups.
pub fn sections_from_settings(settings: &serde_json::Value) -> Vec<SectionEntry> {
    let mut custom = std::collections::BTreeMap::new();
    if let Some(data) = settings["RoomList.CustomSectionData"].as_object() {
        for (tag, entry) in data {
            if tag.starts_with(CUSTOM_PREFIX)
                && entry["tag"].as_str() == Some(tag)
                && let Some(name) = entry["name"].as_str()
            {
                custom.insert(tag.clone(), name.to_owned());
            }
        }
    }
    let mut order = Vec::new();
    if let Some(stored) = settings["RoomList.OrderedCustomSections"].as_array() {
        for tag in stored.iter().filter_map(|value| value.as_str()) {
            if (tag == "chats" || tag == "im.vector.fake.direct" || custom.contains_key(tag))
                && !order.iter().any(|id| id == tag)
            {
                order.push(tag.to_owned());
            }
        }
    }
    if !order.iter().any(|id| id == "im.vector.fake.direct") {
        order.insert(0, "im.vector.fake.direct".into());
    }
    // Keep valid definitions visible even when settings arrive out of order.
    for tag in custom.keys() {
        if !order.contains(tag) {
            let index = order
                .iter()
                .position(|id| id == "chats")
                .unwrap_or(order.len());
            order.insert(index, tag.clone());
        }
    }
    if !order.iter().any(|id| id == "chats") {
        order.push("chats".into());
    }
    let section = |id: &str, name: &str| SectionEntry {
        id: id.into(),
        name: name.into(),
        space_id: settings["RoomList.CustomSectionData"][id]["spaceId"]
            .as_str()
            .map(str::to_owned),
    };
    let mut result = vec![
        section("Unread", "Unread"),
        section("m.favourite", "Favorites"),
    ];
    for tag in order {
        match tag.as_str() {
            "im.vector.fake.direct" => result.push(section("Direct messages", "Direct messages")),
            "chats" => result.push(section("Rooms", "Rooms")),
            _ => result.push(section(&tag, &custom[&tag])),
        }
    }
    result.push(section("m.lowpriority", "Low priority"));
    result
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpaceEntry {
    pub id: String,
    pub name: String,
    pub avatar_url: Option<matrix_sdk::ruma::OwnedMxcUri>,
    /// Active m.space.child links. Unknown/unjoined children are not displayed.
    pub children: Vec<String>,
}

/// Descendant traversal tolerates cycles and shared children without duplicating rooms.
pub fn members(scope: &Scope, spaces: &[SpaceEntry]) -> Option<HashSet<String>> {
    let Scope::Space(root) = scope else {
        return None;
    };
    let mut visited = HashSet::new();
    let mut pending = vec![root.clone()];
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        if let Some(space) = spaces.iter().find(|space| space.id == id) {
            pending.extend(space.children.iter().cloned());
        }
    }
    Some(visited)
}
pub fn includes(scope: &Scope, members: &Option<HashSet<String>>, room: &RoomEntry) -> bool {
    match scope {
        Scope::All => true,
        Scope::Direct => room.direct,
        Scope::Space(_) => members.as_ref().is_some_and(|ids| ids.contains(&room.id)),
    }
}

pub async fn snapshot(
    client: &Client,
) -> anyhow::Result<(Vec<RoomEntry>, Vec<SpaceEntry>, Vec<SectionEntry>)> {
    let settings = client
        .account()
        .account_data_raw(ELEMENT_SETTINGS.into())
        .await?
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw.json().get()).ok())
        .unwrap_or_default();
    let sections = sections_from_settings(&settings);
    let mut rooms = Vec::new();
    let mut spaces = Vec::new();
    for room in client.joined_rooms() {
        if crate::bookmarks::is_bookmarks_room(&room) {
            continue;
        }
        let id = room.room_id().to_string();
        let name = room
            .name()
            .or_else(|| room.cached_display_name().map(|n| n.to_string()))
            .unwrap_or_else(|| id.clone());
        if room.is_space() {
            let mut children = Vec::new();
            for raw in room
                .get_state_events_static::<SpaceChildEventContent>()
                .await?
            {
                if let Ok(matrix_sdk::deserialized_responses::SyncOrStrippedState::Sync(
                    SyncStateEvent::Original(event),
                )) = raw.deserialize()
                    && !event.content.via.is_empty()
                {
                    children.push(event.state_key.to_string());
                }
            }
            children.sort();
            children.dedup();
            spaces.push(SpaceEntry {
                id,
                name,
                avatar_url: room.avatar_url(),
                children,
            });
        } else {
            let unread_count = room.num_unread_messages();
            let levels = room.power_levels().await.ok();
            let direct = room.is_direct().await?;
            let direct_user = direct_peer(room.own_user_id(), room.direct_targets());
            rooms.push(RoomEntry {
                id,
                name,
                topic: room.topic().unwrap_or_default(),
                member_count: room.joined_members_count(),
                // Incomplete cached permissions must not hide the conversation.
                can_edit_topic: levels.as_ref().is_some_and(|levels| {
                    levels.user_can_send_state(
                        room.own_user_id(),
                        matrix_sdk::ruma::events::StateEventType::RoomTopic,
                    )
                }),
                can_invite: levels
                    .as_ref()
                    .is_some_and(|levels| levels.user_can_invite(room.own_user_id())),
                can_pin: levels.as_ref().is_some_and(|levels| {
                    levels.user_can_send_state(
                        room.own_user_id(),
                        matrix_sdk::ruma::events::StateEventType::RoomPinnedEvents,
                    )
                }),
                can_redact_own: levels
                    .as_ref()
                    .is_some_and(|levels| levels.user_can_redact_own_event(room.own_user_id())),
                can_redact_other: levels.as_ref().is_some_and(|levels| {
                    levels.user_can_redact_event_of_other(room.own_user_id())
                }),
                pinned_events: room
                    .pinned_event_ids()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect(),
                tags: room
                    .tags()
                    .await?
                    .unwrap_or_default()
                    .keys()
                    .map(ToString::to_string)
                    .collect(),
                favorite: room.is_favourite(),
                direct,
                direct_user: direct.then_some(direct_user).flatten(),
                unread: room.is_marked_unread()
                    || unread_count > 0
                    || room.num_unread_notifications() > 0,
                unread_count,
            });
        }
    }
    rooms.sort_by_cached_key(|r| (r.name.to_lowercase(), r.id.clone()));
    spaces.sort_by_cached_key(|s| (s.name.to_lowercase(), s.id.clone()));
    Ok((rooms, spaces, sections))
}

/// Exclude self and third-party identifiers; never arbitrarily pick a group peer.
fn direct_peer(
    own: &matrix_sdk::ruma::UserId,
    targets: impl IntoIterator<Item = matrix_sdk::ruma::OwnedDirectUserIdentifier>,
) -> Option<String> {
    let peers: Vec<_> = targets
        .into_iter()
        .filter(|id| id.as_user_id() != Some(own))
        .collect();
    if peers.len() == 1 {
        peers[0].as_user_id().map(ToString::to_string)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dm_presence_peer_uses_unambiguous_matrix_ids_and_excludes_self() {
        let own = matrix_sdk::ruma::user_id!("@me:example.org");
        let targets = |ids: &[&str]| {
            ids.iter()
                .map(|id| matrix_sdk::ruma::OwnedDirectUserIdentifier::from(*id))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            direct_peer(own, targets(&["@peer:example.org"])).as_deref(),
            Some("@peer:example.org")
        );
        assert_eq!(
            direct_peer(own, targets(&["@me:example.org", "@peer:example.org"])).as_deref(),
            Some("@peer:example.org")
        );
        for ids in [
            vec![],
            vec!["@me:example.org"],
            vec!["@a:example.org", "@b:example.org"],
            vec!["email@example.org"],
        ] {
            assert_eq!(direct_peer(own, targets(&ids)), None);
        }
    }

    #[test]
    fn element_sections_validate_definitions_and_preserve_order_and_identity() {
        let settings = serde_json::json!({
            "RoomList.CustomSectionData": {
                "element.io.section.a":{"tag":"element.io.section.a","name":"Rooms","spaceId":"!space:example.org"},
                "element.io.section.b":{"tag":"element.io.section.b","name":"Rooms"},
                "element.io.section.c":{"tag":"element.io.section.c","name":"Unordered"},
                "element.io.section.bad":{"tag":"element.io.section.mismatch","name":"Bad"},
                "element.io.section.null":null,
                "element.io.section.number":{"tag":"element.io.section.number","name":4},
                "other":{"tag":"other","name":"Other"}
            },
            "RoomList.OrderedCustomSections":[null, 5, "chats", "element.io.section.b", "element.io.section.a", "element.io.section.b", "unknown"]
        });
        let sections = sections_from_settings(&settings);
        let ids: Vec<_> = sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "Unread",
                "m.favourite",
                "Direct messages",
                "element.io.section.c",
                "Rooms",
                "element.io.section.b",
                "element.io.section.a",
                "m.lowpriority"
            ]
        );
        let mut room = RoomEntry {
            tags: vec![
                "element.io.section.a".into(),
                "element.io.section.b".into(),
                "m.lowpriority".into(),
            ],
            direct: true,
            unread: true,
            ..Default::default()
        };
        assert_eq!(room.section_in(&sections), "element.io.section.b");
        room.favorite = true;
        assert_eq!(room.section_in(&sections), "m.favourite");
        room.favorite = false;
        room.tags = vec!["m.lowpriority".into()];
        assert_eq!(room.section_in(&sections), "m.lowpriority");
        room.tags = vec!["element.io.section.deleted".into()];
        assert_eq!(room.section_in(&sections), "Direct messages");
        for malformed in [
            serde_json::Value::Null,
            serde_json::json!({
                "RoomList.CustomSectionData": [], "RoomList.OrderedCustomSections": {}
            }),
        ] {
            assert_eq!(sections_from_settings(&malformed).len(), 5);
        }
    }

    #[test]
    fn nested_shared_and_cyclic_membership_is_unique() {
        let spaces = vec![
            SpaceEntry {
                id: "a".into(),
                children: vec!["b".into(), "shared".into()],
                ..Default::default()
            },
            SpaceEntry {
                id: "b".into(),
                children: vec!["a".into(), "shared".into(), "nested".into()],
                ..Default::default()
            },
            SpaceEntry {
                id: "c".into(),
                children: vec!["shared".into()],
                ..Default::default()
            },
        ];
        let ids = members(&Scope::Space("a".into()), &spaces).unwrap();
        assert_eq!(ids.len(), 4);
        assert!(ids.contains("nested"));
        assert!(!ids.contains("c"));
        assert!(
            members(&Scope::Space("c".into()), &spaces)
                .unwrap()
                .contains("shared")
        );
    }
    #[test]
    fn section_precedence_keeps_favorite_and_direct_unread_badges() {
        let mut room = RoomEntry {
            favorite: true,
            direct: true,
            unread: true,
            unread_count: 2,
            ..Default::default()
        };
        assert_eq!(room.section(), "m.favourite");
        room.favorite = false;
        assert_eq!(room.section(), "Direct messages");
        assert!(includes(&Scope::Direct, &None, &room));
        room.direct = false;
        assert_eq!(room.section(), "Rooms");
        room.unread = false;
        assert_eq!(room.section(), "Rooms");
    }
}
