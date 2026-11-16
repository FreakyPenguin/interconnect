# Message formatting

Room messages and thread originals use the SDK's current message content,
including edits, sanitization, and reply-fallback handling. `m.text`, `m.notice`,
and `m.emote` messages with `org.matrix.custom.html` are converted to a native
`message_document::Document`. Other formats and messages beyond parsing limits
use the SDK's plain body. Plain messages are never interpreted as Markdown.

The document stores blocks and styled inline runs, independently of Iced.
Paragraphs, headings, quotes, ordered/unordered lists, code, links, spoilers,
and basic inline styles are supported. Unsupported structures degrade to text;
table cells retain separators. Author CSS, scripts, and arbitrary attributes
are not rendered. Link opening accepts HTTP, HTTPS, mailto and Matrix URLs,
validated again at activation. Hovering shows the destination. Parsing never
fetches links or images; visible inline images use the authenticated media pipeline.

HTML input is capped at 64 KiB, traversal at 4096 nodes and 32 levels, and output
at 64 KiB. Projection runs on a blocking worker, outside the UI thread. Unchanged
formatted content reuses the previous pane's document; formatting-only edits
invalidate it. There is no global document cache retaining departed messages.
Dropping the projection cancels further rows after the current bounded message.
Existing view cancellation and generation checks still govern publication.

The formatted renderer uses the same Iced/Cosmic Text paragraphs for measuring,
drawing, pointer hit-testing, decorations and selection geometry. A read-only
editor stores logical selection positions; its layout is not used for formatted
text. The timeline continues to coordinate selection across rows and preserve
visible message anchors across layout changes. Canonical copy text comes from
the displayed document, rather than the possibly different plain fallback.
Hidden spoilers use placeholder text in both selection and the clipboard.
Revealing changes canonical text and invalidates layout/old selection. Reveal
state is local to the widget and resets when the document or view changes.

## Inline images and local embedded assets

`Inline::Image` carries a validated `ImageSource::Matrix` MXC URI or an
application-only `ImageSource::Local` identity, alternative text, optional width
and height, and baseline descent. Incoming HTML can only produce Matrix sources;
HTTP, file, data and local URLs are inert. Local producers supply decoded RGBA
handles through `inline_image::Assets`; rendering never reads files or decodes
bytes on the UI thread. `ImageSource::Formula` holds an already decoded local
formula asset with dimensions/descent and uses the same layout and drawing path.

Dimensions are logical pixels. Both declared dimensions take precedence over
intrinsic pixels, so loading and failure do not change the reserved rectangle.
With one dimension, intrinsic aspect ratio supplies the other; without decoded
pixels a 24×24 ratio is used provisionally. Missing dimensions may relayout once
pixels arrive. Oversized objects scale proportionally to the available line
width, with a 2048-pixel cap per dimension. Zero/invalid dimensions are treated
as absent. Local baseline descent scales with the rectangle; HTML images align
their bottom to the text baseline.

Image paragraphs replace each image with U+FFFC for shaping, while preserving a
map to its complete alternative text for logical selection and the clipboard.
A narrow, opt-in [Cosmic Text extension](../../vendor/cosmic-text/PATCH.md) supplies
exact glyph width/ascent/descent before native bidi resolution and line wrapping.
The line box then contains all text and image ascents/descents. Text, pixels,
selection, links and hit-testing share the resulting layout. This avoids
positioning images over a separately measured text layout. Text-only paragraphs
retain the ordinary Iced paragraph path.

Placeholders retain their rectangles and show clipped alternative text. Loaded
pixels draw into the same rectangle; pointer selection treats the image as an atomic
object and copying uses alternative text, never the shaping marker. Revealed spoiler
images enter layout and media demand only after explicit reveal. Image state
and intrinsic-size changes use the existing message-anchor scroll restoration.

Widgets report changed visible image demand on ordinary redraw events without
scheduling timers. Reports include conversation generation and document identity;
edits, redactions, navigation and old reports cannot reload obsolete sources.
At most 32 unique visible inline sources join the existing attachment demand.
They share the four download/decode slots, SDK disk cache and 96 MiB attachment
pixel cache. Inline files have a 2 MiB encoded decode limit, 4096-pixel source
dimension limit and 1024×1024 target. GIFs show a static frame. The ordinary media
worker supplies cancellation, failure tracking and reconnect retries. Closing
or hiding an image releases its demand; this does not require evicting its pixels
from the bounded LRU cache.

