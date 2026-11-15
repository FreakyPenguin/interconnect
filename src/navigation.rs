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
    pub can_invite: bool,
    pub pinned_events: Vec<String>,
    pub favorite: bool,
    pub direct: bool,
    pub unread: bool,
    /// SDK client-side unread message count; not a claim about thread unread state.
    pub unread_count: u64,
}
impl RoomEntry {
    pub fn section(&self) -> &'static str {
        if self.favorite {
            "Favorites"
        } else if self.direct {
            "Direct messages"
        } else if self.unread {
            "Unread"
        } else {
            "Rooms"
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpaceEntry {
    pub id: String,
    pub name: String,
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

pub async fn snapshot(client: &Client) -> anyhow::Result<(Vec<RoomEntry>, Vec<SpaceEntry>)> {
    let mut rooms = Vec::new();
    let mut spaces = Vec::new();
    for room in client.joined_rooms() {
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
            spaces.push(SpaceEntry { id, name, children });
        } else {
            let unread_count = room.num_unread_messages();
            let levels = room.power_levels().await.ok();
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
                pinned_events: room
                    .pinned_event_ids()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect(),
                favorite: room.is_favourite(),
                direct: room.is_direct().await?,
                unread: room.is_marked_unread()
                    || unread_count > 0
                    || room.num_unread_notifications() > 0,
                unread_count,
            });
        }
    }
    rooms.sort_by_cached_key(|r| (r.name.to_lowercase(), r.id.clone()));
    spaces.sort_by_cached_key(|s| (s.name.to_lowercase(), s.id.clone()));
    Ok((rooms, spaces))
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(room.section(), "Favorites");
        room.favorite = false;
        assert_eq!(room.section(), "Direct messages");
        assert!(includes(&Scope::Direct, &None, &room));
        room.direct = false;
        assert_eq!(room.section(), "Unread");
        room.unread = false;
        assert_eq!(room.section(), "Rooms");
    }
}
