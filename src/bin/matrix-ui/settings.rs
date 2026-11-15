//! Settings dialog and asynchronous file tasks. No subscriptions or timers.
use super::{App, Message, shell};
use chatclient::settings::{self, SendShortcut, Settings, Store};
use iced::{
    Element, Length, Task,
    widget::{
        button, checkbox, column, container, mouse_area, opaque, pick_list, row, scrollable, text,
    },
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
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Cancel,
    ShowRoomList(bool),
    SendShortcut(SendShortcut),
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
    pub(super) fn boot(path: Option<PathBuf>) -> (Self, Task<Message>) {
        let mut app = Self::new();
        if app.demo.is_some() {
            return (app, Task::none());
        }
        app.settings.initial_load = true;
        match path.map(Ok).unwrap_or_else(settings::default_path) {
            Ok(path) => {
                app.settings.path = Some(path);
                let task = app.load_settings();
                (app, task)
            }
            Err(error) => {
                app.settings.error = Some(format!("{error:#}"));
                (app, Task::none())
            }
        }
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
            Event::Open if self.settings.draft.is_none() && !self.closing => {
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
            Event::Defaults if !self.settings.busy => {
                if let Some(draft) = &mut self.settings.draft {
                    *draft = Settings::default();
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
                        if initial && !self.settings.navigation_touched {
                            self.shell.navigation = settings.navigation.show_room_list;
                        }
                        if self.settings.draft.is_some() {
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
            if self.demo.is_some() || self.settings.store.is_some() {
                save = save.on_press(message(Event::Save));
            }
        }
        if !self.settings.saving {
            cancel = cancel.on_press(message(Event::Cancel));
        }
        let mut content = column![
            shell::title("Settings", 24),
            text("Changes apply when you save. Startup options apply on the next launch.")
                .size(13)
                .color(shell::MUTED),
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
            let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
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
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
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
