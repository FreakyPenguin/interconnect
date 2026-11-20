//! On-demand account profile reads and explicit, single-attempt field updates.
use crate::room_avatar::Upload;
use matrix_sdk::{
    Client,
    config::RequestConfig,
    ruma::{
        OwnedMxcUri,
        api::client::{
            media::create_content,
            profile::{get_profile, set_avatar_url, set_display_name},
        },
        profile::{AvatarUrl, DisplayName, UserProfile},
    },
};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub display_name: Option<String>,
    pub avatar_url: Option<OwnedMxcUri>,
}
impl Details {
    pub(crate) fn from_profile(profile: &UserProfile) -> Result<Self, String> {
        let display_name = if profile
            .get("displayname")
            .is_none_or(|value| value.is_null())
        {
            None
        } else {
            profile
                .get_static::<DisplayName>()
                .map_err(|_| "Could not read the display name.")?
        };
        let avatar_url = if profile
            .get("avatar_url")
            .is_none_or(|value| value.is_null() || value.as_str() == Some(""))
        {
            None
        } else {
            profile
                .get_static::<AvatarUrl>()
                .map_err(|_| "Could not read the avatar URL.")?
        };
        if avatar_url.as_ref().is_some_and(|url| !url.is_valid()) {
            return Err("The current avatar URL is invalid.".into());
        }
        Ok(Self {
            display_name,
            avatar_url,
        })
    }
}
#[derive(Clone, Debug)]
pub enum Change {
    DisplayName(String),
    Avatar(Option<Upload>),
}
pub fn validate_display_name(name: &str) -> Result<(), String> {
    if name.len() > 255 {
        return Err("Keep the display name within 255 bytes.".into());
    }
    if name.chars().any(char::is_control) {
        return Err("Display names cannot contain control characters.".into());
    }
    Ok(())
}
fn config() -> RequestConfig {
    RequestConfig::default()
        .disable_retry()
        .force_auth()
        .timeout(Duration::from_secs(20))
}
pub(crate) async fn load(client: &Client) -> Result<Details, String> {
    let user = client.user_id().ok_or("Sign in to edit your profile.")?;
    let response = tokio::time::timeout(
        Duration::from_secs(30),
        client
            .send(get_profile::v3::Request::new(user.to_owned()))
            .with_request_config(config()),
    )
    .await
    .map_err(|_| "Loading your profile timed out. Try Reload.")?
    .map_err(|_| "Could not load your profile. Try Reload.")?;
    Details::from_profile(&response.data)
}
/// Compare only the field being edited. Profile endpoints have no compare-and-swap;
/// a remote change between the final check and publication remains possible.
pub(crate) async fn save(
    client: &Client,
    baseline: Details,
    change: Change,
) -> Result<Details, String> {
    if let Change::DisplayName(name) = &change {
        validate_display_name(name)?;
    }
    let operation = async {
        let user = client.user_id().ok_or("Sign in to edit your profile.")?;
        let mut current = load(client).await?;
        match change {
            Change::DisplayName(name) => {
                if current.display_name != baseline.display_name {
                    return Err("Your display name changed. Reload before saving again.".into());
                }
                // The legacy endpoint also clears names on older homeservers.
                let name = (!name.trim().is_empty()).then_some(name);
                #[allow(deprecated)]
                let request = set_display_name::v3::Request::new(user.to_owned(), name.clone());
                client
                    .send(request)
                    .with_request_config(config())
                    .await
                    .map_err(
                        |_| "Could not confirm saving your display name. Reload before retrying.",
                    )?;
                current.display_name = name;
            }
            Change::Avatar(upload) => {
                if current.avatar_url != baseline.avatar_url {
                    return Err("Your avatar changed. Reload before saving again.".into());
                }
                let url = if let Some(upload) = upload {
                    let mut request = create_content::v3::Request::new(upload.bytes());
                    request.content_type = Some("image/png".into());
                    let response = client
                        .send(request)
                        .with_request_config(config())
                        .await
                        .map_err(|_| "Could not confirm avatar upload. Reload before retrying.")?;
                    if !response.content_uri.is_valid() {
                        return Err("The server returned an invalid avatar URL.".into());
                    }
                    current = load(client).await?;
                    if current.avatar_url != baseline.avatar_url {
                        return Err(
                            "Your avatar changed during upload. Reload before saving again.".into(),
                        );
                    }
                    Some(response.content_uri)
                } else {
                    None
                };
                #[allow(deprecated)]
                let request = set_avatar_url::v3::Request::new(user.to_owned(), url.clone());
                client.send(request).with_request_config(config()).await
                    .map_err(|_| "Could not confirm saving your avatar. Reload before retrying; an uploaded image may remain on the server.")?;
                current.avatar_url = url;
            }
        }
        Ok(current)
    };
    tokio::time::timeout(Duration::from_secs(60), operation)
        .await
        .map_err(|_| {
            "Saving your profile timed out and may have succeeded. Reload before retrying."
                .to_owned()
        })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::authentication::matrix::MatrixSession;
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, method, path},
    };

    const PROFILE: &str = "/_matrix/client/v3/profile/@me:example.org";
    async fn fixture() -> (MockServer, Client, Arc<Mutex<Value>>) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let profile = Arc::new(Mutex::new(
            json!({"displayname":"Before", "avatar_url":"mxc://example.org/before"}),
        ));
        let response = profile.clone();
        Mock::given(method("GET"))
            .and(path(PROFILE))
            .and(header("authorization", "Bearer fixture-token"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(response.lock().unwrap().clone())
            })
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
                    "user_id":"@me:example.org", "device_id":"TEST", "access_token":"fixture-token"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        (server, client, profile)
    }
    async fn upload() -> Upload {
        let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        image::DynamicImage::new_rgba8(3, 2)
            .save(file.path())
            .unwrap();
        crate::room_avatar::read_image(file.path().to_owned())
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn profile_reads_accept_unset_fields_and_reject_invalid_metadata() {
        let (_server, client, response) = fixture().await;
        for body in [
            json!({}),
            json!({"displayname":null,"avatar_url":null}),
            json!({"avatar_url":""}),
        ] {
            *response.lock().unwrap() = body;
            assert_eq!(load(&client).await.unwrap(), Details::default());
        }
        for body in [
            json!({"displayname":42}),
            json!({"avatar_url":"https://example.org/avatar"}),
        ] {
            *response.lock().unwrap() = body;
            assert!(load(&client).await.is_err());
        }
        assert!(validate_display_name("Zoë 🦊").is_ok());
        assert!(validate_display_name("").is_ok());
        assert!(validate_display_name("line\nbreak").is_err());
        assert!(validate_display_name(&"é".repeat(128)).is_err());
    }
    #[tokio::test]
    async fn name_updates_preserve_remote_avatar_and_can_clear_the_name() {
        let (server, client, response) = fixture().await;
        let baseline = load(&client).await.unwrap();
        response.lock().unwrap()["avatar_url"] = json!("mxc://example.org/remote");
        Mock::given(method("PUT"))
            .and(path(format!("{PROFILE}/displayname")))
            .and(body_json(json!({"displayname":"Zoë 🦊"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let changed = save(
            &client,
            baseline.clone(),
            Change::DisplayName("Zoë 🦊".into()),
        )
        .await
        .unwrap();
        assert_eq!(changed.display_name.as_deref(), Some("Zoë 🦊"));
        assert_eq!(
            changed.avatar_url.unwrap().as_str(),
            "mxc://example.org/remote"
        );
        Mock::given(method("PUT"))
            .and(path(format!("{PROFILE}/displayname")))
            .and(body_json(json!({})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            save(&client, baseline, Change::DisplayName(String::new()))
                .await
                .unwrap()
                .display_name
                .is_none()
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| !request.url.path().ends_with("/avatar_url"))
        );
    }
    #[tokio::test]
    async fn conflicting_or_invalid_names_never_write_and_failed_writes_do_not_retry() {
        let (server, client, response) = fixture().await;
        let baseline = load(&client).await.unwrap();
        response.lock().unwrap()["displayname"] = json!("Remote");
        assert!(
            save(&client, baseline.clone(), Change::DisplayName("New".into()))
                .await
                .unwrap_err()
                .contains("changed")
        );
        assert!(
            save(
                &client,
                baseline.clone(),
                Change::DisplayName("x".repeat(256))
            )
            .await
            .is_err()
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method != "PUT")
        );
        response.lock().unwrap()["displayname"] = json!("Before");
        Mock::given(method("PUT"))
            .and(path(format!("{PROFILE}/displayname")))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(json!({"errcode":"M_UNKNOWN","error":"fixture"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            save(&client, baseline, Change::DisplayName("New".into()))
                .await
                .unwrap_err()
                .contains("Reload")
        );
    }
    #[tokio::test]
    async fn avatar_upload_is_explicit_png_and_removal_does_not_delete_media() {
        let (server, client, response) = fixture().await;
        let baseline = load(&client).await.unwrap();
        let image = upload().await;
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method == "GET")
        );
        Mock::given(method("POST"))
            .and(path("/_matrix/media/v3/upload"))
            .and(header("content-type", "image/png"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"content_uri":"mxc://example.org/new"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path(format!("{PROFILE}/avatar_url")))
            .and(body_json(json!({"avatar_url":"mxc://example.org/new"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let changed = save(&client, baseline, Change::Avatar(Some(image)))
            .await
            .unwrap();
        assert_eq!(
            changed.avatar_url.as_ref().unwrap().as_str(),
            "mxc://example.org/new"
        );
        response.lock().unwrap()["avatar_url"] = json!("mxc://example.org/new");
        Mock::given(method("PUT"))
            .and(path(format!("{PROFILE}/avatar_url")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            save(&client, changed, Change::Avatar(None))
                .await
                .unwrap()
                .avatar_url
                .is_none()
        );
        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|request| request.method != "DELETE"));
        let posted = requests
            .iter()
            .find(|request| request.method == "POST")
            .unwrap();
        assert_eq!(
            image::guess_format(&posted.body).unwrap(),
            image::ImageFormat::Png
        );
        assert!(
            requests
                .iter()
                .all(|request| !request.url.path().ends_with("/displayname"))
        );
    }
    #[tokio::test]
    async fn remote_avatar_change_during_upload_prevents_publication() {
        let (server, client, response) = fixture().await;
        let baseline = load(&client).await.unwrap();
        let updated = response.clone();
        Mock::given(method("POST"))
            .and(path("/_matrix/media/v3/upload"))
            .respond_with(move |_: &wiremock::Request| {
                updated.lock().unwrap()["avatar_url"] = json!("mxc://example.org/remote");
                ResponseTemplate::new(200)
                    .set_body_json(json!({"content_uri":"mxc://example.org/new"}))
            })
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            save(&client, baseline, Change::Avatar(Some(upload().await)))
                .await
                .unwrap_err()
                .contains("during upload")
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method != "PUT")
        );
    }
    #[tokio::test]
    async fn failed_upload_has_one_attempt_and_never_publishes() {
        let (server, client, _) = fixture().await;
        let baseline = load(&client).await.unwrap();
        Mock::given(method("POST"))
            .and(path("/_matrix/media/v3/upload"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(json!({"errcode":"M_UNKNOWN","error":"fixture"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            save(&client, baseline, Change::Avatar(Some(upload().await)))
                .await
                .unwrap_err()
                .contains("Reload")
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method != "PUT")
        );
    }
}
