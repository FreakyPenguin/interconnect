//! Account-scoped drafts and cancellable reads; admitted writes survive panel closure.
use super::{App, MAX_PENDING_OPERATIONS, Message, avatar_editor, settings, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, row, text, text_input},
};
use interconnect::{
    account_profile::{Change, Details},
    room_avatar,
};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    saved: Option<Details>,
    name: String,
    avatar: avatar_editor::State,
    request: u64,
    active: bool,
    load: Option<iced::task::Handle>,
    writes: HashMap<u64, (u64, Option<String>, Change)>,
    error: Option<String>,
    success: bool,
    reload_required: bool,
    demo: Option<Details>,
}

#[cfg(test)]
mod tests {
    use super::super::settings::{Category, Event as SettingsEvent};
    use super::*;

    fn open() -> App {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Settings(SettingsEvent::Open));
        let _ = app.update(Message::Settings(SettingsEvent::Category(
            Category::Account,
        )));
        assert!(app.settings.profile.saved.is_some());
        app
    }
    fn operation(app: &App) -> u64 {
        *app.settings.profile.writes.keys().next().unwrap()
    }
    fn seed_avatar(app: &mut App) {
        app.settings.profile.saved.as_mut().unwrap().avatar_url =
            Some("mxc://demo.invalid/before".into());
    }
    #[test]
    fn name_save_preserves_avatar_draft_and_its_conflict_baseline() {
        let mut app = open();
        let selection = app.selection.clone();
        let drafts = app.drafts.keys().cloned().collect::<Vec<_>>();
        seed_avatar(&mut app);
        let request = app.settings.profile.request;
        let _ = app.update(message(Event::Avatar(
            request,
            avatar_editor::Event::Remove,
        )));
        let _ = app.update(message(Event::Name("Zoë 🦊".into())));
        let _ = app.update(Message::Settings(SettingsEvent::Category(
            Category::General,
        )));
        let _ = app.update(Message::Settings(SettingsEvent::Category(
            Category::Account,
        )));
        assert_eq!(app.settings.profile.name, "Zoë 🦊");
        let _ = app.update(message(Event::SaveName));
        let write = operation(&app);
        let _ = app.update(message(Event::SaveName));
        assert_eq!(app.pending_ops, 1);
        assert!(
            app.model.people_directory.is_none(),
            "demo needs no SDK handle"
        );
        let _ = app.update(message(Event::Saved(
            write,
            Ok(Details {
                display_name: Some("Zoë 🦊".into()),
                avatar_url: Some("mxc://demo.invalid/remote".into()),
            }),
        )));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.model.account_name, "Zoë 🦊");
        assert!(matches!(app.settings.profile.avatar.edit(), Some(None)));
        assert_eq!(
            app.settings
                .profile
                .saved
                .as_ref()
                .unwrap()
                .avatar_url
                .as_ref()
                .unwrap()
                .as_str(),
            "mxc://demo.invalid/before"
        );
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert_eq!(app.drafts.keys().cloned().collect::<Vec<_>>(), drafts);
    }
    #[test]
    fn avatar_removal_preserves_unsaved_name_and_close_requests_discard() {
        let mut app = open();
        seed_avatar(&mut app);
        let _ = app.update(message(Event::Name("Unsent name".into())));
        let request = app.settings.profile.request;
        let _ = app.update(message(Event::Avatar(
            request,
            avatar_editor::Event::Remove,
        )));
        assert_eq!(app.pending_ops, 0, "removal is local until Save");
        let _ = app.update(message(Event::Avatar(request, avatar_editor::Event::Save)));
        let write = operation(&app);
        let baseline_name = app
            .settings
            .profile
            .saved
            .as_ref()
            .unwrap()
            .display_name
            .clone();
        let _ = app.update(message(Event::Saved(
            write,
            Ok(Details {
                display_name: baseline_name.clone(),
                avatar_url: None,
            }),
        )));
        assert_eq!(app.settings.profile.name, "Unsent name");
        assert_eq!(
            app.settings.profile.saved.as_ref().unwrap().display_name,
            baseline_name
        );
        assert!(app.settings.profile.avatar.edit().is_none());
        assert!(app.settings.profile.dirty());
        let _ = app.update(Message::Settings(SettingsEvent::RequestClose));
        assert!(app.settings.draft.is_some());
        let _ = app.update(Message::Escape);
        assert_eq!(app.settings.profile.name, "Unsent name");
        let _ = app.update(Message::Settings(SettingsEvent::Cancel));
        assert!(!app.settings.profile.active);
        assert!(!app.settings.profile.dirty());
        let _ = app.update(Message::Settings(SettingsEvent::Open));
        let _ = app.update(Message::Settings(SettingsEvent::Category(
            Category::Account,
        )));
        assert_eq!(app.settings.profile.name, baseline_name.unwrap_or_default());
        assert!(
            app.settings
                .profile
                .saved
                .as_ref()
                .unwrap()
                .avatar_url
                .is_none()
        );
    }
    #[test]
    fn failed_saves_require_reload_and_stale_results_cannot_edit_another_account() {
        let mut app = open();
        let _ = app.update(message(Event::Name("New name".into())));
        let _ = app.update(message(Event::SaveName));
        let write = operation(&app);
        let _ = app.update(message(Event::Saved(
            write,
            Err("Reload before retrying.".into()),
        )));
        assert!(app.settings.profile.reload_required);
        assert_eq!(app.settings.profile.name, "New name");
        let _ = app.update(message(Event::SaveName));
        assert_eq!(app.pending_ops, 0);
        let _ = app.account_profile_event(Event::Reload);
        assert!(!app.settings.profile.reload_required);
        let _ = app.update(message(Event::Name("Other name".into())));
        let _ = app.update(message(Event::SaveName));
        let write = operation(&app);
        let old_name = app.model.account_name.clone();
        app.account_id = Some("@another:example.org".into());
        app.settings.profile.reset_account();
        let _ = app.update(message(Event::Saved(
            write,
            Ok(Details {
                display_name: Some("Stale".into()),
                avatar_url: None,
            }),
        )));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.model.account_name, old_name);
        assert!(app.settings.profile.saved.is_none());
    }
    #[test]
    fn reopening_waits_for_an_admitted_write_then_reloads_the_demo_profile() {
        let mut app = open();
        let _ = app.update(message(Event::Name("Saved after close".into())));
        let _ = app.update(message(Event::SaveName));
        let write = operation(&app);
        let old_request = app.settings.profile.request;
        let _ = app.update(Message::Settings(SettingsEvent::Cancel));
        let _ = app.update(Message::Settings(SettingsEvent::Open));
        let _ = app.update(Message::Settings(SettingsEvent::Category(
            Category::Account,
        )));
        assert!(app.settings.profile.active);
        assert!(app.settings.profile.saved.is_none());
        assert_ne!(app.settings.profile.request, old_request);
        let _ = app.update(message(Event::Saved(
            write,
            Ok(Details {
                display_name: Some("Saved after close".into()),
                avatar_url: None,
            }),
        )));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.settings.profile.name, "Saved after close");
        assert!(app.settings.profile.saved.is_some());
        let _ = app.update(message(Event::Saved(write, Err("duplicate".into()))));
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn cancelled_profile_reads_reject_late_results_and_shutdown_finishes_after_writes() {
        let mut app = open();
        let old_request = app.settings.profile.request;
        let _ = app.update(Message::Settings(SettingsEvent::Cancel));
        let _ = app.update(Message::Settings(SettingsEvent::Open));
        let _ = app.update(Message::Settings(SettingsEvent::Category(
            Category::Account,
        )));
        let saved = app.settings.profile.saved.clone();
        let _ = app.update(message(Event::Loaded(
            old_request,
            None,
            Ok(Details {
                display_name: Some("Stale".into()),
                avatar_url: None,
            }),
        )));
        assert_eq!(app.settings.profile.saved, saved);
        let _ = app.update(message(Event::Name("Pending".into())));
        let _ = app.update(message(Event::SaveName));
        let write = operation(&app);
        app.closing = true;
        app.selection.shutdown = false;
        let _ = app.update(message(Event::Saved(write, Ok(Details::default()))));
        assert_eq!(app.pending_ops, 0);
        assert!(
            app.selection.shutdown,
            "completion releases shutdown without another event"
        );
    }
}
impl State {
    pub(super) fn dirty(&self) -> bool {
        self.saved
            .as_ref()
            .is_some_and(|saved| self.name != saved.display_name.as_deref().unwrap_or(""))
            || self.avatar.edit().is_some()
            || self.avatar.is_picking()
    }
    pub(super) fn dismiss(&mut self) {
        self.active = false;
        self.load = None;
        self.saved = None;
        self.name.clear();
        self.avatar.reset();
        self.error = None;
        self.success = false;
        self.reload_required = false;
    }
    pub(super) fn reset_account(&mut self) {
        self.dismiss();
        self.avatar = avatar_editor::State::default();
        self.demo = None;
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Reload,
    Name(String),
    SaveName,
    Avatar(u64, avatar_editor::Event),
    Loaded(u64, Option<String>, Result<Details, String>),
    Saved(u64, Result<Details, String>),
}
fn message(event: Event) -> Message {
    Message::Settings(settings::Event::Profile(event))
}
impl App {
    pub(super) fn account_profile_avatar(&self) -> Option<matrix_sdk::ruma::OwnedMxcUri> {
        self.settings.profile.saved.as_ref()?.avatar_url.clone()
    }
    pub(super) fn account_profile_event(&mut self, event: Event) -> Task<Message> {
        // Completion bookkeeping must run even when another dialog covers Settings.
        if let Event::Saved(operation, result) = event {
            let Some((request, account, change)) = self.settings.profile.writes.remove(&operation)
            else {
                return Task::none();
            };
            self.pending_ops = self.pending_ops.saturating_sub(1);
            if account != self.account_id || self.stopped || self.closing {
                return Task::none();
            }
            let state = &mut self.settings.profile;
            if let Ok(details) = &result {
                self.model.account_name = details
                    .display_name
                    .clone()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| self.account_id.clone().unwrap_or_else(|| "Me".into()));
                self.model.account_avatar_url = details.avatar_url.clone();
                if self.demo.is_some() {
                    state.demo = Some(details.clone());
                }
            }
            if state.active && state.request == request {
                match result {
                    Ok(details) => {
                        let mut saved = state.saved.clone().unwrap_or_default();
                        match change {
                            Change::DisplayName(_) => {
                                state.name = details.display_name.clone().unwrap_or_default();
                                saved.display_name = details.display_name.clone();
                                if state.avatar.edit().is_none() {
                                    saved.avatar_url = details.avatar_url.clone();
                                }
                            }
                            Change::Avatar(upload) => {
                                state.avatar.saved(details.avatar_url.clone(), upload);
                                if state.name == saved.display_name.as_deref().unwrap_or("") {
                                    state.name = details.display_name.clone().unwrap_or_default();
                                    saved.display_name = details.display_name.clone();
                                }
                                saved.avatar_url = details.avatar_url.clone();
                            }
                        }
                        state.saved = Some(saved);
                        state.error = None;
                        state.success = true;
                    }
                    Err(error) => {
                        state.error = Some(error);
                        state.reload_required = true;
                    }
                }
            } else if state.active && state.saved.is_none() {
                return self.account_profile_event(Event::Reload);
            }
            return Task::none();
        }
        if let Event::Loaded(request, account, result) = event {
            let state = &mut self.settings.profile;
            if !state.active
                || state.request != request
                || state.load.is_none()
                || account != self.account_id
                || self.closing
                || self.stopped
            {
                return Task::none();
            }
            state.load = None;
            match result {
                Ok(details) => {
                    state.name = details.display_name.clone().unwrap_or_default();
                    state.saved = Some(details);
                    state.error = None;
                }
                Err(error) => state.error = Some(error),
            }
            return Task::none();
        }
        if self.settings.draft.is_none() || self.closing || self.stopped {
            return Task::none();
        }
        if matches!(event, Event::Open | Event::Reload) {
            if matches!(event, Event::Reload) && !self.settings.profile.writes.is_empty() {
                return Task::none();
            }
            if matches!(event, Event::Open) && self.settings.profile.active {
                return Task::none();
            }
            let directory = self.model.people_directory.clone();
            let state = &mut self.settings.profile;
            state.dismiss();
            state.active = true;
            self.generation += 1;
            let request = self.generation;
            state.request = request;
            if !state.writes.is_empty() {
                return Task::none();
            }
            if self.demo.is_some() {
                let details = state
                    .demo
                    .get_or_insert_with(|| Details {
                        display_name: Some(self.model.account_name.clone()),
                        avatar_url: self.model.account_avatar_url.clone(),
                    })
                    .clone();
                state.name = details.display_name.clone().unwrap_or_default();
                state.saved = Some(details);
                return Task::none();
            }
            let Some(directory) = directory else {
                state.error = Some("Sign in to edit your account profile.".into());
                return Task::none();
            };
            let account = self.account_id.clone();
            let (task, handle) = Task::perform(directory.account_profile(), move |result| {
                message(Event::Loaded(request, account.clone(), result))
            })
            .abortable();
            state.load = Some(handle.abort_on_drop());
            return task;
        }
        let state = &mut self.settings.profile;
        if !state.writes.is_empty() {
            return Task::none();
        }
        let Some(saved) = state.saved.clone() else {
            return Task::none();
        };
        let change = match event {
            Event::Name(name) => {
                state.name = name;
                state.success = false;
                return Task::none();
            }
            Event::SaveName
                if !state.avatar.is_picking()
                    && state.name != saved.display_name.as_deref().unwrap_or("") =>
            {
                Change::DisplayName(state.name.clone())
            }
            Event::Avatar(request, event) if request == state.request => {
                if !matches!(event, avatar_editor::Event::Save) {
                    let details = room_avatar::Details {
                        url: saved.avatar_url.clone(),
                        editable: true,
                    };
                    return state
                        .avatar
                        .update(event, &details, true)
                        .map(move |event| message(Event::Avatar(request, event)));
                }
                if state.avatar.is_picking() {
                    return Task::none();
                }
                let Some(upload) = state.avatar.edit() else {
                    return Task::none();
                };
                Change::Avatar(upload)
            }
            _ => return Task::none(),
        };
        if state.reload_required || self.pending_ops >= MAX_PENDING_OPERATIONS {
            return Task::none();
        }
        if let Change::DisplayName(name) = &change
            && let Err(error) = interconnect::account_profile::validate_display_name(name)
        {
            state.error = Some(error);
            return Task::none();
        }
        let directory = self.model.people_directory.clone();
        if self.demo.is_none() && directory.is_none() {
            return Task::none();
        }
        self.generation += 1;
        let operation = self.generation;
        state.writes.insert(
            operation,
            (state.request, self.account_id.clone(), change.clone()),
        );
        state.error = None;
        state.success = false;
        self.pending_ops += 1;
        let demo = self.demo.is_some();
        Task::perform(
            async move {
                if demo {
                    let mut details = saved;
                    match change {
                        Change::DisplayName(name) => {
                            details.display_name = (!name.trim().is_empty()).then_some(name)
                        }
                        Change::Avatar(upload) => {
                            details.avatar_url = upload
                                .map(|_| format!("mxc://demo.invalid/account-{operation}").into())
                        }
                    }
                    Ok(details)
                } else {
                    directory
                        .expect("checked account directory")
                        .change_account_profile(saved, change)
                        .await
                }
            },
            move |result| message(Event::Saved(operation, result)),
        )
    }
    pub(super) fn account_profile_view(&self, enabled: bool) -> Element<'_, Message> {
        let state = &self.settings.profile;
        let available = enabled && state.writes.is_empty() && state.load.is_none();
        let mut content = column![
            shell::title("Account profile", 16),
            text(self.account_id.as_deref().unwrap_or(if self.demo.is_some() { "Demo account" } else { "Not signed in" })).size(13).style(shell::muted_text),
            text("Your display name and avatar are visible to other people on Matrix. Save each change separately.").size(13),
        ].spacing(16).width(Length::Fill);
        if let Some(saved) = &state.saved {
            let mut input = text_input("Display name", &state.name)
                .id("account-display-name")
                .cursor_blinking(false)
                .size(14)
                .padding([9, 12])
                .style(shell::search_style);
            if available {
                input = input.on_input(|name| message(Event::Name(name)));
            }
            let valid_name = interconnect::account_profile::validate_display_name(&state.name);
            content = content.push(
                column![
                    text("Display name").size(13),
                    input,
                    text("Leave blank to use your Matrix ID.")
                        .size(12)
                        .style(shell::muted_text),
                    button(text("Save display name").size(13))
                        .keyboard_navigation(true)
                        .padding([7, 12])
                        .style(shell::primary_button)
                        .on_press_maybe(
                            (available
                                && !state.reload_required
                                && !state.avatar.is_picking()
                                && valid_name.is_ok()
                                && state.name != saved.display_name.as_deref().unwrap_or(""))
                            .then_some(message(Event::SaveName))
                        ),
                ]
                .spacing(10),
            );
            if let Err(error) = valid_name {
                content = content.push(text(error).size(12).style(shell::danger_text));
            }
            let details = room_avatar::Details {
                url: saved.avatar_url.clone(),
                editable: true,
            };
            let request = state.request;
            content = content.push(
                state
                    .avatar
                    .view(
                        &details,
                        &self.model.avatars,
                        available && !state.reload_required,
                    )
                    .map(move |event| message(Event::Avatar(request, event))),
            );
        } else if state.load.is_some() {
            content = content.push(
                text("Loading your profile…")
                    .size(13)
                    .style(shell::muted_text),
            );
        }
        if !state.writes.is_empty() {
            content = content.push(
                text("Saving profile change…")
                    .size(13)
                    .style(shell::muted_text),
            );
        }
        if state.success {
            content = content.push(text("Profile change saved.").size(13));
        }
        if let Some(error) = &state.error {
            content = content.push(text(error).size(13).style(shell::danger_text));
        }
        content = content.push(
            row![
                button(text("Reload profile").size(13))
                    .keyboard_navigation(true)
                    .padding([7, 12])
                    .style(shell::quiet_button)
                    .on_press_maybe(available.then_some(message(Event::Reload)))
            ]
            .spacing(10),
        );
        if self.demo.is_some() {
            content = content.push(
                text("Demo profile changes stay in memory for this session.")
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        content.into()
    }
}
