//! Element-compatible section settings and explicit room membership changes.
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

/// A stable tag is allocated when the create form opens, so retries reuse its ID.
#[derive(Clone, Debug)]
pub enum Change {
    Reorder {
        tag: String,
        target: String,
        before: bool,
    },
    Create {
        tag: String,
        name: String,
        space_id: String,
    },
    Rename {
        tag: String,
        name: String,
    },
    Remove {
        tag: String,
    },
}
impl Change {
    pub fn tag(&self) -> &str {
        match self {
            Self::Create { tag, .. }
            | Self::Rename { tag, .. }
            | Self::Remove { tag }
            | Self::Reorder { tag, .. } => tag,
        }
    }
}
pub fn new_section_tag() -> String {
    format!(
        "element.io.section.{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        rand::random::<u32>(),
        rand::random::<u16>(),
        (rand::random::<u16>() & 0x0fff) | 0x4000,
        (rand::random::<u16>() & 0x3fff) | 0x8000,
        rand::random::<u64>() & 0xffffffffffff
    )
}

/// The two inferred chat groups share Element's single Chats order slot.
pub fn reorder_tag(id: &str) -> Option<&str> {
    match id {
        "Rooms" | "Unread" | "chats" => Some("chats"),
        "Direct messages" | "im.vector.fake.direct" => Some("im.vector.fake.direct"),
        tag if tag.starts_with("element.io.section.") => Some(tag),
        _ => None,
    }
}
pub fn section_order(sections: &[crate::navigation::SectionEntry]) -> Vec<String> {
    let mut order = Vec::new();
    for tag in sections
        .iter()
        .filter_map(|section| reorder_tag(&section.id))
    {
        if !order.iter().any(|id| id == tag) {
            order.push(tag.to_owned());
        }
    }
    order
}
fn reorder_settings(
    mut settings: Value,
    source: &str,
    target: &str,
    before: bool,
) -> Result<Value, String> {
    let root = settings
        .as_object()
        .ok_or("The server returned invalid settings. No changes were made.")?;
    if root
        .get("RoomList.CustomSectionData")
        .is_some_and(|value| !value.is_object())
        || root
            .get("RoomList.OrderedCustomSections")
            .is_some_and(|value| !value.is_array())
    {
        return Err("The server returned invalid section settings. No changes were made.".into());
    }
    let mut order = section_order(&crate::navigation::sections_from_settings(&settings));
    if reorder_tag(source) != Some(source)
        || reorder_tag(target) != Some(target)
        || !order.iter().any(|id| id == source)
        || !order.iter().any(|id| id == target)
    {
        return Err("A section is no longer available for reordering.".into());
    }
    if source == target {
        return Ok(settings);
    }
    order.retain(|id| id != source);
    let index = order.iter().position(|id| id == target).unwrap() + usize::from(!before);
    order.insert(index, source.to_owned());
    settings["RoomList.OrderedCustomSections"] = json!(order);
    Ok(settings)
}

