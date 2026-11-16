//! Explicit room-section moves. Read server tags before a single m.tag replacement.
use matrix_sdk::{
    Client, RoomState,
    ruma::{
        api::{client::config::get_room_account_data, error::ErrorKind},
        events::RoomAccountDataEventType,
        serde::Raw,
    },
};
use serde_json::{Value, json};

pub fn is_section_tag(tag: &str) -> bool {
    matches!(tag, "m.favourite" | "m.lowpriority") || tag.starts_with("element.io.section.")
}

/// Preserve unrelated tag metadata and an existing destination's ordering metadata.
fn move_content(mut content: Value, destination: &str) -> Result<Value, String> {
    let tags = content
        .get_mut("tags")
        .and_then(Value::as_object_mut)
        .ok_or("The server returned invalid room tags. No changes were made.")?;
    let info = tags.get(destination).cloned().unwrap_or_else(|| json!({}));
    tags.retain(|tag, _| !is_section_tag(tag));
    if destination != "chats" {
        tags.insert(destination.into(), info);
    }
    Ok(content)
}

pub(crate) async fn move_room(client: &Client, id: &str, destination: &str) -> Result<(), String> {
    let id = id
        .parse::<matrix_sdk::ruma::OwnedRoomId>()
        .map_err(|_| "Invalid room ID.")?;
    let room = client
        .get_room(&id)
        .filter(|room| room.state() == RoomState::Joined && !room.is_space())
        .ok_or("This room is no longer joined.")?;
    if destination != "chats" && !is_section_tag(destination) {
        return Err("This section is not a move destination.".into());
    }
    if destination.starts_with("element.io.section.") {
        let raw = client
            .account()
            .fetch_account_data(crate::navigation::ELEMENT_SETTINGS.into())
            .await
            .map_err(|_| "Could not check the current section definitions.")?;
        let settings = raw
            .and_then(|raw| serde_json::from_str::<Value>(raw.json().get()).ok())
            .unwrap_or_default();
        if !crate::navigation::sections_from_settings(&settings)
            .iter()
            .any(|section| section.id == destination)
        {
            return Err(
                "This section no longer exists. Reopen the room menu to refresh the choices."
                    .into(),
            );
        }
    }
    let user = client.user_id().ok_or("Sign in to move rooms.")?;
    let request =
        get_room_account_data::v3::Request::new(user.to_owned(), id, RoomAccountDataEventType::Tag);
    let content = match client.send(request).await {
        Ok(response) => serde_json::from_str(response.account_data.json().get())
            .map_err(|_| "The server returned invalid room tags. No changes were made.")?,
        Err(error) if error.client_api_error_kind() == Some(&ErrorKind::NotFound) => {
            json!({"tags":{}})
        }
        Err(_) => return Err("Could not load room tags. No changes were made.".into()),
    };
    let replacement = move_content(content.clone(), destination)?;
    if replacement == content {
        return Ok(());
    }
    if room.state() != RoomState::Joined {
        return Err("This room is no longer joined.".into());
    }
    room.set_account_data_raw(
        RoomAccountDataEventType::Tag,
        Raw::new(&replacement)
            .map_err(|_| "Could not prepare room tags.")?
            .cast_unchecked(),
    )
    .await
    .map_err(|_| "Could not move the room. Check its section before retrying.".to_owned())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, config::SyncSettings};
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[tokio::test]
    async fn moves_replace_all_section_tags_preserve_metadata_and_use_server_state_before_sync() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(json!({
                    "user_id":"@test:example.org", "device_id":"TEST", "access_token":"fake"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"next_batch":"one", "rooms":{"join":{
                "!room:example.org":{"state":{"events":[
                    {"type":"m.room.create","state_key":"","sender":"@test:example.org","event_id":"$create","origin_server_ts":1,"content":{"creator":"@test:example.org"}},
                    {"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":1,"content":{"membership":"join"}}
                ]},"timeline":{"events":[]}}
            }}}))).mount(&server).await;
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .unwrap();
        let tags = Arc::new(Mutex::new(json!({"tags":{
            "m.favourite":{"order":0.1}, "m.lowpriority":{}, "element.io.section.orphan":{},
            "u.work":{"order":0.75,"extension":{"preserve":true}}
        },"extension":"keep"})));
        let settings = Arc::new(Mutex::new(json!({"RoomList.CustomSectionData":{
            "element.io.section.target":{"tag":"element.io.section.target","name":"Projects"}
        }})));
        let data_path =
            "/_matrix/client/v3/user/@test:example.org/rooms/!room:example.org/account_data/m.tag";
        let read_status = Arc::new(std::sync::atomic::AtomicU16::new(200));
        let write_status = Arc::new(std::sync::atomic::AtomicU16::new(200));
        let status = read_status.clone();
        let current = tags.clone();
        Mock::given(method("GET"))
            .and(path(data_path))
            .respond_with(move |_: &wiremock::Request| {
                let code = status.load(std::sync::atomic::Ordering::SeqCst);
                ResponseTemplate::new(code).set_body_json(if code == 200 { current.lock().unwrap().clone() }
                    else { json!({"errcode":if code == 404 {"M_NOT_FOUND"} else {"M_FORBIDDEN"},"error":"fixture"}) })
            })
            .mount(&server)
            .await;
        let status = write_status.clone();
        let current = tags.clone();
        Mock::given(method("PUT"))
            .and(path(data_path))
            .respond_with(move |req: &wiremock::Request| {
                let code = status.load(std::sync::atomic::Ordering::SeqCst);
                if code == 200 {
                    *current.lock().unwrap() = req.body_json::<Value>().unwrap();
                }
                ResponseTemplate::new(code).set_body_json(if code == 200 {
                    json!({})
                } else {
                    json!({"errcode":"M_FORBIDDEN", "error":"fixture"})
                })
            })
            .mount(&server)
            .await;
        let current = settings.clone();
        Mock::given(method("GET"))
            .and(path(
                "/_matrix/client/v3/user/@test:example.org/account_data/im.vector.web.settings",
            ))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(current.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        let directory = crate::people::Directory::new(client.clone());
        for destination in [
            "element.io.section.target",
            "m.lowpriority",
            "m.favourite",
            "chats",
        ] {
            directory
                .clone()
                .move_room_to_section("!room:example.org".into(), destination.into())
                .await
                .unwrap();
            let content = tags.lock().unwrap().clone();
            assert_eq!(content["extension"], "keep");
            assert_eq!(
                content["tags"]["u.work"],
                json!({"order":0.75,"extension":{"preserve":true}})
            );
            let sections: Vec<_> = content["tags"]
                .as_object()
                .unwrap()
                .keys()
                .filter(|key| is_section_tag(key))
                .cloned()
                .collect();
            assert_eq!(
                sections,
                if destination == "chats" {
                    vec![]
                } else {
                    vec![destination.to_owned()]
                }
            );
        }
        // A second Default selection is a no-op; it must not toggle membership.
        directory
            .clone()
            .move_room_to_section("!room:example.org".into(), "chats".into())
            .await
            .unwrap();
        // Deleted destinations and inferred sections must never write.
        *settings.lock().unwrap() = json!({});
        assert!(
            directory
                .clone()
                .move_room_to_section(
                    "!room:example.org".into(),
                    "element.io.section.target".into()
                )
                .await
                .unwrap_err()
                .contains("no longer exists")
        );
        assert!(
            directory
                .clone()
                .move_room_to_section("!room:example.org".into(), "Unread".into())
                .await
                .is_err()
        );
        assert!(
            directory
                .clone()
                .move_room_to_section("!left:example.org".into(), "m.favourite".into())
                .await
                .is_err()
        );
        // Malformed data aborts instead of overwriting unknown content.
        *tags.lock().unwrap() = json!({"tags":[]});
        assert!(
            directory
                .clone()
                .move_room_to_section("!room:example.org".into(), "m.favourite".into())
                .await
                .unwrap_err()
                .contains("invalid room tags")
        );
        read_status.store(403, std::sync::atomic::Ordering::SeqCst);
        assert!(
            directory
                .clone()
                .move_room_to_section("!room:example.org".into(), "m.favourite".into())
                .await
                .unwrap_err()
                .contains("Could not load")
        );
        let writes: Vec<_> = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|req| req.method == "PUT")
            .collect();
        assert_eq!(writes.len(), 4);
        assert!(writes.iter().all(|req| req.url.path() == data_path));
        // Missing tag data is an empty set; write failure is surfaced.
        read_status.store(404, std::sync::atomic::Ordering::SeqCst);
        write_status.store(403, std::sync::atomic::Ordering::SeqCst);
        let error = directory
            .move_room_to_section("!room:example.org".into(), "m.favourite".into())
            .await
            .unwrap_err();
        assert!(error.contains("Could not move"), "{error}");
    }

    #[test]
    fn reselecting_destination_preserves_its_order_and_unknown_tag_fields() {
        let content = json!({"tags":{"m.favourite":{"order":0.2,"extra":true},"element.io.section.old":{},"u.keep":{"order":0.3}}});
        assert_eq!(
            move_content(content, "m.favourite").unwrap(),
            json!({"tags":{"m.favourite":{"order":0.2,"extra":true},"u.keep":{"order":0.3}}})
        );
    }
}
