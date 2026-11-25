//! Native multiline commands that need the current logical cursor or selection.
//! Resolve them only while handling an authorized form/draft update, with no
//! speculative changes to a renderer's preferred column, affinity, or scrolling.
use iced::advanced::text::editor::Editor as _;
use iced::{
    keyboard::{Key, Modifiers, key::Named},
    widget::text_editor::{self, Action, Binding, Content, Cursor, Edit, Motion, Position},
};

#[derive(Clone, Copy, Debug)]
pub(super) enum Command {
    Paragraph { forward: bool, select: bool },
    DeleteTo(Motion),
    DeleteParagraphEnd,
    OpenLine,
}

pub(super) fn command(press: &text_editor::KeyPress) -> Option<Command> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    let base = press.modifiers - Modifiers::SHIFT;
    let word_modifier = if cfg!(target_os = "macos") {
        Modifiers::ALT
    } else {
        Modifiers::CTRL
    };
    if base == word_modifier {
        match press.key.as_ref() {
            Key::Named(Named::Backspace) => return Some(Command::DeleteTo(Motion::WordLeft)),
            Key::Named(Named::Delete) => return Some(Command::DeleteTo(Motion::WordRight)),
            _ => {}
        }
    }
    if !cfg!(target_os = "macos") {
        return None;
    }
    match press.key.as_ref() {
        Key::Named(Named::ArrowUp | Named::ArrowDown) if base == Modifiers::ALT => {
            Some(Command::Paragraph {
                forward: press.key == Key::Named(Named::ArrowDown),
                select: press.modifiers.shift(),
            })
        }
        Key::Named(Named::Backspace) if base == Modifiers::ALT => {
            Some(Command::DeleteTo(Motion::WordLeft))
        }
        Key::Named(Named::Delete) if base == Modifiers::ALT => {
            Some(Command::DeleteTo(Motion::WordRight))
        }
        Key::Named(Named::Backspace) if base == Modifiers::LOGO => {
            Some(Command::DeleteTo(Motion::Home))
        }
        Key::Named(Named::Delete) if base == Modifiers::LOGO => {
            Some(Command::DeleteTo(Motion::End))
        }
        Key::Character("k") if press.modifiers == Modifiers::CTRL => {
            Some(Command::DeleteParagraphEnd)
        }
        Key::Character("o") if press.modifiers == Modifiers::CTRL => Some(Command::OpenLine),
        _ => None,
    }
}

pub(super) fn binding<Message>(
    press: text_editor::KeyPress,
    message: impl FnOnce(Command) -> Message,
) -> Option<Binding<Message>> {
    if let Some(command) = command(&press) {
        Some(Binding::Custom(message(command)))
    } else {
        Binding::from_key_press(press)
    }
}

pub(super) struct Prepared {
    pub edit: Option<Action>,
    restore: Option<Cursor>,
}
impl Prepared {
    pub fn finish(self, content: &mut Content) {
        if let Some(cursor) = self.restore {
            move_to(content, cursor);
        }
    }
}
fn line_len(content: &Content, line: usize) -> usize {
    content.with_editor(|editor| editor.line(line).map_or(0, |line| line.text.len()))
}
fn move_to(content: &mut Content, cursor: Cursor) {
    let cursor = Cursor {
        selection: cursor.selection.filter(|anchor| *anchor != cursor.position),
        ..cursor
    };
    if content.cursor() == cursor {
        return;
    }
    // Iced's move_to does not clear a preceding selection for a None anchor.
    content.perform(Action::Move(Motion::DocumentStart));
    content.move_to(cursor);
}
impl Command {
    pub(super) fn prepare(self, content: &mut Content) -> Prepared {
        let cursor = content.cursor();
        let selected = cursor
            .selection
            .is_some_and(|anchor| anchor != cursor.position);
        let mut prepared = Prepared {
            edit: None,
            restore: None,
        };
        match self {
            Self::Paragraph { forward, select } => {
                let mut position = cursor.position;
                if !select && let Some(anchor) = cursor.selection {
                    let anchor_before =
                        (anchor.line, anchor.column) < (position.line, position.column);
                    if anchor_before != forward {
                        position = anchor;
                    }
                }
                let end = line_len(content, position.line);
                if forward {
                    if position.column == end && position.line + 1 < content.line_count() {
                        position.line += 1;
                    }
                    position.column = line_len(content, position.line);
                } else {
                    if position.column == 0 && position.line > 0 {
                        position.line -= 1;
                    }
                    position.column = 0;
                }
                move_to(
                    content,
                    Cursor {
                        position,
                        selection: select.then_some(cursor.selection.unwrap_or(cursor.position)),
                    },
                );
            }
            Self::DeleteTo(motion) => {
                if !selected {
                    content.perform(Action::Select(motion));
                }
                if content
                    .cursor()
                    .selection
                    .is_some_and(|anchor| anchor != content.cursor().position)
                {
                    prepared.edit = Some(Action::Edit(Edit::Delete));
                }
            }
            Self::DeleteParagraphEnd => {
                if !selected {
                    let end = line_len(content, cursor.position.line);
                    let position = if cursor.position.column == end
                        && cursor.position.line + 1 < content.line_count()
                    {
                        Position {
                            line: cursor.position.line + 1,
                            column: 0,
                        }
                    } else {
                        Position {
                            line: cursor.position.line,
                            column: end,
                        }
                    };
                    move_to(
                        content,
                        Cursor {
                            position,
                            selection: Some(cursor.position),
                        },
                    );
                }
                if content
                    .cursor()
                    .selection
                    .is_some_and(|anchor| anchor != content.cursor().position)
                {
                    prepared.edit = Some(Action::Edit(Edit::Delete));
                }
            }
            Self::OpenLine => {
                let start = cursor
                    .selection
                    .filter(|anchor| {
                        (anchor.line, anchor.column)
                            < (cursor.position.line, cursor.position.column)
                    })
                    .unwrap_or(cursor.position);
                prepared.edit = Some(Action::Edit(Edit::Enter));
                prepared.restore = Some(Cursor {
                    position: start,
                    selection: None,
                });
            }
        }
        prepared
    }
    pub(super) fn perform(self, content: &mut Content) {
        let prepared = self.prepare(content);
        if let Some(action) = prepared.edit.clone() {
            content.perform(action);
        }
        prepared.finish(content);
    }
}
