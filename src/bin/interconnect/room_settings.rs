//! Room-bound settings with bounded asynchronous loads and serialized writes.
#[path = "room_settings/addresses.rs"]
mod addresses;
use super::avatar_editor;
use super::{App, MAX_PENDING_OPERATIONS, Message, current_pane, shell};
use iced::{
    Element, Length, Task,
    widget::{
        button, column, container, mouse_area, opaque, pick_list, row, scrollable, text, text_input,
    },
};
use interconnect::connected::room_settings::{
    Change, Control, Notifications, Settings,
    addresses::{Addresses, Change as AddressChange},
};
use interconnect::room_avatar::{Details, Upload};
use matrix_sdk::ruma::OwnedMxcUri;
use matrix_sdk::ruma::events::room::{history_visibility::HistoryVisibility, join_rules::JoinRule};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct State {
    panel: Option<Panel>,
    control: Option<Control>,
    busy: HashSet<String>,
    demo: HashMap<String, Settings>,
    avatar_writes: HashMap<u64, (u64, Option<String>, String)>,
}
impl State {
    pub fn dismiss(&mut self) {
        self.panel = None;
        self.control = None;
    }
    pub fn is_open(&self) -> bool {
        self.panel.is_some()
    }
}
struct Panel {
    avatar: avatar_editor::State,
    addresses: addresses::Editor,
    target: (u64, String),
    request: u64,
    pending: bool,
    load: Option<iced::task::Handle>,
    saved: Option<Settings>,
    draft: Option<Settings>,
    error: Option<String>,
    success: bool,
    transition: Option<Box<Message>>,
}
impl Panel {
    fn dirty(&self) -> bool {
        self.avatar.edit().is_some()
            || self.avatar.is_picking()
            || self.addresses.dirty()
            || self
                .saved
                .as_ref()
                .zip(self.draft.as_ref())
                .is_some_and(|(saved, draft)| {
                    saved.name != draft.name
                        || saved.topic != draft.topic
                        || saved.notifications != draft.notifications
                        || saved.access != draft.access
                        || saved.history != draft.history
                })
    }
}

pub(super) fn discard_prompt(keep: Message, discard: Message) -> Element<'static, Message> {
    container(column![
        shell::title("Discard unsaved changes?", 20),
        text("Some edits have not been saved. Keep editing to save them individually. Changes already being saved will continue.").size(13),
        row![button(text("Keep editing").size(13)).padding([7, 12]).style(shell::primary_button).on_press(keep),
            button(text("Discard changes").size(13)).padding([7, 12]).style(button::danger).on_press(discard)].spacing(10).wrap(),
    ].spacing(16))
        .padding(24).max_width(460)
        .style(|theme| container::Style { border: iced::Border { color: shell::palette(theme).line, width: 1.0, radius: 12.0.into() }, ..shell::surface(theme, shell::palette(theme).background) }).into()
}

#[derive(Clone, Debug)]
pub enum Event {
    Addresses(addresses::Event),
    Commit(Change),
    Avatar(u64, avatar_editor::Event),
    AvatarSaved(u64, Option<Upload>, Result<Option<OwnedMxcUri>, String>),
    Open(String),
    Close,
    KeepChanges,
    DiscardChanges,
    Members,
    Leave,
    Reload,
    Ignore,
    Focus(bool),
    Name(String),
    Topic(String),
    Notifications(Notifications),
    Access(JoinRule),
    History(HistoryVisibility),
    Save(usize),
    Loaded((u64, String), u64, Box<Result<Settings, String>>),
    Saved((u64, String), u64, Change, Result<(), String>),
}
fn message(event: Event) -> Message {
    Message::RoomSettings(event)
}
fn apply(settings: &mut Settings, change: &Change) {
    match change {
        Change::Address(change) => {
            if let Ok(addresses) = &mut settings.addresses {
                addresses.apply(change);
            }
        }
        Change::Name(_, value) => settings.name = value.clone(),
        Change::Topic(_, value) => settings.topic = value.clone(),
        Change::Notifications(_, value) => settings.notifications = *value,
        Change::Access(_, value) => settings.access = Some(value.clone()),
        Change::History(_, value) => settings.history = value.clone(),
    }
}
impl App {
    /// Capture a user-requested transition before it can discard a form draft.
    pub(super) fn guard_room_settings_transition(&mut self, next: Message) -> bool {
        let Some(panel) = &mut self.room_settings.panel else {
            return false;
        };
        if !panel.dirty() {
            return false;
        }
        panel.transition.get_or_insert_with(|| Box::new(next));
        true
    }
    pub(super) fn room_settings_avatar(&self) -> Option<OwnedMxcUri> {
        self.room_settings
            .panel
            .as_ref()?
            .saved
            .as_ref()?
            .avatar
            .url
            .clone()
    }

