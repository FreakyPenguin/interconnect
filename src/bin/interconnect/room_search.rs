//! Local, event-driven search of the current room's loaded messages.
use super::{App, Message, RowKind, current_pane, shell};
use iced::{
    Element, Length, Task,
    widget::{button, container, row, text, text_input, tooltip},
};

#[derive(Default)]
pub struct State {
    pub open: bool,
    query: String,
    pub current: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Edit(String),
    Step(bool),
}

fn message(event: Event) -> Message {
    Message::RoomSearch(event)
}

/// Case-insensitive matches mapped back to original UTF-8 byte boundaries.
/// Lowercasing can expand a character (for example İ → i + combining dot).
pub(super) fn matching_ranges(body: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let folded = body.to_lowercase();
    let mut mapping = Vec::with_capacity(folded.len());
    for (offset, ch) in body.char_indices() {
        for _ in 0..ch.to_lowercase().map(char::len_utf8).sum::<usize>() {
            mapping.push(offset..offset + ch.len_utf8());
        }
    }
    folded
        .match_indices(&query)
        .map(|(start, found)| mapping[start].start..mapping[start + found.len() - 1].end)
        .collect()
}

impl App {
    pub(super) fn room_search_query(&self) -> Option<&str> {
        self.room_search_active()
            .then_some(self.room_search.query.as_str())
    }
    pub(super) fn room_search_active(&self) -> bool {
        self.room_search.open && !self.room_search.query.trim().is_empty()
    }

    fn room_search_matches(&self) -> Vec<String> {
        if !self.room_search_active() {
            return Vec::new();
        }
        let Some((generation, _)) = &self.selection.room else {
            return Vec::new();
        };
        let Some(pane) = current_pane(&self.model.room, *generation) else {
            return Vec::new();
        };
        let query = self.room_search.query.trim().to_lowercase();
        pane.rows
            .iter()
            .filter(|row| row.kind == RowKind::Message && row.body.to_lowercase().contains(&query))
            .map(|row| row.event_id.clone().unwrap_or_else(|| row.key.clone()))
            .collect()
    }

    pub(super) fn room_search_target(&self) -> Option<String> {
        let target = self.room_search.current.as_ref()?;
        self.room_search_matches()
            .into_iter()
            .find(|key| key == target)
    }

    pub(super) fn room_search_event(&mut self, event: Event) -> Task<Message> {
        if self.closing || self.stopped {
            return Task::none();
        }
        match event {
            Event::Open
                if self.selection.room.is_some()
                    && !self.bookmarks.open
                    && !self.global_search.open
                    && !self.space_settings.is_open() =>
            {
                self.stop_typing();
                // At narrow widths the side pane occupies the room's place.
                // Return to the room so both the field and matches are visible.
                if self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width))
                {
                    if self.selection.members.is_some() {
                        let _ = self.update(Message::Members(super::members::Event::Close));
                    }
                    if self.selection.thread.is_some() {
                        let _ = self.update(Message::CloseThread);
                    }
                }
                self.room_search.open = true;
                return iced::widget::operation::focus("in-room-search")
                    .chain(iced::widget::operation::select_all("in-room-search"));
            }
            Event::Close => {
                self.room_search = State::default();
                return iced::widget::operation::focus("room-composer");
            }
            Event::Edit(query) if self.room_search.open => {
                self.room_search.query = query;
                // Start at the most recent matching message.
                self.room_search.current = self.room_search_matches().pop();
                self.pending_scroll[0] = None;
            }
            Event::Step(previous) if self.room_search.open => {
                let matches = self.room_search_matches();
                if !matches.is_empty() {
                    let current = matches
                        .iter()
                        .position(|key| Some(key) == self.room_search.current.as_ref());
                    let index = match (current, previous) {
                        (Some(index), true) => (index + matches.len() - 1) % matches.len(),
                        (Some(index), false) => (index + 1) % matches.len(),
                        (None, _) => matches.len() - 1,
                    };
                    self.room_search.current = Some(matches[index].clone());
                    self.pending_scroll[0] = None;
                }
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn room_search_button(&self) -> Element<'_, Message> {
        super::tooltips::tooltip(
            container(
                button(
                    container(super::icons::icon(super::icons::Kind::Search))
                        .center_x(28)
                        .center_y(28),
                )
                .padding(0)
                .style(shell::icon_button)
                .on_press(message(Event::Open)),
            )
            .id("room-search-button"),
            "Find in room (Ctrl+F / ⌘F)",
            tooltip::Position::Bottom,
        )
        .into()
    }

