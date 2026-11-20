//! One bounded history read per encrypted conversation, once per session.
//! The SDK owns missing-key requests, deduplication, forwarding, and storage.
use std::{cmp::Reverse, future::Future, time::Duration};

use iced::futures::{Stream, StreamExt, stream};
use matrix_sdk::{
    Client, Room, RoomState, encryption::VerificationState, room::MessagesOptions, ruma::uint,
};
use matrix_sdk_ui::{
    room_list_service::{RoomListLoadingState, State},
    sync_service::SyncService,
};

const ROOM_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) async fn run(
    client: &Client,
    sync: &SyncService,
    mut state: tokio::sync::watch::Receiver<super::Model>,
) {
    let trust = client.encryption().verification_state();
    let updates = stream::iter([trust.get()]).chain(trust);
    let permission = async {
        state
            .wait_for(|s| s.verification.prefetch_keys.is_some())
            .await
            .ok()
            .and_then(|s| s.verification.prefetch_keys)
            .unwrap_or(false)
    };
    when_ready(client, updates, discover_rooms(sync), permission).await;
}

async fn discover_rooms(sync: &SyncService) -> bool {
    let room_list = sync.room_list_service();
    let mut states = room_list.state();
    let Ok(rooms) = room_list.all_rooms().await else {
        return false;
    };
    let mut loading = rooms.loading_state();
    let mut updates = room_list.client().subscribe_to_all_room_updates();
    // Running/Loaded describe the initial batches too. Wait until the known
    // joined/invited rooms cover the server's count, excluding retained left
    // rooms. Cached rooms can contribute when restoring an existing session.
    loop {
        let known =
            room_list.client().joined_rooms().len() + room_list.client().invited_rooms().len();
        if states.get() == State::Running
            && matches!(loading.get(), RoomListLoadingState::Loaded {
                maximum_number_of_rooms: Some(total)
            } if known >= total as usize)
        {
            return true;
        }
        tokio::select! {
            state = states.next() => if state.is_none() { return false; },
            state = loading.next() => if state.is_none() { return false; },
            update = updates.recv() => if matches!(update, Err(tokio::sync::broadcast::error::RecvError::Closed)) { return false; },
        }
    }
}

async fn when_ready(
    client: &Client,
    trust: impl Stream<Item = VerificationState>,
    discovery: impl Future<Output = bool>,
    permission: impl Future<Output = bool>,
) {
    tokio::pin!(trust, discovery, permission);
    let mut verified = false;
    let mut discovered = false;
    let mut permitted = false;
    while !verified || !discovered || !permitted {
        tokio::select! {
            value = trust.next() => match value {
                Some(value) => verified = value == VerificationState::Verified,
                None => return,
            },
            ready = &mut discovery, if !discovered => {
                if !ready { return; }
                discovered = true;
            },
            allowed = &mut permission, if !permitted => {
                if !allowed { return; }
                permitted = true;
            },
        }
    }

    let pass = prefetch(client);
    tokio::pin!(pass);
    loop {
        tokio::select! {
            // A trust revocation cancels an in-flight read and the remaining
            // queue. Already queued SDK requests remain under SDK ownership.
            biased;
            value = trust.next() => if value != Some(VerificationState::Verified) { return; },
            _ = &mut pass => return,
        }
    }
}

async fn prefetch(client: &Client) {
    let mut rooms: Vec<_> = client
        .joined_rooms()
        .into_iter()
        .filter(eligible)
        // Snapshot scores once: sync can change recency while sorting.
        .map(|room| {
            let stamp = room.recency_stamp().map(u64::from);
            let timestamp = room.latest_event_timestamp().map(|ts| u64::from(ts.get()));
            (
                Reverse(stamp),
                Reverse(timestamp),
                room.room_id().to_owned(),
                room,
            )
        })
        .collect();
    rooms.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    for (_, _, _, room) in rooms {
        // One room at a time bounds network and decryption work. No pagination,
        // subscription changes, read receipts, retries, or background timer.
        match tokio::time::timeout(ROOM_TIMEOUT, prefetch_room(&room)).await {
            Ok(Ok(())) => {}
            _ => tracing::debug!(target: "interconnect::key_prefetch", "Skipped unavailable room"),
        }
    }
    tracing::debug!(target: "interconnect::key_prefetch", "Room key prefetch finished");
}

fn eligible(room: &Room) -> bool {
    room.state() == RoomState::Joined
        && !room.is_space()
        && !crate::bookmarks::is_bookmarks_room(room)
        && (room.encryption_state().is_encrypted() || room.encryption_state().is_unknown())
}

async fn prefetch_room(room: &Room) -> matrix_sdk::Result<()> {
    if !eligible(room) {
        return Ok(());
    }
    if room.encryption_state().is_unknown() && !room.latest_encryption_state().await?.is_encrypted()
    {
        return Ok(());
    }
    // Membership may have changed while resolving unknown encryption state.
    if !eligible(room) {
        return Ok(());
    }
    let mut options = MessagesOptions::backward();
    options.limit = uint!(50);
    options.filter.types = Some(vec!["m.room.encrypted".into()]);
    // messages() decrypts through the SDK, which queues missing session keys
    // for the existing encryption sync worker to send to verified own devices.
    // Discard content and tokens; this must not move any UI timeline cursor.
    room.messages(options).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
