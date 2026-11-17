//! Bounded, cached presence snapshots fetched only for explicit view demand.
use super::Model;
use matrix_sdk::{
    Client,
    ruma::{api::client::presence::get_presence, presence::PresenceState},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::watch;

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

/// Explicit view demand. The account session owns the cache and cancels a batch
/// when its room, People generation/page, or refresh request is superseded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Demand {
    pub users: Vec<String>,
    pub refresh: u64,
}

const CACHE_TTL: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUESTED: usize = 50;
const CONCURRENCY: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Request {
    room: Option<(u64, String)>,
    members: Option<u64>,
    demand: Demand,
}
fn requested(client: &Client, selection: &super::Selection) -> Request {
    let mut demand = selection.presence.clone();
    if selection.shutdown || selection.room.is_none() {
        demand.users.clear();
    }
    demand.users.retain(|user| {
        user.parse::<matrix_sdk::ruma::OwnedUserId>()
            .is_ok_and(|user| Some(user.as_ref()) != client.user_id())
    });
    demand.users.sort();
    demand.users.dedup();
    demand.users.truncate(MAX_REQUESTED);
    Request {
        room: selection.room.clone(),
        members: selection.members,
        demand,
    }
}

fn publish(state: &watch::Sender<Model>, users: &Users, loading: bool) {
    state.send_if_modified(|model| {
        if *model.presence == *users && model.presence_loading == loading {
            return false;
        }
        model.presence = Arc::new(users.clone());
        model.presence_loading = loading;
        true
    });
}

pub(super) fn clear(state: &watch::Sender<Model>) {
    publish(state, &Users::new(), false);
}

/// No classic sync, timer, or automatic retry. Cache age is checked only when a
/// view requests users; Refresh bypasses it for the currently requested users.
pub(super) async fn run(
    client: &Client,
    mut selection: watch::Receiver<super::Selection>,
    state: &watch::Sender<Model>,
) {
    use iced::futures::{StreamExt, stream};
    use tokio::time::Instant;
    let mut cache = BTreeMap::<String, (Status, Instant)>::new();
    let mut current = requested(client, &selection.borrow_and_update());
    let mut last_refresh = current.demand.refresh;
    loop {
        let force = current.demand.refresh != last_refresh;
        last_refresh = current.demand.refresh;
        let wanted: Vec<_> = current
            .demand
            .users
            .iter()
            .filter(|user| {
                force
                    || cache
                        .get(*user)
                        .is_none_or(|(_, at)| at.elapsed() >= CACHE_TTL)
            })
            .cloned()
            .collect();
        let users = || {
            cache
                .iter()
                .filter(|(_, (status, _))| *status != Status::Unknown)
                .map(|(user, (status, _))| (user.clone(), *status))
                .collect::<Users>()
        };
        publish(state, &users(), !wanted.is_empty());
        let batch = stream::iter(wanted.into_iter().map(|user| async move {
            let request = get_presence::v3::Request::new(user.parse().expect("validated user ID"));
            let result = tokio::time::timeout(REQUEST_TIMEOUT, client.send(request)).await;
            let status = match result {
                Ok(Ok(response)) => Status::from_presence(&response.presence),
                _ => Status::Unknown,
            };
            (user, status)
        }))
        .buffer_unordered(CONCURRENCY)
        .collect::<Vec<_>>();
        tokio::pin!(batch);
        loop {
            tokio::select! {
                results = &mut batch => {
                    let next = requested(client, &selection.borrow_and_update());
                    if next != current { current = next; break; }
                    for (user, status) in results {
                        if !cache.contains_key(&user) && cache.len() >= MAX_USERS
                            && let Some(oldest) = cache.iter().min_by_key(|(_, (_, at))| *at).map(|(user, _)| user.clone()) {
                            cache.remove(&oldest);
                        }
                        cache.insert(user, (status, Instant::now()));
                    }
                    let users = cache.iter().filter(|(_, (status, _))| *status != Status::Unknown)
                        .map(|(user, (status, _))| (user.clone(), *status)).collect();
                    publish(state, &users, false);
                    // A settled batch waits for explicit new demand. Unrelated UI
                    // updates do not expire the cache or trigger more requests.
                    loop {
                        if selection.changed().await.is_err() { return; }
                        let next = requested(client, &selection.borrow_and_update());
                        if next != current { current = next; break; }
                    }
                    break;
                }
                result = selection.changed() => {
                    if result.is_err() { return; }
                    let next = requested(client, &selection.borrow_and_update());
                    if next != current { current = next; break; }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
