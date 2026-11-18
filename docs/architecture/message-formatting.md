# Message formatting

## Received messages

SDK-sanitized `m.text`, `m.notice`, and `m.emote` HTML becomes a native
`message_document::Document`, including edits and reply-fallback handling.
Paragraphs, headings, quotes, lists, code, links, spoilers, and inline styles are
supported. Unsupported structures degrade to text; author CSS/scripts are inert.
Plain messages remain literal, without Markdown interpretation.

Bare HTTP/HTTPS and `www.` links are detected in plain text/HTML text runs, excluding
code, existing anchors, hidden spoilers, and embedded-object alternatives.
Activation validates HTTP, HTTPS, mailto, or Matrix destinations and uses the
asynchronous system opener. Parsing fetches no links or URL previews.

Projection runs on blocking workers with 64 KiB HTML/output, 4096 nodes, and
32-level traversal limits; oversized messages fall back to plain text. Unchanged
content reuses its document; edits replace it. Cancellation stops further rows
after the current bounded message, and generation checks guard publication.

Measurement, drawing, hit-testing, selection, and decoration share Iced/Cosmic
Text paragraphs. Copy uses the displayed document's canonical text. Hidden
spoilers copy placeholders; reveal resets selection/layout and remains local to
the widget. Room-list, pin, and notification excerpts use the plain SDK fallback.

## Received user mentions

Explicit HTML user anchors with validated `https://matrix.to/#/@…` (including
percent-encoded IDs) or `matrix:u/…` destinations render with a soft accent
background and stronger text, without the ordinary link underline. Sender labels
and inline formatting remain intact; code keeps its code appearance. Wrapping,
bidi, selection, copying, spoiler hiding, and hover/click use the existing shaped
text and link pipeline. Clicking a user mention opens a known one-to-one DM, or
prefills the direct-chat dialog with the exact user ID. Explicit **Open chat** or
Enter then reuses/creates the DM through the existing asynchronous directory
service; dismissing the dialog performs no write. Self mentions do nothing.
Existing modal, busy-operation, and lifecycle guards apply. Other links still use
the system opener. No member lookup, subscription, or timer is added.

Plain `@name`, `@room`, and bare Matrix URLs are not inferred to be mentions.
Room/event links retain normal link styling. Mention completion and authoring
remain future work.

## Inline images and local embedded assets

Incoming HTML images accept validated MXC sources only; HTTP, file, data, and
application-local URLs cannot load images. Application producers supply decoded
local/formula assets. Rendering does not read files or decode on the UI thread.

Declared width/height reserve the rectangle through loading/failure. A single
dimension uses intrinsic aspect ratio when available; missing dimensions can
relayout after decoding. Invalid/zero values count as absent. Objects scale to
line width with a 2048 px dimension cap. HTML images align to the text baseline.

Images shape as U+FFFC with exact width/ascent/descent supplied by the
[Cosmic Text patch](../../vendor/cosmic-text/PATCH.md), before bidi/wrapping.
Selection treats them atomically and copies alternative text, never the shaping
marker. Spoiler images demand media only after reveal. Scroll anchors survive
intrinsic-size changes.

Visible-demand reports carry conversation/document identity. At most 32 unique
inline sources share four download/decode slots and the 96 MiB attachment pixel
cache. Inline files are limited to 2 MiB encoded, 4096 px source dimensions, and
1024×1024 decode targets. GIFs show a static frame. Hiding/closing releases demand;
pixels may remain in the bounded LRU. No image polling timer is added.

## Mathematical messages

Matrix `span[data-mx-maths]` and `div[data-mx-maths]` carry delimiter-free LaTeX.
RaTeX with bundled KaTeX fonts renders inline/display assets on the existing
blocking projection/composition worker. Plain incoming text is never scanned
for dollar signs. Formulas copy their LaTeX source and need no network service,
TeX installation, timer, or persistent cache.

Up to 16 formulas per document, each with 2048 source bytes, are attempted.
Rasterization uses twice logical resolution, capped at 2048 px per dimension,
256K pixels per formula, and 512K pixels per document. Invalid/oversized formulas
retain sender fallback text, or the LaTeX attribute if no fallback exists.
Sources/errors are not logged. Arbitrary LaTeX packages/documents are unsupported.

