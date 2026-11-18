//! Offline code colors prepared with documents, never during UI layout/drawing.
use crate::message_document::{Block, BlockKind, Inline};
use std::{ops::Range, sync::LazyLock};
use syntect::{
    easy::HighlightLines, highlighting::Theme, parsing::SyntaxSet, util::LinesWithEndings,
};
use two_face::theme::EmbeddedThemeName;

// Bound work and cached output independently of the HTML parser's limits.
const MAX_CODE_BYTES: usize = 16 * 1024;
const MAX_LINE_BYTES: usize = 2048;
const MAX_SPANS: usize = 8192;
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
static THEMES: LazyLock<(Theme, Theme)> = LazyLock::new(|| {
    let themes = two_face::theme::extra();
    (
        themes.get(EmbeddedThemeName::InspiredGithub).clone(),
        themes.get(EmbeddedThemeName::Nord).clone(),
    )
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub rgb: [u8; 3],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub light: Vec<Span>,
    pub dark: Vec<Span>,
}

/// Call only on a blocking worker. Unknown/unlabelled or oversized code stays plain.
pub(crate) fn highlight(block: &Block) -> Option<Highlight> {
    let BlockKind::Code {
        language: Some(language),
    } = &block.kind
    else {
        return None;
    };
    if block
        .runs
        .iter()
        .any(|run| !matches!(run.content, Inline::Text(_)))
    {
        return None;
    }
    let length: usize = block.runs.iter().map(|run| run.content.text().len()).sum();
    if length == 0 || length > MAX_CODE_BYTES {
        return None;
    }
    let source: String = block.runs.iter().map(|run| run.content.text()).collect();
    if LinesWithEndings::from(&source).any(|line| line.len() > MAX_LINE_BYTES) {
        return None;
    }
    let token = language.trim().to_ascii_lowercase();
    let syntax = SYNTAXES.find_syntax_by_token(&token)?;
    let colors = |theme| -> Option<Vec<Span>> {
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut spans: Vec<Span> = Vec::new();
        let mut offset = 0;
        for line in LinesWithEndings::from(&source) {
            for (style, text) in highlighter.highlight_line(line, &SYNTAXES).ok()? {
                let end = offset + text.len();
                let color = style.foreground;
                let rgb = [color.r, color.g, color.b];
                if let Some(last) = spans.last_mut().filter(|last| last.rgb == rgb) {
                    last.range.end = end;
                } else {
                    if spans.len() >= MAX_SPANS {
                        return None;
                    }
                    spans.push(Span {
                        range: offset..end,
                        rgb,
                    });
                }
                offset = end;
            }
        }
        Some(spans)
    };
    Some(Highlight {
        light: colors(&THEMES.0)?,
        dark: colors(&THEMES.1)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_document::Document;

    #[test]
    fn received_messages_reuse_cached_colors_and_reject_them_after_edits() {
        let message = |language: &str| {
            serde_json::from_value(serde_json::json!({
                "msgtype": "m.text", "body": "const answer = 42;",
                "format": "org.matrix.custom.html",
                "formatted_body": format!("<pre><code class='language-{language}'>const answer = 42;</code></pre>")
            })).unwrap()
        };
        let first = Document::from_message(&message("typescript"), None).unwrap();
        assert!(first.blocks[0].syntax.is_some());
        let cached = Document::from_message(&message("typescript"), Some(&first)).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &cached));
        let edited = Document::from_message(&message("unknown"), Some(&cached)).unwrap();
        assert!(!std::sync::Arc::ptr_eq(&cached, &edited));
        assert!(edited.blocks[0].syntax.is_none());
    }

    #[test]
    fn bundled_languages_preserve_bytes_and_multiline_state_in_both_themes() {
        for (language, source) in [
            ("rust", "/* start\r\n comment */\r\nlet café = \"👩‍💻\";\n"),
            ("typescript", "const answer: number = 42;\n"),
            ("toml", "[package]\nname = \"example\"\n"),
            ("dockerfile", "FROM debian:stable\nRUN true\n"),
        ] {
            let doc = Document::parse_with_formulas(&format!(
                "<pre><code class='language-{language}'>{source}</code></pre>"
            ))
            .unwrap();
            let block = &doc.blocks[0];
            let text: String = block.runs.iter().map(|r| r.content.text()).collect();
            let highlight = block.syntax.as_ref().expect(language);
            for spans in [&highlight.light, &highlight.dark] {
                let mut offset = 0;
                for span in spans {
                    assert_eq!(span.range.start, offset);
                    assert!(text.is_char_boundary(span.range.end));
                    offset = span.range.end;
                }
                assert_eq!(offset, text.len());
                assert!(spans.len() > 1, "{language}");
            }
            assert_ne!(highlight.light, highlight.dark);
            if language == "rust" {
                let spans = &highlight.light;
                assert!(spans[0].range.end >= text.find("*/").unwrap() + 2);
            }
        }
    }

    #[test]
    fn fallback_bounds_and_spoilers() {
        for html in [
            "<pre><code>let x = 1;</code></pre>".to_owned(),
            "<pre><code class='language-not-a-language'>let x = 1;</code></pre>".to_owned(),
            format!(
                "<pre><code class='language-rust'>{}</code></pre>",
                "x".repeat(MAX_LINE_BYTES + 1)
            ),
            format!(
                "<pre><code class='language-rust'>{}</code></pre>",
                "x\n".repeat(MAX_CODE_BYTES / 2 + 1)
            ),
        ] {
            let doc = Document::parse_with_formulas(&html).unwrap();
            assert!(doc.blocks[0].syntax.is_none());
        }
        let doc = Document::parse_with_formulas("<pre><code class='language-rust'><span data-mx-spoiler>let secret = 1;</span> let visible = 2;</code></pre>").unwrap();
        assert!(doc.blocks[0].syntax.is_some());
        assert!(doc.visible_blocks(&Default::default())[0].syntax.is_none());
        let revealed = doc.blocks[0]
            .runs
            .iter()
            .flat_map(|run| run.style.spoilers.iter().copied())
            .collect();
        assert!(doc.visible_blocks(&revealed)[0].syntax.is_some());
    }
}