    pub(super) fn validate_room_settings(&mut self) {
        if self.room_settings.panel.as_ref().is_some_and(|p| {
            self.closing
                || self.stopped
                || self.selection.room.as_ref() != Some(&p.target)
                || !self.model.rooms.iter().any(|r| r.id == p.target.1)
        }) {
            self.room_settings.panel = None;
        }
    }
    pub(super) fn room_settings_event(&mut self, event: Event) -> Task<Message> {
        if matches!(
            event,
            Event::Close | Event::Reload | Event::Members | Event::Leave
        ) && self.guard_room_settings_transition(message(event.clone()))
        {
            return Task::none();
        }
        match event {
            Event::KeepChanges => {
                if let Some(panel) = &mut self.room_settings.panel {
                    panel.transition = None;
                }
            }
            Event::DiscardChanges => {
                if let Some(panel) = &mut self.room_settings.panel {
                    let next = panel.transition.take();
                    panel.draft = panel.saved.clone();
                    panel.avatar = avatar_editor::State::default();
                    panel.addresses = addresses::Editor::default();
                    if let Some(next) = next {
                        return self.update(*next);
                    }
                }
            }
            Event::Avatar(session, event) => {
                let Some(panel) = &mut self.room_settings.panel else {
                    return Task::none();
                };
                let picker_result = matches!(event, avatar_editor::Event::Picked(..));
                if panel.request != session
                    || (!picker_result
                        && (panel.pending
                            || self.room_settings.busy.contains(&panel.target.1)
                            || self.pending_ops >= MAX_PENDING_OPERATIONS))
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
                    self.room_settings
                        .demo
                        .insert(panel.target.1.clone(), saved.clone());
                }
                let id = panel.target.1.clone();
                let account = self.account_id.clone();
                let directory = self.model.people_directory.clone();
                if self.demo.is_none() && directory.is_none() {
                    return Task::none();
                }
                self.generation += 1;
                let operation = self.generation;
                self.room_settings
                    .avatar_writes
                    .insert(operation, (session, account.clone(), id.clone()));
                self.room_settings.busy.insert(id.clone());
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
                                .room_management(id)
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
                    self.room_settings.avatar_writes.remove(&operation)
                else {
                    return Task::none();
                };
                self.room_settings.busy.remove(&id);
                self.pending_ops = self.pending_ops.saturating_sub(1);
                if account != self.account_id || self.closing || self.stopped {
                    return Task::none();
                }
                if let Ok(url) = &result {
                    if let (Some(url), Some(upload)) = (url, &upload) {
                        std::sync::Arc::make_mut(&mut self.model.avatars)
                            .insert(url.clone(), upload.preview().clone());
                    }
                    if self.demo.is_some()
                        && let Some(settings) = self.room_settings.demo.get_mut(&id)
                    {
                        settings.avatar.url = url.clone();
                    }
                }
                if let Some(panel) = &mut self.room_settings.panel
                    && panel.request == session
                    && panel.target.1 == id
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
                                self.room_settings.demo.insert(id, saved.clone());
                            }
                            panel.success = true;
                        }
                        Err(error) => panel.error = Some(error),
                    }
                }
            }

            Event::Open(id) => {
                if !self.context_menu_allowed()
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self
                        .selection
                        .room
                        .as_ref()
                        .is_none_or(|(_, room)| *room != id)
                {
                    return Task::none();
                }
                self.generation += 1;
                self.room_settings.panel = Some(Panel {
                    avatar: avatar_editor::State::default(),
                    addresses: addresses::Editor::default(),
                    target: self.selection.room.clone().unwrap(),
                    request: self.generation,
                    pending: false,
                    load: None,
                    saved: None,
                    draft: None,
                    error: None,
                    success: false,
                    transition: None,
                });
                self.context_menu = None;
                self.stop_typing();
                self.emoji_completion.clear();
                self.user_completion.clear();
                return self.room_settings_event(Event::Reload);
            }
            Event::Close => {
                self.room_settings.panel = None;
                return iced::widget::operation::focus("room-composer");
            }
            Event::Members | Event::Leave => {
                let Some(panel) = &self.room_settings.panel else {
                    return Task::none();
                };
                if panel.pending || self.room_settings.busy.contains(&panel.target.1) {
                    return Task::none();
                }
                let room = panel.target.1.clone();
                self.room_settings.panel = None;
                return self.update(if matches!(event, Event::Members) {
                    Message::SpaceSettings(super::space_settings::Event::OpenMembers(room))
                } else {
                    Message::Membership(super::membership::Event::Leave(room))
                });
            }
            Event::Reload => {
                let Some(panel) = &mut self.room_settings.panel else {
                    return Task::none();
                };
                if panel.pending
                    || self.room_settings.busy.contains(&panel.target.1)
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                {
                    return Task::none();
                }
                self.generation += 1;
                panel.request = self.generation;
                panel.avatar = avatar_editor::State::default();
                panel.addresses = addresses::Editor::default();
                panel.error = None;
                panel.success = false;
                if self.demo.is_some() {
                    let room = self
                        .model
                        .rooms
                        .iter()
                        .find(|r| r.id == panel.target.1)
                        .unwrap();
                    let mut settings =
                        self.room_settings
                            .demo
                            .get(&room.id)
                            .cloned()
                            .unwrap_or(Settings {
                                addresses: Ok(Addresses::demo(room.can_edit_topic)),
                                avatar: Details {
                                    url: None,
                                    editable: room.can_edit_topic,
                                },
                                control: None,
                                name: room.name.clone(),
                                topic: room.topic.clone(),
                                notifications: Notifications::Default,
                                default_notifications: Notifications::All,
                                access: Some(JoinRule::Invite),
                                history: HistoryVisibility::Shared,
                                encryption: "End-to-end encrypted".into(),
                                editable: [room.can_edit_topic; 4],
                            });
                    settings.name = room.name.clone();
                    settings.topic = room.topic.clone();
                    panel.saved = Some(settings.clone());
                    panel.draft = Some(settings);
                } else if let Some(composer) =
                    current_pane(&self.model.room, panel.target.0).and_then(|p| p.composer.clone())
                {
                    panel.pending = true;
                    let target = panel.target.clone();
                    let request = panel.request;
                    let (task, handle) = Task::perform(
                        composer.room_settings(self.room_settings.control.clone()),
                        move |result| {
                            message(Event::Loaded(target.clone(), request, Box::new(result)))
                        },
                    )
                    .abortable();
                    panel.load = Some(handle.abort_on_drop());
                    return task;
                } else {
                    panel.error = Some("The conversation is still loading. Try Reload.".into());
                }
            }
            Event::Loaded(target, request, result) => {
                if let Some(panel) = &mut self.room_settings.panel
                    && panel.target == target
                    && panel.request == request
                {
                    panel.pending = false;
                    panel.load = None;
                    match *result {
                        Ok(settings) => {
                            self.room_settings.control = settings.control.clone();
                            panel.saved = Some(settings.clone());
                            panel.draft = Some(settings);
                        }
                        Err(error) => {
                            panel.saved = None;
                            panel.draft = None;
                            panel.error = Some(error);
                        }
                    }
                }
            }
            Event::Saved(target, request, change, result) => {
                self.pending_ops = self.pending_ops.saturating_sub(1);
                self.room_settings.busy.remove(&target.1);
                if let Some(panel) = &mut self.room_settings.panel
                    && panel.target == target
                    && panel.request == request
                {
                    panel.pending = false;
                    match result {
                        Ok(()) => {
                            if let Some(saved) = &mut panel.saved {
                                apply(saved, &change);
                            }
                            if let Change::Address(address) = &change {
                                if let Some(draft) = &mut panel.draft {
                                    apply(draft, &change);
                                }
                                panel.addresses.saved(address);
                            }
                            panel.success = true;
                        }
                        Err(error) => panel.error = Some(error),
                    }
                }
            }
            Event::Save(field) => {
                let Some(panel) = &mut self.room_settings.panel else {
                    return Task::none();
                };
                if panel.avatar.is_picking()
                    || panel.pending
                    || self.room_settings.busy.contains(&panel.target.1)
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                {
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
                    2 if saved.notifications != draft.notifications => {
                        Change::Notifications(saved.notifications, draft.notifications)
                    }
                    3 if saved.editable[2] && saved.access != draft.access => Change::Access(
                        saved.access.clone(),
                        draft.access.clone().expect("editable access"),
                    ),
                    4 if saved.editable[3] && saved.history != draft.history => {
                        Change::History(saved.history.clone(), draft.history.clone())
                    }
                    _ => return Task::none(),
                };
                return self.room_settings_event(Event::Commit(change));
            }
            Event::Addresses(event) => {
                let Some(panel) = &mut self.room_settings.panel else {
                    return Task::none();
                };
                if panel.pending
                    || panel.avatar.is_picking()
                    || self.room_settings.busy.contains(&panel.target.1)
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                {
                    return Task::none();
                }
                let Some(Ok(saved)) = panel.saved.as_ref().map(|s| &s.addresses) else {
                    return Task::none();
                };
                panel.success = false;
                panel.error = None;
                match panel.addresses.update(saved, event) {
                    Ok(Some(change)) => {
                        return self.room_settings_event(Event::Commit(Change::Address(change)));
                    }
                    Err(error) => panel.error = Some(error),
                    Ok(None) => {}
                }
            }
            Event::Commit(change) => {
                let Some(panel) = &mut self.room_settings.panel else {
                    return Task::none();
                };
                if panel.pending
                    || panel.avatar.is_picking()
                    || self.room_settings.busy.contains(&panel.target.1)
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || panel.saved.is_none()
                {
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
                            .is_some_and(|s| {
                                after
                                    .all()
                                    .iter()
                                    .any(|a| !s.local.as_ref().is_ok_and(|l| l.contains(a)))
                            })
                    {
                        panel.error = Some(
                            "Add this address locally before publishing it in the demo.".into(),
                        );
                        return Task::none();
                    }
                    apply(panel.saved.as_mut().unwrap(), &change);
                    if let Change::Address(address) = &change {
                        if let Some(draft) = &mut panel.draft {
                            apply(draft, &change);
                        }
                        panel.addresses.saved(address);
                    }
                    let settings = panel.saved.as_ref().unwrap();
                    self.room_settings
                        .demo
                        .insert(panel.target.1.clone(), settings.clone());
                    if let Some(room) = std::sync::Arc::make_mut(&mut self.model.rooms)
                        .iter_mut()
                        .find(|r| r.id == panel.target.1)
                    {
                        if matches!(change, Change::Name(..)) {
                            room.name = settings.name.clone();
                        }
                        if matches!(change, Change::Topic(..)) {
                            room.topic = settings.topic.clone();
                        }
                    }
                    panel.success = true;
                } else if let Some(composer) =
                    current_pane(&self.model.room, panel.target.0).and_then(|p| p.composer.clone())
                {
                    panel.pending = true;
                    self.room_settings.busy.insert(panel.target.1.clone());
                    self.pending_ops += 1;
                    let target = panel.target.clone();
                    let request = panel.request;
                    let completed = change.clone();
                    return Task::perform(
                        composer.change_room_setting(change, self.room_settings.control.clone()),
                        move |result| {
                            message(Event::Saved(
                                target.clone(),
                                request,
                                completed.clone(),
                                result,
                            ))
                        },
                    );
                }
            }
            Event::Focus(backwards) => {
                return iced::advanced::widget::operate(focus_step(backwards));
            }
            Event::Ignore => {}
            event => {
                if let Some(panel) = &mut self.room_settings.panel
                    && !panel.pending
                    && let Some(draft) = &mut panel.draft
                {
                    match event {
                        Event::Name(value) if draft.editable[0] => draft.name = value,
                        Event::Topic(value) if draft.editable[1] => draft.topic = value,
                        Event::Notifications(value) => draft.notifications = value,
                        Event::Access(value)
                            if draft.editable[2]
                                && matches!(
                                    draft.access,
                                    Some(JoinRule::Invite | JoinRule::Public)
                                ) =>
                        {
                            draft.access = Some(value)
                        }
                        Event::History(value) if draft.editable[3] => draft.history = value,
                        _ => {}
                    }
                    panel.success = false;
                }
            }
        }
        Task::none()
    }
    pub(super) fn room_settings_button(&self, room: &str) -> Element<'_, Message> {
        super::tooltips::tooltip(
            button(
                container(super::icons::icon(super::icons::Kind::Settings))
                    .center_x(28)
                    .center_y(28),
            )
            .padding(0)
            .style(shell::icon_button)
            .on_press_maybe(
                self.context_menu_allowed()
                    .then(|| message(Event::Open(room.to_owned()))),
            ),
            "Room settings",
            iced::widget::tooltip::Position::Bottom,
        )
        .into()
    }
    pub(super) fn room_settings_view(&self) -> Element<'_, Message> {
        iced::widget::responsive(move |size| self.room_settings_sized(size)).into()
    }
    fn room_settings_sized(&self, size: iced::Size) -> Element<'_, Message> {
        let panel = self.room_settings.panel.as_ref().unwrap();
        let busy = panel.pending || self.room_settings.busy.contains(&panel.target.1);
        if panel.transition.is_some() {
            return opaque(
                mouse_area(
                    container(discard_prompt(
                        message(Event::KeepChanges),
                        message(Event::DiscardChanges),
                    ))
                    .padding(24)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|theme| shell::surface(theme, shell::palette(theme).scrim)),
                )
                .on_press(message(Event::Ignore))
                .on_scroll(|_| message(Event::Ignore)),
            );
        }
        let mut content = column![shell::title("Room settings", 20)].spacing(14);
        if let (Some(draft), Some(saved)) = (&panel.draft, &panel.saved) {
            let enabled = !busy && self.pending_ops < MAX_PENDING_OPERATIONS;
            content = content.push(
                panel
                    .avatar
                    .view(&saved.avatar, &self.model.avatars, enabled)
                    .map(move |event| message(Event::Avatar(panel.request, event))),
            );
            let name = text_input("Room name", &draft.name)
                .id("room-settings-name")
                .cursor_blinking(false)
                .style(shell::search_style)
                .size(14)
                .on_input_maybe(
                    (enabled && draft.editable[0]).then_some(|v| message(Event::Name(v))),
                );
            let topic = text_input("Room topic", &draft.topic)
                .id("room-settings-topic")
                .cursor_blinking(false)
                .style(shell::search_style)
                .size(14)
                .on_input_maybe(
                    (enabled && draft.editable[1]).then_some(|v| message(Event::Topic(v))),
                );
            content = content.push(setting(
                "Name",
                name,
                0,
                enabled && draft.editable[0] && saved.name != draft.name,
            ));
            content = content.push(setting(
                "Topic",
                topic,
                1,
                enabled && draft.editable[1] && saved.topic != draft.topic,
            ));
            content = content.push(text("Name, topic, access and history changes apply to everyone. Controls require room permissions.").size(12).style(shell::muted_text));
            content = content.push(shell::title("Notifications", 15));
            content = content.push(setting(
                "Notify me about",
                select(
                    vec![
                        Notifications::Default,
                        Notifications::All,
                        Notifications::Mentions,
                        Notifications::Mute,
                    ],
                    draft.notifications,
                    enabled,
                    |v| message(Event::Notifications(v)),
                ),
                2,
                enabled && saved.notifications != draft.notifications,
            ));
            content = content.push(
                text(format!(
                    "Account default: {}. This preference follows your account across clients.",
                    draft.default_notifications
                ))
                .size(12)
                .style(shell::muted_text),
            );
            if !self.settings.current.notifications.enabled {
                content = content
                    .push(text("Desktop alerts are disabled in application Settings.").size(12));
            }
            content = content.push(match &saved.addresses {
                Ok(addresses) => panel
                    .addresses
                    .view(addresses, enabled && !panel.avatar.is_picking()),
                Err(error) => column![shell::title("Room addresses", 15), text(error).size(13)]
                    .spacing(8)
                    .into(),
            });
            content = content.push(shell::title("Security & access", 15));
            content = content.push(text(format!("Encryption: {}", draft.encryption)).size(13));
            let supported = matches!(saved.access, Some(JoinRule::Invite | JoinRule::Public));
            let access: Element<'_, Message> = if supported {
                let value = if draft.access == Some(JoinRule::Public) {
                    Access::Public
                } else {
                    Access::Invite
                };
                select(
                    vec![Access::Invite, Access::Public],
                    value,
                    enabled && draft.editable[2],
                    |v| {
                        message(Event::Access(match v {
                            Access::Invite => JoinRule::Invite,
                            Access::Public => JoinRule::Public,
                        }))
                    },
                )
            } else {
                text(format!(
                    "{} (managed in another client)",
                    draft.access.as_ref().map_or("Unknown", JoinRule::as_str)
                ))
                .size(13)
                .into()
            };
            content = content.push(setting(
                "Who can join",
                access,
                3,
                supported && enabled && draft.editable[2] && saved.access != draft.access,
            ));
            content = content.push(setting(
                "Who can read history",
                select(
                    vec![
                        History::Joined,
                        History::Invited,
                        History::Shared,
                        History::World,
                    ],
                    History::from(&draft.history),
                    enabled && draft.editable[3] && History::from(&saved.history) != History::Other,
                    |v| message(Event::History(v.value())),
                ),
                4,
                enabled && draft.editable[3] && saved.history != draft.history,
            ));
            content = content.push(text("Public access lets anyone join. History visibility does not grant old encryption keys. Public access does not list the room in a directory.").size(12).style(shell::muted_text));
        } else if busy {
            content = content.push(text("Loading room settings…").size(13));
        }
        content = content.push(
            row![
                button("Members & invitations")
                    .style(shell::quiet_button)
                    .on_press_maybe((!busy).then_some(message(Event::Members))),
                button(
                    row![
                        super::icons::icon(super::icons::Kind::ArrowLeft),
                        text("Leave room…")
                    ]
                    .spacing(7)
                    .align_y(iced::Alignment::Center)
                )
                .style(button::danger)
                .on_press_maybe((!busy).then_some(message(Event::Leave))),
            ]
            .spacing(8),
        );
        content = content.push(text(&panel.target.1).size(12).style(shell::muted_text));
        if let Some(error) = &panel.error {
            content = content.push(text(error).size(13));
        }
        if busy && panel.saved.is_some() {
            content = content.push(
                text(if panel.load.is_some() {
                    "Refreshing…"
                } else {
                    "Saving…"
                })
                .size(13),
            );
        }
        if panel.success {
            content = content.push(
                text(if self.demo.is_some() {
                    "Saved for this demo session."
                } else {
                    "Saved."
                })
                .size(13),
            );
        }
        let card = container(
            column![
                scrollable(content)
                    .height(Length::Fill)
                    .direction(shell::scrollbar())
                    .style(shell::scroll_style),
                row![
                    super::tooltips::icon_button(
                        super::icons::Kind::Refresh,
                        "Reload room settings",
                        (!busy).then_some(message(Event::Reload))
                    ),
                    iced::widget::space().width(Length::Fill),
                    super::tooltips::icon_button(
                        super::icons::Kind::Close,
                        "Close room settings",
                        Some(message(Event::Close))
                    )
                ]
                .spacing(8)
            ]
            .spacing(16),
        )
        .id("room-settings-dialog")
        .padding(20)
        .width(560)
        .height((size.height - 32.0).clamp(1.0, 720.0))
        .style(|theme| container::Style {
            border: iced::Border::default().rounded(12),
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
            .on_press(message(Event::Ignore))
            .on_scroll(|_| message(Event::Ignore)),
        )
    }
}
fn setting<'a>(
    label: &'a str,
    control: impl Into<Element<'a, Message>>,
    field: usize,
    changed: bool,
) -> Element<'a, Message> {
    column![
        text(label).size(12).style(shell::muted_text),
        row![
            container(control).width(Length::Fill),
            super::tooltips::icon_button(
                super::icons::Kind::Check,
                "Save",
                changed.then_some(message(Event::Save(field)))
            )
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
    ]
    .spacing(5)
    .into()
}
fn select<'a, T: Clone + Eq + std::fmt::Display + 'a>(
    values: Vec<T>,
    selected: T,
    enabled: bool,
    event: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    if enabled {
        pick_list(values, Some(selected), event)
            .style(shell::select_style)
            .padding([7, 10])
            .width(Length::Fill)
            .text_size(13)
            .into()
    } else {
        text(selected.to_string()).size(13).into()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Invite,
    Public,
}
impl std::fmt::Display for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invite => "Invited people",
            Self::Public => "Anyone",
        })
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum History {
    Joined,
    Invited,
    Shared,
    World,
    Other,
}
impl From<&HistoryVisibility> for History {
    fn from(value: &HistoryVisibility) -> Self {
        match value {
            HistoryVisibility::Joined => Self::Joined,
            HistoryVisibility::Invited => Self::Invited,
            HistoryVisibility::Shared => Self::Shared,
            HistoryVisibility::WorldReadable => Self::World,
            _ => Self::Other,
        }
    }
}
impl History {
    fn value(self) -> HistoryVisibility {
        match self {
            Self::Joined => HistoryVisibility::Joined,
            Self::Invited => HistoryVisibility::Invited,
            Self::Shared => HistoryVisibility::Shared,
            Self::World => HistoryVisibility::WorldReadable,
            Self::Other => unreachable!(),
        }
    }
}
impl std::fmt::Display for History {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Joined => "Members · since joining",
            Self::Invited => "Members · since invitation",
            Self::Shared => "Members · all history",
            Self::World => "Anyone, including non-members",
            Self::Other => "Custom (managed in another client)",
        })
    }
}

