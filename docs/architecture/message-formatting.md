# Message formatting

## Received messages

SDK-sanitized `m.text`, `m.notice`, and `m.emote` HTML projects to native
`message_document::Document` on blocking workers. Paragraphs, headings, lists,
quotes, code, tables, links, spoilers, and inline styles are supported; unsupported
structures degrade to text. CSS/scripts are inert; plain messages stay literal.
Projection is bounded to 64 KiB HTML/output, 4096 nodes, and 32 levels; oversized
content falls back to plain text. Reuse unchanged documents, replace on edit, and
check cancellation/generation before publishing.

Measurement, drawing, hit-testing, selection, and decoration share Iced/Cosmic
Text paragraphs. Copy uses canonical displayed text, image alternatives, and
LaTeX source; hidden spoilers copy placeholders. Revealing changes canonical
text and layout, so affected selections are revalidated. Spoiler reveals, code
expansion, and horizontal offsets survive widget eviction and recent conversation
navigation in the bounded [session cache](timeline.md); edits/account changes
invalidate stale state. Excerpts use the SDK plain fallback.

Bare HTTP/HTTPS/`www.` links exclude code, existing anchors, hidden spoilers, and
object alternatives. Activation validates destinations; ordinary links use the
async system opener and Matrix links route internally. Parsing fetches no previews.

Code uses syntect with two-face's bundled syntax/themes (Oniguruma backend).
Light/dark colors are document-owned and prepared on workers with formulas;
`Document::parse` stays structural, while `parse_with_formulas` prepares assets.
Resize/theme changes reuse colors. Missing languages, blocks over 16 KiB, or lines
over 2048 bytes fall back to uncolored monospace. These are highlighting limits,
independent of the preview limits below. Assets are embedded; no global content
cache.

## Bounded code previews

`formatted::State` shapes a UTF-8-safe prefix of at most 12 lines / 2048 bytes for
collapsed code. The document and canonical selection/copy text retain the entire
block. Expand explicitly opts into full layout; Collapse drops expanded
paragraphs and returns to the prefix budget. These controls do not apply to a
plain-message fallback when HTML exceeds the document parser's limits.

Cached outline icons form an upper-right corner overlay: Copy, Save, and, for
truncated blocks, Expand/Collapse. Hover tooltips name each action. The message
hover toolbar leaves that corner clear and wraps in narrow panes without changing
text width. There is no text-control row or horizontal-arrow control.
A translucent bottom fade with **Expand code** also activates expansion.

Visible corner controls participate in Iced focus operations in visual order.
Tab/Shift+Tab traverses them; an accent outline and tooltip identify focus.
Enter/Space shares the pointer action path, ignoring key repeats; Escape releases
focus. A live-widget block/action key retains focus through reflow, independently
of text selection and cached conversation semantics. Viewport filtering excludes
invisible controls, and document replacement drops keyboard focus. Keyboard
expansion uses the same message-body anchor as pointer expansion.

Code stays unwrapped. Overflow supplies a horizontal track/thumb; dragging,
track presses, and horizontal wheel/trackpad input share the stored horizontal
pixel offset. The overlay stays fixed while glyphs scroll, and hit-testing uses
the same translation. Release/focus loss ends a drag. No idle animation or polling
is introduced. Code controls preserve selection and anchor the message body
through expansion; see [timeline restoration](timeline.md).

Copy and Save use the complete block with spoiler placeholders, never control
labels. Save captures that text, opens an asynchronous file picker suggesting
`code.txt`, and writes asynchronously. Superseding saves, account transitions,
and shutdown abort the task; request/account checks reject stale completion UI.
Plain prose still lays out as a whole, as does explicitly expanded code.

## Tables

Incoming HTML tables and authored Markdown pipe tables become `TableCell` blocks
with table/row/column identities, header state, and alignment. Columns share equal
widths within each table; cells wrap and each row uses its tallest measured cell.
Header styling, borders, text drawing, and hit-testing share those cell bounds.
Cells retain inline styles and validated links. Canonical text separates cells
with tabs and rows with newlines, preserving displayed labels, empty cells, and
hidden-spoiler placeholders.

Existing byte/node/depth limits still apply. Tables allow at most 32 columns;
nested tables reject formatted parsing and use the event's plain fallback.
Row/column spans do not create merged cells. Alignment reads `align` or the
recognized `text-align` value; arbitrary CSS remains inert.

