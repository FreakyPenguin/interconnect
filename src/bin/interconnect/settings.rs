//! Startup settings and asynchronous panel reload/save tasks. No timers.
use super::{App, Message, icons::Kind, shell};
use iced::{
    Element, Length, Task,
    widget::{
        button, checkbox, column, container, mouse_area, opaque, pick_list, row, scrollable, text,
        text_input,
    },
};
use interconnect::settings::{
    self, GifPlayback, Graphics, SendShortcut, Settings, Store, ThemeMode,
};
use std::path::PathBuf;

#[derive(Default)]
pub struct State {
    pub(super) profile: super::account_profile::State,
    pub current: Settings,
    pub draft: Option<Settings>,
    pub path: Option<PathBuf>,
    store: Option<Store>,
    busy: bool,
    pub saving: bool,
    pub error: Option<String>,
    request: u64,
    initial_load: bool,
    pub navigation_touched: bool,
    close_after_save: bool,
    launched_graphics: Graphics,
    gif_seconds_input: String,
    presence_seconds_input: String,
    notification_test_busy: bool,
    notification_test_result: Option<Result<(), String>>,
    category: Category,
    confirm_discard: Option<Discard>,
    confirm_serial: u64,
}
impl State {
    pub(super) fn graphics_restart_required(&self) -> bool {
        self.current.graphics != self.launched_graphics
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    General,
    Account,
    Notifications,
    Media,
    Advanced,
}
impl Category {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Account,
        Self::Notifications,
        Self::Media,
        Self::Advanced,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Account => "Account",
            Self::Notifications => "Notifications",
            Self::Media => "Media & emoji",
            Self::Advanced => "Advanced",
        }
    }
}
#[derive(Clone, Debug)]
enum DiscardAction {
    Close,
    Reload,
    ReloadProfile,
    Transition(Box<Message>),
}
#[derive(Clone, Debug)]
struct Discard {
    serial: u64,
    action: DiscardAction,
}
impl State {
    fn request_discard(&mut self, action: DiscardAction) {
        self.confirm_serial = self.confirm_serial.wrapping_add(1);
        self.confirm_discard = Some(Discard {
            serial: self.confirm_serial,
            action,
        });
    }
    fn dirty(&self) -> bool {
        self.profile.dirty() || self.preferences_dirty()
    }
    fn preferences_dirty(&self) -> bool {
        self.draft.as_ref().is_some_and(|draft| {
            draft != &self.current
                || self.gif_seconds_input != self.current.media.gif_seconds.to_string()
                || self.presence_seconds_input
                    != self.current.presence.poll_interval_seconds.to_string()
        })
    }
}

