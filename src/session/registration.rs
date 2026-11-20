//! Interactive registration stays with the session owner, including browser waits.
use std::fmt;

use anyhow::{Context, bail, ensure};
use matrix_sdk::{
    Client,
    ruma::api::{
        client::{
            account::register::v3::Request,
            uiaa::{AuthData, AuthType, Dummy, RegistrationToken},
        },
        error::ErrorKind,
    },
};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use super::LoginProgress;

/// Each prompt has its own channel, so replies to an old prompt cannot advance a
/// newer stage or attempt. The receiver is dropped on cancellation and shutdown.
#[derive(Clone)]
pub struct RegistrationChallenge {
    pub step: RegistrationStep,
    pub stage: String,
    pub response: mpsc::Sender<RegistrationReply>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum RegistrationStep {
    Browser(String),
    Token { rejected: bool },
}

#[derive(Clone)]
pub enum RegistrationReply {
    Continue,
    Token(Zeroizing<String>),
    Cancel,
}

impl fmt::Debug for RegistrationReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Continue => "Continue",
            Self::Token(_) => "Token(<redacted>)",
            Self::Cancel => "Cancel",
        })
    }
}

impl PartialEq for RegistrationChallenge {
    fn eq(&self, other: &Self) -> bool {
        self.step == other.step
            && self.stage == other.stage
            && self.response.same_channel(&other.response)
    }
}
impl Eq for RegistrationChallenge {}

impl fmt::Debug for RegistrationChallenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RegistrationChallenge(<redacted>)")
    }
}

#[derive(Debug)]
pub struct RegistrationCancelled;
impl fmt::Display for RegistrationCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Registration cancelled")
    }
}
impl std::error::Error for RegistrationCancelled {}

