//! On-demand own-device settings. Submitted writes drain even after the panel closes.
use super::{App, MAX_PENDING_OPERATIONS, Message, shell};
use iced::{
    Element, Task,
    widget::{button, column, row, text, text_input},
};
use interconnect::device_management::{Challenge, Device, Reply, Revocation, Step};
use std::collections::HashMap;
use zeroize::Zeroizing;

#[derive(Default)]
pub(super) struct State {
    account: Option<String>,
    devices: Option<Vec<Device>>,
    request: u64,
    load: Option<iced::task::Handle>,
    writes: HashMap<u64, Write>,
    form: Option<Form>,
    error: Option<String>,
    reload_required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{settings, tests::demo_app};
    use matrix_sdk::{Client, authentication::matrix::MatrixSession};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    fn open() -> App {
        let mut app = demo_app();
        let _ = app.update(Message::Settings(settings::Event::Open));
        let _ = app.update(Message::Settings(settings::Event::Category(
            settings::Category::Devices,
        )));
        app
    }
    fn serial(app: &App) -> u64 {
        app.devices.form.as_ref().unwrap().serial
    }
    fn pending(app: &mut App) -> u64 {
        let form = app.devices.form.as_ref().unwrap();
        let write = Write {
            account: app.account_id.clone(),
            form: form.serial,
            id: form.id.clone(),
        };
        app.generation += 1;
        let operation = app.generation;
        app.devices.writes.insert(operation, write);
        app.pending_ops += 1;
        operation
    }
    async fn password_challenge() -> Challenge {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/devices"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"devices":[{"device_id":"CURRENT"},{"device_id":"DEMO-PHONE"}]}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/delete_devices"))
            .respond_with(ResponseTemplate::new(401).set_body_json(
                json!({"session":"hidden-session","flows":[{"stages":["m.login.password"]}]}),
            ))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .request_config(matrix_sdk::config::RequestConfig::default().disable_retry())
            .build()
            .await
            .unwrap();
        client.matrix_auth().restore_session(serde_json::from_value::<MatrixSession>(json!({"user_id":"@test:example.org","device_id":"CURRENT","access_token":"fixture"})).unwrap(), Default::default()).await.unwrap();
        let Revocation::Authenticate(challenge) = interconnect::people::Directory::new(client)
            .revoke_device("DEMO-PHONE".into(), None)
            .await
            .unwrap()
        else {
            panic!()
        };
        challenge
    }
    #[test]
    fn demo_devices_require_confirmation_protect_current_and_preserve_conversation() {
        let mut app = open();
        let selected = app.selection.room.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 75.0);
        assert_eq!(app.devices.devices.as_ref().unwrap().len(), 3);
        let _ = app.update(message(Event::Revoke("DEMO-CURRENT".into())));
        assert!(app.devices.form.is_none());
        let _ = app.update(message(Event::Rename("DEMO-CURRENT".into())));
        let edit = serial(&app);
        let _ = app.update(message(Event::Name(edit, "bad\nname".into())));
        let _ = app.update(message(Event::Submit(edit)));
        assert!(app.devices.error.is_some());
        let _ = app.update(message(Event::Name(edit, "My desktop".into())));
        let _ = app.update(message(Event::Submit(edit)));
        assert_eq!(app.devices.devices.as_ref().unwrap()[0].name, "My desktop");
        assert!(app.devices.error.is_none());
        let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
        let confirm = serial(&app);
        assert_eq!(app.devices.devices.as_ref().unwrap().len(), 3);
        let _ = app.update(message(Event::Cancel(confirm)));
        let _ = app.update(message(Event::Submit(confirm)));
        assert_eq!(app.devices.devices.as_ref().unwrap().len(), 3);
        let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
        let confirm = serial(&app);
        let _ = app.update(message(Event::Submit(confirm)));
        assert_eq!(app.devices.devices.as_ref().unwrap().len(), 2);
        assert_eq!(app.selection.room, selected);
        assert_eq!(app.drafts[&key].content.text(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 75.0);
        assert_eq!(app.pending_ops, 0);
        assert!(app.settings.path.is_none());
    }
    #[test]
    fn stale_loads_forms_and_old_account_writes_are_rejected_but_drain() {
        let mut app = open();
        let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
        let old = serial(&app);
        let _ = app.update(message(Event::Cancel(old)));
        let _ = app.update(message(Event::Revoke("DEMO-LAPTOP".into())));
        let current = serial(&app);
        let _ = app.update(message(Event::Submit(old)));
        assert_eq!(serial(&app), current);
        let operation = pending(&mut app);
        app.account_id = Some("@other:example.org".into());
        let _ = app.update(message(Event::Saved(
            operation,
            Err("old-account-error".into()),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(app.devices.form.is_none());
        assert!(app.devices.devices.is_none());
        assert!(app.devices.error.is_none());
        let _ = app.update(message(Event::Saved(operation, Err("duplicate".into()))));
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Loaded(999, Some("old".into()), Ok(vec![]))));
        assert!(app.devices.devices.is_none());
    }
    #[tokio::test]
    async fn challenges_and_passwords_clear_when_covered_and_stale_auth_events_cannot_submit() {
        for cover in 0..4 {
            let mut app = open();
            let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
            let operation = pending(&mut app);
            let _ = app.update(message(Event::Saved(
                operation,
                Ok(Revocation::Authenticate(password_challenge().await)),
            )));
            let auth = serial(&app);
            let _ = app.update(message(Event::Password(
                auth,
                Zeroizing::new("secret".into()),
            )));
            assert_eq!(
                app.devices.form.as_ref().unwrap().password.as_str(),
                "secret"
            );
            match cover {
                0 => {
                    let _ = app.update(Message::Settings(settings::Event::Category(
                        settings::Category::General,
                    )));
                }
                1 => {
                    app.verification.open = true;
                }
                2 => {
                    app.membership.open = true;
                }
                _ => {
                    let _ = app.update(Message::Settings(settings::Event::Cancel));
                }
            }
            let _ = app.update(message(Event::Submit(auth)));
            assert!(app.devices.form.is_none());
            assert_eq!(app.pending_ops, 0);
            let _ = app.update(message(Event::Password(
                auth,
                Zeroizing::new("late".into()),
            )));
            assert!(app.devices.form.is_none());
        }
    }
    #[tokio::test]
    async fn closed_panel_does_not_reopen_for_auth_completion_and_network_writes_still_drain() {
        let mut app = open();
        let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
        let operation = pending(&mut app);
        let _ = app.update(Message::Settings(settings::Event::Cancel));
        assert_eq!(app.pending_ops, 1);
        let _ = app.update(message(Event::Saved(
            operation,
            Ok(Revocation::Authenticate(password_challenge().await)),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(app.settings.draft.is_none());
        assert!(app.devices.form.is_none());
    }
    #[tokio::test]
    async fn demo_never_launches_browser_even_for_an_injected_challenge() {
        let mut app = open();
        let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
        let operation = pending(&mut app);
        let mut challenge = password_challenge().await;
        challenge.step = Step::Browser;
        let _ = app.update(message(Event::Saved(
            operation,
            Ok(Revocation::Authenticate(challenge)),
        )));
        let auth = serial(&app);
        let _ = app.update(message(Event::OpenBrowser(auth)));
        assert!(!app.devices.form.as_ref().unwrap().browser_busy);
        assert!(!app.devices.form.as_ref().unwrap().browser_opened);
    }
    #[test]
    fn devices_remain_available_over_search_and_notification_underlays() {
        for search in [false, true] {
            let mut app = demo_app();
            app.global_search.open = search;
            app.notification_center.open = !search;
            let _ = app.update(Message::Settings(settings::Event::Open));
            let _ = app.update(Message::Settings(settings::Event::Category(
                settings::Category::Devices,
            )));
            assert_eq!(app.devices.devices.as_ref().unwrap().len(), 3);
            let _ = app.update(message(Event::Rename("DEMO-CURRENT".into())));
            assert!(app.devices.form.is_some());
        }
    }
    #[tokio::test]
    async fn a_new_auth_stage_rejects_old_password_and_submit_events() {
        let mut app = open();
        let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
        let operation = pending(&mut app);
        let challenge = password_challenge().await;
        let _ = app.update(message(Event::Saved(
            operation,
            Ok(Revocation::Authenticate(challenge.clone())),
        )));
        let old = serial(&app);
        let _ = app.update(message(Event::Password(
            old,
            Zeroizing::new("first-secret".into()),
        )));
        let operation = pending(&mut app);
        let _ = app.update(message(Event::Saved(
            operation,
            Ok(Revocation::Authenticate(challenge)),
        )));
        assert_ne!(old, serial(&app));
        let _ = app.update(message(Event::Password(
            old,
            Zeroizing::new("stale-secret".into()),
        )));
        let _ = app.update(message(Event::Submit(old)));
        assert!(app.devices.form.as_ref().unwrap().password.is_empty());
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn ambiguous_failures_require_refresh_and_debug_redacts_input() {
        let mut app = open();
        let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
        let operation = pending(&mut app);
        let _ = app.update(message(Event::Saved(
            operation,
            Err("Refresh before retrying.".into()),
        )));
        assert!(app.devices.reload_required);
        assert!(app.devices.form.is_none());
        let _ = app.update(message(Event::Revoke("DEMO-PHONE".into())));
        assert!(app.devices.form.is_none());
        let _ = app.update(message(Event::Open));
        assert!(!app.devices.reload_required);
        assert!(
            !format!(
                "{:?}",
                Event::Password(1, Zeroizing::new("private-password".into()))
            )
            .contains("private-password")
        );
    }
}
struct Form {
    serial: u64,
    id: String,
    name: String,
    revoke: bool,
    challenge: Option<Challenge>,
    password: Zeroizing<String>,
    browser_opened: bool,
    browser_busy: bool,
}
struct Write {
    account: Option<String>,
    form: u64,
    id: String,
}
impl State {
    pub(super) fn dismiss(&mut self) {
        self.load = None;
        self.request = 0;
        self.form = None;
    }
}
#[derive(Clone)]
pub enum Event {
    Open,
    Loaded(u64, Option<String>, Result<Vec<Device>, String>),
    Rename(String),
    Revoke(String),
    Name(u64, String),
    Password(u64, Zeroizing<String>),
    Submit(u64),
    Cancel(u64),
    Saved(u64, Result<Revocation, String>),
    OpenBrowser(u64),
    BrowserOpened(u64, bool),
}
impl std::fmt::Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Open => "DevicesOpen",
            Self::Loaded(..) => "DevicesLoaded",
            Self::Rename(..) => "DeviceRename",
            Self::Revoke(..) => "DeviceRevoke",
            Self::Name(..) => "DeviceName",
            Self::Password(..) => "DevicePassword(<redacted>)",
            Self::Submit(..) => "DeviceSubmit",
            Self::Cancel(..) => "DeviceCancel",
            Self::Saved(..) => "DeviceSaved(<redacted>)",
            Self::OpenBrowser(..) => "DeviceOpenBrowser",
            Self::BrowserOpened(..) => "DeviceBrowserOpened",
        })
    }
}
fn message(event: Event) -> Message {
    Message::Devices(event)
}
impl App {
    pub(super) fn devices_available(&self) -> bool {
        self.devices_settings_available()
            && !self.activation_picker_open()
            && !self.sign_out_open
            && !self.verification.open
            && self.emoji_management.is_none()
            && self.image_view.is_none()
            && !self.membership.open
            && !self.draft_persistence.close_requested
            && (self.demo.is_some() || self.model.authentication.is_none())
    }
    pub(super) fn validate_devices(&mut self) {
        if !self.devices_available() || self.devices.account != self.account_id {
            self.devices.dismiss();
            if self.devices.account != self.account_id {
                self.devices.devices = None;
                self.devices.error = None;
            }
        }
    }
    pub(super) fn devices_event(&mut self, event: Event) -> Task<Message> {
        self.validate_devices();
        match event {
            Event::Loaded(request, account, result) => {
                if request != 0
                    && self.devices.request == request
                    && self.account_id == account
                    && self.devices_available()
                {
                    self.devices.load = None;
                    match result {
                        Ok(devices) => {
                            self.devices.devices = Some(devices);
                            self.devices.error = None;
                            self.devices.reload_required = false;
                        }
                        Err(error) => {
                            self.devices.error = Some(error);
                            self.devices.reload_required = true;
                        }
                    }
                }
                return Task::none();
            }
            Event::Saved(operation, result) => {
                let Some(write) = self.devices.writes.remove(&operation) else {
                    return Task::none();
                };
                self.pending_ops = self.pending_ops.saturating_sub(1);
                if self.devices_available()
                    && self.account_id == write.account
                    && self
                        .devices
                        .form
                        .as_ref()
                        .is_some_and(|form| form.serial == write.form && form.id == write.id)
                {
                    match result {
                        Ok(Revocation::Authenticate(challenge)) => {
                            self.generation += 1;
                            let form = self.devices.form.as_mut().unwrap();
                            form.serial = self.generation;
                            form.challenge = Some(challenge);
                            form.password = Zeroizing::new(String::new());
                            form.browser_opened = false;
                            form.browser_busy = false;
                        }
                        Ok(Revocation::Complete) => {
                            self.devices.form = None;
                            self.devices.reload_required = true;
                            return self.devices_event(Event::Open);
                        }
                        Err(error) => {
                            self.devices.form = None;
                            self.devices.error = Some(error);
                            self.devices.reload_required = true;
                        }
                    }
                } else if self.account_id == write.account {
                    self.devices.reload_required = true;
                }
                return if self.closing && self.pending_ops == 0 {
                    self.update(Message::Close)
                } else {
                    Task::none()
                };
            }
            _ => {}
        }
        if !self.devices_available() {
            return Task::none();
        }
        if matches!(event, Event::Open) && self.devices.writes.is_empty() {
            self.devices.dismiss();
            self.devices.account = self.account_id.clone();
            self.devices.error = None;
            self.devices.reload_required = true;
            if self.demo.is_some() {
                self.devices.devices.get_or_insert_with(|| {
                    vec![
                        Device {
                            id: "DEMO-CURRENT".into(),
                            name: "Interconnect · this desktop".into(),
                            current: true,
                            last_seen_ms: None,
                        },
                        Device {
                            id: "DEMO-PHONE".into(),
                            name: "Phone".into(),
                            current: false,
                            last_seen_ms: Some(1_790_000_000_000),
                        },
                        Device {
                            id: "DEMO-LAPTOP".into(),
                            name: "Work laptop".into(),
                            current: false,
                            last_seen_ms: None,
                        },
                    ]
                });
                self.devices.reload_required = false;
                return Task::none();
            }
            let Some(directory) = self.model.people_directory.clone() else {
                self.devices.error = Some("The account is not ready yet.".into());
                return Task::none();
            };
            self.generation += 1;
            let request = self.generation;
            self.devices.request = request;
            let account = self.account_id.clone();
            let (task, handle) = Task::perform(directory.devices(), move |result| {
                message(Event::Loaded(request, account.clone(), result))
            })
            .abortable();
            self.devices.load = Some(handle.abort_on_drop());
            return task;
        }
        if self.devices.account != self.account_id {
            return Task::none();
        }
        let ready = self.devices.writes.is_empty()
            && self.devices.load.is_none()
            && !self.devices.reload_required;
        let revoking = matches!(&event, Event::Revoke(_));
        match event {
            Event::Rename(id) | Event::Revoke(id) if ready && self.devices.form.is_none() => {
                let revoke = revoking;
                let Some(device) = self.devices.devices.as_ref().and_then(|devices| {
                    devices
                        .iter()
                        .find(|device| device.id == id && (!revoke || !device.current))
                }) else {
                    return Task::none();
                };
                self.generation += 1;
                self.devices.form = Some(Form {
                    serial: self.generation,
                    id,
                    name: device.name.clone(),
                    revoke,
                    challenge: None,
                    password: Zeroizing::new(String::new()),
                    browser_opened: false,
                    browser_busy: false,
                });
                self.devices.error = None;
            }
            Event::Name(serial, name) if ready => {
                if let Some(form) = self
                    .devices
                    .form
                    .as_mut()
                    .filter(|form| form.serial == serial && !form.revoke)
                {
                    form.name = name;
                    self.devices.error = None;
                }
            }
            Event::Password(serial, password) if ready => {
                if let Some(form) = self.devices.form.as_mut().filter(|form| {
                    form.serial == serial
                        && matches!(
                            form.challenge.as_ref().map(|c| &c.step),
                            Some(Step::Password { .. })
                        )
                }) {
                    form.password = password;
                }
            }
            Event::Cancel(serial)
                if self
                    .devices
                    .form
                    .as_ref()
                    .is_some_and(|form| form.serial == serial) =>
            {
                self.devices.form = None;
            }
            Event::Submit(serial) if ready && self.pending_ops < MAX_PENDING_OPERATIONS => {
                let Some(form) = self
                    .devices
                    .form
                    .as_mut()
                    .filter(|form| form.serial == serial)
                else {
                    return Task::none();
                };
                if !form.revoke
                    && let Err(error) = interconnect::device_management::validate_name(&form.name)
                {
                    self.devices.error = Some(error);
                    return Task::none();
                }
                let auth = match form.challenge.take() {
                    Some(challenge) => {
                        let reply = match challenge.step {
                            Step::Password { .. } if !form.password.is_empty() => {
                                Reply::Password(std::mem::replace(
                                    &mut form.password,
                                    Zeroizing::new(String::new()),
                                ))
                            }
                            Step::Browser if form.browser_opened => Reply::BrowserContinue,
                            _ => {
                                form.challenge = Some(challenge);
                                return Task::none();
                            }
                        };
                        Some((challenge, reply))
                    }
                    None => None,
                };
                let id = form.id.clone();
                let name = form.name.clone();
                let revoke = form.revoke;
                // Recheck against the current list, including current-device protection.
                if !self.devices.devices.as_ref().is_some_and(|devices| {
                    devices
                        .iter()
                        .any(|device| device.id == id && (!revoke || !device.current))
                }) {
                    return Task::none();
                }
                if self.demo.is_some() {
                    let devices = self.devices.devices.as_mut().unwrap();
                    if revoke {
                        devices.retain(|device| device.id != id);
                    } else if let Some(device) = devices.iter_mut().find(|device| device.id == id) {
                        device.name = name.trim().into();
                    }
                    self.devices.form = None;
                    return Task::none();
                }
                let Some(directory) = self.model.people_directory.clone() else {
                    self.devices.form = None;
                    self.devices.error = Some("The account is not ready yet.".into());
                    return Task::none();
                };
                self.generation += 1;
                let operation = self.generation;
                self.devices.writes.insert(
                    operation,
                    Write {
                        account: self.account_id.clone(),
                        form: serial,
                        id: id.clone(),
                    },
                );
                self.pending_ops += 1;
                return Task::perform(
                    async move {
                        if revoke {
                            directory.revoke_device(id, auth).await
                        } else {
                            directory
                                .rename_device(id, name)
                                .await
                                .map(|()| Revocation::Complete)
                        }
                    },
                    move |result| message(Event::Saved(operation, result)),
                );
            }
            Event::OpenBrowser(serial) if ready && self.demo.is_none() => {
                let Some(form) = self
                    .devices
                    .form
                    .as_mut()
                    .filter(|form| form.serial == serial && !form.browser_busy)
                else {
                    return Task::none();
                };
                let Some(url) = form
                    .challenge
                    .as_ref()
                    .and_then(Challenge::browser_url)
                    .map(str::to_owned)
                else {
                    return Task::none();
                };
                form.browser_busy = true;
                self.devices.error = None;
                return Task::perform(
                    async move {
                        #[cfg(target_os = "macos")]
                        let program = "open";
                        #[cfg(not(target_os = "macos"))]
                        let program = "xdg-open";
                        tokio::time::timeout(
                            std::time::Duration::from_secs(15),
                            tokio::process::Command::new(program)
                                .arg(url)
                                .kill_on_drop(true)
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status(),
                        )
                        .await
                        .is_ok_and(|status| status.is_ok_and(|status| status.success()))
                    },
                    move |ok| message(Event::BrowserOpened(serial, ok)),
                );
            }
            Event::BrowserOpened(serial, ok) => {
                if let Some(form) = self
                    .devices
                    .form
                    .as_mut()
                    .filter(|form| form.serial == serial)
                {
                    form.browser_busy = false;
                    form.browser_opened = ok;
                    if !ok {
                        self.devices.error = Some("Could not open your browser. Try again or use another client to manage this device.".into());
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub(super) fn devices_view(&self, enabled: bool) -> Element<'_, Message> {
        let state = &self.devices;
        let ready = enabled && state.load.is_none() && state.writes.is_empty();
        let mut content = column![shell::title("Devices & sessions", 14), text("Changes apply immediately. Revoking signs out another device; it does not erase data stored on it. Use Sign out for this device.").size(12).style(shell::muted_text), button(text(if state.load.is_some() { "Loading…" } else { "Refresh devices" }).size(12)).keyboard_navigation(true).style(shell::quiet_button).on_press_maybe(ready.then_some(message(Event::Open)))].spacing(12);
        if let Some(error) = &state.error {
            content = content.push(text(error).size(12).style(shell::danger_text));
        }
        if state.reload_required && state.load.is_none() {
            content = content.push(
                text("Refresh devices before making more changes.")
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        if state.account != self.account_id {
            return content.into();
        }
        if let Some(form) = &state.form {
            let serial = form.serial;
            let active = ready && !state.reload_required;
            content = content.push(text(format!("{} · {}", form.name, form.id)).size(13));
            let label = if !form.revoke {
                content = content.push(
                    text_input("Device name", &form.name)
                        .on_input_maybe(
                            active.then_some(move |name| message(Event::Name(serial, name))),
                        )
                        .size(13)
                        .padding(8)
                        .cursor_blinking(false),
                );
                "Rename device"
            } else if let Some(challenge) = &form.challenge {
                match challenge.step {
                    Step::Password { rejected } => {
                        content = content.push(
                            text(if rejected {
                                "Authentication failed. Enter your account password again."
                            } else {
                                "Enter your account password to revoke this device."
                            })
                            .size(12),
                        );
                        content = content.push(
                            text_input("Account password", &form.password)
                                .secure(true)
                                .cursor_blinking(false)
                                .padding(8)
                                .on_input_maybe(active.then_some(move |password| {
                                    message(Event::Password(serial, Zeroizing::new(password)))
                                })),
                        );
                    }
                    Step::Browser => {
                        content = content.push(text("Complete the server’s authentication step in your browser, then continue here.").size(12));
                        content = content.push(
                            button(if form.browser_busy {
                                "Opening browser…"
                            } else {
                                "Open authentication in browser"
                            })
                            .keyboard_navigation(true)
                            .on_press_maybe(
                                (active && !form.browser_busy)
                                    .then_some(message(Event::OpenBrowser(serial))),
                            ),
                        );
                    }
                }
                "Continue revoking"
            } else {
                content = content.push(text("Revoke this device and sign it out?").size(13));
                "Revoke device"
            };
            let can_submit = active
                && !form.browser_busy
                && match form.challenge.as_ref().map(|c| &c.step) {
                    Some(Step::Password { .. }) => !form.password.is_empty(),
                    Some(Step::Browser) => form.browser_opened,
                    None => {
                        form.revoke
                            || interconnect::device_management::validate_name(&form.name).is_ok()
                    }
                };
            content = content.push(
                row![
                    button(label)
                        .keyboard_navigation(true)
                        .style(if form.revoke {
                            button::danger
                        } else {
                            button::primary
                        })
                        .on_press_maybe(can_submit.then_some(message(Event::Submit(serial)))),
                    button("Cancel")
                        .keyboard_navigation(true)
                        .style(shell::quiet_button)
                        .on_press(message(Event::Cancel(serial)))
                ]
                .spacing(8),
            );
        } else if let Some(devices) = &state.devices {
            if devices.is_empty() {
                content = content.push(text("No devices returned by the server.").size(12));
            }
            for device in devices {
                let id = device.id.clone();
                let revoke_id = id.clone();
                let mut item = column![
                    text(if device.name.is_empty() {
                        "Unnamed device"
                    } else {
                        &device.name
                    })
                    .size(14),
                    text(format!(
                        "{}{}",
                        device.id,
                        if device.current {
                            " · This device"
                        } else {
                            ""
                        }
                    ))
                    .size(12)
                    .style(shell::muted_text)
                ]
                .spacing(5);
                if let Some(ms) = device
                    .last_seen_ms
                    .and_then(|ms| i64::try_from(ms).ok())
                    .and_then(chrono::DateTime::from_timestamp_millis)
                {
                    item = item.push(
                        text(format!("Last seen {}", ms.format("%Y-%m-%d %H:%M UTC")))
                            .size(12)
                            .style(shell::muted_text),
                    );
                }
                let active = ready && !state.reload_required;
                item = item.push(
                    row![
                        button("Rename…")
                            .keyboard_navigation(true)
                            .style(shell::quiet_button)
                            .on_press_maybe(active.then_some(message(Event::Rename(id)))),
                        button("Revoke…")
                            .keyboard_navigation(true)
                            .style(shell::quiet_button)
                            .on_press_maybe(
                                (active && !device.current)
                                    .then_some(message(Event::Revoke(revoke_id)))
                            )
                    ]
                    .spacing(8),
                );
                content = content.push(item);
            }
        }
        content.into()
    }
}
