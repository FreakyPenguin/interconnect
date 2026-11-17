//! Read-only image-pack discovery with a narrow SDK state subscription.
use iced::futures::StreamExt;
use matrix_sdk::Client;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

pub const MAX_EMOJI: usize = 1024;
const ROOM_TYPES: [&str; 2] = ["m.room.image_pack", "im.ponies.room_emotes"];
const GLOBAL_TYPES: [&str; 2] = ["m.image_pack.rooms", "im.ponies.emote_rooms"];
pub mod management;

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Emoji {
    pub id: String,
    pub shortcode: String,
    pub name: String,
    pub pack: String,
    pub uri: matrix_sdk::ruma::OwnedMxcUri,
}
// Never log pack names, shortcodes or media identifiers.
impl std::fmt::Debug for Emoji {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CustomEmoji")
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub room: String,
    pub sources: Vec<matrix_sdk::ruma::OwnedRoomId>,
    pub entries: Vec<Emoji>,
}
/// Explicit composer choices remain bound to their original image in that draft.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Selection {
    pub token: String,
    pub emoji: Emoji,
}
pub fn shortcode_valid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn allows_emoticons(value: &Value) -> bool {
    value
        .as_array()
        .is_none_or(|usage| usage.is_empty() || usage.iter().any(|v| v == "emoticon"))
}
fn append_pack(out: &mut Vec<Emoji>, content: &Value, identity: &str, fallback: &str) {
    let Some(images) = content["images"].as_object() else {
        return;
    };
    let pack = content["pack"]["display_name"].as_str().unwrap_or(fallback);
    for (shortcode, image) in images {
        if out.len() >= MAX_EMOJI {
            break;
        }
        // Older implementations allow a per-image usage override.
        let usage = image.get("usage").unwrap_or(&content["pack"]["usage"]);
        if !shortcode_valid(shortcode) || !allows_emoticons(usage) {
            continue;
        }
        let Some(url) = image["url"].as_str() else {
            continue;
        };
        let uri = matrix_sdk::ruma::OwnedMxcUri::from(url);
        if !uri.is_valid() {
            continue;
        }
        out.push(Emoji {
            id: serde_json::to_string(&(identity, shortcode)).expect("string tuple"),
            shortcode: shortcode.clone(),
            name: image["body"]
                .as_str()
                .unwrap_or(shortcode)
                .chars()
                .take(256)
                .collect(),
            pack: pack.chars().take(128).collect(),
            uri,
        });
    }
}
async fn account(client: &Client, kind: &str) -> anyhow::Result<Option<Value>> {
    Ok(client
        .account()
        .account_data_raw(kind.into())
        .await?
        .and_then(|raw| serde_json::from_str(raw.json().get()).ok()))
}
async fn packs(room: &matrix_sdk::Room) -> anyhow::Result<BTreeMap<String, Value>> {
    let mut packs = BTreeMap::new();
    // Stable state wins even when empty (a removed pack must not reappear).
    for kind in ROOM_TYPES.into_iter().rev() {
        for raw in room.get_state_events(kind.into()).await? {
            let event: Value = serde_json::from_str(match &raw {
                matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Sync(raw) => {
                    raw.json().get()
                }
                matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Stripped(raw) => {
                    raw.json().get()
                }
            })?;
            if let Some(key) = event["state_key"].as_str() {
                packs.insert(key.to_owned(), event["content"].clone());
            }
        }
    }
    Ok(packs)
}
pub async fn snapshot(client: &Client, room_id: &str) -> anyhow::Result<Snapshot> {
    let mut out = Snapshot {
        room: room_id.into(),
        sources: Vec::new(),
        entries: Vec::new(),
    };
    // Personal packs are a widely used legacy extension to MSC2545.
    if let Some(content) = account(client, "im.ponies.user_emotes").await? {
        append_pack(&mut out.entries, &content, "user", "Personal emoji");
    }
    let mut enabled = None;
    for kind in GLOBAL_TYPES {
        if let Some(content) = account(client, kind).await? {
            enabled = Some(content);
            break;
        }
    }
    let mut seen = HashSet::new();
    if let Some(rooms) = enabled.as_ref().and_then(|v| v["rooms"].as_object()) {
        for (id, keys) in rooms.iter().take(64) {
            let Some(room) = id
                .parse::<matrix_sdk::ruma::OwnedRoomId>()
                .ok()
                .and_then(|id| client.get_room(&id))
            else {
                continue;
            };
            if room.state() != matrix_sdk::RoomState::Joined {
                continue;
            }
            let Some(keys) = keys.as_object() else {
                continue;
            };
            if keys.is_empty() {
                continue;
            }
            out.sources.push(room.room_id().to_owned());
            let available = packs(&room).await?;
            for key in keys.keys().take(64) {
                if let Some(content) = available.get(key) {
                    let identity = serde_json::to_string(&(id, key))?;
                    seen.insert(identity.clone());
                    append_pack(
                        &mut out.entries,
                        content,
                        &identity,
                        &room.name().unwrap_or_else(|| "Emoji pack".into()),
                    );
                }
            }
        }
    }
    // Only canonical parents are inherited; cycles and work are bounded.
    let mut next = room_id.parse::<matrix_sdk::ruma::OwnedRoomId>().ok();
    let mut visited = HashSet::new();
    for depth in 0..8 {
        let Some(id) = next.take() else { break };
        if !visited.insert(id.clone()) {
            break;
        }
        let Some(room) = client.get_room(&id) else {
            break;
        };
        if room.state() != matrix_sdk::RoomState::Joined || (depth > 0 && !room.is_space()) {
            break;
        }
        out.sources.push(id.clone());
        for (key, content) in packs(&room).await?.iter().take(64) {
            let identity = serde_json::to_string(&(id.as_str(), key))?;
            if seen.insert(identity.clone()) {
                append_pack(
                    &mut out.entries,
                    content,
                    &identity,
                    &room.name().unwrap_or_else(|| "Emoji pack".into()),
                );
            }
        }
        for raw in room.get_state_events("m.space.parent".into()).await? {
            let event: Value = serde_json::from_str(match &raw {
                matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Sync(raw) => {
                    raw.json().get()
                }
                matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Stripped(raw) => {
                    raw.json().get()
                }
            })?;
            if event["content"]["canonical"] == true
                && event["content"]["via"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
            {
                next = event["state_key"].as_str().and_then(|id| id.parse().ok());
                break;
            }
        }
    }
    out.sources.sort();
    out.sources.dedup();
    Ok(out)
}

/// Scoped to the connection. Navigation cancels in-progress cache reads.
pub(crate) async fn run(
    client: Client,
    mut selection: tokio::sync::watch::Receiver<crate::connected::Selection>,
    state: tokio::sync::watch::Sender<crate::connected::Model>,
) {
    let (changed, mut accounts) = tokio::sync::mpsc::channel(1);
    let handler = client.add_event_handler(
        move |event: matrix_sdk::ruma::events::AnyGlobalAccountDataEvent| {
            let changed = changed.clone();
            async move {
                if GLOBAL_TYPES
                    .iter()
                    .chain(["im.ponies.user_emotes"].iter())
                    .any(|kind| event.event_type() == (*kind).into())
                {
                    let _ = changed.try_send(());
                }
            }
        },
    );
    let _guard = client.event_handler_drop_guard(handler);
    let mut updates = client.subscribe_to_all_room_updates();
    // RoomListService does not expose custom required_state. A separate SDK
    // connection requests only pack metadata, without timelines or E2EE work.
    let sync = match client.sliding_sync("emoji-packs") {
        Ok(builder) => builder.build().await.ok(),
        Err(_) => None,
    };
    let Some(sync) = sync else {
        std::future::pending::<()>().await;
        return;
    };
    let mut stream = Box::pin(sync.sync());
    let mut sync_enabled = true;
    let mut subscriptions = Vec::new();

    loop {
        let room = selection.borrow_and_update().room.clone();
        if let Some((_, id)) = &room {
            let result = tokio::select! {
                result = snapshot(&client, id) => result,
                changed = selection.changed() => { if changed.is_err() { return; } continue; }
            };
            if let Ok(snapshot) = result {
                if subscriptions != snapshot.sources {
                    subscriptions = snapshot.sources.clone();
                    let ids: Vec<_> = subscriptions.iter().map(|id| id.as_ref()).collect();
                    let mut settings = matrix_sdk::ruma::api::client::sync::sync_events::v5::request::RoomSubscription::default();
                    settings.required_state = ROOM_TYPES
                        .iter()
                        .chain(["m.space.parent"].iter())
                        .map(|kind| ((*kind).into(), "*".into()))
                        .collect();
                    settings.timeline_limit = matrix_sdk::ruma::uint!(0);
                    sync.set_room_subscriptions(&ids, Some(settings), true);
                }

                state.send_if_modified(|s| {
                    if *s.custom_emoji == snapshot {
                        false
                    } else {
                        s.custom_emoji = Arc::new(snapshot);
                        true
                    }
                });
            }
        } else {
            sync.set_room_subscriptions(&[], None, true);
            subscriptions.clear();
            state.send_if_modified(|s| {
                if s.custom_emoji.room.is_empty() {
                    false
                } else {
                    s.custom_emoji = Arc::default();
                    true
                }
            });
        }
        loop {
            tokio::select! {
                changed = selection.changed() => {
                    if changed.is_err() { return; }
                    if selection.borrow_and_update().room != room { sync_enabled = true; stream = Box::pin(sync.sync()); break; }
                }
                event = updates.recv() => {
                    if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { return; }
                    while updates.try_recv().is_ok() {}
                    if !sync_enabled { sync_enabled = true; stream = Box::pin(sync.sync()); }
                    break;
                }
                _ = accounts.recv() => { if !sync_enabled { sync_enabled = true; stream = Box::pin(sync.sync()); } break; }
                result = stream.next(), if sync_enabled && !subscriptions.is_empty() => {
                    if !matches!(result, Some(Ok(_))) { sync_enabled = false; }
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn sdk_pack_subscription_loads_live_state_and_cancels_without_fetching_media() {
        use matrix_sdk::{
            authentication::matrix::MatrixSession,
            config::{RequestConfig, SyncSettings},
        };
        use std::time::Duration;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"],"unstable_features":{"org.matrix.simplified_msc3575":true}}))).mount(&server).await;
        let event = |kind: &str, key: &str, content: Value| json!({"type":kind,"state_key":key,"content":content,"event_id":format!("${kind}-{key}"),"sender":"@test:example.org","origin_server_ts":1});
        let legacy = json!({"images":{"wave":{"url":"mxc://example.org/legacy"}}});
        let stable = json!({"images":{"wave":{"url":"mxc://example.org/stable"}}});
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one", "rooms":{"join":{"!room:example.org":{"state":{"events":[
                event("m.room.create", "", json!({"room_version":"11","creator":"@test:example.org"})),
                event("m.room.member", "@test:example.org", json!({"membership":"join"})),
                event("im.ponies.room_emotes", "cats", legacy.clone()),
                event("im.ponies.room_emotes", "removed", legacy),
                event("m.room.image_pack", "removed", json!({}))
            ]},"timeline":{"events":[],"limited":false}}}}}))).mount(&server).await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .request_config(RequestConfig::default().retry_limit(0))
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(
                    json!({"user_id":"@test:example.org","device_id":"TEST","access_token":"fake"}),
                )
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let first = snapshot(&client, "!room:example.org").await.unwrap();
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.entries[0].uri.as_str(), "mxc://example.org/legacy");
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = counter.clone();
        Mock::given(method("POST")).and(path("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"))
            .respond_with(move |request: &wiremock::Request| {
                let body: Value = request.body_json().unwrap();
                assert_eq!(body["conn_id"], "emoji-packs");
                let sub = &body["room_subscriptions"]["!room:example.org"];
                // Sticky parameters may be omitted after acknowledgement.
                if !sub.is_null() {
                    assert_eq!(sub["timeline_limit"], 0);
                    assert!(sub["required_state"].as_array().unwrap().contains(&json!(["m.room.image_pack","*"])));
                }
                assert!(request.url.query_pairs().any(|(key, value)| key == "timeout" && value != "0"));
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                ResponseTemplate::new(200).set_delay(Duration::from_millis(30)).set_body_json(json!({"pos":"two","rooms":{"!room:example.org":{"initial":true,"required_state":[event("m.room.image_pack", "cats", stable.clone())],"timeline":[]}}}))
            }).mount(&server).await;
        let (control, selection) = tokio::sync::watch::channel(crate::connected::Selection {
            room: Some((1, "!room:example.org".into())),
            ..Default::default()
        });
        let (state, mut snapshots) =
            tokio::sync::watch::channel(crate::connected::Model::default());
        let task = tokio::spawn(run(client, selection, state));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                snapshots.changed().await.unwrap();
                let value = snapshots.borrow_and_update().custom_emoji.clone();
                if value
                    .entries
                    .iter()
                    .any(|emoji| emoji.uri.as_str() == "mxc://example.org/stable")
                {
                    assert_eq!(value.entries.len(), 1);
                    break;
                }
            }
        })
        .await
        .unwrap();
        drop(control);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        let count = counter.load(std::sync::atomic::Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), count);
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| !request.url.path().contains("/media/"))
        );
    }
    #[test]
    fn validates_packs_and_usage_without_fetching_images() {
        let mut out = Vec::new();
        append_pack(
            &mut out,
            &json!({"pack":{"display_name":"Cats", "usage":["sticker"]},"images":{
                "cat":{"url":"mxc://example.org/cat"},
                "wave":{"url":"mxc://example.org/wave","usage":["emoticon"],"body":"Waving cat"},
                "bad:name":{"url":"mxc://example.org/bad","usage":["emoticon"]},
                "remote":{"url":"https://example.org/cat","usage":["emoticon"]}
            }}),
            "room-pack",
            "Fallback",
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].shortcode, "wave");
        assert_eq!(out[0].pack, "Cats");
        assert_eq!(out[0].name, "Waving cat");
        assert!(shortcode_valid("_wave-2_"));
        assert!(!shortcode_valid(&"a".repeat(101)));
    }
}
