//! Presence snapshots with bounded concurrency and optional navigator-wide polling.
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

/// View demand plus optional navigator-wide polling. View changes supersede
/// member reads; navigator-only work is independent of conversation selection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Demand {
    pub users: Vec<String>,
    /// Unique senders in the actual room/thread viewports and thread original.
    pub timeline_users: Vec<String>,
    /// All unambiguous DM peers, independent of room selection or space filters.
    pub navigator_users: Vec<String>,
    pub refresh: u64,
    pub poll_interval: Option<Duration>,
}

const CACHE_TTL: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUESTED: usize = 50;
const CONCURRENCY: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Request {
    room: Option<(u64, String)>,
    thread: Option<(u64, String)>,
    members: Option<u64>,
    demand: Demand,
}
fn requested(client: &Client, selection: &super::Selection) -> Request {
    let mut demand = selection.presence.clone();
    if selection.shutdown || selection.room.is_none() {
        demand.users.clear();
        demand.timeline_users.clear();
    }
    let valid = |user: &String| {
        user.parse::<matrix_sdk::ruma::OwnedUserId>()
            .is_ok_and(|user| Some(user.as_ref()) != client.user_id())
    };
    demand.users.retain(valid);
    demand.users.sort();
    demand.users.dedup();
    demand.users.truncate(MAX_REQUESTED);
    demand.timeline_users.retain(valid);
    demand.timeline_users.sort();
    demand.timeline_users.dedup();
    if demand
        .poll_interval
        .is_none_or(|interval| interval.is_zero())
        || selection.shutdown
    {
        demand.navigator_users.clear();
    }
    demand.navigator_users.retain(valid);
    demand.navigator_users.sort();
    demand.navigator_users.dedup();
    // Navigator polling already covers these senders. Keeping only additional
    // viewport users avoids tying navigator-only work to room/thread changes.
    demand
        .timeline_users
        .retain(|user| demand.navigator_users.binary_search(user).is_err());
    let view_scoped = !demand.users.is_empty() || !demand.timeline_users.is_empty();
    demand.users.extend(demand.timeline_users.iter().cloned());
    demand.users.extend(demand.navigator_users.iter().cloned());
    demand.users.sort();
    demand.users.dedup();
    if selection.shutdown {
        demand.users.clear();
    }
    Request {
        room: view_scoped.then(|| selection.room.clone()).flatten(),
        thread: (!demand.timeline_users.is_empty())
            .then(|| selection.thread.clone())
            .flatten(),
        members: view_scoped.then_some(selection.members).flatten(),
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

/// Polling arms one deadline after each completed batch, only when opted in and
/// users are requested. Manual and periodic refreshes bypass the cache.
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
    let mut periodic_refresh = false;
    loop {
        trim_cache(&mut cache, &current.demand.users);
        let force = periodic_refresh || current.demand.refresh != last_refresh;
        periodic_refresh = false;
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
        .buffer_unordered(CONCURRENCY);
        tokio::pin!(batch);
        loop {
            tokio::select! {
                result = batch.next() => {
                    let next = requested(client, &selection.borrow_and_update());
                    if next != current { current = next; break; }
                    let loading = result.is_some();
                    if let Some((user, status)) = result {
                        if !cache.contains_key(&user) && cache.len() >= MAX_USERS
                            && let Some(oldest) = cache.iter()
                                .filter(|(user, _)| current.demand.users.binary_search(user).is_err())
                                .min_by_key(|(_, (_, at))| *at).map(|(user, _)| user.clone()) {
                            cache.remove(&oldest);
                        }
                        cache.insert(user, (status, Instant::now()));
                    }
                    let users = cache.iter().filter(|(_, (status, _))| *status != Status::Unknown)
                        .map(|(user, (status, _))| (user.clone(), *status)).collect();
                    // Admit completed reads before a large/slow batch finishes,
                    // preserving progress if scrolling changes the viewport.
                    publish(state, &users, loading);
                    if loading { continue; }
                    // Delay from completion so slow requests never overlap or
                    // accumulate missed ticks. Unrelated updates keep this deadline.
                    let deadline = poll_deadline(&current);
                    loop {
                        tokio::select! {
                            _ = wait_for_poll(deadline) => { periodic_refresh = true; break; }
                            result = selection.changed() => {
                                if result.is_err() { return; }
                                let next = requested(client, &selection.borrow_and_update());
                                if next != current { current = next; break; }
                            }
                        }
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

fn trim_cache(cache: &mut BTreeMap<String, (Status, tokio::time::Instant)>, users: &[String]) {
    let excess = cache.len().saturating_sub(MAX_USERS.max(users.len()));
    if excess == 0 {
        return;
    }
    // Large navigator demand may exceed the normal cache limit. On contraction,
    // release the oldest unrequested entries without evicting current contacts.
    let mut candidates: Vec<_> = cache
        .iter()
        .filter(|(user, _)| users.binary_search(user).is_err())
        .map(|(user, (_, at))| (user.clone(), *at))
        .collect();
    candidates.sort_unstable_by_key(|(_, at)| *at);
    for (user, _) in candidates.into_iter().take(excess) {
        cache.remove(&user);
    }
}

fn poll_deadline(request: &Request) -> Option<tokio::time::Instant> {
    request
        .demand
        .poll_interval
        .filter(|interval| !interval.is_zero() && !request.demand.users.is_empty())
        .map(|interval| tokio::time::Instant::now() + interval)
}

async fn wait_for_poll(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
