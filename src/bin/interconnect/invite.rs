//! Explicit invitations capture their room; late results cannot replace a new form.
use super::people_picker;
use super::{App, MAX_PENDING_OPERATIONS, Message, current_pane, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, text},
};
use interconnect::people::parse_user_id;
use std::collections::HashSet;

pub struct State {
    target: Target,
    picker: people_picker::State,
    pending: bool,
    error: Option<String>,
    sent: HashSet<String>,
    success: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    room: (u64, String),
    panel: u64,
    request: u64,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Picker(u64, people_picker::Event),
    Submit,
    Cancel,
    Completed(Target, String, Result<(), String>),
}
fn message(event: Event) -> Message {
    Message::Invite(event)
}
impl App {
    pub(super) fn can_invite(&self) -> bool {
        !self.closing
            && !self.stopped
            && self.selection.members.is_some()
            && self
                .selection
                .room
                .as_ref()
                .is_some_and(|(_, id)| self.model.rooms.iter().any(|r| &r.id == id && r.can_invite))
    }
    pub(super) fn validate_invite(&mut self) {
        if self.invite.as_ref().is_some_and(|s| {
            !self.can_invite()
                || self.selection.room.as_ref() != Some(&s.target.room)
                || self.selection.members != Some(s.target.panel)
        }) {
            self.invite = None;
        }
    }
    pub(super) fn invite_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open if self.can_invite() && self.invite.is_none() => {
                self.generation += 1;
                self.invite = Some(State {
                    target: Target {
                        room: self.selection.room.clone().unwrap(),
                        panel: self.selection.members.unwrap(),
                        request: self.generation,
                    },
                    picker: people_picker::State::default(),
                    pending: false,
                    error: None,
                    sent: HashSet::new(),
                    success: None,
                });
                return iced::widget::operation::focus(people_picker::SEARCH_ID);
            }
            Event::Picker(request, event) => {
                let directory = self
                    .selection
                    .room
                    .as_ref()
                    .and_then(|(g, _)| current_pane(&self.model.room, *g))
                    .and_then(|p| p.composer.as_ref())
                    .map(|c| c.people_directory());
                if let Some(state) = &mut self.invite
                    && state.target.request == request
                    && !state.pending
                {
                    if matches!(event, people_picker::Event::Submit) {
                        return self.invite_event(Event::Submit);
                    }
                    if matches!(
                        event,
                        people_picker::Event::Edit(_)
                            | people_picker::Event::Select(_)
                            | people_picker::Event::Retry
                    ) {
                        state.error = None;
                        state.success = None;
                    }
                    return state
                        .picker
                        .update(event, directory, self.demo.is_some())
                        .map(move |event| message(Event::Picker(request, event)));
                }
            }
            Event::Cancel => {
                self.invite = None;
                return iced::widget::operation::focus("member-search");
            }
            Event::Submit if self.can_invite() && !self.composing => {
                let Some(state) = &mut self.invite else {
                    return Task::none();
                };
                if state.pending
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self.inviting.contains(&state.target.room.1)
                {
                    return Task::none();
                }
                let user = match parse_user_id(&state.picker.query) {
                    Ok(user) => user.to_string(),
                    Err(error) => {
                        state.error = Some(error);
                        return Task::none();
                    }
                };
                if state.sent.contains(&user) {
                    state.error = Some("This person already has an invitation.".into());
                    return Task::none();
                }
                if let Some(member) = self
                    .model
                    .members
                    .as_ref()
                    .filter(|m| m.generation == state.target.panel)
                    .and_then(|m| m.rows.iter().find(|m| m.user_id == user))
                {
                    state.error = Some(
                        if member.invited {
                            "This person already has an invitation."
                        } else {
                            "This person is already a room member."
                        }
                        .into(),
                    );
                    return Task::none();
                }
                if let Some(demo) = &mut self.demo {
                    let person = state.picker.selected.as_ref().filter(|p| p.user_id == user);
                    demo.invitations
                        .entry(state.target.room.1.clone())
                        .or_default()
                        .push(interconnect::connected::Member {
                            user_id: user.clone(),
                            name: person.map_or_else(|| user.clone(), |p| p.name.clone()),
                            avatar_url: person.and_then(|p| p.avatar_url.clone()),
                            role: String::new(),
                            invited: true,
                        });
                    state.picker.cancel_search();
                    state.sent.insert(user.clone());
                    state.success = Some(format!("Invitation sent to {user} (demo)."));
                    state.error = None;
                    state.picker.clear();
                    let room = state.target.room.clone();
                    self.refresh_invited_room(&room);
                } else if let Some(composer) = current_pane(&self.model.room, state.target.room.0)
                    .and_then(|p| p.composer.clone())
                {
                    state.picker.cancel_search();
                    state.pending = true;
                    state.error = None;
                    state.success = None;
                    self.pending_ops += 1;
                    self.inviting.insert(state.target.room.1.clone());
                    let target = state.target.clone();
                    return Task::perform(composer.invite_member(user.clone()), move |result| {
                        message(Event::Completed(target.clone(), user.clone(), result))
                    });
                }
            }
            Event::Completed(target, user, result) => {
                self.pending_ops -= 1;
                self.inviting.remove(&target.room.1);
                let sent = result.is_ok();
                if let Some(state) = &mut self.invite
                    && state.target == target
                {
                    state.pending = false;
                    match result {
                        Ok(()) => {
                            state.sent.insert(user.clone());
                            state.picker.clear();
                            state.success = Some(format!("Invitation sent to {user}."));
                        }
                        Err(error) => state.error = Some(error),
                    }
                }
                if sent {
                    self.refresh_invited_room(&target.room);
                }
            }
            _ => {}
        }
        Task::none()
    }
    // A successful invite invalidates the SDK member cache. Restart the cancellable
    // roster load with a fresh generation while keeping the current form intact.
    fn refresh_invited_room(&mut self, room: &(u64, String)) {
        if self.selection.room.as_ref() == Some(room) && self.selection.members.is_some() {
            self.generation += 1;
            self.selection.members = Some(self.generation);
            if let Some(state) = &mut self.invite {
                state.target.panel = self.generation;
            }
            self.refresh_demo();
        }
    }
    pub(super) fn invite_avatars(&self) -> Vec<matrix_sdk::ruma::OwnedMxcUri> {
        self.invite
            .as_ref()
            .into_iter()
            .flat_map(|s| s.picker.results.iter().chain(s.picker.selected.iter()))
            .filter_map(|p| p.avatar_url.clone())
            .collect()
    }
    pub(super) fn invite_view(&self) -> Element<'_, Message> {
        let Some(state) = &self.invite else {
            return container(
                button(text("+  Invite people").size(13))
                    .width(Length::Fill)
                    .padding([9, 12])
                    .style(shell::quiet_button)
                    .on_press(message(Event::Open)),
            )
            .padding(16)
            .width(Length::Fill)
            .into();
        };
        let busy = state.pending || self.inviting.contains(&state.target.room.1);
        let request = state.target.request;
        let input = state
            .picker
            .view(&self.model.avatars, !busy)
            .map(move |event| message(Event::Picker(request, event)));
        let mut send = button(
            text(if busy {
                "Inviting…"
            } else {
                "Send invitation"
            })
            .size(13),
        )
        .padding([7, 10])
        .style(shell::primary_button);
        if !busy
            && self.pending_ops < MAX_PENDING_OPERATIONS
            && (self.demo.is_some()
                || current_pane(&self.model.room, state.target.room.0)
                    .is_some_and(|p| p.composer.is_some()))
        {
            send = send.on_press(message(Event::Submit));
        }
        let mut form = column![
            shell::title("Invite to room", 13),
            text("Search your server or enter a full Matrix ID.")
                .size(12)
                .style(shell::muted_text),
            input,
            row![
                send,
                super::tooltips::icon_button(
                    super::icons::Kind::Close,
                    "Close invitations",
                    Some(message(Event::Cancel))
                )
            ]
            .spacing(8)
        ]
        .spacing(10);
        if let Some(error) = &state.error {
            form = form.push(text(error).size(12));
        }
        if let Some(success) = &state.success {
            form = form.push(text(success).size(12).style(shell::muted_text));
        }
        container(form).padding(16).width(Length::Fill).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{members, tests::demo_app};
    use std::sync::Arc;
    fn open(app: &mut App) {
        let _ = app.update(Message::Members(members::Event::Open));
        let _ = app.update(message(Event::Open));
    }
    fn edit(app: &mut App, input: &str) {
        let request = app.invite.as_ref().unwrap().target.request;
        let _ = app.update(message(Event::Picker(
            request,
            people_picker::Event::Edit(input.into()),
        )));
    }
    #[test]
    fn demo_invites_accept_raw_ids_and_directory_selection_without_joining_or_touching_drafts() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent draft");
        let count = app
            .model
            .rooms
            .iter()
            .find(|r| r.id == key.room)
            .unwrap()
            .member_count;
        open(&mut app);
        edit(&mut app, "Alice");
        let _ = app.update(message(Event::Submit));
        assert!(app.invite.as_ref().unwrap().error.is_some());
        edit(&mut app, "@jamie:example.org");
        let _ = app.update(message(Event::Submit));
        assert!(
            app.invite
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("already")
        );
        edit(&mut app, " @new:remote.example ");
        let _ = app.update(message(Event::Submit));
        assert!(
            app.invite
                .as_ref()
                .unwrap()
                .success
                .as_ref()
                .unwrap()
                .contains("@new:remote.example")
        );
        edit(&mut app, "@new:remote.example");
        let _ = app.update(message(Event::Submit));
        assert!(app.invite.as_ref().unwrap().error.is_some());
        edit(&mut app, "Taylor");
        let state = app.invite.as_mut().unwrap();
        state.picker.results = vec![interconnect::people::Person {
            name: "Taylor".into(),
            user_id: "@taylor:example.org".into(),
            avatar_url: None,
        }];
        let request = state.target.request;
        let _ = app.update(message(Event::Picker(
            request,
            people_picker::Event::Select("@taylor:example.org".into()),
        )));
        assert!(app.invite.as_ref().unwrap().success.is_none());
        let _ = app.update(message(Event::Submit));
        assert!(
            app.invite
                .as_ref()
                .unwrap()
                .sent
                .contains("@taylor:example.org")
        );
        assert_eq!(app.drafts[&key].content.text(), "unsent draft");
        assert_eq!(
            app.model
                .rooms
                .iter()
                .find(|r| r.id == key.room)
                .unwrap()
                .member_count,
            count
        );
        assert_eq!(app.pending_ops, 0);
        let roster = &app.model.members.as_ref().unwrap().rows;
        assert!(
            roster
                .iter()
                .any(|m| m.user_id == "@new:remote.example" && m.invited)
        );
        assert!(
            roster
                .iter()
                .any(|m| m.user_id == "@taylor:example.org" && m.name == "Taylor" && m.invited)
        );
        assert_eq!(roster.iter().filter(|m| !m.invited).count(), 5);
        let _ = app.update(Message::Escape);
        assert!(app.invite.is_none());
        assert!(app.selection.members.is_some());
        let _ = app.update(Message::Members(members::Event::Close));
        let _ = app.update(Message::Members(members::Event::Open));
        assert!(
            app.model
                .members
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|m| m.user_id == "@taylor:example.org" && m.invited)
        );
        let _ = app.update(message(Event::Open));
        edit(&mut app, "@taylor:example.org");
        let _ = app.update(message(Event::Submit));
        assert!(
            app.invite
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("already has an invitation")
        );
    }
    #[test]
    fn successful_send_refreshes_only_its_open_room_and_rejects_old_rosters() {
        let mut app = demo_app();
        open(&mut app);
        let target = app.invite.as_ref().unwrap().target.clone();
        app.demo = None;
        app.pending_ops = 1;
        app.inviting.insert(target.room.1.clone());
        app.invite.as_mut().unwrap().pending = true;
        let _ = app.update(message(Event::Completed(
            target.clone(),
            "@new:example.org".into(),
            Ok(()),
        )));
        assert_ne!(app.selection.members, Some(target.panel));
        assert!(
            app.visible_members().is_empty(),
            "old-generation roster is hidden"
        );
        let state = app.invite.as_ref().unwrap();
        assert!(state.success.is_some());
        assert_eq!(Some(state.target.panel), app.selection.members);
        let pending = state.target.clone();
        app.pending_ops = 1;
        app.inviting.insert(pending.room.1.clone());
        let _ = app.update(Message::Room("general".into()));
        open(&mut app);
        let generation = app.selection.members;
        let _ = app.update(message(Event::Completed(
            pending,
            "@late:example.org".into(),
            Ok(()),
        )));
        assert_eq!(app.selection.members, generation);
        assert!(app.invite.as_ref().unwrap().success.is_none());
        assert_eq!(app.pending_ops, 0);
    }

    #[test]
    fn navigation_permission_changes_and_new_forms_reject_stale_results() {
        let mut app = demo_app();
        open(&mut app);
        let old = app.invite.as_ref().unwrap().target.clone();
        app.invite.as_mut().unwrap().pending = true;
        app.pending_ops = 1;
        app.inviting.insert(old.room.1.clone());
        let _ = app.update(Message::Room("general".into()));
        assert!(app.invite.is_none());
        open(&mut app);
        edit(&mut app, "@current:example.org");
        let _ = app.update(message(Event::Completed(
            old.clone(),
            "@old:example.org".into(),
            Err("Late failure".into()),
        )));
        assert!(app.invite.as_ref().unwrap().error.is_none());
        assert_eq!(
            app.invite.as_ref().unwrap().picker.query,
            "@current:example.org"
        );
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Picker(
            old.request,
            people_picker::Event::Edit("stale".into()),
        )));
        assert_eq!(
            app.invite.as_ref().unwrap().picker.query,
            "@current:example.org"
        );
        let mut model = app.model.clone();
        Arc::make_mut(&mut model.rooms)
            .iter_mut()
            .find(|r| r.id == "general")
            .unwrap()
            .can_invite = false;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.invite.is_none());
        assert!(!app.can_invite());
        let _ = app.update(message(Event::Open));
        assert!(app.invite.is_none());
    }
}
