//! Actual widget regressions for the shared native editor bindings.
use super::super::native_editing::{self, Command};
#[derive(Clone, Debug)]
enum TestAction {
    Edit(Action),
    Native(Command),
}
use iced::{
    Event, Renderer, Size, Theme,
    advanced::{Layout, Shell, Widget, layout, mouse, widget},
    keyboard::{
        Key, Modifiers,
        key::{Named, NativeCode, Physical},
    },
    widget::text_editor::{Action, Binding, Content, Cursor, Motion, Position},
};

fn actions(
    content: &Content,
    key: Key,
    modifiers: Modifiers,
    binding: Option<Command>,
) -> Vec<TestAction> {
    let mut editor = iced::widget::text_editor(content)
        .id("native-editor-test")
        .height(180)
        .width(140)
        .size(14)
        .cursor_blinking(false)
        .on_action(TestAction::Edit)
        .key_binding(move |press| {
            if let Some(command) = binding {
                Some(Binding::Custom(TestAction::Native(command)))
            } else {
                native_editing::binding(press, TestAction::Native)
            }
        });
    let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
    let mut tree = widget::Tree::new(&editor as &dyn Widget<TestAction, Theme, Renderer>);
    let node = editor.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(140.0, 180.0)),
    );
    editor.operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut widget::operation::focusable::focus(widget::Id::new("native-editor-test")),
    );
    let mut result = Vec::new();
    let mut shell = Shell::new(&mut result);
    let text = match key.as_ref() {
        Key::Character(value) if modifiers == Modifiers::CTRL => value
            .chars()
            .next()
            .and_then(|c| char::from_u32(u32::from(c) & 0x1f))
            .map(|c| c.to_string().into()),
        _ => None,
    };
    editor.update(
        &mut tree,
        &Event::Keyboard(iced::keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers,
            text,
            repeat: false,
        }),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut iced::advanced::clipboard::Null,
        &mut shell,
        &iced::Rectangle::with_size(Size::new(140.0, 180.0)),
    );
    assert!(!matches!(
        shell.redraw_request(),
        iced::window::RedrawRequest::At(_)
    ));
    result
}

fn apply(content: &mut Content, actions: Vec<TestAction>) {
    for action in actions {
        match action {
            TestAction::Edit(action) => content.perform(action),
            TestAction::Native(command) => command.perform(content),
        }
    }
}

#[test]
fn shared_paragraph_bindings_preserve_preview_state_and_cross_only_hard_lines() {
    for source in [
        "A long wrapped paragraph with café and 👋 repeated several times in the available width.\nsecond paragraph\nlast",
        "مرحبا بالعالم هذه فقرة طويلة جدا تتجاوز عرض المحرر وتحتوي على كلمات كثيرة\nفقرة ثانية\nالنهاية",
    ] {
        let mut content = Content::with_text(source);
        let first_end = content.line(0).unwrap().text.len();
        for (cursor, forward, select, expected) in [
            (
                Cursor {
                    position: Position { line: 0, column: 0 },
                    selection: None,
                },
                false,
                false,
                Position { line: 0, column: 0 },
            ),
            (
                Cursor {
                    position: Position { line: 0, column: 0 },
                    selection: None,
                },
                true,
                false,
                Position {
                    line: 0,
                    column: first_end,
                },
            ),
            (
                Cursor {
                    position: Position { line: 0, column: 0 },
                    selection: None,
                },
                true,
                true,
                Position {
                    line: 0,
                    column: first_end,
                },
            ),
            (
                Cursor {
                    position: Position {
                        line: 0,
                        column: first_end,
                    },
                    selection: None,
                },
                true,
                false,
                Position {
                    line: 1,
                    column: content.line(1).unwrap().text.len(),
                },
            ),
            (
                Cursor {
                    position: Position { line: 1, column: 0 },
                    selection: None,
                },
                false,
                true,
                Position { line: 0, column: 0 },
            ),
            (
                Cursor {
                    position: Position { line: 1, column: 0 },
                    selection: Some(Position { line: 2, column: 0 }),
                },
                false,
                false,
                Position { line: 0, column: 0 },
            ),
            (
                Cursor {
                    position: Position { line: 2, column: 0 },
                    selection: Some(Position { line: 0, column: 0 }),
                },
                false,
                false,
                Position { line: 0, column: 0 },
            ),
        ] {
            // Moving first clears any preceding case's selection.
            content.perform(Action::Move(Motion::DocumentStart));
            content.move_to(cursor);
            let selected_before = content.selection();
            let emitted = actions(
                &content,
                Key::Named(Named::ArrowDown),
                Modifiers::empty(),
                Some(Command::Paragraph { forward, select }),
            );
            assert_eq!(
                content.cursor(),
                cursor,
                "preview must restore cursor and anchor"
            );
            assert_eq!(content.selection(), selected_before);
            assert_eq!(content.text(), source);
            apply(&mut content, emitted);
            assert_eq!(
                content.cursor().position,
                expected,
                "{forward} {select}: {source}"
            );
            if !select {
                assert!(content.selection().is_none());
            }
            assert_eq!(content.text(), source);
        }
    }
}

