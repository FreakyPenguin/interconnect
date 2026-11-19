//! Full-size space settings and shared member management without changing conversations.
#[path = "member_management.rs"]
mod member_management;
#[path = "space_directory.rs"]
mod space_directory;
#[path = "space_exit.rs"]
mod space_exit;

use super::{
    App, MAX_PENDING_OPERATIONS, Message,
    icons::{self, Kind},
    shell,
    tooltips::icon_button,
};
use iced::{
    Element, Length, Task,
    widget::{column, container, pick_list, row, rule, scrollable, text, text_input},
};
use interconnect::{
    navigation::Scope,
    room_management::{Change, Management, Settings},
};
use matrix_sdk::ruma::events::room::join_rules::JoinRule;
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    panel: Option<Panel>,
    writes: HashMap<u64, (Option<String>, String)>,
    demo: HashMap<String, Settings>,
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
    }
}
struct Panel {
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
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(String),
    Browse(String),
    FindRooms,
    Directory(space_directory::Event),
    OpenMembers(String),
    Members(member_management::Event),
    Exit(space_exit::Event),
    Close,
    Reload,
    Name(String),
    Topic(String),
    Access(JoinRule),
    Save(usize),
    Loaded(u64, Box<Result<(Management, Settings), String>>),
    Saved(u64, Change, Result<(), String>),
}
fn message(event: Event) -> Message {
    Message::SpaceSettings(event)
}
impl App {
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
        let opening_space = matches!(&event, Event::Open(_) | Event::Browse(_));
        let browse = matches!(&event, Event::Browse(_));
        match event {
            Event::FindRooms => {
                let id = match &self.shell.space {
                    Scope::Space(id) => id.clone(),
                    _ => String::new(),
                };
                return self.space_settings_event(Event::Browse(id));
            }
            Event::Exit(event) => return self.space_exit_event(event),
            Event::Directory(event) => return self.space_directory_event(event),
            Event::Members(event) => return self.member_management_event(event),
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
                self.global_search.close();
                self.bookmarks.close();
                self.context_menu = None;
                self.shell.spaces.menu = None;
                self.generation += 1;
                self.space_settings.panel = Some(Panel {
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
                                name,
                                topic: String::new(),
                                access: Some(JoinRule::Invite),
                                editable: [editable; 3],
                            });
                    panel.saved = Some(settings.clone());
                    panel.draft = Some(settings);
                    return self.member_management_event(member_management::Event::Reload);
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
                            panel.draft = Some(settings);
                            return self.member_management_event(member_management::Event::Reload);
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
                if panel.pending
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
                if draft.name.trim().is_empty() || draft.name.len() > 255 {
                    panel.error = Some("Enter a name within 255 bytes.".into());
                    return Task::none();
                }
                panel.error = None;
                panel.success = false;
                if self.demo.is_some() {
                    panel.saved.as_mut().unwrap().apply(&change);
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
                if self.space_settings.writes.remove(&request).is_none() {
                    return Task::none();
                }
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
                            panel.success = true;
                        }
                        Err(error) => panel.error = Some(error),
                    }
                }
            }
            edit => {
                if let Some(panel) = &mut self.space_settings.panel
                    && !panel.pending
                    && let Some(draft) = &mut panel.draft
                {
                    match edit {
                        Event::Name(value) if draft.editable[0] => draft.name = value,
                        Event::Topic(value) if draft.editable[1] => draft.topic = value,
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
        if panel.directory.is_some() {
            return self.space_directory_view();
        }
        if panel.exit.is_open() {
            return self.space_exit_view();
        }
        let mut body = column![
            row![
                icons::icon(if panel.space {
                    Kind::Space
                } else {
                    Kind::People
                }),
                shell::title(
                    if panel.space {
                        "Space settings"
                    } else {
                        "Room members"
                    },
                    22
                ),
                iced::widget::space().width(Length::Fill),
                if panel.space {
                    iced::Element::from(icon_button(
                        Kind::Compass,
                        "Find rooms to join",
                        Some(message(Event::Browse(panel.id.clone()))),
                    ))
                } else {
                    iced::Element::from(iced::widget::space().width(0))
                },
                icon_button(
                    Kind::Refresh,
                    if panel.space {
                        "Reload space settings"
                    } else {
                        "Reload room members"
                    },
                    (!panel.pending).then_some(message(Event::Reload))
                ),
                icon_button(
                    Kind::Close,
                    if panel.space {
                        "Close space settings"
                    } else {
                        "Close room members"
                    },
                    Some(message(Event::Close))
                )
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            text(&panel.id).size(11).style(shell::muted_text),
            rule::horizontal(1)
        ]
        .spacing(18);
        if panel.space
            && let (Some(saved), Some(draft)) = (&panel.saved, &panel.draft)
        {
            for (field, label, value, before) in [
                (0, "Name", &draft.name, &saved.name),
                (1, "Topic", &draft.topic, &saved.topic),
            ] {
                let editable = saved.editable[field] && !panel.pending;
                let mut input = text_input(label, value)
                    .cursor_blinking(false)
                    .padding(10)
                    .style(shell::search_style);
                if editable {
                    input = input.on_input(move |value| {
                        message(if field == 0 {
                            Event::Name(value)
                        } else {
                            Event::Topic(value)
                        })
                    });
                }
                body = body.push(
                    column![
                        row![
                            icons::icon(if field == 0 { Kind::Edit } else { Kind::Thread }),
                            text(label).size(13).style(shell::muted_text)
                        ]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                        row![
                            input,
                            icon_button(
                                Kind::Check,
                                if field == 0 {
                                    "Save name"
                                } else {
                                    "Save topic"
                                },
                                (editable && value != before)
                                    .then_some(message(Event::Save(field)))
                            )
                        ]
                        .spacing(8)
                    ]
                    .spacing(8),
                );
            }
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
            body = body.push(
                column![
                    row![
                        icons::icon(Kind::People),
                        text("Who can join").size(13).style(shell::muted_text)
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                    row![
                        access,
                        icon_button(
                            Kind::Check,
                            "Save access",
                            (supported
                                && draft.editable[2]
                                && !panel.pending
                                && draft.access != saved.access)
                                .then_some(message(Event::Save(2)))
                        )
                    ]
                    .spacing(8)
                ]
                .spacing(8),
            );
            if !supported {
                body = body.push(
                    text("This access rule is read-only.")
                        .size(12)
                        .style(shell::muted_text),
                );
            }
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
        if panel.pending {
            body = body.push(text("Working…").size(13));
        }
        if panel.success {
            body = body.push(text("Saved").size(13).style(shell::muted_text));
        }
        if let Some(error) = &panel.error {
            body = body.push(text(error).size(13).style(shell::accent_text));
        }
        container(
            scrollable(container(body).padding(28).max_width(800))
                .direction(shell::scrollbar())
                .style(shell::scroll_style),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
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
