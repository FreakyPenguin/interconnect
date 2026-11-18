//! Local room and space quick-open. No SDK requests or polling.
use super::{App, Message, shell, single_line, space_navigation};
use iced::{
    Element, Length, Task,
    keyboard::{Key, Modifiers, key::Named},
    widget::{button, column, container, mouse_area, opaque, row, scrollable, text, text_input},
};
use interconnect::navigation::{self, RoomEntry, SpaceEntry};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

const SEARCH: &str = "quick-room-search";
const RESULTS: &str = "quick-room-results";
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Rooms,
    Spaces,
}
#[derive(Default)]
pub struct State {
    mode: Mode,
    query: String,
    selected: usize,
    thread: bool,
    spaces: Option<SpaceIndex>,
}
struct SpaceCandidate {
    path: Vec<String>,
    name: String,
    trail: String,
    folded_name: String,
    folded_trail: String,
}
struct SpaceIndex {
    snapshot: Arc<Vec<SpaceEntry>>,
    preferred: Vec<String>,
    account: Option<String>,
    entries: Vec<SpaceCandidate>,
}
impl SpaceIndex {
    fn new(
        snapshot: Arc<Vec<SpaceEntry>>,
        preferred: Vec<String>,
        account: Option<String>,
    ) -> Self {
        // Build canonical routes once per metadata snapshot, never per keystroke.
        let by_id: HashMap<_, _> = snapshot.iter().map(|space| (&space.id, space)).collect();
        let mut paths = HashMap::new();
        let mut pending = VecDeque::new();
        for id in navigation::space_roots(&snapshot) {
            paths.insert(id.clone(), vec![id.clone()]);
            pending.push_back(id);
        }
        while let Some(id) = pending.pop_front() {
            for child in &by_id[&id].children {
                if by_id.contains_key(child) && !paths.contains_key(child) {
                    let mut path = paths[&id].clone();
                    path.push(child.clone());
                    paths.insert(child.clone(), path);
                    pending.push_back(child.clone());
                }
            }
        }
        // Retain the route used to reach the current space and its ancestors.
        if navigation::valid_space_path(&snapshot, &preferred) {
            for (index, id) in preferred.iter().enumerate() {
                paths.insert(id.clone(), preferred[..=index].to_vec());
            }
        }
        let mut entries = vec![SpaceCandidate {
            path: Vec::new(),
            name: "Home".into(),
            trail: "Home".into(),
            folded_name: "home".into(),
            folded_trail: "home".into(),
        }];
        for space in snapshot.iter() {
            let Some(path) = paths.remove(&space.id) else {
                continue;
            };
            let trail = std::iter::once("Home")
                .chain(path.iter().map(|id| by_id[id].name.as_str()))
                .collect::<Vec<_>>()
                .join(" › ");
            entries.push(SpaceCandidate {
                path,
                name: space.name.clone(),
                folded_name: space.name.to_lowercase(),
                folded_trail: trail.to_lowercase(),
                trail,
            });
        }
        Self {
            snapshot,
            preferred,
            account,
            entries,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    OpenSpaces,
    Refocus,
    Edit(String),
    Move(bool),
    Submit,
    Choose(String),
    ChooseSpace(Vec<String>),
    Close,
    Ignore,
}
fn message(event: Event) -> Message {
    Message::QuickRoom(event)
}

// Prefer exact, prefix and substring matches over loose subsequences.
fn score(label: &str, query: &str) -> Option<(u8, usize)> {
    if query.is_empty() {
        return Some((0, 0));
    }
    if label == query {
        return Some((0, 0));
    }
    if label.starts_with(query) {
        return Some((1, label.len()));
    }
    if let Some(start) = label.find(query) {
        return Some((2, start));
    }
    let mut remaining = label.chars().enumerate();
    let mut first = None;
    let mut last = 0;
    for wanted in query.chars() {
        let (index, _) = remaining.find(|(_, ch)| *ch == wanted)?;
        first.get_or_insert(index);
        last = index;
    }
    Some((3, last - first.unwrap_or(0)))
}
impl State {
    fn space_matches(&self) -> Vec<&SpaceCandidate> {
        let Some(spaces) = &self.spaces else {
            return Vec::new();
        };
        let query = self.query.trim().to_lowercase();
        let mut matches: Vec<_> = spaces
            .entries
            .iter()
            .filter_map(|space| {
                let rank = [
                    score(&space.folded_name, &query).map(|(kind, distance)| (kind, 0, distance)),
                    score(&space.folded_trail, &query).map(|(kind, distance)| (kind, 1, distance)),
                ]
                .into_iter()
                .flatten()
                .min()?;
                Some((rank, space))
            })
            .collect();
        matches.sort_by(|a, b| {
            // Keep the root first when browsing without a query.
            let key = |space: &SpaceCandidate| query.is_empty() && !space.path.is_empty();
            (
                key(a.1),
                &a.0,
                &a.1.folded_name,
                &a.1.folded_trail,
                &a.1.path,
            )
                .cmp(&(
                    key(b.1),
                    &b.0,
                    &b.1.folded_name,
                    &b.1.folded_trail,
                    &b.1.path,
                ))
        });
        matches.into_iter().map(|(_, space)| space).collect()
    }

    fn result_count(&self, rooms: &[RoomEntry]) -> usize {
        match self.mode {
            Mode::Rooms => self.matches(rooms).len(),
            Mode::Spaces => self.space_matches().len(),
        }
    }

    fn scroll_to_selection(&self, count: usize) -> Task<Message> {
        iced::widget::operation::snap_to(
            RESULTS,
            iced::widget::operation::RelativeOffset {
                x: 0.0,
                y: self.selected as f32 / count.saturating_sub(1).max(1) as f32,
            },
        )
    }

    fn matches<'a>(&self, rooms: &'a [RoomEntry]) -> Vec<&'a RoomEntry> {
        let query = self.query.trim().to_lowercase();
        let mut matches: Vec<_> = rooms
            .iter()
            .filter_map(|room| {
                let name = room.name.to_lowercase();
                let rank =
                    score(&name, &query).or_else(|| score(&room.id.to_lowercase(), &query))?;
                Some((rank, name, room))
            })
            .collect();
        matches.sort_by(|a, b| (&a.0, &a.1, &a.2.id).cmp(&(&b.0, &b.1, &b.2.id)));
        matches.into_iter().map(|(_, _, room)| room).collect()
    }
}
pub fn shortcut(
    event: iced::Event,
    _: iced::event::Status,
    _: iced::window::Id,
) -> Option<Message> {
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event
    else {
        return None;
    };
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(message(Event::Close)),
        Key::Named(Named::ArrowUp) => Some(message(Event::Move(true))),
        Key::Named(Named::ArrowDown) => Some(message(Event::Move(false))),
        // Enter is submitted by the text input, avoiding a duplicate action.
        Key::Named(Named::Tab) => Some(message(Event::Refocus)),
        Key::Character("p" | "P") if is_shortcut(&key, modifiers) => Some(message(Event::Open)),
        Key::Character("p" | "P") if is_space_shortcut(&key, modifiers) => {
            Some(message(Event::OpenSpaces))
        }
        _ => None,
    }
}
pub fn is_shortcut(key: &Key, modifiers: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character("p" | "P"))
        && (modifiers.control() || modifiers.command())
        && !modifiers.alt()
        && !modifiers.shift()
}
pub fn is_space_shortcut(key: &Key, modifiers: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character("p" | "P"))
        && (modifiers.control() || modifiers.command())
        && !modifiers.alt()
        && modifiers.shift()
}
impl App {
    pub(super) fn refresh_space_picker(&mut self) -> Task<Message> {
        let Some(state) = &self.room_picker else {
            return Task::none();
        };
        if state.mode != Mode::Spaces {
            return Task::none();
        }
        let index = state.spaces.as_ref().unwrap();
        if index.account != self.account_id
            || self.closing
            || self.stopped
            || (self.demo.is_none() && self.model.authentication.is_some())
        {
            self.room_picker = None;
            return Task::none();
        }
        let preferred = self.location_path();
        if Arc::ptr_eq(&index.snapshot, &self.model.spaces) && index.preferred == preferred {
            return Task::none();
        }
        let selected = state
            .space_matches()
            .get(state.selected)
            .map(|s| s.path.last().cloned());
        let index = SpaceIndex::new(
            self.model.spaces.clone(),
            preferred,
            self.account_id.clone(),
        );
        let state = self.room_picker.as_mut().unwrap();
        state.spaces = Some(index);
        let matches = state.space_matches();
        let count = matches.len();
        state.selected = selected
            .and_then(|id| matches.iter().position(|s| s.path.last() == id.as_ref()))
            .unwrap_or(0);
        state.scroll_to_selection(count)
    }

