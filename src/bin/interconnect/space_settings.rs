//! Full-size space settings and shared member management without changing conversations.
#[path = "member_management.rs"]
pub(crate) mod member_management;
#[path = "space_directory.rs"]
mod space_directory;
#[path = "space_exit.rs"]
mod space_exit;

use super::{App, MAX_PENDING_OPERATIONS, Message, icons::Kind, shell, tooltips::icon_button};
use super::{avatar_editor, room_settings::addresses};
use iced::{
    Element, Length, Task,
    widget::{column, container, pick_list, row, rule, scrollable, text, text_editor, text_input},
};
use interconnect::connected::room_settings::addresses::{Addresses, Change as AddressChange};
use interconnect::room_avatar::{Details, Upload};
use interconnect::{
    navigation::Scope,
    room_management::{Change, Management, Settings},
};
use matrix_sdk::ruma::OwnedMxcUri;
use matrix_sdk::ruma::events::room::join_rules::JoinRule;
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    panel: Option<Panel>,
    sidebar: Option<Panel>,
    writes: HashMap<u64, (Option<String>, String)>,
    demo: HashMap<String, Settings>,
    avatar_writes: HashMap<u64, (u64, Option<String>, String)>,
}
impl State {
    pub fn is_open(&self) -> bool {
        self.panel.is_some()
    }
    pub fn close(&mut self) {
        self.panel = None;
    }
    pub fn reset(&mut self) {
        self.close();
        self.demo.clear();
        self.sidebar = None;
    }
}
struct Panel {
    avatar: avatar_editor::State,
    addresses: addresses::Editor,
    topic: text_editor::Content,
    space: bool,
    scope: Scope,
    members: member_management::State,
    directory: Option<space_directory::State>,
    exit: space_exit::State,
    id: String,
    account: Option<String>,
    request: u64,
    pending: bool,
    load: Option<iced::task::Handle>,
    control: Option<Management>,
    saved: Option<Settings>,
    draft: Option<Settings>,
    error: Option<String>,
    success: bool,
    transition: Option<(u64, Box<Message>)>,
}
impl Panel {
    fn dirty(&self) -> bool {
        self.directory.as_ref().is_some_and(|state| state.dirty())
            || self.addresses.dirty()
            || self.avatar.edit().is_some()
            || self.avatar.is_picking()
            || self
                .saved
                .as_ref()
                .zip(self.draft.as_ref())
                .is_some_and(|(saved, draft)| {
                    saved.name != draft.name
                        || saved.topic != draft.topic
                        || saved.access != draft.access
                })
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    Avatar(u64, avatar_editor::Event),
    AvatarSaved(u64, Option<Upload>, Result<Option<OwnedMxcUri>, String>),
    Open(String),
    Browse(String),
    FindRooms,
    Directory(space_directory::Event),
    OpenMembers(String),
    Members(member_management::Event),
    SidebarMembers(member_management::Event),
    Exit(space_exit::Event),
    Close,
    KeepChanges(u64),
    DiscardChanges(u64),
    Reload,
    Name(String),
    #[cfg(test)]
    Topic(String),
    TopicEdit(text_editor::Action),
    Access(JoinRule),
    Save(usize),
    Addresses(addresses::Event),
    Commit(Change),
    Loaded(u64, Box<Result<(Management, Settings), String>>),
    Saved(u64, Change, Result<(), String>),
}
fn message(event: Event) -> Message {
    Message::SpaceSettings(event)
}
impl App {
    pub(super) fn guard_space_settings_transition(&mut self, next: Message) -> bool {
        let Some(panel) = &mut self.space_settings.panel else {
            return false;
        };
        if !panel.dirty() {
            return false;
        }
        if panel.transition.is_none() {
            self.generation += 1;
            panel.transition = Some((self.generation, Box::new(next)));
        }
        true
    }
    /// Resume an unopened form only after its captured account/target write has
    /// released admission. Never refresh over an already loaded or edited form.
    fn reload_waiting_space_settings(
        &mut self,
        account: &Option<String>,
        id: &str,
    ) -> Task<Message> {
        if !self.closing
            && !self.stopped
            && account == &self.account_id
            && !self
                .space_settings
                .writes
                .values()
                .any(|(pending_account, pending_id)| pending_account == account && pending_id == id)
            && self.space_settings.panel.as_ref().is_some_and(|panel| {
                &panel.account == account
                    && panel.id == id
                    && panel.saved.is_none()
                    && panel.load.is_none()
                    && !panel.pending
                    && panel.directory.is_none()
                    && !panel.exit.is_open()
                    && if panel.space {
                        matches!(&self.shell.space, Scope::Space(current) if current == id)
                    } else {
                        self.selection
                            .room
                            .as_ref()
                            .is_some_and(|(_, room)| room == id)
                    }
            })
        {
            return self.space_settings_event(Event::Reload);
        }
        Task::none()
    }
    pub(super) fn space_settings_avatar(&self) -> Option<OwnedMxcUri> {
        self.space_settings
            .panel
            .as_ref()
            .filter(|p| p.space && p.directory.is_none() && !p.exit.is_open())?
            .saved
            .as_ref()?
            .avatar
            .url
            .clone()
    }

