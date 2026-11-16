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
fetches links or inline images.

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

## Extension points

Inline images are embedded objects carrying their MXC source, alternative text,
and optional dimensions. They currently render only alternative text and do not
request media. A future renderer can resolve these objects through the existing
bounded media pipeline, reserve dimensions, and preserve the timeline anchor
when their layout changes. Object offsets and selection/copy policy will need
explicit handling when pixels replace alternative text; the parser and document
structure do not need replacement.

Code blocks preserve literal source whitespace and an optional language hint.
Rendering already splits semantic runs into shaping spans (including emoji
fonts). Syntax highlighting can add cached presentation ranges over the source,
split spans at token boundaries, and supply styles while keeping canonical text
and selection offsets unchanged. No highlighting engine is coupled to the model.

Tables, inline-image loading, syntax highlighting, visual rich-text editing, and
formatted media captions remain follow-up work. Room-list, pin and notification excerpts
continue to use the plain SDK fallback.

## Markdown composition

Room and thread drafts default to Markdown. The composer mode button switches
that draft to literal **Plain text** without rewriting its source or selection.
The choice is kept in memory per conversation, like the draft itself. Attachment
captions remain plain text.

Composition uses CommonMark with strikethrough enabled: emphasis, strong text,
headings, quotes, lists, inline/fenced code and links. Soft line breaks become
explicit breaks for chat. Raw HTML is escaped as literal text; Markdown images
produce their alternative text. Tables, task-list extensions, spoiler authoring,
inline-image loading and code syntax highlighting remain follow-up work.

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
