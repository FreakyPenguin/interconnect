//! Explicit, fresh-server ignored-user account data edits; never an automatic retry.
use matrix_sdk::{
    Client,
    config::RequestConfig,
    ruma::{
        OwnedUserId,
        api::{
            client::config::{get_global_account_data, set_global_account_data},
            error::ErrorKind,
        },
        events::{AnyGlobalAccountDataEventContent, GlobalAccountDataEventType},
        serde::Raw,
    },
};
use serde_json::Value;
use std::time::Duration;

fn config() -> RequestConfig {
    RequestConfig::default()
        .disable_retry()
        .force_auth()
        .timeout(Duration::from_secs(20))
}
fn users(content: &Value) -> Result<Vec<String>, String> {
    let entries = content
        .get("ignored_users")
        .and_then(Value::as_object)
        .ok_or("The ignored-user list is invalid. It was not changed.")?;
    if entries.len() > 10_000 {
        return Err("The ignored-user list is too large to manage here.".into());
    }
    let mut users = Vec::with_capacity(entries.len());
    for (user, entry) in entries {
        user.parse::<OwnedUserId>()
            .map_err(|_| "The ignored-user list contains an invalid ID.")?;
        if !entry.is_object() {
            return Err("The ignored-user list contains an invalid entry.".into());
        }
        users.push(user.clone());
    }
    users.sort();
    Ok(users)
}
async fn fetch(client: &Client) -> Result<Value, String> {
    let own = client.user_id().ok_or("Sign in to manage ignored users.")?;
    let request = get_global_account_data::v3::Request::new(
        own.to_owned(),
        GlobalAccountDataEventType::IgnoredUserList,
    );
    match client.send(request).with_request_config(config()).await {
        Ok(response) => serde_json::from_str(response.account_data.json().get())
            .map_err(|_| "The ignored-user list is invalid. It was not changed.".into()),
        Err(error) if error.client_api_error_kind() == Some(&ErrorKind::NotFound) => {
            Ok(serde_json::json!({"ignored_users": {}}))
        }
        Err(_) => Err("Could not load ignored users. Use Reload before trying again.".into()),
    }
}
pub(crate) async fn load(client: &Client) -> Result<Vec<String>, String> {
    users(&fetch(client).await?)
}
pub(crate) async fn change(
    client: &Client,
    user: String,
    ignored: bool,
) -> Result<Vec<String>, String> {
    let user: OwnedUserId = user
        .trim()
        .parse()
        .map_err(|_| "Enter a full Matrix ID, such as @name:example.org.")?;
    let own = client.user_id().ok_or("Sign in to manage ignored users.")?;
    if user == own {
        return Err("You cannot ignore yourself.".into());
    }
    // SDK ignore_user/unignore_user read the sync cache. Fetch server state so
    // consecutive edits before sync catches up do not discard other entries.
    let mut content = fetch(client).await?;
    users(&content)?;
    let entries = content
        .get_mut("ignored_users")
        .and_then(Value::as_object_mut)
        .unwrap();
    let changed = if ignored {
        if entries.contains_key(user.as_str()) {
            false
        } else {
            entries.insert(user.to_string(), serde_json::json!({}));
            true
        }
    } else {
        entries.remove(user.as_str()).is_some()
    };
    if changed {
        users(&content)?;
        let raw: Raw<AnyGlobalAccountDataEventContent> = Raw::from_json_string(content.to_string())
            .map_err(|_| "Could not prepare the ignored-user list.")?;
        let request = set_global_account_data::v3::Request::new_raw(
            own.to_owned(),
            GlobalAccountDataEventType::IgnoredUserList,
            raw,
        );
        client.send(request).with_request_config(config()).await
            .map_err(|_| "The update could not be confirmed and may have succeeded. Reload before retrying.")?;
    }
    users(&content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::authentication::matrix::MatrixSession;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };
    const ENDPOINT: &str =
        "/_matrix/client/v3/user/@me:example.org/account_data/m.ignored_user_list";
    async fn fixture(value: Value) -> (MockServer, Client, Arc<Mutex<Value>>) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let content = Arc::new(Mutex::new(value));
        let read = content.clone();
        Mock::given(method("GET"))
            .and(path(ENDPOINT))
            .and(header("authorization", "Bearer fixture-token"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(read.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        let write = content.clone();
        Mock::given(method("PUT"))
            .and(path(ENDPOINT))
            .and(header("authorization", "Bearer fixture-token"))
            .respond_with(move |request: &wiremock::Request| {
                *write.lock().unwrap() = serde_json::from_slice(&request.body).unwrap();
                ResponseTemplate::new(200).set_body_json(json!({}))
            })
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .build()
            .await
            .unwrap();
        client.matrix_auth().restore_session(serde_json::from_value::<MatrixSession>(json!({
            "user_id":"@me:example.org", "device_id":"FIXTURE", "access_token":"fixture-token"
        })).unwrap(), Default::default()).await.unwrap();
        (server, client, content)
    }
    #[tokio::test]
    async fn consecutive_changes_before_sync_preserve_remote_users_and_metadata() {
        let (_server, client, content) = fixture(json!({
            "ignored_users":{"@existing:example.org":{"custom":true}}, "unrelated":{"keep":1}
        }))
        .await;
        change(&client, "@one:example.org".into(), true)
            .await
            .unwrap();
        change(&client, "@two:example.org".into(), true)
            .await
            .unwrap();
        let users = change(&client, "@one:example.org".into(), false)
            .await
            .unwrap();
        assert_eq!(users, ["@existing:example.org", "@two:example.org"]);
        let value = content.lock().unwrap();
        assert_eq!(
            value["ignored_users"]["@existing:example.org"]["custom"],
            true
        );
        assert_eq!(value["unrelated"]["keep"], 1);
    }
    #[tokio::test]
    async fn invalid_lists_and_self_ignore_never_write() {
        let (server, client, content) = fixture(json!({"ignored_users": []})).await;
        assert!(
            change(&client, "@someone:example.org".into(), true)
                .await
                .is_err()
        );
        assert!(
            change(&client, "@me:example.org".into(), true)
                .await
                .is_err()
        );
        assert!(change(&client, "invalid".into(), true).await.is_err());
        assert_eq!(*content.lock().unwrap(), json!({"ignored_users": []}));
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method.as_str() != "PUT")
        );
    }
    #[tokio::test]
    async fn ambiguous_update_is_not_retried_and_can_be_reloaded() {
        let (server, client, _content) = fixture(json!({"ignored_users": {}})).await;
        Mock::given(method("PUT"))
            .and(path(ENDPOINT))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(json!({"errcode":"M_UNKNOWN", "error":"fixture"})),
            )
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        assert!(
            change(&client, "@someone:example.org".into(), true)
                .await
                .unwrap_err()
                .contains("Reload")
        );
        assert_eq!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|request| request.method.as_str() == "PUT")
                .count(),
            1
        );
        assert!(load(&client).await.unwrap().is_empty());
    }
}
