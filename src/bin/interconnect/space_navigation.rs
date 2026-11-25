//! Local, event-driven space browsing. No SDK requests or navigation of timelines.
use super::{
    App, Message,
    icons::{Kind, icon, sized},
    navigation_control, popover, shell,
};
use iced::{
    Alignment, Border, Element, Length, Task,
    advanced::widget::{
        self,
        operation::{Focusable, Outcome},
    },
    keyboard::{Key, Modifiers, key::Named},
    widget::{button, column, container, row, scrollable, text},
};
use interconnect::navigation::{self, Scope};
use std::{collections::HashSet, time::Instant};

pub const HOME: &str = "space-home";
pub const ROOTS: &str = "space-roots";
pub const ACCOUNT: &str = "space-account";
pub const TOGGLE: &str = "space-subrooms";
const RESULTS: &str = "space-menu-results";
const HOME_KEY: &str = "$home";

#[derive(Default)]
pub struct State {
    pub path: Vec<String>,
    pub include_subspaces: HashSet<String>,
    pub expanded: HashSet<String>,
    pub menu: Option<Menu>,
    typed: String,
    last_typed: Option<Instant>,
}
pub struct Menu {
    anchor: String,
    parent: Vec<String>,
}
#[derive(Clone, Debug)]
pub enum Event {
    FocusLocation,
    CycleRegion(bool),
    CycleFrom(Option<String>, bool),
    Open { anchor: String, parent: Vec<String> },
    Choose(Vec<String>),
    Expand(Vec<String>),
    ToggleSubspaces,
    TrailKey(String, Key),
    MenuKey(Vec<String>, Key, Modifiers),
    Dismiss,
}
fn message(event: Event) -> Message {
    Message::SpaceNavigation(event)
}
pub(super) fn is_toggle_shortcut(key: &Key, modifiers: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character("l" | "L"))
        && modifiers.command()
        && modifiers.shift()
        && !modifiers.alt()
}
pub fn scope_key(scope: &Scope) -> String {
    match scope {
        Scope::Space(id) => id.clone(),
        Scope::Home | Scope::All => HOME_KEY.into(),
        Scope::Direct => "$direct".into(),
    }
}
pub fn recursive(app: &App, scope: &Scope) -> bool {
    *scope == Scope::All
        || app
            .shell
            .spaces
            .include_subspaces
            .contains(&scope_key(scope))
}
fn crumb_id(index: usize) -> String {
    format!("space-crumb:{index}")
}
fn first_crumb(length: usize, width: f32) -> usize {
    let count = ((width - 220.0).max(110.0) / 150.0).floor() as usize;
    length.saturating_sub(count.max(1))
}
fn menu_id(path: &[String]) -> String {
    format!("space-choice:{}", serde_json::to_string(path).unwrap())
}
fn focus(id: impl Into<widget::Id>) -> Task<Message> {
    iced::widget::operation::focus(id)
}
fn bar_control<'a>(
    content: Element<'a, Message>,
    id: String,
    action: Message,
    enabled: bool,
) -> Element<'a, Message> {
    let key_id = id.clone();
    navigation_control::view(
        button(content)
            .padding([5, 7])
            .style(shell::icon_button)
            .on_press_maybe(enabled.then(|| action.clone())),
        id,
        action,
        move |key, modifiers| keys(key_id.clone(), key, modifiers),
    )
}
fn keys(id: String, key: &Key, modifiers: Modifiers) -> Option<Message> {
    if matches!(key, Key::Named(Named::Tab)) && (modifiers - Modifiers::SHIFT).is_empty() {
        return Some(Message::Focus(modifiers.shift()));
    }
    if modifiers.is_empty()
        && matches!(
            key,
            Key::Named(
                Named::ArrowLeft | Named::ArrowRight | Named::ArrowDown | Named::Home | Named::End
            )
        )
    {
        return Some(message(Event::TrailKey(id, key.clone())));
    }
    None
}
struct FindFocus {
    backwards: bool,
    focused: Option<String>,
    ids: Vec<String>,
}
impl widget::Operation<Message> for FindFocus {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn widget::Operation<Message>)) {
        visit(self);
    }
    fn focusable(
        &mut self,
        id: Option<&widget::Id>,
        _: iced::Rectangle,
        state: &mut dyn Focusable,
    ) {
        if state.is_focused() {
            self.focused = self
                .ids
                .iter()
                .find(|candidate| id == Some(&widget::Id::from((*candidate).clone())))
                .cloned();
        }
    }
    fn finish(&self) -> Outcome<Message> {
        Outcome::Some(message(Event::CycleFrom(
            self.focused.clone(),
            self.backwards,
        )))
    }
}
#[derive(Clone)]
struct Entry {
    path: Vec<String>,
    depth: usize,
}

