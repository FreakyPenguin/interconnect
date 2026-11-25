//! External launches use the same guarded Matrix navigation as in-app links.
use super::{App, Message, shell};
use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, row, scrollable, text},
};
use interconnect::activation::{self, Claim, Handle, Registry, Request, Server, Target};

pub(super) enum Launch {
    Forwarded,
    Profile {
        profile: String,
        registry: Registry,
        server: Server,
        request: Option<Request>,
    },
    Picker(Picker),
}

pub(super) async fn prepare(arguments: &super::cli::Arguments) -> anyhow::Result<Launch> {
    let registry = Registry::system()?;
    prepare_in(arguments, registry).await
}
async fn prepare_in(
    arguments: &super::cli::Arguments,
    registry: Registry,
) -> anyhow::Result<Launch> {
    let link = arguments.external_link()?;
    if !arguments.explicit_profile() && (link.is_some() || cfg!(target_os = "macos")) {
        let targets = match registry.discover().await {
            Ok(targets) => targets,
            Err(_) => return Ok(Launch::Picker(Picker::new(registry, Request::new(link), Vec::new(), Some("An active profile is unavailable. Refresh the list before choosing a destination.".into())))),
        };
        if link.is_some() && targets.len() == 1 {
            let request = Request::new(link);
            return match registry.send(&targets[0], &request).await {
                Ok(()) => Ok(Launch::Forwarded),
                Err(error) => Ok(Launch::Picker(Picker::new(
                    registry,
                    request,
                    targets,
                    Some(error.into()),
                ))),
            };
        }
        if !targets.is_empty() {
            return Ok(Launch::Picker(Picker::new(
                registry,
                Request::new(link),
                targets,
                None,
            )));
        }
    }
    let profile = activation::canonical_profile(&arguments.profile_directory()?)?;
    let request = Request::new(link);
    match registry.claim(&profile).await? {
        Claim::Existing(target) => match registry.send(&target, &request).await {
            Ok(()) => Ok(Launch::Forwarded),
            Err(error) => Ok(Launch::Picker(Picker::new(
                registry,
                request,
                vec![target],
                Some(error.into()),
            ))),
        },
        Claim::Owner(server) => Ok(Launch::Profile {
            profile,
            registry,
            server,
            request: request.has_link().then_some(request),
        }),
    }
}

#[derive(Default)]
pub(super) struct State {
    pub handle: Option<Handle>,
    pub native: Option<Handle>,
    pub registry: Option<Registry>,
    picker: Option<Picker>,
    routing: bool,
    routing_token: u64,
    dispatching: bool,
}
#[derive(Clone, Debug)]
pub enum Event {
    Wake,
    NativeWake,
    Discovered(u64, Request, Result<Vec<Target>, String>),
    Picker(u64, PickerEvent),
}

