//! Room-scoped ephemeral activity. No timers exist when nobody is typing.
use super::{Model, Selection};
use matrix_sdk::{Client, Room};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast::error::RecvError, watch};
use tokio_util::sync::CancellationToken;

const IDLE: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const INCOMING_EXPIRY: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub generation: u64,
    pub at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Typing {
    pub generation: u64,
    pub names: Vec<String>,
}

pub(super) fn incoming(
    client: Client,
    id: String,
    generation: u64,
    state: watch::Sender<Model>,
) -> super::ViewFuture {
    let Ok(id) = id.parse::<matrix_sdk::ruma::OwnedRoomId>() else {
        return super::inactive();
    };
    let Some(room) = client.get_room(&id) else {
        return super::inactive();
    };
    incoming_room(room, generation, state, INCOMING_EXPIRY)
}

fn incoming_room(
    room: Room,
    generation: u64,
    state: watch::Sender<Model>,
    ttl: Duration,
) -> super::ViewFuture {
    // Register before requesting the room subscription so its first response cannot be lost.
    let (guard, mut events) = room.subscribe_to_typing_notifications();
    Box::pin(async move {
        let _guard = guard;
        let mut expires = None;
        loop {
            let users = tokio::select! {
                event = events.recv() => match event {
                    Ok(users) => users,
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                },
                _ = deadline(expires) => Vec::new(),
            };
            // Coalesce queued complete snapshots before resolving cached names.
            let mut users = users;
            while let Ok(latest) = events.try_recv() {
                users = latest;
            }
            users.sort();
            users.dedup();
            expires = (!users.is_empty()).then(|| tokio::time::Instant::now() + ttl);
            let mut names = Vec::new();
            // Bound store work even in large public rooms.
            for user in users.iter().take(3) {
                let name = room
                    .get_member_no_sync(user)
                    .await
                    .ok()
                    .flatten()
                    .map(|member| member.name().to_owned())
                    .unwrap_or_else(|| user.to_string());
                names.push(name);
            }
            if users.len() > 3 {
                names.push(format!("{} others", users.len() - 3));
            }
            let typing = Some(Typing { generation, names });
            state.send_if_modified(|s| {
                if s.typing == typing {
                    false
                } else {
                    s.typing = typing;
                    true
                }
            });
        }
    })
}

