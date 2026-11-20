//! Event-driven sign-in and registration. SDK work stays in the connection owner.
use super::{App, Message, branding, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use interconnect::connected::{Authentication, LoginRequest};
use interconnect::session::{
    LoginMethod, RegistrationChallenge, RegistrationReply, RegistrationStep,
};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct State {
    pub homeserver: String,
    pub username: String,
    pub password: Zeroizing<String>,
    pub confirm_password: Zeroizing<String>,
    pub registering: bool,
    registration_token: Zeroizing<String>,
    token_prompt: Option<RegistrationChallenge>,
    pub sender: Option<tokio::sync::mpsc::Sender<LoginRequest>>,
    pub browser_error: Option<String>,
    verify_after_login: bool,
    recover_after_registration: bool,
    cancel_requested: bool,
}

#[derive(Clone)]
pub enum Event {
    Ready(tokio::sync::mpsc::Sender<LoginRequest>),
    Homeserver(String),
    Username(String),
    Password(Zeroizing<String>),
    ConfirmPassword(Zeroizing<String>),
    ToggleRegistration,
    SubmitRegistration,
    RegistrationResponse(RegistrationChallenge, RegistrationReply),
    RegistrationToken(RegistrationChallenge, Zeroizing<String>),
    SubmitToken(RegistrationChallenge),
    SubmitPassword,
    Submit,
    CancelSignIn,
    OpenBrowser,
    BrowserOpened(String, Result<(), String>),
    CopyLink,
}

impl std::fmt::Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Ready(_) => "Ready",
            Self::Homeserver(_) => "Homeserver",
            Self::Username(_) => "Username",
            Self::Password(_) => "Password(<redacted>)",
            Self::ConfirmPassword(_) => "ConfirmPassword(<redacted>)",
            Self::ToggleRegistration => "ToggleRegistration",
            Self::SubmitRegistration => "SubmitRegistration",
            Self::RegistrationResponse(_, _) => "RegistrationResponse",
            Self::RegistrationToken(_, _) => "RegistrationToken(<redacted>)",
            Self::SubmitToken(_) => "SubmitToken",
            Self::SubmitPassword => "SubmitPassword",
            Self::Submit => "Submit",
            Self::CancelSignIn => "CancelSignIn",
            Self::OpenBrowser => "OpenBrowser",
            Self::BrowserOpened(_, _) => "BrowserOpened",
            Self::CopyLink => "CopyLink",
        };
        f.write_str(name)
    }
}