impl App {
    pub(super) fn activation_subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            self.activation
                .handle
                .as_ref()
                .map_or_else(Subscription::none, |handle| {
                    Subscription::run_with(handle.clone(), |handle| {
                        let handle = handle.clone();
                        iced::stream::channel(1, async move |mut output| {
                            use iced::futures::SinkExt;
                            loop {
                                handle.notified().await;
                                if output.send(Message::Activation(Event::Wake)).await.is_err() {
                                    break;
                                }
                            }
                        })
                    })
                }),
            native_subscription(self.activation.native.as_ref())
                .map(|_| Message::Activation(Event::NativeWake)),
        ])
    }
    pub(super) fn activation_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Wake | Event::NativeWake => {
                return if self.closing || self.stopped {
                    Task::none()
                } else {
                    focus()
                };
            }
            Event::Discovered(token, request, result) => {
                if !self.activation.routing || self.activation.routing_token != token {
                    return Task::none();
                }
                self.activation.routing = false;
                if self.closing || self.stopped {
                    return Task::none();
                }
                let Some(registry) = self.activation.registry.clone() else {
                    return Task::none();
                };
                match result {
                    Ok(targets) => {
                        let auto = targets.len() <= 1;
                        self.activation.picker =
                            Some(Picker::new(registry, request, targets, None));
                        if auto {
                            let event = self
                                .activation
                                .picker
                                .as_ref()
                                .unwrap()
                                .targets
                                .first()
                                .map_or(PickerEvent::OpenDefault, |target| {
                                    PickerEvent::Choose(target.clone())
                                });
                            let id = self.activation.picker.as_ref().unwrap().id;
                            return self.activation_event(Event::Picker(id, event));
                        }
                    }
                    Err(error) => {
                        self.activation.picker =
                            Some(Picker::new(registry, request, Vec::new(), Some(error)))
                    }
                }
            }
            Event::Picker(id, event) => {
                if self.closing || self.stopped {
                    return Task::none();
                }
                let Some(picker) = self
                    .activation
                    .picker
                    .as_mut()
                    .filter(|picker| picker.id == id)
                else {
                    return Task::none();
                };
                let result = picker.update(event);
                if picker.finished {
                    self.activation.picker = None;
                }
                return result.map(move |event| Message::Activation(Event::Picker(id, event)));
            }
        }
        Task::none()
    }
    pub(super) fn activation_picker_open(&self) -> bool {
        self.activation.picker.is_some()
    }
    pub(super) fn cancel_activation_picker(&mut self) -> Task<Message> {
        let Some(id) = self.activation.picker.as_ref().map(|picker| picker.id) else {
            return Task::none();
        };
        self.activation_event(Event::Picker(id, PickerEvent::Cancel))
    }
    pub(super) fn activation_picker_view(&self) -> Element<'_, Message> {
        let id = self.activation.picker.as_ref().expect("picker open").id;
        self.activation
            .picker
            .as_ref()
            .expect("picker open")
            .view()
            .map(move |event| Message::Activation(Event::Picker(id, event)))
    }
    pub(super) fn activate_external(&mut self) -> Task<Message> {
        if let Some(handle) = &self.activation.handle {
            handle.update(
                self.account_id.as_deref(),
                &self.model.account_name,
                !self.closing && !self.stopped,
            );
        }
        if let Some(native) = &self.activation.native {
            native.update(None, "", !self.closing && !self.stopped);
        }
        if self.activation.dispatching
            || self.activation.routing
            || self.activation.picker.is_some()
            || self.closing
            || self.stopped
            || self.selection.sign_out != 0
        {
            return Task::none();
        }
        // Native OS events always apply the global policy, regardless of the
        // original --profile used to launch this receiving process.
        if self.activation.registry.is_some()
            && let Some(incoming) = self.activation.native.as_ref().and_then(Handle::pop)
            && let Some(link) = incoming.link
        {
            self.activation.routing = true;
            self.activation.routing_token = rand::random();
            let token = self.activation.routing_token;
            let Some(registry) = self.activation.registry.clone() else {
                return Task::none();
            };
            let request = Request::new(Some(link));
            let captured = request.clone();
            return Task::perform(
                async move {
                    registry.discover().await.map_err(|_| {
                        "An active profile is unavailable. Try opening the link again.".to_owned()
                    })
                },
                move |result| {
                    Message::Activation(Event::Discovered(token, captured.clone(), result))
                },
            );
        }
        if self
            .activation
            .handle
            .as_ref()
            .is_some_and(Handle::pop_focus)
        {
            return Task::batch([focus(), Task::done(Message::Activation(Event::Wake))]);
        }
        if self.model.authentication.is_some()
            || self.account_id.is_none() && self.demo.is_none()
            || !self.preferences_ready && self.demo.is_none()
            || self.model.people_directory.is_none() && self.demo.is_none()
            || !self.context_menu_allowed()
            || self.context_menu.is_some()
            || self.composing
            || self.space_settings.is_open()
            || self.emoji_management.is_some()
            || self.notification_center.open
            || self.global_search.open
            || self.bookmarks.open
        {
            return Task::none();
        }
        let Some(incoming) = self.activation.handle.as_ref().and_then(Handle::pop) else {
            return Task::none();
        };
        if incoming.user.is_some() && incoming.user != self.account_id {
            self.model.navigation_error =
                Some("A pending link was cancelled because the account changed.".into());
            return Task::done(Message::Activation(Event::Wake));
        }
        self.activation.dispatching = true;
        let navigate = incoming.link.map_or_else(Task::none, |link| {
            self.update(Message::OpenMessageLink(link.as_str().into()))
        });
        self.activation.dispatching = false;
        Task::batch([
            navigate,
            focus(),
            Task::done(Message::Activation(Event::Wake)),
        ])
    }
}
fn focus() -> Task<Message> {
    iced::window::latest().then(|window| {
        window.map_or_else(Task::none, |id| {
            iced::window::minimize(id, false).chain(Task::batch([
                iced::window::gain_focus(id),
                iced::window::request_user_attention(
                    id,
                    Some(iced::window::UserAttention::Informational),
                ),
            ]))
        })
    })
}

