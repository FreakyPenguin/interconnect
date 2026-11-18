//! Track named occurrences through actual editor replacements, never by name search.
use super::Draft;
use iced::{advanced::text::editor::Position, widget::text_editor};
use std::ops::Range;

fn offset(source: &str, position: Position) -> usize {
    source
        .split_inclusive('\n')
        .take(position.line)
        .map(str::len)
        .sum::<usize>()
        + position.column
}
fn verified(
    old: &str,
    new: &str,
    range: Range<usize>,
    inserted: usize,
) -> Option<(Range<usize>, usize)> {
    (old.get(..range.start)? == new.get(..range.start)?
        && old.get(range.end..)? == new.get(range.start.checked_add(inserted)?..)?)
    .then_some((range, inserted))
}
impl Draft {
    pub(super) fn perform(&mut self, action: text_editor::Action) {
        if self.markdown.named_mentions.is_empty()
            || !matches!(action, text_editor::Action::Edit(_))
        {
            self.content.perform(action);
            return;
        }
        let old = self.content.text();
        let cursor = offset(&old, self.content.cursor().position);
        let selected = self.content.selection().map_or(0, |text| text.len());
        let indent = matches!(
            &action,
            text_editor::Action::Edit(text_editor::Edit::Indent | text_editor::Edit::Unindent)
        );
        let backspace = matches!(
            &action,
            text_editor::Action::Edit(text_editor::Edit::Backspace)
        );
        let delete = matches!(
            &action,
            text_editor::Action::Edit(text_editor::Edit::Delete)
        );
        self.content.perform(action);
        let new = self.content.text();
        if old == new && selected == 0 {
            return;
        }
        let changes = if indent {
            // Indent/unindent changes leading whitespace on each affected line.
            let mut changes = Vec::new();
            let mut start = 0;
            let old_lines: Vec<_> = old.split_inclusive('\n').collect();
            let new_lines: Vec<_> = new.split_inclusive('\n').collect();
            let mut valid = old_lines.len() == new_lines.len();
            for (before, after) in old_lines.into_iter().zip(new_lines) {
                let before_indent = before.len() - before.trim_start_matches([' ', '\t']).len();
                let after_indent = after.len() - after.trim_start_matches([' ', '\t']).len();
                valid &= before[before_indent..] == after[after_indent..];
                if before_indent != after_indent {
                    changes.push((start..start + before_indent, after_indent));
                }
                start += before.len();
            }
            changes.reverse();
            valid.then_some(changes)
        } else {
            let replacement = if selected > 0 {
                new.len()
                    .checked_add(selected)
                    .and_then(|len| len.checked_sub(old.len()))
                    .and_then(|inserted| {
                        let end = offset(&new, self.content.cursor().position);
                        let start = end.checked_sub(inserted)?;
                        verified(&old, &new, start..start + selected, inserted)
                    })
            } else if backspace {
                old.len().checked_sub(new.len()).and_then(|removed| {
                    verified(&old, &new, cursor.checked_sub(removed)?..cursor, 0)
                })
            } else if delete {
                old.len()
                    .checked_sub(new.len())
                    .and_then(|removed| verified(&old, &new, cursor..cursor + removed, 0))
            } else {
                new.len()
                    .checked_sub(old.len())
                    .and_then(|inserted| verified(&old, &new, cursor..cursor, inserted))
            };
            replacement.map(|change| vec![change])
        };
        if let Some(changes) = changes {
            interconnect::composition::update_named_mentions(
                &mut self.markdown.named_mentions,
                &new,
                &changes,
            );
        } else {
            // An unrecognized editor transformation must never assign a name to
            // the wrong identity. Normal character, selection and indent edits
            // take the exact-range paths above.
            self.markdown.named_mentions.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::text::editor::{Cursor, Position};
    use interconnect::composition::{self, Format, NamedMention};
    fn draft(source: &str, ranges: &[Range<usize>]) -> Draft {
        let mut draft = Draft {
            content: text_editor::Content::with_text(source),
            ..Default::default()
        };
        draft.markdown.named_mentions = ranges
            .iter()
            .enumerate()
            .map(|(i, range)| NamedMention {
                range: range.clone(),
                user_id: format!("@person{i}:example.org").parse().unwrap(),
            })
            .collect();
        draft
    }
    fn select(draft: &mut Draft, line: usize, range: Range<usize>) {
        draft.content.move_to(Cursor {
            position: Position {
                line,
                column: range.end,
            },
            selection: Some(Position {
                line,
                column: range.start,
            }),
        });
    }
    #[tokio::test]
    async fn removing_a_last_name_keeps_the_identity_in_both_modes() {
        for name in ["Maya Chen", "Zoë 李", "👩🏽‍💻 Chen"] {
            let mut draft = draft(name, std::slice::from_ref(&(0..name.len())));
            select(&mut draft, 0, name.rfind(' ').unwrap()..name.len());
            draft.perform(text_editor::Action::Edit(text_editor::Edit::Delete));
            let label = name.rsplit_once(' ').unwrap().0;
            assert_eq!(draft.content.text(), label);
            for format in [Format::Markdown, Format::Plain] {
                let prepared = composition::prepare_with_bindings(
                    draft.content.text(),
                    format,
                    draft.markdown.bindings(),
                )
                .await
                .unwrap();
                assert_eq!(prepared.document.unwrap().text(&Default::default()), label);
                assert_eq!(prepared.mentions[0].as_str(), "@person0:example.org");
            }
        }
    }
    #[tokio::test]
    async fn deleting_the_entire_name_removes_its_binding_and_notification_metadata() {
        for edit in [text_editor::Edit::Delete, text_editor::Edit::Backspace] {
            let mut draft = draft("Hi Maya Chen!", std::slice::from_ref(&(3..12)));
            select(&mut draft, 0, 3..12);
            draft.perform(text_editor::Action::Edit(edit));
            assert_eq!(draft.content.text(), "Hi !");
            assert!(draft.markdown.named_mentions.is_empty());
            let prepared = composition::prepare_with_bindings(
                draft.content.text(),
                Format::Plain,
                draft.markdown.bindings(),
            )
            .await
            .unwrap();
            assert!(prepared.mentions.is_empty());
            assert!(prepared.message_content().mentions.is_none());
        }
        let mut draft = draft("Hi Maya Chen!", std::slice::from_ref(&(3..12)));
        draft.content.move_to(Cursor {
            position: Position {
                line: 0,
                column: 12,
            },
            selection: None,
        });
        for _ in 0..9 {
            draft.perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
        }
        assert_eq!(draft.content.text(), "Hi !");
        assert!(draft.markdown.named_mentions.is_empty());
    }

    #[test]
    fn deleting_one_of_two_identical_names_keeps_the_survivors_own_identity() {
        let mut draft = draft("Maya Maya", &[0..4, 5..9]);
        select(&mut draft, 0, 0..5);
        draft.perform(text_editor::Action::Edit(text_editor::Edit::Delete));
        assert_eq!(draft.content.text(), "Maya");
        assert_eq!(draft.markdown.named_mentions.len(), 1);
        assert_eq!(
            draft.markdown.named_mentions[0].user_id.as_str(),
            "@person1:example.org"
        );
        assert_eq!(draft.markdown.named_mentions[0].range, 0..4);
        draft.perform(text_editor::Action::SelectAll);
        draft.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
            std::sync::Arc::new("Maya".into()),
        )));
        assert!(
            draft.markdown.named_mentions.is_empty(),
            "replacing the whole name must not retain its previous identity"
        );
    }
    #[test]
    fn word_selection_unicode_prefixes_and_multiline_indentation_keep_ranges_exact() {
        let mut draft = draft("Maya Chen\nJules", &[0..9, 10..15]);
        draft.perform(text_editor::Action::SelectAll);
        draft.perform(text_editor::Action::Edit(text_editor::Edit::Indent));
        let indented = draft.content.text();
        assert_eq!(
            &indented[draft.markdown.named_mentions[0].range.clone()],
            "Maya Chen"
        );
        assert_eq!(
            &indented[draft.markdown.named_mentions[1].range.clone()],
            "Jules"
        );
        draft.perform(text_editor::Action::Edit(text_editor::Edit::Unindent));
        assert_eq!(draft.content.text(), "Maya Chen\nJules");
        draft.content.move_to(Cursor {
            position: Position { line: 0, column: 2 },
            selection: None,
        });
        draft.perform(text_editor::Action::SelectWord);
        draft.perform(text_editor::Action::Edit(text_editor::Edit::Delete));
        let source = draft.content.text();
        assert_eq!(
            &source[draft.markdown.named_mentions[0].range.clone()],
            "Chen"
        );
        draft.content.move_to(Cursor {
            position: Position { line: 0, column: 0 },
            selection: None,
        });
        draft.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
            std::sync::Arc::new("👋\n".into()),
        )));
        let source = draft.content.text();
        assert_eq!(
            &source[draft.markdown.named_mentions[0].range.clone()],
            "Chen"
        );
        assert_eq!(
            &source[draft.markdown.named_mentions[1].range.clone()],
            "Jules"
        );
    }
}
