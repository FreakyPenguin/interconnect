//! Shared member editor hosted by space settings and room member management.
use super::{App, Event as PanelEvent, MAX_PENDING_OPERATIONS, Message, message};
use crate::{
    icons::{self, Kind},
    people_picker, shell,
    tooltips::icon_button,
};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, pick_list, row, rule, text, text_input},
};
use interconnect::{
    connected::Member,
    room_management::{
        BanDetails, BannedMember, ManagedMember, Management, MemberChange, Role, Roster,
    },
};
use matrix_sdk::ruma::events::room::power_levels::UserPowerLevel;

const PAGE: usize = 50;
#[derive(Default)]
pub(super) struct State {
    roster: Option<Roster>,
    query: String,
    banned: bool,
    ban_user: String,
    page: usize,
    request: u64,
    load: Option<iced::task::Handle>,
    picker: people_picker::State,
    confirm: Option<MemberChange>,
    reason: String,
    reason_request: u64,
    feedback: Option<String>,
}
impl State {
    fn begin_confirmation(&mut self, change: MemberChange) {
        self.reason.clear();
        self.reason_request += 1;
        self.confirm = Some(change);
    }

    pub(super) fn can_remove_others(&self, own: &str) -> bool {
        self.roster.as_ref().is_some_and(|r| {
            r.rows
                .iter()
                .all(|m| m.member.user_id == own || m.can_remove)
        })
    }
    pub(super) fn other_counts(&self, own: &str) -> (usize, usize) {
        self.roster.as_ref().map_or((0, 0), |r| {
            r.rows.iter().filter(|m| m.member.user_id != own).fold(
                (0, 0),
                |(joined, invited), m| {
                    if m.member.invited {
                        (joined, invited + 1)
                    } else {
                        (joined + 1, invited)
                    }
                },
            )
        })
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    Ready(u64, Box<Result<(Management, Roster), String>>),
    Reload,
    Search(String),
    Page(usize),
    Picker(u64, people_picker::Event),
    Invite,
    Remove(String),
    Ban(String),
    BanInput(u64, String),
    PrepareBanId(u64),
    Unban(String),
    Banned(bool),
    Role(String, Role),
    Reason(u64, u64, String),
    Confirm,
    Cancel,
    Ignore,
    Loaded(u64, u64, Box<Result<Roster, String>>),
    Saved(u64, u64, MemberChange, Result<(), String>),
}
#[derive(Clone, Copy)]
pub(crate) enum Host {
    Panel,
    Sidebar,
}
impl Host {
    pub(crate) fn message(self, event: Event) -> Message {
        match self {
            Self::Panel => message(PanelEvent::Members(event)),
            Self::Sidebar => message(PanelEvent::SidebarMembers(event)),
        }
    }
}
fn event(event: Event) -> Message {
    Host::Panel.message(event)
}
fn power(role: Role) -> matrix_sdk::ruma::Int {
    match role {
        Role::Member => matrix_sdk::ruma::int!(0),
        Role::Moderator => matrix_sdk::ruma::int!(50),
        Role::Admin => matrix_sdk::ruma::int!(100),
    }
}
pub(crate) fn apply_snapshot(
    snapshot: &mut interconnect::connected::Members,
    change: &MemberChange,
) {
    let rows = std::sync::Arc::make_mut(&mut snapshot.rows);
    match change {
        MemberChange::Unban(_) => {}
        MemberChange::Remove { user, .. }
        | MemberChange::Ban { user, .. }
        | MemberChange::BanId(user) => rows.retain(|member| member.user_id != *user),
        MemberChange::Role { user, role, .. } => {
            if let Some(member) = rows.iter_mut().find(|member| member.user_id == *user) {
                member.role = role.to_string();
            }
        }
        MemberChange::Invite(user) => {
            if !rows.iter().any(|member| member.user_id == *user) {
                rows.push(Member {
                    name: user.clone(),
                    user_id: user.clone(),
                    avatar_url: None,
                    invited: true,
                    role: String::new(),
                });
                rows.sort_by_cached_key(|member| {
                    (
                        member.invited,
                        member.name.to_lowercase(),
                        member.user_id.clone(),
                    )
                });
            }
        }
    }
}
fn apply(roster: &mut Roster, change: &MemberChange, details: BanDetails) {
    match change {
        MemberChange::Invite(user) => {
            if !roster.rows.iter().any(|m| m.member.user_id == *user) {
                roster.rows.push(ManagedMember {
                    member: Member {
                        user_id: user.clone(),
                        name: user.clone(),
                        avatar_url: None,
                        role: String::new(),
                        invited: true,
                    },
                    power: UserPowerLevel::Int(matrix_sdk::ruma::int!(0)),
                    can_remove: false,
                    can_ban: false,
                    roles: vec![],
                });
            }
        }
        MemberChange::Unban(user) => roster.banned.retain(|m| m.user_id != *user),
        MemberChange::Ban { user, .. } | MemberChange::BanId(user) => {
            if let Some(index) = roster.rows.iter().position(|m| m.member.user_id == *user) {
                let member = roster.rows.remove(index);
                roster.banned.push(BannedMember {
                    user_id: user.clone(),
                    name: member.member.name,
                    can_unban: member.can_remove && member.can_ban,
                    details: Some(details.clone()),
                });
            } else if !roster.banned.iter().any(|m| m.user_id == *user) {
                roster.banned.push(BannedMember {
                    user_id: user.clone(),
                    name: user.clone(),
                    can_unban: roster.can_unban,
                    details: Some(details),
                });
            }
            roster
                .banned
                .sort_by_cached_key(|m| (m.name.to_lowercase(), m.user_id.clone()));
        }
        MemberChange::Remove { user, .. } => roster.rows.retain(|m| m.member.user_id != *user),
        MemberChange::Role { user, role, .. } => {
            if let Some(member) = roster.rows.iter_mut().find(|m| m.member.user_id == *user) {
                member.power = UserPowerLevel::Int(power(*role));
                member.member.role = role.to_string();
                // Permissions are a snapshot; require Reload before another administrative action.
                member.can_remove = false;
                member.can_ban = false;
                member.roles.clear();
            }
        }
    }
    roster.rows.sort_by_cached_key(|m| {
        (
            m.member.invited,
            m.member.name.to_lowercase(),
            m.member.user_id.clone(),
        )
    });
}
impl App {
    pub(crate) fn member_management_event(&mut self, host: Host, incoming: Event) -> Task<Message> {
        if let Event::Saved(panel_request, operation, change, result) = incoming {
            let Some((account, room)) = self.space_settings.writes.remove(&operation) else {
                return Task::none();
            };
            self.inviting.remove(&room);
            self.pending_ops = self.pending_ops.saturating_sub(1);
            if result.is_ok()
                && account == self.account_id
                && self
                    .selection
                    .room
                    .as_ref()
                    .is_some_and(|(_, id)| *id == room)
                && let Some(snapshot) = &mut self.model.members
                && Some(snapshot.generation) == self.selection.members
            {
                apply_snapshot(snapshot, &change);
            }

            if let Some(panel) = (match host {
                Host::Panel => &mut self.space_settings.panel,
                Host::Sidebar => &mut self.space_settings.sidebar,
            }) && panel.request == panel_request
                && panel.account == account
            {
                panel.pending = false;
                match result {
                    Ok(()) => {
                        if let Some(roster) = &mut panel.members.roster {
                            apply(
                                roster,
                                &change,
                                BanDetails {
                                    actor: account.clone(),
                                    timestamp_ms: None,
                                    reason: interconnect::moderation::reason(&panel.members.reason)
                                        .ok()
                                        .flatten(),
                                },
                            );
                        }
                        panel.members.confirm = None;
                        panel.members.reason.clear();
                        panel.members.picker.clear();
                        panel.members.feedback = Some(change_feedback(&change));
                        panel.success = true;
                    }
                    Err(error) => panel.error = Some(error),
                }
            }
            return self.reload_waiting_space_settings(&account, &room);
        }
        let Some(panel) = (match host {
            Host::Panel => &mut self.space_settings.panel,
            Host::Sidebar => &mut self.space_settings.sidebar,
        }) else {
            return Task::none();
        };
        let session = panel.request;
        match incoming {
            Event::Ready(request, result) => {
                if request != session {
                    return Task::none();
                }
                panel.pending = false;
                panel.load = None;
                match *result {
                    Ok((control, roster)) => {
                        panel.control = Some(control);
                        panel.members.roster = Some(roster);
                    }
                    Err(error) => panel.error = Some(error),
                }
            }

            Event::Loaded(request, operation, result) => {
                if request != session || operation != panel.members.request {
                    return Task::none();
                }
                panel.pending = false;
                panel.members.load = None;
                match *result {
                    Ok(roster) => {
                        panel.members.roster = Some(roster);
                    }
                    Err(error) => panel.error = Some(error),
                }
            }
            Event::Reload => {
                if panel.pending
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|t| t == &(panel.account.clone(), panel.id.clone()))
                {
                    return Task::none();
                }
                panel.error = None;
                panel.members.feedback = None;
                panel.members.confirm = None;
                self.generation += 1;
                panel.members.request = self.generation;
                if let Some(demo) = &self.demo {
                    let direct = !panel.space
                        && self
                            .model
                            .rooms
                            .iter()
                            .any(|r| r.id == panel.id && r.direct);
                    let rows = demo.members(0, &panel.id, direct).rows;
                    let can_manage = if matches!(host, Host::Sidebar) {
                        self.model
                            .rooms
                            .iter()
                            .find(|r| r.id == panel.id)
                            .is_some_and(|r| r.can_invite)
                    } else {
                        panel.saved.as_ref().is_some_and(|s| s.editable[0])
                    };
                    let roster = Roster {
                        banned: demo
                            .banned_members
                            .get(&panel.id)
                            .cloned()
                            .unwrap_or_default(),
                        can_invite: can_manage,
                        can_ban: can_manage,
                        can_unban: can_manage,
                        rows: rows
                            .iter()
                            .cloned()
                            .map(|member| {
                                let value = match member.role.as_str() {
                                    "Admin" => 100,
                                    "Moderator" => 50,
                                    _ => 0,
                                };
                                let own = member.user_id == "@you:example.org";
                                let roles = if can_manage && !own && value < 100 && !member.invited
                                {
                                    [Role::Member, Role::Moderator, Role::Admin]
                                        .into_iter()
                                        .filter(|r| i64::from(power(*r)) != value)
                                        .collect()
                                } else {
                                    vec![]
                                };
                                ManagedMember {
                                    can_remove: can_manage && !own && value < 100,
                                    can_ban: can_manage && !own && value < 100,
                                    power: UserPowerLevel::Int(value.try_into().unwrap()),
                                    member,
                                    roles,
                                }
                            })
                            .collect(),
                    };
                    panel.members.roster = Some(roster);
                } else if let Some(control) = panel.control.clone() {
                    panel.pending = true;
                    let operation = panel.members.request;
                    let (task, handle) = Task::perform(control.members(), move |result| {
                        host.message(Event::Loaded(session, operation, Box::new(result)))
                    })
                    .abortable();
                    panel.members.load = Some(handle.abort_on_drop());
                    return task;
                }
            }
            Event::Banned(banned) => {
                panel.members.banned = banned;
                panel.members.page = 0;
                panel.members.query.clear();
                panel.members.confirm = None;
            }
            Event::Search(query) => {
                panel.members.confirm = None;
                panel.members.query = query;
                panel.members.page = 0;
            }
            Event::Page(page) => {
                panel.members.confirm = None;
                panel.members.page = page;
            }
            Event::Picker(request, picker) if request == session && !panel.pending => {
                if matches!(picker, people_picker::Event::Submit) {
                    return self.member_management_event(host, Event::Invite);
                }
                if matches!(
                    picker,
                    people_picker::Event::Edit(_)
                        | people_picker::Event::Select(_)
                        | people_picker::Event::Retry
                ) {
                    panel.error = None;
                    panel.success = false;
                }
                return panel
                    .members
                    .picker
                    .update(
                        picker,
                        self.model.people_directory.clone(),
                        self.demo.is_some(),
                    )
                    .map(move |picker| host.message(Event::Picker(session, picker)));
            }
            Event::BanInput(request, input) if request == session && !panel.pending => {
                panel.members.confirm = None;
                panel.members.ban_user = input;
                panel.error = None;
            }
            Event::PrepareBanId(request) if request == session && !panel.pending => {
                panel.members.confirm = None;
                let Some(roster) = &panel.members.roster else {
                    return Task::none();
                };
                if !roster.can_ban {
                    return Task::none();
                }
                let user = match interconnect::people::parse_user_id(&panel.members.ban_user) {
                    Ok(user) => user.to_string(),
                    Err(error) => {
                        panel.error = Some(error);
                        return Task::none();
                    }
                };
                let own = self.account_id.as_deref().unwrap_or("@you:example.org");
                if user == own
                    || roster
                        .rows
                        .iter()
                        .any(|m| m.member.user_id == user && !m.can_ban)
                {
                    panel.error = Some("You cannot ban yourself or this person's role does not permit you to ban them.".into());
                } else if roster.banned.iter().any(|m| m.user_id == user) {
                    panel.error = Some("This person is already banned.".into());
                } else {
                    panel.error = None;
                    panel.success = false;
                    panel.members.feedback = None;
                    panel.members.begin_confirmation(MemberChange::BanId(user));
                }
            }
            Event::Remove(user) if !panel.pending => {
                panel.success = false;
                panel.members.feedback = None;
                if let Some(member) = panel.members.roster.as_ref().and_then(|r| {
                    r.rows
                        .iter()
                        .find(|m| m.member.user_id == user && m.can_remove)
                }) {
                    panel.members.begin_confirmation(MemberChange::Remove {
                        user,
                        invited: member.member.invited,
                    });
                }
            }
            Event::Unban(user) if !panel.pending => {
                panel.success = false;
                panel.members.feedback = None;
                if panel
                    .members
                    .roster
                    .as_ref()
                    .is_some_and(|r| r.banned.iter().any(|m| m.user_id == user && m.can_unban))
                {
                    panel.members.begin_confirmation(MemberChange::Unban(user));
                }
            }
            Event::Ban(user) if !panel.pending => {
                panel.success = false;
                panel.members.feedback = None;
                if let Some(member) = panel.members.roster.as_ref().and_then(|r| {
                    r.rows
                        .iter()
                        .find(|m| m.member.user_id == user && m.can_ban)
                }) {
                    panel.members.begin_confirmation(MemberChange::Ban {
                        user,
                        invited: member.member.invited,
                    });
                }
            }
            Event::Role(user, role) if !panel.pending => {
                panel.success = false;
                panel.members.feedback = None;
                if let Some(member) = panel.members.roster.as_ref().and_then(|r| {
                    r.rows
                        .iter()
                        .find(|m| m.member.user_id == user && m.roles.contains(&role))
                }) {
                    panel.members.begin_confirmation(MemberChange::Role {
                        user,
                        before: member.power,
                        role,
                    });
                }
            }
            Event::Reason(request, confirmation, reason)
                if request == session
                    && confirmation == panel.members.reason_request
                    && !panel.pending
                    && panel.members.confirm.is_some() =>
            {
                panel.members.reason = reason;
                panel.error = None;
            }
            Event::Cancel if !panel.pending => panel.members.confirm = None,
            Event::Invite | Event::Confirm
                if !panel.pending
                    && self.pending_ops < MAX_PENDING_OPERATIONS
                    && !self.composing
                    && !self.inviting.contains(&panel.id) =>
            {
                let change = if matches!(incoming, Event::Invite) {
                    let Some(roster) = &panel.members.roster else {
                        return Task::none();
                    };
                    if !roster.can_invite {
                        return Task::none();
                    }
                    let user =
                        match interconnect::people::parse_user_id(&panel.members.picker.query) {
                            Ok(user) => user.to_string(),
                            Err(error) => {
                                panel.error = Some(error);
                                return Task::none();
                            }
                        };
                    if roster.rows.iter().any(|m| m.member.user_id == user) {
                        panel.error = Some(
                            "This person is already a member or has a pending invitation.".into(),
                        );
                        return Task::none();
                    }
                    MemberChange::Invite(user)
                } else {
                    let Some(change) = panel.members.confirm.clone() else {
                        return Task::none();
                    };
                    change
                };
                let reason =
                    if matches!(change, MemberChange::Invite(_) | MemberChange::Role { .. }) {
                        String::new()
                    } else {
                        panel.members.reason.clone()
                    };
                if let Err(error) = interconnect::moderation::reason(&reason) {
                    panel.error = Some(error);
                    return Task::none();
                }
                panel.error = None;
                panel.success = false;
                panel.members.picker.cancel_search();
                if let Some(demo) = &mut self.demo {
                    if let Some(roster) = &mut panel.members.roster {
                        apply(
                            roster,
                            &change,
                            BanDetails {
                                actor: Some("@you:example.org".into()),
                                timestamp_ms: Some(chrono::Utc::now().timestamp_millis()),
                                reason: interconnect::moderation::reason(&panel.members.reason)
                                    .ok()
                                    .flatten(),
                            },
                        );
                        demo.banned_members
                            .insert(panel.id.clone(), roster.banned.clone());
                        demo.managed_members.insert(
                            panel.id.clone(),
                            roster.rows.iter().map(|m| m.member.clone()).collect(),
                        );
                    }
                    if let MemberChange::Remove { user, .. }
                    | MemberChange::Ban { user, .. }
                    | MemberChange::BanId(user) = &change
                        && let Some(invitations) = demo.invitations.get_mut(&panel.id)
                    {
                        invitations.retain(|m| m.user_id != *user);
                    }
                    if self
                        .selection
                        .room
                        .as_ref()
                        .is_some_and(|(_, id)| *id == panel.id)
                        && let Some(snapshot) = &mut self.model.members
                        && Some(snapshot.generation) == self.selection.members
                    {
                        apply_snapshot(snapshot, &change);
                    }
                    panel.members.confirm = None;
                    panel.members.reason.clear();
                    panel.members.picker.clear();
                    panel.members.feedback = Some(change_feedback(&change));
                    panel.success = true;
                } else if let Some(control) = panel.control.clone() {
                    self.generation += 1;
                    let operation = self.generation;
                    panel.pending = true;
                    panel.members.request = operation;
                    self.inviting.insert(panel.id.clone());
                    self.pending_ops += 1;
                    self.space_settings
                        .writes
                        .insert(operation, (panel.account.clone(), panel.id.clone()));
                    let completed = change.clone();
                    return Task::perform(
                        control.change_member_with_reason(change, reason),
                        move |result| {
                            host.message(Event::Saved(
                                session,
                                operation,
                                completed.clone(),
                                result,
                            ))
                        },
                    );
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub(crate) fn sidebar_member_controls<'a>(
        &'a self,
        member: &'a Member,
    ) -> Element<'a, Message> {
        if let Some(panel) = &self.space_settings.sidebar
            && let Some(managed) = panel.members.roster.as_ref().and_then(|roster| {
                roster.rows.iter().find(|row| {
                    row.member.user_id == member.user_id && row.member.invited == member.invited
                })
            })
        {
            return member_controls(
                managed,
                panel.pending || self.inviting.contains(&panel.id),
                Host::Sidebar,
            );
        }
        text(if member.invited {
            "Invited"
        } else if member.role.is_empty() {
            "Member"
        } else {
            &member.role
        })
        .size(11)
        .style(shell::muted_text)
        .into()
    }
    pub(crate) fn member_sidebar_feedback(&self) -> Option<Element<'_, Message>> {
        let panel = self.space_settings.sidebar.as_ref()?;
        if panel.error.is_none() && !panel.pending && panel.members.feedback.is_none() {
            return None;
        }
        let mut content = column![].spacing(8);
        if let Some(error) = &panel.error {
            content = content.push(text(error).size(12));
        }
        if panel.pending {
            content = content.push(
                text(if panel.load.is_some() || panel.members.load.is_some() {
                    "Loading member controls…"
                } else {
                    "Saving…"
                })
                .size(12),
            );
        }
        if let Some(feedback) = &panel.members.feedback {
            content = content.push(text(feedback).size(12).style(shell::muted_text));
        }
        Some(content.into())
    }
    pub(crate) fn member_sidebar_confirmation(&self, user: &str) -> Option<Element<'_, Message>> {
        let panel = self.space_settings.sidebar.as_ref()?;
        let change = panel
            .members
            .confirm
            .as_ref()
            .filter(|change| change_user(change) == user)?;
        Some(confirmation(
            change,
            false,
            panel.pending,
            Host::Sidebar,
            &panel.members.reason,
            (panel.request, panel.members.reason_request),
        ))
    }
    pub(crate) fn member_management_avatars(&self) -> Vec<matrix_sdk::ruma::OwnedMxcUri> {
        self.space_settings
            .panel
            .as_ref()
            .into_iter()
            .flat_map(|p| {
                p.members
                    .picker
                    .results
                    .iter()
                    .chain(p.members.picker.selected.iter())
            })
            .filter_map(|p| p.avatar_url.clone())
            .collect()
    }
    pub(in crate::space_settings) fn member_management_view(&self) -> Element<'_, Message> {
        let panel = self.space_settings.panel.as_ref().unwrap();
        let state = &panel.members;
        let busy = panel.pending || self.inviting.contains(&panel.id);
        let mut content = column![
            rule::horizontal(1),
            row![
                icons::icon(Kind::People),
                shell::title("Members", 18),
                iced::widget::space().width(Length::Fill),
                icon_button(
                    Kind::Refresh,
                    "Reload members",
                    (!busy).then_some(event(Event::Reload))
                )
            ]
            .spacing(8),
            text("Membership and roles are snapshots. Reload for the latest state.")
                .size(12)
                .style(shell::muted_text)
        ]
        .spacing(12);
        let Some(roster) = &state.roster else {
            return content.into();
        };
        content = content.push(
            row![
                button(text("Members").size(13))
                    .keyboard_navigation(true)
                    .style(move |theme, status| shell::navigation_button(
                        theme,
                        !state.banned,
                        status
                    ))
                    .on_press(event(Event::Banned(false))),
                button(text(format!("Banned ({})", roster.banned.len())).size(13))
                    .keyboard_navigation(true)
                    .style(move |theme, status| shell::navigation_button(
                        theme,
                        state.banned,
                        status
                    ))
                    .on_press(event(Event::Banned(true))),
            ]
            .spacing(8),
        );
        if roster.can_ban {
            let request = panel.request;
            let mut form = column![
                shell::title("Ban by Matrix ID", 14),
                text_input("@name:server", &state.ban_user)
                    .cursor_blinking(false)
                    .padding(10)
                    .on_input_maybe(
                        (!busy).then_some(move |value| event(Event::BanInput(request, value)))
                    ),
                button(text("Review ban…").size(13))
                    .keyboard_navigation(true)
                    .padding([7, 10])
                    .style(shell::quiet_button)
                    .on_press_maybe(
                        (!busy && interconnect::people::parse_user_id(&state.ban_user).is_ok())
                            .then_some(event(Event::PrepareBanId(request)))
                    ),
            ]
            .spacing(8);
            if let Some(change @ MemberChange::BanId(_)) = &state.confirm {
                form = form.push(confirmation(
                    change,
                    panel.space,
                    busy,
                    Host::Panel,
                    &state.reason,
                    (panel.request, state.reason_request),
                ));
            }
            content = content.push(form);
        }
        if roster.can_invite && !state.banned {
            let request = panel.request;
            content = content.push(
                column![
                    shell::title("Invite people", 14),
                    state
                        .picker
                        .view(&self.model.avatars, !busy)
                        .map(move |picker| event(Event::Picker(request, picker))),
                    button(
                        row![
                            icons::icon(Kind::PersonAdd),
                            text("Send invitation").size(13)
                        ]
                        .spacing(7)
                        .align_y(iced::Alignment::Center)
                    )
                    .padding([7, 10])
                    .style(shell::primary_button)
                    .keyboard_navigation(true)
                    .on_press_maybe(
                        (!busy
                            && self.pending_ops < MAX_PENDING_OPERATIONS
                            && interconnect::people::parse_user_id(&state.picker.query).is_ok())
                        .then_some(event(Event::Invite))
                    )
                ]
                .spacing(8),
            );
        }
        content = content.push(
            text_input(
                if state.banned {
                    "Find a banned member…"
                } else {
                    "Find a member…"
                },
                &state.query,
            )
            .cursor_blinking(false)
            .on_input(|q| event(Event::Search(q)))
            .padding(10)
            .style(shell::search_style),
        );
        let query = state.query.to_lowercase();
        if state.banned {
            let rows: Vec<_> = roster
                .banned
                .iter()
                .filter(|m| {
                    m.name.to_lowercase().contains(&query)
                        || m.user_id.to_lowercase().contains(&query)
                })
                .collect();
            let page = state.page.min(rows.len().saturating_sub(1) / PAGE);
            content = content.push(text("Unbanning does not invite or rejoin a person. Space bans do not apply to child rooms.").size(12).style(shell::muted_text));
            for member in rows.iter().skip(page * PAGE).take(PAGE) {
                content = content.push(
                    column![
                        row![
                            column![
                                super::super::single_line::label(&member.name, 14),
                                super::super::single_line::label(&member.user_id, 11)
                                    .style(shell::muted_text)
                            ]
                            .spacing(3)
                            .width(Length::Fill),
                            button(text("Unban…").size(12))
                                .padding([6, 8])
                                .style(shell::quiet_button)
                                .keyboard_navigation(true)
                                .on_press_maybe(
                                    (member.can_unban && !busy)
                                        .then(|| event(Event::Unban(member.user_id.clone())))
                                ),
                        ]
                        .spacing(12)
                        .align_y(iced::Alignment::Center),
                    ]
                    .spacing(8),
                );
                if let Some(details) = &member.details {
                    content = content.push(
                        text(format!(
                            "Banned by {}",
                            details
                                .actor
                                .as_deref()
                                .unwrap_or("an unavailable moderator")
                        ))
                        .size(12)
                        .style(shell::muted_text),
                    );
                    let time = details
                        .timestamp_ms
                        .and_then(chrono::DateTime::from_timestamp_millis)
                        .map(|time| {
                            time.with_timezone(&chrono::Local)
                                .format("%b %-d, %Y · %H:%M")
                                .to_string()
                        });
                    content = content.push(
                        text(time.unwrap_or_else(|| {
                            "Time unavailable · Reload for server details".into()
                        }))
                        .size(12)
                        .style(shell::muted_text),
                    );
                    content = content.push(
                        text(details.reason.as_deref().unwrap_or("No reason available.")).size(13),
                    );
                } else {
                    content = content.push(
                        text("Ban details unavailable. Use Reload to check again.")
                            .size(12)
                            .style(shell::muted_text),
                    );
                }
                if let Some(change) = state
                    .confirm
                    .as_ref()
                    .filter(|c| change_user(c) == member.user_id)
                {
                    content = content.push(confirmation(
                        change,
                        panel.space,
                        busy,
                        Host::Panel,
                        &state.reason,
                        (panel.request, state.reason_request),
                    ));
                }
                content = content.push(rule::horizontal(1));
            }
            if rows.is_empty() {
                content = content.push(
                    text(if roster.banned.is_empty() {
                        "No banned members."
                    } else {
                        "No matching banned members."
                    })
                    .size(13),
                );
            }
            if rows.len() > PAGE {
                content = content.push(
                    row![
                        icon_button(
                            Kind::ChevronLeft,
                            "Previous banned members",
                            (page > 0).then(|| event(Event::Page(page - 1)))
                        ),
                        text(format!("{} / {}", page + 1, rows.len().div_ceil(PAGE))).size(12),
                        icon_button(
                            Kind::ChevronRight,
                            "Next banned members",
                            ((page + 1) * PAGE < rows.len()).then(|| event(Event::Page(page + 1)))
                        ),
                    ]
                    .spacing(10),
                );
            }
            if let Some(feedback) = &state.feedback {
                content = content.push(text(feedback).size(13).style(shell::muted_text));
            }
            return content.into();
        }
        let rows: Vec<_> = roster
            .rows
            .iter()
            .filter(|m| {
                m.member.name.to_lowercase().contains(&query)
                    || m.member.user_id.to_lowercase().contains(&query)
            })
            .collect();
        let page = state.page.min(rows.len().saturating_sub(1) / PAGE);
        let pending = roster.rows.iter().filter(|m| m.member.invited).count();
        content = content.push(
            text(format!(
                "{} members · {pending} pending invitations",
                roster.rows.len() - pending
            ))
            .size(12)
            .style(shell::muted_text),
        );
        for managed in rows.iter().skip(page * PAGE).take(PAGE) {
            let member = &managed.member;
            let identity = column![
                super::super::single_line::label(&member.name, 14),
                super::super::single_line::label(&member.user_id, 11).style(shell::muted_text),
            ]
            .spacing(3)
            .width(Length::Fill);
            let mut entry = column![
                row![identity, member_controls(managed, busy, Host::Panel)]
                    .spacing(12)
                    .align_y(iced::Alignment::Center)
            ]
            .spacing(10);
            if let Some(change) = state.confirm.as_ref().filter(|change| {
                !matches!(change, MemberChange::BanId(_)) && change_user(change) == member.user_id
            }) {
                entry = entry.push(confirmation(
                    change,
                    panel.space,
                    busy,
                    Host::Panel,
                    &state.reason,
                    (panel.request, state.reason_request),
                ));
            }
            entry = entry.push(rule::horizontal(1));
            content = content.push(entry);
        }
        if let Some(feedback) = &state.feedback {
            content = content.push(text(feedback).size(13).style(shell::muted_text));
        }
        if rows.is_empty() {
            content = content.push(text("No matching members.").size(13));
        }
        if rows.len() > PAGE {
            content = content.push(
                row![
                    icon_button(
                        Kind::ChevronLeft,
                        "Previous members",
                        (page > 0).then(|| event(Event::Page(page - 1)))
                    ),
                    text(format!("{} / {}", page + 1, rows.len().div_ceil(PAGE))).size(12),
                    icon_button(
                        Kind::ChevronRight,
                        "Next members",
                        ((page + 1) * PAGE < rows.len()).then(|| event(Event::Page(page + 1)))
                    )
                ]
                .spacing(10),
            );
        }
        content.into()
    }
}

