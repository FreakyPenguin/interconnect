//! Event-driven password and browser sign-in. SDK work stays in the connection owner.
use super::{App, Message, branding, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use interconnect::connected::{Authentication, LoginRequest};
use interconnect::session::LoginMethod;
use zeroize::Zeroizing;

#[derive(Default)]
pub struct State {
    pub homeserver: String,
    pub username: String,
    pub password: Zeroizing<String>,
    pub sender: Option<tokio::sync::mpsc::Sender<LoginRequest>>,
    pub browser_error: Option<String>,
}

#[derive(Clone)]
pub enum Event {
    Ready(tokio::sync::mpsc::Sender<LoginRequest>),
    Homeserver(String),
    Username(String),
    Password(Zeroizing<String>),
    SubmitPassword,
    Submit,
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
            Self::SubmitPassword => "SubmitPassword",
            Self::Submit => "Submit",
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
            Event::Submit | Event::SubmitPassword
                if !self.closing
                    && matches!(
                        self.model.authentication,
                        Some(Authentication::SignIn { .. })
                    ) =>
            {
                let homeserver = self.login.homeserver.trim();
                let method = if matches!(event, Event::SubmitPassword) {
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
                    self.login.browser_error = None;
                    self.model.authentication = Some(Authentication::Waiting { url: None });
                }
            }
            Event::OpenBrowser => {
                if let Some(Authentication::Waiting { url: Some(url) }) = &self.model.authentication
                {
                    let url = url.clone();
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
            Event::BrowserOpened(url, result) if matches!(&self.model.authentication, Some(Authentication::Waiting { url: Some(current) }) if current == &url) =>
            {
                self.login.browser_error = result.err();
            }
            Event::CopyLink => {
                if let Some(Authentication::Waiting { url: Some(url) }) = &self.model.authentication
                {
                    return iced::clipboard::write(url.clone());
                }
            }
            _ => {}
        }
        Task::none()
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
                content = content.push(shell::title("Sign in to Matrix", 20));
                if let Some(error) = error {
                    content = content.push(text(error.clone()).size(13));
                }
                content = content.push(text("Homeserver").size(13));
                let input = text_input("https://matrix.org", &self.login.homeserver)
                    .on_input(|value| Message::Login(Event::Homeserver(value)))
                    .on_submit(Message::Login(Event::Submit))
                    .padding(12);
                let enabled = !self.login.homeserver.trim().is_empty()
                    && self.login.sender.is_some()
                    && !self.closing;
                content = content
                    .push(input)
                    .push(
                        text_input("Username or Matrix ID", &self.login.username)
                            .on_input(|value| Message::Login(Event::Username(value)))
                            .padding(12),
                    )
                    .push(
                        text_input("Password", &self.login.password)
                            .secure(true)
                            .on_input(|value| {
                                Message::Login(Event::Password(Zeroizing::new(value)))
                            })
                            .on_submit(Message::Login(Event::SubmitPassword))
                            .padding(12),
                    )
                    .push(
                        button("Sign in").padding([10, 16]).on_press_maybe(
                            (enabled
                                && !self.login.username.trim().is_empty()
                                && !self.login.password.is_empty())
                            .then_some(Message::Login(Event::SubmitPassword)),
                        ),
                    )
                    .push(
                        button("Continue in browser")
                            .padding([10, 16])
                            .on_press_maybe(enabled.then_some(Message::Login(Event::Submit))),
                    )
                    .push(
                        text("Your session is saved securely in your system keyring.")
                            .size(12)
                            .style(shell::muted_text),
                    );
            }
            Authentication::Waiting { url } => {
                content = content.push(shell::title("Signing in", 20));
                if url.is_some() {
                    content = content.push(text("Open the sign-in page and return here when you're done."))
                        .push(row![button("Open browser").on_press(Message::Login(Event::OpenBrowser)).padding(10),
                            button("Copy sign-in link").on_press(Message::Login(Event::CopyLink)).padding(10)].spacing(12))
                        .push(text("Sign-in expires after three minutes. In a container, forward localhost port 8765 to your browser's host.").size(12).style(shell::muted_text));
                } else {
                    content = content.push(text("Connecting to your homeserver…"));
                }
            }
            Authentication::Failed(error) => {
                content = content
                    .push(shell::title("Could not open profile", 20))
                    .push(text(error.clone()).size(13))
                    .push(
                        text("Resolve the profile or keyring problem and reopen Interconnect.")
                            .size(13),
                    );
            }
        }
        if let Some(error) = &self.login.browser_error {
            content = content.push(text(error).size(13));
        }
        if self.closing {
            content = content.push(text("Closing…"));
        }
        container(
            scrollable(container(content).padding(32).center_x(Length::Fill))
                .height(Length::Shrink),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> (App, tokio::sync::mpsc::Receiver<LoginRequest>) {
        let mut app = App::new();
        app.model.authentication = Some(Authentication::SignIn { error: None });
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let _ = app.login_update(Event::Ready(sender));
        (app, receiver)
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
    fn all_login_states_fit_the_minimum_window_and_settle_without_redraws() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let (mut app, _) = form();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for authentication in [
            Authentication::Checking,
            Authentication::SignIn { error: None },
            Authentication::SignIn { error: Some("Sign-in could not finish. Check your credentials and homeserver, then try again.".into()) },
            Authentication::Waiting { url: None },
            Authentication::Waiting { url: Some("https://matrix.example/login".into()) },
            Authentication::Failed("System keyring is unavailable.".into()),
        ] {
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