/// Only fixed, actionable messages are shown; server diagnostics can contain
/// submitted credentials or other private authentication data.
#[derive(Debug)]
pub struct RegistrationFailure(pub &'static str);
impl fmt::Display for RegistrationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for RegistrationFailure {}

pub(super) async fn register(
    client: &Client,
    username: String,
    password: Zeroizing<String>,
    mut progress: impl FnMut(LoginProgress) + Send,
) -> anyhow::Result<()> {
    let auth = client.matrix_auth();
    let mut authentication = None;
    let mut sent_dummy = false;
    let mut token_submitted = false;
    loop {
        let mut request = Request::new();
        request.username = Some(username.clone());
        request.password = Some(password.to_string());
        request.initial_device_display_name = Some("Interconnect".into());
        request.auth = authentication.take();
        let error = match auth.register(request).await {
            Ok(_) => {
                ensure!(
                    auth.session().is_some(),
                    RegistrationFailure(
                        "Your account may have been created, but the server did not return a session. Return to sign in."
                    )
                );
                return Ok(());
            }
            Err(error) => error,
        };
        let Some(info) = error.as_uiaa_response() else {
            return Err(RegistrationFailure(match error.client_api_error_kind() {
                Some(ErrorKind::UserInUse) => "This username is already taken. Choose another, or sign in to your existing account.",
                Some(ErrorKind::InvalidUsername) => "This server does not accept that username. Enter a username without @ or a server name.",
                Some(ErrorKind::WeakPassword) => "This server requires a stronger password. Choose a longer password and try again.",
                Some(ErrorKind::Forbidden) => "Registration is not available on this homeserver. Check with its administrator or choose another homeserver.",
                Some(ErrorKind::Exclusive) => "This username is reserved. Choose another username.",
                Some(ErrorKind::LimitExceeded(_)) => "Too many registration attempts. Wait before trying again.",
                _ => "Registration could not finish. Check your connection and homeserver. If your account was created, return to sign in instead of registering again.",
            }).into());
        };
        let session = info.session.clone().context(RegistrationFailure(
            "The homeserver did not provide a registration session. Try another homeserver or contact its administrator."
        ))?;
        // Prefer the flow requiring the fewest remaining stages. Preserve its
        // advertised order and never accept terms on the user's behalf.
        let remaining = info.flows.iter()
            .map(|flow| flow.stages.iter().filter(|stage| !info.completed.contains(stage)).collect::<Vec<_>>())
            .filter(|stages| !stages.is_empty())
            .min_by_key(Vec::len)
            .context(RegistrationFailure("The homeserver returned no registration steps. Try again or contact its administrator."))?;
        let stage = remaining[0];
        if *stage == AuthType::Dummy {
            // A server repeating an unsuccessful dummy stage must not cause a
            // tight retry loop. All other retries require a user action.
            if sent_dummy {
                bail!(RegistrationFailure(
                    "The homeserver could not complete registration. Try again or contact its administrator."
                ));
            }
            sent_dummy = true;
            let mut dummy = Dummy::new();
            dummy.session = Some(session);
            authentication = Some(AuthData::Dummy(dummy));
            continue;
        }
        let token_required = *stage == AuthType::RegistrationToken;
        let step = if token_required {
            RegistrationStep::Token {
                rejected: token_submitted || info.auth_error.is_some(),
            }
        } else {
            let mut url = client.homeserver();
            url.set_query(None);
            url.set_fragment(None);
            url.path_segments_mut()
                .map_err(|_| RegistrationFailure("Invalid homeserver URL"))?
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
            RegistrationStep::Browser(url.into())
        };
        let label = match stage {
            AuthType::ReCaptcha => "Complete the CAPTCHA",
            AuthType::EmailIdentity => "Verify your email address",
            AuthType::Msisdn => "Verify your phone number",
            AuthType::RegistrationToken => "Enter your registration token",
            AuthType::Terms => "Review the server's terms",
            _ => "Complete the server's registration step",
        };
        let (response, mut replies) = mpsc::channel(1);
        progress(LoginProgress::Registration(RegistrationChallenge {
            step,
            stage: label.into(),
            response,
        }));
        authentication = Some(match replies.recv().await {
            Some(RegistrationReply::Cancel) | None => return Err(RegistrationCancelled.into()),
            Some(RegistrationReply::Token(token)) if token_required && !token.trim().is_empty() => {
                token_submitted = true;
                let mut data = RegistrationToken::new(token.to_string());
                data.session = Some(session);
                AuthData::RegistrationToken(data)
            }
            Some(RegistrationReply::Continue) if !token_required => {
                AuthData::fallback_acknowledgement(session)
            }
            _ => bail!(RegistrationFailure(
                "The registration step could not be submitted. Try again."
            )),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::sliding_sync::VersionBuilder;
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    async fn fixture() -> (MockServer, Client) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions": ["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(VersionBuilder::Native)
            .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
            .build()
            .await
            .unwrap();
        (server, client)
    }

    fn success() -> Value {
        json!({"user_id":"@registered:example.org", "device_id":"REGISTRATION", "access_token":"fixture-token"})
    }

    async fn run(
        client: &Client,
        progress: impl FnMut(LoginProgress) + Send,
    ) -> anyhow::Result<()> {
        register(
            client,
            "fixture".into(),
            Zeroizing::new("fixture-password".into()),
            progress,
        )
        .await
    }

    #[tokio::test]
    async fn direct_and_dummy_registration_install_the_returned_identity_without_sync() {
        for dummy in [false, true] {
            let (server, client) = fixture().await;
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/register"))
                .respond_with(move |request: &wiremock::Request| {
                    let body: Value = request.body_json().unwrap();
                    assert_eq!(body["username"], "fixture");
                    assert_eq!(body["password"], "fixture-password");
                    assert_eq!(body["initial_device_display_name"], "Interconnect");
                    assert!(body.get("inhibit_login").is_none());
                    if dummy && body.get("auth").is_none() {
                        ResponseTemplate::new(401).set_body_json(json!({
                            "session":"fixture-session", "flows":[{"stages":["m.login.dummy"]}]
                        }))
                    } else {
                        if dummy {
                            assert_eq!(
                                body["auth"],
                                json!({"type":"m.login.dummy", "session":"fixture-session"})
                            );
                        }
                        ResponseTemplate::new(200).set_body_json(success())
                    }
                })
                .expect(if dummy { 2 } else { 1 })
                .mount(&server)
                .await;
            run(&client, |_| panic!("no browser required"))
                .await
                .unwrap();
            assert_eq!(
                client.user_id().unwrap().as_str(),
                "@registered:example.org"
            );
            assert_eq!(client.device_id().unwrap().as_str(), "REGISTRATION");
            assert!(
                !server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .any(|r| r.url.path().ends_with("/sync"))
            );
        }
    }

    #[tokio::test]
    async fn browser_stages_wait_for_explicit_replies_and_preserve_the_session() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let (server, client) = fixture().await;
        let count = Arc::new(AtomicUsize::new(0));
        let requests = count.clone();
        Mock::given(method("POST")).and(path("/_matrix/client/v3/register"))
            .respond_with(move |request: &wiremock::Request| {
                let body: Value = request.body_json().unwrap();
                let step = requests.fetch_add(1, Ordering::SeqCst);
                if step > 0 { assert_eq!(body["auth"], json!({"session":"session &/?#"})); }
                if step == 3 { return ResponseTemplate::new(200).set_body_json(success()); }
                // First Continue was premature: re-present the same stage without
                // sending another request automatically. Then advance to terms.
                ResponseTemplate::new(401).set_body_json(json!({
                    "session":"session &/?#", "completed": if step == 2 { vec!["m.login.recaptcha"] } else { vec![] },
                    "flows":[{"stages":["m.login.recaptcha", "m.login.terms"]}]
                }))
            }).expect(4).mount(&server).await;
        let (prompts, mut received) = mpsc::unbounded_channel();
        let registration = run(&client, |step| {
            let LoginProgress::Registration(challenge) = step else {
                panic!("unexpected SSO")
            };
            prompts.send(challenge).unwrap();
        });
        let respond = async {
            let mut previous: Option<RegistrationChallenge> = None;
            for step in 0..3 {
                let challenge = received.recv().await.unwrap();
                assert_eq!(count.load(Ordering::SeqCst), step + 1);
                let url = url::Url::parse(match &challenge.step {
                    RegistrationStep::Browser(url) => url,
                    _ => panic!("expected browser stage"),
                })
                .unwrap();
                assert_eq!(
                    url.query_pairs().collect::<Vec<_>>(),
                    vec![("session".into(), "session &/?#".into())]
                );
                assert!(url.path().ends_with(if step == 2 {
                    "/m.login.terms/fallback/web"
                } else {
                    "/m.login.recaptcha/fallback/web"
                }));
                assert!(!format!("{challenge:?}").contains("session &"));
                if let Some(previous) = previous {
                    assert!(
                        previous
                            .response
                            .try_send(RegistrationReply::Continue)
                            .is_err()
                    );
                }
                challenge
                    .response
                    .try_send(RegistrationReply::Continue)
                    .unwrap();
                previous = Some(challenge);
            }
        };
        let (result, ()) = tokio::join!(registration, respond);
        result.unwrap();
        assert!(client.matrix_auth().session().is_some());
    }

    #[tokio::test]
    async fn registration_tokens_are_submitted_natively_and_rejected_tokens_can_be_retried() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        for terms in [false, true] {
            let (server, client) = fixture().await;
            let count = Arc::new(AtomicUsize::new(0));
            let requests = count.clone();
            Mock::given(method("POST")).and(path("/_matrix/client/v3/register"))
                .respond_with(move |request: &wiremock::Request| {
                    let body: Value = request.body_json().unwrap();
                    let step = requests.fetch_add(1, Ordering::SeqCst);
                    match step {
                        0 => assert!(body.get("auth").is_none()),
                        1 | 2 => assert_eq!(body["auth"], json!({
                            "type":"m.login.registration_token", "session":"token-session",
                            "token": if step == 1 { "invalid-fixture-token" } else { "valid-fixture-token" },
                        })),
                        3 => assert_eq!(body["auth"], json!({"session":"token-session"})),
                        _ => panic!("unexpected registration request"),
                    }
                    if step == 3 || (step == 2 && !terms) {
                        return ResponseTemplate::new(200).set_body_json(success());
                    }
                    let mut response = json!({
                        "session":"token-session",
                        "flows":[{"stages": if terms { vec!["m.login.registration_token", "m.login.terms"] } else { vec!["m.login.registration_token"] }}],
                        "completed": if step == 2 { vec!["m.login.registration_token"] } else { vec![] },
                    });
                    if step == 1 {
                        response["errcode"] = json!("M_UNAUTHORIZED");
                        response["error"] = json!("private-token-error");
                    }
                    ResponseTemplate::new(401).set_body_json(response)
                }).expect(if terms { 4 } else { 3 }).mount(&server).await;
            let (prompts, mut received) = mpsc::unbounded_channel();
            let registration = run(&client, |step| {
                let LoginProgress::Registration(challenge) = step else {
                    panic!("unexpected SSO")
                };
                prompts.send(challenge).unwrap();
            });
            let respond = async {
                let mut previous: Option<RegistrationChallenge> = None;
                for step in 0..if terms { 3 } else { 2 } {
                    let challenge = received.recv().await.unwrap();
                    assert_eq!(count.load(Ordering::SeqCst), step + 1);
                    if let Some(previous) = previous {
                        assert!(
                            previous
                                .response
                                .try_send(RegistrationReply::Token(Zeroizing::new(
                                    "stale-fixture-token".into()
                                )))
                                .is_err()
                        );
                    }
                    let reply = if step < 2 {
                        assert!(
                            matches!(challenge.step, RegistrationStep::Token { rejected } if rejected == (step == 1))
                        );
                        RegistrationReply::Token(Zeroizing::new(
                            if step == 0 {
                                "invalid-fixture-token"
                            } else {
                                "valid-fixture-token"
                            }
                            .into(),
                        ))
                    } else {
                        assert!(
                            matches!(&challenge.step, RegistrationStep::Browser(url) if url.contains("/m.login.terms/fallback/web?session=token-session"))
                        );
                        RegistrationReply::Continue
                    };
                    let debug = format!("{challenge:?} {reply:?}");
                    assert!(!debug.contains("fixture-token"));
                    assert!(!debug.contains("private-token-error"));
                    challenge.response.try_send(reply).unwrap();
                    previous = Some(challenge);
                }
            };
            let (result, ()) = tokio::join!(registration, respond);
            result.unwrap();
            assert_eq!(
                client.user_id().unwrap().as_str(),
                "@registered:example.org"
            );
            assert!(
                !server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .any(|r| r.url.path().contains("/auth/"))
            );
        }
    }

    #[tokio::test]
    async fn cancel_and_dropping_the_attempt_do_not_create_a_session() {
        for (cancel, token_stage) in [(true, false), (false, false), (true, true), (false, true)] {
            let (server, client) = fixture().await;
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/register"))
                .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                    "session":"fixture-session", "flows":[{"stages":[if token_stage { "m.login.registration_token" } else { "m.login.email.identity" }]}]
                })))
                .expect(1)
                .mount(&server)
                .await;
            let (prompts, mut received) = mpsc::unbounded_channel();
            let mut attempt = Box::pin(run(&client, |step| {
                let LoginProgress::Registration(challenge) = step else {
                    panic!("unexpected SSO")
                };
                prompts.send(challenge).unwrap();
            }));
            let challenge = tokio::select! {
                challenge = received.recv() => challenge.unwrap(),
                _ = &mut attempt => panic!("registration did not wait"),
            };
            if cancel {
                challenge
                    .response
                    .try_send(RegistrationReply::Cancel)
                    .unwrap();
                assert!(attempt.await.unwrap_err().is::<RegistrationCancelled>());
            } else {
                drop(attempt);
                assert!(
                    challenge
                        .response
                        .try_send(RegistrationReply::Continue)
                        .is_err()
                );
            }
            assert!(client.matrix_auth().session().is_none());
        }
    }

    #[tokio::test]
    async fn errors_are_actionable_and_malformed_flows_do_not_loop() {
        for (status, body, expected, count) in [
            (
                400,
                json!({"errcode":"M_USER_IN_USE", "error":"private-server-detail"}),
                "already taken",
                1,
            ),
            (
                400,
                json!({"errcode":"M_INVALID_USERNAME", "error":"private-server-detail"}),
                "does not accept",
                1,
            ),
            (
                400,
                json!({"errcode":"M_WEAK_PASSWORD", "error":"private-server-detail"}),
                "stronger password",
                1,
            ),
            (
                403,
                json!({"errcode":"M_FORBIDDEN", "error":"private-server-detail"}),
                "not available",
                1,
            ),
            (
                401,
                json!({"flows":[{"stages":["m.login.dummy"]}]}),
                "did not provide",
                1,
            ),
            (
                401,
                json!({"session":"fixture", "flows":[]}),
                "no registration steps",
                1,
            ),
            (
                401,
                json!({"session":"fixture", "flows":[{"stages":["m.login.dummy"]}]}),
                "could not complete",
                2,
            ),
            (
                200,
                json!({"user_id":"@fixture:example.org"}),
                "did not return a session",
                1,
            ),
        ] {
            let (server, client) = fixture().await;
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/register"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .expect(count)
                .mount(&server)
                .await;
            let error = run(&client, |_| panic!("unexpected prompt"))
                .await
                .unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert!(!format!("{error:?}").contains("private-server-detail"));
            assert!(client.matrix_auth().session().is_none());
        }
    }
}
