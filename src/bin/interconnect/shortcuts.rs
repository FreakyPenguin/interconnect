//! Platform shortcut conventions shared by handlers and displayed hints.
use iced::keyboard::Modifiers;

pub(super) fn primary(shift: bool) -> Modifiers {
    Modifiers::COMMAND
        | if shift {
            Modifiers::SHIFT
        } else {
            Modifiers::empty()
        }
}

pub(super) fn label(macos: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        macos
    } else {
        other
    }
}

/// Form and picker traversal only consumes unmodified Tab (or Shift+Tab).
pub(super) fn traversal(modifiers: Modifiers) -> bool {
    (modifiers - Modifiers::SHIFT).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, create_room, new_chat, room_picker, shell_shortcut};
    use iced::{
        Event,
        event::Status,
        keyboard::{self, Key, key::Named},
    };

    fn event(key: Key, modifiers: Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn app_commands_use_only_the_exact_platform_modifier() {
        for (letter, shift) in [
            ("n", false),
            ("N", true),
            ("k", false),
            ("K", true),
            ("p", false),
            ("P", true),
            ("l", false),
            ("L", true),
            ("f", false),
            ("F", true),
            ("j", false),
            ("J", true),
            ("t", false),
            (",", false),
        ] {
            let key = Key::Character(letter.into());
            let modifiers = primary(shift);
            assert!(shell_shortcut(&key, modifiers).is_some(), "{letter}");
            let other_platform = if cfg!(target_os = "macos") {
                Modifiers::CTRL
            } else {
                Modifiers::LOGO
            } | if shift {
                Modifiers::SHIFT
            } else {
                Modifiers::empty()
            };
            assert!(shell_shortcut(&key, other_platform).is_none(), "{letter}");
            assert!(
                shell_shortcut(&key, modifiers | other_platform).is_none(),
                "{letter}"
            );
            if !cfg!(target_os = "macos") || !matches!(letter, "j" | "J") {
                assert!(
                    shell_shortcut(&key, modifiers | Modifiers::ALT).is_none(),
                    "{letter}"
                );
            }
        }
    }

    #[test]
    fn picker_navigation_ignores_modified_or_already_captured_keys() {
        for key in [Named::ArrowUp, Named::ArrowDown, Named::Tab] {
            for modifiers in [
                Modifiers::CTRL,
                Modifiers::ALT,
                Modifiers::LOGO,
                Modifiers::ALT | Modifiers::SHIFT,
            ] {
                let key = event(Key::Named(key), modifiers);
                assert!(
                    new_chat::shortcut(key.clone(), Status::Ignored, iced::window::Id::unique())
                        .is_none()
                );
                assert!(
                    room_picker::shortcut(key, Status::Ignored, iced::window::Id::unique())
                        .is_none()
                );
            }
            let key = event(Key::Named(key), Modifiers::empty());
            assert!(
                new_chat::shortcut(key.clone(), Status::Captured, iced::window::Id::unique())
                    .is_none()
            );
            assert!(
                room_picker::shortcut(key.clone(), Status::Captured, iced::window::Id::unique())
                    .is_none()
            );
            assert!(
                new_chat::shortcut(key.clone(), Status::Ignored, iced::window::Id::unique())
                    .is_some()
            );
            assert!(
                room_picker::shortcut(key, Status::Ignored, iced::window::Id::unique()).is_some()
            );
        }
        for key in [Named::ArrowUp, Named::ArrowDown] {
            let key = event(Key::Named(key), Modifiers::SHIFT);
            assert!(
                new_chat::shortcut(key.clone(), Status::Ignored, iced::window::Id::unique())
                    .is_none()
            );
            assert!(
                room_picker::shortcut(key, Status::Ignored, iced::window::Id::unique()).is_none()
            );
        }
    }

    #[test]
    fn form_submission_and_traversal_require_exact_modifiers() {
        let id = iced::window::Id::unique();
        let submit = event(Key::Named(Named::Enter), primary(false));
        assert!(matches!(
            create_room::shortcut(submit.clone(), Status::Ignored, id),
            Some(Message::CreateRoom(create_room::Event::Submit))
        ));
        assert!(create_room::shortcut(submit, Status::Captured, id).is_none());
        for modifiers in [
            Modifiers::empty(),
            primary(true),
            primary(false) | Modifiers::ALT,
        ] {
            assert!(
                create_room::shortcut(
                    event(Key::Named(Named::Enter), modifiers),
                    Status::Ignored,
                    id
                )
                .is_none()
            );
        }
        for modifiers in [Modifiers::empty(), Modifiers::SHIFT] {
            assert!(
                create_room::shortcut(
                    event(Key::Named(Named::Tab), modifiers),
                    Status::Ignored,
                    id
                )
                .is_some()
            );
        }
        for modifiers in [Modifiers::CTRL, Modifiers::ALT, Modifiers::LOGO] {
            assert!(
                create_room::shortcut(
                    event(Key::Named(Named::Tab), modifiers),
                    Status::Ignored,
                    id
                )
                .is_none()
            );
            assert!(
                crate::room_settings::shortcut(
                    event(Key::Named(Named::Tab), modifiers),
                    Status::Ignored,
                    id
                )
                .is_none()
            );
        }
    }

    #[test]
    fn room_cycle_keeps_control_and_leaves_command_tab_to_the_system() {
        assert!(matches!(
            shell_shortcut(&Key::Named(Named::Tab), Modifiers::CTRL),
            Some(Message::AdjacentRoom(false))
        ));
        assert!(matches!(
            shell_shortcut(&Key::Named(Named::Tab), Modifiers::CTRL | Modifiers::SHIFT),
            Some(Message::AdjacentRoom(true))
        ));
        if cfg!(target_os = "macos") {
            assert!(shell_shortcut(&Key::Named(Named::Tab), Modifiers::COMMAND).is_none());
            assert!(
                shell_shortcut(
                    &Key::Named(Named::Tab),
                    Modifiers::COMMAND | Modifiers::SHIFT
                )
                .is_none()
            );
        }
    }
}
