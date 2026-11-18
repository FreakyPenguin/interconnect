//! Recover explicit user anchors without guessing identities from display names.
use crate::message_document::{Document, user_mention};
use matrix_sdk::ruma::OwnedUserId;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MentionLink {
    /// Editable Markdown link; protected in both composition modes.
    pub token: String,
    pub user_id: OwnedUserId,
    pub label: String,
}
/// Keep authored Markdown when its explicit links already represent the anchors.
/// Otherwise use canonical visible text with unambiguous links. This handles
/// duplicate labels and plain fallbacks that differ from the formatted message.
pub(crate) fn recover_mentions(
    body: &str,
    document: &Document,
    emoji: &[crate::custom_emoji::Selection],
) -> (String, Vec<MentionLink>, Vec<super::NamedMention>, bool) {
    let mut canonical = String::new();
    let mut anchors = Vec::new();
    for block in &document.blocks {
        if !canonical.is_empty() {
            canonical.push_str("\n\n");
        }
        let mut runs = block.runs.iter().peekable();
        while let Some(run) = runs.next() {
            let start = canonical.len();
            let text = if run.style.mention.is_none()
                && let crate::message_document::Inline::Image {
                    source: Some(crate::message_document::ImageSource::Matrix(uri)),
                    alt,
                    ..
                } = &run.content
            {
                emoji
                    .iter()
                    .find(|choice| &choice.emoji.uri == uri && &choice.emoji.name == alt)
                    .map_or_else(|| run.content.text(), |choice| choice.token.as_str())
            } else {
                run.content.text()
            };
            canonical.push_str(text);
            if let Some(user) = &run.style.mention {
                while runs.peek().is_some_and(|next| {
                    next.style.mention.as_ref() == Some(user) && next.style.link == run.style.link
                }) {
                    canonical.push_str(runs.next().unwrap().content.text());
                }
                anchors.push((
                    start..canonical.len(),
                    user.clone(),
                    canonical[start..].to_owned(),
                ));
            }
        }
    }
    if anchors.is_empty() {
        return (body.into(), Vec::new(), Vec::new(), false);
    }
    if canonical == body
        && anchors
            .iter()
            .any(|(_, user, label)| label != user.as_str())
    {
        let named = anchors
            .into_iter()
            .map(|(range, user_id, _)| super::NamedMention { range, user_id })
            .collect();
        return (body.into(), Vec::new(), named, true);
    }
    let mut candidates = markdown_links(body);
    // A full ID inside Markdown code/HTML or another link cannot stand in for
    // an anchor. Plain-mode messages can contain literal backticks around a
    // real mention, so those need explicit recovery rather than Markdown mode.
    let users: std::collections::BTreeSet<_> =
        anchors.iter().map(|(_, user, _)| user.clone()).collect();
    let mut literal_ids = std::collections::BTreeMap::new();
    let mut literal_depth = 0usize;
    if body.len() <= 64 * 1024 && users.len() <= 256 {
        for event in Parser::new(body) {
            match event {
                Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. }) => {
                    literal_depth += 1
                }
                Event::End(TagEnd::CodeBlock | TagEnd::Link | TagEnd::Image) => {
                    literal_depth = literal_depth.saturating_sub(1)
                }
                Event::Text(text) if literal_depth == 0 => {
                    for user in &users {
                        let protected = super::protect_user(&text, user.as_str(), '\0');
                        let count = (text.len() - protected.len()) / (user.as_str().len() - 1);
                        *literal_ids.entry(user.clone()).or_insert(0usize) += count;
                    }
                }
                _ => {}
            }
        }
    }
    let mut remaining = candidates.clone();
    let represented = anchors.iter().all(|(_, user, label)| {
        if let Some(index) = remaining
            .iter()
            .position(|candidate| &candidate.user_id == user && &candidate.label == label)
        {
            remaining.remove(index);
            true
        } else {
            if label != user.as_str() {
                return false;
            }
            let count = literal_ids.entry(user.clone()).or_default();
            if *count == 0 {
                false
            } else {
                *count -= 1;
                true
            }
        }
    });
    if represented {
        candidates.retain(|candidate| {
            anchors
                .iter()
                .any(|(_, user, label)| &candidate.user_id == user && &candidate.label == label)
        });
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|candidate| seen.insert(candidate.clone()));
        return (body.into(), candidates, Vec::new(), false);
    }
    if let Some(named) = markdown_ranges(body, &canonical, &anchors, document) {
        return (body.into(), Vec::new(), named, false);
    }
    let named = anchors
        .into_iter()
        .map(|(range, user_id, _)| super::NamedMention { range, user_id })
        .collect();
    (canonical, Vec::new(), named, true)
}