impl App {
    pub(super) fn space_icon(&self, id: &str) -> Element<'static, Message> {
        let space = self.model.spaces.iter().find(|space| space.id == id);
        let image = space
            .and_then(|space| space.avatar_url.as_ref())
            .and_then(|uri| self.model.avatars.get(uri));
        let content: Element<'static, Message> = if let Some(handle) = image {
            iced::widget::image(handle.clone())
                .width(20)
                .height(20)
                .content_fit(iced::ContentFit::Cover)
                .border_radius(4)
                .into()
        } else {
            let initial = space
                .and_then(|space| space.name.trim().chars().next())
                .unwrap_or('#')
                .to_uppercase()
                .collect::<String>();
            container(text(initial).size(11))
                .center_x(20)
                .center_y(20)
                .style(|theme: &iced::Theme| container::Style {
                    border: Border::default().rounded(4),
                    ..shell::surface(theme, theme.extended_palette().primary.weak.color)
                })
                .into()
        };
        container(content).id(format!("space-icon:{id}")).into()
    }

    fn unread_spaces(&self) -> HashSet<String> {
        let mut parents: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();
        for space in self.model.spaces.iter() {
            for child in &space.children {
                parents.entry(child).or_default().push(&space.id);
            }
        }
        let mut pending: Vec<&str> = self
            .model
            .rooms
            .iter()
            .filter(|room| room.unread)
            .map(|room| room.id.as_str())
            .collect();
        let mut unread = HashSet::new();
        while let Some(id) = pending.pop() {
            if let Some(parents) = parents.get(id) {
                for parent in parents {
                    if unread.insert((*parent).to_owned()) {
                        pending.push(parent);
                    }
                }
            }
        }
        unread
    }
    pub(super) fn space_navigation_allowed(&self) -> bool {
        self.context_menu_allowed()
            && self.context_menu.is_none()
            && self.emoji_management.is_none()
            && !self.section_dragging()
            && !self.composing
            && !self.emoji_completion.is_active()
            && !self.user_completion.is_active()
    }
    pub(super) fn location_path(&self) -> Vec<String> {
        match &self.shell.space {
            Scope::Space(id) => {
                navigation::space_path(&self.model.spaces, id, &self.shell.spaces.path)
            }
            _ => Vec::new(),
        }
    }
    fn location_focus(&self) -> String {
        self.location_path()
            .len()
            .checked_sub(1)
            .map_or_else(|| HOME.into(), crumb_id)
    }
    fn trail_ids(&self) -> Vec<String> {
        let length = self.location_path().len();
        let width = self.window_width.unwrap_or(1280.0);
        let start = first_crumb(length, width);
        ["space-sidebar".into(), HOME.into(), ROOTS.into()]
            .into_iter()
            .chain(
                (start.saturating_sub(usize::from(start > 0 && width >= 500.0))..length)
                    .map(crumb_id),
            )
            .chain(["notification-bell".into(), ACCOUNT.into()])
            .collect()
    }
    fn child_spaces(&self, parent: &[String]) -> Vec<String> {
        let mut children = parent.last().map_or_else(
            || navigation::space_roots(&self.model.spaces),
            |id| {
                self.model
                    .spaces
                    .iter()
                    .find(|s| &s.id == id)
                    .map_or_else(Vec::new, |s| {
                        s.children
                            .iter()
                            .filter(|id| {
                                self.model.spaces.iter().any(|s| &s.id == *id)
                                    && !parent.contains(id)
                            })
                            .cloned()
                            .collect()
                    })
            },
        );
        children.sort_by_cached_key(|id| {
            self.model
                .spaces
                .iter()
                .find(|s| &s.id == id)
                .map(|s| (s.name.to_lowercase(), id.clone()))
        });
        children
    }
    fn menu_entries(&self) -> Vec<Entry> {
        let Some(menu) = &self.shell.spaces.menu else {
            return Vec::new();
        };
        let mut pending: Vec<_> = self
            .child_spaces(&menu.parent)
            .into_iter()
            .rev()
            .map(|id| {
                let mut path = menu.parent.clone();
                path.push(id);
                Entry { path, depth: 0 }
            })
            .collect();
        let mut entries = Vec::new();
        // Bound work for pathological shared/cyclic graphs. Every branch can
        // still be selected and browsed as the next location.
        while let Some(entry) = pending.pop() {
            if entries.len() == 1024 {
                break;
            }
            if self
                .shell
                .spaces
                .expanded
                .contains(entry.path.last().unwrap())
                && entry.depth < 64
            {
                for id in self.child_spaces(&entry.path).into_iter().rev() {
                    let mut path = entry.path.clone();
                    path.push(id);
                    pending.push(Entry {
                        path,
                        depth: entry.depth + 1,
                    });
                }
            }
            entries.push(entry);
        }
        entries
    }
    fn focus_entry(&self, path: &[String]) -> Task<Message> {
        let entries = self.menu_entries();
        let Some(index) = entries.iter().position(|entry| entry.path == path) else {
            return Task::none();
        };
        focus(menu_id(path)).chain(iced::widget::operation::snap_to(
            RESULTS,
            iced::widget::operation::RelativeOffset {
                x: 0.0,
                y: index as f32 / entries.len().saturating_sub(1).max(1) as f32,
            },
        ))
    }
    pub(super) fn validate_space_navigation(&mut self) {
        self.shell.spaces.path = self.location_path();
        if self.shell.spaces.menu.as_ref().is_some_and(|menu| {
            !self.space_navigation_allowed()
                || (!menu.parent.is_empty()
                    && !navigation::valid_space_path(&self.model.spaces, &menu.parent))
                || !self.trail_ids().contains(&menu.anchor)
        }) {
            self.shell.spaces.menu = None;
        }
    }
    pub(super) fn space_navigation_event(&mut self, event: Event) -> Task<Message> {
        if matches!(event, Event::Dismiss) {
            return self
                .shell
                .spaces
                .menu
                .take()
                .map_or_else(Task::none, |menu| focus(menu.anchor));
        }
        if !self.space_navigation_allowed() {
            return Task::none();
        }
        self.pending_room_focus = None;
        self.pending_thread_focus = None;
        match event {
            Event::FocusLocation => {
                self.stop_typing();
                self.shell.spaces.menu = None;
                return focus(self.location_focus());
            }
            Event::CycleRegion(backwards) => {
                if self.shell.spaces.menu.is_some() {
                    return self
                        .space_navigation_event(Event::CycleFrom(Some(ROOTS.into()), backwards));
                }
                let mut ids = self.trail_ids();
                ids.extend([
                    TOGGLE.into(),
                    "room-search".into(),
                    "room-composer".into(),
                    "thread-composer".into(),
                    "notifications-unread".into(),
                ]);
                ids.extend(
                    [
                        "nav-message-search",
                        "nav-saved",
                        "nav-invitations",
                        "nav-new-chat",
                        "nav-create-room",
                        "nav-manage-sections",
                    ]
                    .into_iter()
                    .map(String::from),
                );
                ids.extend(
                    self.model
                        .sections
                        .iter()
                        .map(|s| format!("nav-section:{}", s.id)),
                );
                for section in self.model.sections.iter() {
                    ids.extend(
                        self.model
                            .rooms
                            .iter()
                            .map(|r| format!("navigation-room:{}:{}", section.id, r.id)),
                    );
                }
                return iced::advanced::widget::operate(FindFocus {
                    backwards,
                    focused: None,
                    ids,
                });
            }
            Event::CycleFrom(current, backwards) => {
                self.stop_typing();
                self.shell.spaces.menu = None;
                if self.notification_center.open {
                    return if current
                        .as_deref()
                        .is_some_and(|id| self.trail_ids().iter().any(|candidate| candidate == id))
                    {
                        focus("notifications-unread")
                    } else {
                        focus(self.location_focus())
                    };
                }
                let region = current.as_deref().map_or(2, |id| {
                    if self.trail_ids().iter().any(|candidate| candidate == id) {
                        0
                    } else if id == TOGGLE
                        || id == "room-search"
                        || id.starts_with("navigation-room:")
                        || id.starts_with("nav-")
                    {
                        1
                    } else {
                        2
                    }
                });
                match (region + if backwards { 2 } else { 1 }) % 3 {
                    0 => return focus(self.location_focus()),
                    1 => {
                        self.settings.navigation_touched = true;
                        if self
                            .window_width
                            .is_some_and(|width| width < shell::COMPACT_WIDTH)
                        {
                            self.shell.narrow_navigation = true;
                        } else {
                            self.shell.navigation = true;
                        }
                        return focus(TOGGLE);
                    }
                    _ => {
                        return focus(if self.selection.thread.is_some() {
                            "thread-composer"
                        } else {
                            "room-composer"
                        });
                    }
                }
            }
            Event::Open { anchor, parent } => {
                if !self.trail_ids().contains(&anchor)
                    || (!parent.is_empty()
                        && !navigation::valid_space_path(&self.model.spaces, &parent))
                {
                    return Task::none();
                }
                self.stop_typing();
                self.shell.spaces.typed.clear();
                let path = self.location_path();
                self.shell.spaces.expanded.extend(path.iter().cloned());
                self.shell.spaces.menu = Some(Menu {
                    anchor: anchor.clone(),
                    parent,
                });
                let preferred = if let Some(index) = path
                    .iter()
                    .enumerate()
                    .find_map(|(index, _)| (crumb_id(index) == anchor).then_some(index))
                {
                    path[..=index].to_vec()
                } else {
                    path
                };
                let entries = self.menu_entries();
                let selected = entries
                    .iter()
                    .find(|entry| entry.path == preferred)
                    .or_else(|| entries.first());
                return selected.map_or_else(Task::none, |entry| self.focus_entry(&entry.path));
            }
            Event::Choose(path) => {
                if !path.is_empty() && !navigation::valid_space_path(&self.model.spaces, &path) {
                    return Task::none();
                }
                self.shell.spaces.path = path.clone();
                self.shell.spaces.menu = None;
                let scope = path
                    .last()
                    .map_or(Scope::Home, |id| Scope::Space(id.clone()));
                return self
                    .update(Message::Space(scope))
                    .chain(focus(self.location_focus()));
            }
            Event::Expand(path) => {
                if self.shell.spaces.menu.is_none()
                    || !self.menu_entries().iter().any(|entry| entry.path == path)
                {
                    return Task::none();
                }
                let id = path.last().unwrap();
                if !self.shell.spaces.expanded.remove(id) {
                    self.shell.spaces.expanded.insert(id.clone());
                }
                return self.focus_entry(&path);
            }
            Event::ToggleSubspaces => {
                self.pending_navigation = None;
                if self.shell.space == Scope::All {
                    self.shell.space = Scope::Home;
                    self.shell.spaces.include_subspaces.insert(HOME_KEY.into());
                }
                let key = scope_key(&self.shell.space);
                if !self.shell.spaces.include_subspaces.remove(&key) {
                    self.shell.spaces.include_subspaces.insert(key);
                }
            }
            Event::TrailKey(id, key) => {
                let ids = self.trail_ids();
                let Some(index) = ids.iter().position(|candidate| candidate == &id) else {
                    return Task::none();
                };
                match key {
                    Key::Named(Named::ArrowLeft) => {
                        return focus(ids[index.saturating_sub(1)].clone());
                    }
                    Key::Named(Named::ArrowRight) => {
                        return focus(ids[(index + 1).min(ids.len() - 1)].clone());
                    }
                    Key::Named(Named::Home) => return focus(HOME),
                    Key::Named(Named::End) => return focus(ACCOUNT),
                    Key::Named(Named::ArrowDown) => {
                        if id == ACCOUNT {
                            return self.update(Message::ContextMenu(
                                super::context_menu::Event::Open(
                                    super::context_menu::Target::Account,
                                    iced::Point::ORIGIN,
                                ),
                            ));
                        }
                        if id == "space-sidebar" {
                            return self.space_navigation_event(Event::CycleFrom(
                                Some(ROOTS.into()),
                                false,
                            ));
                        }
                        let path = self.location_path();
                        let parent = path
                            .iter()
                            .enumerate()
                            .find_map(|(index, _)| {
                                (crumb_id(index) == id).then(|| path[..index].to_vec())
                            })
                            .unwrap_or_default();
                        return self.space_navigation_event(Event::Open {
                            anchor: if id == HOME { ROOTS.into() } else { id },
                            parent,
                        });
                    }
                    _ => {}
                }
            }
            Event::MenuKey(path, key, modifiers) => {
                if self.shell.spaces.menu.is_none() {
                    return Task::none();
                }
                let entries = self.menu_entries();
                let Some(index) = entries.iter().position(|entry| entry.path == path) else {
                    return Task::none();
                };
                let id = path.last().unwrap();
                match key.as_ref() {
                    Key::Named(Named::ArrowUp | Named::ArrowDown | Named::Home | Named::End) => {
                        let next = match key {
                            Key::Named(Named::ArrowUp) => index.saturating_sub(1),
                            Key::Named(Named::ArrowDown) => (index + 1).min(entries.len() - 1),
                            Key::Named(Named::Home) => 0,
                            _ => entries.len() - 1,
                        };
                        return self.focus_entry(&entries[next].path);
                    }
                    Key::Named(Named::ArrowRight) => {
                        let children = self.child_spaces(&path);
                        if self.shell.spaces.expanded.contains(id) {
                            if let Some(child) = children.first() {
                                let mut next = path;
                                next.push(child.clone());
                                return self.focus_entry(&next);
                            }
                        } else if !children.is_empty() {
                            self.shell.spaces.expanded.insert(id.clone());
                            return self.focus_entry(&path);
                        }
                    }
                    Key::Named(Named::ArrowLeft) => {
                        if self.shell.spaces.expanded.remove(id) {
                            return self.focus_entry(&path);
                        }
                        return self.focus_entry(&path[..path.len() - 1]);
                    }
                    Key::Named(Named::Tab) => {
                        let menu = self.shell.spaces.menu.take().unwrap();
                        let ids = self.trail_ids();
                        let index = ids.iter().position(|id| id == &menu.anchor).unwrap_or(0);
                        let next = if modifiers.shift() {
                            index.saturating_sub(1)
                        } else {
                            (index + 1).min(ids.len() - 1)
                        };
                        return focus(ids[next].clone());
                    }
                    Key::Character(value)
                        if !modifiers.command() && !modifiers.control() && !modifiers.alt() =>
                    {
                        let now = Instant::now();
                        if self
                            .shell
                            .spaces
                            .last_typed
                            .is_none_or(|last| now.duration_since(last).as_millis() > 800)
                        {
                            self.shell.spaces.typed.clear();
                        }
                        self.shell.spaces.last_typed = Some(now);
                        self.shell.spaces.typed.push_str(value);
                        let query = self.shell.spaces.typed.to_lowercase();
                        if let Some(entry) = entries
                            .iter()
                            .cycle()
                            .skip(index + 1)
                            .take(entries.len())
                            .find(|entry| {
                                self.model.spaces.iter().any(|s| {
                                    Some(&s.id) == entry.path.last()
                                        && s.name.to_lowercase().starts_with(&query)
                                })
                            })
                        {
                            return self.focus_entry(&entry.path);
                        }
                    }
                    _ => {}
                }
            }
            Event::Dismiss => {}
        }
        Task::none()
    }

    pub(super) fn space_bar(&self, width: f32) -> Element<'_, Message> {
        let enabled = self.space_navigation_allowed();
        let unread = self.unread_spaces();
        let home = bar_control(
            container(sized(Kind::Home, 21.0)).id(HOME).into(),
            HOME.into(),
            message(Event::Choose(Vec::new())),
            enabled,
        );
        let roots = bar_control(
            container(sized(Kind::ChevronDown, 12.0)).id(ROOTS).into(),
            ROOTS.into(),
            message(Event::Open {
                anchor: ROOTS.into(),
                parent: Vec::new(),
            }),
            enabled,
        );
        let home = super::tooltips::tooltip(home, "Home", iced::widget::tooltip::Position::Bottom);
        let roots = super::tooltips::tooltip(
            roots,
            if cfg!(target_os = "macos") {
                "Browse spaces · Search: Command+Shift+P"
            } else {
                "Browse spaces · Search: Ctrl+Shift+P"
            },
            iced::widget::tooltip::Position::Bottom,
        );
        let mut trail = row![home, roots].spacing(2).align_y(Alignment::Center);
        let path = self.location_path();
        // At narrow widths retain Home, the current location and its menu;
        // the roots dropdown still exposes the complete hierarchy.
        let start = first_crumb(path.len(), width);
        if start > 0 && width >= 500.0 {
            let id = crumb_id(start - 1);
            trail = trail.push(bar_control(
                container(text("…").size(14)).id(id.clone()).into(),
                id.clone(),
                message(Event::Open {
                    anchor: id,
                    parent: Vec::new(),
                }),
                enabled,
            ));
        }
        for (index, id) in path.iter().enumerate().skip(start) {
            let name = self
                .model
                .spaces
                .iter()
                .find(|s| &s.id == id)
                .map_or("Space", |s| s.name.as_str());
            let anchor = crumb_id(index);
            let current = index + 1 == path.len();
            let mut label = row![
                self.space_icon(id),
                super::single_line::label(name, if current { 14 } else { 13 })
                    .shrink()
                    .font(iced::Font {
                        weight: if current {
                            iced::font::Weight::Semibold
                        } else {
                            iced::font::Weight::Normal
                        },
                        ..iced::Font::with_name("Inter")
                    })
                    .style(move |theme| iced::widget::text::Style {
                        color: Some(if current {
                            shell::palette(theme).ink
                        } else {
                            shell::palette(theme).muted
                        }),
                    }),
                sized(Kind::ChevronDown, 11.0)
            ];
            if unread.contains(id) {
                label = label.push(text("•").size(10).style(shell::accent_text));
            }
            let crumb = container(label.spacing(6).align_y(Alignment::Center)).id(anchor.clone());
            // Ancestors stay compact; the current location uses the remaining
            // trail width so its full name is visible whenever it fits.
            let crumb = if current {
                crumb
            } else {
                crumb.max_width((width - 220.0).clamp(40.0, 116.0))
            };
            trail = trail
                .push(sized(Kind::ChevronRight, 12.0))
                .push(bar_control(
                    crumb.into(),
                    anchor.clone(),
                    message(Event::Open {
                        anchor,
                        parent: path[..index].to_vec(),
                    }),
                    enabled,
                ));
        }
        let account_name = self.account_label();
        let image = self
            .model
            .account_avatar_url
            .as_ref()
            .and_then(|uri| self.model.avatars.get(uri));
        let account_action = Message::ContextMenu(super::context_menu::Event::Open(
            super::context_menu::Target::Account,
            iced::Point::new((width - 16.0).max(0.0), 48.0),
        ));
        let mut account = widget_account(
            super::shell::avatar(account_name, image),
            account_action,
            enabled,
        );
        if self.utilities_need_attention() {
            account =
                container(row![account, text("•").size(13).style(shell::accent_text)].spacing(0))
                    .into();
        }
        let toggle = bar_control(
            container(icon(Kind::Sidebar)).into(),
            "space-sidebar".into(),
            Message::ToggleNavigation,
            enabled,
        );
        let toggle = super::tooltips::tooltip(
            toggle,
            if self.navigation_visible(width) {
                "Hide room sidebar"
            } else {
                "Show room sidebar"
            },
            iced::widget::tooltip::Position::Bottom,
        );
        let demo: Element<'_, Message> = if self.demo.is_some() {
            text("Demo").size(10).style(shell::muted_text).into()
        } else {
            iced::widget::space().into()
        };
        container(
            row![
                toggle,
                // Give the breadcrumb trail the width between the fixed controls.
                container(trail).width(Length::Fill).clip(true),
                demo,
                self.notification_bell(),
                account
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([6, 10])
        .style(|theme| shell::surface(theme, shell::palette(theme).rail))
        .into()
    }

    pub(super) fn subspaces_toggle(&self) -> Element<'_, Message> {
        let included = recursive(self, &self.shell.space);
        let action = message(Event::ToggleSubspaces);
        let control = navigation_control::view(
            button(
                container(icon(Kind::Subspaces))
                    .center_x(32)
                    .center_y(32)
                    .id(TOGGLE),
            )
            .padding(0)
            .style(move |theme, status| shell::composer_toggle(theme, included, status))
            .on_press_maybe(self.space_navigation_allowed().then(|| action.clone())),
            TOGGLE,
            action,
            |key, modifiers| {
                (matches!(key, Key::Named(Named::Tab)) && (modifiers - Modifiers::SHIFT).is_empty())
                    .then_some(Message::Focus(modifiers.shift()))
            },
        );
        super::tooltips::tooltip(
            control,
            format!(
                "Include subspaces: {} · {}",
                if included { "on" } else { "off" },
                if cfg!(target_os = "macos") {
                    "Cmd+Shift+L"
                } else {
                    "Ctrl+Shift+L"
                }
            ),
            iced::widget::tooltip::Position::Bottom,
        )
        .into()
    }
    pub(super) fn space_menu_view<'a>(
        &'a self,
        base: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let menu = self
            .shell
            .spaces
            .menu
            .as_ref()
            .filter(|_| self.space_navigation_allowed());
        let content = menu.map(|menu| {
            let heading = menu
                .parent
                .last()
                .and_then(|id| self.model.spaces.iter().find(|s| &s.id == id))
                .map_or_else(|| "Spaces".into(), |s| format!("Spaces in {}", s.name));
            let mut choices = column![].spacing(1);
            let unread = self.unread_spaces();
            for entry in self.menu_entries() {
                let id = entry.path.last().unwrap();
                let name = self
                    .model
                    .spaces
                    .iter()
                    .find(|s| &s.id == id)
                    .map_or("Space", |s| s.name.as_str());
                let has_children = !self.child_spaces(&entry.path).is_empty();
                let expanded = self.shell.spaces.expanded.contains(id);
                let expansion: Element<'_, Message> = if has_children {
                    button(
                        container(sized(
                            if expanded {
                                Kind::ChevronDown
                            } else {
                                Kind::ChevronRight
                            },
                            12.0,
                        ))
                        .center_x(22)
                        .center_y(28),
                    )
                    .padding(0)
                    .style(shell::icon_button)
                    .on_press(message(Event::Expand(entry.path.clone())))
                    .into()
                } else {
                    iced::widget::space().width(22).into()
                };
                let selected = matches!(&self.shell.space, Scope::Space(current) if current == id);
                let action = message(Event::Choose(entry.path.clone()));
                let key_path = entry.path.clone();
                let mut label = row![self.space_icon(id), super::single_line::label(name, 13)]
                    .align_y(Alignment::Center)
                    .spacing(6);
                if unread.contains(id) {
                    label = label.push(text("•").size(10).style(shell::accent_text));
                }
                let control = navigation_control::view(
                    button(label)
                        .width(Length::Fill)
                        .padding([5, 8])
                        .style(move |theme, status| {
                            shell::navigation_button(theme, selected, status)
                        })
                        .on_press(action.clone()),
                    menu_id(&entry.path),
                    action,
                    move |key, modifiers| {
                        if !matches!(key, Key::Named(Named::Tab))
                            && let Some(shortcut) = super::shell_shortcut(key, modifiers)
                        {
                            return Some(shortcut);
                        }
                        if matches!(
                            key,
                            Key::Named(
                                Named::ArrowUp
                                    | Named::ArrowDown
                                    | Named::ArrowLeft
                                    | Named::ArrowRight
                                    | Named::Home
                                    | Named::End
                                    | Named::Tab
                            )
                        ) || matches!(key, Key::Character(_))
                        {
                            Some(message(Event::MenuKey(
                                key_path.clone(),
                                key.clone(),
                                modifiers,
                            )))
                        } else {
                            None
                        }
                    },
                );
                choices = choices.push(
                    container(row![expansion, control].align_y(Alignment::Center)).padding(
                        iced::Padding {
                            left: entry.depth.min(10) as f32 * 12.0,
                            ..Default::default()
                        },
                    ),
                );
            }
            if self.menu_entries().is_empty() {
                choices = choices.push(text("No spaces here").size(13).style(shell::muted_text));
            }
            container(
                column![
                    text(heading).size(12).style(shell::muted_text),
                    scrollable(choices)
                        .id(RESULTS)
                        .height(Length::Shrink)
                        .direction(shell::scrollbar())
                        .style(shell::scroll_style),
                    text("↑↓ move · ←→ expand · Enter open")
                        .size(11)
                        .style(shell::muted_text)
                ]
                .spacing(8),
            )
            .padding(10)
            .width(300)
            .max_height(420)
            .style(|theme| container::Style {
                border: Border {
                    color: shell::palette(theme).line,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..shell::surface(theme, shell::palette(theme).background)
            })
            .into()
        });
        popover::start_aligned(
            base,
            self.shell
                .spaces
                .menu
                .as_ref()
                .map(|menu| menu.anchor.clone().into()),
            content,
            message(Event::Dismiss),
        )
    }
}