## Further formatting work

Code blocks preserve literal source whitespace and an optional language hint.
Rendering already splits semantic runs into shaping spans (including emoji
fonts). Syntax highlighting can add cached presentation ranges over the source,
split spans at token boundaries, and supply styles while keeping canonical text
and selection offsets unchanged. No highlighting engine is coupled to the model.

Tables, syntax highlighting, visual rich-text editing, and
formatted media captions remain follow-up work. Room-list, pin and notification excerpts
continue to use the plain SDK fallback.

## Mathematical messages

Matrix's `span[data-mx-maths]` and `div[data-mx-maths]` carry LaTeX source without
delimiters. Inline formulas participate in native text wrapping/baseline layout;
display formulas occupy a centered block and use display-style operators.
`Document::parse_with_formulas` typesets them with RaTeX 0.1.16 and bundled KaTeX
fonts on the existing projection/composition blocking worker. Ordinary `parse`
only projects the sender's fallback, without invoking the renderer. Incoming
plain text is never scanned for dollar signs.

Successful formulas become document-owned decoded RGBA assets at twice their
logical resolution, with transparent backgrounds and the message ink color.
No media demand, external math service, TeX installation, timer or persistent
formula cache is involved. Unchanged HTML reuses the previous document/assets;
edits replace them under the existing cancellation and generation checks.
Selection treats formulas as atomic objects and copies their LaTeX source;
hidden spoilers retain the existing selection/copy protections.

At most 16 formulas are attempted per document, each with at most 2048 source
bytes. RaTeX bounds nesting/macro expansion and validates layout work before
building drawing items. Rasterization is capped at 2048 pixels per dimension,
256K pixels per formula and 512K pixels (2 MiB RGBA) per document. Oversized,
malformed or unsupported formulas retain the sender's HTML fallback. Missing
fallbacks show the attribute's LaTeX source. Sources/errors are never logged.
This supports RaTeX's math subset, not arbitrary LaTeX documents or packages.

## Markdown composition

Room and thread drafts default to Markdown. The composer mode button switches
that draft to literal **Plain text** without rewriting its source or selection.
The choice is kept in memory per conversation, like the draft itself. Attachment
captions remain plain text.

Composition uses CommonMark with strikethrough enabled: emphasis, strong text,
headings, quotes, lists, inline/fenced code and links. Soft line breaks become
explicit breaks for chat. Raw HTML is escaped as literal text; Markdown images
produce their alternative text. Tables, task-list extensions, spoiler authoring,
image authoring and code syntax highlighting remain follow-up work.

Markdown mode additionally accepts `$…$` for inline math and `$$…$$` for display
math through pulldown-cmark's math extension. These are serialized as Matrix
`data-mx-maths` markup with escaped attribute/text fallbacks, rather than
pulldown-cmark's presentation classes. Code spans/blocks and Plain text mode
remain literal. Element's alternative `\(…\)`/`\[…\]` composer delimiters are
not authoring syntax here; receiving their resulting Matrix HTML is supported.

The explicit **Preview** button snapshots the source and runs the same conversion
as Send. It uses the existing document renderer in a bounded panel above the
source editor. Editing hides/invalidates the preview; typing never starts a
parser or preview timer. Closing/replacing a preview aborts its task, and request,
account, format and source checks reject obsolete results. Navigating may retain
a completed preview with its original draft, without navigating on completion.

Conversion has a two-worker semaphore, 64 KiB input/output bounds, 2048 parser
events and 24 nesting levels. Ordinary text takes a conservative literal fast
path. Generated HTML is checked against the renderer's document limits, and
only allowlisted link destinations are emitted. Formatting errors retain the
source draft and permit switching to Plain text.

The original Markdown source is the Matrix `body` fallback; generated HTML is
`formatted_body` with `org.matrix.custom.html`. Preview and send share this
payload builder. The SDK owns queue admission, local echoes, thread relations,
encryption and retries. Drafts clear only on successful admission. Demo conversion
uses the same worker and renderer, entirely in memory with no account/network
access. Editing previously sent messages is outside this composition workflow.
