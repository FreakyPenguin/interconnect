//! Presence is absent from sliding-sync responses in the pinned SDK. A filtered
//! classic sync supplies it, with no room history or account-data projection.
use super::Model;
use matrix_sdk::{
    Client,
    config::{SyncSettings, SyncToken},
    ruma::{
        api::client::{filter::FilterDefinition, sync::sync_events::v3::Filter},
        events::presence::PresenceEvent,
        presence::PresenceState,
        serde::Raw,
    },
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::watch;

const LONG_POLL: Duration = Duration::from_secs(30);
const MAX_USERS: usize = 8192;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    Online,
    Away,
    Offline,
    #[default]
    Unknown,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Away => "Away",
            Self::Offline => "Offline",
            Self::Unknown => "Status unavailable",
        }
    }
    fn from_presence(presence: &PresenceState) -> Self {
        match presence {
            PresenceState::Online => Self::Online,
            PresenceState::Unavailable => Self::Away,
            PresenceState::Offline => Self::Offline,
            _ => Self::Unknown,
        }
    }
}
pub type Users = BTreeMap<String, Status>;

fn filter() -> Filter {
    let mut definition = FilterDefinition::ignore_all();
    definition.presence = Default::default();
    Filter::FilterDefinition(definition)
}

fn apply(events: &[Raw<PresenceEvent>], users: &mut Users) {
    for event in events.iter().filter_map(|event| event.deserialize().ok()) {
        let user = event.sender.to_string();
        if !users.contains_key(&user) && users.len() >= MAX_USERS {
            users.pop_first();
        }
        users.insert(user, Status::from_presence(&event.content.presence));
    }
}

fn publish(users: &Users, state: &watch::Sender<Model>) {
    state.send_if_modified(|model| {
        if *users == *model.presence {
            return false;
        }
        model.presence = Arc::new(users.clone());
        true
    });
}

pub(super) fn clear(state: &watch::Sender<Model>) {
    state.send_if_modified(|model| {
        if model.presence.is_empty() {
            return false;
        }
        model.presence = Arc::default();
        true
    });
}

/// Owned by the authenticated connection future; dropping it cancels requests
/// and backoff. Never run raw `/sync` and discard its to-device events: advancing
/// that token can acknowledge encryption/verification messages. `sync_once`
/// processes them through the same SDK before returning the next token.
pub(super) async fn run(client: &Client, state: &watch::Sender<Model>) {
    let mut users = Users::new();
    let mut token = SyncToken::NoToken;
    let mut backoff = Duration::from_secs(2);
    loop {
        let started = tokio::time::Instant::now();
        let settings = SyncSettings::default()
            .filter(filter())
            .token(token.clone())
            .timeout(LONG_POLL);
        match client.sync_once(settings).await {
            Ok(response) => {
                token = SyncToken::Specific(response.next_batch);
                apply(&response.presence, &mut users);
                publish(&users, state);
                backoff = Duration::from_secs(2);
                // Protect against servers returning immediately despite the long
                // poll. Normal idle responses never schedule an extra timer.
                let earliest = started + Duration::from_secs(1);
                if tokio::time::Instant::now() < earliest {
                    tokio::time::sleep_until(earliest).await;
                }
            }
            Err(_) => {
                // Do not present cached online users as live during an outage.
                clear(state);
                // Retain the last processed token/cache privately. Recovery can
                // replay deltas and restore unchanged users, even if the SDK
                // deduplicates a response with the same next_batch token.
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}

#[cfg(test)]
mod tests;