    pub(crate) fn reload_member_sidebar(&mut self) -> Task<Message> {
        if self.selection.members.is_none() || self.closing || self.stopped {
            return Task::none();
        }
        if self
            .space_settings
            .sidebar
            .as_ref()
            .is_some_and(|p| p.pending)
        {
            return Task::none();
        }
        if self
            .space_settings
            .sidebar
            .as_ref()
            .is_some_and(|p| p.control.is_some())
            || self.demo.is_some() && self.space_settings.sidebar.is_some()
        {
            self.member_management_event(
                member_management::Host::Sidebar,
                member_management::Event::Reload,
            )
        } else {
            self.open_member_sidebar()
        }
    }
    pub(crate) fn open_member_sidebar(&mut self) -> Task<Message> {
        let Some((_, id)) = self.selection.room.clone() else {
            return Task::none();
        };
        self.generation += 1;
        let request = self.generation;
        self.space_settings.sidebar = Some(Panel {
            avatar: avatar_editor::State::default(),
            addresses: addresses::Editor::default(),
            topic: text_editor::Content::new(),
            space: false,
            scope: self.shell.space.clone(),
            members: member_management::State::default(),
            directory: None,
            exit: space_exit::State::default(),
            id: id.clone(),
            account: self.account_id.clone(),
            request,
            pending: false,
            load: None,
            control: None,
            saved: None,
            draft: None,
            error: None,
            success: false,
            transition: None,
        });
        if self.demo.is_some() {
            return self.member_management_event(
                member_management::Host::Sidebar,
                member_management::Event::Reload,
            );
        }
        let Some(directory) = self.model.people_directory.clone() else {
            self.space_settings.sidebar.as_mut().unwrap().error =
                Some("Member controls are not ready. Reload to try again.".into());
            return Task::none();
        };
        let panel = self.space_settings.sidebar.as_mut().unwrap();
        panel.pending = true;
        let (task, handle) = Task::perform(
            async move {
                let control = directory.room_management(id).await?;
                let roster = control.clone().members().await?;
                Ok((control, roster))
            },
            move |result| {
                member_management::Host::Sidebar
                    .message(member_management::Event::Ready(request, Box::new(result)))
            },
        )
        .abortable();
        panel.load = Some(handle.abort_on_drop());
        task
    }
    pub(crate) fn close_member_sidebar(&mut self) {
        self.space_settings.sidebar = None;
    }
    pub(super) fn close_space_settings(&mut self) {
        if self.space_settings.is_open() {
            self.pending_scroll = [
                Some(self.saved_scroll(false)),
                Some(self.saved_scroll(true)),
            ];
            self.space_settings.close();
        }
    }
    pub(super) fn validate_space_settings(&mut self) {
        if let Some(panel) = &mut self.space_settings.panel
            && let Some(directory) = &mut panel.directory
        {
            directory.validate_parent(self.model.spaces.iter().find(|s| s.id == panel.id));
        }
        if self.space_settings.sidebar.as_ref().is_some_and(|p| {
            self.closing
                || self.stopped
                || p.account != self.account_id
                || self.selection.members.is_none()
                || self
                    .selection
                    .room
                    .as_ref()
                    .is_none_or(|(_, id)| *id != p.id)
                || !self.model.rooms.iter().any(|r| r.id == p.id)
        }) {
            self.space_settings.sidebar = None;
        }

        if self.space_settings.panel.as_ref().is_some_and(|p| {
            self.closing
                || self.stopped
                || p.account != self.account_id
                || (self.demo.is_none() && self.model.authentication.is_some())
                || if p.exit.is_writing() {
                    false
                } else if p.exit.has_left() {
                    self.shell.space != p.scope
                } else if p.directory.is_some() {
                    self.shell.space != p.scope
                        || (p.space && !self.model.spaces.iter().any(|s| s.id == p.id))
                } else if p.space {
                    self.shell.space != Scope::Space(p.id.clone())
                        || !self.model.spaces.iter().any(|s| s.id == p.id)
                } else {
                    self.selection
                        .room
                        .as_ref()
                        .is_none_or(|(_, id)| *id != p.id)
                        || !self.model.rooms.iter().any(|r| r.id == p.id)
                }
        }) {
            self.close_space_settings();
        }
    }
    pub(super) fn space_settings_event(&mut self, event: Event) -> Task<Message> {
        if matches!(event, Event::Close)
            && let Some(panel) = &mut self.space_settings.panel
            && panel.transition.take().is_some()
        {
            return Task::none();
        }
        if matches!(
            event,
            Event::Close
                | Event::Reload
                | Event::Browse(_)
                | Event::Open(_)
                | Event::OpenMembers(_)
                | Event::Exit(space_exit::Event::Prepare(_))
        ) && self.guard_space_settings_transition(message(event.clone()))
        {
            return Task::none();
        }
        let opening_space = matches!(&event, Event::Open(_) | Event::Browse(_));
        let browse = matches!(&event, Event::Browse(_));
        match event {
            Event::KeepChanges(request) => {
                if let Some(panel) = &mut self.space_settings.panel
                    && panel
                        .transition
                        .as_ref()
                        .is_some_and(|(serial, _)| *serial == request)
                {
                    panel.transition = None;
                }
            }
            Event::DiscardChanges(request) => {
                if let Some(panel) = &mut self.space_settings.panel
                    && panel
                        .transition
                        .as_ref()
                        .is_some_and(|(serial, _)| *serial == request)
                {
                    let Some((_, next)) = panel.transition.take() else {
                        return Task::none();
                    };
                    panel.draft = panel.saved.clone();
                    panel.topic = text_editor::Content::with_text(
                        panel.saved.as_ref().map_or("", |s| s.topic.as_str()),
                    );
                    panel.avatar.reset();
                    panel.addresses = addresses::Editor::default();
                    if let Some(state) = &mut panel.directory {
                        state.discard_address();
                    }
                    return self.update(*next);
                }
            }
            Event::SidebarMembers(member_management::Event::Reload) => {
                return self.reload_member_sidebar();
            }
            Event::SidebarMembers(event) => {
                return self.member_management_event(member_management::Host::Sidebar, event);
            }
            Event::Avatar(session, event) => {
                let Some(panel) = &mut self.space_settings.panel else {
                    return Task::none();
                };
                let picker_result = matches!(event, avatar_editor::Event::Picked(..));
                if panel.request != session
                    || (!picker_result
                        && (panel.pending
                            || self.space_settings.writes.values().any(|target| {
                                target == &(panel.account.clone(), panel.id.clone())
                            })
                            || self.pending_ops >= MAX_PENDING_OPERATIONS
                            || !panel.space
                            || panel.directory.is_some()
                            || panel.exit.is_open()))
                {
                    return Task::none();
                }
                let Some(saved) = &panel.saved else {
                    return Task::none();
                };
                if !matches!(event, avatar_editor::Event::Save) {
                    return panel
                        .avatar
                        .update(event, &saved.avatar, true)
                        .map(move |event| message(Event::Avatar(session, event)));
                }
                if !saved.avatar.editable || panel.avatar.is_picking() {
                    return Task::none();
                }
                let Some(upload) = panel.avatar.edit() else {
                    return Task::none();
                };
                let baseline = saved.avatar.url.clone();
                if self.demo.is_some() {
                    self.space_settings
                        .demo
                        .insert(panel.id.clone(), saved.clone());
                }
                let id = panel.id.clone();
                let account = self.account_id.clone();
                let directory = self.model.people_directory.clone();
                if self.demo.is_none() && directory.is_none() {
                    return Task::none();
                }
                self.generation += 1;
                let operation = self.generation;
                self.space_settings
                    .avatar_writes
                    .insert(operation, (session, account.clone(), id.clone()));
                self.space_settings
                    .writes
                    .insert(operation, (account.clone(), id.clone()));
                panel.pending = true;
                panel.error = None;
                panel.success = false;
                self.pending_ops += 1;
                let completed = upload.clone();
                let demo = self.demo.is_some();
                return Task::perform(
                    async move {
                        if demo {
                            Ok(upload.map(|_| {
                                OwnedMxcUri::from(format!("mxc://demo.invalid/avatar-{operation}"))
                            }))
                        } else {
                            directory
                                .expect("ready directory")
                                .space_management(id)
                                .await?
                                .save_avatar(baseline, upload)
                                .await
                        }
                    },
                    move |result| message(Event::AvatarSaved(operation, completed.clone(), result)),
                );
            }
            Event::AvatarSaved(operation, upload, result) => {
                let Some((session, account, id)) =
                    self.space_settings.avatar_writes.remove(&operation)
                else {
                    return Task::none();
                };
                self.space_settings.writes.remove(&operation);
                self.pending_ops = self.pending_ops.saturating_sub(1);
                if account != self.account_id || self.closing || self.stopped {
                    return Task::none();
                }
                if let Ok(url) = &result {
                    if let (Some(url), Some(upload)) = (url, &upload) {
                        std::sync::Arc::make_mut(&mut self.model.avatars)
                            .insert(url.clone(), upload.preview().clone());
                    }
                    if self.demo.is_some() {
                        if let Some(settings) = self.space_settings.demo.get_mut(&id) {
                            settings.avatar.url = url.clone();
                        }
                        if let Some(space) = std::sync::Arc::make_mut(&mut self.model.spaces)
                            .iter_mut()
                            .find(|space| space.id == id)
                        {
                            space.avatar_url = url.clone();
                        }
                    }
                }
                if let Some(panel) = &mut self.space_settings.panel
                    && panel.request == session
                    && panel.id == id
                {
                    panel.pending = false;
                    match result {
                        Ok(url) => {
                            if let Some(saved) = &mut panel.saved {
                                saved.avatar.url = url.clone();
                            }
                            if let Some(draft) = &mut panel.draft {
                                draft.avatar.url = url.clone();
                            }
                            panel.avatar.saved(url, upload);
                            if self.demo.is_some()
                                && let Some(saved) = &panel.saved
                            {
                                self.space_settings.demo.insert(id.clone(), saved.clone());
                            }
                            panel.success = true;
                        }
                        Err(error) => panel.error = Some(error),
                    }
                }
                return self.reload_waiting_space_settings(&account, &id);
            }

            Event::FindRooms => {
                let id = match &self.shell.space {
                    Scope::Space(id) => id.clone(),
                    _ => String::new(),
                };
                return self.space_settings_event(Event::Browse(id));
            }
            Event::Exit(event) => return self.space_exit_event(event),
            Event::Directory(event) => return self.space_directory_event(event),
            Event::Members(event) => {
                return self.member_management_event(member_management::Host::Panel, event);
            }
            Event::Open(id) | Event::Browse(id) | Event::OpenMembers(id) => {
                let space = opening_space && !id.is_empty();
                if !self.context_menu_allowed()
                    || if space {
                        self.shell.space != Scope::Space(id.clone())
                            || !self.model.spaces.iter().any(|s| s.id == id)
                    } else if browse {
                        matches!(self.shell.space, Scope::Space(_))
                    } else {
                        self.selection
                            .room
                            .as_ref()
                            .is_none_or(|(_, room)| *room != id)
                            || !self.model.rooms.iter().any(|r| r.id == id)
                    }
                {
                    return Task::none();
                }
                self.stop_typing();
                self.emoji_completion.clear();
                self.user_completion.clear();
                self.notification_center.close();
                self.global_search.close();
                self.bookmarks.close();
                self.context_menu = None;
                self.shell.spaces.menu = None;
                self.generation += 1;
                self.space_settings.panel = Some(Panel {
                    avatar: avatar_editor::State::default(),
                    addresses: addresses::Editor::default(),
                    topic: text_editor::Content::new(),
                    space,
                    scope: self.shell.space.clone(),
                    members: member_management::State::default(),
                    directory: browse.then(|| space_directory::State::new(space)),
                    exit: space_exit::State::default(),
                    id,
                    account: self.account_id.clone(),
                    request: self.generation,
                    pending: false,
                    load: None,
                    control: None,
                    saved: None,
                    draft: None,
                    error: None,
                    success: false,
                    transition: None,
                });
                let task = self.space_settings_event(Event::Reload);
                return if browse {
                    Task::batch([task, iced::widget::operation::focus("space-room-filter")])
                } else {
                    task
                };
            }
            Event::Close => {
                self.close_space_settings();
                return iced::widget::operation::focus(if self.selection.members.is_some() {
                    "member-search"
                } else if self.selection.thread.is_some() {
                    "thread-composer"
                } else {
                    "room-composer"
                });
            }
            Event::Reload => {
                if self
                    .space_settings
                    .panel
                    .as_ref()
                    .is_some_and(|p| p.directory.is_some())
                {
                    return self.space_directory_event(space_directory::Event::Reload);
                }
                let Some(panel) = &mut self.space_settings.panel else {
                    return Task::none();
                };
                if panel.pending
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|target| target == &(panel.account.clone(), panel.id.clone()))
                {
                    return Task::none();
                }
                self.generation += 1;
                panel.request = self.generation;
                panel.avatar = avatar_editor::State::default();
                panel.addresses = addresses::Editor::default();
                panel.error = None;
                panel.success = false;
                panel.exit = space_exit::State::default();
                if self.demo.is_some() {
                    let (name, editable) = if panel.space {
                        let Some(space) = self.model.spaces.iter().find(|s| s.id == panel.id)
                        else {
                            return Task::none();
                        };
                        (space.name.clone(), space.can_manage_rooms)
                    } else {
                        let Some(room) = self.model.rooms.iter().find(|r| r.id == panel.id) else {
                            return Task::none();
                        };
                        (room.name.clone(), room.can_invite)
                    };
                    let settings =
                        self.space_settings
                            .demo
                            .get(&panel.id)
                            .cloned()
                            .unwrap_or(Settings {
                                addresses: Ok(Addresses::demo(editable)),
                                avatar: Details {
                                    url: self
                                        .model
                                        .spaces
                                        .iter()
                                        .find(|space| space.id == panel.id)
                                        .and_then(|space| space.avatar_url.clone()),
                                    editable,
                                },
                                name,
                                topic: String::new(),
                                access: Some(JoinRule::Invite),
                                editable: [editable; 3],
                            });
                    panel.saved = Some(settings.clone());
                    panel.topic = text_editor::Content::with_text(&settings.topic);
                    panel.draft = Some(settings);
                    return self.member_management_event(
                        member_management::Host::Panel,
                        member_management::Event::Reload,
                    );
                } else if let Some(directory) = self.model.people_directory.clone() {
                    panel.pending = true;
                    let id = panel.id.clone();
                    let request = panel.request;
                    let space = panel.space;
                    let (task, handle) = Task::perform(
                        async move {
                            let control = if space {
                                directory.space_management(id).await?
                            } else {
                                directory.room_management(id).await?
                            };
                            let settings = control.clone().settings().await?;
                            Ok((control, settings))
                        },
                        move |result| message(Event::Loaded(request, Box::new(result))),
                    )
                    .abortable();
                    panel.load = Some(handle.abort_on_drop());
                    return task;
                } else {
                    panel.error = Some("The account is not ready. Try Reload.".into());
                }
            }
            Event::Loaded(request, result) => {
                if let Some(panel) = &mut self.space_settings.panel
                    && panel.request == request
                {
                    panel.pending = false;
                    panel.load = None;
                    match *result {
                        Ok((control, settings)) => {
                            panel.control = Some(control);
                            panel.saved = Some(settings.clone());
                            panel.topic = text_editor::Content::with_text(&settings.topic);
                            panel.draft = Some(settings);
                            return self.member_management_event(
                                member_management::Host::Panel,
                                member_management::Event::Reload,
                            );
                        }
                        Err(error) => {
                            panel.error = Some(error);
                            panel.saved = None;
                            panel.draft = None;
                        }
                    }
                }
            }
            Event::Save(field) => {
                let Some(panel) = &mut self.space_settings.panel else {
                    return Task::none();
                };
                if panel.avatar.is_picking()
                    || panel.pending
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|target| target == &(panel.account.clone(), panel.id.clone()))
                {
                    return Task::none();
                }
                if !panel.space {
                    return Task::none();
                }
                let (Some(saved), Some(draft)) = (&panel.saved, &panel.draft) else {
                    return Task::none();
                };
                let change = match field {
                    0 if saved.editable[0] && saved.name != draft.name => {
                        Change::Name(saved.name.clone(), draft.name.clone())
                    }
                    1 if saved.editable[1] && saved.topic != draft.topic => {
                        Change::Topic(saved.topic.clone(), draft.topic.clone())
                    }
                    2 if saved.editable[2] && saved.access != draft.access => Change::Access(
                        saved.access.clone(),
                        draft.access.clone().expect("selected access"),
                    ),
                    _ => return Task::none(),
                };
                return self.space_settings_event(Event::Commit(change));
            }
            Event::Addresses(event) => {
                let Some(panel) = &mut self.space_settings.panel else {
                    return Task::none();
                };
                if !panel.space
                    || panel.directory.is_some()
                    || panel.exit.is_open()
                    || panel.pending
                    || panel.avatar.is_picking()
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|target| target == &(panel.account.clone(), panel.id.clone()))
                {
                    return Task::none();
                }
                let Some(Ok(saved)) = panel.saved.as_ref().map(|s| &s.addresses) else {
                    return Task::none();
                };
                panel.error = None;
                panel.success = false;
                match panel.addresses.update(saved, event) {
                    Ok(Some(change)) => {
                        return self.space_settings_event(Event::Commit(Change::Address(change)));
                    }
                    Err(error) => panel.error = Some(error),
                    Ok(None) => {}
                }
            }
            Event::Commit(change) => {
                let Some(panel) = &mut self.space_settings.panel else {
                    return Task::none();
                };
                if !panel.space
                    || panel.directory.is_some()
                    || panel.exit.is_open()
                    || panel.pending
                    || panel.avatar.is_picking()
                    || panel.saved.is_none()
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|target| target == &(panel.account.clone(), panel.id.clone()))
                {
                    return Task::none();
                }
                if matches!(&change, Change::Name(_, value) if value.trim().is_empty() || value.len() > 255)
                {
                    panel.error = Some("Enter a name within 255 bytes.".into());
                    return Task::none();
                }
                panel.error = None;
                panel.success = false;
                if self.demo.is_some() {
                    if let Change::Address(AddressChange::Published(_, after)) = &change
                        && panel
                            .saved
                            .as_ref()
                            .and_then(|s| s.addresses.as_ref().ok())
                            .is_some_and(|saved| {
                                after.all().iter().any(|a| {
                                    !saved.local.as_ref().is_ok_and(|local| local.contains(a))
                                })
                            })
                    {
                        panel.error = Some(
                            "Add this address locally before publishing it in the demo.".into(),
                        );
                        return Task::none();
                    }
                    panel.saved.as_mut().unwrap().apply(&change);
                    if let Change::Address(address) = &change {
                        panel.addresses.saved(address);
                        if let Some(draft) = &mut panel.draft {
                            draft.apply(&change);
                        }
                    }
                    self.space_settings
                        .demo
                        .insert(panel.id.clone(), panel.saved.clone().unwrap());
                    if let Some(space) = std::sync::Arc::make_mut(&mut self.model.spaces)
                        .iter_mut()
                        .find(|s| s.id == panel.id)
                    {
                        space.name = panel.saved.as_ref().unwrap().name.clone();
                    }
                    panel.success = true;
                } else if let Some(control) = panel.control.clone() {
                    panel.pending = true;
                    self.pending_ops += 1;
                    let request = panel.request;
                    self.space_settings
                        .writes
                        .insert(request, (panel.account.clone(), panel.id.clone()));
                    let completed = change.clone();
                    return Task::perform(control.save(change), move |result| {
                        message(Event::Saved(request, completed.clone(), result))
                    });
                }
            }
            Event::Saved(request, change, result) => {
                let Some((account, id)) = self.space_settings.writes.remove(&request) else {
                    return Task::none();
                };
                self.pending_ops = self.pending_ops.saturating_sub(1);
                if let Some(panel) = &mut self.space_settings.panel
                    && panel.request == request
                {
                    panel.pending = false;
                    match result {
                        Ok(()) => {
                            if let Some(saved) = &mut panel.saved {
                                saved.apply(&change);
                            }
                            if let Change::Address(address) = &change {
                                panel.addresses.saved(address);
                                if let Some(draft) = &mut panel.draft {
                                    draft.apply(&change);
                                }
                            }
                            panel.success = true;
                        }
                        Err(error) => panel.error = Some(error),
                    }
                }
                return self.reload_waiting_space_settings(&account, &id);
            }
            edit => {
                if let Some(panel) = &mut self.space_settings.panel
                    && !panel.pending
                    && let Some(draft) = &mut panel.draft
                {
                    match edit {
                        Event::Name(value) if draft.editable[0] => draft.name = value,
                        #[cfg(test)]
                        Event::Topic(value) if draft.editable[1] => {
                            panel.topic = text_editor::Content::with_text(&value);
                            draft.topic = value;
                        }
                        Event::TopicEdit(action) if draft.editable[1] => {
                            panel.topic.perform(action);
                            draft.topic = panel.topic.text();
                        }
                        Event::Access(value)
                            if draft.editable[2]
                                && matches!(
                                    draft.access,
                                    Some(JoinRule::Invite | JoinRule::Public)
                                )
                                && matches!(value, JoinRule::Invite | JoinRule::Public) =>
                        {
                            draft.access = Some(value)
                        }
                        _ => {}
                    }
                    panel.success = false;
                }
            }
        }
        Task::none()
    }
    pub(super) fn space_settings_view(&self) -> Element<'_, Message> {
        let panel = self.space_settings.panel.as_ref().unwrap();
        if let Some((serial, _)) = &panel.transition {
            return super::focus_scope::dismissible(
                container(super::room_settings::discard_prompt(
                    message(Event::KeepChanges(*serial)),
                    message(Event::DiscardChanges(*serial)),
                ))
                .padding(24)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
                "space-settings-discard",
                Message::Escape,
            );
        }
        if panel.directory.is_some() {
            return self.space_directory_view();
        }
        if panel.exit.is_open() {
            return self.space_exit_view();
        }
        let header = container(
            row![
                shell::title(
                    if panel.space {
                        "Space settings"
                    } else {
                        "Room members"
                    },
                    20
                ),
                iced::widget::space().width(Length::Fill),
                if panel.space {
                    Element::from(icon_button(
                        Kind::Compass,
                        "Find rooms to join",
                        Some(message(Event::Browse(panel.id.clone()))),
                    ))
                } else {
                    Element::from(iced::widget::space().width(0))
                },
                icon_button(
                    Kind::Close,
                    if panel.space {
                        "Close space settings"
                    } else {
                        "Close room members"
                    },
                    Some(message(Event::Close))
                ),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        )
        .padding([18, 24])
        .width(Length::Fill);
        let mut body = column![
            text(&panel.id)
                .size(11)
                .style(shell::muted_text)
                .wrapping(text::Wrapping::Glyph)
        ]
        .spacing(16);
        if panel.space
            && let (Some(saved), Some(draft)) = (&panel.saved, &panel.draft)
        {
            body = body.push(
                panel
                    .avatar
                    .view(
                        &saved.avatar,
                        &self.model.avatars,
                        !panel.pending && self.pending_ops < MAX_PENDING_OPERATIONS,
                    )
                    .map(move |event| message(Event::Avatar(panel.request, event))),
            );
            let editable_name = saved.editable[0] && !panel.pending;
            let name = text_input("Space name", &draft.name)
                .id("space-settings-name")
                .cursor_blinking(false)
                .padding([7, 10])
                .size(13)
                .style(shell::search_style)
                .on_input_maybe(editable_name.then_some(|value| message(Event::Name(value))));
            body = body.push(super::room_settings::setting_field(
                "Name",
                name,
                message(Event::Save(0)),
                editable_name && draft.name != saved.name,
            ));
            let mut topic = text_editor(&panel.topic)
                .id("space-settings-topic")
                .cursor_blinking(false)
                .placeholder("Space topic")
                .height(84)
                .size(13)
                .style(shell::editor_style);
            if saved.editable[1] && !panel.pending {
                topic = topic.on_action(|action| message(Event::TopicEdit(action)));
            }
            body = body.push(super::room_settings::setting_field(
                "Topic",
                topic,
                message(Event::Save(1)),
                saved.editable[1] && !panel.pending && draft.topic != saved.topic,
            ));
            let supported = matches!(draft.access, Some(JoinRule::Invite | JoinRule::Public));
            let access: Element<'_, Message> = if supported && draft.editable[2] && !panel.pending {
                pick_list(
                    vec![Access::Private, Access::Public],
                    draft.access.as_ref().map(|r| {
                        if *r == JoinRule::Public {
                            Access::Public
                        } else {
                            Access::Private
                        }
                    }),
                    |v| {
                        message(Event::Access(match v {
                            Access::Private => JoinRule::Invite,
                            Access::Public => JoinRule::Public,
                        }))
                    },
                )
                .style(shell::select_style)
                .keyboard_navigation(true)
                .padding([7, 10])
                .text_size(13)
                .width(Length::Fill)
                .into()
            } else {
                text(
                    draft
                        .access
                        .as_ref()
                        .map_or("Unknown".into(), |rule| format!("{rule:?}")),
                )
                .size(14)
                .into()
            };
            body = body.push(super::room_settings::setting_field(
                "Who can join",
                access,
                message(Event::Save(2)),
                supported && draft.editable[2] && !panel.pending && draft.access != saved.access,
            ));
            if !supported {
                body = body.push(
                    text("This access rule is read-only.")
                        .size(12)
                        .style(shell::muted_text),
                );
            }
            body = body.push(match &saved.addresses {
                Ok(saved) => panel.addresses.view_for(
                    saved,
                    !panel.pending
                        && !panel.avatar.is_picking()
                        && self.pending_ops < MAX_PENDING_OPERATIONS,
                    true,
                    |event| message(Event::Addresses(event)),
                ),
                Err(error) => text(error).size(13).into(),
            });
            body = body.push(
                text("Space membership does not grant membership in its rooms or subspaces.")
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        body = body.push(self.member_management_view());
        if panel.space && panel.saved.is_some() {
            body = body.push(self.space_exit_actions());
        }
        let mut feedback = column![].spacing(6);
        if panel.pending {
            feedback = feedback.push(text("Working…").size(13));
        }
        if panel.success {
            feedback = feedback.push(text("Change saved.").size(13).style(shell::muted_text));
        }
        if let Some(error) = &panel.error {
            feedback = feedback.push(text(error).size(13).style(shell::danger_text));
        }
        let footer = container(
            column![
                feedback,
                row![
                    super::tooltips::icon_button(
                        super::icons::Kind::Refresh,
                        "Reload space settings",
                        (!panel.pending).then_some(message(Event::Reload)),
                    ),
                    iced::widget::space().width(Length::Fill),
                    text(if panel.dirty() {
                        "Unsaved changes · save each edited field"
                    } else {
                        "Changes save individually"
                    })
                    .size(12)
                    .style(shell::muted_text)
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center)
            ]
            .spacing(6),
        )
        .padding([14, 24]);
        super::focus_scope::dismissible(
            column![
                header,
                rule::horizontal(1),
                scrollable(container(body).padding(24).max_width(800))
                    .id("space-settings-body")
                    .height(Length::Fill)
                    .direction(shell::scrollbar())
                    .style(shell::scroll_style),
                rule::horizontal(1),
                footer,
            ]
            .height(Length::Fill)
            .width(Length::Fill),
            "space-settings-scope",
            Message::Escape,
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Private,
    Public,
}
impl std::fmt::Display for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Private => "Private · invitation only",
            Self::Public => "Public · anyone can join",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::demo_app;
    use super::*;
    fn open(app: &mut App) -> u64 {
        let _ = app.update(message(Event::Open("studio-space".into())));
        app.space_settings.panel.as_ref().unwrap().request
    }
    fn discard_serial(app: &App) -> u64 {
        app.space_settings
            .panel
            .as_ref()
            .unwrap()
            .transition
            .as_ref()
            .unwrap()
            .0
    }
    #[test]
    fn space_aliases_save_independently_and_preserve_conversation_and_other_edits() {
        use addresses::Event as Edit;
        let mut app = demo_app();
        let selection = app.selection.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 155.0);
        open(&mut app);
        let _ = app.update(message(Event::Name("Unsaved name".into())));
        let alias = matrix_sdk::ruma::owned_room_alias_id!("#friendly:demo.invalid");
        for edit in [
            Edit::Local("friendly".into()),
            Edit::AddLocal,
            Edit::Published("friendly".into()),
            Edit::Publish,
            Edit::Main(Some(alias.clone())),
            Edit::Directory(true),
        ] {
            let _ = app.update(message(Event::Addresses(edit)));
        }
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert_eq!(panel.draft.as_ref().unwrap().name, "Unsaved name");
        assert_ne!(panel.saved.as_ref().unwrap().name, "Unsaved name");
        let saved = panel.saved.as_ref().unwrap().addresses.as_ref().unwrap();
        assert_eq!(saved.published.main, Some(alias.clone()));
        assert_eq!(saved.local, Ok(vec![alias.clone()]));
        assert_eq!(saved.directory, Ok(true));
        let _ = app.update(message(Event::Addresses(Edit::Remove(alias.clone()))));
        let _ = app.update(message(Event::Addresses(Edit::ConfirmRemove)));
        assert_eq!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .saved
                .as_ref()
                .unwrap()
                .addresses
                .as_ref()
                .unwrap()
                .local,
            Ok(vec![alias.clone()])
        );
        let _ = app.update(message(Event::Addresses(Edit::Unpublish(alias.clone()))));
        let _ = app.update(message(Event::Addresses(Edit::Remove(alias.clone()))));
        let _ = app.update(message(Event::Addresses(Edit::CancelRemove)));
        let _ = app.update(message(Event::Addresses(Edit::Remove(alias))));
        let _ = app.update(message(Event::Addresses(Edit::ConfirmRemove)));
        let _ = app.update(message(Event::Close));
        let _ = app.update(message(Event::DiscardChanges(discard_serial(&app))));
        open(&mut app);
        let panel = app.space_settings.panel.as_ref().unwrap();
        let saved = panel.saved.as_ref().unwrap().addresses.as_ref().unwrap();
        assert!(saved.published.all().is_empty());
        assert_eq!(saved.local, Ok(vec![]));
        assert_eq!(saved.directory, Ok(true));
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert_eq!(app.drafts[&key].content.text(), "unsent");
        assert_eq!(app.scroll_positions[&key], 155.0);
    }

    #[test]
    fn reopening_a_space_form_waits_for_old_metadata_then_loads_once() {
        let mut app = demo_app();
        let request = open(&mut app);
        let id = app.space_settings.panel.as_ref().unwrap().id.clone();
        let account = app.account_id.clone();
        app.space_settings.panel.as_mut().unwrap().pending = true;
        app.space_settings
            .writes
            .insert(request, (account.clone(), id.clone()));
        app.pending_ops = 1;
        let _ = app.update(message(Event::Close));
        let reopened = open(&mut app);
        assert!(app.space_settings.panel.as_ref().unwrap().saved.is_none());
        let _ = app.update(message(Event::Saved(
            request,
            Change::Topic(String::new(), "Updated".into()),
            Ok(()),
        )));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert_eq!(panel.id, id);
        assert_eq!(panel.account, account);
        assert!(panel.saved.is_some());
        assert!(panel.request > reopened);
        assert_eq!(app.pending_ops, 0);
        let current = panel.request;
        let _ = app.update(message(Event::Name("Keep the loaded draft".into())));
        let _ = app.update(message(Event::Saved(
            request,
            Change::Topic(String::new(), "Updated".into()),
            Ok(()),
        )));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert_eq!(panel.request, current);
        assert_eq!(panel.draft.as_ref().unwrap().name, "Keep the loaded draft");
    }

    #[tokio::test]
    async fn reopening_a_space_form_resumes_after_an_old_avatar_write() {
        let mut app = demo_app();
        let session = open(&mut app);
        let upload = avatar_editor::test_upload().await;
        let _ = app.update(message(Event::Avatar(session, avatar_editor::Event::Pick)));
        let _ = app.update(message(Event::Avatar(
            session,
            avatar_editor::Event::Picked(1, Ok(Some(upload.clone()))),
        )));
        let _ = app.update(message(Event::Avatar(session, avatar_editor::Event::Save)));
        let operation = app.generation;
        let _ = app.update(message(Event::Close));
        let _ = app.update(message(Event::DiscardChanges(discard_serial(&app))));
        open(&mut app);
        assert!(app.space_settings.panel.as_ref().unwrap().saved.is_none());
        let url: OwnedMxcUri = "mxc://demo.invalid/old-space-avatar".into();
        let _ = app.update(message(Event::AvatarSaved(
            operation,
            Some(upload),
            Ok(Some(url.clone())),
        )));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert_eq!(panel.saved.as_ref().unwrap().avatar.url, Some(url));
        assert_eq!(app.pending_ops, 0);
    }

    #[test]
    fn an_old_confirmation_cannot_accept_a_new_prompt_in_the_same_panel() {
        let mut app = demo_app();
        open(&mut app);
        let _ = app.update(message(Event::Name("Keep both edits".into())));
        let _ = app.update(message(Event::Close));
        let old = discard_serial(&app);
        let _ = app.update(message(Event::KeepChanges(old)));
        let _ = app.update(message(Event::Close));
        let current = discard_serial(&app);
        assert_ne!(old, current);
        let _ = app.update(message(Event::DiscardChanges(old)));
        let _ = app.update(message(Event::KeepChanges(old)));
        assert_eq!(discard_serial(&app), current);
        assert_eq!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .name,
            "Keep both edits"
        );
        let _ = app.update(message(Event::DiscardChanges(current)));
        assert!(!app.space_settings.is_open());
    }

    #[test]
    fn saving_a_topic_is_independent_of_an_invalid_unsaved_name() {
        let mut app = demo_app();
        open(&mut app);
        let _ = app.update(message(Event::Name(String::new())));
        let _ = app.update(message(Event::Topic("A useful topic".into())));
        let _ = app.update(message(Event::Save(1)));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert_eq!(panel.saved.as_ref().unwrap().topic, "A useful topic");
        assert!(panel.draft.as_ref().unwrap().name.is_empty());
        assert!(panel.error.is_none());
        let _ = app.update(message(Event::Save(0)));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert!(panel.error.is_some());
        assert!(!panel.saved.as_ref().unwrap().name.is_empty());
    }

    #[test]
    fn stale_or_unrequested_discard_cannot_drop_a_space_draft() {
        let mut app = demo_app();
        let request = open(&mut app);
        let _ = app.update(message(Event::Name("Keep this draft".into())));
        let _ = app.update(message(Event::DiscardChanges(request)));
        assert_eq!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .name,
            "Keep this draft"
        );
        let _ = app.update(message(Event::Close));
        let _ = app.update(message(Event::DiscardChanges(request.wrapping_add(1000))));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .transition
                .is_some()
        );
        let _ = app.update(message(Event::Close));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .transition
                .is_none()
        );
    }

    #[test]
    fn multiline_space_topic_stays_separate_from_an_unsaved_name() {
        let mut app = demo_app();
        open(&mut app);
        let _ = app.update(message(Event::Topic("First line\nSecond line".into())));
        let _ = app.update(message(Event::Name("Still editing the name".into())));
        let _ = app.update(message(Event::Save(1)));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert_eq!(
            panel.saved.as_ref().unwrap().topic,
            "First line\nSecond line"
        );
        assert_eq!(panel.topic.text(), "First line\nSecond line");
        assert!(panel.dirty());
        assert_ne!(panel.saved.as_ref().unwrap().name, "Still editing the name");
    }

    #[test]
    fn dirty_space_settings_preserve_edits_until_discard_is_confirmed() {
        let mut app = demo_app();
        open(&mut app);
        let _ = app.update(message(Event::Topic("Uncommitted space topic".into())));
        let _ = app.update(message(Event::Close));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert!(panel.transition.is_some());
        assert_eq!(
            panel.draft.as_ref().unwrap().topic,
            "Uncommitted space topic"
        );
        let _ = app.update(message(Event::KeepChanges(discard_serial(&app))));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .transition
                .is_none()
        );
        let _ = app.update(message(Event::Reload));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .transition
                .is_some()
        );
        let _ = app.update(message(Event::DiscardChanges(discard_serial(&app))));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert!(panel.transition.is_none());
        assert_eq!(
            panel.draft.as_ref().unwrap().topic,
            panel.saved.as_ref().unwrap().topic
        );
        let _ = app.update(message(Event::Name("Another unsaved edit".into())));
        let _ = app.update(message(Event::Exit(space_exit::Event::Prepare(
            interconnect::space_exit::Action::Leave,
        ))));
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert!(panel.transition.is_some());
        assert!(!panel.exit.is_open());
    }

    #[tokio::test]
    async fn avatar_drafts_save_separately_and_late_results_release_once() {
        let mut app = demo_app();
        let session = open(&mut app);
        let id = "studio-space".to_owned();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 145.0);
        let upload = avatar_editor::test_upload().await;
        let _ = app.update(message(Event::Name("Uncommitted metadata".into())));
        let _ = app.update(message(Event::Avatar(session, avatar_editor::Event::Pick)));
        let _ = app.update(message(Event::Avatar(
            session,
            avatar_editor::Event::Picked(1, Ok(Some(upload.clone()))),
        )));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .saved
                .as_ref()
                .unwrap()
                .avatar
                .url
                .is_none()
        );
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Avatar(session, avatar_editor::Event::Save)));
        let operation = app.generation;
        assert_eq!(app.pending_ops, 1);
        let _ = app.update(message(Event::Close));
        let _ = app.update(message(Event::DiscardChanges(discard_serial(&app))));
        let url: OwnedMxcUri = "mxc://demo.invalid/saved-avatar".into();
        let _ = app.update(message(Event::AvatarSaved(
            operation,
            Some(upload.clone()),
            Ok(Some(url.clone())),
        )));
        let _ = app.update(message(Event::AvatarSaved(
            operation,
            Some(upload),
            Ok(Some(url.clone())),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(!app.space_settings.writes.contains_key(&operation));
        assert_eq!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .avatar_url,
            Some(url.clone())
        );
        open(&mut app);
        let panel = app.space_settings.panel.as_ref().unwrap();
        assert_eq!(panel.saved.as_ref().unwrap().avatar.url, Some(url));
        assert_ne!(panel.saved.as_ref().unwrap().name, "Uncommitted metadata");
        let session = panel.request;
        let _ = app.update(message(Event::Avatar(
            session.wrapping_sub(1),
            avatar_editor::Event::Remove,
        )));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .avatar
                .edit()
                .is_none()
        );
        let _ = app.update(message(Event::Avatar(
            session,
            avatar_editor::Event::Remove,
        )));
        let _ = app.update(message(Event::Avatar(session, avatar_editor::Event::Save)));
        let operation = app.generation;
        let _ = app.update(message(Event::AvatarSaved(operation, None, Ok(None))));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .saved
                .as_ref()
                .unwrap()
                .avatar
                .url
                .is_none()
        );
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.drafts[&key].content.text(), "unsent");
        assert_eq!(app.scroll_positions[&key], 145.0);
    }
    #[test]
    fn space_settings_preserve_conversation_draft_scroll_and_suppress_hidden_actions() {
        let mut app = demo_app();
        let selection = app.selection.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("draft");
        app.scroll_positions.insert(key.clone(), 150.0);
        open(&mut app);
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert!(!app.selection.conversation_visible);
        let _ = app.update(Message::Send(key.clone()));
        assert_eq!(app.drafts[&key].content.text(), "draft");
        let _ = app.update(message(Event::Name("Renamed".into())));
        let _ = app.update(message(Event::Save(0)));
        assert_eq!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "studio-space")
                .unwrap()
                .name,
            "Renamed"
        );
        let _ = app.update(message(Event::Close));
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.scroll_positions[&key], 150.0);
        assert_eq!(app.drafts[&key].content.text(), "draft");
    }
    #[test]
    fn navigation_accounts_and_stale_completions_cannot_replace_settings() {
        let mut app = demo_app();
        let old = open(&mut app);
        let _ = app.update(message(Event::Close));
        let request = open(&mut app);
        let _ = app.update(message(Event::Loaded(old, Box::new(Err("stale".into())))));
        assert_eq!(app.space_settings.panel.as_ref().unwrap().request, request);
        assert!(app.space_settings.panel.as_ref().unwrap().error.is_none());
        let selection = app.selection.room.clone();
        let _ = app.update(Message::Space(Scope::Home));
        assert!(!app.space_settings.is_open());
        assert_eq!(app.selection.room, selection);
        let _ = app.update(Message::Space(Scope::Space("studio-space".into())));
        open(&mut app);
        app.account_id = Some("@other:example.org".into());
        app.validate_space_settings();
        assert!(!app.space_settings.is_open());
    }
    #[test]
    fn writes_release_shutdown_bookkeeping_once_after_dismissal() {
        let mut app = demo_app();
        let request = open(&mut app);
        app.space_settings
            .writes
            .insert(request, (app.account_id.clone(), "studio-space".into()));
        app.pending_ops = 1;
        let _ = app.update(message(Event::Close));
        let change = Change::Topic("".into(), "Updated".into());
        let _ = app.update(message(Event::Saved(request, change.clone(), Ok(()))));
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(Event::Saved(request, change, Ok(()))));
        assert_eq!(app.pending_ops, 0);
        assert!(!app.space_settings.is_open());
    }
    #[test]
    fn full_size_panel_lays_out_at_narrow_and_wide_widths() {
        use iced::advanced::{layout, widget};
        let mut app = demo_app();
        open(&mut app);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for width in [640.0, 1280.0] {
            let size = iced::Size::new(width, 720.0);
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert_eq!(node.size(), size);
        }
    }
}