impl App {
    pub fn login_update(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Ready(sender) => self.login.sender = Some(sender),
            Event::ToggleRegistration
                if !self.closing
                    && matches!(
                        self.model.authentication,
                        Some(Authentication::SignIn { .. })
                    ) =>
            {
                self.login.registering = !self.login.registering;
                self.login.password = Zeroizing::new(String::new());
                self.login.confirm_password = Zeroizing::new(String::new());
                self.login.clear_registration_token();
                self.login.browser_error = None;
                self.model.authentication = Some(Authentication::SignIn { error: None });
            }
            Event::Homeserver(value)
                if matches!(
                    self.model.authentication,
                    Some(Authentication::SignIn { .. })
                ) =>
            {
                self.login.homeserver = value;
            }
            Event::Username(value)
                if matches!(
                    self.model.authentication,
                    Some(Authentication::SignIn { .. })
                ) =>
            {
                self.login.username = value
            }
            Event::Password(password)
                if matches!(
                    self.model.authentication,
                    Some(Authentication::SignIn { .. })
                ) =>
            {
                self.login.password = password
            }
            Event::ConfirmPassword(password)
                if !self.closing
                    && self.login.registering
                    && matches!(
                        self.model.authentication,
                        Some(Authentication::SignIn { .. })
                    ) =>
            {
                self.login.confirm_password = password;
            }
            Event::Submit | Event::SubmitPassword | Event::SubmitRegistration
                if !self.closing
                    && matches!(
                        self.model.authentication,
                        Some(Authentication::SignIn { .. })
                    ) =>
            {
                let homeserver = self.login.homeserver.trim();
                let method = if matches!(event, Event::SubmitRegistration) {
                    if !self.login.registering || !self.login.registration_ready() {
                        return Task::none();
                    }
                    LoginMethod::Register {
                        username: self.login.username.trim().to_owned(),
                        password: self.login.password.clone(),
                    }
                } else if self.login.registering {
                    return Task::none();
                } else if matches!(event, Event::SubmitPassword) {
                    if self.login.username.trim().is_empty() || self.login.password.is_empty() {
                        return Task::none();
                    }
                    LoginMethod::Password {
                        username: self.login.username.trim().to_owned(),
                        password: self.login.password.clone(),
                    }
                } else {
                    LoginMethod::Browser
                };
                if !homeserver.is_empty()
                    && self.login.sender.as_ref().is_some_and(|s| {
                        s.try_send(LoginRequest::SignIn {
                            homeserver: homeserver.to_owned(),
                            method,
                        })
                        .is_ok()
                    })
                {
                    self.login.password = Zeroizing::new(String::new());
                    self.login.confirm_password = Zeroizing::new(String::new());
                    self.login.browser_error = None;
                    self.login.verify_after_login = true;
                    self.login.recover_after_registration =
                        matches!(event, Event::SubmitRegistration);
                    self.model.authentication = Some(Authentication::Waiting { url: None });
                }
            }
            Event::CancelSignIn
                if !self.closing
                    && !self.login.cancel_requested
                    && matches!(
                        self.model.authentication,
                        Some(Authentication::Waiting { .. })
                    ) =>
            {
                if self
                    .login
                    .sender
                    .as_ref()
                    .is_some_and(|sender| sender.try_send(LoginRequest::Cancel).is_ok())
                {
                    self.login.cancel_requested = true;
                    self.login.browser_error = None;
                } else {
                    self.login.browser_error = Some(
                        "Could not request cancellation yet. Try Cancel again in a moment.".into(),
                    );
                }
                // The connection owner publishes the result after safe cleanup.
                // Never display a fresh form while credential work is admitted.
            }
            Event::RegistrationToken(challenge, token)
                if !self.closing
                    && matches!(&self.model.authentication, Some(Authentication::Registration(current)) if current == &challenge && matches!(current.step, RegistrationStep::Token { .. })) =>
            {
                self.login.token_prompt = Some(challenge);
                self.login.registration_token = token;
            }
            Event::SubmitToken(challenge)
                if !self.closing
                    && matches!(&self.model.authentication, Some(Authentication::Registration(current)) if current == &challenge)
                    && !self
                        .login
                        .registration_token_for(&challenge)
                        .trim()
                        .is_empty() =>
            {
                let token = std::mem::take(&mut self.login.registration_token);
                return self.login_update(Event::RegistrationResponse(
                    challenge,
                    RegistrationReply::Token(token),
                ));
            }
            Event::RegistrationResponse(challenge, reply)
                if !self.closing
                    && matches!(&self.model.authentication, Some(Authentication::Registration(current)) if current == &challenge) =>
            {
                let valid = match &reply {
                    RegistrationReply::Cancel => true,
                    RegistrationReply::Continue => {
                        matches!(challenge.step, RegistrationStep::Browser(_))
                    }
                    RegistrationReply::Token(token) => {
                        matches!(challenge.step, RegistrationStep::Token { .. })
                            && !token.trim().is_empty()
                    }
                };
                if valid && challenge.response.try_send(reply).is_ok() {
                    self.login.clear_registration_token();
                    self.login.browser_error = None;
                    self.model.authentication = Some(Authentication::Waiting { url: None });
                }
            }
            Event::OpenBrowser => {
                if let Some(url) = authentication_url(self.model.authentication.as_ref()) {
                    let url = url.to_owned();
                    let opened_url = url.clone();
                    return Task::perform(
                        async move {
                            #[cfg(target_os = "macos")]
                            let program = "open";
                            #[cfg(not(target_os = "macos"))]
                            let program = "xdg-open";
                            // Await process completion asynchronously. Some launchers
                            // stay alive with the browser; they must not occupy a
                            // Tokio blocking worker or delay runtime shutdown.
                            let opened = tokio::process::Command::new(program)
                                .arg(url)
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status()
                                .await;
                            match opened {
                                Ok(status) if status.success() => Ok(()),
                                _ => {
                                    Err("Could not open your browser. Copy the link instead."
                                        .into())
                                }
                            }
                        },
                        move |result| {
                            Message::Login(Event::BrowserOpened(opened_url.clone(), result))
                        },
                    );
                }
            }
            Event::BrowserOpened(url, result)
                if authentication_url(self.model.authentication.as_ref()) == Some(url.as_str()) =>
            {
                self.login.browser_error = result.err();
            }
            Event::CopyLink => {
                if let Some(url) = authentication_url(self.model.authentication.as_ref()) {
                    return iced::clipboard::write(url.to_owned());
                }
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn finish_login(&mut self) -> Task<Message> {
        if !matches!(
            self.model.authentication,
            Some(Authentication::Waiting { .. })
        ) {
            self.login.cancel_requested = false;
        }
        if self.closing || self.login.token_prompt.as_ref().is_some_and(|prompt| {
            !matches!(&self.model.authentication, Some(Authentication::Registration(current)) if current == prompt)
        }) {
            self.login.clear_registration_token();
        }
        if !self.login.verify_after_login {
            return Task::none();
        }
        if self.closing
            || self.stopped
            || self.demo.is_some()
            || self.selection.sign_out != 0
            || matches!(
                self.model.authentication,
                Some(Authentication::SignIn { .. } | Authentication::Failed(_))
            )
        {
            self.login.verify_after_login = false;
            return Task::none();
        }
        // Authentication and the verification worker arrive in separate snapshots.
        // Wait for readiness through normal UI events, without a timer or SDK work.
        if self.model.authentication.is_some() || self.model.verification.control.is_none() {
            return Task::none();
        }
        if self.login.recover_after_registration {
            if self.model.recovery.control.is_none() || !self.context_menu_allowed() {
                return Task::none();
            }
            self.login.verify_after_login = false;
            self.login.recover_after_registration = false;
            self.context_menu = None;
            self.shell.spaces.menu = None;
            self.stop_typing();
            return self.verification_update(super::verification::Event::OpenRecovery);
        }
        if self.verification.open
            || self.model.verification.trust == matrix_sdk::encryption::VerificationState::Verified
        {
            self.login.verify_after_login = false;
            return Task::none();
        }
        if !self.context_menu_allowed() {
            return Task::none();
        }
        self.login.verify_after_login = false;
        self.context_menu = None;
        self.shell.spaces.menu = None;
        self.stop_typing();
        let task = self.verification_update(super::verification::Event::Open);
        self.publish_notification_context();
        task
    }

    pub fn login_view(&self, authentication: &Authentication) -> Element<'_, Message> {
        let mut content = column![
            branding::logo_image(80.0),
            shell::title("Interconnect", 30),
            text("A space for your conversations.")
                .size(15)
                .style(shell::muted_text),
        ]
        .spacing(18)
        .max_width(420);
        match authentication {
            Authentication::Checking => content = content.push(text("Opening your profile…")),
            Authentication::SignIn { error } => {
                let registering = self.login.registering;
                let submit = if registering {
                    Event::SubmitRegistration
                } else {
                    Event::SubmitPassword
                };
                content = content.push(shell::title(
                    if registering {
                        "Create a Matrix account"
                    } else {
                        "Sign in to Matrix"
                    },
                    20,
                ));
                if let Some(error) = error {
                    content = content.push(text(error.clone()).size(13).style(shell::danger_text));
                }
                content = content.push(text("Homeserver").size(13));
                let input = text_input("https://matrix.org", &self.login.homeserver)
                    .id("login-homeserver")
                    .cursor_blinking(false)
                    .style(shell::search_style)
                    .on_input(|value| Message::Login(Event::Homeserver(value)))
                    .on_submit(Message::Login(if registering {
                        Event::SubmitRegistration
                    } else {
                        Event::Submit
                    }))
                    .padding(12);
                let enabled = !self.login.homeserver.trim().is_empty()
                    && self.login.sender.is_some()
                    && !self.closing;
                content = content
                    .push(input)
                    .push(
                        text(if registering {
                            "Username"
                        } else {
                            "Username or Matrix ID"
                        })
                        .size(13),
                    )
                    .push(
                        text_input(
                            if registering {
                                "Username (without @ or server)"
                            } else {
                                "Username or Matrix ID"
                            },
                            &self.login.username,
                        )
                        .id("login-username")
                        .cursor_blinking(false)
                        .style(shell::search_style)
                        .on_input(|value| Message::Login(Event::Username(value)))
                        .padding(12),
                    )
                    .push(text("Password").size(13))
                    .push(
                        text_input("Password", &self.login.password)
                            .id("login-password")
                            .cursor_blinking(false)
                            .style(shell::search_style)
                            .secure(true)
                            .on_input(|value| {
                                Message::Login(Event::Password(Zeroizing::new(value)))
                            })
                            .on_submit(Message::Login(submit.clone()))
                            .padding(12),
                    );
                if registering {
                    content = content.push(text("Confirm password").size(13)).push(
                        text_input("Confirm password", &self.login.confirm_password)
                            .id("login-confirm-password")
                            .cursor_blinking(false)
                            .style(shell::search_style)
                            .secure(true)
                            .on_input(|value| {
                                Message::Login(Event::ConfirmPassword(Zeroizing::new(value)))
                            })
                            .on_submit(Message::Login(Event::SubmitRegistration))
                            .padding(12),
                    );
                    if !self.login.confirm_password.is_empty()
                        && self.login.password != self.login.confirm_password
                    {
                        content = content.push(
                            text("Passwords must match.")
                                .size(13)
                                .style(shell::danger_text),
                        );
                    }
                    if self.login.username.contains(['@', ':']) {
                        content = content.push(
                            text("Enter a username without @ or a server name.")
                                .size(13)
                                .style(shell::danger_text),
                        );
                    }
                }
                content = content.push(
                    button(if registering {
                        "Create account"
                    } else {
                        "Sign in"
                    })
                    .keyboard_navigation(true)
                    .padding([10, 16])
                    .style(shell::primary_button)
                    .on_press_maybe(
                        (enabled
                            && !self.login.username.trim().is_empty()
                            && !self.login.password.is_empty()
                            && (!registering || self.login.registration_ready()))
                        .then_some(Message::Login(submit)),
                    ),
                );
                if !registering {
                    content = content.push(
                        button("Continue in browser")
                            .keyboard_navigation(true)
                            .style(shell::quiet_button)
                            .padding([10, 16])
                            .on_press_maybe(enabled.then_some(Message::Login(Event::Submit))),
                    );
                }
                content = content
                    .push(
                        button(if registering {
                            "Already have an account? Sign in"
                        } else {
                            "New to Matrix? Create an account"
                        })
                        .keyboard_navigation(true)
                        .style(button::text)
                        .on_press_maybe(
                            (!self.closing).then_some(Message::Login(Event::ToggleRegistration)),
                        ),
                    )
                    .push(
                        text("Your session is saved securely in your system keyring.")
                            .size(12)
                            .style(shell::muted_text),
                    );
            }
            Authentication::Waiting { url } => {
                content = content.push(shell::title(
                    if self.login.registering {
                        "Creating your account"
                    } else {
                        "Signing in"
                    },
                    20,
                ));
                if url.is_some() {
                    content = content.push(text("Open the sign-in page and return here when you're done."))
                        .push(row![button("Open browser").keyboard_navigation(true).on_press(Message::Login(Event::OpenBrowser)).padding(10),
                            button("Copy sign-in link").keyboard_navigation(true).on_press(Message::Login(Event::CopyLink)).padding(10)].spacing(12))
                        .push(text("Sign-in expires after three minutes. In a container, forward localhost port 8765 to your browser's host.").size(12).style(shell::muted_text));
                } else {
                    content = content.push(text("Connecting to your homeserver…"));
                }
                content = content.push(
                    button(
                        text(if self.login.cancel_requested {
                            "Cancelling…"
                        } else if self.login.registering {
                            "Cancel account creation"
                        } else {
                            "Cancel sign-in"
                        })
                        .size(13),
                    )
                    .keyboard_navigation(true)
                    .padding([7, 14])
                    .style(shell::quiet_button)
                    .on_press_maybe(
                        (!self.closing
                            && !self.login.cancel_requested
                            && self.login.sender.is_some())
                        .then_some(Message::Login(Event::CancelSignIn)),
                    ),
                );
                if self.login.cancel_requested {
                    content = content.push(text("If sign-in already completed, it will finish safely. A device approved in your browser may need removing in another client.")
                        .size(12).style(shell::muted_text));
                }
            }
            Authentication::Registration(challenge) => {
                content = content
                    .push(shell::title("Finish creating your account", 20))
                    .push(text(challenge.stage.clone()));
                match &challenge.step {
                    RegistrationStep::Token { rejected } => {
                        let token = self.login.registration_token_for(challenge);
                        content = content.push(
                            text(
                                "Use the registration token provided by your server administrator.",
                            )
                            .size(14),
                        );
                        if *rejected {
                            content = content.push(text("The token was not accepted. Check it with your server administrator and try again.").size(13));
                        }
                        let prompt = challenge.clone();
                        content = content
                            .push(text("Registration token").size(13))
                            .push(
                                text_input("Registration token", token)
                                    .id("login-registration-token")
                                    .cursor_blinking(false)
                                    .style(shell::search_style)
                                    .secure(true)
                                    .on_input(move |value| {
                                        Message::Login(Event::RegistrationToken(
                                            prompt.clone(),
                                            Zeroizing::new(value),
                                        ))
                                    })
                                    .on_submit(Message::Login(Event::SubmitToken(
                                        challenge.clone(),
                                    )))
                                    .padding(12),
                            )
                            .push(
                                button("Continue")
                                    .keyboard_navigation(true)
                                    .padding(10)
                                    .on_press_maybe(
                                        (!self.closing && !token.trim().is_empty()).then_some(
                                            Message::Login(Event::SubmitToken(challenge.clone())),
                                        ),
                                    ),
                            );
                    }
                    RegistrationStep::Browser(_) => {
                        content = content
                    .push(text("Complete this step in your browser, then return here and continue. The server may ask for more than one step.").size(14))
                    .push(row![
                        button("Open browser").keyboard_navigation(true).on_press(Message::Login(Event::OpenBrowser)).padding(10),
                        button("Copy link").keyboard_navigation(true).on_press(Message::Login(Event::CopyLink)).padding(10),
                    ].spacing(12))
                    .push(button("Continue").keyboard_navigation(true).on_press_maybe((!self.closing).then_some(Message::Login(Event::RegistrationResponse(challenge.clone(), RegistrationReply::Continue)))).padding(10));
                    }
                }
                content = content
                    .push(
                        button("Cancel")
                            .keyboard_navigation(true)
                            .on_press_maybe((!self.closing).then_some(Message::Login(
                                Event::RegistrationResponse(
                                    challenge.clone(),
                                    RegistrationReply::Cancel,
                                ),
                            )))
                            .padding(10),
                    )
                    .push(
                        text("Registration expires after fifteen minutes.")
                            .size(12)
                            .style(shell::muted_text),
                    );
            }
            Authentication::Failed(error) => {
                content = content
                    .push(shell::title("Could not open profile", 20))
                    .push(text(error.clone()).size(13).style(shell::danger_text))
                    .push(
                        text("Resolve the profile or keyring problem, then reopen Interconnect. Existing account data is retained.")
                            .size(13),
                    )
                    .push(button("Close Interconnect").keyboard_navigation(true).padding([7, 14])
                        .style(shell::quiet_button).on_press_maybe((!self.closing).then_some(Message::Close)));
            }
        }
        if let Some(error) = &self.login.browser_error {
            content = content.push(text(error).size(13).style(shell::danger_text));
        }
        if self.closing {
            content = content.push(text("Closing…"));
        }
        let form = container(
            scrollable(container(content).padding(32).center_x(Length::Fill))
                .height(Length::Shrink),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill);
        if matches!(authentication, Authentication::Waiting { .. }) {
            super::focus_scope::dismissible(
                form,
                "login-waiting-focus",
                Message::Login(Event::CancelSignIn),
            )
        } else {
            super::focus_scope::view(form, "login-focus")
        }
    }
}

impl State {
    fn clear_registration_token(&mut self) {
        self.registration_token = Zeroizing::new(String::new());
        self.token_prompt = None;
    }