/// Read before creating the runtime/window: the renderer needs these settings
/// before Iced starts. Subsequent reloads and saves stay on blocking workers.
#[derive(Clone)]
pub(super) struct Startup {
    path: Option<PathBuf>,
    loaded: Option<Result<(Store, Settings), String>>,
}
impl Startup {
    pub(super) fn load(path: Option<PathBuf>, demo: bool) -> Self {
        if demo {
            return Self {
                path: None,
                loaded: None,
            };
        }
        match path.map(Ok).unwrap_or_else(settings::default_path) {
            Ok(path) => Self {
                loaded: Some(Store::load(path.clone()).map_err(|error| format!("{error:#}"))),
                path: Some(path),
            },
            Err(error) => Self {
                path: None,
                loaded: Some(Err(format!("{error:#}"))),
            },
        }
    }
    pub(super) fn graphics(&self) -> Graphics {
        self.loaded
            .as_ref()
            .and_then(|loaded| loaded.as_ref().ok())
            .map(|(_, settings)| settings.graphics.clone())
            .unwrap_or_default()
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Profile(super::account_profile::Event),
    Open,
    Cancel,
    RequestClose,
    KeepEditing(u64),
    DiscardChanges(u64),
    Category(Category),
    ShowRoomList(bool),
    ShowUnreadSection(bool),
    UnreadFirst(bool),
    LimitRoomWidth(bool),
    Theme(ThemeMode),
    SendShortcut(SendShortcut),
    GifPlayback(GifPlayback),
    GifSeconds(String),
    GpuEnabled(bool),
    LowPower(bool),
    NotificationsEnabled(bool),
    NotificationPreview(bool),
    PresencePolling(bool),
    PresenceSeconds(String),
    TestNotification,
    NotificationTested(Result<(), String>),
    Defaults,
    Reload,
    Save,
    Loaded(u64, Result<(Store, Settings), String>),
    Saved(u64, Settings, Result<Store, String>),
    Ignore,
}
fn message(event: Event) -> Message {
    Message::Settings(event)
}
impl App {
    pub(super) fn update_notification_context(&mut self) {
        self.refresh_presence_demand();
        self.selection.notifications = self.settings.current.notifications.clone();
        self.selection.conversation_visible = !self.room_settings.is_open()
            && !self.sign_out_open
            && !self.draft_persistence.close_requested
            && !self.membership.open
            && self.message_delete.is_none()
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.space_settings.is_open()
            && self.window_focused
            && self.emoji_management.is_none()
            && self.settings.draft.is_none()
            && self.room_picker.is_none()
            && self.new_chat.is_none()
            && self.create_room.is_none()
            && !self.verification.open
            && self.image_view.is_none()
            && !self.closing
            && !self.stopped;
    }
    pub(super) fn publish_notification_context(&mut self) {
        self.update_notification_context();
        if let Some(control) = &self.control {
            control.send_if_modified(|selection| {
                if *selection == self.selection {
                    false
                } else {
                    *selection = self.selection.clone();
                    true
                }
            });
        }
    }
    fn valid_gif_seconds(&self) -> bool {
        self.settings
            .draft
            .as_ref()
            .is_none_or(|draft| draft.media.gif_playback == GifPlayback::Once)
            || self
                .settings
                .gif_seconds_input
                .parse::<u16>()
                .is_ok_and(|seconds| (1..=300).contains(&seconds))
    }
    fn valid_presence_seconds(&self) -> bool {
        self.settings
            .draft
            .as_ref()
            .is_none_or(|draft| !draft.presence.poll_enabled)
            || self
                .settings
                .presence_seconds_input
                .parse::<u16>()
                .is_ok_and(|seconds| (15..=3600).contains(&seconds))
    }
    pub(super) fn boot(startup: Startup) -> (Self, Task<Message>) {
        let mut app = Self::new();
        let demo_topic = if startup.loaded.is_none() {
            app.enable_demo()
        } else {
            Task::none()
        };
        let demo_code = if app.demo.is_some() {
            Task::perform(
                async {
                    tokio::task::spawn_blocking(|| {
                        interconnect::message_document::Document::parse_with_formulas(
                            super::demo::CODE_SAMPLE,
                        )
                        .map(std::sync::Arc::new)
                    })
                    .await
                    .ok()
                    .flatten()
                },
                Message::DemoCodeReady,
            )
        } else {
            Task::none()
        };
        let Some(loaded) = startup.loaded else {
            return (app, Task::batch([demo_topic, demo_code]));
        };
        app.settings.initial_load = true;
        app.settings.path = startup.path;
        app.settings.busy = true;
        let task = app.settings_update(Event::Loaded(0, loaded));
        app.update_notification_context();
        (app, Task::batch([task, demo_code]))
    }
    fn load_settings(&mut self) -> Task<Message> {
        if self.settings.busy {
            return Task::none();
        }
        let Some(path) = self.settings.path.clone() else {
            return Task::none();
        };
        self.settings.busy = true;
        self.settings.request += 1;
        let request = self.settings.request;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || Store::load(path))
                    .await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| format!("{error:#}"))
            },
            move |result| message(Event::Loaded(request, result)),
        )
    }
    pub(super) fn settings_update(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::NotificationsEnabled(enabled) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.notifications.enabled = enabled;
                }
            }
            Event::NotificationPreview(show) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.notifications.show_preview = show;
                }
            }
            Event::TestNotification if !self.settings.notification_test_busy && !self.closing => {
                self.settings.notification_test_busy = true;
                self.settings.notification_test_result = None;
                return Task::perform(interconnect::notifications::test_notification(), |result| {
                    message(Event::NotificationTested(result))
                });
            }
            Event::NotificationTested(result) => {
                self.settings.notification_test_busy = false;
                self.settings.notification_test_result = Some(result);
            }
            Event::Category(category) if self.settings.draft.is_some() && !self.settings.saving => {
                self.settings.category = category;
                let profile = if category == Category::Account {
                    self.account_profile_event(super::account_profile::Event::Open)
                } else {
                    Task::none()
                };
                return Task::batch([
                    profile,
                    iced::widget::operation::snap_to(
                        "settings-body",
                        iced::widget::operation::RelativeOffset::START,
                    ),
                ]);
            }
            Event::Profile(super::account_profile::Event::Reload)
                if self.settings.profile.dirty() && self.settings.draft.is_some() =>
            {
                self.settings.request_discard(DiscardAction::ReloadProfile);
            }
            Event::Profile(event) => {
                if self.settings.confirm_discard.is_some()
                    && !matches!(
                        event,
                        super::account_profile::Event::Loaded(..)
                            | super::account_profile::Event::Saved(..)
                    )
                {
                    return Task::none();
                }
                return self.account_profile_event(event);
            }
            Event::RequestClose if !self.settings.saving => {
                // Escape on a discard prompt keeps editing, matching room,
                // space, and emoji editors. Never turn a second Escape into
                // acceptance of the pending close/reload/application exit.
                if self.settings.confirm_discard.take().is_some() {
                    return Task::none();
                }
                if self.settings.dirty() {
                    self.settings.request_discard(DiscardAction::Close);
                } else {
                    return self.settings_update(Event::Cancel);
                }
            }
            Event::KeepEditing(serial)
                if self
                    .settings
                    .confirm_discard
                    .as_ref()
                    .is_some_and(|confirm| confirm.serial == serial) =>
            {
                self.settings.confirm_discard = None;
            }
            Event::DiscardChanges(serial)
                if !self.settings.busy
                    && self
                        .settings
                        .confirm_discard
                        .as_ref()
                        .is_some_and(|confirm| confirm.serial == serial) =>
            {
                match self.settings.confirm_discard.take().unwrap().action {
                    DiscardAction::Close => return self.settings_update(Event::Cancel),
                    DiscardAction::Reload => return self.load_settings(),
                    DiscardAction::ReloadProfile => {
                        return self.account_profile_event(super::account_profile::Event::Reload);
                    }
                    DiscardAction::Transition(next) => {
                        self.settings.draft = None;
                        self.settings.profile.dismiss();
                        return self.update(*next);
                    }
                }
            }
            Event::Open if self.settings.draft.is_none() && !self.closing => {
                self.settings.category = Category::General;
                self.settings.confirm_discard = None;
                self.settings.gif_seconds_input =
                    self.settings.current.media.gif_seconds.to_string();
                self.settings.presence_seconds_input = self
                    .settings
                    .current
                    .presence
                    .poll_interval_seconds
                    .to_string();
                self.settings.draft = Some(self.settings.current.clone());
                // Remove focus from composers/search while preferences cover the workspace.
                return iced::widget::operation::focus("settings-dialog");
            }
            Event::Cancel if !self.settings.saving => {
                self.settings.profile.dismiss();
                self.settings.draft = None;
                self.settings.confirm_discard = None;
                return iced::widget::operation::focus(if self.selection.thread.is_some() {
                    "thread-composer"
                } else {
                    "room-composer"
                });
            }
            Event::ShowRoomList(show) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.navigation.show_room_list = show;
                }
            }
            Event::ShowUnreadSection(show) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.navigation.show_unread_section = show;
                }
            }
            Event::UnreadFirst(enabled) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.navigation.unread_first = enabled;
                }
            }
            Event::Theme(mode) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.appearance.theme = mode;
                }
            }
            Event::LimitRoomWidth(limited) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.appearance.limit_room_width = limited;
                }
            }
            Event::SendShortcut(shortcut) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.composer.send_shortcut = shortcut;
                }
            }
            Event::GifPlayback(playback) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.media.gif_playback = playback;
                }
            }
            Event::GifSeconds(input) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    if let Ok(seconds) = input.parse::<u16>()
                        && (1..=300).contains(&seconds)
                    {
                        draft.media.gif_seconds = seconds;
                    }
                    self.settings.gif_seconds_input = input;
                }
            }
            Event::PresencePolling(enabled) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.presence.poll_enabled = enabled;
                }
            }
            Event::PresenceSeconds(input) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    if let Ok(seconds) = input.parse::<u16>()
                        && (15..=3600).contains(&seconds)
                    {
                        draft.presence.poll_interval_seconds = seconds;
                    }
                    self.settings.presence_seconds_input = input;
                }
            }
            Event::GpuEnabled(enabled) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    draft.graphics.gpu_enabled = enabled;
                }
            }
            Event::LowPower(low_power) if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft
                    && draft.graphics.gpu_enabled
                {
                    draft.graphics.low_power = low_power;
                }
            }
            Event::Defaults if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    *draft = Settings::default();
                    self.settings.gif_seconds_input = draft.media.gif_seconds.to_string();
                    self.settings.presence_seconds_input =
                        draft.presence.poll_interval_seconds.to_string();
                }
            }
            Event::Reload if self.settings.draft.is_some() && !self.settings.busy => {
                if self.settings.preferences_dirty() {
                    self.settings.request_discard(DiscardAction::Reload);
                } else {
                    return self.load_settings();
                }
            }
            Event::Loaded(request, result)
                if request == self.settings.request
                    && self.settings.busy
                    && !self.settings.saving =>
            {
                self.settings.busy = false;
                let initial = std::mem::take(&mut self.settings.initial_load);
                match result {
                    Ok((store, settings)) => {
                        if initial {
                            self.settings.launched_graphics = settings.graphics.clone();
                        }
                        if initial && !self.settings.navigation_touched {
                            self.shell.navigation = settings.navigation.show_room_list;
                        }
                        if self.settings.draft.is_some() {
                            self.settings.gif_seconds_input =
                                settings.media.gif_seconds.to_string();
                            self.settings.presence_seconds_input =
                                settings.presence.poll_interval_seconds.to_string();
                            self.settings.draft = Some(settings.clone());
                        }
                        // Reload fills the form. Save is the only operation that
                        // applies an open form, so Cancel remains meaningful.
                        if initial || self.settings.draft.is_none() {
                            self.settings.current = settings;
                        }
                        self.settings.store = Some(store);
                        self.settings.error = None;
                    }
                    Err(error) => {
                        self.settings.store = None;
                        self.settings.error = Some(error);
                    }
                }
            }
            Event::Save if !self.settings.busy => {
                let Some(settings) = self.settings.draft.clone() else {
                    return Task::none();
                };
                if !self.valid_gif_seconds() || !self.valid_presence_seconds() {
                    return Task::none();
                }
                if self.demo.is_some() {
                    self.settings.current = settings;
                    return self.settings_update(Event::RequestClose);
                }
                let Some(mut store) = self.settings.store.clone() else {
                    return Task::none();
                };
                self.settings.busy = true;
                self.settings.saving = true;
                self.settings.request += 1;
                let request = self.settings.request;
                let value = settings.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            store.save(&value)?;
                            Ok::<_, anyhow::Error>(store)
                        })
                        .await
                        .map_err(|error| error.to_string())?
                        .map_err(|error| format!("{error:#}"))
                    },
                    move |result| message(Event::Saved(request, settings.clone(), result)),
                );
            }
            Event::Saved(request, settings, result)
                if request == self.settings.request && self.settings.saving =>
            {
                self.settings.busy = false;
                self.settings.saving = false;
                let close = std::mem::take(&mut self.settings.close_after_save);
                match result {
                    Ok(store) => {
                        self.settings.current = settings;
                        self.settings.store = Some(store);
                        self.settings.error = None;
                        if close {
                            self.settings.draft = None;
                            return self.update(Message::Close);
                        }
                        return self.settings_update(Event::RequestClose);
                    }
                    Err(error) => self.settings.error = Some(error),
                }
            }
            _ => {}
        }
        Task::none()
    }
    /// Capture the requested application transition, preserving unsaved values
    /// until the exact displayed confirmation is accepted.
    pub(super) fn guard_settings_transition(&mut self, next: Message) -> bool {
        if self.settings.draft.is_none() || self.settings.saving {
            return false;
        }
        if self.settings.dirty() {
            self.settings
                .request_discard(DiscardAction::Transition(Box::new(next)));
            true
        } else {
            self.settings.draft = None;
            self.settings.profile.dismiss();
            self.settings.confirm_discard = None;
            false
        }
    }

    pub(super) fn defer_close_for_settings(&mut self) -> bool {
        if self.settings.saving {
            self.settings.close_after_save = true;
            true
        } else {
            false
        }
    }
    pub(super) fn settings_view(&self) -> Element<'_, Message> {
        super::focus_scope::dismissible(
            iced::widget::responsive(move |size| self.settings_panel(size)),
            "settings-focus",
            Message::Escape,
        )
    }
    fn settings_panel(&self, size: iced::Size) -> Element<'_, Message> {
        let draft = self.settings.draft.as_ref().expect("open settings");
        let available = !self.settings.busy;
        let mut notifications = checkbox(draft.notifications.enabled)
            .keyboard_navigation(true)
            .label("Enable desktop notifications")
            .size(16)
            .text_size(13)
            .spacing(9);
        let mut previews = checkbox(draft.notifications.show_preview)
            .keyboard_navigation(true)
            .label("Show room, sender, and message text")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            notifications =
                notifications.on_toggle(|value| message(Event::NotificationsEnabled(value)));
            if draft.notifications.enabled {
                previews = previews.on_toggle(|value| message(Event::NotificationPreview(value)));
            }
        }
        let mut notification_test = button(
            text(if self.settings.notification_test_busy {
                "Sending sample…"
            } else {
                "Send sample notification"
            })
            .size(13),
        )
        .keyboard_navigation(true)
        .padding([7, 10])
        .style(shell::quiet_button);
        if !self.settings.notification_test_busy {
            notification_test = notification_test.on_press(message(Event::TestNotification));
        }
        let mut visibility = checkbox(draft.navigation.show_room_list)
            .keyboard_navigation(true)
            .label("Show room list at startup")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            visibility = visibility.on_toggle(|show| message(Event::ShowRoomList(show)));
        }
        let mut presence_polling = checkbox(draft.presence.poll_enabled)
            .keyboard_navigation(true)
            .label("Refresh presence periodically")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            presence_polling =
                presence_polling.on_toggle(|enabled| message(Event::PresencePolling(enabled)));
        }
        let mut presence_controls = column![presence_polling].spacing(12);
        if draft.presence.poll_enabled {
            let mut seconds = text_input("15–3600", &self.settings.presence_seconds_input)
                .cursor_blinking(false)
                .width(88)
                .size(13)
                .padding([7, 10])
                .style(shell::search_style);
            if available {
                seconds = seconds.on_input(|value| message(Event::PresenceSeconds(value)));
            }
            presence_controls = presence_controls.push(
                row![
                    text("Refresh every").size(13),
                    seconds,
                    text("seconds").size(13)
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            );
        }
        let mut unread_section = checkbox(draft.navigation.show_unread_section)
            .keyboard_navigation(true)
            .label("Show unread section")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            unread_section =
                unread_section.on_toggle(|show| message(Event::ShowUnreadSection(show)));
        }
        let mut unread_first = checkbox(draft.navigation.unread_first)
            .keyboard_navigation(true)
            .label("Show unread rooms first in each section")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            unread_first = unread_first.on_toggle(|enabled| message(Event::UnreadFirst(enabled)));
        }
        let mut room_width = checkbox(draft.appearance.limit_room_width)
            .keyboard_navigation(true)
            .label("Limit room content width")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            room_width = room_width.on_toggle(|value| message(Event::LimitRoomWidth(value)));
        }
        let shortcut = pick_list(
            [SendShortcut::CtrlEnter, SendShortcut::Enter],
            Some(draft.composer.send_shortcut),
            |shortcut| message(Event::SendShortcut(shortcut)),
        )
        .keyboard_navigation(true)
        .text_size(13)
        .padding([7, 10])
        .style(shell::select_style);
        let mut gpu = checkbox(draft.graphics.gpu_enabled)
            .keyboard_navigation(true)
            .label("Use GPU rendering")
            .size(16)
            .text_size(13)
            .spacing(9);
        let mut low_power = checkbox(draft.graphics.low_power)
            .keyboard_navigation(true)
            .label("Prefer a low-power GPU")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            gpu = gpu.on_toggle(|value| message(Event::GpuEnabled(value)));
        }
        if available && draft.graphics.gpu_enabled {
            low_power = low_power.on_toggle(|value| message(Event::LowPower(value)));
        }
        let reload = super::tooltips::icon_button(
            Kind::Refresh,
            "Reload settings file",
            (available && self.settings.path.is_some()).then_some(message(Event::Reload)),
        );
        let defaults = button(text("Use defaults").size(13))
            .keyboard_navigation(true)
            .padding([7, 10])
            .style(shell::quiet_button)
            .on_press_maybe(available.then_some(message(Event::Defaults)));
        let save = super::tooltips::icon_action(
            Kind::Save,
            if self.settings.saving {
                "Saving settings…"
            } else {
                "Save settings"
            },
            (available
                && self.settings.confirm_discard.is_none()
                && self.settings.preferences_dirty()
                && (self.demo.is_some() || self.settings.store.is_some())
                && self.valid_gif_seconds()
                && self.valid_presence_seconds())
            .then_some(message(Event::Save)),
            shell::primary_button,
        );
        let playback = pick_list(
            [GifPlayback::Once, GifPlayback::Duration],
            Some(draft.media.gif_playback),
            |value| message(Event::GifPlayback(value)),
        )
        .keyboard_navigation(true)
        .text_size(13)
        .padding([7, 10])
        .style(shell::select_style);
        let mut gif_controls = row![text("Play").size(13), playback]
            .spacing(12)
            .align_y(iced::Alignment::Center);
        if draft.media.gif_playback == GifPlayback::Duration {
            let mut seconds = text_input("1–300", &self.settings.gif_seconds_input)
                .id("settings-gif-seconds")
                .cursor_blinking(false)
                .width(72)
                .size(13)
                .padding([7, 10])
                .style(shell::search_style);
            if available {
                seconds = seconds.on_input(|value| message(Event::GifSeconds(value)));
            }
            gif_controls = gif_controls.push(seconds).push(text("seconds").size(13));
        }
        let mut notification_controls = column![
            notifications,
            previews,
            caption("Alerts follow your Matrix notification rules. The conversation you are viewing stays quiet."),
            notification_test,
            caption("The sample uses no account or message content. On macOS, allow notifications when prompted."),
        ].spacing(12).width(Length::Fill);
        if let Some(result) = &self.settings.notification_test_result {
            notification_controls = notification_controls.push(match result {
                Ok(()) => {
                    caption("Sample accepted by the desktop. Check your notification center.")
                }
                Err(error) => caption(format!("Sample notification failed: {error}"))
                    .style(shell::danger_text),
            });
        }
        if let Some(error) = &self.model.notification_error {
            notification_controls =
                notification_controls.push(caption(error).style(shell::danger_text));
        }
        let mut content = match self.settings.category {
            Category::General => column![
                column![
                    shell::title("Appearance", 14),
                    row![text("Theme").size(13), pick_list(
                        ThemeMode::ALL, Some(draft.appearance.theme),
                        move |mode| message(if available { Event::Theme(mode) } else { Event::Ignore }),
                    ).keyboard_navigation(true).text_size(13).padding([7, 10]).style(shell::select_style)].spacing(16).align_y(iced::Alignment::Center),
                    room_width,
                    caption("Center messages and the composer in a readable column."),
                ].spacing(12),
                iced::widget::rule::horizontal(1),
                column![
                    shell::title("Messages", 14),
                    row![text("Send with").size(13), shortcut].spacing(16).align_y(iced::Alignment::Center),
                    caption("With Enter to send, use Shift+Enter for a new line. Ctrl+Enter always sends."),
                ].spacing(12),
                iced::widget::rule::horizontal(1),
                column![
                    shell::title("Room list", 14),
                    visibility,
                    unread_section,
                    caption("Show unread rooms at the top, as well as in their usual sections."),
                    unread_first,
                ].spacing(12),
            ],
            Category::Account => column![self.account_profile_view(available && self.settings.confirm_discard.is_none())],
            Category::Notifications => column![
                shell::title("Desktop notifications", 14), notification_controls,
            ],
            Category::Media => column![
                column![
                    shell::title("GIF playback", 14),
                    caption("GIFs stay still until you choose Play on a preview or in the image viewer."),
                    gif_controls,
                    caption(if self.valid_gif_seconds() {
                        "Duration repeats the GIF for 1–300 whole seconds. Once plays one loop."
                    } else {
                        "Enter a whole number from 1 to 300 seconds to save."
                    }).style(if self.valid_gif_seconds() { shell::muted_text } else { shell::danger_text }),
                ].spacing(12),
                iced::widget::rule::horizontal(1),
                column![
                    shell::title("Custom emoji", 14),
                    caption("Manage your personal emoji and packs shared with rooms."),
                    button(text("Manage emoji packs…").size(13)).keyboard_navigation(true)
                        .padding([7, 10])
                        .style(shell::quiet_button)
                        .on_press_maybe((available && (self.demo.is_some() || self.model.emoji_management.is_some()))
                            .then_some(Message::EmojiManagement(super::emoji_management::Event::Open))),
                    caption("Emoji pack changes are saved separately from these preferences."),
                ].spacing(12),
            ],
            Category::Advanced => {
                let mut advanced = column![
                    column![
                        shell::title("Presence refresh", 14),
                        presence_controls,
                        caption(if self.valid_presence_seconds() {
                            "Refresh contacts and visible people while focused. Choose 15–3600 seconds; 60 is the default."
                        } else {
                            "Enter a whole number from 15 to 3600 seconds to save."
                        }).style(if self.valid_presence_seconds() { shell::muted_text } else { shell::danger_text }),
                    ].spacing(12),
                    iced::widget::rule::horizontal(1),
                    column![
                        row![shell::title("Graphics", 14), text("Restart required").size(11).style(shell::muted_text)]
                            .spacing(10).align_y(iced::Alignment::Center),
                        gpu,
                        caption("Uses the CPU renderer when disabled. GPU rendering includes a software fallback."),
                        low_power,
                        caption("Graphics and room-list startup changes apply on the next launch."),
                    ].spacing(12),
                    iced::widget::rule::horizontal(1),
                    shell::title("Configuration", 14),
                ].spacing(20);
                if self.demo.is_some() {
                    advanced = advanced.push(caption("Demo settings stay in memory and disappear on exit."));
                } else if let Some(path) = &self.settings.path {
                    advanced = advanced.push(caption(path.display().to_string()).wrapping(text::Wrapping::Glyph))
                        .push(caption("Reload fills this form from the TOML file. Choose Save to apply it."));
                }
                advanced.push(row![defaults, reload].spacing(8))
            }
        }.spacing(20).width(Length::Fill);
        if self.settings.busy && !self.settings.saving {
            content = content.push(caption("Loading settings…"));
        }
        // Feedback stays near the toolbar, without reserving an empty footer.
        let mut feedback = column![].spacing(8).width(Length::Fill);
        let has_feedback = self.settings.error.is_some()
            || !self.valid_gif_seconds()
            || !self.valid_presence_seconds()
            || self.settings.confirm_discard.is_some();
        if let Some(error) = &self.settings.error {
            feedback = feedback.push(caption(error).style(shell::danger_text));
        }
        if !self.valid_gif_seconds() || !self.valid_presence_seconds() {
            let category = if !self.valid_gif_seconds() {
                Category::Media
            } else {
                Category::Advanced
            };
            feedback = feedback.push(
                button(text(format!("Review invalid values in {}", category.label())).size(12))
                    .keyboard_navigation(true)
                    .style(shell::quiet_button)
                    .on_press(message(Event::Category(category))),
            );
        }
        if let Some(confirm) = &self.settings.confirm_discard {
            let serial = confirm.serial;
            feedback = feedback.push(
                text(match &confirm.action {
                    DiscardAction::Close => "Discard your unsaved settings changes?",
                    DiscardAction::Transition(_) => {
                        "Close Interconnect and discard your unsaved settings changes?"
                    }
                    DiscardAction::Reload => "Reload the file and discard your unsaved changes?",
                    DiscardAction::ReloadProfile => {
                        "Reload your profile and discard unsaved profile changes?"
                    }
                })
                .size(13),
            );
            feedback = feedback.push(
                row![
                    iced::widget::space().width(Length::Fill),
                    button(text("Keep editing").size(13))
                        .keyboard_navigation(true)
                        .padding([7, 14])
                        .style(shell::quiet_button)
                        .on_press(message(Event::KeepEditing(serial))),
                    button(
                        text(match &confirm.action {
                            DiscardAction::Close => "Discard changes",
                            DiscardAction::Reload => "Reload file",
                            DiscardAction::ReloadProfile => "Reload profile",
                            DiscardAction::Transition(_) => "Discard and close",
                        })
                        .size(13)
                    )
                    .keyboard_navigation(true)
                    .padding([7, 14])
                    .style(button::danger)
                    .on_press(message(Event::DiscardChanges(serial))),
                ]
                .spacing(8),
            );
        }
        let close = super::tooltips::icon_button(
            Kind::Close,
            "Close settings",
            (!self.settings.saving).then_some(message(Event::RequestClose)),
        );
        let actions = container(
            row![
                container(save).id("settings-save"),
                container(close).id("settings-close"),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .id("settings-actions");
        let categories = row(Category::ALL.into_iter().map(|category| {
            button(text(category.label()).size(13))
                .keyboard_navigation(true)
                .padding([7, 10])
                .style(move |theme, status| {
                    shell::navigation_button(theme, self.settings.category == category, status)
                })
                .on_press_maybe(
                    (!self.settings.saving).then_some(message(Event::Category(category))),
                )
                .into()
        }))
        .spacing(4)
        .wrap();
        let title = row![
            shell::title("Settings", 20),
            text(if self.settings.saving {
                "Saving…"
            } else if self.settings.dirty() {
                "Unsaved changes"
            } else {
                ""
            })
            .size(12)
            .style(shell::muted_text),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);
        let header: Element<'_, Message> = if size.width >= 900.0 {
            row![
                title,
                iced::widget::space().width(24),
                categories,
                iced::widget::space().width(Length::Fill),
                actions
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            column![
                row![title, iced::widget::space().width(Length::Fill), actions]
                    .align_y(iced::Alignment::Center),
                categories,
            ]
            .spacing(8)
            .into()
        };
        let mut panel = column![
            container(header)
                .padding([12, 20])
                .width(Length::Fill)
                .id("settings-header"),
            iced::widget::rule::horizontal(1),
        ];
        if has_feedback {
            panel = panel
                .push(container(feedback).padding([10, 20]).width(Length::Fill))
                .push(iced::widget::rule::horizontal(1));
        }
        panel = panel.push(
            scrollable(container(content).padding(24).width(Length::Fill))
                .id("settings-body")
                .direction(shell::scrollbar())
                .style(shell::scroll_style)
                .height(Length::Fill),
        );
        // Fill the workspace and swallow empty-space input so the retained shell
        // cannot scroll or receive pointer actions beneath preferences.
        opaque(
            mouse_area(
                container(panel)
                    .id("settings-dialog")
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .clip(true)
                    .style(|theme| shell::surface(theme, shell::palette(theme).background)),
            )
            .on_press(message(Event::Ignore))
            .on_scroll(|_| message(Event::Ignore)),
        )
    }
}

