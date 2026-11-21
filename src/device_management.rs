//! Explicit own-device management. UIAA challenges are bound to one account and target.
use matrix_sdk::{
    Client,
    ruma::{
        OwnedDeviceId,
        api::client::uiaa::{
            AuthData, AuthType, Dummy, MatrixUserIdentifier, Password, UserIdentifier,
        },
    },
};
use std::{fmt, time::Duration};
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub current: bool,
    pub last_seen_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Password { rejected: bool },
    Browser,
}

#[derive(Clone)]
pub struct Challenge {
    user: String,
    current: String,
    target: String,
    homeserver: String,
    session: Zeroizing<String>,
    url: Zeroizing<String>,
    pub step: Step,
}
impl fmt::Debug for Challenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceChallenge(<redacted>)")
    }
}
impl Challenge {
    pub fn browser_url(&self) -> Option<&str> {
        (self.step == Step::Browser).then_some(self.url.as_str())
    }
}
#[derive(Clone)]
pub enum Reply {
    Password(Zeroizing<String>),
    BrowserContinue,
}
impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceReply(<redacted>)")
    }
}
#[derive(Clone, Debug)]
pub enum Revocation {
    Complete,
    Authenticate(Challenge),
}

const TIMEOUT: Duration = Duration::from_secs(30);
fn identity(client: &Client) -> Result<(String, String), String> {
    Ok((
        client
            .user_id()
            .ok_or("Sign in to manage devices.")?
            .to_string(),
        client
            .device_id()
            .ok_or("The current device is unavailable. Sign in again.")?
            .to_string(),
    ))
}
pub(crate) async fn load(client: &Client) -> Result<Vec<Device>, String> {
    let (_, current) = identity(client)?;
    let response = tokio::time::timeout(TIMEOUT, client.devices())
        .await
        .map_err(|_| "Loading devices timed out. Refresh to try again.")?
        .map_err(|_| "Could not load devices. Refresh to try again.")?;
    let mut devices: Vec<_> = response
        .devices
        .into_iter()
        .map(|device| Device {
            current: device.device_id.as_str() == current,
            id: device.device_id.to_string(),
            name: device.display_name.unwrap_or_default(),
            last_seen_ms: device.last_seen_ts.map(|ts| ts.get().into()),
        })
        .collect();
    devices.sort_by(|a, b| {
        (!a.current, a.name.to_lowercase(), &a.id).cmp(&(!b.current, b.name.to_lowercase(), &b.id))
    });
    Ok(devices)
}
async fn target(client: &Client, id: &str, revoke: bool) -> Result<OwnedDeviceId, String> {
    let (_, current) = identity(client)?;
    if revoke && current == id {
        return Err("Use Sign out to remove the current device.".into());
    }
    if !load(client).await?.iter().any(|device| device.id == id) {
        return Err("This device is no longer available. Refresh the list.".into());
    }
    Ok(id.into())
}
pub fn validate_name(input: &str) -> Result<&str, String> {
    let name = input.trim();
    if name.is_empty() || name.chars().count() > 128 || input.chars().any(char::is_control) {
        return Err("Enter a device name of 1–128 characters without line breaks.".into());
    }
    Ok(name)
}
pub(crate) async fn rename(client: &Client, id: &str, name: &str) -> Result<(), String> {
    let name = validate_name(name)?;
    tokio::time::timeout(TIMEOUT, async {
        let id = target(client, id, false).await?;
        client
            .rename_device(&id, name)
            .await
            .map(|_| ())
            .map_err(|_| "Renaming may not have completed. Refresh before retrying.".into())
    })
    .await
    .map_err(|_| "Renaming timed out and may have succeeded. Refresh before retrying.".to_owned())?
}
pub(crate) async fn revoke(
    client: &Client,
    id: &str,
    response: Option<(Challenge, Reply)>,
) -> Result<Revocation, String> {
    tokio::time::timeout(TIMEOUT, revoke_inner(client, id, response))
        .await
        .map_err(|_| {
            "Revoking timed out and may have succeeded. Refresh before retrying.".to_owned()
        })?
}

