//! Account-wide ignored users. Reads cancel with Settings; admitted writes finish.
use super::{App, MAX_PENDING_OPERATIONS, Message, settings, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, row, text, text_input},
};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    account: Option<String>,
    users: Option<Vec<String>>,
    input: String,
    confirm: Option<(String, bool)>,
    request: u64,
    load: Option<iced::task::Handle>,
    writes: HashMap<u64, Option<String>>,
    error: Option<String>,
    reload_required: bool,
}
impl State {
    pub(super) fn dismiss(&mut self) {
        self.load = None;
        self.request = 0;
        self.confirm = None;
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Select(String, Option<String>, u64),
    Input(String),
    Loaded(u64, Option<String>, Result<Vec<String>, String>),
    Propose(String, bool),
    Confirm,
    Cancel,
    Saved(u64, Result<Vec<String>, String>),
}
fn message(event: Event) -> Message {
    Message::IgnoredUsers(event)
}
impl App {
    pub(super) fn ignored_users_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Loaded(request, account, result) => {
                if self.ignored_users.request == request && self.account_id == account {
                    self.ignored_users.load = None;
                    match result {
                        Ok(users) => {
                            self.ignored_users.users = Some(users);
                            self.ignored_users.error = None;
                            self.ignored_users.reload_required = false;
                        }
                        Err(error) => self.ignored_users.error = Some(error),
                    }
                }
                return Task::none();
            }
            Event::Saved(operation, result) => {
                let Some(account) = self.ignored_users.writes.remove(&operation) else {
                    return Task::none();
                };
                self.pending_ops = self.pending_ops.saturating_sub(1);
                if self.account_id == account {
                    match result {
                        Ok(users) => {
                            self.ignored_users.users = Some(users);
                            self.ignored_users.error = None;
                            self.ignored_users.reload_required = false;
                        }
                        Err(error) => {
                            self.ignored_users.error = Some(error);
                            self.ignored_users.reload_required = true;
                        }
                    }
                }
                return if self.closing && self.pending_ops == 0 {
                    self.update(Message::Close)
                } else {
                    Task::none()
                };
            }
            Event::Select(user, account, panel)
                if self.context_menu_allowed()
                    && self.account_id == account
                    && self.selection.members == Some(panel) =>
            {
                let open = self.update(Message::Settings(settings::Event::Open));
                let category = self.update(Message::Settings(settings::Event::Category(
                    settings::Category::Account,
                )));
                self.ignored_users.input = user;
                return Task::batch([open, category]);
            }
            _ => {}
        }
        if !self.ignored_users_available()
            || (!matches!(event, Event::Open) && self.ignored_users.account != self.account_id)
        {
            return Task::none();
        }
        match event {
            Event::Open if self.ignored_users.writes.is_empty() => {
                self.ignored_users.dismiss();
                if self.ignored_users.account != self.account_id {
                    self.ignored_users.users = None;
                    self.ignored_users.input.clear();
                    self.ignored_users.account = self.account_id.clone();
                }
                self.ignored_users.error = None;
                self.ignored_users.reload_required = true;
                if self.demo.is_some() {
                    self.ignored_users.users.get_or_insert_default();
                    self.ignored_users.reload_required = false;
                    return Task::none();
                }
                let Some(directory) = self.model.people_directory.clone() else {
                    return Task::none();
                };
                self.generation += 1;
                let request = self.generation;
                self.ignored_users.request = request;
                let account = self.account_id.clone();
                let (task, handle) = Task::perform(directory.ignored_users(), move |result| {
                    message(Event::Loaded(request, account.clone(), result))
                })
                .abortable();
                self.ignored_users.load = Some(handle.abort_on_drop());
                return task;
            }
            Event::Input(input) => {
                self.ignored_users.input = input;
                self.ignored_users.confirm = None;
            }
            Event::Propose(user, ignored)
                if self.ignored_users.users.is_some()
                    && self.ignored_users.writes.is_empty()
                    && self.ignored_users.load.is_none()
                    && !self.ignored_users.reload_required =>
            {
                match interconnect::people::parse_user_id(&user) {
                    Ok(user) if self.account_id.as_deref() != Some(user.as_str()) => {
                        self.ignored_users.confirm = Some((user.to_string(), ignored))
                    }
                    Ok(_) => self.ignored_users.error = Some("You cannot ignore yourself.".into()),
                    Err(error) => self.ignored_users.error = Some(error),
                }
            }
            Event::Cancel => self.ignored_users.confirm = None,
            Event::Confirm
                if self.ignored_users.writes.is_empty()
                    && self.pending_ops < MAX_PENDING_OPERATIONS
                    && !self.ignored_users.reload_required =>
            {
                let Some((user, ignored)) = self.ignored_users.confirm.take() else {
                    return Task::none();
                };
                let Some(directory) = self
                    .model
                    .people_directory
                    .clone()
                    .filter(|_| self.demo.is_none())
                else {
                    if self.demo.is_some() {
                        let users = self.ignored_users.users.get_or_insert_default();
                        users.retain(|id| id != &user);
                        if ignored {
                            users.push(user);
                            users.sort();
                        }
                    }
                    return Task::none();
                };
                self.generation += 1;
                let operation = self.generation;
                self.ignored_users
                    .writes
                    .insert(operation, self.account_id.clone());
                self.ignored_users.error = None;
                self.pending_ops += 1;
                return Task::perform(
                    directory.change_ignored_user(user, ignored),
                    move |result| message(Event::Saved(operation, result)),
                );
            }
            _ => {}
        }
        Task::none()
    }
    pub(super) fn ignored_users_view(&self, enabled: bool) -> Element<'_, Message> {
        let state = &self.ignored_users;
        if state.account != self.account_id {
            return column![
                shell::title("Ignored users", 14),
                button(text("Load ignored users").size(12))
                    .keyboard_navigation(true)
                    .on_press_maybe(enabled.then_some(message(Event::Open)))
                    .style(shell::quiet_button),
            ]
            .spacing(10)
            .into();
        }
        let ready = enabled && state.load.is_none() && state.writes.is_empty();
        let change = ready && state.users.is_some() && !state.reload_required;
        let input_ignored = state
            .users
            .as_ref()
            .is_some_and(|users| users.iter().any(|user| user == state.input.trim()));
        let mut content = column![
            shell::title("Ignored users", 14),
            text("Hide messages and invitations from these people across your account. Ignoring does not stop them seeing your messages or ban them from rooms. Changes take effect as account data syncs.").size(12).style(shell::muted_text),
            row![text_input("@name:example.org", &state.input).on_input_maybe(change.then_some(|input| message(Event::Input(input)))).size(13).padding([7,10]).cursor_blinking(false),
                button(text(if input_ignored { "Unignore…" } else { "Ignore…" }).size(12)).keyboard_navigation(true).on_press_maybe(change.then(|| message(Event::Propose(state.input.clone(), !input_ignored)))).style(shell::quiet_button)],
            button(text(if state.load.is_some() { "Loading…" } else { "Reload ignored users" }).size(12)).keyboard_navigation(true).on_press_maybe(ready.then_some(message(Event::Open))).style(shell::quiet_button),
        ].spacing(10).width(Length::Fill);
        if let Some(error) = &state.error {
            content = content.push(text(error).size(12).style(shell::danger_text));
        }
        if state.account == self.account_id
            && let Some(users) = &state.users
        {
            if users.is_empty() {
                content = content.push(text("No ignored users.").size(12).style(shell::muted_text));
            }
            for user in users.iter().take(100) {
                content = content.push(
                    row![
                        text(user).size(12).width(Length::Fill),
                        button(text("Unignore…").size(12))
                            .keyboard_navigation(true)
                            .on_press_maybe(
                                change.then(|| message(Event::Propose(user.clone(), false)))
                            )
                            .style(shell::quiet_button)
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                );
            }
            if users.len() > 100 {
                content = content.push(text("Showing the first 100. Enter a full Matrix ID above to manage another person.").size(12));
            }
        }
        if let Some((user, ignored)) = &state.confirm {
            content = content.push(
                column![
                    text(format!(
                        "{} {user}?",
                        if *ignored { "Ignore" } else { "Unignore" }
                    ))
                    .size(13),
                    row![
                        button(text("Cancel").size(12))
                            .keyboard_navigation(true)
                            .on_press(message(Event::Cancel))
                            .style(shell::quiet_button),
                        button(
                            text(if *ignored {
                                "Ignore user"
                            } else {
                                "Unignore user"
                            })
                            .size(12)
                        )
                        .keyboard_navigation(true)
                        .on_press_maybe(change.then_some(message(Event::Confirm)))
                        .style(shell::primary_button)
                    ]
                    .spacing(8)
                ]
                .spacing(8),
            );
        }
        content.into()
    }
    pub(super) fn ignore_member_action(
        &self,
        member: &interconnect::connected::Member,
    ) -> Option<Message> {
        (self.context_menu_allowed() && self.account_id.as_deref() != Some(member.user_id.as_str()))
            .then(|| {
                message(Event::Select(
                    member.user_id.clone(),
                    self.account_id.clone(),
                    self.selection.members.unwrap_or_default(),
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open() -> App {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Settings(settings::Event::Open));
        let _ = app.update(Message::Settings(settings::Event::Category(
            settings::Category::Account,
        )));
        app
    }
    #[test]
    fn demo_ignore_and_unignore_require_confirmation_and_preserve_conversation() {
        let mut app = open();
        let selection = app.selection.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("keep draft");
        let _ = app.update(message(Event::Propose("@maya:example.org".into(), true)));
        assert!(app.ignored_users.users.as_ref().unwrap().is_empty());
        let _ = app.update(message(Event::Cancel));
        let _ = app.update(message(Event::Confirm));
        assert!(app.ignored_users.users.as_ref().unwrap().is_empty());
        let _ = app.update(message(Event::Propose("@maya:example.org".into(), true)));
        let _ = app.update(message(Event::Confirm));
        assert_eq!(
            app.ignored_users.users.as_ref().unwrap(),
            &["@maya:example.org"]
        );
        let _ = app.update(message(Event::Propose("@maya:example.org".into(), false)));
        let _ = app.update(message(Event::Confirm));
        assert!(app.ignored_users.users.as_ref().unwrap().is_empty());
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert_eq!(app.drafts[&key].content.text(), "keep draft");
        assert_eq!(app.pending_ops, 0);
        assert!(app.model.people_directory.is_none());
    }
    #[test]
    fn stale_loads_and_account_writes_do_not_publish_into_another_account() {
        let mut app = open();
        app.ignored_users.request = 45;
        let original = app.ignored_users.users.clone();
        let _ = app.update(message(Event::Loaded(
            44,
            app.account_id.clone(),
            Ok(vec!["@stale:example.org".into()]),
        )));
        assert_eq!(app.ignored_users.users, original);
        app.ignored_users
            .writes
            .insert(46, Some("@old:example.org".into()));
        app.pending_ops = 1;
        let _ = app.update(message(Event::Saved(
            46,
            Ok(vec!["@other-account:example.org".into()]),
        )));
        assert_eq!(app.ignored_users.users, original);
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn people_shortcut_prefills_account_settings_and_rejects_a_closed_roster() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Members(super::super::members::Event::Open));
        let member = app
            .visible_members()
            .into_iter()
            .find(|member| member.user_id == "@maya:example.org")
            .unwrap()
            .clone();
        let stale = app.ignore_member_action(&member).unwrap();
        let _ = app.update(Message::Members(super::super::members::Event::Close));
        let _ = app.update(stale);
        assert!(app.settings.draft.is_none());
        let _ = app.update(Message::Members(super::super::members::Event::Open));
        let action = app.ignore_member_action(&member).unwrap();
        let _ = app.update(action);
        assert!(app.settings.draft.is_some());
        assert_eq!(app.ignored_users.input, "@maya:example.org");
        assert!(app.ignored_users.users.is_some());
        assert!(app.ignored_users.confirm.is_none());
        let _ = app.update(message(Event::Propose("@maya:example.org".into(), true)));
        assert!(app.ignored_users.confirm.is_some());
    }
    #[test]
    fn failures_require_reload_and_closed_settings_cannot_admit_changes() {
        let mut app = open();
        app.ignored_users.writes.insert(10, app.account_id.clone());
        app.pending_ops = 1;
        let _ = app.update(message(Event::Saved(10, Err("ambiguous".into()))));
        let _ = app.update(message(Event::Propose("@maya:example.org".into(), true)));
        assert!(app.ignored_users.confirm.is_none());
        let _ = app.update(message(Event::Open));
        assert!(!app.ignored_users.reload_required);
        let _ = app.update(Message::Settings(settings::Event::Cancel));
        let _ = app.update(message(Event::Propose("@maya:example.org".into(), true)));
        assert!(app.ignored_users.confirm.is_none());
    }
}