fn native_subscription(native: Option<&Handle>) -> Subscription<()> {
    native.map_or_else(Subscription::none, |handle| {
        Subscription::run_with(handle.clone(), |handle| {
            let handle = handle.clone();
            iced::stream::channel(1, async move |mut output| {
                use iced::futures::SinkExt;
                loop {
                    handle.notified().await;
                    if output.send(()).await.is_err() {
                        break;
                    }
                }
            })
        })
    })
}

#[derive(Clone, Debug)]
pub enum PickerEvent {
    Focus(bool),
    SystemTheme(iced::theme::Mode),
    Choose(Target),
    Refresh,
    Refreshed(Result<Vec<Target>, String>),
    Delivered(Result<(), String>),
    Cancel,
    OpenDefault,
    Native,
}
pub(super) struct Picker {
    id: u64,
    registry: Option<Registry>,
    request: Request,
    targets: Vec<Target>,
    error: Option<String>,
    busy: bool,
    native_request: bool,
    auto: bool,
    pub native: Option<Handle>,
    pub finished: bool,
    pub theme: interconnect::settings::ThemeMode,
    pub system_theme: iced::theme::Mode,
}
impl Picker {
    fn new(
        registry: Registry,
        request: Request,
        targets: Vec<Target>,
        error: Option<String>,
    ) -> Self {
        Self {
            id: rand::random(),
            registry: Some(registry),
            request,
            targets,
            error,
            busy: false,
            native_request: false,
            auto: false,
            native: None,
            finished: false,
            theme: interconnect::settings::ThemeMode::Light,
            system_theme: iced::theme::Mode::Light,
        }
    }
    pub fn demo() -> Self {
        let mut targets = Vec::new();
        for (label, name, user) in [
            ("personal", "Jamie", "@jamie:example.org"),
            ("work", "Jamie at work", "@jamie:work.example.org"),
        ] {
            let handle = Handle::local();
            let mut target = handle.target();
            target.label = label.into();
            target.account_name = name.into();
            target.user = Some(user.into());
            targets.push(target);
        }
        Self {
            id: rand::random(),
            registry: None,
            request: Request::new(None),
            targets,
            error: None,
            busy: false,
            native_request: false,
            auto: false,
            native: None,
            finished: false,
            theme: interconnect::settings::ThemeMode::Light,
            system_theme: iced::theme::Mode::Light,
        }
    }
    pub fn update(&mut self, event: PickerEvent) -> Task<PickerEvent> {
        match event {
            PickerEvent::Focus(backwards) => {
                return if backwards {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            PickerEvent::SystemTheme(mode) => self.system_theme = mode,
            PickerEvent::Cancel => {
                self.finished = true;
            }
            PickerEvent::Choose(target) if !self.busy && self.targets.contains(&target) => {
                let Some(registry) = self.registry.clone() else {
                    self.finished = true;
                    return Task::none();
                };
                let request = self.request.clone();
                self.busy = true;
                self.error = None;
                return Task::perform(
                    async move {
                        registry
                            .send(&target, &request)
                            .await
                            .map_err(str::to_owned)
                    },
                    PickerEvent::Delivered,
                );
            }
            PickerEvent::Refresh if !self.busy => {
                let Some(registry) = self.registry.clone() else {
                    return Task::none();
                };
                self.busy = true;
                return Task::perform(
                    async move {
                        registry.discover().await.map_err(|_| {
                            "An active profile is not responding. Try Refresh again.".into()
                        })
                    },
                    PickerEvent::Refreshed,
                );
            }
            PickerEvent::Refreshed(result) => {
                self.busy = false;
                match result {
                    Ok(targets) => {
                        self.targets = targets;
                        self.error = None;
                        if self.auto {
                            self.auto = false;
                            if self.targets.len() <= 1 {
                                let event = self
                                    .targets
                                    .first()
                                    .map_or(PickerEvent::OpenDefault, |target| {
                                        PickerEvent::Choose(target.clone())
                                    });
                                return self.update(event);
                            }
                        }
                    }
                    Err(error) => {
                        self.auto = false;
                        self.error = Some(error);
                    }
                }
            }
            PickerEvent::Delivered(result) => {
                self.busy = false;
                match result {
                    Ok(()) => self.finished = true,
                    Err(error) => self.error = Some(error),
                }
            }
            PickerEvent::OpenDefault if !self.busy => {
                let Some(registry) = self.registry.clone() else {
                    self.finished = true;
                    return Task::none();
                };
                let request = self.request.clone();
                self.busy = true;
                return Task::perform(open_default(registry, request), PickerEvent::Delivered);
            }
            PickerEvent::Native
                if !self.busy && !self.native_request && !self.request.has_link() =>
            {
                if let Some(incoming) = self.native.as_ref().and_then(Handle::pop)
                    && let Some(link) = incoming.link
                {
                    self.request = Request::new(Some(link));
                    self.native_request = true;
                    self.auto = true;
                    return self.update(PickerEvent::Refresh);
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub fn view(&self) -> Element<'_, PickerEvent> {
        let mut content = column![
            shell::title("Open in which profile?", 21),
            text(if self.request.has_link() {
                "Choose the running account that should receive this Matrix link."
            } else {
                "Choose a running account, or open the default profile."
            })
            .size(13)
            .style(shell::muted_text)
        ]
        .spacing(14);
        if self.registry.is_none() {
            content = content.push(text("Local demo · no accounts or links are accessed").size(12));
        }
        for target in &self.targets {
            let label = column![
                text(if target.account_name.is_empty() {
                    &target.label
                } else {
                    &target.account_name
                })
                .size(16),
                text(target.user.as_deref().unwrap_or("Not signed in yet")).size(13),
                text(format!(
                    "Profile: {}",
                    if self
                        .targets
                        .iter()
                        .filter(|other| other.label == target.label && other.user == target.user)
                        .count()
                        > 1
                    {
                        target.profile_directory()
                    } else {
                        &target.label
                    }
                ))
                .size(12)
                .style(shell::muted_text),
            ]
            .spacing(5);
            content = content.push(
                button(label)
                    .width(Length::Fill)
                    .padding(14)
                    .style(shell::quiet_button)
                    .keyboard_navigation(true)
                    .on_press_maybe((!self.busy).then(|| PickerEvent::Choose(target.clone()))),
            );
        }
        if self.targets.is_empty() {
            content = content.push(
                text("No active profiles. Open the default profile or refresh the list.").size(13),
            );
        }
        if let Some(error) = &self.error {
            content = content.push(text(error).size(13).style(shell::danger_text));
        }
        if self.busy {
            content = content.push(text("Delivering…").size(13));
        }
        content = content.push(
            row![
                button("Refresh")
                    .keyboard_navigation(true)
                    .style(shell::quiet_button)
                    .on_press_maybe((!self.busy).then_some(PickerEvent::Refresh)),
                button("Open default profile")
                    .keyboard_navigation(true)
                    .style(shell::quiet_button)
                    .on_press_maybe((!self.busy).then_some(PickerEvent::OpenDefault)),
                button("Cancel")
                    .keyboard_navigation(true)
                    .style(shell::quiet_button)
                    .on_press(PickerEvent::Cancel),
            ]
            .spacing(8)
            .wrap(),
        );
        container(scrollable(content).height(Length::Fill))
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme| shell::surface(theme, shell::palette(theme).background))
            .into()
    }
    pub fn subscription(&self) -> Subscription<PickerEvent> {
        Subscription::batch([
            native_subscription(self.native.as_ref()).map(|_| PickerEvent::Native),
            iced::system::theme_changes().map(PickerEvent::SystemTheme),
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Window(iced::window::Event::CloseRequested) => {
                    Some(PickerEvent::Cancel)
                }
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(PickerEvent::Cancel),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                    modifiers,
                    ..
                }) => Some(PickerEvent::Focus(modifiers.shift())),
                _ => None,
            }),
        ])
    }
    pub fn iced_theme(&self) -> iced::Theme {
        use interconnect::settings::ThemeMode;
        super::appearance::theme(match self.theme {
            ThemeMode::System => {
                if self.system_theme == iced::theme::Mode::Dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                }
            }
            theme => theme,
        })
    }
    pub fn desktop_update(&mut self, event: PickerEvent) -> Task<PickerEvent> {
        let cancel = matches!(event, PickerEvent::Cancel);
        let task = self.update(event);
        if self.finished {
            if let Some(native) = &self.native {
                if cancel {
                    native.cancel();
                }
                if !native.close_if_empty() {
                    self.finished = false;
                    self.native_request = false;
                    self.request = Request::new(None);
                    return task.chain(Task::done(PickerEvent::Native));
                }
            }
            return task.chain(iced::exit());
        }
        // A native wake can arrive while a manual refresh is busy. Retain it
        // at the source, then pump as soon as the welcome is ready again.
        if !self.busy
            && !self.native_request
            && !self.request.has_link()
            && self
                .native
                .as_ref()
                .is_some_and(|native| native.pending() > 0)
        {
            return task.chain(self.update(PickerEvent::Native));
        }
        task
    }
}

