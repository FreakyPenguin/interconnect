//! Markdown authoring and Matrix payloads share one bounded conversion path.
use crate::message_document::{Document, safe_link};
use matrix_sdk::ruma::events::room::message::{RoomMessageEventContent, TextMessageEventContent};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
use std::{
    io::{self, Write},
    sync::Arc,
};

const MAX_BYTES: usize = 64 * 1024;
static CONVERSIONS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
const TOO_COMPLEX: &str =
    "Message is too large or complex to format. Shorten it or choose Plain text.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
        }
    }
    pub fn text_content(&self) -> TextMessageEventContent {
        match &self.html {
            Some(html) => TextMessageEventContent::html(self.source.clone(), html.clone()),
            None => TextMessageEventContent::plain(self.source.clone()),
        }
    }
    pub fn message_content(&self) -> RoomMessageEventContent {
        RoomMessageEventContent::new(matrix_sdk::ruma::events::room::message::MessageType::Text(
            self.text_content(),
        ))
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
    if source.trim().is_empty() {
        return Err("Message is empty".into());
    }
    if !needs_conversion(&source, format) {
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
        convert(source)
    })
    .await
    .map_err(|_| "Could not format message".to_owned())?
}
fn convert(source: String) -> Result<Prepared, String> {
    if source.len() > MAX_BYTES {
        return Err(TOO_COMPLEX.into());
    }
    let mut events = Vec::new();
    let mut depth = 0usize;
    let mut links = Vec::new();
    let mut has_display_math = false;
    for (count, event) in Parser::new_ext(
        &source,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_MATH,
    )
    .enumerate()
    {
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
            Event::InlineMath(latex) => Event::InlineHtml(math_html(&latex, false).into()),
            Event::DisplayMath(latex) => {
                has_display_math = true;
                Event::Html(math_html(&latex, true).into())
            }
            // Chat newlines are intentional. Raw HTML is literal source text.
            Event::SoftBreak => Event::HardBreak,
            Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
            Event::Start(Tag::HtmlBlock) => Event::Start(Tag::Paragraph),
            Event::End(TagEnd::HtmlBlock) => Event::End(TagEnd::Paragraph),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let link = safe_link(&dest_url);
                links.push(link.is_some());
                let Some(link) = link else {
                    continue;
                };
                Event::Start(Tag::Link {
                    link_type,
                    dest_url: link.into(),
                    title,
                    id,
                })
            }
            Event::End(TagEnd::Link) if !links.pop().unwrap_or(false) => continue,
            // Inline-image authoring is deferred. Retain alternative text without
            // emitting a remote resource request for other Matrix clients.
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => continue,
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
    })
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