pub(crate) fn member_controls<'a>(
    managed: &'a ManagedMember,
    busy: bool,
    host: Host,
) -> Element<'a, Message> {
    let member = &managed.member;
    let role: Element<'_, Message> = if !managed.roles.is_empty() && !busy {
        let user = member.user_id.clone();
        let current = RoleChoice {
            label: if member.role.is_empty() {
                "Member".into()
            } else {
                member.role.clone()
            },
            role: None,
        };
        let choices = std::iter::once(current.clone())
            .chain(managed.roles.iter().map(|role| RoleChoice {
                label: role.to_string(),
                role: Some(*role),
            }))
            .collect::<Vec<_>>();
        pick_list(choices, Some(current), move |choice| {
            host.message(
                choice
                    .role
                    .map_or(Event::Ignore, |role| Event::Role(user.clone(), role)),
            )
        })
        .style(shell::select_style)
        .keyboard_navigation(true)
        .padding([6, 8])
        .width(Length::Fill)
        .text_size(12)
        .into()
    } else {
        text(if member.invited {
            "Invited"
        } else if member.role.is_empty() {
            "Member"
        } else {
            &member.role
        })
        .size(12)
        .style(shell::muted_text)
        .into()
    };
    let removal: Element<'_, Message> = if managed.can_remove {
        icon_button(
            if member.invited {
                Kind::Close
            } else {
                Kind::PersonRemove
            },
            if member.invited {
                "Revoke invitation"
            } else {
                "Remove member"
            },
            (!busy).then(|| host.message(Event::Remove(member.user_id.clone()))),
        )
        .into()
    } else {
        iced::widget::space().into()
    };

    let ban = button(text("Ban").size(12))
        .padding([6, 8])
        .style(shell::quiet_button)
        .keyboard_navigation(true)
        .on_press_maybe(
            (managed.can_ban && !busy).then(|| host.message(Event::Ban(member.user_id.clone()))),
        );
    let moderation: Element<'_, Message> = if managed.can_ban {
        row![ban, removal].spacing(4).into()
    } else {
        removal
    };
    if matches!(host, Host::Sidebar) {
        row![
            container(role).width(128),
            iced::widget::space().width(Length::Fill),
            moderation,
        ]
        .spacing(8)
        .width(Length::Fill)
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        row![container(role).width(128), moderation]
            .spacing(12)
            .align_y(iced::Alignment::Center)
            .into()
    }
}
#[derive(Clone, PartialEq, Eq)]
struct RoleChoice {
    label: String,
    role: Option<Role>,
}
impl std::fmt::Display for RoleChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
fn change_user(change: &MemberChange) -> &str {
    match change {
        MemberChange::Invite(user)
        | MemberChange::BanId(user)
        | MemberChange::Ban { user, .. }
        | MemberChange::Unban(user)
        | MemberChange::Remove { user, .. }
        | MemberChange::Role { user, .. } => user,
    }
}
fn change_feedback(change: &MemberChange) -> String {
    match change {
        MemberChange::Invite(user) => format!("Invitation sent to {user}."),
        MemberChange::Ban { user, .. } | MemberChange::BanId(user) => format!("{user} banned."),
        MemberChange::Unban(user) => {
            format!("{user} unbanned. They have not been invited or rejoined.")
        }
        MemberChange::Remove {
            user,
            invited: true,
        } => format!("Invitation for {user} revoked."),
        MemberChange::Remove {
            user,
            invited: false,
        } => format!("{user} removed."),
        MemberChange::Role { user, role, .. } => {
            format!("{user} is now {role}. Reload permissions before another role change.")
        }
    }
}
fn confirmation<'a>(
    change: &'a MemberChange,
    space: bool,
    busy: bool,
    host: Host,
    reason: &'a str,
    request: (u64, u64),
) -> Element<'a, Message> {
    let description = match change {
        MemberChange::Remove { user, invited } => format!(
            "{} {user}?",
            if *invited {
                "Revoke the invitation for"
            } else {
                "Remove"
            }
        ),
        MemberChange::Role { user, role, .. } => format!("Make {user} {role}?"),
        MemberChange::Ban { user, .. } | MemberChange::BanId(user) => {
            format!("Ban {user}? They cannot join or be invited until unbanned.")
        }
        MemberChange::Unban(user) => {
            format!("Unban {user}? This allows joining or inviting under the current access rules.")
        }
        _ => String::new(),
    };
    let label = match change {
        MemberChange::Remove { invited: true, .. } => "Revoke invitation",
        MemberChange::Remove { invited: false, .. } => "Remove member",
        MemberChange::Role { .. } => "Change role",
        MemberChange::Invite(_) => "Send invitation",
        MemberChange::Ban { .. } | MemberChange::BanId(_) => "Ban person",
        MemberChange::Unban(_) => "Unban member",
    };
    let destructive = matches!(
        change,
        MemberChange::Remove { .. } | MemberChange::Ban { .. } | MemberChange::BanId(_)
    );
    let mut content = column![
        text(description).size(14),
        text(if space {
            "This changes only this space. Membership in its rooms stays intact."
        } else {
            "This changes membership or permissions in this room."
        })
        .size(12)
        .style(shell::muted_text),
    ]
    .spacing(10);
    if !matches!(change, MemberChange::Invite(_) | MemberChange::Role { .. }) {
        content = content.push(
            text_input("Reason (optional, visible in room history)", reason)
                .cursor_blinking(false)
                .padding(8)
                .on_input_maybe((!busy).then_some(move |value| {
                    host.message(Event::Reason(request.0, request.1, value))
                })),
        );
    }
    if let Err(error) = interconnect::moderation::reason(reason) {
        content = content.push(text(error).size(12).style(shell::danger_text));
    }
    content = content.push(
        row![
            button(
                row![icons::icon(Kind::Check), text(label).size(13)]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
            )
            .style(if destructive {
                button::danger
            } else {
                shell::primary_button
            })
            .keyboard_navigation(true)
            .on_press_maybe(
                (!busy && interconnect::moderation::reason(reason).is_ok())
                    .then_some(host.message(Event::Confirm))
            ),
            button(text("Cancel").size(13))
                .padding([7, 10])
                .style(shell::quiet_button)
                .keyboard_navigation(true)
                .on_press_maybe((!busy).then_some(host.message(Event::Cancel)))
        ]
        .spacing(8)
        .wrap(),
    );
    container(content)
        .padding(12)
        .style(|theme| shell::surface(theme, shell::palette(theme).rail))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{space_settings, tests::demo_app};
    fn open(app: &mut App, space: bool) -> u64 {
        let target = if space {
            PanelEvent::Open("studio-space".into())
        } else {
            PanelEvent::OpenMembers("studio".into())
        };
        let _ = app.update(message(target));
        app.space_settings.panel.as_ref().unwrap().request
    }
    fn rows(app: &App) -> &[ManagedMember] {
        &app.space_settings
            .panel
            .as_ref()
            .unwrap()
            .members
            .roster
            .as_ref()
            .unwrap()
            .rows
    }
    fn open_sidebar(app: &mut App) -> u64 {
        let _ = app.update(Message::Members(crate::members::Event::Open));
        app.space_settings.sidebar.as_ref().unwrap().request
    }
    #[test]
    fn reopening_room_members_resumes_after_an_old_member_write_finishes() {
        let mut app = demo_app();
        let session = open(&mut app, false);
        let id = app.space_settings.panel.as_ref().unwrap().id.clone();
        app.generation += 1;
        let operation = app.generation;
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), id.clone()));
        app.space_settings.panel.as_mut().unwrap().pending = true;
        app.pending_ops = 1;
        let _ = app.update(message(PanelEvent::Close));
        let reopened = open(&mut app, false);
        assert!(app.space_settings.panel.as_ref().unwrap().saved.is_none());
        let _ = app.update(event(Event::Saved(
            session,
            operation,
            MemberChange::Remove {
                user: "@maya:example.org".into(),
                invited: false,
            },
            Ok(()),
        )));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert_eq!(panel.id, id);
        assert!(panel.saved.is_some());
        assert!(panel.members.roster.is_some());
        assert!(panel.request > reopened);
        assert_eq!(app.pending_ops, 0);
    }

    #[test]
    fn member_confirmations_do_not_survive_a_filter_or_page_change() {
        let mut app = demo_app();
        open(&mut app, true);
        let _ = app.update(event(Event::Remove("@maya:example.org".into())));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .members
                .confirm
                .is_some()
        );
        let _ = app.update(event(Event::Search("Jamie".into())));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .members
                .confirm
                .is_none()
        );
        let _ = app.update(event(Event::Remove("@maya:example.org".into())));
        let _ = app.update(event(Event::Page(0)));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .members
                .confirm
                .is_none()
        );
    }

    #[test]
    fn people_sidebar_shares_role_removal_and_invitation_confirmations() {
        let mut app = demo_app();
        let _ = app.update(Message::WindowResized(1400.0));
        let room = app.selection.room.clone();
        let scope = app.shell.space.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 135.0);
        open_sidebar(&mut app);
        assert!(!app.space_settings.is_open());
        assert!(!app.side_panel_replaces_room(1400.0));
        for action in [
            Event::Remove("@you:example.org".into()),
            Event::Role("@alex:example.org".into(), Role::Member),
        ] {
            let _ = app.update(Host::Sidebar.message(action));
            assert!(
                app.space_settings
                    .sidebar
                    .as_ref()
                    .unwrap()
                    .members
                    .confirm
                    .is_none()
            );
        }
        let _ = app
            .update(Host::Sidebar.message(Event::Role("@sam:example.org".into(), Role::Moderator)));
        assert!(
            app.space_settings
                .sidebar
                .as_ref()
                .unwrap()
                .members
                .confirm
                .is_some()
        );
        let _ = app.members_view();
        let _ = app.update(Host::Sidebar.message(Event::Cancel));
        assert!(
            app.model
                .members
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|m| m.user_id == "@sam:example.org")
                .unwrap()
                .role
                .is_empty()
        );
        let _ = app
            .update(Host::Sidebar.message(Event::Role("@sam:example.org".into(), Role::Moderator)));
        let _ = app.update(Host::Sidebar.message(Event::Confirm));
        assert_eq!(
            app.model
                .members
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|m| m.user_id == "@sam:example.org")
                .unwrap()
                .role,
            "Moderator"
        );
        for user in ["@maya:example.org", "@casey:example.org"] {
            let _ = app.update(Host::Sidebar.message(Event::Remove(user.into())));
            assert!(
                app.space_settings
                    .sidebar
                    .as_ref()
                    .unwrap()
                    .members
                    .confirm
                    .is_some()
            );
            let _ = app.update(Host::Sidebar.message(Event::Confirm));
            assert!(
                !app.model
                    .members
                    .as_ref()
                    .unwrap()
                    .rows
                    .iter()
                    .any(|m| m.user_id == user)
            );
        }
        let _ = app.update(Message::Members(crate::members::Event::Search(
            "sam".into(),
        )));
        let _ = app.update(Host::Sidebar.message(Event::Reload));
        assert_eq!(app.visible_members().len(), 1);
        assert_eq!(app.selection.room, room);
        assert_eq!(app.shell.space, scope);
        assert_eq!(app.drafts[&key].content.text(), "unsent");
        assert_eq!(app.scroll_positions[&key], 135.0);
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn sidebar_sessions_reject_late_reads_and_release_writes_once_after_closing() {
        let mut app = demo_app();
        let old = open_sidebar(&mut app);
        let id = app.selection.room.as_ref().unwrap().1.clone();
        app.generation += 1;
        let operation = app.generation;
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), id.clone()));
        app.inviting.insert(id.clone());
        app.pending_ops = 1;
        let _ = app.update(Message::Members(crate::members::Event::Close));
        assert!(app.space_settings.sidebar.is_none());
        let current = open_sidebar(&mut app);
        let _ = app
            .update(Host::Sidebar.message(Event::Ready(old, Box::new(Err("late lookup".into())))));
        let _ = app.update(Host::Sidebar.message(Event::Loaded(
            old,
            operation,
            Box::new(Err("late roster".into())),
        )));
        let saved = Host::Sidebar.message(Event::Saved(
            old,
            operation,
            MemberChange::Role {
                user: "@sam:example.org".into(),
                before: UserPowerLevel::Int(matrix_sdk::ruma::int!(0)),
                role: Role::Admin,
            },
            Err("late write".into()),
        ));
        let _ = app.update(saved.clone());
        let _ = app.update(saved);
        assert_eq!(app.pending_ops, 0);
        assert!(!app.inviting.contains(&id));
        let panel = app.space_settings.sidebar.as_ref().unwrap();
        assert_eq!(panel.request, current);
        assert!(panel.error.is_none());
        let _ = app.update(Message::Thread("proposal".into()));
        assert!(app.space_settings.sidebar.is_none());
    }
    #[test]
    fn moderation_reasons_survive_errors_and_are_specific_to_each_confirmation() {
        for space in [false, true] {
            let mut app = demo_app();
            let session = open(&mut app, space);
            let _ = app.update(event(Event::Ban("@maya:example.org".into())));
            let _ = app.update(event(Event::Reason(session, 1, "  Spam  ".into())));
            let _ = app.update(event(Event::Reason(session + 999, 1, "stale".into())));
            assert_eq!(
                app.space_settings.panel.as_ref().unwrap().members.reason,
                "  Spam  "
            );
            let _ = app.update(event(Event::Confirm));
            let details = app
                .space_settings
                .panel
                .as_ref()
                .unwrap()
                .members
                .roster
                .as_ref()
                .unwrap()
                .banned[0]
                .details
                .as_ref()
                .unwrap();
            assert_eq!(details.reason.as_deref(), Some("Spam"));
            assert_eq!(details.actor.as_deref(), Some("@you:example.org"));
            assert!(details.timestamp_ms.is_some());
            let _ = app.update(event(Event::Reload));
            let _ = app.update(event(Event::Unban("@maya:example.org".into())));
            assert!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .reason
                    .is_empty()
            );
            let _ = app.update(event(Event::Reason(
                session,
                1,
                "stale previous confirmation".into(),
            )));
            assert!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .reason
                    .is_empty()
            );
            let _ = app.update(event(Event::Reason(session, 2, "Reviewed".into())));
            app.generation += 1;
            let operation = app.generation;
            let panel = app.space_settings.panel.as_mut().unwrap();
            panel.pending = true;
            app.space_settings
                .writes
                .insert(operation, (panel.account.clone(), panel.id.clone()));
            app.pending_ops += 1;
            let _ = app.update(event(Event::Saved(
                session,
                operation,
                MemberChange::Unban("@maya:example.org".into()),
                Err("fixture refusal".into()),
            )));
            let panel = app.space_settings.panel.as_ref().unwrap();
            assert_eq!(panel.members.reason, "Reviewed");
            assert!(panel.members.confirm.is_some());
            assert_eq!(app.pending_ops, 0);
            let _ = app.update(event(Event::Confirm));
            assert!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .roster
                    .as_ref()
                    .unwrap()
                    .banned
                    .is_empty()
            );
        }
    }
    #[test]
    fn ban_by_id_is_confirmed_session_scoped_and_preserves_the_conversation() {
        for space in [false, true] {
            let mut app = demo_app();
            let selection = app.selection.room.clone();
            let key = app.draft_key(false).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("unsent");
            app.scroll_positions.insert(key.clone(), 120.0);
            let session = open(&mut app, space);
            for input in ["Maya", "@you:example.org", "@alex:example.org"] {
                let _ = app.update(event(Event::BanInput(session, input.into())));
                let _ = app.update(event(Event::PrepareBanId(session)));
                assert!(
                    app.space_settings
                        .panel
                        .as_ref()
                        .unwrap()
                        .members
                        .confirm
                        .is_none()
                );
            }
            let user = "@departed:elsewhere.org";
            let _ = app.update(event(Event::BanInput(session, user.into())));
            let _ = app.update(event(Event::PrepareBanId(session)));
            assert!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .roster
                    .as_ref()
                    .unwrap()
                    .banned
                    .is_empty()
            );
            let _ = app.update(event(Event::Cancel));
            let _ = app.update(event(Event::Confirm));
            assert!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .roster
                    .as_ref()
                    .unwrap()
                    .banned
                    .is_empty()
            );
            let _ = app.update(event(Event::PrepareBanId(session)));
            let _ = app.update(event(Event::Confirm));
            let _ = app.update(event(Event::Reload));
            assert_eq!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .roster
                    .as_ref()
                    .unwrap()
                    .banned[0]
                    .user_id,
                user
            );
            let _ = app.update(event(Event::PrepareBanId(session)));
            assert!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .confirm
                    .is_none()
            );
            let _ = app.update(event(Event::BanInput(
                session + 999,
                "@stale:example.org".into(),
            )));
            assert_eq!(
                app.space_settings.panel.as_ref().unwrap().members.ban_user,
                user
            );
            assert_eq!(app.selection.room, selection);
            assert_eq!(app.drafts[&key].content.text(), "unsent");
            assert_eq!(app.scroll_positions[&key], 120.0);
            assert_eq!(app.pending_ops, 0);
        }
    }
    #[test]
    fn banned_lists_survive_reload_and_unbanning_never_rejoins_or_invites() {
        for space in [false, true] {
            let mut app = demo_app();
            let selected = app.selection.room.clone();
            open(&mut app, space);
            let user = "@maya:example.org";
            let _ = app.update(event(Event::Ban(user.into())));
            let _ = app.update(event(Event::Confirm));
            let _ = app.update(event(Event::Banned(true)));
            let _ = app.update(event(Event::Reload));
            assert_eq!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .roster
                    .as_ref()
                    .unwrap()
                    .banned[0]
                    .user_id,
                user
            );
            let _ = app.update(event(Event::Unban(user.into())));
            let _ = app.update(event(Event::Search("hidden".into())));
            let _ = app.update(event(Event::Confirm));
            assert_eq!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .roster
                    .as_ref()
                    .unwrap()
                    .banned
                    .len(),
                1
            );
            let _ = app.update(event(Event::Search(String::new())));
            let _ = app.update(event(Event::Unban(user.into())));
            let _ = app.update(event(Event::Confirm));
            let _ = app.update(event(Event::Reload));
            assert!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .roster
                    .as_ref()
                    .unwrap()
                    .banned
                    .is_empty()
            );
            assert!(!rows(&app).iter().any(|m| m.member.user_id == user));
            assert_eq!(app.selection.room, selected);
            assert_eq!(app.pending_ops, 0);
        }
    }
    #[test]
    fn bans_require_confirmation_in_rooms_spaces_and_sidebar_and_preserve_drafts() {
        for space in [false, true] {
            let mut app = demo_app();
            let selected = app.selection.room.clone();
            let key = app.draft_key(false).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("unfinished");
            app.scroll_positions.insert(key.clone(), 120.0);
            open(&mut app, space);
            let user = "@maya:example.org";
            let _ = app.update(event(Event::Ban(user.into())));
            assert!(rows(&app).iter().any(|m| m.member.user_id == user));
            let _ = app.update(event(Event::Cancel));
            let _ = app.update(event(Event::Confirm));
            assert!(rows(&app).iter().any(|m| m.member.user_id == user));
            let _ = app.update(event(Event::Ban("@you:example.org".into())));
            assert!(
                app.space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .members
                    .confirm
                    .is_none()
            );
            let _ = app.update(event(Event::Ban(user.into())));
            let _ = app.update(event(Event::Confirm));
            assert!(!rows(&app).iter().any(|m| m.member.user_id == user));
            let _ = app.update(event(Event::Reload));
            assert!(!rows(&app).iter().any(|m| m.member.user_id == user));
            assert_eq!(app.selection.room, selected);
            assert_eq!(app.drafts[&key].content.text(), "unfinished");
            assert_eq!(app.scroll_positions[&key], 120.0);
        }
        let mut app = demo_app();
        open_sidebar(&mut app);
        let _ = app.update(Host::Sidebar.message(Event::Ban("@maya:example.org".into())));
        let _ = app.update(Host::Sidebar.message(Event::Confirm));
        assert!(
            !app.model
                .members
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|m| m.user_id == "@maya:example.org")
        );
    }
    #[test]
    fn room_and_space_editors_share_invites_confirmations_and_preserve_the_conversation() {
        for space in [false, true] {
            let mut app = demo_app();
            let selection = app.selection.room.clone();
            let session = open(&mut app, space);
            let _ = app.update(event(Event::Picker(
                session,
                people_picker::Event::Edit("@new:example.org".into()),
            )));
            let _ = app.update(event(Event::Invite));
            assert!(
                rows(&app)
                    .iter()
                    .any(|m| m.member.user_id == "@new:example.org" && m.member.invited)
            );
            let _ = app.update(event(Event::Invite));
            assert!(app.space_settings.panel.as_ref().unwrap().error.is_some());
            let _ = app.update(event(Event::Remove("@maya:example.org".into())));
            assert!(
                rows(&app)
                    .iter()
                    .any(|m| m.member.user_id == "@maya:example.org")
            );
            let _ = app.update(event(Event::Cancel));
            let _ = app.update(event(Event::Confirm));
            assert!(
                rows(&app)
                    .iter()
                    .any(|m| m.member.user_id == "@maya:example.org")
            );
            let _ = app.update(event(Event::Remove("@maya:example.org".into())));
            let _ = app.update(event(Event::Confirm));
            assert!(
                !rows(&app)
                    .iter()
                    .any(|m| m.member.user_id == "@maya:example.org")
            );
            let _ = app.update(event(Event::Role(
                "@sam:example.org".into(),
                Role::Moderator,
            )));
            let _ = app.update(event(Event::Confirm));
            assert_eq!(
                rows(&app)
                    .iter()
                    .find(|m| m.member.user_id == "@sam:example.org")
                    .unwrap()
                    .member
                    .role,
                "Moderator"
            );
            let _ = app.update(event(Event::Reload));
            assert!(
                !rows(&app)
                    .iter()
                    .any(|m| m.member.user_id == "@maya:example.org")
            );
            let _ = app.update(message(PanelEvent::Close));
            open(&mut app, space);
            assert_eq!(
                rows(&app)
                    .iter()
                    .find(|m| m.member.user_id == "@sam:example.org")
                    .unwrap()
                    .member
                    .role,
                "Moderator"
            );
            assert_eq!(app.selection.room, selection);
            assert_eq!(app.pending_ops, 0);
        }
    }
    #[test]
    fn stale_loads_picker_results_and_write_completions_cannot_replace_new_panels() {
        let mut app = demo_app();
        let old = open(&mut app, true);
        app.generation += 1;
        let operation = app.generation;
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), "studio-space".into()));
        app.pending_ops = 1;
        let _ = app.update(message(PanelEvent::Close));
        let current = open(&mut app, false);
        let before = rows(&app).len();
        let _ = app.update(event(Event::Loaded(
            old,
            operation,
            Box::new(Ok(Roster {
                rows: vec![],
                banned: vec![],
                can_invite: false,
                can_ban: false,
                can_unban: false,
            })),
        )));
        let _ = app.update(event(Event::Picker(
            old,
            people_picker::Event::Edit("@stale:example.org".into()),
        )));
        let change = MemberChange::Remove {
            user: "@maya:example.org".into(),
            invited: false,
        };
        let _ = app.update(event(Event::Saved(old, operation, change.clone(), Ok(()))));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(rows(&app).len(), before);
        assert_eq!(app.space_settings.panel.as_ref().unwrap().request, current);
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .members
                .picker
                .query
                .is_empty()
        );
        let _ = app.update(event(Event::Saved(old, operation, change, Ok(()))));
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn paging_search_and_permission_controls_are_bounded() {
        let mut app = demo_app();
        open(&mut app, true);
        let _ = app.update(event(Event::Remove("@alex:example.org".into())));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .members
                .confirm
                .is_none()
        );
        let _ = app.update(event(Event::Role("@you:example.org".into(), Role::Admin)));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .members
                .confirm
                .is_none()
        );
        let _ = app.update(event(Event::Page(usize::MAX)));
        let _ = app.update(event(Event::Search("no matches".into())));
        assert_eq!(app.space_settings.panel.as_ref().unwrap().members.page, 0);
        let _ = app.member_management_view();
    }
    #[test]
    fn management_transitions_release_hidden_member_avatars_and_request_picker_results() {
        let mut app = demo_app();
        let _ = app.update(Message::Members(crate::members::Event::Open));
        let uri: matrix_sdk::ruma::OwnedMxcUri = "mxc://example.org/hidden-member".into();
        std::sync::Arc::make_mut(&mut app.model.members.as_mut().unwrap().rows)[0].avatar_url =
            Some(uri.clone());
        app.demo = None;
        app.model.authentication = None;
        app.refresh_media_demand();
        assert!(app.selection.avatars.contains(&uri));
        let _ = app.update(message(PanelEvent::OpenMembers("studio".into())));
        assert!(app.space_settings.is_open());
        assert!(!app.selection.avatars.contains(&uri));
        let picker_uri: matrix_sdk::ruma::OwnedMxcUri = "mxc://example.org/picker-person".into();
        app.space_settings
            .panel
            .as_mut()
            .unwrap()
            .members
            .picker
            .results
            .push(interconnect::people::Person {
                user_id: "@picker:example.org".into(),
                name: "Picker".into(),
                avatar_url: Some(picker_uri.clone()),
            });
        let _ = app.update(event(Event::Search("query".into())));
        assert!(app.selection.avatars.contains(&picker_uri));
        let _ = app.update(message(PanelEvent::Close));
        assert!(!app.selection.avatars.contains(&picker_uri));
    }
    #[test]
    fn closing_management_schedules_scroll_restoration_for_both_conversation_panes() {
        let mut app = demo_app();
        let _ = app.update(Message::OpenLastThread);
        let room = app.draft_key(false).unwrap();
        let thread = app.draft_key(true).unwrap();
        app.scroll_positions.insert(room, 170.0);
        app.scroll_positions.insert(thread, 85.0);
        let selection = app.selection.clone();
        open(&mut app, true);
        app.close_space_settings();
        assert_eq!(app.pending_scroll, [Some(170.0), Some(85.0)]);
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert!(!app.space_settings.is_open());
    }
    #[test]
    fn room_people_and_settings_open_the_same_member_editor() {
        let mut app = demo_app();
        let _ = app.update(Message::RoomSettings(crate::room_settings::Event::Open(
            "studio".into(),
        )));
        let _ = app.update(Message::RoomSettings(crate::room_settings::Event::Members));
        assert!(app.space_settings.is_open());
        assert!(!app.room_settings.is_open());
        assert!(!app.space_settings.panel.as_ref().unwrap().space);
        let _ = app.update(Message::SpaceSettings(space_settings::Event::Close));
        let _ = app.update(Message::Members(crate::members::Event::Open));
        open(&mut app, false);
        assert!(app.space_settings.is_open());
    }
}
