//! Local room quick-open. No SDK requests or subscriptions beyond keyboard input.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    keyboard::{Key, Modifiers, key::Named},
    widget::{button, column, container, mouse_area, opaque, row, scrollable, text, text_input},
};
use interconnect::navigation::RoomEntry;

const SEARCH: &str = "quick-room-search";
const RESULTS: &str = "quick-room-results";
#[derive(Default)]
pub struct State {
    query: String,
    selected: usize,
    thread: bool,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Edit(String),
    Move(bool),
    Submit,
    Choose(String),
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
        Key::Named(Named::Tab) => Some(message(Event::Open)),
        Key::Character("p" | "P") if is_shortcut(&key, modifiers) => Some(message(Event::Open)),
        _ => None,
    }
}
pub fn is_shortcut(key: &Key, modifiers: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character("p" | "P"))
        && (modifiers.control() || modifiers.command())
        && !modifiers.alt()
        && !modifiers.shift()
}
impl App {
    pub(super) fn quick_room_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open => {
                if self.closing
                    || self.stopped
                    || self.settings.draft.is_some()
                    || self.verification.open
                    || self.image_view.is_some()
                    || self.reaction_picker.is_some()
                    || self.composer_picker.is_some()
                    || self.invite.is_some()
                    || self.topic_editor.is_some()
                    || (self.demo.is_none() && self.model.authentication.is_some())
                {
                    return Task::none();
                }
                if self.room_picker.is_none() {
                    self.stop_typing();
                    self.emoji_completion.clear();
                    self.room_picker = Some(State {
                        thread: self.selection.thread.is_some(),
                        ..State::default()
                    });
                }
                return iced::widget::operation::focus(SEARCH);
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
                    let count = state.matches(&self.model.rooms).len();
                    if count > 0 {
                        let index = state.selected.min(count - 1);
                        state.selected = if previous {
                            (index + count - 1) % count
                        } else {
                            (index + 1) % count
                        };
                        return iced::widget::operation::snap_to(
                            RESULTS,
                            iced::widget::operation::RelativeOffset {
                                x: 0.0,
                                y: state.selected as f32 / count.saturating_sub(1).max(1) as f32,
                            },
                        );
                    }
                }
            }
            Event::Submit | Event::Choose(_) => {
                let Some(state) = &self.room_picker else {
                    return Task::none();
                };
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
                        Task::none()
                    } else {
                        self.update(Message::Room(id))
                    };
                    return task.chain(iced::widget::operation::focus("room-composer"));
                }
            }
            Event::Ignore => {}
        }
        Task::none()
    }
    pub(super) fn quick_room_view(&self) -> Element<'_, Message> {
        let state = self.room_picker.as_ref().unwrap();
        let matches = state.matches(&self.model.rooms);
        let selected = state.selected.min(matches.len().saturating_sub(1));
        let mut results = column![].spacing(2);
        for (index, room) in matches.iter().enumerate() {
            let active = index == selected;
            let label = column![
                text(&room.name).size(14),
                text(&room.id).size(11).color(shell::MUTED)
            ]
            .spacing(2);
            results = results.push(
                button(
                    row![
                        text(if room.direct { "@" } else { "#" }).color(shell::MUTED),
                        label
                    ]
                    .spacing(12),
                )
                .width(Length::Fill)
                .padding([8, 12])
                .on_press(message(Event::Choose(room.id.clone())))
                .style(move |theme, status| {
                    let mut style = shell::quiet_button(theme, status);
                    style.text_color = shell::INK;
                    if active {
                        style.background = Some(iced::Color::from_rgb8(226, 234, 247).into());
                    }
                    style
                }),
            );
        }
        if matches.is_empty() {
            results = results.push(
                container(text("No matching rooms").size(13).color(shell::MUTED)).padding(16),
            );
        }
        let card = container(
            column![
                text_input("Switch to room…", &state.query)
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
                    "{} rooms   ·   ↑ ↓ select   ·   Enter open   ·   Esc close",
                    matches.len()
                ))
                .size(11)
                .color(shell::MUTED)
            ]
            .spacing(10),
        )
        .padding(12)
        .width(560)
        .height(420)
        .max_height(420)
        .style(|_| container::Style {
            border: iced::Border::default().rounded(12),
            ..shell::surface(iced::Color::WHITE)
        });
        opaque(
            mouse_area(
                container(opaque(mouse_area(card).on_press(message(Event::Ignore))))
                    .center_x(Length::Fill)
                    .height(Length::Fill)
                    .padding([48, 16])
                    .style(|_| shell::surface(iced::Color::BLACK.scale_alpha(0.25))),
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
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
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
        let mut messages = Vec::new();
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