fn widget_account<'a>(
    avatar: Element<'a, Message>,
    action: Message,
    enabled: bool,
) -> Element<'a, Message> {
    navigation_control::view(
        container(
            button(avatar)
                .padding(2)
                .style(shell::icon_button)
                .on_press_maybe(enabled.then(|| action.clone())),
        )
        .id("account-menu-button"),
        ACCOUNT,
        action,
        |key, modifiers| keys(ACCOUNT.into(), key, modifiers),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{Shell, layout, mouse};
    use interconnect::navigation::SpaceEntry;

    fn nested_app() -> App {
        let mut app = crate::tests::demo_app();
        app.model.authentication = None;
        app.model.spaces = [
            ("parent", vec!["child", "studio", "maya"]),
            ("other", vec!["child"]),
            ("child", vec!["grandchild", "images"]),
            ("grandchild", vec!["child", "inline-images"]),
        ]
        .into_iter()
        .map(|(id, children)| SpaceEntry {
            id: id.into(),
            name: id.into(),
            children: children.into_iter().map(String::from).collect(),
            ..Default::default()
        })
        .collect::<Vec<_>>()
        .into();
        app.shell.space = Scope::Space("parent".into());
        app.shell.spaces.path = vec!["parent".into()];
        app
    }
    fn act(app: &mut App, event: Event) {
        let _ = app.update(message(event));
    }

    #[test]
    fn recursive_toggle_and_home_preserve_conversation_thread_drafts_and_scroll() {
        let mut app = nested_app();
        let _ = app.update(Message::Thread("proposal".into()));
        let selected = (app.selection.room.clone(), app.selection.thread.clone());
        let key = app.draft_key(true).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Keep this draft");
        app.scroll_positions.insert(key.clone(), 89.0);
        assert!(app.visible_rooms().contains(&"studio".into()));
        assert!(!app.visible_rooms().contains(&"images".into()));
        let shortcut = super::super::shell_shortcut(
            &Key::Character("L".into()),
            Modifiers::COMMAND | Modifiers::SHIFT,
        )
        .unwrap();
        let _ = app.update(shortcut);
        assert!(app.visible_rooms().contains(&"images".into()));
        assert!(app.visible_rooms().contains(&"inline-images".into()));
        act(
            &mut app,
            Event::Choose(vec!["other".into(), "child".into()]),
        );
        assert_eq!(app.location_path(), ["other", "child"]);
        assert!(!recursive(&app, &app.shell.space));
        assert!(!app.visible_rooms().contains(&"inline-images".into()));
        act(&mut app, Event::Choose(Vec::new()));
        assert!(
            !app.visible_rooms().contains(&"maya".into()),
            "space-linked DMs follow Home's filter"
        );
        assert!(!app.visible_rooms().contains(&"studio".into()));
        act(&mut app, Event::ToggleSubspaces);
        assert_eq!(app.visible_rooms().len(), app.model.rooms.len());

        let mut legacy_direct = App::new();
        let mut model = crate::tests::demo_app().model;
        model.saved_navigation = Some(interconnect::navigation_preferences::Preferences {
            scope: Scope::Direct,
            ..Default::default()
        });
        let _ = legacy_direct.update(Message::State(Box::new(model)));
        assert_eq!(legacy_direct.shell.space, Scope::Home);
        act(&mut app, Event::Choose(vec!["parent".into()]));
        assert!(
            recursive(&app, &app.shell.space),
            "each location remembers its own toggle"
        );
        assert_eq!(
            (app.selection.room.clone(), app.selection.thread.clone()),
            selected
        );
        assert_eq!(app.drafts[&key].content.text().trim(), "Keep this draft");
        assert_eq!(app.scroll_positions[&key], 89.0);
        let shown: HashSet<_> = app.visible_rooms().into_iter().collect();
        assert_eq!(shown.len(), app.visible_rooms().len());
    }

    #[test]
    fn a_priority_confirmation_cannot_leave_a_breadcrumb_menu_above_it() {
        let mut app = nested_app();
        act(
            &mut app,
            Event::Open {
                anchor: crumb_id(0),
                parent: Vec::new(),
            },
        );
        assert!(app.shell.spaces.menu.is_some());
        {
            let view = app.space_menu_view(iced::widget::space());
            assert!(!view.as_widget().children()[1].children.is_empty());
        }
        app.sign_out_open = true;
        let view = app.space_menu_view(iced::widget::space());
        assert!(view.as_widget().children()[1].children.is_empty());
    }

    #[test]
    fn menus_keep_siblings_expand_without_navigating_and_reject_stale_links() {
        let mut app = nested_app();
        let scope = app.shell.space.clone();
        act(
            &mut app,
            Event::Open {
                anchor: crumb_id(0),
                parent: Vec::new(),
            },
        );
        let entries = app.menu_entries();
        assert!(entries.iter().any(|e| e.path == ["other"]));
        assert!(entries.iter().any(|e| e.path == ["parent", "child"]));
        act(
            &mut app,
            Event::MenuKey(
                vec!["parent".into(), "child".into()],
                Key::Named(Named::ArrowRight),
                Modifiers::default(),
            ),
        );
        assert!(app.shell.spaces.expanded.contains("child"));
        assert!(
            app.menu_entries()
                .iter()
                .any(|e| e.path == ["parent", "child", "grandchild"])
        );
        assert_eq!(app.shell.space, scope);
        assert!(
            app.menu_entries()
                .iter()
                .all(|e| navigation::valid_space_path(&app.model.spaces, &e.path))
        );
        act(&mut app, Event::Dismiss);
        assert!(app.shell.spaces.menu.is_none());
        let mut state = app.model.clone();
        std::sync::Arc::make_mut(&mut state.spaces)
            .iter_mut()
            .find(|s| s.id == "parent")
            .unwrap()
            .children
            .clear();
        let _ = app.update(Message::State(Box::new(state)));
        act(
            &mut app,
            Event::Choose(vec!["parent".into(), "child".into()]),
        );
        assert_eq!(
            app.shell.space, scope,
            "a removed parent-child relation cannot dispatch old navigation"
        );
        app.composing = true;
        let included = recursive(&app, &app.shell.space);
        let shortcut = super::super::shell_shortcut(
            &Key::Character("L".into()),
            Modifiers::COMMAND | Modifiers::SHIFT,
        )
        .unwrap();
        let _ = app.update(shortcut);
        assert_eq!(
            recursive(&app, &app.shell.space),
            included,
            "IME blocks the global toggle shortcut"
        );
        act(&mut app, Event::Choose(Vec::new()));
        assert_eq!(app.shell.space, scope, "IME composition takes precedence");
        app.composing = false;
        act(
            &mut app,
            Event::Open {
                anchor: ROOTS.into(),
                parent: Vec::new(),
            },
        );
        assert!(app.shell.spaces.menu.is_some());
        let _ = app.update(Message::GlobalSearch(
            super::super::global_search::Event::Open,
        ));
        assert!(
            app.shell.spaces.menu.is_none(),
            "existing view shortcuts dismiss the space menu"
        );
    }

    #[test]
    fn old_all_preferences_restore_as_recursive_home_and_new_routes_round_trip() {
        let mut app = App::new();
        let mut model = crate::tests::demo_app().model;
        model.saved_navigation = Some(interconnect::navigation_preferences::Preferences {
            scope: Scope::All,
            ..Default::default()
        });
        let _ = app.update(Message::State(Box::new(model)));
        assert_eq!(app.shell.space, Scope::Home);
        assert!(recursive(&app, &Scope::Home));
        assert_eq!(app.visible_rooms().len(), app.model.rooms.len());

        let mut app = nested_app();
        app.demo = None;
        app.preferences_ready = true;
        act(
            &mut app,
            Event::Choose(vec!["other".into(), "child".into()]),
        );
        act(&mut app, Event::ToggleSubspaces);
        let saved = app.selection.preferences.clone().unwrap();
        assert_eq!(saved.space_path, ["other", "child"]);
        assert!(saved.include_subspaces.contains(&"child".into()));
        let mut fresh = App::new();
        let mut model = app.model.clone();
        model.saved_navigation = Some(saved);
        let _ = fresh.update(Message::State(Box::new(model)));
        assert_eq!(fresh.location_path(), ["other", "child"]);
        assert!(recursive(&fresh, &fresh.shell.space));
    }

    #[test]
    fn pending_metadata_keeps_the_saved_shared_parent_route_and_unread_propagates_through_cycles() {
        let app = nested_app();
        let mut fresh = App::new();
        let saved = interconnect::navigation_preferences::Preferences {
            scope: Scope::Space("child".into()),
            space_path: vec!["other".into(), "child".into()],
            ..Default::default()
        };
        let mut model = app.model.clone();
        model.spaces = Default::default();
        model.saved_navigation = Some(saved.clone());
        let _ = fresh.update(Message::State(Box::new(model)));
        assert!(fresh.pending_navigation.is_some());
        let mut model = app.model.clone();
        model.saved_navigation = Some(saved);
        let _ = fresh.update(Message::State(Box::new(model)));
        assert_eq!(fresh.location_path(), ["other", "child"]);

        let mut app = app;
        for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
            room.unread = room.id == "inline-images";
        }
        assert_eq!(
            app.unread_spaces(),
            ["parent", "other", "child", "grandchild"]
                .into_iter()
                .map(String::from)
                .collect()
        );
        for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
            room.unread = false;
        }
        assert!(app.unread_spaces().is_empty());
    }

    #[test]
    fn breadcrumb_bar_keeps_home_account_and_trail_accessible_at_narrow_widths() {
        #[derive(Default)]
        struct Probe {
            toggle: Option<iced::Rectangle>,
            home: Option<iced::Rectangle>,
            bell: Option<iced::Rectangle>,
            account: Option<iced::Rectangle>,
            crumbs: Vec<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn focusable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _state: &mut dyn widget::operation::Focusable,
            ) {
                if id == Some(&"space-sidebar".into()) {
                    self.toggle = Some(bounds);
                }
            }
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn widget::Operation)) {
                visit(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&HOME.into()) {
                    self.home = Some(bounds);
                }
                if id == Some(&"account-menu-button".into()) {
                    self.account = Some(bounds);
                }
                if id == Some(&"notification-bell-button".into()) {
                    self.bell = Some(bounds);
                }
                for index in 0..3 {
                    if id == Some(&crumb_id(index).into()) {
                        self.crumbs.push(bounds);
                    }
                }
            }
        }
        let mut app = nested_app();
        act(
            &mut app,
            Event::Choose(vec!["parent".into(), "child".into(), "grandchild".into()]),
        );
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "grandchild")
            .unwrap()
            .name = "Current space with a longer name".into();
        app.notification_center.records = (0..128)
            .map(|index| interconnect::notifications::Record {
                entry: interconnect::notifications::Entry {
                    id: format!("notification-{index}"),
                    target: interconnect::notifications::Target::Conversation {
                        event: None,
                        room: "studio".into(),
                        thread: None,
                    },
                    timestamp: 0,
                    title: "Unread notification".into(),
                    body: String::new(),
                },
                read: false,
            })
            .collect();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for width in [320.0, 640.0, 800.0, 1400.0] {
            let size = iced::Size::new(width, 56.0);
            let mut view = app.space_bar(width);
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut probe = Probe::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            let home = probe.home.unwrap();
            let account = probe.account.unwrap();
            let bell = probe.bell.unwrap();
            let toggle = probe.toggle.unwrap();
            assert!(toggle.width >= 20.0 && toggle.x < 30.0);
            assert!(home.width >= 20.0 && home.x >= toggle.x + toggle.width);
            assert!(
                account.width >= 30.0 && account.x > home.x,
                "account squeezed at {width}: {account:?}; crumbs: {:?}",
                probe.crumbs
            );
            assert!(
                account.x + account.width <= width,
                "account clipped at {width}: {account:?}"
            );
            let current = probe.crumbs.last().unwrap();
            assert!(
                current.x + current.width < bell.x && bell.x + bell.width <= account.x,
                "notification bell overlaps the breadcrumb/account at {width}"
            );
            if width >= 800.0 {
                assert!(
                    current.width > 200.0,
                    "current name is still capped at {width}: {current:?}"
                );
            }
            for crumb in probe.crumbs.iter().take(probe.crumbs.len() - 1) {
                assert!(
                    crumb.width <= 130.0,
                    "breadcrumb stretched at {width}: {crumb:?}"
                );
            }
            for pair in probe.crumbs.windows(2) {
                assert!(
                    pair[1].x - pair[0].x - pair[0].width < 35.0,
                    "breadcrumb trail has an unexplained gap"
                );
            }
        }
    }

    #[test]
    fn popup_clamps_to_window_and_escape_captures_once_without_closing_conversation() {
        let mut app = nested_app();
        act(
            &mut app,
            Event::Open {
                anchor: ROOTS.into(),
                parent: Vec::new(),
            },
        );
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(640.0, 480.0);
        let messages = {
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut overlay = view
                .as_widget_mut()
                .overlay(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &iced::Rectangle::with_size(size),
                    iced::Vector::ZERO,
                )
                .unwrap();
            let overlay = overlay.as_overlay_mut();
            let node = overlay.layout(&renderer, size);
            let bounds = node.children()[0].bounds();
            assert!(bounds.x >= 8.0 && bounds.y >= 8.0);
            assert!(bounds.x + bounds.width <= 632.0 && bounds.y + bounds.height <= 472.0);
            let event = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: Key::Named(Named::Escape),
                modified_key: Key::Named(Named::Escape),
                physical_key: iced::keyboard::key::Physical::Code(
                    iced::keyboard::key::Code::Escape,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers: Modifiers::default(),
                text: None,
                repeat: false,
            });
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            overlay.update(
                &event,
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
            );
            assert!(shell.is_event_captured());
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            messages
        };
        assert!(matches!(
            messages.as_slice(),
            [Message::SpaceNavigation(Event::Dismiss)]
        ));
        for event in messages {
            let _ = app.update(event);
        }
        assert!(app.shell.spaces.menu.is_none());
        assert!(app.selection.room.is_some());
    }
}
