//! Account navigation is a projection of SDK metadata, independent of open timelines.
use matrix_sdk::{
    Client,
    ruma::events::{StateEventType, SyncStateEvent, space::child::SpaceChildEventContent},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Home,
    #[default]
    All,
    Direct,
    Space(String),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoomEntry {
    pub id: String,
    pub name: String,
    pub canonical_alias: Option<String>,
    pub topic: String,
    /// Joined members, excluding invitations and departed users.
    pub member_count: u64,
    pub can_edit_topic: bool,
    pub can_pin: bool,
    pub can_redact_own: bool,
    pub can_redact_other: bool,
    pub can_invite: bool,
    pub can_notify_room: bool,
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
                Scope::Home | Scope::All | Scope::Direct => {
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
    /// Permission to change this space’s direct child links; unknown permissions deny.
    pub can_manage_rooms: bool,
    pub id: String,
    pub name: String,
    pub avatar_url: Option<matrix_sdk::ruma::OwnedMxcUri>,
    /// Active m.space.child links. Unknown/unjoined children are not displayed.
    pub children: Vec<String>,
}

/// Descendants for a selected space, or space-linked exclusions for Home.
/// Home includes the complement plus every DM, matching Element's default.
/// Traversal tolerates cycles and shared children without duplicating rooms.
pub fn members(scope: &Scope, spaces: &[SpaceEntry]) -> Option<HashSet<String>> {
    if *scope == Scope::Home {
        return Some(
            spaces
                .iter()
                .flat_map(|space| space.children.iter().cloned())
                .collect(),
        );
    }
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
        Scope::Home => room.direct || members.as_ref().is_none_or(|ids| !ids.contains(&room.id)),
        Scope::All => true,
        Scope::Direct => room.direct,
        Scope::Space(_) => members.as_ref().is_some_and(|ids| ids.contains(&room.id)),
    }
}

/// Room membership for the desktop's current location. Home is the virtual
/// parent of every space; its direct rooms have no joined-space child link.
/// DMs follow the same rule as other rooms. Unknown children do not add rooms.
pub fn location_members(
    scope: &Scope,
    spaces: &[SpaceEntry],
    recursive: bool,
) -> Option<HashSet<String>> {
    match scope {
        Scope::Home if recursive => None,
        Scope::Space(id) if !recursive => Some(
            spaces
                .iter()
                .find(|space| &space.id == id)
                .into_iter()
                .flat_map(|space| space.children.iter().cloned())
                .collect(),
        ),
        _ => members(scope, spaces),
    }
}

pub fn location_includes(
    scope: &Scope,
    members: &Option<HashSet<String>>,
    room: &RoomEntry,
) -> bool {
    match scope {
        Scope::Home => members.as_ref().is_none_or(|ids| !ids.contains(&room.id)),
        _ => includes(scope, members, room),
    }
}

/// Stable roots, including a representative for each otherwise unreachable
/// cyclic component. Shared children remain available beneath each parent.
pub fn space_roots(spaces: &[SpaceEntry]) -> Vec<String> {
    let children: HashSet<_> = spaces.iter().flat_map(|s| s.children.iter()).collect();
    let mut ordered: Vec<_> = spaces.iter().collect();
    ordered.sort_by_cached_key(|s| (s.name.to_lowercase(), s.id.clone()));
    let mut roots: Vec<_> = ordered
        .iter()
        .filter(|s| !children.contains(&s.id))
        .map(|s| s.id.clone())
        .collect();
    let mut reachable = HashSet::new();
    for root in &roots {
        reachable.extend(members(&Scope::Space(root.clone()), spaces).unwrap_or_default());
    }
    for space in ordered {
        if !reachable.contains(&space.id) {
            roots.push(space.id.clone());
            reachable.extend(members(&Scope::Space(space.id.clone()), spaces).unwrap_or_default());
        }
    }
    roots
}

pub fn valid_space_path(spaces: &[SpaceEntry], path: &[String]) -> bool {
    let mut seen = HashSet::new();
    !path.is_empty()
        && path.iter().enumerate().all(|(index, id)| {
            seen.insert(id)
                && spaces.iter().any(|space| &space.id == id)
                && (index == 0
                    || spaces
                        .iter()
                        .find(|space| space.id == path[index - 1])
                        .is_some_and(|space| space.children.contains(id)))
        })
}

/// Prefer the route the user followed. Fall back to a breadth-first route from
/// a visible root after metadata changes or restoring older preferences.
pub fn space_path(spaces: &[SpaceEntry], target: &str, preferred: &[String]) -> Vec<String> {
    if let Some(index) = preferred.iter().position(|id| id == target)
        && valid_space_path(spaces, &preferred[..=index])
    {
        return preferred[..=index].to_vec();
    }
    let mut pending: std::collections::VecDeque<_> =
        space_roots(spaces).into_iter().map(|id| vec![id]).collect();
    let mut visited = HashSet::new();
    while let Some(path) = pending.pop_front() {
        let id = path.last().unwrap();
        if !visited.insert(id.clone()) {
            continue;
        }
        if id == target {
            return path;
        }
        if let Some(space) = spaces.iter().find(|s| &s.id == id) {
            for child in &space.children {
                if spaces.iter().any(|s| &s.id == child) && !visited.contains(child) {
                    let mut next = path.clone();
                    next.push(child.clone());
                    pending.push_back(next);
                }
            }
        }
    }
    Vec::new()
}

pub struct Snapshot {
    pub rooms: Vec<RoomEntry>,
    pub spaces: Vec<SpaceEntry>,
    pub sections: Vec<SectionEntry>,
    pub all_rooms_in_home: bool,
}

/// Read Element's account preference without changing its settings.
fn all_rooms_in_home(settings: &serde_json::Value) -> bool {
    settings["Spaces.allRoomsInHome"].as_bool().unwrap_or(false)
}

pub async fn snapshot(client: &Client) -> anyhow::Result<Snapshot> {
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
                can_manage_rooms: room.power_levels().await.ok().is_some_and(|levels| {
                    levels.user_can_send_state(room.own_user_id(), StateEventType::SpaceChild)
                }),
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
                canonical_alias: room.canonical_alias().map(|alias| alias.to_string()),
                topic: room.topic().unwrap_or_default(),
                member_count: room.joined_members_count(),
                // Incomplete cached permissions must not hide the conversation.
                can_edit_topic: levels.as_ref().is_some_and(|levels| {
                    levels.user_can_send_state(
                        room.own_user_id(),
                        matrix_sdk::ruma::events::StateEventType::RoomTopic,
                    )
                }),
                can_notify_room: levels.as_ref().is_some_and(|levels| {
                    levels.user_can_trigger_room_notification(room.own_user_id())
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
    Ok(Snapshot {
        rooms,
        spaces,
        sections,
        all_rooms_in_home: all_rooms_in_home(&settings),
    })
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

    fn graph() -> Vec<SpaceEntry> {
        [
            ("a", vec!["b", "direct", "dm"]),
            ("b", vec!["nested", "shared", "a"]),
            ("c", vec!["shared", "b"]),
            ("loop", vec!["loop", "unknown"]),
        ]
        .into_iter()
        .map(|(id, children)| SpaceEntry {
            id: id.into(),
            name: id.into(),
            children: children.into_iter().map(String::from).collect(),
            ..Default::default()
        })
        .collect()
    }

    #[test]
    fn location_filter_has_direct_recursive_and_strict_home_semantics() {
        let spaces = graph();
        let scope = Scope::Space("a".into());
        let direct = location_members(&scope, &spaces, false);
        let recursive = location_members(&scope, &spaces, true);
        for (id, in_direct, in_recursive) in [
            ("direct", true, true),
            ("nested", false, true),
            ("shared", false, true),
            ("outside", false, false),
        ] {
            let room = RoomEntry {
                id: id.into(),
                ..Default::default()
            };
            assert_eq!(location_includes(&scope, &direct, &room), in_direct);
            assert_eq!(location_includes(&scope, &recursive, &room), in_recursive);
        }
        let dm = RoomEntry {
            id: "dm".into(),
            direct: true,
            ..Default::default()
        };
        assert!(!location_includes(
            &Scope::Home,
            &location_members(&Scope::Home, &spaces, false),
            &dm
        ));
        assert!(location_includes(
            &Scope::Home,
            &location_members(&Scope::Home, &spaces, true),
            &dm
        ));
        assert_eq!(
            recursive
                .unwrap()
                .iter()
                .filter(|id| id.as_str() == "shared")
                .count(),
            1
        );
    }

    #[test]
    fn paths_keep_shared_parent_context_and_all_cycles_remain_reachable() {
        let spaces = graph();
        let roots = space_roots(&spaces);
        assert!(roots.contains(&"c".into()));
        assert!(roots.contains(&"loop".into()));
        for space in &spaces {
            let path = space_path(&spaces, &space.id, &[]);
            assert!(valid_space_path(&spaces, &path));
            assert_eq!(path.last(), Some(&space.id));
        }
        assert_eq!(
            space_path(&spaces, "b", &["a".into(), "b".into()]),
            ["a", "b"]
        );
        assert_eq!(
            space_path(&spaces, "b", &["c".into(), "b".into()]),
            ["c", "b"]
        );
        assert!(!valid_space_path(
            &spaces,
            &["a".into(), "b".into(), "a".into()]
        ));
        assert!(!valid_space_path(&spaces, &["c".into(), "unknown".into()]));
        assert!(space_path(&spaces, "missing", &[]).is_empty());
    }
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
    fn home_includes_orphans_and_all_dms_but_excludes_space_rooms_even_if_favorite() {
        let spaces = vec![
            SpaceEntry {
                id: "parent".into(),
                children: vec!["child".into(), "room".into(), "dm".into()],
                ..Default::default()
            },
            SpaceEntry {
                id: "child".into(),
                children: vec!["parent".into(), "nested".into()],
                ..Default::default()
            },
            SpaceEntry {
                id: "shared".into(),
                children: vec!["room".into()],
                ..Default::default()
            },
        ];
        let linked = members(&Scope::Home, &spaces);
        let mut room = RoomEntry {
            id: "orphan".into(),
            ..Default::default()
        };
        assert!(includes(&Scope::Home, &linked, &room));
        room.id = "nested".into();
        assert!(!includes(&Scope::Home, &linked, &room));
        room.id = "room".into();
        room.favorite = true;
        assert!(!includes(&Scope::Home, &linked, &room));
        room.id = "dm".into();
        room.direct = true;
        assert!(includes(&Scope::Home, &linked, &room));
        assert!(includes(&Scope::All, &linked, &room));
        room.direct = false;
        assert!(includes(&Scope::Home, &members(&Scope::Home, &[]), &room));
    }

    #[test]
    fn element_home_setting_defaults_to_false_and_home_uses_global_sections() {
        for settings in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!({"Spaces.allRoomsInHome":false}),
            serde_json::json!({"Spaces.allRoomsInHome":"true"}),
        ] {
            assert!(!all_rooms_in_home(&settings));
        }
        assert!(all_rooms_in_home(
            &serde_json::json!({"Spaces.allRoomsInHome":true})
        ));
        let mut section = SectionEntry {
            id: "element.io.section.home".into(),
            name: "Custom".into(),
            space_id: Some("home-space".into()),
        };
        assert!(section.show_empty(&Scope::Home));
        section.space_id = Some("parent".into());
        assert!(!section.show_empty(&Scope::Home));
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