async fn open_default(registry: Registry, request: Request) -> Result<(), String> {
    let path = interconnect::session::default_profile_directory()
        .map_err(|_| "Could not find the default profile.".to_owned())?;
    let canonical = activation::canonical_profile(&path)
        .map_err(|_| "Could not resolve the default profile.".to_owned())?;
    let mut child = tokio::process::Command::new(
        std::env::current_exe().map_err(|_| "Could not find Interconnect.".to_owned())?,
    )
    .arg("--profile")
    .arg("default")
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .spawn()
    .map_err(|_| "Could not start the default profile.".to_owned())?;
    // Reap without tying a long-lived account process to the temporary picker.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    for _ in 0..40 {
        let targets = registry.discover().await.map_err(|_| {
            "Could not find the default profile. Refresh before retrying.".to_owned()
        })?;
        if let Some(target) = targets
            .into_iter()
            .find(|target| activation::same_profile(target, &canonical))
        {
            return registry
                .send(&target, &request)
                .await
                .map_err(str::to_owned);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err("The default profile is still starting. Refresh before retrying.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{settings, tests::demo_app};
    use clap::Parser;
    use interconnect::activation::Link;

    fn request() -> Request {
        Request::new(Some(
            Link::parse("matrix:roomid/demo-general:example.org/e/message").unwrap(),
        ))
    }
    fn registry() -> (tempfile::TempDir, Registry) {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::open(directory.path().join("ipc")).unwrap();
        (directory, registry)
    }
    async fn owner(registry: &Registry, profile: &str) -> Server {
        let profile = activation::canonical_profile(profile).unwrap();
        let Claim::Owner(server) = registry.claim(&profile).await.unwrap() else {
            panic!();
        };
        server
            .handle()
            .update(Some("@fixture:example.org"), "Fixture", true);
        server
    }
    #[tokio::test]
    async fn startup_routes_zero_one_many_and_explicit_profiles_before_opening_stores() {
        let (directory, registry) = registry();
        let args = super::super::cli::Arguments::try_parse_from([
            "interconnect",
            "--open",
            "matrix:roomid/fixture:example.org",
        ])
        .unwrap();
        let default = args.profile_directory().unwrap();
        let existed = std::path::Path::new(&default).exists();
        let Launch::Profile { server, .. } = prepare_in(&args, registry.clone()).await.unwrap()
        else {
            panic!();
        };
        assert_eq!(
            std::path::Path::new(&default).exists(),
            existed,
            "activation cannot create an account store"
        );
        drop(server);
        // Exercise a symlinked ancestor on every platform, like /var ->
        // /private/var for temporary profiles on macOS. The profiles stay absent.
        let alias = directory.path().join("profiles");
        std::os::unix::fs::symlink(directory.path(), &alias).unwrap();
        let personal_path = alias.join("personal");
        let personal = owner(&registry, personal_path.to_str().unwrap()).await;
        assert!(matches!(
            prepare_in(&args, registry.clone()).await.unwrap(),
            Launch::Forwarded
        ));
        assert_eq!(personal.handle().pending(), 1);
        let work_path = alias.join("work");
        let work = owner(&registry, work_path.to_str().unwrap()).await;
        assert!(matches!(
            prepare_in(&args, registry.clone()).await.unwrap(),
            Launch::Picker(_)
        ));
        let explicit = super::super::cli::Arguments::try_parse_from([
            "interconnect",
            "--profile-path",
            work_path.to_str().unwrap(),
            "--open",
            "matrix:roomid/fixture:example.org",
        ])
        .unwrap();
        assert!(matches!(
            prepare_in(&explicit, registry).await.unwrap(),
            Launch::Forwarded
        ));
        assert_eq!(work.handle().pending(), 1);
        assert!(!work_path.exists());
        assert!(!personal_path.exists());
    }
    #[test]
    fn queued_links_survive_settings_and_sign_in_then_preserve_drafts_and_event_focus() {
        let mut app = demo_app();
        let old_room = app.selection.room.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent draft");
        app.scroll_positions.insert(key.clone(), 71.0);
        let handle = Handle::local();
        app.activation.handle = Some(handle.clone());
        let _ = app.update(Message::Settings(settings::Event::Open));
        handle.enqueue(request()).unwrap();
        let _ = app.update(Message::Activation(Event::Wake));
        assert_eq!(handle.pending(), 1);
        assert_eq!(app.selection.room, old_room);
        app.model.authentication = Some(interconnect::connected::Authentication::Checking);
        let _ = app.update(Message::Settings(settings::Event::Cancel));
        assert_eq!(handle.pending(), 1);
        app.model.authentication = None;
        let _ = app.update(Message::Activation(Event::Wake));
        assert_eq!(handle.pending(), 0);
        assert_eq!(app.selection.room.as_ref().unwrap().1, "general");
        assert_eq!(app.selection.event_focus[0].as_ref().unwrap().1, "$message");
        assert_eq!(app.drafts[&key].content.text().trim_end(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 71.0);
        assert!(app.settings.path.is_none());
        assert!(app.model.people_directory.is_none());
    }
    #[test]
    fn pending_links_cannot_cross_account_changes_and_shutdown_rejects_native_admission() {
        let mut app = demo_app();
        app.model.authentication = None;
        let selected = app.selection.room.clone();
        let handle = Handle::local();
        handle.update(Some("@old:example.org"), "Old", true);
        handle.enqueue(request()).unwrap();
        app.activation.handle = Some(handle.clone());
        app.account_id = Some("@new:example.org".into());
        let _ = app.update(Message::Activation(Event::Wake));
        assert_eq!(handle.pending(), 0);
        assert_eq!(app.selection.room, selected);
        assert!(
            app.model
                .navigation_error
                .as_ref()
                .unwrap()
                .contains("account changed")
        );
        let native = Handle::local();
        app.activation.native = Some(native.clone());
        app.closing = true;
        let _ = app.update(Message::Activation(Event::NativeWake));
        assert!(!native.native_link("matrix:roomid/fixture:example.org"));
        let (_directory, registry) = registry();
        app.activation.registry = Some(registry);
        app.activation.routing = true;
        app.activation.routing_token = 7;
        let _ = app.update(Message::Activation(Event::Discovered(
            7,
            request(),
            Ok(vec![]),
        )));
        assert!(
            !app.activation_picker_open(),
            "late discovery must not launch a default profile while closing"
        );
    }
    #[test]
    fn focus_only_launch_does_not_wait_for_account_readiness() {
        let mut app = demo_app();
        app.model.authentication = Some(interconnect::connected::Authentication::Checking);
        let handle = Handle::local();
        handle.enqueue(Request::new(None)).unwrap();
        app.activation.handle = Some(handle.clone());
        let _ = app.update(Message::Activation(Event::Wake));
        assert_eq!(handle.pending(), 0);
    }
    #[test]
    fn picker_nonce_rejects_late_completions_and_hidden_navigation() {
        let mut app = demo_app();
        let first = Picker::demo();
        let old = first.id;
        app.activation.picker = Some(first);
        let selected = app.selection.room.clone();
        let _ = app.update(Message::Room("general".into()));
        assert_eq!(app.selection.room, selected);
        let _ = app.update(Message::Activation(Event::Picker(old, PickerEvent::Cancel)));
        let next = Picker::demo();
        let current = next.id;
        app.activation.picker = Some(next);
        for event in [
            PickerEvent::Delivered(Ok(())),
            PickerEvent::Refreshed(Ok(vec![])),
            PickerEvent::Cancel,
        ] {
            let _ = app.update(Message::Activation(Event::Picker(old, event)));
            assert_eq!(app.activation.picker.as_ref().unwrap().id, current);
            assert_eq!(app.activation.picker.as_ref().unwrap().targets.len(), 2);
        }
        assert!(!app.context_menu_allowed());
        assert!(!app.devices_available());
    }
    #[test]
    fn standalone_native_requests_serialize_and_apply_zero_one_many_policy() {
        for count in 0..=2 {
            let mut picker = Picker::demo();
            let native = Handle::local();
            assert!(native.native_link("matrix:roomid/first:example.org"));
            assert!(native.native_link("matrix:roomid/second:example.org"));
            picker.native = Some(native.clone());
            let _ = picker.update(PickerEvent::Native);
            assert_eq!(native.pending(), 1);
            let first_id = serde_json::to_string(&picker.request).unwrap();
            let _ = picker.update(PickerEvent::Native);
            assert_eq!(native.pending(), 1);
            assert_eq!(serde_json::to_string(&picker.request).unwrap(), first_id);
            // A real registry means auto-forward/default tasks are scheduled,
            // never executed by this pure UI test.
            let (_directory, registry) = registry();
            picker.registry = Some(registry);
            picker.busy = true;
            let targets = picker.targets.iter().take(count).cloned().collect();
            let _ = picker.update(PickerEvent::Refreshed(Ok(targets)));
            assert_eq!(picker.busy, count < 2);
            assert!(picker.request.has_link());
        }
    }
    #[test]
    fn native_wake_during_welcome_refresh_is_pumped_after_completion() {
        let (_directory, registry) = registry();
        let mut picker = Picker::new(registry, Request::new(None), vec![], None);
        let native = Handle::local();
        picker.native = Some(native.clone());
        picker.busy = true;
        assert!(native.native_link("matrix:roomid/fixture:example.org"));
        let _ = picker.desktop_update(PickerEvent::Native);
        assert_eq!(native.pending(), 1);
        let _ = picker.desktop_update(PickerEvent::Refreshed(Ok(vec![])));
        assert_eq!(native.pending(), 0);
        assert!(picker.native_request && picker.busy && picker.request.has_link());
    }
    #[test]
    fn sign_out_keeps_profile_activation_ownership_but_cancels_old_account_requests() {
        let mut app = demo_app();
        let handle = Handle::local();
        handle.enqueue(request()).unwrap();
        let identity = handle.target().instance;
        app.activation.handle = Some(handle.clone());
        app.signed_out(interconnect::connected::Model::default());
        assert_eq!(
            app.activation.handle.as_ref().unwrap().target().instance,
            identity
        );
        assert_eq!(handle.pending(), 0);
        assert_eq!(handle.target().user, None);
    }
}