    pub(super) fn room_search_view(&self) -> Element<'_, Message> {
        if !self.room_search.open {
            return iced::widget::space().height(0).into();
        }
        let matches = self.room_search_matches();
        let current = matches
            .iter()
            .position(|key| Some(key) == self.room_search.current.as_ref());
        let label = if !self.room_search_active() {
            "Loaded messages".into()
        } else if matches.is_empty() {
            "No matches".into()
        } else {
            format!(
                "{} / {}",
                current.map_or(0, |index| index + 1),
                matches.len()
            )
        };
        let mut previous =
            button(super::icons::icon(super::icons::Kind::ArrowUp)).style(shell::icon_button);
        let mut next =
            button(super::icons::icon(super::icons::Kind::ArrowDown)).style(shell::icon_button);
        if !matches.is_empty() {
            previous = previous.on_press(message(Event::Step(true)));
            next = next.on_press(message(Event::Step(false)));
        }
        container(
            row![
                text_input("Find in loaded room messages…", &self.room_search.query)
                    .style(shell::search_style)
                    .id("in-room-search")
                    .padding([7, 10])
                    .cursor_blinking(false)
                    .size(13)
                    .on_input(|query| message(Event::Edit(query)))
                    .on_submit(message(Event::Step(true)))
                    .width(Length::Fill),
                text(label).size(11).style(shell::muted_text),
                super::tooltips::tooltip(
                    previous,
                    "Previous match · Enter",
                    tooltip::Position::Bottom
                ),
                super::tooltips::tooltip(next, "Next match", tooltip::Position::Bottom),
                super::tooltips::tooltip(
                    button(super::icons::icon(super::icons::Kind::Close))
                        .style(shell::icon_button)
                        .on_press(message(Event::Close)),
                    "Close search · Escape",
                    tooltip::Position::Bottom
                ),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding([8, 24])
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DraftKey, tests::demo_app};
    use std::sync::Arc;

    fn app_with_messages() -> App {
        let mut app = demo_app();
        let pane = app.model.room.as_mut().unwrap();
        let mut sample = pane
            .rows
            .iter()
            .find(|row| row.kind == RowKind::Message)
            .unwrap()
            .clone();
        let mut rows = Vec::new();
        for (key, body, kind) in [
            ("first", "HéLLo world", RowKind::Message),
            ("activity", "héllo joined", RowKind::Activity),
            ("second", "Another héllo", RowKind::Message),
            ("other", "Something else", RowKind::Message),
        ] {
            sample.key = key.into();
            sample.event_id = Some(key.into());
            sample.body = body.into();
            sample.kind = kind;
            rows.push(sample.clone());
        }
        pane.rows = rows.into();
        let _ = app.update(message(Event::Open));
        app
    }

    #[test]
    fn match_ranges_preserve_unicode_boundaries_and_find_repeated_terms() {
        for (body, query, expected) in [
            (
                "Hello HELLO hello",
                " hello ",
                vec!["Hello", "HELLO", "hello"],
            ),
            ("é İSTANBUL 👩🏽‍💻 İstanbul", "i", vec!["İ", "İ"]),
            ("İstanbul", "\u{307}", vec!["İ"]),
            ("ΟΣ ος", "ος", vec!["ΟΣ", "ος"]),
            ("שלום שלום", "שלום", vec!["שלום", "שלום"]),
            ("café 👩🏽‍💻 café", "CAFÉ", vec!["café", "café"]),
            ("hello", " ", vec![]),
            ("hello", "absent", vec![]),
        ] {
            let ranges = matching_ranges(body, query);
            assert_eq!(
                ranges
                    .into_iter()
                    .map(|range| &body[range])
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn search_is_case_insensitive_and_navigation_wraps_without_sdk_navigation() {
        let mut app = app_with_messages();
        let selection = app.selection.clone();
        let _ = app.update(message(Event::Edit(" HÉLLO ".into())));
        assert_eq!(app.room_search_matches(), ["first", "second"]);
        assert_eq!(app.room_search_target().as_deref(), Some("second"));
        let _ = app.update(message(Event::Step(true)));
        assert_eq!(app.room_search_target().as_deref(), Some("first"));
        let _ = app.update(message(Event::Step(true)));
        assert_eq!(app.room_search_target().as_deref(), Some("second"));
        let _ = app.update(message(Event::Step(false)));
        assert_eq!(app.room_search_target().as_deref(), Some("first"));
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.event_focus, selection.event_focus);
    }

    #[test]
    fn updates_and_stale_panes_cannot_leave_an_invalid_target() {
        let mut app = app_with_messages();
        let _ = app.update(message(Event::Edit("héllo".into())));
        Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)[2].body = "Edited".into();
        assert!(app.room_search_target().is_none());
        let _ = app.update(message(Event::Step(true)));
        assert_eq!(app.room_search_target().as_deref(), Some("first"));
        app.model.room.as_mut().unwrap().generation += 1;
        assert!(app.room_search_matches().is_empty());
        assert!(app.room_search_target().is_none());
        let _ = app.update(message(Event::Step(false)));
        let _ = app.update(message(Event::Edit("   ".into())));
        assert!(!app.room_search_active());
    }

    #[test]
    fn search_keeps_drafts_and_space_scope_and_escape_only_closes_search() {
        let mut app = app_with_messages();
        let key = DraftKey {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            root: None,
        };
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent draft");
        app.scroll_positions.insert(key.clone(), 120.0);
        let _ = app.update(Message::Thread("review".into()));
        let thread = app.selection.thread.clone();
        let _ = app.update(message(Event::Edit("héllo".into())));
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert!(app.room_search.open);
        let _ = app.update(Message::Escape);
        assert!(!app.room_search.open);
        assert_eq!(app.selection.thread, thread);
        assert_eq!(app.drafts[&key].content.text(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 120.0);
        let _ = app.update(message(Event::Open));
        let _ = app.update(Message::Room("general".into()));
        assert!(!app.room_search.open);
        let _ = app.update(message(Event::Edit("stale".into())));
        assert!(app.room_search.query.is_empty());
    }

    #[test]
    fn narrow_window_search_returns_to_the_room() {
        let mut app = app_with_messages();
        let _ = app.update(Message::Thread("review".into()));
        app.window_width = Some(800.0);
        assert!(app.side_panel_replaces_room(800.0));
        let _ = app.update(message(Event::Open));
        assert!(app.room_search.open);
        assert!(app.selection.thread.is_none());
        assert!(!app.side_panel_replaces_room(800.0));
    }

    #[test]
    fn shortcut_opens_search_and_respects_modals() {
        use iced::keyboard::{Key, Modifiers};
        let shortcut =
            super::super::shell_shortcut(&Key::Character("f".into()), Modifiers::CTRL).unwrap();
        assert!(matches!(shortcut, Message::RoomSearch(Event::Open)));
        let mut app = app_with_messages();
        let _ = app.update(message(Event::Close));
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        let _ = app.update(shortcut);
        assert!(!app.room_search.open);
        assert!(
            super::super::shell_shortcut(&Key::Character("f".into()), Modifiers::empty()).is_none()
        );
    }
}