## Message text sizing

`appearance.message_text_size` selects a 12/14/16/18 logical-pixel body size,
independent of interface scaling. Plain editors update their font metrics;
formatted paragraphs invalidate layout while retaining canonical positions and
sparse interaction state. Headings scale relative to the 14 px default. Code,
tables, and message previews use the body preference; room headers, controls,
composer editing, and authored image/formula dimensions retain their own sizes.
Interface and desktop scaling still apply to the result.

## Received user mentions

Validated Matrix user anchors use an accent background; explicit room anchors use
teal. Known users reuse DMs; unknown users prefill chat creation, requiring explicit
submission. Self links do nothing. Known rooms open internally; unknown rooms
prefill Join. Event permalinks navigate to the event; joined-space links filter
without switching conversation. Labels preserve inline formatting and spoiler privacy.

Room-wide tokens are amber only with `m.mentions.room` metadata, excluding code,
links, and image alternatives. Cache identity includes that flag. No inference
from plain `@name`/`@room` or legacy body-only notification text.

## Inline images and local embedded assets

Incoming images accept validated MXC only; app producers supply decoded local or
formula assets. File access/decoding stays off the UI thread. Declared dimensions
reserve space through loading/failure; a single dimension uses intrinsic ratio,
missing dimensions may relayout, invalid/zero dimensions count as absent. Images
scale to line width with a 2048 px cap and align to the baseline.

The [Cosmic Text patch](../../vendor/cosmic-text/PATCH.md) shapes U+FFFC with explicit
width/ascent/descent before bidi/wrapping. Selection is atomic and copies alternatives,
not the marker. Spoiler images request media only after reveal; intrinsic-size
changes preserve scroll anchors.

At most 32 visible inline sources share four download/decode slots and a 96 MiB
attachment pixel cache. Limits: 2 MiB encoded, 4096 px source dimensions, 1024×1024
decode targets. Inline GIFs are static. Demand carries document/conversation
identity; hiding/closing releases it while bounded LRU pixels may remain.

## Mathematical messages

Matrix `span[data-mx-maths]` / `div[data-mx-maths]` carry delimiter-free LaTeX.
RaTeX with bundled KaTeX fonts renders on projection/composition workers. Incoming
plain text is never scanned for dollar signs; copy returns source. No TeX install,
network, persistent formula cache, or arbitrary LaTeX packages/documents.

Limits: 16 formulas/document, 2048 source bytes each, 2× logical resolution,
2048 px/dimension, 256K pixels/formula, 512K/document. Invalid/oversized formulas
retain fallback text (or LaTeX attribute). Sources/errors are not logged.

## Markdown composition

Drafts default to Markdown; Plain text preserves source. CommonMark with
strikethrough and pipe-table extensions supports headings, lists, quotes, code,
links, and tables. Soft breaks become chat breaks, raw HTML escapes, and image
syntax becomes alternative text. `$…$` and
`$$…$$` produce Matrix math HTML; code/Plain text stay literal. Backslash math
delimiters are not composer syntax. Attachment captions stay plain.

Preview snapshots explicitly and shares Send's payload builder; typing does not
parse. Edits invalidate preview; account/request/source checks reject stale work.
Conversion uses two workers with 64 KiB input/output, 2048 parser events, and 24
nesting levels. Errors retain drafts and allow switching to Plain text.

Source becomes `body`; HTML becomes `formatted_body` with `org.matrix.custom.html`.
The SDK owns queue admission, encryption, relations, local echoes, and retries.
See [limitations](../limitations.md) for unsupported authoring features.

## User mention composition