fn caption<'a>(label: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(label)
        .size(12)
        .style(shell::muted_text)
        .width(Length::Fill)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_changes_preserve_drafts_and_dirty_close_requires_a_choice() {
        let mut app = super::super::tests::demo_app();
        let selection = app.selection.clone();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::Theme(ThemeMode::Dark));
        let _ = app.settings_update(Event::Category(Category::Media));
        let _ = app.settings_update(Event::GifPlayback(GifPlayback::Duration));
        let _ = app.settings_update(Event::GifSeconds("invalid".into()));
        let draft = app.settings.draft.clone();
        let _ = app.settings_update(Event::Category(Category::General));
        assert_eq!(app.settings.draft, draft);
        assert_eq!(app.settings.gif_seconds_input, "invalid");
        let _ = app.settings_update(Event::RequestClose);
        assert!(app.settings.draft.is_some());
        assert!(matches!(
            app.settings.confirm_discard.as_ref(),
            Some(Discard {
                action: DiscardAction::Close,
                ..
            })
        ));
        let _ = app.settings_update(Event::KeepEditing(
            app.settings.confirm_discard.as_ref().unwrap().serial,
        ));
        assert!(app.settings.confirm_discard.is_none());
        assert_eq!(app.settings.draft, draft);
        let _ = app.settings_update(Event::RequestClose);
        let _ = app.settings_update(Event::DiscardChanges(
            app.settings.confirm_discard.as_ref().unwrap().serial,
        ));
        assert!(app.settings.draft.is_none());
        assert_eq!(app.settings.current, Settings::default());
        assert_eq!(app.selection, selection);
        let _ = app.settings_update(Event::Open);
        assert_eq!(app.settings.category, Category::General);
        let _ = app.settings_update(Event::RequestClose);
        assert!(
            app.settings.draft.is_none(),
            "clean forms close immediately"
        );
    }

    #[test]
    fn escape_from_each_discard_prompt_keeps_the_form_and_cancels_the_transition() {
        let (_directory, mut app) = loaded_app();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::Theme(ThemeMode::Dark));
        let draft = app.settings.draft.clone();
        for action in [
            DiscardAction::Close,
            DiscardAction::Reload,
            DiscardAction::Transition(Box::new(Message::Close)),
        ] {
            app.settings.request_discard(action);
            let stale = app.settings.confirm_discard.as_ref().unwrap().serial;
            let _ = app.settings_update(Event::RequestClose);
            assert!(app.settings.confirm_discard.is_none());
            assert_eq!(app.settings.draft, draft);
            assert!(!app.settings.busy && !app.closing);
            let _ = app.settings_update(Event::DiscardChanges(stale));
            assert_eq!(app.settings.draft, draft);
            assert!(!app.settings.busy && !app.closing);
        }
    }

    #[test]
    fn focused_duration_input_keeps_a_steady_caret_without_redraw_deadlines() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = super::super::tests::demo_app();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::Category(Category::Media));
        let _ = app.settings_update(Event::GifPlayback(GifPlayback::Duration));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut view = app.settings_view();
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(640.0, 480.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        view.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut widget::operation::focusable::focus("settings-gif-seconds".into()),
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
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            assert!(messages.is_empty());
        }
    }

    #[test]
    fn application_exit_captures_choice_and_rejects_old_confirmations() {
        let mut app = super::super::tests::demo_app();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::Theme(ThemeMode::Dark));
        assert!(app.guard_settings_transition(Message::Close));
        let old = app.settings.confirm_discard.as_ref().unwrap().serial;
        assert!(!app.closing);
        let _ = app.settings_update(Event::RequestClose);
        assert!(
            app.settings.confirm_discard.is_none(),
            "Escape keeps editing"
        );
        assert!(app.settings.draft.is_some());
        let _ = app.settings_update(Event::RequestClose);
        let current = app.settings.confirm_discard.as_ref().unwrap().serial;
        assert_ne!(old, current);
        let _ = app.settings_update(Event::DiscardChanges(old));
        assert!(app.settings.draft.is_some());
        assert!(!app.closing);
        let _ = app.settings_update(Event::KeepEditing(current));
        assert_eq!(
            app.settings.draft.as_ref().unwrap().appearance.theme,
            ThemeMode::Dark
        );
        assert!(app.guard_settings_transition(Message::Exit));
        let exit = app.settings.confirm_discard.as_ref().unwrap().serial;
        assert!(
            matches!(&app.settings.confirm_discard.as_ref().unwrap().action,
            DiscardAction::Transition(next) if matches!(next.as_ref(), Message::Exit))
        );
        // Inspect the normal asynchronous-close state; demo exits immediately.
        app.demo = None;
        let _ = app.settings_update(Event::DiscardChanges(exit));
        assert!(app.settings.draft.is_none());
        assert!(app.closing);
        assert_eq!(app.settings.current.appearance.theme, ThemeMode::Light);
    }

    #[test]
    fn reloading_an_open_form_requires_discard_and_stages_until_save() {
        let (_directory, mut app) = loaded_app();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::Theme(ThemeMode::Dark));
        let _ = app.settings_update(Event::Reload);
        assert!(!app.settings.busy);
        assert!(matches!(
            app.settings.confirm_discard.as_ref(),
            Some(Discard {
                action: DiscardAction::Reload,
                ..
            })
        ));
        let _ = app.settings_update(Event::KeepEditing(
            app.settings.confirm_discard.as_ref().unwrap().serial,
        ));
        assert_eq!(
            app.settings.draft.as_ref().unwrap().appearance.theme,
            ThemeMode::Dark
        );
        let _ = app.settings_update(Event::Reload);
        let _ = app.settings_update(Event::DiscardChanges(
            app.settings.confirm_discard.as_ref().unwrap().serial,
        ));
        assert!(app.settings.busy);
        let request = app.settings.request;
        let mut file_settings = Settings::default();
        file_settings.composer.send_shortcut = SendShortcut::Enter;
        let mut store = app.settings.store.clone().unwrap();
        store.save(&file_settings).unwrap();
        let _ = app.settings_update(Event::Loaded(request, Ok((store, file_settings.clone()))));
        assert_eq!(app.settings.draft, Some(file_settings));
        assert_eq!(app.settings.current, Settings::default());
        let _ = app.settings_update(Event::Cancel);
        assert_eq!(app.settings.current, Settings::default());
    }

    #[test]
    fn theme_changes_apply_on_save_and_preserve_conversation_state() {
        use interconnect::settings::ThemeMode;
        let mut app = crate::tests::demo_app();
        let selection = app.selection.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent theme test");
        app.scroll_positions.insert(key.clone(), 51.0);
        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::Theme(ThemeMode::Dark)));
        assert_eq!(app.settings.current.appearance.theme, ThemeMode::Light);
        let _ = app.update(Message::Settings(Event::Cancel));
        assert_eq!(app.settings.current.appearance.theme, ThemeMode::Light);
        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::Theme(ThemeMode::Dark)));
        let _ = app.update(Message::Settings(Event::Save));
        assert_eq!(app.settings.current.appearance.theme, ThemeMode::Dark);
        assert_eq!(app.selection, selection);
        assert_eq!(
            app.drafts[&key].content.text().trim_end(),
            "Unsent theme test"
        );
        assert_eq!(app.scroll_positions[&key], 51.0);
        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::Defaults));
        let _ = app.update(Message::Settings(Event::Save));
        assert_eq!(app.settings.current.appearance.theme, ThemeMode::Light);
    }

    #[test]
    fn presence_polling_validates_interval_and_applies_on_save_with_cancel_and_defaults() {
        let mut app = super::super::tests::demo_app();
        let _ = app.settings_update(Event::Open);
        assert_eq!(app.settings.presence_seconds_input, "60");
        let _ = app.settings_update(Event::PresencePolling(true));
        for invalid in ["", "0", "14", "3601", "1.5", "bad"] {
            let _ = app.settings_update(Event::PresenceSeconds(invalid.into()));
            assert!(!app.valid_presence_seconds());
            let _ = app.settings_update(Event::Save);
            assert!(app.settings.draft.is_some());
            assert!(!app.settings.current.presence.poll_enabled);
        }
        let _ = app.settings_update(Event::PresenceSeconds("120".into()));
        let _ = app.settings_update(Event::Cancel);
        assert_eq!(app.settings.current.presence, settings::Presence::default());
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::PresencePolling(true));
        let _ = app.settings_update(Event::PresenceSeconds("120".into()));
        let _ = app.settings_update(Event::Save);
        assert!(app.settings.current.presence.poll_enabled);
        assert_eq!(app.settings.current.presence.poll_interval_seconds, 120);
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::Defaults);
        assert_eq!(app.settings.presence_seconds_input, "60");
        let _ = app.settings_update(Event::Save);
        assert_eq!(app.settings.current.presence, settings::Presence::default());
    }

    #[test]
    fn notification_preferences_and_focus_reach_backend_only_when_applied() {
        let mut app = super::super::tests::demo_app();
        let (control, backend) = tokio::sync::watch::channel(app.selection.clone());
        app.control = Some(control);
        let _ = app.update(Message::WindowFocused(true));
        assert!(backend.borrow().conversation_visible);
        let _ = app.update(message(Event::Open));
        assert!(!backend.borrow().conversation_visible);
        let _ = app.update(message(Event::NotificationsEnabled(false)));
        let _ = app.update(message(Event::NotificationPreview(false)));
        assert!(backend.borrow().notifications.enabled);
        assert!(backend.borrow().notifications.show_preview);
        let _ = app.update(Message::Escape);
        assert!(app.settings.draft.is_some());
        let serial = app.settings.confirm_discard.as_ref().unwrap().serial;
        let _ = app.update(message(Event::DiscardChanges(serial)));
        assert!(backend.borrow().conversation_visible);
        assert!(backend.borrow().notifications.enabled);
        let _ = app.update(message(Event::Open));
        let _ = app.update(message(Event::NotificationsEnabled(false)));
        let _ = app.update(message(Event::NotificationPreview(false)));
        let _ = app.update(message(Event::Save));
        assert!(!backend.borrow().notifications.enabled);
        assert!(!backend.borrow().notifications.show_preview);
        assert!(backend.borrow().conversation_visible);
        let _ = app.update(Message::WindowFocused(false));
        assert!(!backend.borrow().conversation_visible);
    }

    #[test]
    fn gif_settings_validate_duration_and_apply_only_on_save() {
        let mut app = super::super::tests::demo_app();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::GifPlayback(GifPlayback::Duration));
        for invalid in ["", "0", "301", "1.5", "abc"] {
            let _ = app.settings_update(Event::GifSeconds(invalid.into()));
            assert!(!app.valid_gif_seconds());
            let _ = app.settings_update(Event::Save);
            assert!(app.settings.draft.is_some());
            assert_eq!(app.settings.current.media.gif_playback, GifPlayback::Once);
        }
        let _ = app.settings_update(Event::GifSeconds("17".into()));
        let _ = app.settings_update(Event::Save);
        assert!(app.settings.draft.is_none());
        assert_eq!(
            app.settings.current.media.gif_playback,
            GifPlayback::Duration
        );
        assert_eq!(app.settings.current.media.gif_seconds, 17);
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::Defaults);
        assert_eq!(app.settings.gif_seconds_input, "5");
        let _ = app.settings_update(Event::Cancel);
        assert_eq!(app.settings.current.media.gif_seconds, 17);
    }

    #[test]
    fn startup_reads_graphics_before_boot_and_demo_skips_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        std::fs::write(&path, "[graphics]\ngpu_enabled = false\nlow_power = false\n[navigation]\nshow_room_list = false").unwrap();
        let startup = Startup::load(Some(path.clone()), false);
        assert!(!startup.graphics().gpu_enabled && !startup.graphics().low_power);
        let (app, _) = App::boot(startup);
        assert!(!app.settings.current.graphics.gpu_enabled);
        assert!(!app.settings.graphics_restart_required());
        assert!(!app.shell.navigation);
        assert_eq!(app.settings.path.as_ref(), Some(&path));
        assert!(app.settings.store.is_some());
        std::fs::write(&path, "[graphics]\ngpu_enabled = 'broken'").unwrap();
        let startup = Startup::load(Some(path.clone()), false);
        assert_eq!(startup.graphics(), Graphics::default());
        let (app, _) = App::boot(startup);
        assert!(app.settings.error.is_some());
        assert!(app.settings.store.is_none());
        let demo = Startup::load(Some(path), true);
        assert!(demo.path.is_none() && demo.loaded.is_none());
        assert_eq!(demo.graphics(), Graphics::default());
        let (app, _) = App::boot(demo);
        assert!(app.demo.is_some());
        assert!(!app.model.rooms.is_empty());
        assert!(app.settings.path.is_none() && app.settings.store.is_none());
    }

    #[test]
    fn graphics_edits_cancel_save_and_reload_require_restart_without_changing_session() {
        let (_directory, mut app) = loaded_app();
        let selection = app.selection.clone();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::GpuEnabled(false));
        let _ = app.settings_update(Event::LowPower(false));
        assert!(
            app.settings.draft.as_ref().unwrap().graphics.low_power,
            "disabled control retains its value"
        );
        assert!(!app.settings.graphics_restart_required());
        let _ = app.settings_update(Event::Cancel);
        assert_eq!(app.settings.current.graphics, Graphics::default());
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::LowPower(false));
        let _ = app.settings_update(Event::GpuEnabled(false));
        let _ = app.settings_update(Event::Save);
        let request = app.settings.request;
        let draft = app.settings.draft.clone().unwrap();
        let _ = app.settings_update(Event::Saved(request, draft, Err("disk full".into())));
        assert!(!app.settings.graphics_restart_required());
        let _ = app.settings_update(Event::Save);
        let request = app.settings.request;
        let draft = app.settings.draft.clone().unwrap();
        let mut store = app.settings.store.clone().unwrap();
        store.save(&draft).unwrap();
        let _ = app.settings_update(Event::Saved(request, draft.clone(), Ok(store)));
        assert!(app.settings.graphics_restart_required());
        assert_eq!(app.settings.current.graphics, draft.graphics);
        assert_eq!(app.selection, selection);
        assert!(!app.closing);
        let startup = Startup::load(app.settings.path.clone(), false);
        assert_eq!(startup.graphics(), draft.graphics);
        let (restarted, _) = App::boot(startup);
        assert!(!restarted.settings.graphics_restart_required());
        // Reloading the original values clears a pending restart notice.
        let mut store = app.settings.store.clone().unwrap();
        store.save(&Settings::default()).unwrap();
        app.settings.busy = true;
        let _ = app.settings_update(Event::Loaded(
            request,
            Store::load(app.settings.path.clone().unwrap()).map_err(|e| e.to_string()),
        ));
        assert!(!app.settings.graphics_restart_required());
    }

    #[test]
    fn cancel_and_save_keep_settings_and_conversation_state_separate() {
        let mut app = super::super::tests::demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 84.0);
        let selection = app.selection.clone();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::SendShortcut(SendShortcut::Enter));
        let _ = app.settings_update(Event::ShowRoomList(false));
        let _ = app.settings_update(Event::ShowUnreadSection(false));
        let _ = app.settings_update(Event::UnreadFirst(false));
        let _ = app.settings_update(Event::LimitRoomWidth(false));
        let _ = app.update(Message::Room("another-room".into()));
        let _ = app.update(Message::Send(key.clone()));
        let _ = app.update(Message::ToggleNavigation);
        let _ = app.update(Message::SearchFocus);
        let _ = app.update(Message::Escape);
        assert!(app.settings.draft.is_some());
        let serial = app.settings.confirm_discard.as_ref().unwrap().serial;
        let _ = app.update(message(Event::DiscardChanges(serial)));
        assert!(app.settings.draft.is_none());
        assert_eq!(app.settings.current, Settings::default());
        assert_eq!(app.selection, selection);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "unsent");
        assert_eq!(app.scroll_positions[&key], 84.0);
        assert!(app.shell.navigation);
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::SendShortcut(SendShortcut::Enter));
        let _ = app.settings_update(Event::ShowRoomList(false));
        let _ = app.settings_update(Event::ShowUnreadSection(false));
        let _ = app.settings_update(Event::UnreadFirst(false));
        let _ = app.settings_update(Event::LimitRoomWidth(false));
        let _ = app.settings_update(Event::Save);
        assert!(app.settings.draft.is_none());
        assert_eq!(
            app.settings.current.composer.send_shortcut,
            SendShortcut::Enter
        );
        assert!(!app.settings.current.navigation.show_room_list);
        assert!(!app.settings.current.navigation.show_unread_section);
        assert!(!app.settings.current.navigation.unread_first);
        assert!(!app.settings.current.appearance.limit_room_width);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "unsent");
        assert_eq!(app.scroll_positions[&key], 84.0);
        // Startup-only visibility must not disturb this session's scroll layout.
        assert!(app.shell.navigation);
        assert!(app.settings.path.is_none());
        assert!(app.settings.store.is_none());
    }

    fn loaded_app() -> (tempfile::TempDir, App) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        let mut app = App::new();
        app.demo = None;
        app.settings.path = Some(path.clone());
        app.settings.busy = true;
        app.settings.initial_load = true;
        app.settings.request = 1;
        let _ = app.settings_update(Event::Loaded(
            1,
            Store::load(path).map_err(|e| e.to_string()),
        ));
        (directory, app)
    }

    #[test]
    fn load_errors_retain_working_settings_and_block_saving_until_repaired() {
        let (_directory, mut app) = loaded_app();
        let _ = app.settings_update(Event::Open);
        app.settings.busy = true;
        app.settings.request = 2;
        let _ = app.settings_update(Event::Loaded(1, Err("stale".into())));
        assert!(app.settings.busy);
        assert!(app.settings.error.is_none());
        let _ = app.settings_update(Event::Loaded(2, Err("broken TOML".into())));
        assert!(!app.settings.busy);
        assert!(app.settings.store.is_none());
        assert_eq!(app.settings.current, Settings::default());
        let _ = app.settings_update(Event::Save);
        assert!(!app.settings.saving);
        let path = app.settings.path.clone().unwrap();
        app.settings.busy = true;
        app.settings.request = 3;
        let _ = app.settings_update(Event::Loaded(
            3,
            Store::load(path).map_err(|e| e.to_string()),
        ));
        assert!(app.settings.error.is_none());
        assert!(app.settings.store.is_some());
    }

    #[test]
    fn save_completion_is_single_flight_and_failed_saves_do_not_apply_or_close() {
        let (_directory, mut app) = loaded_app();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::SendShortcut(SendShortcut::Enter));
        // Drop the task before polling it; inject the completion to exercise UI ordering.
        let _ = app.settings_update(Event::Save);
        let request = app.settings.request;
        assert!(app.settings.saving);
        let _ = app.settings_update(Event::Save);
        let _ = app.settings_update(Event::Reload);
        let _ = app.update(Message::Escape);
        assert_eq!(app.settings.request, request);
        assert!(app.settings.draft.is_some());
        let _ = app.update(Message::Close);
        assert!(!app.closing);
        assert!(!app.selection.shutdown);
        let _ = app.settings_update(Event::Saved(
            request - 1,
            Settings::default(),
            Err("stale".into()),
        ));
        assert!(app.settings.saving);
        let draft = app.settings.draft.clone().unwrap();
        let _ = app.settings_update(Event::Saved(request, draft, Err("disk full".into())));
        assert!(!app.settings.saving);
        assert!(app.settings.draft.is_some());
        assert_eq!(app.settings.current, Settings::default());
        assert_eq!(app.settings.error.as_deref(), Some("disk full"));
        assert!(!app.closing);
        // Successful retry applies the draft, then honors a deferred close request.
        let _ = app.settings_update(Event::Save);
        let request = app.settings.request;
        let draft = app.settings.draft.clone().unwrap();
        let _ = app.update(Message::Close);
        let _ = app.settings_update(Event::Saved(
            request,
            draft,
            Ok(app.settings.store.clone().unwrap()),
        ));
        assert!(app.closing);
        assert!(app.selection.shutdown);
        assert!(app.settings.draft.is_none());
        assert_eq!(
            app.settings.current.composer.send_shortcut,
            SendShortcut::Enter
        );
    }

    #[test]
    fn startup_visibility_does_not_override_user_navigation_and_reload_is_not_startup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        std::fs::write(&path, "[navigation]\nshow_room_list = false").unwrap();
        for touched in [false, true] {
            let mut app = App::new();
            app.settings.busy = true;
            app.settings.initial_load = true;
            app.settings.navigation_touched = touched;
            let loaded = Store::load(path.clone()).unwrap();
            let _ = app.settings_update(Event::Loaded(0, Ok(loaded)));
            assert_eq!(app.shell.navigation, touched);
            app.shell.navigation = true;
            app.settings.busy = true;
            let loaded = Store::load(path.clone()).unwrap();
            let _ = app.settings_update(Event::Loaded(0, Ok(loaded)));
            assert!(app.shell.navigation);
        }
    }

    #[test]
    fn opening_and_closing_settings_preserves_the_live_timeline_widget_offset() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe(Option<f32>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.0 = Some((content.height - bounds.height).max(0.0) - translation.y);
                }
            }
        }
        fn inspect(app: &App, tree: &mut widget::Tree, set: bool) -> Option<f32> {
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut view = app.view();
            tree.diff(view.as_widget());
            let size = iced::Size::new(1240.0, 820.0);
            let node =
                view.as_widget_mut()
                    .layout(tree, &renderer, &layout::Limits::new(size, size));
            if set {
                view.as_widget_mut().operate(
                    tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        widget::Id::new("room-timeline"),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(120.0),
                        },
                    ),
                );
            }
            let mut probe = Probe::default();
            view.as_widget_mut()
                .operate(tree, layout::Layout::new(&node), &renderer, &mut probe);
            probe.0
        }
        let mut app = super::super::tests::demo_app();
        let _ = app.update(Message::Room("busy".into()));
        let mut tree = widget::Tree::empty();
        assert!((inspect(&app, &mut tree, true).unwrap() - 120.0).abs() < 1.0);
        let _ = app.settings_update(Event::Open);
        assert!(
            inspect(&app, &mut tree, false).is_none(),
            "covered conversation widgets do not participate in operations"
        );
        let _ = app.settings_update(Event::Cancel);
        assert!((inspect(&app, &mut tree, false).unwrap() - 120.0).abs() < 1.0);
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            let _ = app.settings_update(Event::Open);
            let _ = app.settings_update(Event::Theme(mode));
            let _ = app.settings_update(Event::Save);
            assert_eq!(app.settings.current.appearance.theme, mode);
            assert!((inspect(&app, &mut tree, false).unwrap() - 120.0).abs() < 1.0);
        }
    }

    #[test]
    fn preferences_panel_fills_window_and_captures_empty_space_at_minimum_size() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = super::super::tests::demo_app();
        let _ = app.settings_update(Event::Open);
        let _ = app.settings_update(Event::Category(Category::Notifications));
        let mut view = app.view();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(640.0, 480.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        assert_eq!(node.size(), size);
        #[derive(Default)]
        struct Probe {
            panel: Option<iced::Rectangle>,
            header: Option<iced::Rectangle>,
            actions: Option<iced::Rectangle>,
            body: Option<iced::Rectangle>,
            save: Option<iced::Rectangle>,
            close: Option<iced::Rectangle>,
            captions: Vec<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                match id {
                    Some(id) if id == &widget::Id::new("settings-dialog") => {
                        self.panel = Some(bounds)
                    }
                    Some(id) if id == &widget::Id::new("settings-header") => {
                        self.header = Some(bounds)
                    }
                    Some(id) if id == &widget::Id::new("settings-actions") => {
                        self.actions = Some(bounds)
                    }
                    Some(id) if id == &widget::Id::new("settings-save") => self.save = Some(bounds),
                    Some(id) if id == &widget::Id::new("settings-close") => {
                        self.close = Some(bounds)
                    }
                    _ => {}
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: iced::Rectangle,
                _: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("settings-body")) {
                    self.body = Some(bounds);
                }
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, value: &str) {
                if value.starts_with("The sample uses") {
                    self.captions.push(bounds);
                }
            }
        }
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
        assert_eq!(probe.panel.unwrap(), iced::Rectangle::with_size(size));
        let header = probe.header.unwrap();
        let actions = probe.actions.unwrap();
        let body = probe.body.unwrap();
        assert!(
            header.height <= 100.0,
            "compact heading and category navigation"
        );
        assert!(body.height >= 375.0, "the body gets all remaining height");
        assert_eq!(body.y + body.height, size.height, "no permanent footer");
        assert!(actions.y + actions.height <= body.y);
        for bounds in [probe.save.unwrap(), probe.close.unwrap()] {
            assert!(bounds.y >= actions.y && bounds.y + bounds.height <= body.y);
            assert!(bounds.width <= 36.0, "routine actions use compact icons");
        }
        assert_eq!(probe.captions.len(), 1);
        let caption = probe.captions[0];
        assert!(caption.x >= body.x && caption.x + caption.width <= body.x + body.width);
        let event = iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
        });
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &event,
            layout::Layout::new(&node),
            mouse::Cursor::Available(iced::Point::new(5.0, 40.0)),
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        assert!(shell.is_event_captured());
        assert!(
            messages
                .iter()
                .all(|message| matches!(message, Message::Settings(Event::Ignore)))
        );
    }
}