/// Edit only the requested definition/order entries; preserve other clients' settings.
pub fn change_settings(mut settings: Value, change: &Change) -> Result<Value, String> {
    if let Change::Reorder {
        tag,
        target,
        before,
    } = change
    {
        return reorder_settings(settings, tag, target, *before);
    }
    let tag = change.tag();
    if !tag.starts_with("element.io.section.") {
        return Err("Built-in sections cannot be edited.".into());
    }
    let name = match change {
        Change::Create { name, .. } | Change::Rename { name, .. } => {
            let name = name.trim();
            if name.is_empty() {
                return Err("Enter a section name.".into());
            }
            Some(name)
        }
        Change::Remove { .. } => None,
        Change::Reorder { .. } => unreachable!(),
    };
    let root = settings
        .as_object_mut()
        .ok_or("The server returned invalid settings. No changes were made.")?;
    let definitions = root
        .entry("RoomList.CustomSectionData")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("The server returned invalid section definitions. No changes were made.")?;
    match change {
        Change::Create { space_id, .. } => {
            let entry = json!({"tag":tag,"name":name.unwrap(),"spaceId":space_id});
            if let Some(existing) = definitions.get(tag) {
                if existing["tag"] != tag
                    || existing["name"] != name.unwrap()
                    || existing["spaceId"] != *space_id
                {
                    return Err(
                        "This section already exists. Reopen section management to refresh it."
                            .into(),
                    );
                }
            } else {
                definitions.insert(tag.into(), entry);
            }
        }
        Change::Rename { .. } => {
            let entry = definitions
                .get_mut(tag)
                .and_then(Value::as_object_mut)
                .filter(|entry| entry.get("tag").and_then(Value::as_str) == Some(tag))
                .ok_or("This section no longer exists.")?;
            entry.insert("name".into(), json!(name.unwrap()));
        }
        Change::Remove { .. } => {
            definitions.remove(tag);
        }
        Change::Reorder { .. } => unreachable!(),
    }
    if !matches!(change, Change::Rename { .. }) {
        let order = root
            .entry("RoomList.OrderedCustomSections")
            .or_insert_with(|| json!(["im.vector.fake.direct", "chats"]))
            .as_array_mut()
            .ok_or("The server returned invalid section order. No changes were made.")?;
        match change {
            Change::Create { .. } => {
                if !order.iter().any(|value| value == "chats") {
                    order.push(json!("chats"));
                }
                if !order.iter().any(|value| value == "im.vector.fake.direct") {
                    order.insert(0, json!("im.vector.fake.direct"));
                }
                if !order.iter().any(|value| value == tag) {
                    let index = order.iter().position(|value| value == "chats").unwrap();
                    order.insert(index, json!(tag));
                }
            }
            Change::Remove { .. } => order.retain(|value| value != tag),
            _ => unreachable!(),
        }
    }
    Ok(settings)
}