fn focus_index<T>(target: usize) -> impl iced::advanced::widget::Operation<T> {
    struct FocusIndex {
        target: usize,
        current: usize,
    }
    impl<T> iced::advanced::widget::Operation<T> for FocusIndex {
        fn focusable(
            &mut self,
            _: Option<&iced::advanced::widget::Id>,
            _: iced::Rectangle,
            state: &mut dyn iced::advanced::widget::operation::focusable::Focusable,
        ) {
            if self.current == self.target {
                state.focus();
            } else {
                state.unfocus();
            }
            self.current += 1;
        }
        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<T>),
        ) {
            operate(self);
        }
    }
    FocusIndex { target, current: 0 }
}
fn focus_step<T: Send + 'static>(backwards: bool) -> impl iced::advanced::widget::Operation<T> {
    use iced::advanced::widget::{
        Operation,
        operation::{self, focusable},
    };
    let next: fn(focusable::Count) -> Box<dyn Operation<T>> = if backwards {
        |count| {
            if count.focused == Some(0) {
                Box::new(focus_index(count.total.saturating_sub(1)))
            } else {
                Box::new(focusable::focus_previous())
            }
        }
    } else {
        |count| {
            if count.focused == Some(count.total.saturating_sub(1)) {
                Box::new(focusable::focus("room-settings-name".into()))
            } else {
                Box::new(focusable::focus_next())
            }
        }
    };
    operation::scope(
        "room-settings-dialog".into(),
        operation::then(focusable::count(), next),
    )
}