#[test]
fn shared_native_deletion_preserves_boundaries_and_existing_selection() {
    for (source, position, selection, motion, expected) in [
        (
            "café 👋",
            Position { line: 0, column: 0 },
            None,
            Motion::WordLeft,
            "café 👋",
        ),
        (
            "café 👋",
            Position {
                line: 0,
                column: "café 👋".len(),
            },
            None,
            Motion::WordRight,
            "café 👋",
        ),
        (
            "café 👋",
            Position {
                line: 0,
                column: "café 👋".len(),
            },
            None,
            Motion::Home,
            "",
        ),
        (
            "one two three",
            Position { line: 0, column: 7 },
            Some(Position { line: 0, column: 4 }),
            Motion::Home,
            "one  three",
        ),
    ] {
        let mut content = Content::with_text(source);
        content.move_to(Cursor {
            position,
            selection,
        });
        let cursor = content.cursor();
        let selected = content.selection();
        let emitted = actions(
            &content,
            Key::Named(Named::Backspace),
            Modifiers::empty(),
            Some(Command::DeleteTo(motion)),
        );
        assert_eq!(content.cursor(), cursor);
        assert_eq!(content.selection(), selected);
        apply(&mut content, emitted);
        assert_eq!(content.text(), expected);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn mac_editor_bindings_work_without_composer_overrides() {
    for (key, modifiers, source, caret, expected, end) in [
        (
            Key::Named(Named::ArrowUp),
            Modifiers::COMMAND,
            "first\nsecond",
            Position { line: 1, column: 3 },
            "first\nsecond",
            Position { line: 0, column: 0 },
        ),
        (
            Key::Named(Named::ArrowDown),
            Modifiers::COMMAND,
            "first\nsecond",
            Position { line: 0, column: 2 },
            "first\nsecond",
            Position { line: 1, column: 6 },
        ),
        (
            Key::Named(Named::Backspace),
            Modifiers::ALT,
            "one two",
            Position { line: 0, column: 7 },
            "one ",
            Position { line: 0, column: 4 },
        ),
        (
            Key::Character("f".into()),
            Modifiers::CTRL,
            "café",
            Position { line: 0, column: 3 },
            "café",
            Position { line: 0, column: 5 },
        ),
        (
            Key::Character("a".into()),
            Modifiers::CTRL,
            "first",
            Position { line: 0, column: 3 },
            "first",
            Position { line: 0, column: 0 },
        ),
        (
            Key::Character("e".into()),
            Modifiers::CTRL,
            "first",
            Position { line: 0, column: 3 },
            "first",
            Position { line: 0, column: 5 },
        ),
        (
            Key::Character("n".into()),
            Modifiers::CTRL,
            "first\nsecond",
            Position { line: 0, column: 0 },
            "first\nsecond",
            Position { line: 1, column: 0 },
        ),
        (
            Key::Character("p".into()),
            Modifiers::CTRL,
            "first\nsecond",
            Position { line: 1, column: 0 },
            "first\nsecond",
            Position { line: 0, column: 0 },
        ),
        (
            Key::Character("o".into()),
            Modifiers::CTRL,
            "first",
            Position { line: 0, column: 3 },
            "fir\nst",
            Position { line: 0, column: 3 },
        ),
        (
            Key::Character("k".into()),
            Modifiers::CTRL,
            "first\nsecond",
            Position { line: 0, column: 3 },
            "fir\nsecond",
            Position { line: 0, column: 3 },
        ),
        (
            Key::Character("k".into()),
            Modifiers::CTRL,
            "first\nsecond",
            Position { line: 0, column: 5 },
            "firstsecond",
            Position { line: 0, column: 5 },
        ),
    ] {
        let mut content = Content::with_text(source);
        content.move_to(Cursor {
            position: caret,
            selection: None,
        });
        let emitted = actions(&content, key, modifiers, None);
        apply(&mut content, emitted);
        assert_eq!(content.text(), expected);
        assert_eq!(content.cursor().position, end);
    }
}

#[test]
fn paragraph_end_deletion_removes_only_the_logical_newline_in_rtl() {
    for source in ["مرحبا\nثانية", "مرحبا"] {
        let mut content = Content::with_text(source);
        let end = content.line(0).unwrap().text.len();
        content.move_to(Cursor {
            position: Position {
                line: 0,
                column: end,
            },
            selection: None,
        });
        let emitted = actions(
            &content,
            Key::Character("k".into()),
            Modifiers::CTRL,
            Some(Command::DeleteParagraphEnd),
        );
        apply(&mut content, emitted);
        assert_eq!(content.text(), source.replace('\n', ""));
        assert_eq!(
            content.cursor().position,
            Position {
                line: 0,
                column: end
            }
        );
    }
}

#[test]
fn very_long_paragraph_uses_a_direct_logical_target() {
    let source = "café 👋 paragraph words ".repeat(20_000);
    let mut content = Content::with_text(&source);
    Command::Paragraph {
        forward: true,
        select: false,
    }
    .perform(&mut content);
    assert_eq!(
        content.cursor().position,
        Position {
            line: 0,
            column: source.len()
        }
    );
    assert!(content.selection().is_none());
    assert_eq!(content.text(), source);
}

#[test]
fn open_line_restores_logical_insertion_point_in_rtl_and_replaces_selection() {
    for (source, caret, selection, expected, end) in [
        (
            "مرحبا ثانية",
            "مرحبا ".len(),
            None,
            "مرحبا \nثانية",
            "مرحبا ".len(),
        ),
        ("one two three", 7, Some(4), "one \n three", 4),
    ] {
        let mut content = Content::with_text(source);
        content.move_to(Cursor {
            position: Position {
                line: 0,
                column: caret,
            },
            selection: selection.map(|column| Position { line: 0, column }),
        });
        Command::OpenLine.perform(&mut content);
        assert_eq!(content.text(), expected);
        assert_eq!(
            content.cursor(),
            Cursor {
                position: Position {
                    line: 0,
                    column: end
                },
                selection: None
            }
        );
    }
}
