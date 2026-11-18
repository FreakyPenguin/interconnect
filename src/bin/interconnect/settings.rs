//! Startup settings and asynchronous dialog reload/save tasks. No timers.
use super::{
    App, Message,
    icons::{Kind, icon},
    shell,
};
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
}
impl State {
    pub(super) fn graphics_restart_required(&self) -> bool {
        self.current.graphics != self.launched_graphics
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
    Open,
    Cancel,
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
            return (app, demo_code);
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
            Event::Open if self.settings.draft.is_none() && !self.closing => {
                self.settings.gif_seconds_input =
                    self.settings.current.media.gif_seconds.to_string();
                self.settings.presence_seconds_input = self
                    .settings
                    .current
                    .presence
                    .poll_interval_seconds
                    .to_string();
                self.settings.draft = Some(self.settings.current.clone());
                // Remove focus from composers/search while the modal is visible.
                return iced::widget::operation::focus("settings-dialog");
            }
            Event::Cancel if !self.settings.saving => {
                self.settings.draft = None;
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
            Event::Reload if self.settings.draft.is_some() => return self.load_settings(),
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
                        self.settings.current = settings;
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
                    return self.settings_update(Event::Cancel);
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
                        return self.settings_update(Event::Cancel);
                    }
                    Err(error) => self.settings.error = Some(error),
                }
            }
            _ => {}
        }
        Task::none()
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
        iced::widget::responsive(move |size| self.settings_card(size)).into()
    }
    fn settings_card(&self, size: iced::Size) -> Element<'_, Message> {
        let draft = self.settings.draft.as_ref().expect("open dialog");
        let available = !self.settings.busy;
        let mut notifications = checkbox(draft.notifications.enabled)
            .label("Enable desktop notifications")
            .size(16)
            .text_size(13)
            .spacing(9);
        let mut previews = checkbox(draft.notifications.show_preview)
            .label("Show room, sender, and message text")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            notifications =
                notifications.on_toggle(|value| message(Event::NotificationsEnabled(value)));
            previews = previews.on_toggle(|value| message(Event::NotificationPreview(value)));
        }
        let mut notification_test = button(
            text(if self.settings.notification_test_busy {
                "Sending sample…"
            } else {
                "Send sample notification"
            })
            .size(13),
        )
        .padding([7, 10])
        .style(shell::quiet_button);
        if !self.settings.notification_test_busy {
            notification_test = notification_test.on_press(message(Event::TestNotification));
        }
        let mut visibility = checkbox(draft.navigation.show_room_list)
            .label("Show room list at startup")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            visibility = visibility.on_toggle(|show| message(Event::ShowRoomList(show)));
        }
        let mut presence_polling = checkbox(draft.presence.poll_enabled)
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
            .label("Show unread section")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            unread_section =
                unread_section.on_toggle(|show| message(Event::ShowUnreadSection(show)));
        }
        let mut unread_first = checkbox(draft.navigation.unread_first)
            .label("Show unread rooms first in each section")
            .size(16)
            .text_size(13)
            .spacing(9);
        if available {
            unread_first = unread_first.on_toggle(|enabled| message(Event::UnreadFirst(enabled)));
        }
        let mut room_width = checkbox(draft.appearance.limit_room_width)
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
        .text_size(13)
        .padding([7, 10])
        .style(shell::select_style);
        let mut gpu = checkbox(draft.graphics.gpu_enabled)
            .label("Use GPU rendering")
            .size(16)
            .text_size(13)
            .spacing(9);
        let mut low_power = checkbox(draft.graphics.low_power)
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
        let mut reload = button(text("Reload file").size(13))
            .padding([7, 10])
            .style(shell::quiet_button);
        let mut defaults = button(text("Use defaults").size(13))
            .padding([7, 10])
            .style(shell::quiet_button);
        let mut cancel = button(text("Cancel").size(13))
            .padding([7, 14])
            .style(shell::quiet_button);
        let mut save = button(
            text(if self.settings.saving {
                "Saving…"
            } else {
                "Save"
            })
            .size(13),
        )
        .padding([7, 18])
        .style(shell::primary_button);
        if available {
            defaults = defaults.on_press(message(Event::Defaults));
            if self.settings.path.is_some() {
                reload = reload.on_press(message(Event::Reload));
            }
            if (self.demo.is_some() || self.settings.store.is_some())
                && self.valid_gif_seconds()
                && self.valid_presence_seconds()
            {
                save = save.on_press(message(Event::Save));
            }
        }
        if !self.settings.saving {
            cancel = cancel.on_press(message(Event::Cancel));
        }
        let playback = pick_list(
            [GifPlayback::Once, GifPlayback::Duration],
            Some(draft.media.gif_playback),
            |value| message(Event::GifPlayback(value)),
        )
        .text_size(13)
        .padding([7, 10])
        .style(shell::select_style);
        let mut gif_controls = row![text("Play").size(13), playback]
            .spacing(12)
            .align_y(iced::Alignment::Center);
        if draft.media.gif_playback == GifPlayback::Duration {
            let mut seconds = text_input("1–300", &self.settings.gif_seconds_input)
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
            notification_controls = notification_controls.push(caption(match result {
                Ok(()) => "Sample accepted by the desktop. Check your notification center.",
                Err(error) => error.as_str(),
            }));
        }
        if let Some(error) = &self.model.notification_error {
            notification_controls =
                notification_controls.push(caption(error).style(shell::danger_text));
        }
        let mut content = column![
            column![shell::title("Notifications", 14), notification_controls].spacing(12),
            iced::widget::rule::horizontal(1),
            column![
                shell::title("Presence", 14),
                presence_controls,
                caption(if self.valid_presence_seconds() {
                    "Refresh all DM contacts, visible message senders, and the People page while focused. Choose 15–3600 seconds; 60 is the default."
                } else {
                    "Enter a whole number from 15 to 3600 seconds to save."
                }),
            ].spacing(12),
            iced::widget::rule::horizontal(1),
            column![
                shell::title("Navigation", 14),
                visibility,
                unread_section,
                caption("Show unread rooms at the top, in addition to their usual sections. Follows the selected space and room filter."),
                unread_first,
                caption("Place unread rooms and direct messages before read ones within their sections. Keep each group in alphabetical order."),
                button(text("Manage emoji packs…").size(13))
                    .padding([7, 10])
                    .style(shell::quiet_button)
                    .on_press_maybe((available && (self.demo.is_some() || self.model.emoji_management.is_some()))
                        .then_some(Message::EmojiManagement(super::emoji_management::Event::Open))),
            ].spacing(12),
            iced::widget::rule::horizontal(1),
            column![
                shell::title("Appearance", 14),
                row![text("Theme").size(13), pick_list(
                    ThemeMode::ALL, Some(draft.appearance.theme),
                    move |mode| message(if available { Event::Theme(mode) } else { Event::Ignore }),
                ).style(shell::select_style)].spacing(16).align_y(iced::Alignment::Center),
                caption("Apply light or dark colors when you save. No restart required."),
                room_width,
                caption("Center headers, messages, and the composer in a readable column. Disable to use the full pane width."),
            ].spacing(12),
            iced::widget::rule::horizontal(1),
            column![
                shell::title("Messages", 14),
                row![text("Send with").size(13), shortcut].spacing(16).align_y(iced::Alignment::Center),
                caption("With Enter to send, use Shift+Enter for a new line. Ctrl+Enter always sends."),
            ].spacing(12),
            iced::widget::rule::horizontal(1),
            column![
                shell::title("GIF playback", 14),
                caption("GIFs stay still until you click Play on a preview or in the image viewer."),
                gif_controls,
                caption(if self.valid_gif_seconds() {
                    "Duration repeats the GIF for 1–300 whole seconds. Once plays one loop."
                } else {
                    "Enter a whole number from 1 to 300 seconds to save."
                }),
            ].spacing(12),
            iced::widget::rule::horizontal(1),
            column![
                row![shell::title("Graphics", 14), text("Restart required").size(11).style(shell::muted_text)]
                    .spacing(10).align_y(iced::Alignment::Center),
                gpu,
                caption("Uses the CPU renderer when disabled. GPU rendering includes a software fallback."),
                low_power,
                caption("Selects a lower-power adapter when GPU rendering is enabled. Changes apply on the next launch."),
            ].spacing(12),
            iced::widget::rule::horizontal(1),
        ].spacing(20).width(Length::Fill);
        if self.demo.is_some() {
            content = content.push(caption(
                "Demo settings stay in memory and disappear on exit.",
            ));
        } else if let Some(path) = &self.settings.path {
            content = content.push(caption(path.display().to_string()).wrapping(text::Wrapping::Glyph))
                .push(caption("Edit this TOML file, then reload to apply it. Reload discards unsaved changes."));
        }
        if self.settings.busy && !self.settings.saving {
            content = content.push(caption("Loading settings…"));
        }
        // Save errors belong next to the persistent actions, rather than below the fold.
        let mut actions = column![].spacing(10).width(Length::Fill);
        if let Some(error) = &self.settings.error {
            actions = actions.push(caption(error).style(shell::danger_text));
        }
        actions =
            actions.push(row![iced::widget::space().width(Length::Fill), cancel, save].spacing(8));
        let footer = container(actions)
            .padding([14, 24])
            .width(Length::Fill)
            .id("settings-actions");
        let close = super::tooltips::tooltip(
            button(container(icon(Kind::Close)).center_x(28).center_y(28))
                .padding(0)
                .style(shell::icon_button)
                .on_press_maybe((!self.settings.saving).then_some(message(Event::Cancel))),
            "Close settings",
            iced::widget::tooltip::Position::Bottom,
        );
        let header = container(
            column![
                row![
                    shell::title("Settings", 20),
                    iced::widget::space().width(Length::Fill),
                    close
                ]
                .align_y(iced::Alignment::Center),
                caption("Changes apply when you save. Startup options apply on the next launch."),
            ]
            .spacing(8),
        )
        .padding([18, 24])
        .width(Length::Fill);
        content = content.push(row![defaults, reload].spacing(8));
        // Constrain content before laying out text so captions wrap within the card.
        let body = scrollable(container(content).padding(24).width(Length::Fill))
            .id("settings-body")
            .direction(shell::scrollbar())
            .style(shell::scroll_style)
            .height(Length::Fill);
        let card = container(
            column![
                header,
                iced::widget::rule::horizontal(1),
                body,
                iced::widget::rule::horizontal(1),
                footer,
            ]
            .spacing(0),
        )
        .width((size.width - 32.0).clamp(1.0, 560.0))
        .height((size.height - 32.0).clamp(1.0, 760.0))
        .clip(true)
        .style(|theme| container::Style {
            // Large shadows in tiny-skia 0.14 repaint outside the damage region.
            // A border gives the card separation without darkening on scroll.
            border: iced::Border {
                color: shell::palette(theme).line,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..shell::surface(theme, shell::palette(theme).background)
        });
        opaque(
            mouse_area(
                container(card)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .padding(16)
                    .style(|theme| shell::surface(theme, shell::palette(theme).scrim)),
            )
            .on_scroll(|_| message(Event::Ignore))
            .on_press(message(Event::Ignore)),
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
        fn inspect(app: &App, tree: &mut widget::Tree, set: bool) -> f32 {
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
            probe.0.unwrap()
        }
        let mut app = super::super::tests::demo_app();
        let _ = app.update(Message::Room("busy".into()));
        let mut tree = widget::Tree::empty();
        assert!((inspect(&app, &mut tree, true) - 120.0).abs() < 1.0);
        let _ = app.settings_update(Event::Open);
        assert!((inspect(&app, &mut tree, false) - 120.0).abs() < 1.0);
        let _ = app.settings_update(Event::Cancel);
        assert!((inspect(&app, &mut tree, false) - 120.0).abs() < 1.0);
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            let _ = app.settings_update(Event::Open);
            let _ = app.settings_update(Event::Theme(mode));
            let _ = app.settings_update(Event::Save);
            assert_eq!(app.settings.current.appearance.theme, mode);
            assert!((inspect(&app, &mut tree, false) - 120.0).abs() < 1.0);
        }
    }

    #[test]
    fn modal_layout_and_backdrop_capture_input_at_minimum_window_size() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = super::super::tests::demo_app();
        let _ = app.settings_update(Event::Open);
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
            actions: Option<iced::Rectangle>,
            body: Option<iced::Rectangle>,
            content: Option<iced::Rectangle>,
            save: Option<iced::Rectangle>,
            cancel: Option<iced::Rectangle>,
            captions: Vec<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("settings-actions")) {
                    self.actions = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                _: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("settings-body")) {
                    self.body = Some(bounds);
                    self.content = Some(content);
                }
            }
            fn text(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, value: &str) {
                match value {
                    "Save" => self.save = Some(bounds),
                    "Cancel" => self.cancel = Some(bounds),
                    value if value.starts_with("The sample uses") => self.captions.push(bounds),
                    _ => {}
                }
            }
        }
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
        let actions = probe.actions.unwrap();
        let body = probe.body.unwrap();
        assert!(body.height > 100.0);
        assert!(
            probe.content.unwrap().height > body.height,
            "only the preferences scroll"
        );
        assert!(body.y + body.height <= actions.y);
        for bounds in [probe.save.unwrap(), probe.cancel.unwrap()] {
            assert!(bounds.y >= actions.y && bounds.y + bounds.height <= size.height - 16.0);
        }
        assert_eq!(probe.captions.len(), 1);
        let caption = probe.captions[0];
        assert!(caption.x >= body.x && caption.x + caption.width <= body.x + body.width);
        assert!(caption.height > 20.0, "long descriptions wrap");
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
