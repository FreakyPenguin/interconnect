//! Full-size space settings independent of conversation selection.
use super::{App, MAX_PENDING_OPERATIONS, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, pick_list, row, rule, scrollable, text, text_input},
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
    pub(super) fn validate_space_settings(&mut self) {
        if self.space_settings.panel.as_ref().is_some_and(|p| {
            self.closing
                || self.stopped
                || p.account != self.account_id
                || (self.demo.is_none() && self.model.authentication.is_some())
                || self.shell.space != Scope::Space(p.id.clone())
                || !self.model.spaces.iter().any(|s| s.id == p.id)
        }) {
            self.space_settings.close();
        }
    }
    pub(super) fn space_settings_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open(id) => {
                if !self.context_menu_allowed()
                    || self.shell.space != Scope::Space(id.clone())
                    || !self.model.spaces.iter().any(|s| s.id == id)
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
                return self.space_settings_event(Event::Reload);
            }
            Event::Close => {
                self.space_settings.close();
                return iced::widget::operation::focus("room-composer");
            }
            Event::Reload => {
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
                if self.demo.is_some() {
                    let Some(space) = self.model.spaces.iter().find(|s| s.id == panel.id) else {
                        return Task::none();
                    };
                    let settings =
                        self.space_settings
                            .demo
                            .get(&panel.id)
                            .cloned()
                            .unwrap_or(Settings {
                                name: space.name.clone(),
                                topic: String::new(),
                                access: Some(JoinRule::Invite),
                                editable: [space.can_manage_rooms; 3],
                            });
                    panel.saved = Some(settings.clone());
                    panel.draft = Some(settings);
                } else if let Some(directory) = self.model.people_directory.clone() {
                    panel.pending = true;
                    let id = panel.id.clone();
                    let request = panel.request;
                    let (task, handle) = Task::perform(
                        async move {
                            let control = directory.space_management(id).await?;
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
        let mut body = column![
            row![
                shell::title("Space settings", 22),
                iced::widget::space().width(Length::Fill),
                button("Reload")
                    .style(shell::quiet_button)
                    .on_press_maybe((!panel.pending).then_some(message(Event::Reload))),
                button("Close")
                    .style(shell::quiet_button)
                    .on_press(message(Event::Close))
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            text(&panel.id).size(11).style(shell::muted_text),
            rule::horizontal(1)
        ]
        .spacing(18);
        if let (Some(saved), Some(draft)) = (&panel.saved, &panel.draft) {
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
                        text(label).size(13).style(shell::muted_text),
                        row![
                            input,
                            button("Save").style(shell::quiet_button).on_press_maybe(
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
                    text("Who can join").size(13).style(shell::muted_text),
                    row![
                        access,
                        button("Save").style(shell::quiet_button).on_press_maybe(
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