## Markdown composition

Room/thread drafts default to Markdown; Plain text changes mode without rewriting
the source. Mode and explicit emoji choices persist with the
[encrypted draft](drafts.md). Attachment captions remain plain.

CommonMark plus strikethrough supports headings, lists, quotes, code, and links.
Soft line breaks become explicit chat breaks. Raw HTML is escaped and Markdown
images become alternative text. `$…$` and `$$…$$` serialize to Matrix math HTML;
code and Plain text stay literal. Element's alternative backslash delimiters are
not composer syntax here, though their resulting Matrix HTML can be received.

Preview explicitly snapshots the draft and uses the same payload builder as Send;
typing does not parse or schedule preview work. Edits invalidate the preview;
account/request/source checks reject stale results. Conversion has two workers,
64 KiB input/output, 2048 parser events, and 24 nesting levels. Errors retain the
draft and permit switching to Plain text.

The original source is the Matrix `body`; generated HTML is `formatted_body` with
`org.matrix.custom.html`. The SDK owns queue admission, encryption, relations,
local echoes, and retries. No full tables, syntax highlighting, WYSIWYG editing,
inline-image authoring, or formatted media captions are implemented.

## Custom emoji

Pack discovery feeds composer/reaction pickers and shortcode completion:

| Purpose | Preferred type | Legacy type |
| --- | --- | --- |
| Room pack | `m.room.image_pack` | `im.ponies.room_emotes` |
| Global references | `m.image_pack.rooms` | `im.ponies.emote_rooms` |
| Personal pack | — | `im.ponies.user_emotes` |

Stable packs take priority per pack; sticker-only packs are excluded. Sources
include current room, up to eight joined canonical parent spaces, and at most
64 globally referenced rooms; projection caps at 64 packs/source and 1024 emoji.
A narrow `emoji-packs` sliding-sync connection requests pack/parent state with
zero history and no encryption extensions. It ends with the session. Errors
retain caches and retry on later events, without an application refresh timer.

Choosing an emoji binds a readable `:wave:` token to that image in the draft;
collisions use `:wave/2:`. Manually typed tokens do not silently resolve. Up to
256 choices persist per draft, unaffected by later catalog changes. Preview/Send
emit escaped image HTML in either mode, preserving literal code/raw HTML/image
alternatives. The plain body keeps tokens. Received emoji use the same inline
pipeline; SDK sanitization removes the emoticon marker, so sender dimensions
(normally height 32) remain. MXC reaction keys render as static images.

Picker/completion/reaction/preview demand adds at most 80 unique images to the
same four slots/96 MiB cache. Packs are not bulk downloaded. **Emoji media is
unencrypted even in encrypted rooms.** Animation and standalone stickers are
unsupported. Protocol references: [image packs](https://spec.matrix.org/v1.19/client-server-api/#image-packs)
and [MSC2545](https://github.com/matrix-org/matrix-spec-proposals/blob/main/proposals/2545-emotes.md).

### Pack management

An account-scoped runtime-owned service serializes writes. Open/Reload performs
bounded fresh reads (up to 128 known packs), without a new background subscription.
Existing event types and unknown JSON/sticker fields are preserved; new room
packs/global references use stable types. Invalid MXC, duplicate shortcodes, and
content over 48 KiB are rejected. Permissions are rechecked before writes;
server enforcement remains authoritative.

Fresh reads detect intervening edits, but Matrix offers no conditional write
here: another client's update between GET and PUT can race. Local preparation
runs on blocking workers; explicit Save alone uploads, without source paths or
filenames. Metadata failure after upload can leave unused media. Deleting entries
removes references, not media; existing messages/draft bindings keep their MXC URI.

Read/file tasks are abortable and account/dialog scoped. Submitted writes finish
against the captured target and participate in shutdown; load/global operations
have 30-second deadlines and saves 60 seconds. Demo edits stay in memory and
never upload. See [settings](../settings.md#emoji-packs) for file/shortcode limits.
