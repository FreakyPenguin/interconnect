//! Explicit catch-up navigation using room metadata; no timeline scans or timers.
use super::{App, Message};
use iced::{
    Task,
    keyboard::{Key, Modifiers, key::Named},
};

pub(super) fn shortcut(key: &Key, modifiers: Modifiers) -> Option<Message> {
    let base = modifiers - Modifiers::SHIFT;
    if matches!(key.as_ref(), Key::Character("j" | "J"))
        && (base == Modifiers::COMMAND || base == Modifiers::CTRL)
    {
        return Some(Message::UnreadRoom {
            previous: modifiers.shift(),
            account_wide: true,
        });
    }
    if modifiers != (Modifiers::ALT | Modifiers::SHIFT) {
        return None;
    }
    let previous = match key {
        Key::Named(Named::ArrowUp) => true,
        Key::Named(Named::ArrowDown) => false,
        _ => return None,
    };
    Some(Message::UnreadRoom {
        previous,
        account_wide: false,
    })
}

impl App {
    pub(super) fn navigate_unread(&mut self, previous: bool, account_wide: bool) -> Task<Message> {
        if !self.context_menu_allowed()
            || self.composing
            || self.context_menu.is_some()
            || self.emoji_management.is_some()
            || self.section_dragging()
        {
            return Task::none();
        }
        let rooms = self.ordered_rooms(account_wide);
        let current = self.selection.room.as_ref().map(|(_, room)| room);
        let start = rooms.iter().position(|room| Some(room) == current);
        let target = (0..rooms.len()).find_map(|step| {
            let index = match (start, previous) {
                (Some(start), false) => (start + step + 1) % rooms.len(),
                (Some(start), true) => {
                    (start + rooms.len() - (step + 1) % rooms.len()) % rooms.len()
                }
                (None, false) => step,
                (None, true) => rooms.len() - step - 1,
            };
            let id = &rooms[index];
            (Some(id) != current
                && self
                    .model
                    .rooms
                    .iter()
                    .any(|room| &room.id == id && room.unread))
            .then(|| id.clone())
        });
        target.map_or_else(Task::none, |room| self.update(Message::Room(room)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DraftKey, Scope, tests::demo_app};

    fn app() -> App {
        let mut app = demo_app();
        app.shell.space = Scope::All;
        for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
            room.unread = false;
        }
        app
    }

    #[test]
    fn wraps_skips_read_and_current_rooms_and_preserves_drafts_positions_and_history() {
        let mut app = app();
        let ordered = app.visible_rooms();
        let first = ordered.first().unwrap().clone();
        let last = ordered.last().unwrap().clone();
        let _ = app.update(Message::Room(first.clone()));
        let key = DraftKey {
            room: first.clone(),
            root: None,
        };
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("keep draft");
        app.scroll_positions.insert(key.clone(), 140.0);
        for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
            room.unread = room.id == first || room.id == last;
        }
        let _ = app.update(Message::UnreadRoom {
            previous: true,
            account_wide: false,
        });
        assert_eq!(app.selection.room.as_ref().unwrap().1, last);
        assert_eq!(app.drafts[&key].content.text(), "keep draft");
        assert_eq!(app.scroll_positions[&key], 140.0);
        let _ = app.update(Message::ConversationHistory(true));
        assert_eq!(app.selection.room.as_ref().unwrap().1, first);
        let _ = app.update(Message::UnreadRoom {
            previous: false,
            account_wide: false,
        });
        assert_eq!(app.selection.room.as_ref().unwrap().1, last);
        for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
            room.unread = room.id == last;
        }
        let selection = app.selection.clone();
        assert_eq!(
            app.update(Message::UnreadRoom {
                previous: false,
                account_wide: false
            })
            .units(),
            0
        );
        assert_eq!(app.selection, selection);
    }

    #[test]
    fn account_wide_bypasses_space_and_text_filters_without_changing_them() {
        let mut app = app();
        app.shell.space = Scope::Space("space3".into());
        app.shell.query = "no matching room".into();
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "release")
            .unwrap()
            .unread = true;
        let selected = app.selection.clone();
        let _ = app.update(Message::UnreadRoom {
            previous: false,
            account_wide: false,
        });
        assert_eq!(app.selection, selected);
        let scope = app.shell.space.clone();
        let _ = app.update(
            crate::shell_shortcut(&Key::Character("j".into()), Modifiers::COMMAND).unwrap(),
        );
        assert_eq!(app.selection.room.as_ref().unwrap().1, "release");
        assert_eq!(app.shell.space, scope);
        assert_eq!(app.shell.query, "no matching room");
    }

    #[test]
    fn empty_lists_modals_and_ime_do_not_navigate() {
        for guard in ["empty", "ime", "settings", "closing"] {
            let mut app = app();
            for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
                room.unread = true;
            }
            match guard {
                "empty" => std::sync::Arc::make_mut(&mut app.model.rooms).clear(),
                "ime" => app.composing = true,
                "settings" => {
                    let _ = app.update(Message::Settings(crate::settings::Event::Open));
                }
                "closing" => app.closing = true,
                _ => unreachable!(),
            }
            let selected = app.selection.clone();
            let _ = app.update(Message::UnreadRoom {
                previous: true,
                account_wide: true,
            });
            assert_eq!(app.selection, selected, "{guard}");
        }
    }

    #[test]
    fn shortcuts_require_exact_modifiers_and_leave_ordinary_room_switching_intact() {
        let down = Key::Named(Named::ArrowDown);
        assert!(matches!(
            crate::shell_shortcut(&down, Modifiers::ALT),
            Some(Message::AdjacentRoom(false))
        ));
        assert!(matches!(
            crate::shell_shortcut(&down, Modifiers::ALT | Modifiers::SHIFT),
            Some(Message::UnreadRoom {
                previous: false,
                account_wide: false
            })
        ));
        for base in [Modifiers::CTRL, Modifiers::COMMAND] {
            for (letter, modifiers, previous) in
                [("j", base, false), ("J", base | Modifiers::SHIFT, true)]
            {
                assert!(matches!(
                    crate::shell_shortcut(&Key::Character(letter.into()), modifiers),
                    Some(Message::UnreadRoom { previous: actual, account_wide: true })
                        if actual == previous
                ));
                assert!(
                    shortcut(&Key::Character(letter.into()), modifiers | Modifiers::ALT).is_none()
                );
            }
        }
        assert!(
            shortcut(
                &down,
                Modifiers::ALT | Modifiers::SHIFT | Modifiers::COMMAND
            )
            .is_none()
        );
        assert!(shortcut(&down, Modifiers::SHIFT).is_none());
        assert!(
            shortcut(
                &Key::Named(Named::ArrowLeft),
                Modifiers::ALT | Modifiers::SHIFT
            )
            .is_none()
        );
    }
}