    pub(super) fn validate_room_focus(&mut self) {
        if self.pending_room_focus.is_some_and(|generation| {
            self.selection
                .room
                .as_ref()
                .is_none_or(|(current, _)| *current != generation)
                || !self.window_focused
                || self.closing
                || self.stopped
                || self.room_picker.is_some()
                || self.create_room.is_some()
                || self.new_chat.is_some()
                || self.settings.draft.is_some()
                || self.verification.open
                || self.image_view.is_some()
                || self.reaction_picker.is_some()
                || self.composer_picker.is_some()
                || self.emoji_management.is_some()
                || self.invite.is_some()
                || self.topic_editor.is_some()
                || self.room_search.open
                || self.bookmarks.open
                || self.global_search.open
                || self.selection.members.is_some()
                || (self.demo.is_none() && self.model.authentication.is_some())
        }) {
            self.pending_room_focus = None;
        }
    }

    fn take_room_focus(&mut self) -> Option<impl iced::advanced::widget::Operation + use<>> {
        self.validate_room_focus();
        let generation = self.pending_room_focus?;
        let pane = super::current_pane(&self.model.room, generation)?;
        self.pending_room_focus = None;
        if pane.error.is_some() {
            return None;
        }
        Some(iced::advanced::widget::operation::focusable::focus(
            "room-composer".into(),
        ))
    }

