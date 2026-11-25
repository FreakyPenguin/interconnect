//! On-demand alias and directory management. Writes never automatically retry.
use matrix_sdk::{
    Room, RoomState,
    config::RequestConfig,
    ruma::{
        OwnedRoomAliasId, RoomAliasId,
        api::{
            client::{
                alias::{create_alias, delete_alias},
                directory::{get_room_visibility, set_room_visibility},
                room::{Visibility, aliases},
                state::{get_state_event_for_key, send_state_event},
            },
            error::ErrorKind,
        },
        events::{
            EmptyStateKey, StateEventType,
            room::{
                canonical_alias::RoomCanonicalAliasEventContent,
                power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent},
            },
        },
    },
};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Published {
    pub main: Option<OwnedRoomAliasId>,
    pub alternatives: Vec<OwnedRoomAliasId>,
}
impl Published {
    pub fn all(&self) -> Vec<OwnedRoomAliasId> {
        let mut aliases = self.alternatives.clone();
        aliases.extend(self.main.clone());
        aliases.sort();
        aliases.dedup();
        aliases
    }
}
#[derive(Clone, Debug)]
pub struct Addresses {
    pub server: String,
    pub published: Published,
    pub editable: bool,
    pub local: Result<Vec<OwnedRoomAliasId>, String>,
    pub directory: Result<bool, String>,
}
#[derive(Clone, Debug)]
pub enum Change {
    Published(Published, Published),
    Directory(bool, bool),
    AddLocal(OwnedRoomAliasId),
    RemoveLocal(OwnedRoomAliasId),
}
impl Addresses {
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Published(_, value) => self.published = value.clone(),
            Change::Directory(_, value) => self.directory = Ok(*value),
            Change::AddLocal(alias) => {
                if let Ok(local) = &mut self.local {
                    local.push(alias.clone());
                    local.sort();
                    local.dedup();
                }
            }
            Change::RemoveLocal(alias) => {
                if let Ok(local) = &mut self.local {
                    local.retain(|a| a != alias);
                }
            }
        }
    }
    pub fn demo(editable: bool) -> Self {
        Self {
            server: "demo.invalid".into(),
            published: Published::default(),
            editable,
            local: Ok(Vec::new()),
            directory: Ok(false),
        }
    }
}
/// Accept a localpart or a complete alias, but never silently change its server.
pub fn parse(input: &str, server: &str, local: bool) -> Result<OwnedRoomAliasId, String> {
    let input = input.trim();
    let part = input.strip_prefix('#').unwrap_or(input);
    if part.is_empty() || part.chars().any(char::is_whitespace) || part.contains(['/', '?', '#']) {
        return Err("Enter a room address, such as #my-room:example.org.".into());
    }
    let full = if part.contains(':') {
        format!("#{part}")
    } else {
        format!("#{part}:{server}")
    };
    let alias = RoomAliasId::parse(full).map_err(|_| "Enter a valid room address.".to_owned())?;
    if local && alias.server_name().as_str() != server {
        return Err(format!("Local addresses must belong to {server}."));
    }
    Ok(alias)
}
fn config() -> RequestConfig {
    RequestConfig::default()
        .disable_retry()
        .timeout(Duration::from_secs(15))
}
async fn published(room: &Room) -> Result<Published, String> {
    let request = get_state_event_for_key::v3::Request::new(
        room.room_id().to_owned(),
        StateEventType::RoomCanonicalAlias,
        String::new(),
    );
    match room.client().send(request).await {
        Ok(response) => {
            let content: RoomCanonicalAliasEventContent =
                serde_json::from_str(response.event_or_content.get())
                    .map_err(|_| "Could not read published addresses.")?;
            Ok(Published {
                main: content.alias,
                alternatives: content.alt_aliases,
            })
        }
        Err(error) if error.client_api_error_kind() == Some(&ErrorKind::NotFound) => {
            Ok(Published::default())
        }
        Err(_) => Err("Could not load published addresses. Try Reload.".into()),
    }
}
async fn local(room: &Room) -> Result<Vec<OwnedRoomAliasId>, String> {
    let mut aliases = room
        .client()
        .send(aliases::v3::Request::new(room.room_id().to_owned()))
        .await
        .map_err(|_| "Local addresses unavailable. Try Reload.".to_owned())?
        .aliases;
    aliases.retain(|a| a.server_name() == room.own_user_id().server_name());
    aliases.sort();
    aliases.dedup();
    Ok(aliases)
}
async fn directory(room: &Room) -> Result<bool, String> {
    let value = room
        .client()
        .send(get_room_visibility::v3::Request::new(
            room.room_id().to_owned(),
        ))
        .await
        .map_err(|_| "Directory visibility unavailable. Try Reload.".to_owned())?
        .visibility;
    match value {
        Visibility::Public => Ok(true),
        Visibility::Private => Ok(false),
        _ => Err("This directory visibility is unsupported.".into()),
    }
}
pub(crate) async fn read(room: &Room) -> Result<Addresses, String> {
    // Independent failures leave the remaining settings usable; no recurring refresh.
    let operation = async {
        let (published, local, directory, levels) = tokio::join!(
            published(room),
            local(room),
            directory(room),
            room.power_levels()
        );
        Ok(Addresses {
            server: room.own_user_id().server_name().to_string(),
            published: published?,
            editable: levels
                .map_err(|_| "Could not read address permissions.")?
                .user_can_send_state(room.own_user_id(), StateEventType::RoomCanonicalAlias),
            local,
            directory,
        })
    };
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .map_err(|_| "Loading addresses timed out. Try Reload.".to_owned())?
}
async fn check_permission(room: &Room) -> Result<(), String> {
    let request = get_state_event_for_key::v3::Request::new(
        room.room_id().to_owned(),
        StateEventType::RoomPowerLevels,
        String::new(),
    );
    let content: Option<RoomPowerLevelsEventContent> = match room.client().send(request).await {
        Ok(response) => Some(
            serde_json::from_str(response.event_or_content.get())
                .map_err(|_| "Could not read address permissions.")?,
        ),
        Err(error) if error.client_api_error_kind() == Some(&ErrorKind::NotFound) => None,
        Err(_) => return Err("Could not check address permissions.".into()),
    };
    let rules = room.clone_info().room_version_rules_or_default();
    let levels = RoomPowerLevels::new(
        content.into(),
        &rules.authorization,
        room.creators().ok_or("Unknown room creators.")?,
    );
    if !levels.user_can_send_state(room.own_user_id(), StateEventType::RoomCanonicalAlias) {
        return Err("You no longer have permission to publish addresses.".into());
    }
    Ok(())
}
async fn resolves_here(room: &Room, alias: &RoomAliasId) -> Result<(), String> {
    let resolved = room.client().resolve_room_alias(alias).await.map_err(|_| {
        "Could not resolve this address. Add it as a local address on its server first.".to_owned()
    })?;
    if resolved.room_id != room.room_id() {
        return Err("This address points to another room.".into());
    }
    Ok(())
}
pub(crate) async fn save(room: &Room, change: Change) -> Result<(), String> {
    let operation = async {
        if room.state() != RoomState::Joined {
            return Err("You are no longer joined to this room.".into());
        }
        match change {
            Change::Published(before, after) => {
                for alias in after.all() {
                    resolves_here(room, &alias).await?;
                }
                check_permission(room).await?;
                if published(room).await? != before {
                    return Err("Published addresses changed. Reload before saving again.".into());
                }
                let mut content = RoomCanonicalAliasEventContent::new();
                content.alias = after.main;
                content.alt_aliases = after.alternatives;
                let request = send_state_event::v3::Request::new(
                    room.room_id().to_owned(),
                    &EmptyStateKey,
                    &content,
                )
                .map_err(|_| "Could not prepare the address change.")?;
                room.client()
                    .send(request)
                    .with_request_config(config())
                    .await
                    .map_err(write_error)?;
            }
            Change::Directory(before, after) => {
                if directory(room).await? != before {
                    return Err("Directory visibility changed. Reload before saving again.".into());
                }
                // Directory policy is server-specific; the server authorizes this write.
                let visibility = if after {
                    Visibility::Public
                } else {
                    Visibility::Private
                };
                room.client()
                    .send(set_room_visibility::v3::Request::new(
                        room.room_id().to_owned(),
                        visibility,
                    ))
                    .with_request_config(config())
                    .await
                    .map_err(write_error)?;
            }
            Change::AddLocal(alias) => {
                parse(
                    alias.as_str(),
                    room.own_user_id().server_name().as_str(),
                    true,
                )?;
                if local(room).await?.contains(&alias) {
                    return Err("This local address already exists. Reload to see it.".into());
                }
                // Alias creation/deletion policy is enforced by the homeserver, not state power levels.
                room.client()
                    .send(create_alias::v3::Request::new(
                        alias,
                        room.room_id().to_owned(),
                    ))
                    .with_request_config(config())
                    .await
                    .map_err(write_error)?;
            }
            Change::RemoveLocal(alias) => {
                parse(
                    alias.as_str(),
                    room.own_user_id().server_name().as_str(),
                    true,
                )?;
                if published(room).await?.all().contains(&alias) {
                    return Err("Unpublish this address before removing it locally.".into());
                }
                resolves_here(room, &alias).await?;
                room.client()
                    .send(delete_alias::v3::Request::new(alias))
                    .with_request_config(config())
                    .await
                    .map_err(write_error)?;
            }
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(30), operation)
        .await
        .map_err(|_| uncertain())?
}
fn write_error(error: matrix_sdk::HttpError) -> String {
    match error.client_api_error_kind() {
        Some(ErrorKind::Forbidden) => {
            "The server denied this address change. Check your room permissions and server policy."
                .into()
        }
        Some(ErrorKind::RoomInUse) => {
            "This room address is already in use. Choose another address.".into()
        }
        _ => uncertain(),
    }
}
fn uncertain() -> String {
    "Could not confirm the address change. The server may have denied it or it may have succeeded. Reload before retrying.".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn address_input_respects_account_server_including_ports() {
        for input in ["friendly", "#friendly", "#friendly:example.org:8448"] {
            assert_eq!(
                parse(input, "example.org:8448", true).unwrap().as_str(),
                "#friendly:example.org:8448"
            );
        }
        assert!(parse("#friendly:elsewhere.org", "example.org", true).is_err());
        assert!(parse("#friendly:elsewhere.org", "example.org", false).is_ok());
        for input in [
            "",
            "#",
            "white space",
            "https://example.org",
            "#room?bad",
            "##room",
        ] {
            assert!(parse(input, "example.org", true).is_err(), "{input}");
        }
    }
}
