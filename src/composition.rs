//! Markdown authoring and Matrix payloads share one bounded conversion path.
use crate::message_document::{Document, safe_link};
use matrix_sdk::ruma::events::room::message::{RoomMessageEventContent, TextMessageEventContent};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
use std::{
    io::{self, Write},
    sync::Arc,
};

mod mentions;
mod named_mentions;
pub use mentions::MentionLink;
pub(crate) use mentions::recover_mentions;
pub use named_mentions::{MentionTarget, NamedMention, update_named_mentions};

#[derive(Clone, Debug, Default)]
pub struct Bindings {
    pub emoji: Vec<crate::custom_emoji::Selection>,
    pub mentions: Vec<matrix_sdk::ruma::OwnedUserId>,
    pub mention_links: Vec<MentionLink>,
    pub named_mentions: Vec<NamedMention>,
}

const MAX_BYTES: usize = 64 * 1024;
static CONVERSIONS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
const TOO_COMPLEX: &str =
    "Message is too large or complex to format. Shorten it or choose Plain text.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Format {
    #[default]
    Markdown,
    Plain,
}
#[derive(Clone)]
pub struct Prepared {
    pub source: String,
    pub html: Option<String>,
    pub document: Option<Arc<Document>>,
    pub mentions: Vec<matrix_sdk::ruma::OwnedUserId>,
}
impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("formatted", &self.html.is_some())
            .finish_non_exhaustive()
    }
}
impl Prepared {
    pub fn plain(source: String) -> Self {
        Self {
            source,
            html: None,
            document: None,
            mentions: Vec::new(),
        }
    }
    pub fn text_content(&self) -> TextMessageEventContent {
        match &self.html {
            Some(html) => TextMessageEventContent::html(self.source.clone(), html.clone()),
            None => TextMessageEventContent::plain(self.source.clone()),
        }
    }
    pub fn message_content(&self) -> RoomMessageEventContent {
        let content = RoomMessageEventContent::new(
            matrix_sdk::ruma::events::room::message::MessageType::Text(self.text_content()),
        );
        if self.mentions.is_empty() {
            content
        } else {
            content.add_mentions(matrix_sdk::ruma::events::Mentions::with_user_ids(
                self.mentions.clone(),
            ))
        }
    }
}
/// A conservative fast path, never a Markdown parser. All possible syntax and
/// line/indentation structure goes to CommonMark; ordinary chat text stays literal.
pub fn needs_conversion(source: &str, format: Format) -> bool {
    format == Format::Markdown
        && (source.starts_with(char::is_whitespace)
            || source.contains([
                '*', '_', '~', '`', '[', ']', '<', '>', '#', '\\', '&', '\n', '\r', '=', '-', '+',
                '.', '!', '$',
            ]))
}
pub async fn prepare(source: String, format: Format) -> Result<Prepared, String> {
    prepare_with_emoji(source, format, Vec::new()).await
}
pub async fn prepare_with_emoji(
    source: String,
    format: Format,
    emoji: Vec<crate::custom_emoji::Selection>,
) -> Result<Prepared, String> {
    prepare_with_mentions(source, format, emoji, Vec::new()).await
}
pub async fn prepare_with_mentions(
    source: String,
    format: Format,
    emoji: Vec<crate::custom_emoji::Selection>,
    mentions: Vec<matrix_sdk::ruma::OwnedUserId>,
) -> Result<Prepared, String> {
    prepare_with_mention_links(source, format, emoji, mentions, Vec::new()).await
}
pub async fn prepare_with_mention_links(
    source: String,
    format: Format,
    emoji: Vec<crate::custom_emoji::Selection>,
    mentions: Vec<matrix_sdk::ruma::OwnedUserId>,
    mention_links: Vec<MentionLink>,
) -> Result<Prepared, String> {
    prepare_with_bindings(
        source,
        format,
        Bindings {
            emoji,
            mentions,
            mention_links,
            ..Default::default()
        },
    )
    .await
}
pub async fn prepare_with_bindings(
    source: String,
    format: Format,
    bindings: Bindings,
) -> Result<Prepared, String> {
    let Bindings {
        mut emoji,
        mut mentions,
        mention_links,
        mut named_mentions,
    } = bindings;
    named_mentions.retain(|mention| mention.valid(&source));
    emoji.retain(|choice| !choice.token.is_empty() && source.contains(&choice.token));
    mentions.retain(|user| source.contains(user.as_str()));
    mentions.sort();
    mentions.dedup();
    if source.trim().is_empty() {
        return Err("Message is empty".into());
    }
    if !needs_conversion(&source, format)
        && emoji.is_empty()
        && mentions.is_empty()
        && mention_links.is_empty()
        && named_mentions.is_empty()
    {
        return Ok(Prepared::plain(source));
    }
    if source.len() > MAX_BYTES {
        return Err(TOO_COMPLEX.into());
    }
    let permit = CONVERSIONS
        .acquire()
        .await
        .map_err(|_| "Formatting is unavailable".to_owned())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        convert_with_choices(
            source,
            format,
            Bindings {
                emoji,
                mentions,
                mention_links,
                named_mentions,
            },
        )
    })
    .await
    .map_err(|_| "Could not format message".to_owned())?
}
fn convert_with_choices(
    source: String,
    format: Format,
    bindings: Bindings,
) -> Result<Prepared, String> {
    let Bindings {
        emoji,
        mentions,
        mention_links,
        mut named_mentions,
    } = bindings;
    if source.len() > MAX_BYTES {
        return Err(TOO_COMPLEX.into());
    }
    let mention_links = mentions::refresh_links(&source, &mention_links);
    if emoji.len() > 256
        || mentions.len() > 256
        || mention_links.len() > 256
        || named_mentions.len() > 256
    {
        return Err(TOO_COMPLEX.into());
    }
    // Protect explicit choices before CommonMark sees underscores in shortcodes.
    // Private-use markers are chosen outside the source and restored in all
    // literal contexts; they never reach the payload or canonical copy text.
    let mut markers = Vec::new();
    let mut protected = source.clone();
    let mut available = ('\u{e000}'..='\u{f8ff}').filter(|ch| !source.contains(*ch));
    named_mentions.sort_by_key(|mention| mention.range.start);
    let mut end = 0;
    for mention in &named_mentions {
        if !mention.valid(&source) || mention.range.start < end {
            return Err(TOO_COMPLEX.into());
        }
        end = mention.range.end;
    }
    // Replace occurrences from the end so duplicate names retain separate IDs.
    for mention in named_mentions.iter().rev() {
        let marker = available.next().ok_or_else(|| TOO_COMPLEX.to_owned())?;
        let label = &source[mention.range.clone()];
        protected.replace_range(mention.range.clone(), &marker.to_string());
        markers.push((marker, Choice::Named(&mention.target, label)));
    }
    // Protect complete recovered anchors first; IDs inside their destinations
    // must never be mistaken for newly typed full-ID mentions.
    for link in &mention_links {
        let marker = available.next().ok_or_else(|| TOO_COMPLEX.to_owned())?;
        protected = protected.replace(&link.token, &marker.to_string());
        markers.push((marker, Choice::Link(link)));
    }
    for choice in &emoji {
        if choice.token.is_empty() || !choice.emoji.uri.is_valid() {
            continue;
        }
        let marker = available.next().ok_or_else(|| TOO_COMPLEX.to_owned())?;
        protected = protected.replace(&choice.token, &marker.to_string());
        markers.push((marker, Choice::Emoji(choice)));
    }
    // Longest IDs first prevent a shorter selected ID from consuming a prefix.
    let mut users: Vec<_> = mentions.iter().collect();
    users.sort_by_key(|user| std::cmp::Reverse(user.as_str().len()));
    for user in users {
        let marker = available.next().ok_or_else(|| TOO_COMPLEX.to_owned())?;
        protected = protect_user(&protected, user.as_str(), marker);
        markers.push((marker, Choice::User(user)));
    }
    let mut mentioned = std::collections::BTreeSet::new();
    let restore = |text: &str| {
        let mut text = text.to_owned();
        for (marker, choice) in &markers {
            text = text.replace(*marker, choice.token());
        }
        text
    };
    let mut events = Vec::new();
    let mut depth = 0usize;
    let mut links = Vec::new();
    let mut has_display_math = false;
    let parsed: Box<dyn Iterator<Item = Event<'_>>> = if format == Format::Plain {
        Box::new(protected.split('\n').enumerate().flat_map(|(index, line)| {
            (index > 0)
                .then_some(Event::HardBreak)
                .into_iter()
                .chain(std::iter::once(Event::Text(line.into())))
        }))
    } else {
        Box::new(Parser::new_ext(
            &protected,
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_MATH,
        ))
    };
    let mut code = false;
    let mut image_depth = 0usize;
    for (count, event) in parsed.enumerate() {
        if count >= 2048 {
            return Err(TOO_COMPLEX.into());
        }
        if matches!(event, Event::Start(_)) {
            depth += 1;
            if depth > 24 {
                return Err(TOO_COMPLEX.into());
            }
        }
        if matches!(event, Event::End(_)) {
            depth = depth.saturating_sub(1);
        }
        let event = match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                code = true;
                let kind = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => {
                        pulldown_cmark::CodeBlockKind::Fenced(restore(&info).into())
                    }
                    kind => kind,
                };
                Event::Start(Tag::CodeBlock(kind))
            }
            Event::End(TagEnd::CodeBlock) => {
                code = false;
                Event::End(TagEnd::CodeBlock)
            }
            Event::Text(ref text) if !code && image_depth == 0 && !markers.is_empty() => {
                selected_text(
                    text,
                    &markers,
                    &mut events,
                    &mut mentioned,
                    links.is_empty(),
                );
                if events.len() > 4096 {
                    return Err(TOO_COMPLEX.into());
                }
                continue;
            }
            Event::Text(text) => Event::Text(restore(&text).into()),
            Event::Code(text) => Event::Code(restore(&text).into()),
            Event::InlineMath(latex) => {
                Event::InlineHtml(math_html(&restore(&latex), false).into())
            }
            Event::DisplayMath(latex) => {
                has_display_math = true;
                Event::Html(math_html(&restore(&latex), true).into())
            }
            // Chat newlines are intentional. Raw HTML is literal source text.
            Event::SoftBreak => Event::HardBreak,
            Event::Html(text) | Event::InlineHtml(text) => Event::Text(restore(&text).into()),
            Event::Start(Tag::HtmlBlock) => Event::Start(Tag::Paragraph),
            Event::End(TagEnd::HtmlBlock) => Event::End(TagEnd::Paragraph),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = restore(&dest_url);
                let link = safe_link(&dest_url);
                links.push(link.is_some());
                let Some(link) = link else {
                    continue;
                };
                Event::Start(Tag::Link {
                    link_type,
                    dest_url: link.into(),
                    title: restore(&title).into(),
                    id: restore(&id).into(),
                })
            }
            Event::End(TagEnd::Link) if !links.pop().unwrap_or(false) => continue,
            // Inline-image authoring is deferred. Retain alternative text without
            // emitting a remote resource request for other Matrix clients.
            Event::Start(Tag::Image { .. }) => {
                image_depth += 1;
                continue;
            }
            Event::End(TagEnd::Image) => {
                image_depth = image_depth.saturating_sub(1);
                continue;
            }
            event => event,
        };
        events.push(event);
    }
    let mut output = BoundedHtml(Vec::new());
    html::write_html_io(&mut output, events.into_iter()).map_err(|_| TOO_COMPLEX.to_owned())?;
    let mut html = String::from_utf8(output.0).expect("CommonMark emits UTF-8");
    if has_display_math {
        // CommonMark math events can occur inside a paragraph. Normalize the
        // fragment so Matrix's display-math div is never sent nested in a p.
        html = matrix_sdk::ruma::html::Html::parse(&html).to_string();
        if html.len() > MAX_BYTES {
            return Err(TOO_COMPLEX.into());
        }
    }
    let document = Document::parse_with_formulas(&html).ok_or_else(|| TOO_COMPLEX.to_owned())?;
    Ok(Prepared {
        source,
        html: Some(html),
        document: Some(Arc::new(document)),
        mentions: mentioned.into_iter().collect(),
    })
}
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn protect_user(source: &str, user: &str, marker: char) -> String {
    let mut output = String::new();
    let mut copied = 0;
    for (start, _) in source.match_indices(user) {
        let end = start + user.len();
        let before = source[..start].chars().next_back();
        let after = &source[end..];
        let boundary = before.is_none_or(|c| {
            c.is_whitespace() || matches!(c, '(' | '[' | '{' | '\'' | '"' | '`' | '*' | '~')
        }) && (after.chars().next().is_none_or(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    ',' | ';' | '!' | '?' | ')' | ']' | '}' | '`' | '*' | '~' | '\'' | '"'
                )
        }) || after
            .strip_prefix('.')
            .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace)));
        if boundary {
            output.push_str(&source[copied..start]);
            output.push(marker);
            copied = end;
        }
    }
    output.push_str(&source[copied..]);
    output
}
enum Choice<'a> {
    Emoji(&'a crate::custom_emoji::Selection),
    User(&'a matrix_sdk::ruma::OwnedUserId),
    Link(&'a MentionLink),
    Named(&'a MentionTarget, &'a str),
}
impl Choice<'_> {
    fn token(&self) -> &str {
        match self {
            Self::Emoji(choice) => &choice.token,
            Self::User(user) => user.as_str(),
            Self::Link(link) => &link.token,
            Self::Named(_, label) => label,
        }
    }
}
fn selected_text<'a>(
    text: &str,
    choices: &[(char, Choice<'_>)],
    events: &mut Vec<Event<'a>>,
    mentioned: &mut std::collections::BTreeSet<matrix_sdk::ruma::OwnedUserId>,
    allow_users: bool,
) {
    let mut start = 0;
    for (offset, ch) in text.char_indices() {
        let Some((_, choice)) = choices.iter().find(|(marker, _)| *marker == ch) else {
            continue;
        };
        if offset > start {
            events.push(Event::Text(text[start..offset].to_owned().into()));
        }
        if let Choice::Named(user, label) = choice {
            if allow_users {
                events.push(Event::InlineHtml(
                    format!(
                        "<a href=\"{}\">{}</a>",
                        escaped(&user.uri()),
                        escaped(label).replace('\n', "<br>")
                    )
                    .into(),
                ));
                if let Some(user) = user.user_id() {
                    mentioned.insert(user.clone());
                }
            } else {
                events.push(Event::Text((*label).to_owned().into()));
            }
            start = offset + ch.len_utf8();
            if events.len() > 4096 {
                return;
            }
            continue;
        }
        if let Choice::Link(link) = choice {
            if allow_users {
                events.push(Event::InlineHtml(
                    format!(
                        "<a href=\"{}\">{}</a>",
                        escaped(&link.target.uri()),
                        escaped(&link.label).replace('\n', "<br>")
                    )
                    .into(),
                ));
                if let Some(user) = link.target.user_id() {
                    mentioned.insert(user.clone());
                }
            } else {
                events.push(Event::Text(link.token.clone().into()));
            }
            start = offset + ch.len_utf8();
            if events.len() > 4096 {
                return;
            }
            continue;
        }
        if let Choice::User(user) = choice {
            if !allow_users {
                events.push(Event::Text(user.as_str().to_owned().into()));
                start = offset + ch.len_utf8();
                if events.len() > 4096 {
                    return;
                }
                continue;
            }
            events.push(Event::InlineHtml(
                format!(
                    "<a href=\"{}\">{}</a>",
                    escaped(&user.matrix_to_uri().to_string()),
                    escaped(user.as_str())
                )
                .into(),
            ));
            mentioned.insert((*user).clone());
            start = offset + ch.len_utf8();
            if events.len() > 4096 {
                return;
            }
            continue;
        }
        let Choice::Emoji(choice) = choice else {
            unreachable!()
        };
        let emoji = &choice.emoji;
        events.push(Event::InlineHtml(
            format!(
                "<img data-mx-emoticon src=\"{}\" alt=\"{}\" title=\"{}\" height=\"32\" />",
                escaped(emoji.uri.as_str()),
                escaped(&emoji.name),
                escaped(&emoji.shortcode)
            )
            .into(),
        ));
        start = offset + ch.len_utf8();
        if events.len() > 4096 {
            return;
        }
    }
    if start < text.len() {
        events.push(Event::Text(text[start..].to_owned().into()));
    }
}
fn math_html(latex: &str, display: bool) -> String {
    let escaped = latex
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    let tag = if display { "div" } else { "span" };
    format!("<{tag} data-mx-maths=\"{escaped}\"><code>{escaped}</code></{tag}>")
}
struct BoundedHtml(Vec<u8>);
impl Write for BoundedHtml {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len() + bytes.len() > MAX_BYTES {
            return Err(io::Error::other(TOO_COMPLEX));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_document::{BlockKind, Inline};
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn room_labels_roundtrip_edits_without_creating_notification_mentions() {
        for format in [Format::Plain, Format::Markdown] {
            let source = "Maya in #Design team";
            let bindings = Bindings {
                named_mentions: vec![
                    NamedMention {
                        range: 0..4,
                        target: MentionTarget::User {
                            user_id: "@maya:example.org".parse().unwrap(),
                        },
                    },
                    NamedMention {
                        range: 8..20,
                        target: MentionTarget::Room {
                            room_id: "!design:example.org".parse().unwrap(),
                            via: vec!["route.example.org".parse().unwrap()],
                        },
                    },
                ],
                ..Default::default()
            };
            let prepared = prepare_with_bindings(source.into(), format, bindings.clone())
                .await
                .unwrap();
            let wire = serde_json::to_value(prepared.message_content()).unwrap();
            assert_eq!(
                wire["m.mentions"]["user_ids"],
                serde_json::json!(["@maya:example.org"])
            );
            assert!(wire["m.mentions"].get("room").is_none());
            let document = prepared.document.unwrap();
            let (body, links, names, _) = recover_mentions(source, &document, &[]);
            assert_eq!(body, source);
            assert!(links.is_empty());
            assert_eq!(names, bindings.named_mentions);
            let room = prepare_with_bindings(
                "#Design team".into(),
                format,
                Bindings {
                    named_mentions: vec![NamedMention {
                        range: 0..12,
                        target: bindings.named_mentions[1].target.clone(),
                    }],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert!(room.mentions.is_empty());
            assert!(room.html.unwrap().contains("via=route.example.org"));
        }
    }
    #[tokio::test]
    async fn selected_users_emit_links_and_notification_metadata_in_both_modes() {
        let user: matrix_sdk::ruma::OwnedUserId = "@maya:example.org".parse().unwrap();
        for format in [Format::Plain, Format::Markdown] {
            let prepared = prepare_with_mentions(
                "Hi \"@maya:example.org\" and @maya:example.org! @room @other:example.org".into(),
                format,
                vec![],
                vec![user.clone(), user.clone()],
            )
            .await
            .unwrap();
            assert_eq!(prepared.mentions, vec![user.clone()]);
            let wire = serde_json::to_value(prepared.message_content()).unwrap();
            assert_eq!(
                wire["m.mentions"]["user_ids"],
                serde_json::json!(["@maya:example.org"])
            );
            assert!(wire["m.mentions"].get("room").is_none());
            assert_eq!(
                wire["formatted_body"]
                    .as_str()
                    .unwrap()
                    .matches("<a href=")
                    .count(),
                2
            );
            assert_eq!(
                prepared.document.unwrap().text(&Default::default()),
                prepared.source
            );
        }
        assert!(
            prepare("@maya:example.org".into(), Format::Plain)
                .await
                .unwrap()
                .mentions
                .is_empty()
        );
    }
    #[tokio::test]
    async fn mention_choices_stay_literal_in_code_html_links_and_image_alternatives() {
        let user: matrix_sdk::ruma::OwnedUserId = "@maya:example.org".parse().unwrap();
        for source in [
            "`@maya:example.org`",
            "```\n@maya:example.org\n```",
            "[else](https://example.org/@maya:example.org)",
            "[@maya:example.org](https://example.org)",
            "![alt @maya:example.org](https://example.org/image)",
            "<span title=\"@maya:example.org\">literal</span>",
        ] {
            let prepared =
                prepare_with_mentions(source.into(), Format::Markdown, vec![], vec![user.clone()])
                    .await
                    .unwrap();
            assert!(prepared.mentions.is_empty(), "{source}");
            assert!(!prepared.html.unwrap().contains("matrix.to"), "{source}");
        }
        let prepared = prepare_with_mentions(
            "@maya:example.org.evil @maya:example.orgsuffix x@maya:example.org".into(),
            Format::Plain,
            vec![],
            vec![user],
        )
        .await
        .unwrap();
        assert!(prepared.mentions.is_empty());
        assert!(prepared.html.unwrap().contains("@maya:example.orgsuffix"));
    }
    fn choice(shortcode: &str) -> crate::custom_emoji::Selection {
        crate::custom_emoji::Selection {
            token: format!(":{shortcode}:"),
            emoji: crate::custom_emoji::Emoji {
                id: "pack/one".into(),
                shortcode: shortcode.into(),
                pack: "Cats".into(),
                name: "A cat \"<&>".into(),
                uri: "mxc://example.org/cat".into(),
            },
        }
    }
    #[tokio::test]
    async fn custom_emoji_payload_is_explicit_escaped_and_literal_inside_code() {
        for name in ["wave", "blob_wave", "a_b_c", "_wave_"] {
            let choice = choice(name);
            let token = &choice.token;
            let source = format!(
                "Hello {token} **{token}** `{token}`\n\n```\n{token}\n```\n\n![{token}](https://example.org/image)"
            );
            let prepared =
                prepare_with_emoji(source.clone(), Format::Markdown, vec![choice.clone()])
                    .await
                    .unwrap();
            let html = prepared.html.as_ref().unwrap();
            assert_eq!(
                html.matches("data-mx-emoticon").count(),
                2,
                "{name}: {html}"
            );
            assert!(html.contains("A cat &quot;&lt;&amp;&gt;"));
            assert_eq!(prepared.message_content().body(), source);
            assert_eq!(prepared.document.unwrap().image_keys().len(), 1);
            assert!(
                !prepare(source.clone(), Format::Markdown)
                    .await
                    .unwrap()
                    .html
                    .unwrap()
                    .contains("data-mx-emoticon")
            );
            let literal = prepare_with_emoji(
                format!("<b> **literal** {token}"),
                Format::Plain,
                vec![choice.clone()],
            )
            .await
            .unwrap()
            .html
            .unwrap();
            assert!(literal.contains("&lt;b&gt; **literal**"));
            assert_eq!(literal.matches("data-mx-emoticon").count(), 1);
        }
    }

    #[tokio::test]
    async fn math_preview_and_payload_use_matrix_markup_and_literal_code_stays_literal() {
        let source = "Inline $\\frac{a}{b}$ end.\n\n$$\\sum_{n=1}^{10} n$$\n\n`$literal$`\n\n```tex\n$x^2$\n```";
        let prepared = prepare(source.into(), Format::Markdown).await.unwrap();
        let html = prepared.html.as_ref().unwrap();
        assert!(html.contains(r#"<span data-mx-maths="\frac{a}{b}">"#));
        assert!(html.contains(r#"<div data-mx-maths="\sum_{n=1}^{10} n">"#));
        assert!(!html.contains("<p><div"));
        assert!(html.contains("<code>$literal$</code>"));
        assert!(html.contains("$x^2$\n</code>"));
        assert_eq!(prepared.message_content().body(), source);
        assert!(
            prepared
                .document
                .unwrap()
                .blocks
                .iter()
                .any(|b| matches!(b.kind, BlockKind::Math))
        );
        assert!(
            prepare(source.into(), Format::Plain)
                .await
                .unwrap()
                .html
                .is_none()
        );
        assert!(
            prepare("$x$".into(), Format::Markdown)
                .await
                .unwrap()
                .html
                .unwrap()
                .contains("data-mx-maths")
        );
    }

    #[tokio::test]
    async fn math_attributes_escape_untrusted_source_and_unsupported_math_keeps_fallback() {
        let prepared = prepare(
            r#"$\text{a" <b> & c}$ and $\unsupported{x}$"#.into(),
            Format::Markdown,
        )
        .await
        .unwrap();
        let html = prepared.html.unwrap();
        assert!(!html.contains("<b>"));
        assert!(html.contains("&quot; &lt;b&gt; &amp; c"));
        let text = prepared.document.unwrap().text(&BTreeSet::new());
        assert!(text.contains(r"\unsupported{x}"));
    }
    #[tokio::test]
    async fn commonmark_payload_and_preview_share_source_and_formatting() {
        let source = "# Title\n\n**bold _italic_** and ~~old~~ with `code`\nnext\n\n> quote\n\n3. third\n4. fourth\n\n```rust\nlet x = 1;\n  x\n```";
        let prepared = prepare(source.into(), Format::Markdown).await.unwrap();
        assert_eq!(prepared.source, source);
        let content = prepared.message_content();
        let wire = serde_json::to_value(content).unwrap();
        assert_eq!(wire["body"], source);
        assert_eq!(wire["format"], "org.matrix.custom.html");
        assert_eq!(
            wire["formatted_body"],
            prepared.html.as_ref().unwrap().as_str()
        );
        let doc = prepared.document.unwrap();
        assert!(doc.blocks.iter().any(|block| matches!(&block.kind, BlockKind::Code { language: Some(language) } if language == "rust")));
        assert!(
            doc.blocks
                .iter()
                .flat_map(|block| &block.runs)
                .any(|run| run.style.bold && run.style.italic)
        );
        assert!(doc.text(&BTreeSet::new()).contains("code\nnext"));
    }
    #[tokio::test]
    async fn literal_mode_escaping_and_unsafe_links_never_emit_active_html() {
        let source = "**literal** <b>HTML</b> & stuff";
        let plain = prepare(source.into(), Format::Plain).await.unwrap();
        assert!(plain.html.is_none());
        assert_eq!(plain.source, source);
        let prepared = prepare("<script>alert(1)</script>\n\n[bad](javascript:alert) and ![diagram](https://example.org/image) and \\*literal\\*".into(), Format::Markdown).await.unwrap();
        let html = prepared.html.unwrap();
        assert!(!html.contains("<script>"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("<img"));
        let doc = prepared.document.unwrap();
        let text = doc.text(&BTreeSet::new());
        assert!(text.contains("<script>alert(1)</script>"));
        assert!(text.contains("diagram"));
        assert!(text.contains("*literal*"));
        assert!(
            doc.blocks
                .iter()
                .flat_map(|block| &block.runs)
                .all(|run| matches!(run.content, Inline::Text(_)))
        );
    }
    #[tokio::test]
    async fn ordinary_text_is_literal_and_formatting_is_bounded() {
        let plain = prepare("hello café 👋".into(), Format::Markdown)
            .await
            .unwrap();
        assert!(plain.html.is_none());
        assert!(prepare(" ".into(), Format::Markdown).await.is_err());
        assert!(
            prepare(format!("**{}**", "a".repeat(MAX_BYTES)), Format::Markdown)
                .await
                .is_err()
        );
        assert!(
            prepare(format!("{}text", "> ".repeat(30)), Format::Markdown)
                .await
                .is_err()
        );
        assert!(prepare("a ".repeat(MAX_BYTES), Format::Plain).await.is_ok());
    }
}