    pub(super) fn restore_room_focus(&mut self) -> Task<Message> {
        self.take_room_focus()
            .map_or_else(Task::none, iced::advanced::widget::operate)
            .discard()
    }

    pub(super) fn quick_room_event(&mut self, event: Event) -> Task<Message> {
        let _ = self.refresh_space_picker();
        match event {
            Event::Open | Event::OpenSpaces => {
                if self.create_room.is_some()
                    || self.new_chat.is_some()
                    || self.closing
                    || self.stopped
                    || self.settings.draft.is_some()
                    || self.verification.open
                    || self.image_view.is_some()
                    || self.reaction_picker.is_some()
                    || self.composer_picker.is_some()
                    || self.invite.is_some()
                    || self.topic_editor.is_some()
                    || self.composing
                    || (self.demo.is_none() && self.model.authentication.is_some())
                {
                    return Task::none();
                }
                let mode = if matches!(event, Event::OpenSpaces) {
                    Mode::Spaces
                } else {
                    Mode::Rooms
                };
                let reset = self
                    .room_picker
                    .as_ref()
                    .is_none_or(|state| state.mode != mode);
                if reset {
                    self.stop_typing();
                    self.emoji_completion.clear();
                    self.user_completion.clear();
                    self.room_picker = Some(State {
                        mode,
                        thread: self.selection.thread.is_some(),
                        spaces: (mode == Mode::Spaces).then(|| {
                            SpaceIndex::new(
                                self.model.spaces.clone(),
                                self.location_path(),
                                self.account_id.clone(),
                            )
                        }),
                        ..State::default()
                    });
                }
                let focus = iced::widget::operation::focus(SEARCH);
                return if reset {
                    Task::batch([
                        focus,
                        iced::widget::operation::snap_to(
                            RESULTS,
                            iced::widget::operation::RelativeOffset::START,
                        ),
                    ])
                } else {
                    focus
                };
            }
            Event::Refocus => {
                if self.room_picker.is_some() {
                    return iced::widget::operation::focus(SEARCH);
                }
            }
            Event::Close => {
                if let Some(state) = self.room_picker.take() {
                    return iced::widget::operation::focus(if state.thread {
                        "thread-composer"
                    } else {
                        "room-composer"
                    });
                }
            }
            Event::Edit(query) => {
                if let Some(state) = &mut self.room_picker {
                    state.query = query;
                    state.selected = 0;
                    return iced::widget::operation::snap_to(
                        RESULTS,
                        iced::widget::operation::RelativeOffset::START,
                    );
                }
            }
            Event::Move(previous) => {
                if let Some(state) = &mut self.room_picker {
                    let count = state.result_count(&self.model.rooms);
                    if count > 0 {
                        let index = state.selected.min(count - 1);
                        state.selected = if previous {
                            (index + count - 1) % count
                        } else {
                            (index + 1) % count
                        };
                        return state.scroll_to_selection(count);
                    }
                }
            }
            Event::Submit | Event::Choose(_) | Event::ChooseSpace(_) => {
                let Some(state) = &self.room_picker else {
                    return Task::none();
                };
                if state.mode == Mode::Spaces {
                    let matches = state.space_matches();
                    let target = match &event {
                        Event::Submit => matches
                            .get(state.selected.min(matches.len().saturating_sub(1)))
                            .copied(),
                        Event::ChooseSpace(path) => {
                            matches.iter().copied().find(|space| &space.path == path)
                        }
                        _ => None,
                    }
                    .map(|space| space.path.clone());
                    if let Some(path) = target
                        && (path.is_empty()
                            || navigation::valid_space_path(&self.model.spaces, &path))
                    {
                        self.room_picker = None;
                        return self.update(Message::SpaceNavigation(
                            space_navigation::Event::Choose(path),
                        ));
                    }
                    return Task::none();
                }
                if matches!(event, Event::ChooseSpace(_)) {
                    return Task::none();
                }
                let matches = state.matches(&self.model.rooms);
                let target = if let Event::Choose(id) = event {
                    matches
                        .iter()
                        .find(|room| room.id == id)
                        .map(|room| room.id.clone())
                } else {
                    matches
                        .get(state.selected.min(matches.len().saturating_sub(1)))
                        .map(|room| room.id.clone())
                };
                if let Some(id) = target {
                    self.room_picker = None;
                    let task = if self
                        .selection
                        .room
                        .as_ref()
                        .is_some_and(|(_, current)| current == &id)
                    {
                        self.pending_room_focus = self
                            .selection
                            .room
                            .as_ref()
                            .map(|(generation, _)| *generation);
                        Task::none()
                    } else {
                        self.update(Message::Room(id))
                    };
                    return task;
                }
            }
            Event::Ignore => {}
        }
        Task::none()
    }
    pub(super) fn quick_room_view(&self) -> Element<'_, Message> {
        let state = self.room_picker.as_ref().unwrap();
        let matches = if state.mode == Mode::Rooms {
            state.matches(&self.model.rooms)
        } else {
            Vec::new()
        };
        let spaces = state.space_matches();
        let count = matches.len() + spaces.len();
        let selected = state.selected.min(count.saturating_sub(1));
        let mut results = column![].spacing(2);
        for (index, space) in spaces.iter().enumerate() {
            let active = index == selected;
            let icon = space.path.last().map_or_else(
                || super::icons::sized(super::icons::Kind::Home, 20.0),
                |id| self.space_icon(id),
            );
            results = results.push(
                button(
                    row![
                        icon,
                        column![
                            single_line::label(&space.name, 14),
                            single_line::label(&space.trail, 11).style(shell::muted_text),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                )
                .width(Length::Fill)
                .padding([8, 12])
                .on_press(message(Event::ChooseSpace(space.path.clone())))
                .style(move |theme, status| {
                    let mut style = shell::quiet_button(theme, status);
                    style.text_color = shell::palette(theme).ink;
                    if active {
                        style.background = Some(shell::palette(theme).selected.into());
                    }
                    style
                }),
            );
        }
        for (index, room) in matches.iter().enumerate() {
            let active = index == selected;
            let label = column![
                text(&room.name).size(14),
                text(&room.id).size(11).style(shell::muted_text)
            ]
            .spacing(2);
            results = results.push(
                button(
                    row![
                        text(if room.direct { "@" } else { "#" }).style(shell::muted_text),
                        label
                    ]
                    .spacing(12),
                )
                .width(Length::Fill)
                .padding([8, 12])
                .on_press(message(Event::Choose(room.id.clone())))
                .style(move |theme, status| {
                    let mut style = shell::quiet_button(theme, status);
                    style.text_color = shell::palette(theme).ink;
                    if active {
                        style.background = Some(shell::palette(theme).selected.into());
                    }
                    style
                }),
            );
        }
        if count == 0 {
            results = results.push(
                container(
                    text(if state.mode == Mode::Spaces {
                        "No matching spaces"
                    } else {
                        "No matching rooms"
                    })
                    .size(13)
                    .style(shell::muted_text),
                )
                .padding(16),
            );
        }
        let card = container(
            column![
                text_input(
                    if state.mode == Mode::Spaces {
                        "Switch to space…"
                    } else {
                        "Switch to room…"
                    },
                    &state.query
                )
                .id(SEARCH)
                .cursor_blinking(false)
                .size(16)
                .padding(12)
                .on_input(|query| message(Event::Edit(query)))
                .on_submit(message(Event::Submit)),
                scrollable(results)
                    .id(RESULTS)
                    .height(Length::Fill)
                    .direction(shell::scrollbar())
                    .style(shell::scroll_style),
                text(format!(
                    "{} {}{}   ·   ↑ ↓ select   ·   Enter open   ·   Esc close",
                    count,
                    if state.mode == Mode::Spaces {
                        "location"
                    } else {
                        "room"
                    },
                    if count == 1 { "" } else { "s" },
                ))
                .size(11)
                .style(shell::muted_text)
            ]
            .spacing(10),
        )
        .padding(12)
        .width(560)
        .height(420)
        .max_height(420)
        .style(|theme| container::Style {
            border: iced::Border::default().rounded(12),
            ..shell::surface(theme, shell::palette(theme).background)
        });
        opaque(
            mouse_area(
                container(opaque(mouse_area(card).on_press(message(Event::Ignore))))
                    .center_x(Length::Fill)
                    .height(Length::Fill)
                    .padding([48, 16])
                    .style(|theme| shell::surface(theme, shell::palette(theme).scrim)),
            )
            .on_press(message(Event::Close))
            .on_scroll(|_| message(Event::Ignore)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DraftKey, tests::demo_app};
    use super::*;
    use iced::widget::text_editor::{Action, Edit};

    fn space_app() -> App {
        let mut app = demo_app();
        let spaces = Arc::make_mut(&mut app.model.spaces);
        spaces
            .iter_mut()
            .find(|s| s.id == "home-space")
            .unwrap()
            .children
            .push("personal-design".into());
        spaces.push(SpaceEntry {
            id: "personal-design".into(),
            name: "Design".into(),
            ..Default::default()
        });
        spaces
            .iter_mut()
            .find(|s| s.id == "mobile-space")
            .unwrap()
            .children
            .push("design-space".into());
        spaces
            .iter_mut()
            .find(|s| s.id == "community-space")
            .unwrap()
            .children
            .push("design-space".into());
        app
    }

    fn picker_key(key: Key, modifiers: Modifiers) -> iced::Event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn space_search_disambiguates_names_and_matches_breadcrumbs() {
        let mut app = space_app();
        let _ = app.update(message(Event::OpenSpaces));
        let state = app.room_picker.as_ref().unwrap();
        assert!(
            state.space_matches()[0].path.is_empty(),
            "Home is first without a query"
        );
        let _ = app.update(message(Event::Edit(" DESIGN ".into())));
        let matches = app.room_picker.as_ref().unwrap().space_matches();
        assert_eq!(matches[0].name, "Design");
        assert_eq!(matches[1].name, "Design");
        assert_ne!(matches[0].path, matches[1].path);
        assert_ne!(matches[0].trail, matches[1].trail);
        let _ = app.update(message(Event::Edit("personal design".into())));
        let matches = app.room_picker.as_ref().unwrap().space_matches();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].trail, "Home › Personal › Design");
        let _ = app.update(message(Event::Edit("prs ds".into())));
        assert_eq!(
            app.room_picker.as_ref().unwrap().space_matches()[0]
                .path
                .last()
                .unwrap(),
            "personal-design"
        );
        let _ = app.update(message(Event::Edit("no matching location".into())));
        let _ = app.update(message(Event::Move(true)));
        let _ = app.update(message(Event::Submit));
        assert!(app.room_picker.as_ref().unwrap().space_matches().is_empty());
        let _ = app.update(message(Event::Edit("home".into())));
        assert!(
            app.room_picker.as_ref().unwrap().space_matches()[0]
                .path
                .is_empty()
        );
    }

    #[test]
    fn space_index_handles_cycles_shared_children_and_preferred_routes_once() {
        let mut app = space_app();
        app.shell.space = navigation::Scope::Space("design-space".into());
        app.shell.spaces.path = vec![
            "studio-space".into(),
            "product-space".into(),
            "design-space".into(),
        ];
        let _ = app.update(message(Event::OpenSpaces));
        let index = app.room_picker.as_ref().unwrap().spaces.as_ref().unwrap();
        assert_eq!(index.entries.len(), app.model.spaces.len() + 1);
        let design = index
            .entries
            .iter()
            .find(|s| s.path.last().is_some_and(|id| id == "design-space"))
            .unwrap();
        assert_eq!(design.trail, "Home › Studio › Product › Design");
        assert_eq!(design.path, app.shell.spaces.path);
        for entry in &index.entries {
            assert!(
                entry.path.is_empty()
                    || navigation::valid_space_path(&app.model.spaces, &entry.path)
            );
        }
        let snapshot = index.snapshot.clone();
        let entries = index.entries.as_ptr();
        let _ = app.update(message(Event::Edit("design".into())));
        let index = app.room_picker.as_ref().unwrap().spaces.as_ref().unwrap();
        assert!(Arc::ptr_eq(&snapshot, &index.snapshot));
        assert_eq!(
            entries,
            index.entries.as_ptr(),
            "typing must not rebuild the graph"
        );
    }

    #[test]
    fn choosing_or_canceling_spaces_preserves_conversation_thread_draft_and_scroll() {
        for click in [false, true] {
            let mut app = space_app();
            let _ = app.update(Message::Thread("proposal".into()));
            let selected = (app.selection.room.clone(), app.selection.thread.clone());
            let key = app.draft_key(true).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("Unsent thread draft");
            app.scroll_positions.insert(key.clone(), 123.0);
            app.shell.query = "studio".into();
            let scope = app.shell.space.clone();
            let _ = app.update(message(Event::OpenSpaces));
            let _ = app.update(message(Event::Edit("personal design".into())));
            let _ = app.update(message(Event::Close));
            assert_eq!(app.shell.space, scope);
            assert_eq!(app.shell.query, "studio");
            let _ = app.update(message(Event::OpenSpaces));
            let _ = app.update(message(Event::Edit("personal design".into())));
            let _ = app.update(message(if click {
                Event::ChooseSpace(vec!["home-space".into(), "personal-design".into()])
            } else {
                Event::Submit
            }));
            assert!(app.room_picker.is_none());
            assert_eq!(
                app.shell.space,
                navigation::Scope::Space("personal-design".into())
            );
            assert_eq!(app.location_path(), ["home-space", "personal-design"]);
            assert_eq!(
                (app.selection.room.clone(), app.selection.thread.clone()),
                selected
            );
            assert_eq!(app.drafts[&key].content.text(), "Unsent thread draft");
            assert_eq!(app.scroll_positions[&key], 123.0);
            assert!(
                app.shell.query.is_empty(),
                "choosing a space clears the sidebar query like the breadcrumb menus"
            );
            let _ = app.update(message(Event::OpenSpaces));
            let _ = app.update(message(Event::Edit("home".into())));
            let _ = app.update(message(Event::Submit));
            assert_eq!(app.shell.space, navigation::Scope::Home);
            assert_eq!(
                (app.selection.room.clone(), app.selection.thread.clone()),
                selected
            );
        }
    }

    #[test]
    fn space_metadata_refresh_preserves_identity_and_rejects_stale_paths_and_accounts() {
        let mut app = space_app();
        let _ = app.update(message(Event::OpenSpaces));
        let _ = app.update(message(Event::Edit("personal".into())));
        let _ = app.update(message(Event::Move(false)));
        let path = app.room_picker.as_ref().unwrap().space_matches()[1]
            .path
            .clone();
        assert_eq!(path.last().unwrap(), "personal-design");
        let mut snapshot = app.model.clone();
        Arc::make_mut(&mut snapshot.spaces)
            .iter_mut()
            .find(|s| s.id == "personal-design")
            .unwrap()
            .name = "Aardvark".into();
        let _ = app.update(Message::State(Box::new(snapshot)));
        let state = app.room_picker.as_ref().unwrap();
        assert_eq!(
            state.space_matches()[state.selected].path,
            path,
            "rename reorders matches but preserves selection"
        );
        assert!(
            state.space_matches()[state.selected]
                .trail
                .ends_with("Aardvark")
        );
        let mut snapshot = app.model.clone();
        Arc::make_mut(&mut snapshot.spaces)
            .iter_mut()
            .find(|s| s.id == "home-space")
            .unwrap()
            .children
            .clear();
        let _ = app.update(Message::State(Box::new(snapshot)));
        let scope = app.shell.space.clone();
        let _ = app.update(message(Event::ChooseSpace(path)));
        assert!(app.room_picker.is_some());
        assert_eq!(app.shell.space, scope);
        let _ = app.update(message(Event::Edit(String::new())));
        Arc::make_mut(&mut app.model.spaces).retain(|s| s.id != "personal-design");
        let _ = app.update(message(Event::ChooseSpace(vec!["personal-design".into()])));
        assert!(app.room_picker.is_some());
        assert_eq!(app.shell.space, scope);
        app.account_id = Some("another-account".into());
        let _ = app.update(message(Event::ChooseSpace(Vec::new())));
        assert!(app.room_picker.is_none());
        assert_eq!(app.shell.space, scope);
    }

    #[test]
    fn picker_shortcuts_switch_modes_and_tab_preserves_space_query_and_selection() {
        let mut app = space_app();
        for modifiers in [Modifiers::CTRL, Modifiers::COMMAND] {
            let _ = app.update(
                super::super::shell_shortcut(
                    &Key::Character("P".into()),
                    modifiers | Modifiers::SHIFT,
                )
                .unwrap(),
            );
            assert!(app.room_picker.as_ref().unwrap().mode == Mode::Spaces);
            let _ = app.update(message(Event::Edit("design".into())));
            let _ = app.update(message(Event::Move(false)));
            for event in [
                picker_key(Key::Named(Named::Tab), Modifiers::empty()),
                picker_key(Key::Character("P".into()), modifiers | Modifiers::SHIFT),
            ] {
                let _ = app.update(
                    shortcut(
                        event,
                        iced::event::Status::Captured,
                        iced::window::Id::unique(),
                    )
                    .unwrap(),
                );
                let state = app.room_picker.as_ref().unwrap();
                assert!(state.mode == Mode::Spaces);
                assert_eq!(state.query, "design");
                assert_eq!(state.selected, 1);
            }
            let _ = app.update(message(Event::Choose("home".into())));
            assert!(
                app.room_picker.is_some(),
                "stale room choices cannot close the space picker"
            );
            let _ = app.update(
                shortcut(
                    picker_key(Key::Character("p".into()), modifiers),
                    iced::event::Status::Captured,
                    iced::window::Id::unique(),
                )
                .unwrap(),
            );
            let state = app.room_picker.as_ref().unwrap();
            assert!(state.mode == Mode::Rooms);
            assert!(state.query.is_empty());
            let _ = app.update(message(Event::ChooseSpace(Vec::new())));
            assert!(
                app.room_picker.is_some(),
                "stale space choices cannot close the room picker"
            );
        }
        for modifiers in [
            Modifiers::empty(),
            Modifiers::SHIFT,
            Modifiers::CTRL,
            Modifiers::CTRL | Modifiers::SHIFT | Modifiers::ALT,
        ] {
            assert!(!is_space_shortcut(&Key::Character("p".into()), modifiers));
        }
    }

    #[test]
    fn space_picker_respects_modal_and_ime_guards() {
        for guard in [
            "ime",
            "settings",
            "create room",
            "new chat",
            "closing",
            "authentication",
        ] {
            let mut app = space_app();
            match guard {
                "ime" => app.composing = true,
                "settings" => {
                    let _ = app.update(Message::Settings(super::super::settings::Event::Open));
                }
                "create room" => {
                    let _ = app.update(Message::CreateRoom(super::super::create_room::Event::Open));
                }
                "new chat" => {
                    let _ = app.update(Message::NewChat(super::super::new_chat::Event::Open));
                }
                "closing" => app.closing = true,
                "authentication" => app.demo = None,
                _ => unreachable!(),
            }
            let _ = app.update(message(Event::OpenSpaces));
            assert!(app.room_picker.is_none(), "{guard}");
        }
        use iced::advanced::subscription::{self, Event as RuntimeEvent};
        use iced::futures::{FutureExt, StreamExt};
        let mut app = space_app();
        let _ = app.update(message(Event::OpenSpaces));
        app.composing = true;
        for recipe in subscription::into_recipes(app.subscription()) {
            let input = RuntimeEvent::Interaction {
                window: iced::window::Id::unique(),
                status: iced::event::Status::Captured,
                event: picker_key(Key::Named(Named::ArrowDown), Modifiers::empty()),
            };
            let output = recipe
                .stream(Box::pin(iced::futures::stream::iter([input])))
                .next()
                .now_or_never()
                .flatten();
            assert!(
                !matches!(output, Some(Message::QuickRoom(_))),
                "IME navigation belongs to the input method"
            );
        }
    }

    fn loading_room(via_picker: bool) -> (App, super::super::Pane) {
        let mut app = demo_app();
        let _ = app.update(Message::WindowFocused(true));
        let mut pane = app.model.room.clone().unwrap();
        app.demo = None;
        app.model.authentication = None;
        let task = if via_picker {
            let _ = app.update(message(Event::Open));
            let _ = app.update(message(Event::Edit("home".into())));
            app.update(message(Event::Submit))
        } else {
            app.update(Message::Room("home".into()))
        };
        pane.generation = app.selection.room.as_ref().unwrap().0;
        assert_eq!(task.units(), 0, "the composer is still absent");
        assert_eq!(app.pending_room_focus, Some(pane.generation));
        (app, pane)
    }

    #[test]
    fn connected_pane_arrival_focuses_the_composer_once() {
        for via_picker in [false, true] {
            let (mut app, pane) = loading_room(via_picker);
            app.pending_scroll = [None, None];
            let mut snapshot = app.model.clone();
            snapshot.room = Some(pane);
            assert_eq!(
                app.update(Message::State(Box::new(snapshot.clone())))
                    .units(),
                1
            );
            assert!(app.pending_room_focus.is_none());
            assert_eq!(app.update(Message::State(Box::new(snapshot))).units(), 0);
        }
    }

    #[test]
    fn ready_rooms_focus_after_enter_or_click_including_the_current_room() {
        for room in ["studio", "home"] {
            for source in ["submit", "picker click", "sidebar"] {
                let mut app = demo_app();
                let _ = app.update(Message::WindowFocused(true));
                let task = if source == "sidebar" {
                    app.update(Message::Room(room.into()))
                } else {
                    let _ = app.update(message(Event::Open));
                    let _ = app.update(message(Event::Edit(room.into())));
                    app.update(message(if source == "picker click" {
                        Event::Choose(room.into())
                    } else {
                        Event::Submit
                    }))
                };
                assert!(task.units() > 0, "{room} {source}");
                assert_eq!(app.selection.room.as_ref().unwrap().1, room);
                assert!(app.room_picker.is_none());
                assert!(app.pending_room_focus.is_none());
            }
        }
    }

    #[test]
    fn room_open_focus_operation_routes_typing_to_the_loaded_composer() {
        use iced::advanced::{Shell, layout, mouse, widget};

        let (mut app, pane) = loading_room(false);
        assert!(app.take_room_focus().is_none());
        app.model.room = Some(pane);
        let mut focus = app.take_room_focus().expect("matching pane is ready");
        let key = app.draft_key(false).unwrap();
        let original = DraftKey {
            room: "studio".into(),
            root: None,
        };
        app.drafts.get_mut(&original).unwrap().content =
            iced::widget::text_editor::Content::with_text("other draft");
        app.scroll_positions.insert(original.clone(), 135.0);
        app.scroll_positions.insert(key.clone(), 0.0);
        let scroll = app.scroll_positions.clone();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(1280.0, 720.0);
        let mut view = app.view();
        let mut tree = widget::Tree::new(&view);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        view.as_widget_mut()
            .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut focus);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: Key::Character("x".into()),
                modified_key: Key::Character("x".into()),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers: Modifiers::empty(),
                text: Some("x".into()),
                repeat: false,
            }),
            layout::Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        assert!(shell.is_event_captured());
        assert!(messages.iter().any(|message| {
            matches!(message, Message::Edit(target, Action::Edit(Edit::Insert('x'))) if target == &key)
        }));
        drop(view);
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.drafts[&key].content.text(), "x");
        assert_eq!(app.drafts[&original].content.text(), "other draft");
        assert_eq!(app.scroll_positions, scroll);
        assert!(app.take_room_focus().is_none());
    }

    #[test]
    fn delayed_focus_is_canceled_by_navigation_dialogs_and_focus_changes() {
        for interrupt in [
            "room",
            "thread",
            "settings",
            "picker",
            "filter",
            "unfocused",
            "error",
        ] {
            let (mut app, mut pane) = loading_room(true);
            match interrupt {
                "room" => {
                    let _ = app.update(Message::Room("studio".into()));
                }
                "thread" => {
                    let _ = app.update(Message::Thread("root".into()));
                }
                "settings" => {
                    let _ = app.update(Message::Settings(super::super::settings::Event::Open));
                }
                "picker" => {
                    let _ = app.update(message(Event::Open));
                }
                "filter" => {
                    let _ = app.update(Message::SearchFocus);
                }
                "unfocused" => {
                    let _ = app.update(Message::WindowFocused(false));
                }
                "error" => pane.error = Some("could not load".into()),
                _ => unreachable!(),
            }
            let mut snapshot = app.model.clone();
            snapshot.room = Some(pane);
            let _ = app.update(Message::State(Box::new(snapshot)));
            let pending = if interrupt == "room" {
                Some(app.selection.room.as_ref().unwrap().0)
            } else {
                None
            };
            assert_eq!(app.pending_room_focus, pending, "{interrupt}");
            assert!(app.take_room_focus().is_none(), "{interrupt}");
        }
    }

    #[test]
    fn fuzzy_search_ranks_and_handles_unicode_and_room_ids() {
        let rooms: Vec<_> = [
            ("!one:example.org", "General"),
            ("!two:example.org", "Gentle rain"),
            ("!three:example.org", "General discussion"),
            ("!four:example.org", "Équipe"),
        ]
        .into_iter()
        .map(|(id, name)| RoomEntry {
            id: id.into(),
            name: name.into(),
            ..RoomEntry::default()
        })
        .collect();
        let mut state = State {
            query: " GENERAL ".into(),
            ..State::default()
        };
        assert_eq!(
            state
                .matches(&rooms)
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["General", "General discussion"]
        );
        state.query = "gnrl".into();
        assert_eq!(state.matches(&rooms).len(), 2);
        state.query = "ÉQ".into();
        assert_eq!(state.matches(&rooms)[0].name, "Équipe");
        state.query = "!two:".into();
        assert_eq!(state.matches(&rooms)[0].name, "Gentle rain");
    }

    #[test]
    fn quick_open_crosses_spaces_preserves_drafts_scroll_and_sidebar() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let _ = app.update(Message::Edit(
            key.clone(),
            Action::Edit(Edit::Paste(std::sync::Arc::new("draft".into()))),
        ));
        app.scroll_positions.insert(key.clone(), 135.0);
        let scope = app.shell.space.clone();
        app.shell.query = "studio".into();
        let _ = app.update(message(Event::Open));
        let _ = app.update(message(Event::Edit("home".into())));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
        let _ = app.update(message(Event::Submit));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "home");
        assert_eq!(app.shell.space, scope);
        assert_eq!(app.shell.query, "studio");
        assert_eq!(app.drafts[&key].content.text(), "draft");
        assert_eq!(app.scroll_positions[&key], 135.0);
        let _ = app.update(message(Event::Open));
        let _ = app.update(message(Event::Edit("studio".into())));
        let _ = app.update(message(Event::Choose("studio".into())));
        assert_eq!(app.saved_scroll(false), 135.0);
        assert_eq!(
            app.draft_key(false),
            Some(DraftKey {
                room: "studio".into(),
                root: None
            })
        );
    }

    #[test]
    fn cancel_does_not_close_thread_and_empty_or_removed_results_are_safe() {
        let mut app = demo_app();
        let _ = app.update(Message::OpenLastThread);
        let selection = app.selection.clone();
        let _ = app.update(message(Event::Open));
        let _ = app.update(Message::AdjacentRoom(false));
        let _ = app.update(Message::Escape);
        assert_eq!(app.selection, selection);
        assert!(app.room_picker.is_none());
        let _ = app.update(message(Event::Open));
        let _ = app.update(message(Event::Edit("no such room".into())));
        let _ = app.update(message(Event::Move(true)));
        let _ = app.update(message(Event::Submit));
        assert!(app.room_picker.is_some());
        let _ = app.update(message(Event::Edit(String::new())));
        let count = app.model.rooms.len();
        let _ = app.update(message(Event::Move(true)));
        assert_eq!(app.room_picker.as_ref().unwrap().selected, count - 1);
        app.model.rooms = Vec::new().into();
        let _ = app.update(message(Event::Submit));
        assert_eq!(app.selection.room, selection.room);
    }

    #[test]
    fn picker_input_submits_once_and_backdrop_captures_at_minimum_size() {
        for event in [Event::Open, Event::OpenSpaces] {
            assert_picker_input_and_layout(event);
        }
    }

    fn assert_picker_input_and_layout(open: Event) {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        if matches!(open, Event::OpenSpaces) {
            for space in Arc::make_mut(&mut app.model.spaces) {
                space.name = "Café 👩🏽‍💻 — Research and references ".repeat(10);
            }
        }
        let _ = app.update(message(open));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let size = iced::Size::new(640.0, 480.0);
        let mut view = app.quick_room_view();
        let mut tree = widget::Tree::new(&view);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        assert_eq!(node.size(), size);
        view.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut widget::operation::focusable::focus(SEARCH.into()),
        );
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            layout::Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
        assert!(messages.is_empty());
        let event = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: Key::Named(Named::Enter),
            modified_key: Key::Named(Named::Enter),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        });
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &event,
            layout::Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        assert!(shell.is_event_captured());
        assert!(matches!(
            messages.as_slice(),
            [Message::QuickRoom(Event::Submit)]
        ));
        assert!(
            shortcut(
                event,
                iced::event::Status::Captured,
                iced::window::Id::unique()
            )
            .is_none()
        );
        messages.clear();
        let mut shell = Shell::new(&mut messages);
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
            [Message::QuickRoom(Event::Close)]
        ));
    }

    #[test]
    fn shortcut_works_with_control_and_platform_command() {
        for modifiers in [Modifiers::CTRL, Modifiers::COMMAND] {
            assert!(matches!(
                super::super::shell_shortcut(&Key::Character("p".into()), modifiers),
                Some(Message::QuickRoom(Event::Open))
            ));
        }
        assert!(!is_shortcut(
            &Key::Character("p".into()),
            Modifiers::empty()
        ));
        assert!(!is_shortcut(
            &Key::Character("p".into()),
            Modifiers::CTRL | Modifiers::SHIFT
        ));
    }
}