async fn revoke_inner(
    client: &Client,
    id: &str,
    response: Option<(Challenge, Reply)>,
) -> Result<Revocation, String> {
    let (user, current) = identity(client)?;
    let mut auth = if let Some((challenge, reply)) = response {
        if challenge.user != user
            || challenge.current != current
            || challenge.target != id
            || challenge.homeserver != client.homeserver().as_str()
        {
            return Err(
                "This authentication request is no longer valid. Refresh and try again.".into(),
            );
        }
        Some(match (challenge.step, reply) {
            (Step::Password { .. }, Reply::Password(password)) if !password.is_empty() => {
                let mut data = Password::new(
                    UserIdentifier::Matrix(MatrixUserIdentifier::new(user.clone())),
                    password.to_string(),
                );
                data.session = Some(challenge.session.to_string());
                AuthData::Password(data)
            }
            (Step::Browser, Reply::BrowserContinue) => {
                AuthData::fallback_acknowledgement(challenge.session.to_string())
            }
            _ => return Err("Complete the displayed authentication step.".into()),
        })
    } else {
        None
    };
    let id = target(client, id, true).await?;
    let mut sent_dummy = false;
    loop {
        let error = match client
            .delete_devices(std::slice::from_ref(&id), auth.take())
            .await
        {
            Ok(_) => return Ok(Revocation::Complete),
            Err(error) => error,
        };
        let Some(info) = error.as_uiaa_response() else {
            return Err("Revoking may not have completed. Refresh before retrying.".into());
        };
        let session = info.session.clone().ok_or(
            "The server did not provide an authentication session. Use another client to manage this device.",
        )?;
        let remaining = info
            .flows
            .iter()
            .map(|flow| {
                flow.stages
                    .iter()
                    .filter(|stage| !info.completed.contains(stage))
                    .collect::<Vec<_>>()
            })
            .filter(|stages| !stages.is_empty())
            .min_by_key(Vec::len)
            .ok_or(
                "The server returned no supported authentication step. Use another client to manage this device.",
            )?;
        let stage = remaining[0];
        if *stage == AuthType::Dummy {
            if sent_dummy {
                return Err(
                    "The server could not complete device authentication. Refresh and try again."
                        .into(),
                );
            }
            sent_dummy = true;
            let mut dummy = Dummy::new();
            dummy.session = Some(session);
            auth = Some(AuthData::Dummy(dummy));
            continue;
        }
        // Fallback is built from the authenticated homeserver, never a server-supplied URL.
        let mut url = client.homeserver();
        url.set_query(None);
        url.set_fragment(None);
        url.path_segments_mut()
            .map_err(|_| "Invalid homeserver URL.")?
            .pop_if_empty()
            .extend([
                "_matrix",
                "client",
                "v3",
                "auth",
                stage.as_str(),
                "fallback",
                "web",
            ]);
        url.query_pairs_mut().append_pair("session", &session);
        let step = if *stage == AuthType::Password {
            Step::Password {
                rejected: info.auth_error.is_some(),
            }
        } else {
            Step::Browser
        };
        return Ok(Revocation::Authenticate(Challenge {
            user,
            current,
            target: id.to_string(),
            homeserver: client.homeserver().to_string(),
            session: Zeroizing::new(session),
            url: Zeroizing::new(url.into()),
            step,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{authentication::matrix::MatrixSession, config::RequestConfig};
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    async fn fixture() -> (MockServer, Client) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .request_config(RequestConfig::default().disable_retry())
            .build()
            .await
            .unwrap();
        client.matrix_auth().restore_session(serde_json::from_value::<MatrixSession>(json!({"user_id":"@test:example.org", "device_id":"CURRENT", "access_token":"fixture"})).unwrap(), Default::default()).await.unwrap();
        Mock::given(method("GET")).and(path("/_matrix/client/v3/devices"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"devices":[
                {"device_id":"OTHER", "display_name":"Phone", "last_seen_ts":1234, "last_seen_ip":"192.0.2.1"},
                {"device_id":"CURRENT", "display_name":"Desktop"}, {"device_id":"THIRD"}
            ]}))).mount(&server).await;
        (server, client)
    }
    fn challenge(stages: &[&str], completed: &[&str], rejected: bool) -> ResponseTemplate {
        let mut body = json!({"session":"private-session", "flows":[{"stages":stages}], "completed":completed});
        if rejected {
            body["errcode"] = json!("M_FORBIDDEN");
            body["error"] = json!("private-server-detail");
        }
        ResponseTemplate::new(401).set_body_json(body)
    }
    async fn mutations(server: &MockServer) -> Vec<Value> {
        server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| {
                r.method.as_str() != "GET"
                    && (r.url.path().contains("/devices/")
                        || r.url.path().ends_with("/delete_devices"))
            })
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }
    #[tokio::test]
    async fn list_and_rename_use_own_server_devices_and_validate_targets() {
        let (server, client) = fixture().await;
        let devices = load(&client).await.unwrap();
        assert!(devices[0].current);
        assert_eq!(devices[0].id, "CURRENT");
        assert_eq!(
            devices
                .iter()
                .find(|d| d.id == "OTHER")
                .unwrap()
                .last_seen_ms,
            Some(1234)
        );
        Mock::given(method("PUT"))
            .and(path("/_matrix/client/v3/devices/CURRENT"))
            .and(body_json(json!({"display_name":"New desktop"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        rename(&client, "CURRENT", " New desktop ").await.unwrap();
        for (id, name) in [("UNKNOWN", "name"), ("OTHER", ""), ("OTHER", "bad\nname")] {
            assert!(rename(&client, id, name).await.is_err());
        }
        assert!(
            revoke(&client, "CURRENT", None)
                .await
                .unwrap_err()
                .contains("Sign out")
        );
        assert!(revoke(&client, "UNKNOWN", None).await.is_err());
        assert_eq!(
            mutations(&server).await,
            vec![json!({"display_name":"New desktop"})]
        );
    }
    #[tokio::test]
    async fn password_challenge_retries_and_is_bound_to_exact_identity_and_target() {
        let (server, client) = fixture().await;
        Mock::given(method("POST")).and(path("/_matrix/client/v3/delete_devices"))
            .respond_with(|request: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                if body["auth"]["password"] == "correct" {
                    assert_eq!(body, json!({"devices":["OTHER"],"auth":{"type":"m.login.password","session":"private-session","identifier":{"type":"m.id.user","user":"@test:example.org"},"password":"correct"}}));
                    ResponseTemplate::new(200).set_body_json(json!({}))
                } else { challenge(&["m.login.password"], &[], body.get("auth").is_some()) }
            }).expect(3).mount(&server).await;
        let Revocation::Authenticate(first) = revoke(&client, "OTHER", None).await.unwrap() else {
            panic!()
        };
        assert_eq!(first.step, Step::Password { rejected: false });
        let before = mutations(&server).await.len();
        assert!(
            revoke(
                &client,
                "THIRD",
                Some((
                    first.clone(),
                    Reply::Password(Zeroizing::new("correct".into()))
                ))
            )
            .await
            .is_err()
        );
        for binding in 0..3 {
            let mut wrong = first.clone();
            match binding {
                0 => wrong.current = "another-device".into(),
                1 => wrong.user = "@another:example.org".into(),
                _ => wrong.homeserver = "https://another.example.org/".into(),
            }
            assert!(
                revoke(
                    &client,
                    "OTHER",
                    Some((wrong, Reply::Password(Zeroizing::new("correct".into()))))
                )
                .await
                .is_err()
            );
        }
        assert_eq!(mutations(&server).await.len(), before);
        let Revocation::Authenticate(second) = revoke(
            &client,
            "OTHER",
            Some((first, Reply::Password(Zeroizing::new("wrong".into())))),
        )
        .await
        .unwrap() else {
            panic!()
        };
        assert_eq!(second.step, Step::Password { rejected: true });
        assert!(!format!("{second:?}").contains("private-session"));
        assert!(matches!(
            revoke(
                &client,
                "OTHER",
                Some((second, Reply::Password(Zeroizing::new("correct".into()))))
            )
            .await
            .unwrap(),
            Revocation::Complete
        ));
        assert!(
            !format!(
                "{:?}",
                Reply::Password(Zeroizing::new("secret-password".into()))
            )
            .contains("secret-password")
        );
    }
    #[tokio::test]
    async fn multi_stage_auth_uses_homeserver_fallback_and_explicit_continue() {
        let (server, client) = fixture().await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/delete_devices"))
            .respond_with(|request: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                if body["auth"]["type"] == "m.login.password" {
                    challenge(
                        &["m.login.password", "m.login.sso"],
                        &["m.login.password"],
                        false,
                    )
                } else if body["auth"]["session"] == "private-session" {
                    assert_eq!(body["auth"], json!({"session":"private-session"}));
                    ResponseTemplate::new(200).set_body_json(json!({}))
                } else {
                    challenge(&["m.login.password", "m.login.sso"], &[], false)
                }
            })
            .expect(3)
            .mount(&server)
            .await;
        let Revocation::Authenticate(password) = revoke(&client, "OTHER", None).await.unwrap()
        else {
            panic!()
        };
        let Revocation::Authenticate(browser) = revoke(
            &client,
            "OTHER",
            Some((password, Reply::Password(Zeroizing::new("correct".into())))),
        )
        .await
        .unwrap() else {
            panic!()
        };
        assert_eq!(browser.step, Step::Browser);
        assert_eq!(
            browser.browser_url().unwrap(),
            format!(
                "{}/_matrix/client/v3/auth/m.login.sso/fallback/web?session=private-session",
                server.uri()
            )
        );
        assert!(!format!("{browser:?}").contains("private-session"));
        assert!(matches!(
            revoke(&client, "OTHER", Some((browser, Reply::BrowserContinue)))
                .await
                .unwrap(),
            Revocation::Complete
        ));
    }
    #[tokio::test]
    async fn dummy_auth_is_bounded_and_errors_do_not_echo_server_details() {
        let (server, client) = fixture().await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/delete_devices"))
            .respond_with(challenge(&["m.login.dummy"], &[], true))
            .expect(2)
            .mount(&server)
            .await;
        let error = revoke(&client, "OTHER", None).await.unwrap_err();
        assert!(!error.contains("private-server-detail"));
        assert!(error.contains("could not complete"));
    }
    #[tokio::test]
    async fn successful_dummy_auth_and_server_rejection_are_not_silently_retried() {
        let (server, client) = fixture().await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/delete_devices"))
            .respond_with(|request: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                if body["auth"]["type"] == "m.login.dummy" {
                    ResponseTemplate::new(200).set_body_json(json!({}))
                } else {
                    challenge(&["m.login.dummy"], &[], false)
                }
            })
            .expect(2)
            .mount(&server)
            .await;
        assert!(matches!(
            revoke(&client, "OTHER", None).await.unwrap(),
            Revocation::Complete
        ));
        Mock::given(method("PUT"))
            .and(path("/_matrix/client/v3/devices/OTHER"))
            .respond_with(
                ResponseTemplate::new(403).set_body_json(
                    json!({"errcode":"M_FORBIDDEN","error":"private-server-detail"}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            !rename(&client, "OTHER", "New name")
                .await
                .unwrap_err()
                .contains("private-server-detail")
        );
    }
}
