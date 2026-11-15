//! Startup settings and asynchronous dialog reload/save tasks. No timers.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{
        button, checkbox, column, container, mouse_area, opaque, pick_list, row, scrollable, text,
        text_input,
    },
};
use interconnect::settings::{self, GifPlayback, Graphics, SendShortcut, Settings, Store};
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
    SendShortcut(SendShortcut),
    GifPlayback(GifPlayback),
    GifSeconds(String),
    GpuEnabled(bool),
    LowPower(bool),
    NotificationsEnabled(bool),
    NotificationPreview(bool),
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
        self.selection.notifications = self.settings.current.notifications.clone();
        self.selection.conversation_visible = self.window_focused
            && self.settings.draft.is_none()
            && self.room_picker.is_none()
            && self.new_chat.is_none()
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
    pub(super) fn boot(startup: Startup) -> (Self, Task<Message>) {
        let mut app = Self::new();
        let Some(loaded) = startup.loaded else {
            return (app, Task::none());
        };
        app.settings.initial_load = true;
        app.settings.path = startup.path;
        app.settings.busy = true;
        let task = app.settings_update(Event::Loaded(0, loaded));
        app.update_notification_context();
        (app, task)
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
                if !self.valid_gif_seconds() {
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
        let mut notifications =
            checkbox(draft.notifications.enabled).label("Enable desktop notifications");
        let mut previews =
            checkbox(draft.notifications.show_preview).label("Show room, sender, and message text");
        if available {
            notifications =
                notifications.on_toggle(|value| message(Event::NotificationsEnabled(value)));
            previews = previews.on_toggle(|value| message(Event::NotificationPreview(value)));
        }
        let mut notification_test = button(if self.settings.notification_test_busy {
            "Sending sample…"
        } else {
            "Send sample notification"
        })
        .style(shell::quiet_button);
        if !self.settings.notification_test_busy {
            notification_test = notification_test.on_press(message(Event::TestNotification));
        }
        let mut visibility =
            checkbox(draft.navigation.show_room_list).label("Show room list at startup");
        if available {
            visibility = visibility.on_toggle(|show| message(Event::ShowRoomList(show)));
        }
        let shortcut = pick_list(
            [SendShortcut::CtrlEnter, SendShortcut::Enter],
            Some(draft.composer.send_shortcut),
            |shortcut| message(Event::SendShortcut(shortcut)),
        );
        let mut gpu = checkbox(draft.graphics.gpu_enabled).label("Use GPU rendering");
        let mut low_power = checkbox(draft.graphics.low_power).label("Prefer a low-power GPU");
        if available {
            gpu = gpu.on_toggle(|value| message(Event::GpuEnabled(value)));
        }
        if available && draft.graphics.gpu_enabled {
            low_power = low_power.on_toggle(|value| message(Event::LowPower(value)));
        }
        let mut reload = button("Reload file").style(shell::quiet_button);
        let mut defaults = button("Use defaults").style(shell::quiet_button);
        let mut cancel = button("Cancel").style(shell::quiet_button);
        let mut save = button(if self.settings.saving {
            "Saving…"
        } else {
            "Save"
        });
        if available {
            defaults = defaults.on_press(message(Event::Defaults));
            if self.settings.path.is_some() {
                reload = reload.on_press(message(Event::Reload));
            }
            if (self.demo.is_some() || self.settings.store.is_some()) && self.valid_gif_seconds() {
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
        );
        let mut gif_controls = row![text("Play").size(14), playback]
            .spacing(12)
            .align_y(iced::Alignment::Center);
        if draft.media.gif_playback == GifPlayback::Duration {
            let mut seconds = text_input("1–300", &self.settings.gif_seconds_input).width(80);
            if available {
                seconds = seconds.on_input(|value| message(Event::GifSeconds(value)));
            }
            gif_controls = gif_controls.push(seconds).push(text("seconds").size(14));
        }
        let mut notification_controls = column![
            notifications,
            previews,
            text("Alerts follow your Matrix notification rules. The conversation you are viewing stays quiet.").size(12).color(shell::MUTED),
            notification_test,
            text("The sample uses no account or message content. On macOS, allow notifications when prompted.").size(12).color(shell::MUTED),
        ].spacing(12);
        if let Some(result) = &self.settings.notification_test_result {
            notification_controls = notification_controls.push(
                text(match result {
                    Ok(()) => "Sample accepted by the desktop. Check your notification center.",
                    Err(error) => error.as_str(),
                })
                .size(12),
            );
        }
        if let Some(error) = &self.model.notification_error {
            notification_controls = notification_controls.push(
                text(error)
                    .size(12)
                    .color(iced::Color::from_rgb8(174, 73, 78)),
            );
        }
        let mut content = column![
            shell::title("Settings", 24),
            text("Changes apply when you save. Startup options apply on the next launch.")
                .size(13)
                .color(shell::MUTED),
            iced::widget::rule::horizontal(1),
            shell::title("Notifications", 15),
            notification_controls,
            iced::widget::rule::horizontal(1),
            shell::title("Navigation", 15),
            visibility,
            shell::title("Messages", 15),
            row![text("Send with").size(14), shortcut]
                .spacing(16)
                .align_y(iced::Alignment::Center),
            text("With Enter to send, use Shift+Enter for a new line. Ctrl+Enter always sends.")
                .size(12)
                .color(shell::MUTED),
            iced::widget::rule::horizontal(1),
            shell::title("GIF playback", 15),
            text("GIFs stay still until you click Play in the image viewer.").size(12).color(shell::MUTED),
            gif_controls,
            text(if self.valid_gif_seconds() { "Duration repeats the GIF for 1–300 whole seconds. Once plays one loop." } else { "Enter a whole number from 1 to 300 seconds to save." }).size(12).color(shell::MUTED),
            iced::widget::rule::horizontal(1),
            shell::title("Graphics · restart required", 15),
            gpu,
            text("Uses the CPU renderer when disabled. GPU rendering includes a software fallback.").size(12).color(shell::MUTED),
            low_power,
            text("Selects a lower-power adapter when GPU rendering is enabled. Changes apply on the next launch.").size(12).color(shell::MUTED),
            iced::widget::rule::horizontal(1),
        ]
        .spacing(16);
        if self.demo.is_some() {
            content = content.push(
                text("Demo settings stay in memory and disappear on exit.")
                    .size(12)
                    .color(shell::MUTED),
            );
        } else if let Some(path) = &self.settings.path {
            content = content.push(text(path.display().to_string()).size(12))
                .push(text("Edit this TOML file, then reload to apply it. Reload discards unsaved changes.")
                    .size(12).color(shell::MUTED));
        }
        if self.settings.busy && !self.settings.saving {
            content = content.push(text("Loading settings…").size(12));
        }
        if let Some(error) = &self.settings.error {
            content = content.push(
                text(error)
                    .size(12)
                    .color(iced::Color::from_rgb8(174, 73, 78)),
            );
        }
        content = content
            .push(row![defaults, reload].spacing(8))
            .push(row![iced::widget::space().width(Length::Fill), cancel, save].spacing(8));
        // Keep long paths and parser errors scrollable at the minimum window size.
        let card = container(scrollable(content).style(shell::scroll_style))
            .padding(24)
            .max_width(540)
            .max_height((size.height - 32.0).max(1.0))
            .style(|_| container::Style {
                border: iced::Border::default().rounded(12),
                ..shell::surface(iced::Color::WHITE)
            });
        opaque(
            mouse_area(
                container(card)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .padding(16)
                    .style(|_| shell::surface(iced::Color::BLACK.scale_alpha(0.25))),
            )
            .on_scroll(|_| message(Event::Ignore))
            .on_press(message(Event::Ignore)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let _ = app.settings_update(Event::Save);
        assert!(app.settings.draft.is_none());
        assert_eq!(
            app.settings.current.composer.send_shortcut,
            SendShortcut::Enter
        );
        assert!(!app.settings.current.navigation.show_room_list);
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
