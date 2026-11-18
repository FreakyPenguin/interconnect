//! Bounded, renderer-independent Matrix HTML documents. No network resources are loaded here.
use matrix_sdk::ruma::{
    events::room::message::{MessageFormat, MessageType},
    html::{Html, NodeRef},
};
use std::{collections::BTreeSet, ops::Range, sync::Arc};

const MAX_BYTES: usize = 64 * 1024;
const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<String>,
    /// Explicit Matrix user anchor; labels remain sender-provided text.
    pub mention: Option<matrix_sdk::ruma::OwnedUserId>,
    pub spoilers: Vec<usize>,
}

/// Image identities are independent of transport. Local assets are supplied by
/// the application, never by incoming HTML. Formula assets are rendered offline
/// on the projection/composition worker and owned by the document.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum ImageSource {
    Matrix(matrix_sdk::ruma::OwnedMxcUri),
    Local(u64),
    Formula(Arc<crate::formula::Image>),
}
impl std::fmt::Debug for ImageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Matrix(_) => "MatrixImage",
            Self::Local(_) => "LocalImage",
            Self::Formula(_) => "FormulaImage",
        })
    }
}
/// Embedded objects retain metadata even while the renderer only displays alternative text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Image {
        source: Option<ImageSource>,
        alt: String,
        width: Option<u32>,
        height: Option<u32>,
        /// Logical pixels below the text baseline; incoming HTML defaults to zero.
        descent: u32,
    },
}
impl Inline {
    pub fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Image { alt, .. } => {
                if alt.is_empty() {
                    "[Image]"
                } else {
                    alt
                }
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub content: Inline,
    pub style: Style,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BlockKind {
    #[default]
    Paragraph,
    /// Display math gets its own centered block; inline math stays in its paragraph.
    Math,
    Heading(u8),
    /// Language is deliberately independent of any syntax-highlighting library.
    Code {
        language: Option<String>,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub quote_depth: usize,
    pub list_depth: usize,
    pub runs: Vec<Run>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Document {
    source: String,
    plain: bool,
    pub blocks: Vec<Block>,
}
// Documents can contain private messages. Do not print their contents in model diagnostics.
impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("blocks", &self.blocks.len())
            .finish_non_exhaustive()
    }
}
impl Document {
    pub fn from_message(message: &MessageType, previous: Option<&Arc<Self>>) -> Option<Arc<Self>> {
        let body = match message {
            MessageType::Text(m) => &m.body,
            MessageType::Notice(m) => &m.body,
            MessageType::Emote(m) => &m.body,
            _ => return None,
        };
        let html = Self::html(message);
        let source = html.unwrap_or(body);
        if let Some(previous) =
            previous.filter(|doc| doc.source == source && doc.plain == html.is_none())
        {
            return Some(previous.clone());
        }
        html.and_then(Self::parse_with_formulas)
            .or_else(|| Self::plain_links(body))
            .map(Arc::new)
    }
    /// Literal text with clickable web URLs; no Markdown or HTML interpretation.
    /// Messages without links keep the ordinary plain-text renderer.
    pub fn plain_links(source: &str) -> Option<Self> {
        if source.len() > MAX_BYTES {
            return None;
        }
        let runs = linked_runs(source, &Style::default());
        runs.iter()
            .any(|run| run.style.link.is_some())
            .then(|| Self {
                source: source.into(),
                plain: true,
                blocks: vec![Block {
                    runs,
                    ..Block::default()
                }],
            })
    }
    pub(crate) fn html(message: &MessageType) -> Option<&str> {
        let formatted = match message {
            MessageType::Text(m) => m.formatted.as_ref(),
            MessageType::Notice(m) => m.formatted.as_ref(),
            MessageType::Emote(m) => m.formatted.as_ref(),
            _ => None,
        }?;
        (formatted.format == MessageFormat::Html).then_some(formatted.body.as_str())
    }
    /// Parse a fragment with hard input and traversal bounds; failure uses the event's plain fallback.
    pub fn parse(source: &str) -> Option<Self> {
        Self::parse_inner(source, false)
    }
    /// Parse and typeset Matrix math on a blocking worker, never on the UI thread.
    /// Unchanged documents should be reused, as in `from_message`.
    pub fn parse_with_formulas(source: &str) -> Option<Self> {
        Self::parse_inner(source, true)
    }
    fn parse_inner(source: &str, formulas: bool) -> Option<Self> {
        if source.len() > MAX_BYTES {
            return None;
        }
        let html = Html::parse(source);
        let mut builder = Builder {
            formulas,
            formula_pixels: crate::formula::MAX_DOCUMENT_PIXELS,
            ..Default::default()
        };
        for node in html.children() {
            builder.visit(&node, Style::default(), 0)?;
        }
        builder.flush();
        if builder.bytes + builder.blocks.len().saturating_sub(1) * 2 > MAX_BYTES {
            return None;
        }
        (!builder.blocks.is_empty()).then(|| Self {
            source: source.into(),
            plain: false,
            blocks: builder.blocks,
        })
    }
    pub fn image_keys(&self) -> Vec<crate::image_media::Key> {
        let mut keys = Vec::new();
        for run in self.blocks.iter().flat_map(|block| &block.runs) {
            if let Inline::Image {
                source: Some(ImageSource::Matrix(uri)),
                ..
            } = &run.content
            {
                let key = crate::image_media::Key::inline(uri.clone());
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys
    }
    /// Visible canonical text. Hidden spoilers never enter selection or clipboard text.
    pub fn text(&self, revealed: &BTreeSet<usize>) -> String {
        self.visible_blocks(revealed)
            .into_iter()
            .map(|b| {
                b.runs
                    .into_iter()
                    .map(|r| r.content.text().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    pub fn visible_blocks(&self, revealed: &BTreeSet<usize>) -> Vec<Block> {
        let mut hidden = BTreeSet::new();
        self.blocks
            .iter()
            .map(|block| {
                let mut visible = block.clone();
                visible.runs = block
                    .runs
                    .iter()
                    .filter_map(|run| {
                        if let Some(id) =
                            run.style.spoilers.iter().find(|id| !revealed.contains(id))
                        {
                            if !hidden.insert(*id) {
                                return None;
                            }
                            Some(Run {
                                content: Inline::Text("[Spoiler — click to reveal]".into()),
                                style: Style {
                                    spoilers: vec![*id],
                                    ..Style::default()
                                },
                            })
                        } else {
                            Some(run.clone())
                        }
                    })
                    .collect();
                visible
            })
            .collect()
    }
}
/// Split only text runs. Existing anchors, code and embedded objects stay intact.
fn linked_runs(text: &str, style: &Style) -> Vec<Run> {
    let mut finder = linkify::LinkFinder::new();
    finder.kinds(&[linkify::LinkKind::Url]);
    finder.url_must_have_scheme(false);
    let mut runs = Vec::new();
    let mut end = 0;
    for found in finder.links(text) {
        let value = found.as_str();
        let destination = if value
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("www."))
        {
            safe_link(&format!("https://{value}"))
        } else {
            safe_link(value).filter(|url| url.starts_with("https:") || url.starts_with("http:"))
        };
        let Some(destination) = destination else {
            continue;
        };
        if found.start() > end {
            runs.push(Run {
                content: Inline::Text(text[end..found.start()].into()),
                style: style.clone(),
            });
        }
        let mut link_style = style.clone();
        link_style.link = Some(destination);
        runs.push(Run {
            content: Inline::Text(value.into()),
            style: link_style,
        });
        end = found.end();
    }
    if end < text.len() {
        runs.push(Run {
            content: Inline::Text(text[end..].into()),
            style: style.clone(),
        });
    }
    runs
}
/// The opener accepts only these explicit schemes; relative and executable URLs are inert.
pub fn safe_link(value: &str) -> Option<String> {
    if value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let parsed = url::Url::parse(value).ok()?;
    match parsed.scheme() {
        "https" | "http" if parsed.host_str().is_some() => Some(parsed.into()),
        "mailto" | "matrix" if !parsed.path().is_empty() => Some(parsed.into()),
        _ => None,
    }
}
/// Only explicit user permalinks are mentions; room/event/web links remain links.
fn user_mention(link: &str) -> Option<matrix_sdk::ruma::OwnedUserId> {
    use matrix_sdk::ruma::{MatrixToUri, MatrixUri, matrix_uri::MatrixId};
    let id = if link.starts_with("matrix:") {
        MatrixUri::parse(link).ok()?.id().clone()
    } else {
        MatrixToUri::parse(link).ok()?.id().clone()
    };
    match id {
        MatrixId::User(id) => Some(id),
        _ => None,
    }
}
#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    current: Block,
    nodes: usize,
    bytes: usize,
    quotes: usize,
    lists: Vec<Option<u64>>,
    spoiler: usize,
    pre: usize,
    anchors: usize,
    formulas: bool,
    formula_count: usize,
    formula_pixels: usize,
}
impl Builder {
    fn flush(&mut self) {
        if !self.current.runs.is_empty() {
            self.blocks.push(std::mem::take(&mut self.current));
        }
        self.current.quote_depth = self.quotes;
        self.current.list_depth = self.lists.len();
    }
    fn push(&mut self, content: Inline, style: Style) -> Option<()> {
        self.bytes += content.text().len();
        if self.bytes > MAX_BYTES {
            return None;
        }
        if !content.text().is_empty() {
            self.current.runs.push(Run { content, style });
        }
        Some(())
    }
    fn text(&mut self, text: &str, style: Style) -> Option<()> {
        let text = if self.pre > 0 {
            text.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            let mut normalized = String::new();
            let mut space = self
                .current
                .runs
                .last()
                .is_none_or(|r| r.content.text().ends_with([' ', '\n']));
            for c in text.chars() {
                if matches!(c, ' ' | '\n' | '\r' | '\t' | '\u{000c}') {
                    if !space {
                        normalized.push(' ');
                    }
                    space = true;
                } else {
                    normalized.push(c);
                    space = false;
                }
            }
            normalized
        };
        if self.anchors > 0 || style.link.is_some() || style.code {
            self.push(Inline::Text(text), style)
        } else {
            for run in linked_runs(&text, &style) {
                self.push(run.content, run.style)?;
            }
            Some(())
        }
    }
    fn visit(&mut self, node: &NodeRef, mut style: Style, depth: usize) -> Option<()> {
        self.nodes += 1;
        if self.nodes > MAX_NODES || depth > MAX_DEPTH {
            return None;
        }
        if let Some(text) = node.as_text() {
            return self.text(&text.borrow(), style);
        }
        let Some(element) = node.as_element() else {
            return Some(());
        };
        let tag = element.name.local.as_ref();
        let attr = |name: &str| {
            element
                .attrs
                .borrow()
                .iter()
                .find(|a| a.name.local.as_ref() == name)
                .map(|a| a.value.to_string())
        };
        if matches!(tag, "script" | "style" | "iframe" | "object" | "mx-reply") {
            return Some(());
        }
        let block = matches!(
            tag,
            "p" | "div"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "blockquote"
                | "ul"
                | "ol"
                | "li"
                | "pre"
                | "table"
                | "tr"
                | "details"
                | "summary"
                | "hr"
        );
        if block {
            self.flush();
        }
        match tag {
            "b" | "strong" => style.bold = true,
            "i" | "em" => style.italic = true,
            "u" => style.underline = true,
            "s" | "del" | "strike" => style.strike = true,
            "code" => {
                style.code = true;
                if self.pre > 0 {
                    self.current.kind = BlockKind::Code {
                        language: attr("class").and_then(|c| {
                            c.split_whitespace()
                                .find_map(|s| s.strip_prefix("language-").map(str::to_owned))
                        }),
                    };
                }
            }
            "a" => {
                self.anchors += 1;
                style.link = attr("href").and_then(|v| safe_link(&v));
                style.mention = style.link.as_deref().and_then(user_mention);
            }
            "br" => return self.push(Inline::Text("\n".into()), style),
            "blockquote" => {
                self.quotes += 1;
                self.current.quote_depth = self.quotes;
            }
            "ul" | "ol" => {
                self.lists.push(if tag == "ol" {
                    Some(attr("start").and_then(|v| v.parse().ok()).unwrap_or(1))
                } else {
                    None
                });
                self.current.list_depth = self.lists.len();
            }
            "li" => {
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let marker = format!("{n}. ");
                        *n = n.saturating_add(1);
                        marker
                    }
                    _ => "• ".into(),
                };
                self.push(Inline::Text(marker), style.clone())?;
            }
            "pre" => {
                self.pre += 1;
                self.current.kind = BlockKind::Code { language: None };
                style.code = true;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.current.kind = BlockKind::Heading(tag[1..].parse().ok()?);
                style.bold = true;
            }
            "img" => {
                return self.push(
                    Inline::Image {
                        source: attr("src")
                            .map(matrix_sdk::ruma::OwnedMxcUri::from)
                            .filter(|uri| uri.is_valid())
                            .map(ImageSource::Matrix),
                        alt: attr("alt")
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "[Image]".into()),
                        width: if attr("data-mx-emoticon").is_some() {
                            None
                        } else {
                            attr("width").and_then(|s| s.parse().ok())
                        },
                        height: if attr("data-mx-emoticon").is_some() {
                            Some(24)
                        } else {
                            attr("height").and_then(|s| s.parse().ok())
                        },
                        descent: 0,
                    },
                    style,
                );
            }
            "hr" => {
                self.push(Inline::Text("────────".into()), style.clone())?;
            }
            "td" | "th" if !self.current.runs.is_empty() => {
                self.push(Inline::Text(" | ".into()), style.clone())?;
            }
            _ => {}
        }
        if attr("data-mx-spoiler").is_some() {
            self.spoiler += 1;
            style.spoilers.push(self.spoiler);
        }
        let math = matches!(tag, "span" | "div")
            .then(|| attr("data-mx-maths"))
            .flatten()
            .filter(|s| !s.trim().is_empty() && self.pre == 0 && !style.code);
        if let Some(latex) = &math {
            let display = tag == "div";
            let image = if self.formulas && self.formula_count < crate::formula::MAX_FORMULAS {
                self.formula_count += 1;
                crate::formula::render(latex, display, &mut self.formula_pixels)
            } else {
                None
            };
            if let Some(image) = image {
                if display {
                    self.current.kind = BlockKind::Math;
                }
                self.push(
                    Inline::Image {
                        width: Some(image.width),
                        height: Some(image.height),
                        descent: image.descent,
                        source: Some(ImageSource::Formula(image)),
                        alt: latex.clone(),
                    },
                    style,
                )?;
                if display {
                    self.flush();
                }
                return Some(());
            }
        }
        let before = self.bytes;
        for child in node.children() {
            self.visit(&child, style.clone(), depth + 1)?;
        }
        // A sender may omit the fallback. Never lose the formula entirely.
        if let Some(latex) = math
            && self.bytes == before
        {
            self.text(&latex, style)?;
        }
        if block {
            self.flush();
        }
        match tag {
            "blockquote" => self.quotes -= 1,
            "ul" | "ol" => {
                self.lists.pop();
            }
            "pre" => self.pre -= 1,
            "a" => self.anchors -= 1,
            _ => {}
        }
        self.current.quote_depth = self.quotes;
        self.current.list_depth = self.lists.len();
        Some(())
    }
}
/// Byte ranges for a block within the canonical message text.
pub fn block_ranges(blocks: &[Block]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = 0;
    blocks.iter().map(move |block| {
        let end = start
            + block
                .runs
                .iter()
                .map(|r| r.content.text().len())
                .sum::<usize>();
        let range = start..end;
        start = end + 2;
        range
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(doc: &Document) -> Vec<&str> {
        doc.blocks
            .iter()
            .flat_map(|b| &b.runs)
            .filter_map(|r| r.style.link.as_deref())
            .collect()
    }
    #[test]
    fn bare_links_preserve_literal_text_and_trim_surrounding_punctuation() {
        let source = "  café 👩🏽‍💻 **literal** <b>HTML</b>\n\n(https://en.wikipedia.org/wiki/Matrix_(protocol)).\nhttps://example.org/search?q=café&lang=en, www.matrix.org!\n  trailing  ";
        let doc = Document::plain_links(source).unwrap();
        assert_eq!(doc.text(&BTreeSet::new()), source);
        assert_eq!(
            links(&doc),
            vec![
                "https://en.wikipedia.org/wiki/Matrix_(protocol)",
                "https://example.org/search?q=caf%C3%A9&lang=en",
                "https://www.matrix.org/",
            ]
        );
        assert!(
            Document::plain_links("example.org file:///tmp/a javascript:alert(1) user@example.org")
                .is_none()
        );
        assert!(
            Document::plain_links(&format!("https://example.org/{}", "a".repeat(MAX_BYTES)))
                .is_none()
        );
    }
    #[test]
    fn explicit_user_mentions_preserve_labels_styles_and_spoiler_privacy() {
        use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
        let message = MessageType::Text(TextMessageEventContent::html(
            "fallback",
            "<p>Hello <a href='https://matrix.to/#/%40maya%3Aexample.org'><b>Maya 👋</b></a> and <a href='matrix:u/jules:example.org'>Jules</a>. <span data-mx-spoiler><a href='https://matrix.to/#/@secret:example.org'>Secret</a></span></p>",
        ));
        let doc = Document::from_message(&message, None).unwrap();
        let mentions: Vec<_> = doc
            .blocks
            .iter()
            .flat_map(|b| &b.runs)
            .filter(|r| r.style.mention.is_some())
            .collect();
        assert_eq!(mentions.len(), 3);
        assert_eq!(
            mentions[0].style.mention.as_deref().unwrap().as_str(),
            "@maya:example.org"
        );
        assert_eq!(mentions[0].content.text(), "Maya 👋");
        assert!(mentions[0].style.bold);
        assert_eq!(
            mentions[1].style.mention.as_deref().unwrap().as_str(),
            "@jules:example.org"
        );
        assert!(!doc.text(&BTreeSet::new()).contains("Secret"));
        assert!(
            doc.visible_blocks(&BTreeSet::new())
                .iter()
                .flat_map(|b| &b.runs)
                .all(|r| r
                    .style
                    .mention
                    .as_deref()
                    .is_none_or(|id| id.as_str() != "@secret:example.org"))
        );
        assert!(doc.text(&BTreeSet::from([1])).contains("Secret"));
        assert!(Arc::ptr_eq(
            &doc,
            &Document::from_message(&message, Some(&doc)).unwrap()
        ));
    }

    #[test]
    fn mention_appearance_requires_an_explicit_valid_user_anchor() {
        for link in [
            "https://matrix.to.evil.org/#/@maya:example.org",
            "https://example.org/#/@maya:example.org",
            "https://matrix.to/#/!room:example.org",
            "https://matrix.to/#/%23room:example.org",
            "https://matrix.to/#/!room:example.org/$event",
            "https://matrix.to/#/not-a-user",
            "matrix:r/room:example.org",
            "javascript:alert(1)",
        ] {
            let doc = Document::parse(&format!("<a href='{link}'>@Maya</a>")).unwrap();
            assert!(
                doc.blocks
                    .iter()
                    .flat_map(|b| &b.runs)
                    .all(|r| r.style.mention.is_none())
            );
        }
        let doc =
            Document::plain_links("@maya:example.org @room https://matrix.to/#/@maya:example.org")
                .unwrap();
        assert!(doc.blocks[0].runs.iter().all(|r| r.style.mention.is_none()));
    }

    #[test]
    fn bare_links_in_html_keep_styles_anchors_code_and_spoilers() {
        let doc = Document::parse("<p><b>https://example.org/</b> <a href='https://destination.org/'>https://label.org/</a> <code>https://code.org/</code></p><pre>https://block.org/</pre><span data-mx-spoiler>https://hidden.org/</span>").unwrap();
        assert_eq!(
            links(&doc),
            vec![
                "https://example.org/",
                "https://destination.org/",
                "https://hidden.org/"
            ]
        );
        assert!(doc.blocks[0].runs[0].style.bold);
        assert!(!doc.text(&BTreeSet::new()).contains("hidden.org"));
        assert_eq!(
            links(&Document {
                blocks: doc.visible_blocks(&BTreeSet::new()),
                ..doc.clone()
            }),
            vec!["https://example.org/", "https://destination.org/"]
        );
    }
    #[test]
    fn plain_link_documents_reuse_and_invalidate_without_html_source_collisions() {
        use matrix_sdk::ruma::events::room::message::{
            EmoteMessageEventContent, NoticeMessageEventContent, TextMessageEventContent,
        };
        for message in [
            MessageType::Text(TextMessageEventContent::plain("https://example.org/")),
            MessageType::Notice(NoticeMessageEventContent::plain("https://example.org/")),
            MessageType::Emote(EmoteMessageEventContent::plain("https://example.org/")),
        ] {
            let first = Document::from_message(&message, None).unwrap();
            let again = Document::from_message(&message, Some(&first)).unwrap();
            assert!(Arc::ptr_eq(&first, &again));
            let html = MessageType::Text(TextMessageEventContent::html(
                "fallback",
                "https://example.org/",
            ));
            let formatted = Document::from_message(&html, Some(&first)).unwrap();
            assert!(!Arc::ptr_eq(&first, &formatted));
            let edited = MessageType::Text(TextMessageEventContent::plain("https://edited.org/"));
            assert_eq!(
                links(&Document::from_message(&edited, Some(&first)).unwrap()),
                vec!["https://edited.org/"]
            );
            let removed = MessageType::Text(TextMessageEventContent::plain("removed"));
            assert!(Document::from_message(&removed, Some(&first)).is_none());
        }
    }
    #[test]
    fn matrix_math_uses_attribute_source_display_blocks_and_document_lifetime() {
        use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
        let html = r#"<p>Before <span data-mx-maths="\frac{a}{b}"><code>fallback</code></span> after.</p><div data-mx-maths="\sum_{n=1}^{10}n"><code>sum</code></div>End"#;
        let message = MessageType::Text(TextMessageEventContent::html("fallback", html));
        let doc = Document::from_message(&message, None).unwrap();
        let again = Document::from_message(&message, Some(&doc)).unwrap();
        assert!(Arc::ptr_eq(&doc, &again));
        assert_eq!(
            doc.text(&BTreeSet::new()),
            "Before \\frac{a}{b} after.\n\n\\sum_{n=1}^{10}n\n\nEnd"
        );
        assert!(matches!(doc.blocks[1].kind, BlockKind::Math));
        assert!(matches!(
            doc.blocks[0].runs[1].content,
            Inline::Image {
                source: Some(ImageSource::Formula(_)),
                ..
            }
        ));
        assert!(
            doc.image_keys().is_empty(),
            "math never requests Matrix media"
        );
    }

    #[test]
    fn math_fallbacks_spoilers_and_formula_limits_preserve_readable_text() {
        let doc = Document::parse_with_formulas(r#"<p><span data-mx-maths="\unsupported">readable <b>fallback</b></span> <span data-mx-maths="\unsupported"></span></p><span data-mx-spoiler data-mx-maths="x^2"><code>x squared</code></span>"#).unwrap();
        let text = doc.text(&BTreeSet::new());
        assert!(text.contains("readable fallback \\unsupported"));
        assert!(!text.contains("x^2"));
        assert!(doc.text(&BTreeSet::from([1])).contains("x^2"));
        let html =
            r#"<span data-mx-maths="x">fallback</span>"#.repeat(crate::formula::MAX_FORMULAS + 1);
        let doc = Document::parse_with_formulas(&html).unwrap();
        let rendered = doc
            .blocks
            .iter()
            .flat_map(|b| &b.runs)
            .filter(|r| {
                matches!(
                    r.content,
                    Inline::Image {
                        source: Some(ImageSource::Formula(_)),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(rendered, crate::formula::MAX_FORMULAS);
        assert!(doc.text(&BTreeSet::new()).ends_with("fallback"));
    }
    #[test]
    fn plain_unknown_formats_and_formatting_only_edits_have_distinct_paths() {
        let message = |html: Option<&str>| -> MessageType {
            let mut value = serde_json::json!({ "msgtype": "m.text", "body": "same fallback" });
            if let Some(html) = html {
                value["format"] = serde_json::json!("org.matrix.custom.html");
                value["formatted_body"] = serde_json::json!(html);
            }
            serde_json::from_value(value).unwrap()
        };
        assert!(Document::from_message(&message(None), None).is_none());
        let first = Document::from_message(&message(Some("<b>same</b>")), None).unwrap();
        let cached = Document::from_message(&message(Some("<b>same</b>")), Some(&first)).unwrap();
        assert!(Arc::ptr_eq(&first, &cached));
        let edited = Document::from_message(&message(Some("<i>same</i>")), Some(&cached)).unwrap();
        assert!(!Arc::ptr_eq(&first, &edited));
        assert_eq!(first.text(&BTreeSet::new()), edited.text(&BTreeSet::new()));
        assert!(edited.blocks[0].runs[0].style.italic);
        let unknown = serde_json::from_value(serde_json::json!({ "msgtype":"m.text", "body":"fallback", "format":"com.example.unknown", "formatted_body":"<b>text</b>" })).unwrap();
        assert!(Document::from_message(&unknown, Some(&first)).is_none());
    }
    #[test]
    fn parses_structure_combined_styles_entities_and_whitespace() {
        let doc = Document::parse("<h2>Title</h2><p>Hello <b>bold <i>&amp; italic</i></b><br>next</p><ol start='3'><li>three</li><li>four</li></ol><blockquote>quote</blockquote>").unwrap();
        assert_eq!(
            doc.text(&BTreeSet::new()),
            "Title\n\nHello bold & italic\nnext\n\n3. three\n\n4. four\n\nquote"
        );
        assert!(
            doc.blocks[1]
                .runs
                .iter()
                .any(|r| r.style.bold && r.style.italic)
        );
        assert_eq!(doc.blocks.last().unwrap().quote_depth, 1);
    }
    #[test]
    fn inline_image_sources_and_dimensions_are_untrusted_metadata() {
        for source in [
            "https://example.org/image",
            "file:///tmp/image",
            "data:image/png,abc",
            "mxc://",
            "local:1",
        ] {
            let doc = Document::parse(&format!("<img src='{source}' alt='safe alternative' width='4294967295' height='0' data-baseline='999'>")).unwrap();
            assert!(doc.image_keys().is_empty());
            assert!(matches!(
                &doc.blocks[0].runs[0].content,
                Inline::Image {
                    source: None,
                    descent: 0,
                    ..
                }
            ));
        }
    }

    #[test]
    fn custom_emoticons_override_sender_dimensions_and_keep_accessible_text() {
        let doc = Document::parse("<p>Hi <img data-mx-emoticon='' src='mxc://example.org/cat' alt='A waving cat' width='800' height='900'></p>").unwrap();
        assert_eq!(doc.text(&Default::default()), "Hi A waving cat");
        assert!(matches!(
            &doc.blocks[0].runs[1].content,
            Inline::Image {
                width: None,
                height: Some(24),
                ..
            }
        ));
    }
    #[test]
    fn code_and_images_retain_extension_metadata() {
        let doc = Document::parse("<pre><code class='language-rust'>let a = 1;\n  a</code></pre><p><img src='mxc://example/a' alt='diagram' width='64' height='32'></p>").unwrap();
        assert_eq!(
            doc.blocks[0].kind,
            BlockKind::Code {
                language: Some("rust".into())
            }
        );
        assert_eq!(doc.blocks[0].runs[0].content.text(), "let a = 1;\n  a");
        assert!(matches!(
            &doc.blocks[1].runs[0].content,
            Inline::Image {
                width: Some(64),
                height: Some(32),
                ..
            }
        ));
    }
    #[test]
    fn spoilers_are_hidden_from_canonical_copy_text_until_revealed() {
        let doc = Document::parse("before <span data-mx-spoiler>secret <b>ending</b></span> after")
            .unwrap();
        assert!(!doc.text(&BTreeSet::new()).contains("secret"));
        assert_eq!(doc.text(&BTreeSet::from([1])), "before secret ending after");
    }
    #[test]
    fn unsafe_content_links_and_excessive_input_are_inert() {
        let doc = Document::parse(
            "<script>secret</script><p><a href='javascript:alert(1)'>label</a> &lt;b&gt;</p>",
        )
        .unwrap();
        assert_eq!(doc.text(&BTreeSet::new()), "label <b>");
        assert!(doc.blocks[0].runs[0].style.link.is_none());
        assert!(safe_link("file:///etc/passwd").is_none());
        assert!(safe_link("https://example.org").is_some());
        let anchor =
            Document::parse("<a href='javascript:alert(1)'>https://example.org/</a>").unwrap();
        assert!(links(&anchor).is_empty(), "an inert anchor must stay inert");
        assert!(Document::parse(&"a".repeat(MAX_BYTES + 1)).is_none());
        assert!(Document::parse(&format!("{}x{}", "<b>".repeat(40), "</b>".repeat(40))).is_none());
    }
}