    fn registration_token_for(&self, prompt: &RegistrationChallenge) -> &str {
        if self.token_prompt.as_ref() == Some(prompt) {
            &self.registration_token
        } else {
            ""
        }
    }

    fn registration_ready(&self) -> bool {
        !self.username.trim().is_empty()
            && !self.username.contains(['@', ':'])
            && !self.password.is_empty()
            && self.password == self.confirm_password
    }
}

fn authentication_url(authentication: Option<&Authentication>) -> Option<&str> {
    match authentication {
        Some(Authentication::Waiting { url: Some(url) }) => Some(url),
        Some(Authentication::Registration(RegistrationChallenge {
            step: RegistrationStep::Browser(url),
            ..
        })) => Some(url),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interconnect::connected::Model;
    use matrix_sdk::encryption::VerificationState;

    fn form() -> (App, tokio::sync::mpsc::Receiver<LoginRequest>) {
        let mut app = App::new();
        app.model.authentication = Some(Authentication::SignIn { error: None });
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let _ = app.login_update(Event::Ready(sender));
        (app, receiver)
    }

    async fn verification_fixture() -> (
        interconnect::verification::Snapshot,
        impl std::future::Future<Output = ()>,
    ) {
        let (model, worker) = security_fixture().await;
        (model.verification, worker)
    }

    async fn security_fixture() -> (Model, impl std::future::Future<Output = ()>) {
        let client = matrix_sdk::Client::builder()
            .homeserver_url("https://example.org")
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value(serde_json::json!({
                    "user_id": "@fixture:example.org",
                    "device_id": "FIXTURE",
                    "access_token": "test-only-token"
                }))
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        let (state, snapshots) = tokio::sync::watch::channel(Model::default());
        let stop = tokio_util::sync::CancellationToken::new();
        let mut worker = Box::pin(async move {
            tokio::join!(
                interconnect::verification::Service::new(client.clone())
                    .run(state.clone(), stop.clone()),
                interconnect::recovery::run(client, state, stop),
            );
        });
        // Initialize the real command channel, then leave the worker unpolled.
        // UI commands stay local: no discovery, network, or sync is performed.
        assert!(iced::futures::poll!(&mut worker).is_pending());
        let snapshot = snapshots.borrow().clone();
        assert!(snapshot.verification.control.is_some());
        assert!(snapshot.recovery.control.is_some());
        (snapshot, worker)
    }

    #[tokio::test]
    async fn registration_offers_recovery_once_even_when_new_device_is_verified() {
        let (mut ready, _worker) = security_fixture().await;
        ready.authentication = None;
        ready.verification.trust = VerificationState::Verified;
        let (mut app, mut requests) = form();
        let _ = app.login_update(Event::ToggleRegistration);
        let _ = app.login_update(Event::Homeserver("https://example.org".into()));
        let _ = app.login_update(Event::Username("fixture".into()));
        let _ = app.login_update(Event::Password(Zeroizing::new("fixture-password".into())));
        let _ = app.login_update(Event::ConfirmPassword(Zeroizing::new(
            "fixture-password".into(),
        )));
        let _ = app.login_update(Event::SubmitRegistration);
        assert!(matches!(
            requests.try_recv(),
            Ok(LoginRequest::SignIn {
                method: LoginMethod::Register { .. },
                ..
            })
        ));
        let mut early = ready.clone();
        early.recovery = Default::default();
        let _ = app.update(Message::State(Box::new(early)));
        assert!(!app.verification.open);
        assert!(app.login.verify_after_login);
        let _ = app.update(Message::State(Box::new(ready.clone())));
        assert!(app.verification.open);
        assert!(app.verification.recovery.open);
        assert!(!app.login.verify_after_login);
        let _ = app.update(Message::Escape);
        assert!(!app.verification.open);
        let _ = app.update(Message::State(Box::new(ready)));
        assert!(!app.verification.open);
    }

    fn submit(app: &mut App, password: bool) {
        let _ = app.update(Message::Login(Event::Homeserver(
            "https://example.org".into(),
        )));
        let _ = app.update(Message::Login(Event::Username("fixture".into())));
        let _ = app.update(Message::Login(Event::Password(Zeroizing::new(
            "test-only-password".into(),
        ))));
        let _ = app.update(Message::Login(if password {
            Event::SubmitPassword
        } else {
            Event::Submit
        }));
    }

    #[tokio::test]
    async fn sign_in_opens_verification_once_when_worker_is_ready() {
        let (snapshot, _worker) = verification_fixture().await;
        for (password, trust) in [
            (true, VerificationState::Unknown),
            (false, VerificationState::Unverified),
        ] {
            let (mut app, mut requests) = form();
            submit(&mut app, password);
            assert!(requests.try_recv().is_ok());
            assert!(app.login.verify_after_login);
            // The authenticated snapshot may arrive before verification is ready.
            let _ = app.update(Message::State(Box::new(Model {
                authentication: None,
                account_id: Some(snapshot.user.clone()),
                ..Model::default()
            })));
            assert!(!app.verification.open);
            assert!(app.login.verify_after_login);
            let mut ready = app.model.clone();
            ready.verification = snapshot.clone();
            ready.verification.trust = trust;
            let _ = app.update(Message::State(Box::new(ready.clone())));
            assert!(app.verification.open);
            assert!(!app.login.verify_after_login);
            let _ = app.update(Message::Escape);
            assert!(!app.verification.open);
            let _ = app.update(Message::State(Box::new(ready)));
            assert!(
                !app.verification.open,
                "dismissal must survive later snapshots"
            );
        }
    }

    #[tokio::test]
    async fn restoration_failure_verified_sessions_and_shutdown_do_not_prompt() {
        let (snapshot, _worker) = verification_fixture().await;
        let ready = Model {
            authentication: None,
            account_id: Some(snapshot.user.clone()),
            verification: snapshot,
            ..Model::default()
        };
        // Restoring a session never submitted the sign-in form.
        let mut restored = App::new();
        let _ = restored.update(Message::State(Box::new(ready.clone())));
        assert!(!restored.verification.open);

        let (mut failed, mut requests) = form();
        submit(&mut failed, true);
        requests.try_recv().unwrap();
        let _ = failed.update(Message::State(Box::new(Model {
            authentication: Some(Authentication::SignIn {
                error: Some("Try again".into()),
            }),
            ..Model::default()
        })));
        assert!(!failed.login.verify_after_login);
        let _ = failed.update(Message::State(Box::new(ready.clone())));
        assert!(!failed.verification.open);
        // A fresh explicit retry arms onboarding again.
        failed.model.authentication = Some(Authentication::SignIn { error: None });
        submit(&mut failed, false);
        requests.try_recv().unwrap();
        let _ = failed.update(Message::State(Box::new(ready.clone())));
        assert!(failed.verification.open);

        let (mut verified, mut requests) = form();
        submit(&mut verified, true);
        requests.try_recv().unwrap();
        let mut trusted = ready.clone();
        trusted.verification.trust = VerificationState::Verified;
        let _ = verified.update(Message::State(Box::new(trusted)));
        assert!(!verified.verification.open);
        assert!(!verified.login.verify_after_login);
        let _ = verified.update(Message::State(Box::new(ready.clone())));
        assert!(!verified.verification.open);

        let (mut closing, mut requests) = form();
        submit(&mut closing, true);
        requests.try_recv().unwrap();
        let _ = closing.update(Message::Close);
        let _ = closing.update(Message::State(Box::new(ready)));
        assert!(!closing.verification.open);
        assert!(!closing.login.verify_after_login);
    }

    #[test]
    fn submit_is_trimmed_and_single_flight() {
        let (mut app, mut requests) = form();
        let _ = app.login_update(Event::Submit);
        assert!(requests.try_recv().is_err());
        let _ = app.login_update(Event::Homeserver("  https://matrix.example  ".into()));
        let _ = app.login_update(Event::Submit);
        assert!(
            matches!(requests.try_recv(), Ok(LoginRequest::SignIn { homeserver, method: LoginMethod::Browser }) if homeserver == "https://matrix.example")
        );
        let _ = app.login_update(Event::Submit);
        assert!(requests.try_recv().is_err());
        assert!(matches!(
            app.model.authentication,
            Some(Authentication::Waiting { url: None })
        ));
        // Inputs from an older form cannot alter an in-flight attempt.
        let _ = app.login_update(Event::Homeserver("https://other.example".into()));
        assert_eq!(app.login.homeserver.trim(), "https://matrix.example");
    }

    #[test]
    fn failed_attempt_can_retry_and_closing_prevents_login() {
        let (mut app, mut requests) = form();
        let _ = app.login_update(Event::Homeserver("https://matrix.example".into()));
        let _ = app.login_update(Event::Submit);
        requests.try_recv().unwrap();
        app.model.authentication = Some(Authentication::SignIn {
            error: Some("Timed out".into()),
        });
        let _ = app.login_update(Event::Submit);
        assert!(requests.try_recv().is_ok());
        app.model.authentication = Some(Authentication::SignIn { error: None });
        let _ = app.update(Message::Close);
        assert!(app.selection.shutdown);
        let _ = app.login_update(Event::Submit);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn password_submit_clears_the_form_secret_and_redacts_messages() {
        let (mut app, mut requests) = form();
        let _ = app.login_update(Event::Homeserver("https://matrix.example".into()));
        let _ = app.login_update(Event::Username(" fixture ".into()));
        let event = Event::Password(Zeroizing::new("fixture-secret".into()));
        assert!(!format!("{event:?}").contains("fixture-secret"));
        let _ = app.login_update(event);
        let _ = app.login_update(Event::SubmitPassword);
        assert!(app.login.password.is_empty());
        let request = requests.try_recv().unwrap();
        assert!(!format!("{request:?}").contains("fixture-secret"));
        assert!(matches!(request, LoginRequest::SignIn {
            method: LoginMethod::Password { username, password }, ..
        } if username == "fixture" && password.as_str() == "fixture-secret"));
    }

    #[test]
    fn registration_requires_confirmation_and_clears_secrets_on_submit_or_mode_change() {
        let (mut app, mut requests) = form();
        let _ = app.login_update(Event::ToggleRegistration);
        let _ = app.login_update(Event::Homeserver(" https://matrix.example ".into()));
        let _ = app.login_update(Event::Username(" fixture ".into()));
        let _ = app.login_update(Event::Password(Zeroizing::new("fixture-secret".into())));
        let confirmation = Event::ConfirmPassword(Zeroizing::new("other-secret".into()));
        assert!(!format!("{confirmation:?}").contains("other-secret"));
        let _ = app.login_update(confirmation);
        for event in [
            Event::Submit,
            Event::SubmitPassword,
            Event::SubmitRegistration,
        ] {
            let _ = app.login_update(event);
            assert!(requests.try_recv().is_err());
        }
        let _ = app.login_update(Event::ConfirmPassword(Zeroizing::new(
            "fixture-secret".into(),
        )));
        let _ = app.login_update(Event::Username("@fixture:example.org".into()));
        let _ = app.login_update(Event::SubmitRegistration);
        assert!(requests.try_recv().is_err());
        let _ = app.login_update(Event::Username(" fixture ".into()));
        let _ = app.login_update(Event::SubmitRegistration);
        let request = requests.try_recv().unwrap();
        let debug = format!("{request:?}");
        assert!(!debug.contains("fixture"));
        assert!(matches!(request, LoginRequest::SignIn {
            homeserver, method: LoginMethod::Register { username, password }
        } if homeserver == "https://matrix.example" && username == "fixture" && password.as_str() == "fixture-secret"));
        assert!(app.login.password.is_empty());
        assert!(app.login.confirm_password.is_empty());
        assert!(app.login.verify_after_login);
        let _ = app.login_update(Event::SubmitRegistration);
        let _ = app.login_update(Event::ToggleRegistration);
        assert!(requests.try_recv().is_err());
        assert!(app.login.registering);
        app.model.authentication = Some(Authentication::SignIn {
            error: Some("Try again".into()),
        });
        let _ = app.login_update(Event::Password(Zeroizing::new("fixture-secret".into())));
        let _ = app.login_update(Event::ConfirmPassword(Zeroizing::new(
            "fixture-secret".into(),
        )));
        let _ = app.login_update(Event::ToggleRegistration);
        assert!(!app.login.registering);
        assert!(app.login.password.is_empty());
        assert!(app.login.confirm_password.is_empty());
        assert_eq!(
            app.model.authentication,
            Some(Authentication::SignIn { error: None })
        );
    }

    fn challenge() -> (
        RegistrationChallenge,
        tokio::sync::mpsc::Receiver<RegistrationReply>,
    ) {
        let (response, replies) = tokio::sync::mpsc::channel(1);
        (RegistrationChallenge {
            step: RegistrationStep::Browser("https://matrix.example/_matrix/client/v3/auth/m.login.terms/fallback/web?session=fixture".into()),
            stage: "Review the server's terms".into(),
            response,
        }, replies)
    }

    fn token_challenge(
        rejected: bool,
    ) -> (
        RegistrationChallenge,
        tokio::sync::mpsc::Receiver<RegistrationReply>,
    ) {
        let (mut prompt, replies) = challenge();
        prompt.step = RegistrationStep::Token { rejected };
        prompt.stage = "Enter your registration token".into();
        (prompt, replies)
    }

    #[test]
    fn token_submission_is_native_redacted_and_scoped_to_the_current_prompt() {
        let (mut app, _) = form();
        let (current, mut replies) = token_challenge(false);
        let (old, mut old_replies) = token_challenge(false);
        app.model.authentication = Some(Authentication::Registration(current.clone()));
        assert!(authentication_url(app.model.authentication.as_ref()).is_none());
        let _ = app.login_update(Event::SubmitToken(current.clone()));
        let _ = app.login_update(Event::RegistrationResponse(
            current.clone(),
            RegistrationReply::Continue,
        ));
        assert!(replies.try_recv().is_err());
        let event = Event::RegistrationToken(
            current.clone(),
            Zeroizing::new("fixture-token-secret".into()),
        );
        assert!(!format!("{event:?}").contains("fixture-token-secret"));
        let _ = app.login_update(event);
        let _ = app.login_update(Event::RegistrationToken(
            old.clone(),
            Zeroizing::new("stale-secret".into()),
        ));
        let _ = app.login_update(Event::SubmitToken(old));
        assert!(old_replies.try_recv().is_err());
        assert_eq!(
            app.login.registration_token_for(&current),
            "fixture-token-secret"
        );
        let _ = app.login_update(Event::SubmitToken(current.clone()));
        let reply = replies.try_recv().unwrap();
        assert!(!format!("{reply:?}").contains("fixture-token-secret"));
        assert!(
            matches!(reply, RegistrationReply::Token(token) if token.as_str() == "fixture-token-secret")
        );
        assert!(app.login.registration_token.is_empty());
        assert!(app.login.token_prompt.is_none());
        let _ = app.login_update(Event::SubmitToken(current));
        assert!(replies.try_recv().is_err());
    }

    #[test]
    fn tokens_clear_when_cancelled_replaced_or_closing() {
        for action in 0..4 {
            let (mut app, _) = form();
            let (prompt, mut replies) = token_challenge(false);
            app.model.authentication = Some(Authentication::Registration(prompt.clone()));
            let _ = app.update(Message::Login(Event::RegistrationToken(
                prompt.clone(),
                Zeroizing::new("fixture-token-secret".into()),
            )));
            assert!(!app.login.registration_token.is_empty());
            match action {
                0 => {
                    let _ = app.update(Message::Login(Event::RegistrationResponse(
                        prompt,
                        RegistrationReply::Cancel,
                    )));
                    assert!(matches!(replies.try_recv(), Ok(RegistrationReply::Cancel)));
                }
                1 | 2 => {
                    let authentication = if action == 1 {
                        Authentication::Registration(token_challenge(true).0)
                    } else {
                        Authentication::SignIn {
                            error: Some("Expired".into()),
                        }
                    };
                    let _ = app.update(Message::State(Box::new(Model {
                        authentication: Some(authentication),
                        ..Model::default()
                    })));
                }
                _ => {
                    let _ = app.update(Message::Close);
                }
            }
            assert!(app.login.registration_token.is_empty());
            assert!(app.login.token_prompt.is_none());
        }
    }

    #[test]
    fn registration_replies_are_single_flight_and_reject_stale_prompts() {
        let (mut app, _) = form();
        let (old, mut old_replies) = challenge();
        let (current, mut replies) = challenge();
        app.model.authentication = Some(Authentication::Registration(current.clone()));
        let _ = app.login_update(Event::RegistrationResponse(
            old,
            RegistrationReply::Continue,
        ));
        assert!(old_replies.try_recv().is_err());
        assert!(replies.try_recv().is_err());
        let _ = app.login_update(Event::RegistrationResponse(
            current.clone(),
            RegistrationReply::Continue,
        ));
        assert!(matches!(
            replies.try_recv(),
            Ok(RegistrationReply::Continue)
        ));
        let _ = app.login_update(Event::RegistrationResponse(
            current.clone(),
            RegistrationReply::Cancel,
        ));
        assert!(replies.try_recv().is_err());
        app.model.authentication = Some(Authentication::Registration(current.clone()));
        app.closing = true;
        let _ = app.login_update(Event::RegistrationResponse(
            current.clone(),
            RegistrationReply::Continue,
        ));
        assert!(replies.try_recv().is_err());
        app.closing = false;
        let _ = app.login_update(Event::RegistrationResponse(
            current,
            RegistrationReply::Cancel,
        ));
        assert!(matches!(replies.try_recv(), Ok(RegistrationReply::Cancel)));
    }

    #[test]
    fn new_account_clears_previous_drafts_but_same_account_keeps_them() {
        use super::super::{Draft, DraftKey};
        let (mut app, _) = form();
        app.account_id = Some("@first:example.org".into());
        let key = DraftKey {
            room: "!room:example.org".into(),
            root: None,
        };
        app.drafts.insert(key.clone(), Draft::default());
        app.scroll_positions.insert(key.clone(), 100.0);
        let _ = app.update(Message::State(Box::new(interconnect::connected::Model {
            account_id: app.account_id.clone(),
            ..Default::default()
        })));
        assert!(app.drafts.contains_key(&key));
        let _ = app.update(Message::State(Box::new(interconnect::connected::Model {
            account_id: Some("@second:example.org".into()),
            ..Default::default()
        })));
        assert!(app.drafts.is_empty());
        assert!(app.scroll_positions.is_empty());
    }

    #[test]
    fn cancellation_waits_for_authoritative_state_and_rejects_repeated_requests() {
        let (mut app, mut requests) = form();
        submit(&mut app, false);
        assert!(matches!(
            requests.try_recv(),
            Ok(LoginRequest::SignIn { .. })
        ));
        let _ = app.login_update(Event::CancelSignIn);
        assert!(matches!(requests.try_recv(), Ok(LoginRequest::Cancel)));
        assert!(app.login.cancel_requested);
        assert!(matches!(
            app.model.authentication,
            Some(Authentication::Waiting { .. })
        ));
        let _ = app.login_update(Event::CancelSignIn);
        assert!(requests.try_recv().is_err());
        app.model.authentication = Some(Authentication::SignIn { error: None });
        let _ = app.finish_login();
        assert!(!app.login.cancel_requested);
        let _ = app.login_update(Event::CancelSignIn);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn cancellation_queue_failure_keeps_waiting_and_allows_explicit_retry() {
        let (mut app, mut requests) = form();
        submit(&mut app, false);
        let _ = app.login_update(Event::CancelSignIn);
        assert!(!app.login.cancel_requested);
        assert!(app.login.browser_error.is_some());
        assert!(matches!(
            app.model.authentication,
            Some(Authentication::Waiting { .. })
        ));
        assert!(matches!(
            requests.try_recv(),
            Ok(LoginRequest::SignIn { .. })
        ));
        let _ = app.login_update(Event::CancelSignIn);
        assert!(matches!(requests.try_recv(), Ok(LoginRequest::Cancel)));
        assert!(app.login.cancel_requested);
        assert!(app.login.browser_error.is_none());
    }

    #[test]
    fn focused_sign_in_and_registration_fields_do_not_schedule_caret_redraws() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let (mut app, _) = form();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        app.login.registering = true;
        for id in [
            "login-homeserver",
            "login-username",
            "login-password",
            "login-confirm-password",
        ] {
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let size = iced::Size::new(640.0, 480.0);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus(widget::Id::new(id)),
            );
            let now = iced::time::Instant::now();
            for millis in [0, 500, 1000, 1500] {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(
                        now + std::time::Duration::from_millis(millis),
                    )),
                    layout::Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                assert_eq!(
                    shell.redraw_request(),
                    iced::window::RedrawRequest::Wait,
                    "{id}"
                );
                assert!(messages.is_empty());
            }
        }
    }

    #[test]
    fn all_login_states_fit_the_minimum_window_and_settle_without_redraws() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let (mut app, _) = form();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for (registering, authentication) in [
            (true, Authentication::SignIn { error: None }),
            (true, Authentication::SignIn { error: Some("Registration could not finish. Check your homeserver and system keyring. If your account was created, return to sign in.".into()) }),
            (true, Authentication::Waiting { url: None }),
            (true, Authentication::Registration(challenge().0)),
            (true, Authentication::Registration(token_challenge(false).0)),
            (true, Authentication::Registration(token_challenge(true).0)),
        ].into_iter().chain([
            Authentication::Checking,
            Authentication::SignIn { error: None },
            Authentication::SignIn { error: Some("Sign-in could not finish. Check your credentials and homeserver, then try again.".into()) },
            Authentication::Waiting { url: None },
            Authentication::Waiting { url: Some("https://matrix.example/login".into()) },
            Authentication::Failed("System keyring is unavailable.".into()),
        ].into_iter().map(|authentication| (false, authentication))) {
            app.login.registering = registering;
            app.model.authentication = Some(authentication);
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let size = iced::Size::new(640.0, 480.0);
            let node = view.as_widget_mut().layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert_eq!(node.size(), size);
            for child in node.children() {
                let bounds = child.bounds();
                assert!(bounds.x >= 0.0 && bounds.y >= 0.0);
                assert!(bounds.x + bounds.width <= size.width);
                assert!(bounds.y + bounds.height <= size.height);
            }
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            assert!(messages.is_empty());
        }
    }

    #[test]
    fn stale_browser_results_do_not_change_the_current_attempt() {
        let (mut app, _) = form();
        app.model.authentication = Some(Authentication::Waiting {
            url: Some("https://current.example".into()),
        });
        let _ = app.login_update(Event::BrowserOpened(
            "https://previous.example".into(),
            Err("Old error".into()),
        ));
        assert!(app.login.browser_error.is_none());
        let _ = app.login_update(Event::BrowserOpened(
            "https://current.example".into(),
            Err("Open failed".into()),
        ));
        assert_eq!(app.login.browser_error.as_deref(), Some("Open failed"));
    }
}