`@` completion loads joined members on demand through the account directory
handle: 30-second deadline, at most 4096 people, six visible matches ranked by
exact/prefix then case-insensitive matching. No directory-wide search, avatar
fetching, member subscription, or polling. Closing/navigation cancels the load;
account/conversation/generation/caret/source checks reject stale choices.
See [keyboard controls](../keyboard.md#composition) for triggers and acceptance.

User/room choices bind visible UTF-8 byte ranges to stable Matrix targets (up to
256 named selections/draft). Partial label edits retain identity; whole deletion/
replacement removes it. Native editor geometry paints decorations without changing
shaping, clipboard text, or caret positions. Cursor-only changes do not parse.
`#` searches joined-room names/canonical aliases/IDs without directory/member loads.
Bindings persist in [encrypted drafts](drafts.md); legacy `user_id` bindings remain compatible.

Preview/Send/edit preparation emits escaped anchors in either mode. Code/raw HTML/
existing links/image alternatives stay literal. Only emitted user anchors add
`m.mentions.user_ids`; room links never notify. Literal names/tokens infer nothing.
Explicit empty mention metadata suppresses legacy body-based notification matching.

Room-wide choices bind `room_wide: true` and emit `m.mentions.room`. Any token
alteration drops identity. Completion uses cached permission; the worker rechecks
`user_can_trigger_room_notification` before queue admission. SDK edits compute
notification differences to avoid re-notifying surviving broadcasts.

Edit recovery uses HTML positions, preserving duplicate labels/targets without
name guessing. Temporary edits have separate bindings; save/cancel restores the
unsent draft. Removing the last represented occurrence removes its metadata;
unrepresented legacy mentions/room flags remain. Oversized conversions fail
without dropping bindings. See the [mention spec](https://spec.matrix.org/v1.17/client-server-api/#user-and-room-mentions).

## Custom emoji

| Purpose | Preferred type | Legacy type |
| --- | --- | --- |
| Room pack | `m.room.image_pack` | `im.ponies.room_emotes` |
| Global references | `m.image_pack.rooms` | `im.ponies.emote_rooms` |
| Personal pack | — | `im.ponies.user_emotes` |

Stable packs win per pack; sticker-only packs are excluded. Discovery includes
current room, eight joined canonical parent spaces, and 64 globally referenced
rooms; projection caps at 64 packs/source and 1024 emoji. A session-owned
`emoji-packs` sliding-sync stream requests pack/parent state with zero history and
no encryption extensions. Errors retain caches and retry on later events.

Explicit choices bind `:wave:` tokens (collisions: `:wave/2:`) to MXC, up to 256
per draft, independent of later catalog changes. Manually typed tokens do not
resolve. Preview/Send emits image HTML in either mode, respecting literal contexts;
plain body retains tokens. Received emoji use inline images; SDK sanitization
strips the emoticon marker, so sender dimensions remain. MXC reactions are static.

Picker/completion/reaction/preview demand adds up to 80 unique images to the same
four slots/96 MiB cache; no bulk downloads. **Media is unencrypted even in encrypted
rooms.** No animation/standalone stickers. See [image packs](https://spec.matrix.org/v1.19/client-server-api/#image-packs)
and [MSC2545](https://github.com/matrix-org/matrix-spec-proposals/blob/main/proposals/2545-emotes.md).

### Pack management

Account-scoped serialized writes preserve event types and unknown JSON/sticker
fields; new room/global entries use stable types. Open/Reload reads up to 128 packs.
Reject invalid MXC, duplicate shortcodes, or content over 48 KiB. Recheck permissions
and conflicts; GET/PUT races remain possible without conditional Matrix writes.

Preparation runs on blocking workers; only Save uploads, without paths/filenames.
Failed publication can leave unused media; deletion removes references, not media
or existing bindings. Reads/file tasks cancel; admitted writes retain target and
participate in shutdown. Read/global deadlines are 30 seconds; saves 60 seconds.
Demo stays local. See [file/shortcode limits](../settings.md#emoji-packs).

## Room topics

The selected room's plain topic source is interpreted as Markdown using the
same bounded asynchronous composition workers and native document renderer.
Only the current account/room generation/source is cached; changes abort old
publication and stale completions are rejected. Space filtering reuses the
unchanged document. Formatting runs on content changes, never on redraw or
editor keystrokes. Excessive formatting falls back to selectable literal text.

The header retains inline Markdown styles in a bounded, grapheme-safe one-line
snippet. A caret expands/collapses the topic directly below it, with a bounded
scrollable body, selection, validated links, Copy Markdown, and permission-checked
editing. Expansion keeps the conversation and composer interactive; space filters
retain it, while conversation/account changes invalidate it. External links retain
expansion; Matrix links use ordinary internal routing. Raw HTML escapes and images
become alternatives; there is no topic media or link-preview demand. Editing uses
a separate multiline dialog and preserves the original source. The existing
asynchronous writer owns permissions, request identity, pending operations, and
errors. Rich snippets cache layout and truncate at grapheme boundaries while
retaining emphasis, code, link colors/underlines, and strikethrough.