async fn deadline(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

async fn notice(room: &Room, typing: bool) {
    // Ephemeral best effort; never delay navigation or message queue admission.
    let _ = tokio::time::timeout(REQUEST_TIMEOUT, room.typing_notice(typing)).await;
}

fn desired(selection: &Selection) -> Option<(&str, &Activity)> {
    let (generation, room) = selection.room.as_ref()?;
    let activity = selection.typing.as_ref()?;
    (!selection.shutdown
        && selection.privacy.send_typing
        && activity.generation == *generation
        && activity.at.elapsed() < IDLE)
        .then_some((room.as_str(), activity))
}

pub(super) async fn outgoing(
    client: Client,
    mut selection: watch::Receiver<Selection>,
    stop: CancellationToken,
) {
    let mut active: Option<Room> = None;
    let mut seen = None;
    let mut expires = None;
    'worker: loop {
        if stop.is_cancelled() {
            break;
        }
        let next = selection.borrow_and_update().clone();
        let wanted = desired(&next);
        if active
            .as_ref()
            .is_some_and(|room| wanted.is_none_or(|(id, _)| room.room_id().as_str() != id))
        {
            notice(&active.take().unwrap(), false).await;
            continue;
        }
        if let Some((id, activity)) = wanted {
            if seen.as_ref() != Some(activity) {
                seen = Some(activity.clone());
                expires = Some(tokio::time::Instant::from_std(activity.at + IDLE));
                if let Ok(id) = id.parse::<matrix_sdk::ruma::OwnedRoomId>()
                    && let Some(room) = client.get_room(&id)
                {
                    active = Some(room.clone());
                    let sending = notice(&room, true);
                    tokio::pin!(sending);
                    loop {
                        tokio::select! {
                            biased;
                            _ = stop.cancelled() => break 'worker,
                            changed = selection.changed() => {
                                if changed.is_err() {
                                    break 'worker;
                                }
                                if desired(&selection.borrow_and_update())
                                        .is_none_or(|(wanted, _)| wanted != room.room_id().as_str())
                                {
                                    break;
                                }
                            }
                            _ = &mut sending => break,
                        }
                    }
                    // A consumed policy/navigation update must be reconciled immediately.
                    continue;
                }
            }
        } else {
            seen = None;
            expires = None;
        }
        if next.shutdown || stop.is_cancelled() {
            break;
        }
        tokio::select! {
            _ = stop.cancelled() => break,
            changed = selection.changed() => if changed.is_err() { break; },
            _ = deadline(expires) => {},
        }
    }
    if let Some(room) = active {
        notice(&room, false).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    async fn fixture() -> (
        MockServer,
        Client,
        std::sync::Arc<std::sync::Mutex<serde_json::Value>>,
    ) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let body = std::sync::Arc::new(std::sync::Mutex::new(json!({
            "next_batch":"one", "rooms":{"join":{"!room:example.org":{
                "state":{"events":[
                    {"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":1,"content":{"membership":"join"}},
                    {"type":"m.room.member","state_key":"@other:example.org","sender":"@other:example.org","event_id":"$other","origin_server_ts":1,"content":{"membership":"join","displayname":"Other"}}
                ]}, "timeline":{"events":[],"limited":false}
            }}}
        })));
        let response = body.clone();
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(response.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(".*/typing/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(json!({
                    "user_id":"@test:example.org","device_id":"TEST","access_token":"fake"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        (server, client, body)
    }

    async fn requests(server: &MockServer, count: usize) -> Vec<serde_json::Value> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let requests: Vec<_> = server
                    .received_requests()
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|r| r.method.as_str() == "PUT" && r.url.path().contains("/typing/"))
                    .map(|r| serde_json::from_slice(&r.body).unwrap())
                    .collect();
                if requests.len() >= count {
                    return requests;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn disabling_during_inflight_notice_stops_promptly_without_idle_deadline() {
        let (server, client, _) = fixture().await;
        Mock::given(method("PUT"))
            .and(path_regex(".*/typing/.*"))
            .and(wiremock::matchers::body_partial_json(
                json!({"typing":true}),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({}))
                    .set_delay(Duration::from_secs(1)),
            )
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        let (control, selection) = watch::channel(Selection {
            room: Some((1, "!room:example.org".into())),
            typing: Some(Activity {
                generation: 1,
                at: Instant::now(),
            }),
            ..Default::default()
        });
        let stop = CancellationToken::new();
        let worker = tokio::spawn(outgoing(client, selection, stop.clone()));
        requests(&server, 1).await;
        control.send_modify(|s| s.privacy.send_typing = false);
        let sent = tokio::time::timeout(Duration::from_millis(500), requests(&server, 2))
            .await
            .unwrap();
        assert_eq!(sent[1], json!({"typing":false}));
        stop.cancel();
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn disabling_typing_stops_active_notice_and_suppresses_fresh_edits() {
        let (server, client, _) = fixture().await;
        let (control, selection) = watch::channel(Selection {
            room: Some((1, "!room:example.org".into())),
            typing: Some(Activity {
                generation: 1,
                at: Instant::now(),
            }),
            ..Default::default()
        });
        let stop = CancellationToken::new();
        let worker = tokio::spawn(outgoing(client, selection, stop.clone()));
        requests(&server, 1).await;
        control.send_modify(|s| s.privacy.send_typing = false);
        assert_eq!(requests(&server, 2).await[1], json!({"typing":false}));
        control.send_modify(|s| s.typing.as_mut().unwrap().at = Instant::now());
        tokio::time::sleep(Duration::from_millis(50)).await;
        stop.cancel();
        worker.await.unwrap();
        assert_eq!(requests(&server, 2).await.len(), 2);
    }

    #[tokio::test]
    async fn outgoing_throttles_stops_on_navigation_and_cleans_up_on_shutdown() {
        let (server, client, _) = fixture().await;
        let (control, selection) = watch::channel(Selection {
            room: Some((1, "!room:example.org".into())),
            ..Default::default()
        });
        let stop = CancellationToken::new();
        let worker = tokio::spawn(outgoing(client, selection, stop.clone()));
        control.send_modify(|s| {
            s.typing = Some(Activity {
                generation: 1,
                at: Instant::now(),
            })
        });
        assert_eq!(
            requests(&server, 1).await[0],
            json!({"typing":true,"timeout":4000})
        );
        control.send_modify(|s| {
            s.typing = Some(Activity {
                generation: 1,
                at: Instant::now(),
            })
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(requests(&server, 1).await.len(), 1);
        // Navigation invalidates activity even if the UI's stop command is coalesced.
        control.send_modify(|s| s.room = Some((2, "!elsewhere:example.org".into())));
        assert_eq!(requests(&server, 2).await[1], json!({"typing":false}));
        control.send_modify(|s| {
            s.room = Some((3, "!room:example.org".into()));
            s.typing = Some(Activity {
                generation: 3,
                at: Instant::now(),
            });
        });
        requests(&server, 3).await;
        stop.cancel();
        worker.await.unwrap();
        let sent = requests(&server, 4).await;
        assert_eq!(sent.len(), 4);
        assert_eq!(sent[3], json!({"typing":false}));
    }

    #[tokio::test]
    async fn outgoing_idle_expiry_and_stale_edits_do_not_restart_activity() {
        let (server, client, _) = fixture().await;
        let (control, selection) = watch::channel(Selection {
            room: Some((1, "!room:example.org".into())),
            typing: Some(Activity {
                generation: 1,
                at: Instant::now() - IDLE + Duration::from_millis(300),
            }),
            ..Default::default()
        });
        let stop = CancellationToken::new();
        let worker = tokio::spawn(outgoing(client, selection, stop.clone()));
        requests(&server, 1).await;
        assert_eq!(requests(&server, 2).await[1], json!({"typing":false}));
        control.send_modify(|s| s.conversation_visible = true);
        stop.cancel();
        worker.await.unwrap();
        assert_eq!(requests(&server, 2).await.len(), 2);
    }

    #[tokio::test]
    async fn incoming_filters_self_resolves_cached_names_replaces_and_cancels() {
        let (_server, client, body) = fixture().await;
        let (state, mut snapshots) = watch::channel(Model::default());
        let room = client
            .get_room(matrix_sdk::ruma::room_id!("!room:example.org"))
            .unwrap();
        let mut incoming = incoming_room(room, 7, state, Duration::from_millis(150));
        // Deliver the first sync before polling the view, as subscription setup can do.
        for (index, (users, expected)) in [
            (
                json!([
                    "@test:example.org",
                    "@other:example.org",
                    "@other:example.org"
                ]),
                vec!["Other"],
            ),
            (
                json!(["@unknown:example.org"]),
                vec!["@unknown:example.org"],
            ),
            (json!([]), vec![]),
        ]
        .into_iter()
        .enumerate()
        {
            *body.lock().unwrap() = json!({"next_batch":format!("next-{index}"), "rooms":{"join":{"!room:example.org":{
                "ephemeral":{"events":[{"type":"m.typing","content":{"user_ids":users}}]},
                "timeline":{"events":[],"limited":false}
            }}}});
            client
                .sync_once(SyncSettings::default().timeout(Duration::ZERO))
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    tokio::select! {
                        _ = &mut incoming => panic!("worker ended"),
                        changed = snapshots.changed() => {
                            changed.unwrap();
                            if snapshots.borrow().typing.as_ref().is_some_and(|typing| typing.generation == 7 && typing.names == expected) { break; }
                        }
                    }
                }
            }).await.unwrap();
        }
        // A fresh nonempty snapshot must expire even if synchronization stalls.
        *body.lock().unwrap() = json!({"next_batch":"expiry", "rooms":{"join":{"!room:example.org":{
            "ephemeral":{"events":[{"type":"m.typing","content":{"user_ids":["@other:example.org"]}}]},
            "timeline":{"events":[],"limited":false}
        }}}});
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut saw_typing = false;
            loop {
                tokio::select! {
                    _ = &mut incoming => panic!("worker ended"),
                    changed = snapshots.changed() => {
                        changed.unwrap();
                        let empty = snapshots.borrow().typing.as_ref().unwrap().names.is_empty();
                        if !empty { saw_typing = true; }
                        if empty && saw_typing { break; }
                    }
                }
            }
        })
        .await
        .unwrap();
        // Dropping the owned future releases both event handler and state sender.
        drop(incoming);
        assert!(snapshots.changed().await.is_err());
    }

    #[test]
    fn desired_rejects_old_generations_expired_activity_and_shutdown() {
        let mut selection = Selection {
            room: Some((1, "!room:example.org".into())),
            typing: Some(Activity {
                generation: 1,
                at: Instant::now(),
            }),
            ..Default::default()
        };
        assert!(desired(&selection).is_some());
        selection.typing.as_mut().unwrap().generation = 0;
        assert!(desired(&selection).is_none());
        selection.typing.as_mut().unwrap().generation = 1;
        selection.typing.as_mut().unwrap().at -= IDLE;
        assert!(desired(&selection).is_none());
        selection.typing.as_mut().unwrap().at = Instant::now();
        selection.shutdown = true;
        assert!(desired(&selection).is_none());
    }
}