pub fn shortcut(
    event: iced::Event,
    _: iced::event::Status,
    _: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{Key, key::Named};
    if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
        match key {
            Key::Named(Named::Escape) => Some(message(Event::Close)),
            Key::Named(Named::Tab) => Some(message(Event::Focus(modifiers.shift()))),
            _ => None,
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn open(app: &mut App) {
        let room = app.selection.room.as_ref().unwrap().1.clone();
        let _ = app.update(message(Event::Open(room)));
    }
    #[test]
    fn demo_addresses_preserve_other_drafts_and_require_confirmation_for_removal() {
        use addresses::Event as Edit;
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.scroll_positions.insert(key.clone(), 123.0);
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        open(&mut app);
        let _ = app.update(message(Event::Name("Unsaved name".into())));
        let alias = matrix_sdk::ruma::owned_room_alias_id!("#friendly:demo.invalid");
        // The demo never resolves addresses on the network.
        for edit in [Edit::Published("friendly".into()), Edit::Publish] {
            let _ = app.update(message(Event::Addresses(edit)));
        }
        assert!(app.room_settings.panel.as_ref().unwrap().error.is_some());
        for edit in [
            Edit::Local("friendly".into()),
            Edit::AddLocal,
            Edit::Publish,
            Edit::Main(Some(alias.clone())),
            Edit::Directory(true),
        ] {
            let _ = app.update(message(Event::Addresses(edit)));
        }
        let panel = app.room_settings.panel.as_ref().unwrap();
        let saved = panel.saved.as_ref().unwrap().addresses.as_ref().unwrap();
        assert_eq!(saved.published.main.as_ref(), Some(&alias));
        assert_eq!(saved.local, Ok(vec![alias.clone()]));
        assert_eq!(saved.directory, Ok(true));
        assert_eq!(panel.draft.as_ref().unwrap().name, "Unsaved name");
        let _ = app.update(message(Event::Close));
        open(&mut app);
        let saved = app
            .room_settings
            .panel
            .as_ref()
            .unwrap()
            .saved
            .as_ref()
            .unwrap()
            .addresses
            .as_ref()
            .unwrap();
        assert_eq!(saved.published.main.as_ref(), Some(&alias));
        assert_eq!(saved.directory, Ok(true));
        // Published mappings cannot be removed, even by an injected action.
        for edit in [Edit::Remove(alias.clone()), Edit::ConfirmRemove] {
            let _ = app.update(message(Event::Addresses(edit)));
        }
        assert_eq!(
            app.room_settings
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
        assert_eq!(
            app.room_settings
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
        let _ = app.update(message(Event::Addresses(Edit::ConfirmRemove)));
        assert_eq!(
            app.room_settings
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
            Ok(vec![])
        );
        assert_eq!(app.drafts[&key].content.text(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 123.0);
        let _ = app.update(Message::Room("release".into()));
        open(&mut app);
        assert!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .saved
                .as_ref()
                .unwrap()
                .addresses
                .as_ref()
                .unwrap()
                .published
                .all()
                .is_empty()
        );
    }

    #[test]
    fn dirty_room_settings_require_explicit_discard_before_close_or_reload() {
        let mut app = demo_app();
        open(&mut app);
        let original = app
            .room_settings
            .panel
            .as_ref()
            .unwrap()
            .saved
            .as_ref()
            .unwrap()
            .name
            .clone();
        let _ = app.update(message(Event::Name("Uncommitted room name".into())));
        let _ = app.update(message(Event::Close));
        let panel = app.room_settings.panel.as_ref().unwrap();
        assert!(panel.transition.is_some());
        assert_eq!(panel.draft.as_ref().unwrap().name, "Uncommitted room name");
        let _ = app.update(message(Event::KeepChanges));
        assert!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .transition
                .is_none()
        );
        let _ = app.update(message(Event::Reload));
        assert!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .transition
                .is_some()
        );
        let _ = app.update(message(Event::DiscardChanges));
        assert_eq!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .name,
            original
        );
        let _ = app.update(message(Event::Addresses(addresses::Event::Local(
            "unfinished".into(),
        ))));
        let _ = app.update(message(Event::Members));
        assert!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .transition
                .is_some()
        );
        assert!(!app.space_settings.is_open());
        let _ = app.update(message(Event::DiscardChanges));
        assert!(!app.room_settings.is_open());
        assert!(app.space_settings.is_open());
    }

    #[tokio::test]
    async fn avatar_drafts_save_separately_and_late_results_release_once() {
        let mut app = demo_app();
        open(&mut app);
        let session = app.room_settings.panel.as_ref().unwrap().request;
        let id = app.room_settings.panel.as_ref().unwrap().target.1.clone();
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
            app.room_settings
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
        assert!(!app.room_settings.busy.contains(&id));
        open(&mut app);
        let panel = app.room_settings.panel.as_ref().unwrap();
        assert_eq!(panel.saved.as_ref().unwrap().avatar.url, Some(url));
        assert_ne!(panel.saved.as_ref().unwrap().name, "Uncommitted metadata");
        let session = panel.request;
        let _ = app.update(message(Event::Avatar(
            session.wrapping_sub(1),
            avatar_editor::Event::Remove,
        )));
        assert!(
            app.room_settings
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
            app.room_settings
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
    fn demo_settings_persist_per_room_without_changing_drafts_or_scroll() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent draft");
        app.scroll_positions.insert(key.clone(), 140.0);
        open(&mut app);
        assert!(!app.selection.conversation_visible);
        for event in [
            Event::Notifications(Notifications::Mute),
            Event::Save(2),
            Event::Name("New name".into()),
            Event::Save(0),
            Event::Topic("New topic".into()),
            Event::Save(1),
            Event::Access(JoinRule::Public),
            Event::Save(3),
            Event::History(HistoryVisibility::Joined),
            Event::Save(4),
            Event::Close,
        ] {
            let _ = app.update(message(event));
        }
        open(&mut app);
        let saved = app
            .room_settings
            .panel
            .as_ref()
            .unwrap()
            .saved
            .as_ref()
            .unwrap();
        assert_eq!(saved.notifications, Notifications::Mute);
        assert_eq!(saved.name, "New name");
        assert_eq!(saved.topic, "New topic");
        assert_eq!(saved.access, Some(JoinRule::Public));
        assert_eq!(saved.history, HistoryVisibility::Joined);
        let _ = app.update(message(Event::Name("Discarded".into())));
        let _ = app.update(Message::Escape);
        open(&mut app);
        assert_eq!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .name,
            "New name"
        );
        let _ = app.update(message(Event::Close));
        let _ = app.update(Message::Room("general".into()));
        open(&mut app);
        assert_eq!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .saved
                .as_ref()
                .unwrap()
                .notifications,
            Notifications::Default
        );
        assert_eq!(app.drafts[&key].content.text(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 140.0);
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn covered_input_permissions_and_stale_results_are_rejected() {
        let mut app = demo_app();
        open(&mut app);
        let target = app.room_settings.panel.as_ref().unwrap().target.clone();
        let request = app.room_settings.panel.as_ref().unwrap().request;
        let key = app.draft_key(false).unwrap();
        assert!(app.attachment_drop_target(&key, target.0).is_none());
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert!(app.settings.draft.is_none());
        app.pending_ops = 1;
        let _ = app.update(Message::Topic(super::super::topic::Event::Saved(
            target.clone(),
            request,
            Err("Old topic write".into()),
        )));
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::Room("release".into()));
        assert!(!app.room_settings.is_open());
        open(&mut app);
        let _ = app.update(message(Event::Loaded(
            target.clone(),
            request,
            Box::new(Err("Late load".into())),
        )));
        assert!(app.room_settings.panel.as_ref().unwrap().error.is_none());
        app.pending_ops = 1;
        app.room_settings.busy.insert(target.1.clone());
        let _ = app.update(message(Event::Saved(
            target,
            request,
            Change::Name("before".into(), "after".into()),
            Err("Late write".into()),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(app.room_settings.busy.is_empty());
        assert!(app.room_settings.panel.as_ref().unwrap().error.is_none());
        let _ = app.update(message(Event::Name("Forbidden".into())));
        let _ = app.update(message(Event::Save(0)));
        assert_ne!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .saved
                .as_ref()
                .unwrap()
                .name,
            "Forbidden"
        );
        let _ = app.update(message(Event::Notifications(Notifications::Mentions)));
        let _ = app.update(message(Event::Save(2)));
        assert_eq!(
            app.room_settings
                .panel
                .as_ref()
                .unwrap()
                .saved
                .as_ref()
                .unwrap()
                .notifications,
            Notifications::Mentions
        );
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use iced::advanced::{
        layout, mouse,
        widget::{
            self, Operation,
            operation::{self, Outcome},
        },
    };

    #[test]
    fn panel_fits_minimum_window_traps_text_focus_and_has_no_caret_timer() {
        let mut app = crate::tests::demo_app();
        let room = app.selection.room.as_ref().unwrap().1.clone();
        let _ = app.update(message(Event::Open(room)));
        for size in [
            iced::Size::new(640.0, 480.0),
            iced::Size::new(1400.0, 900.0),
        ] {
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut view = app.room_settings_sized(size);
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert_eq!(node.size(), size);
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut operation::focusable::focus::<()>("room-settings-name".into()),
            );
            for expected in [Some(0), Some(1), Some(2), Some(3), Some(0)] {
                let mut count = operation::map(
                    operation::scope("room-settings-dialog".into(), operation::focusable::count()),
                    move |count| {
                        assert_eq!(count.total, 4);
                        assert_eq!(count.focused, expected);
                    },
                );
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut count,
                );
                assert!(matches!(count.finish(), Outcome::Some(())));
                let mut next: Box<dyn Operation<()>> = Box::new(focus_step(false));
                loop {
                    view.as_widget_mut().operate(
                        &mut tree,
                        layout::Layout::new(&node),
                        &renderer,
                        next.as_mut(),
                    );
                    match next.finish() {
                        Outcome::Chain(op) => next = op,
                        _ => break,
                    }
                }
            }
            for expected in [Some(0), Some(3)] {
                let mut previous: Box<dyn Operation<()>> = Box::new(focus_step(true));
                loop {
                    view.as_widget_mut().operate(
                        &mut tree,
                        layout::Layout::new(&node),
                        &renderer,
                        previous.as_mut(),
                    );
                    match previous.finish() {
                        Outcome::Chain(op) => previous = op,
                        _ => break,
                    }
                }
                let mut count = operation::map(
                    operation::scope("room-settings-dialog".into(), operation::focusable::count()),
                    move |count| assert_eq!(count.focused, expected),
                );
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut count,
                );
                assert!(matches!(count.finish(), Outcome::Some(())));
            }
            let now = iced::time::Instant::now();
            for millis in [0, 750, 1250] {
                let mut messages = Vec::new();
                let mut shell = iced::advanced::Shell::new(&mut messages);
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
                if millis > 0 {
                    assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                }
            }
            let mut messages = Vec::new();
            let mut shell = iced::advanced::Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                layout::Layout::new(&node),
                mouse::Cursor::Available(iced::Point::new(5.0, 10.0)),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert!(shell.is_event_captured());
            assert!(matches!(
                messages.as_slice(),
                [Message::RoomSettings(Event::Ignore)]
            ));
        }
    }
}