fn markdown_links(body: &str) -> Vec<MentionLink> {
    let mut candidates = Vec::new();
    let mut current = None;
    if body.len() <= 64 * 1024 {
        for (event, range) in Parser::new(body).into_offset_iter() {
            match event {
                Event::Start(Tag::Link { dest_url, .. }) => {
                    current =
                        user_mention(&dest_url).map(|user| (range.start, user, String::new()));
                }
                Event::Text(text) | Event::Code(text) => {
                    if let Some((_, _, label)) = &mut current {
                        label.push_str(&text);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some((_, _, label)) = &mut current {
                        label.push(' ');
                    }
                }
                Event::End(TagEnd::Link) => {
                    if let Some((start, user_id, label)) = current.take() {
                        candidates.push(MentionLink {
                            token: body[start..range.end].into(),
                            user_id,
                            label,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    candidates
}
pub(super) fn refresh_links(source: &str, original: &[MentionLink]) -> Vec<MentionLink> {
    if original.is_empty() {
        return Vec::new();
    }
    let mut links: Vec<_> = original
        .iter()
        .filter(|link| !link.token.is_empty() && source.contains(&link.token))
        .cloned()
        .collect();
    for candidate in markdown_links(source) {
        if original
            .iter()
            .any(|link| link.user_id == candidate.user_id)
            && !links.contains(&candidate)
        {
            links.push(candidate);
        }
    }
    links
}

/// Recover offsets in our own Markdown source when its rendered text matches.
/// Lists/images have synthetic visible text and deliberately use the canonical
/// fallback rather than guessing between identical labels and list markers.
fn markdown_ranges(
    body: &str,
    canonical: &str,
    anchors: &[(std::ops::Range<usize>, OwnedUserId, String)],
    document: &Document,
) -> Option<Vec<super::NamedMention>> {
    if body.len() > 64 * 1024
        || document.blocks.iter().any(|block| {
            block.list_depth > 0
                || block
                    .runs
                    .iter()
                    .any(|run| matches!(run.content, crate::message_document::Inline::Image { .. }))
        })
    {
        return None;
    }
    let events: Vec<_> = Parser::new_ext(body, pulldown_cmark::Options::ENABLE_STRIKETHROUGH)
        .into_offset_iter()
        .take(2049)
        .collect();
    if events.len() > 2048 {
        return None;
    }
    let mut html = super::BoundedHtml(Vec::new());
    pulldown_cmark::html::write_html_io(
        &mut html,
        events.iter().map(|(event, _)| match event.clone() {
            Event::SoftBreak => Event::HardBreak,
            other => other,
        }),
    )
    .ok()?;
    let rendered = Document::parse(std::str::from_utf8(&html.0).ok()?)?;
    if rendered.text(&Default::default()) != canonical {
        return None;
    }
    let mut cursor = 0;
    let mut mapped = Vec::new();
    for (event, source) in &events {
        let (Event::Text(text) | Event::Code(text)) = event else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let start = cursor + canonical.get(cursor..)?.find(text.as_ref())?;
        cursor = start + text.len();
        if matches!(event, Event::Text(_)) && body.get(source.clone()) == Some(text.as_ref()) {
            mapped.push((start..cursor, source.clone()));
        }
    }
    anchors
        .iter()
        .map(|(range, user_id, label)| {
            let (visible, source) = mapped
                .iter()
                .find(|(visible, _)| visible.start <= range.start && visible.end >= range.end)?;
            let start = source.start + range.start - visible.start;
            let range = start..start + label.len();
            (body.get(range.clone()) == Some(label.as_str())).then(|| super::NamedMention {
                range,
                user_id: user_id.clone(),
            })
        })
        .collect()
}
