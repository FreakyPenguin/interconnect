//! Per-occurrence identities for editable, visible mention labels.
use matrix_sdk::ruma::OwnedUserId;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NamedMention {
    pub range: Range<usize>,
    pub user_id: OwnedUserId,
}
impl NamedMention {
    pub fn valid(&self, source: &str) -> bool {
        self.range.start < self.range.end
            && source
                .get(self.range.clone())
                .is_some_and(|label| !label.trim().is_empty())
    }
}
/// Changes are exact editor replacements in descending source order. Identity
/// follows surviving label characters; replacing/deleting an entire name drops it.
pub fn update_named_mentions(
    mentions: &mut Vec<NamedMention>,
    source: &str,
    changes: &[(Range<usize>, usize)],
) {
    for (edit, inserted) in changes {
        let shift = |offset: usize| offset.saturating_sub(edit.end - edit.start) + inserted;
        mentions.retain_mut(|mention| {
            let range = &mut mention.range;
            if edit.end <= range.start {
                range.start = shift(range.start);
                range.end = shift(range.end);
            } else if edit.start >= range.end {
                // Appending body text at a mention boundary does not extend it.
            } else if edit.start <= range.start && edit.end >= range.end {
                return false;
            } else {
                if edit.end < range.end {
                    range.end = shift(range.end);
                } else {
                    range.end = edit.start + if edit.end == range.end { *inserted } else { 0 };
                }
                if edit.start < range.start {
                    range.start = edit.start + inserted;
                }
            }
            range.start < range.end
        });
    }
    mentions.retain_mut(|mention| {
        let Some(label) = source.get(mention.range.clone()) else {
            return false;
        };
        let trimmed = label.trim();
        mention.range.start += label.len() - label.trim_start().len();
        mention.range.end = mention.range.start + trimmed.len();
        !trimmed.is_empty()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mention(range: Range<usize>, user: &str) -> NamedMention {
        NamedMention {
            range,
            user_id: user.parse().unwrap(),
        }
    }
    #[test]
    fn shortening_names_preserves_identity_but_whole_replacements_remove_it() {
        let mut names = vec![mention(3..12, "@maya:example.org")];
        update_named_mentions(&mut names, "Hi Maya!", &[(7..12, 0)]);
        assert_eq!(names[0].range, 3..7);
        update_named_mentions(&mut names, "Hi Alex!", &[(3..7, 4)]);
        assert!(names.is_empty());
    }
    #[test]
    fn exact_ranges_keep_duplicate_names_and_utf8_identities_separate() {
        let mut names = vec![
            mention(0..4, "@one:example.org"),
            mention(5..9, "@two:example.org"),
        ];
        update_named_mentions(&mut names, "Maya", &[(0..5, 0)]);
        assert_eq!(names, vec![mention(0..4, "@two:example.org")]);
        update_named_mentions(&mut names, "👋 Maya", &[(0..0, 5)]);
        assert_eq!(names[0].range, 5..9);
        update_named_mentions(&mut names, "👋 Maya and more", &[(9..9, 9)]);
        assert_eq!(names[0].range, 5..9);
    }
}