pub(crate) async fn change_section(client: &Client, change: Change) -> Result<(), String> {
    let settings = client
        .account()
        .fetch_account_data(crate::navigation::ELEMENT_SETTINGS.into())
        .await
        .map_err(|_| "Could not load current section settings. No changes were made.")?
        .map(|raw| serde_json::from_str::<Value>(raw.json().get()))
        .transpose()
        .map_err(|_| "The server returned invalid settings. No changes were made.")?
        .unwrap_or_else(|| json!({}));
    let replacement = change_settings(settings.clone(), &change)?;
    if replacement != settings {
        client
            .account()
            .set_account_data_raw(
                crate::navigation::ELEMENT_SETTINGS.into(),
                Raw::new(&replacement)
                    .map_err(|_| "Could not prepare section settings.")?
                    .cast_unchecked(),
            )
            .await
            .map_err(|_| {
                "Could not save the section. Check section management before retrying.".to_owned()
            })?;
    }
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

    #[tokio::test]
    async fn section_management_reads_fresh_settings_and_atomically_preserves_other_data() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
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
        let settings = Arc::new(Mutex::new(json!({
            "unrelated":{"preserve":true},
            "RoomList.CustomSectionData":{"element.io.section.existing":{"tag":"element.io.section.existing","name":"Existing","extra":1}},
            "RoomList.OrderedCustomSections":["chats","element.io.section.existing"]
        })));
        let read_status = Arc::new(std::sync::atomic::AtomicU16::new(200));
        let write_status = Arc::new(std::sync::atomic::AtomicU16::new(200));
        let settings_path =
            "/_matrix/client/v3/user/@test:example.org/account_data/im.vector.web.settings";
        let data = settings.clone();
        let status = read_status.clone();
        Mock::given(method("GET")).and(path(settings_path)).respond_with(move |_: &wiremock::Request| {
            let code = status.load(std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(code).set_body_json(if code == 200 { data.lock().unwrap().clone() }
                else { json!({"errcode":if code == 404 {"M_NOT_FOUND"} else {"M_FORBIDDEN"},"error":"fixture"}) })
        }).mount(&server).await;
        let data = settings.clone();
        let status = write_status.clone();
        Mock::given(method("PUT"))
            .and(path(settings_path))
            .respond_with(move |req: &wiremock::Request| {
                let code = status.load(std::sync::atomic::Ordering::SeqCst);
                if code == 200 {
                    *data.lock().unwrap() = req.body_json().unwrap();
                }
                ResponseTemplate::new(code).set_body_json(if code == 200 {
                    json!({})
                } else {
                    json!({"errcode":"M_FORBIDDEN","error":"fixture"})
                })
            })
            .mount(&server)
            .await;
        let directory = crate::people::Directory::new(client);
        let tag = new_section_tag();
        let change = Change::Create {
            tag: tag.clone(),
            name: "  Projects  ".into(),
            space_id: "!space:example.org".into(),
        };
        directory
            .clone()
            .change_section(change.clone())
            .await
            .unwrap();
        {
            let data = settings.lock().unwrap();
            assert_eq!(
                data["RoomList.CustomSectionData"][&tag],
                json!({"tag":tag,"name":"Projects","spaceId":"!space:example.org"})
            );
            assert_eq!(
                data["RoomList.OrderedCustomSections"],
                json!([
                    "im.vector.fake.direct",
                    tag,
                    "chats",
                    "element.io.section.existing"
                ])
            );
        }
        // Retry uses the same ID and performs no duplicate creation/write.
        directory.clone().change_section(change).await.unwrap();
        settings.lock().unwrap()["new_remote_setting"] = json!(123);
        settings.lock().unwrap()["RoomList.CustomSectionData"][&tag]["extension"] =
            json!({"keep":true});
        directory
            .clone()
            .change_section(Change::Rename {
                tag: tag.clone(),
                name: "Renamed".into(),
            })
            .await
            .unwrap();
        {
            let data = settings.lock().unwrap();
            assert_eq!(
                data["RoomList.CustomSectionData"][&tag]["spaceId"],
                "!space:example.org"
            );
            assert_eq!(
                data["RoomList.CustomSectionData"][&tag]["extension"],
                json!({"keep":true})
            );
            assert_eq!(data["new_remote_setting"], 123);
            assert_eq!(data["unrelated"], json!({"preserve":true}));
        }
        let reorder = Change::Reorder {
            tag: tag.clone(),
            target: "chats".into(),
            before: false,
        };
        directory
            .clone()
            .change_section(reorder.clone())
            .await
            .unwrap();
        directory.clone().change_section(reorder).await.unwrap();
        assert_eq!(
            settings.lock().unwrap()["RoomList.OrderedCustomSections"],
            json!([
                "im.vector.fake.direct",
                "chats",
                tag,
                "element.io.section.existing"
            ])
        );
        assert_eq!(settings.lock().unwrap()["new_remote_setting"], 123);
        assert_eq!(
            settings.lock().unwrap()["RoomList.CustomSectionData"][&tag]["extension"],
            json!({"keep":true})
        );
        assert!(
            directory
                .clone()
                .change_section(Change::Reorder {
                    tag: tag.clone(),
                    target: "element.io.section.deleted".into(),
                    before: true
                })
                .await
                .unwrap_err()
                .contains("no longer available")
        );
        directory
            .clone()
            .change_section(Change::Remove { tag: tag.clone() })
            .await
            .unwrap();
        directory
            .clone()
            .change_section(Change::Remove { tag: tag.clone() })
            .await
            .unwrap();
        assert!(
            settings.lock().unwrap()["RoomList.CustomSectionData"]
                .get(&tag)
                .is_none()
        );
        assert_eq!(
            settings.lock().unwrap()["RoomList.OrderedCustomSections"],
            json!([
                "im.vector.fake.direct",
                "chats",
                "element.io.section.existing"
            ])
        );
        assert!(
            directory
                .clone()
                .change_section(Change::Rename {
                    tag: tag.clone(),
                    name: "Stale".into()
                })
                .await
                .unwrap_err()
                .contains("no longer exists")
        );
        *settings.lock().unwrap() = json!({"RoomList.CustomSectionData":[]});
        assert!(
            directory
                .clone()
                .change_section(Change::Create {
                    tag: tag.clone(),
                    name: "Bad".into(),
                    space_id: "home-space".into()
                })
                .await
                .unwrap_err()
                .contains("invalid section definitions")
        );
        read_status.store(403, std::sync::atomic::Ordering::SeqCst);
        assert!(
            directory
                .clone()
                .change_section(Change::Remove { tag: tag.clone() })
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
        assert!(writes.iter().all(|req| req.url.path() == settings_path));
        read_status.store(404, std::sync::atomic::Ordering::SeqCst);
        write_status.store(403, std::sync::atomic::Ordering::SeqCst);
        assert!(
            directory
                .change_section(Change::Create {
                    tag,
                    name: "Refused".into(),
                    space_id: "home-space".into()
                })
                .await
                .unwrap_err()
                .contains("Could not save")
        );
    }

    #[test]
    fn reorder_preserves_definitions_and_hidden_sections_and_rejects_fixed_or_stale_ids() {
        let original = json!({"other":{"keep":true},"RoomList.CustomSectionData":{
            "element.io.section.a":{"tag":"element.io.section.a","name":"A","spaceId":"!hidden:example.org","extra":7},
            "element.io.section.b":{"tag":"element.io.section.b","name":"B"}
        },"RoomList.OrderedCustomSections":["element.io.section.a","chats","element.io.section.b","im.vector.fake.direct"]});
        let change = Change::Reorder {
            tag: "element.io.section.a".into(),
            target: "element.io.section.b".into(),
            before: false,
        };
        let result = change_settings(original.clone(), &change).unwrap();
        assert_eq!(
            result["RoomList.OrderedCustomSections"],
            json!([
                "chats",
                "element.io.section.b",
                "element.io.section.a",
                "im.vector.fake.direct"
            ])
        );
        assert_eq!(
            result["RoomList.CustomSectionData"],
            original["RoomList.CustomSectionData"]
        );
        assert_eq!(result["other"], original["other"]);
        assert_eq!(change_settings(result.clone(), &change).unwrap(), result);
        let result = change_settings(
            result,
            &Change::Reorder {
                tag: "im.vector.fake.direct".into(),
                target: "chats".into(),
                before: true,
            },
        )
        .unwrap();
        assert_eq!(
            result["RoomList.OrderedCustomSections"],
            json!([
                "im.vector.fake.direct",
                "chats",
                "element.io.section.b",
                "element.io.section.a"
            ])
        );
        for (tag, target) in [
            ("m.favourite", "chats"),
            ("chats", "m.lowpriority"),
            ("chats", "element.io.section.deleted"),
            ("element.io.section.deleted", "chats"),
        ] {
            assert!(
                change_settings(
                    original.clone(),
                    &Change::Reorder {
                        tag: tag.into(),
                        target: target.into(),
                        before: true
                    }
                )
                .is_err()
            );
        }
        let default = change_settings(
            json!({"other":1}),
            &Change::Reorder {
                tag: "chats".into(),
                target: "im.vector.fake.direct".into(),
                before: true,
            },
        )
        .unwrap();
        assert_eq!(
            default,
            json!({"other":1,"RoomList.OrderedCustomSections":["chats","im.vector.fake.direct"]})
        );
        assert!(change_settings(json!({"RoomList.OrderedCustomSections":{}}), &change).is_err());
    }

    #[test]
    fn invalid_names_builtin_edits_and_malformed_order_never_replace_settings() {
        for change in [
            Change::Create {
                tag: "element.io.section.test".into(),
                name: "  ".into(),
                space_id: "home-space".into(),
            },
            Change::Rename {
                tag: "m.favourite".into(),
                name: "No".into(),
            },
            Change::Remove {
                tag: "m.lowpriority".into(),
            },
        ] {
            assert!(change_settings(json!({}), &change).is_err());
        }
        let change = Change::Create {
            tag: "element.io.section.test".into(),
            name: "Test".into(),
            space_id: "home-space".into(),
        };
        for settings in [json!([]), json!({"RoomList.OrderedCustomSections":{}})] {
            assert!(change_settings(settings, &change).is_err());
        }
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
