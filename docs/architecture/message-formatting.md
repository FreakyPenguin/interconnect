# Message formatting

## Received messages

SDK-sanitized `m.text`, `m.notice`, and `m.emote` HTML projects to native
`message_document::Document` on blocking workers. Paragraphs, headings, lists,
quotes, code, links, spoilers, and inline styles are supported; unsupported
structures degrade to text. CSS/scripts are inert; plain messages stay literal.
Projection is bounded to 64 KiB HTML/output, 4096 nodes, and 32 levels; oversized
content falls back to plain text. Reuse unchanged documents, replace on edit, and
check cancellation/generation before publishing.

Measurement, drawing, hit-testing, selection, and decoration share Iced/Cosmic
Text paragraphs. Copy uses canonical displayed text, image alternatives, and
LaTeX source; hidden spoilers copy placeholders. Reveal is widget-local and
resets selection/layout. Excerpts use the SDK plain fallback.

Bare HTTP/HTTPS/`www.` links exclude code, existing anchors, hidden spoilers, and
object alternatives. Activation validates destinations; ordinary links use the
async system opener and Matrix links route internally. Parsing fetches no previews.

Code uses syntect with two-face's bundled syntax/themes (Oniguruma backend).
Light/dark colors are document-owned and prepared on workers with formulas;
`Document::parse` stays structural, while `parse_with_formulas` prepares assets.
Resize/theme changes reuse colors. Missing languages, blocks over 16 KiB, or lines
over 2048 bytes fall back to monospace. Assets are embedded; no global content cache.

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

Drafts default to Markdown; Plain text preserves source. CommonMark plus
strikethrough supports headings/lists/quotes/code/links; soft breaks become chat
breaks, raw HTML escapes, and image syntax becomes alternative text. `$…$` and
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
