# UI validation

The demo provides local fixtures for UI development. Connected validation uses a
test account, dedicated rooms, and a second Matrix client for comparison. It
exercises real account state, including message sends and room-management writes.

## Build and automated checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --bin interconnect
```

For UI-only changes, use `cargo test --locked --bin interconnect` as the targeted
suite. Mock homeserver fixtures cover protocol and lifecycle behavior without
accessing real accounts. They complement desktop and connected acceptance.

## Demo checks

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo mode uses local sample conversations, people, media, verification, and
in-memory settings. It does not open saved accounts or connect to Matrix.
Sending attachments does not read/upload their contents. The explicit **Send
sample notification** action still uses the OS notification service.

The minimum window size is 640×480. Layout coverage includes ordinary desktop
sizes, sidebar visibility, narrow-window thread replacement, and both renderers;
see [renderer selection](../development.md#renderer-selection).

| Area | Controls and expected behavior |
| --- | --- |
| Navigation | Space selection filters the room list without switching the open conversation. Sections collapse independently; search filters rooms. |
| Quick switcher | Ctrl+P searches across all rooms. Up/Down selects, Enter/click opens, and Escape/backdrop dismisses. The sidebar filter remains unchanged. |
| New chat | Ctrl+N or **+** opens people search. `Taylor` is a demo fixture; repeated selection reuses the same chat. Cancellation preserves the open thread and drafts. |
| Room creation | Ctrl+Shift+N or **#+** opens the form, defaulting to the selected space or No space. Name is required. Joining, directory listing, encryption, and history options are independent where supported. Ctrl+Enter creates; Escape cancels. |
| Drafts and scrolling | Room/thread drafts and reading positions survive navigation and dialogs. Jump-to-newest resumes following incoming messages. |
| Threads | The reply arrow or Alt+T opens a thread. Its original appears once above replies; long originals and replies scroll independently. |
| Thread activity | Studio → Thread activity has interleaved replies. **New sample reply** updates the latest summary and new count for its thread. |
| Event layout | Studio → Room activity contains compact activity rows, long topics, and selectable text across message types. |
| Emoji | Studio → Emoji preview contains flags, skin tones, joined sequences, and text-presentation variants. Pickers insert glyphs; `:se` opens inline completion. |
| Images | Studio → Images has ready and failed fixtures. Escape closes the original viewer, preserving drafts and scroll positions. Originals also open from threads. |
| GIFs | Images open still. Play/Stop, once/duration settings, close, and focus loss control playback. Reopening stays still. |
| Attachments | Pick, paste, and drop stage one file per draft. The hover outline identifies the target; multiple files produce an error. Overlays block underlying drop targets. |
| Settings | Save applies changes; Cancel discards the dialog draft. Graphics changes show restart status. Demo settings stay in memory. |

Ctrl+, opens Settings, Ctrl+K focuses room search, Alt+Up/Down selects adjacent
filtered rooms, and Escape clears search or closes a thread. macOS uses Command
for the primary shortcuts. Ctrl+Enter sends by default; Enter inserts a newline.
With Enter-to-send enabled, Shift+Enter inserts a newline. IME composition
suppresses sending and navigation shortcuts. Full button keyboard traversal and
focus restoration remain unfinished.

## Desktop login

```sh
cargo run --locked --bin interconnect -- /path/to/test-profile
```

A missing or signed-out profile opens password/browser sign-in. A saved profile
restores its device without creating another one. Linux requires an unlocked
Secret Service and a display; see [development setup](../development.md) and
[profile storage](../settings.md#account-profiles). Browser SSO listens on localhost
port 8765; forward the same port to the browser's host in a container. Sign-in
has a three-minute deadline and closing cancels pending authentication.

Login acceptance covers successful sign-in, invalid credentials, unavailable
servers, browser cancellation/timeout, close during login, and same-device
relaunch. Revoking a device stops sync before returning to sign-in. Retry creates
a separate store and retains earlier stores/keys. If the server accepts login but
local setup fails, an abandoned device may remain visible in other clients.
Restoration errors do not automatically replace the saved device. Verification
and historical-key availability are separate from successful login.

## Navigation and timelines

Encrypted and unencrypted rooms load existing messages without a new send.
Comparison with a second client covers ordering, sender labels, reactions, image
content, decryption failures, topics, and pins.

History loads automatically in 50-event batches, buffering three viewport heights
(at least 1,200 pixels). Errors retain loaded messages and offer retry; exhausted
history shows **Beginning of conversation**. Threads load their original and
paginated replies, including replies received while the pane was closed.

A newly opened conversation starts at the bottom and follows incoming messages.
Scrolling up holds the visible message position; jump-to-newest resumes following.
Room/thread positions are independent. Closed-pane restoration uses pixel distance
from the bottom, so content changes while closed may affect it. Drafts, threads,
and reading positions are not restored across process exit.

Space filtering preserves the open conversation. Nested/shared spaces, collapsed
sections, unread indicators, and the last selected space/room use the navigation
model. Selection generations prevent a late snapshot from a previous room/thread
appearing in the current pane.

## Read receipts and unread clearing

A focused, unobscured room at the newest messages sends a public `m.read`
receipt and moves `m.fully_read` to the last visible server event. Opening a
room with a restored older position does not clear its unread badge. Empty rooms
can clear a manual unread flag without an event receipt. The SDK clears
`m.marked_unread` as part of an unthreaded receipt; badge/count changes follow SDK
sync state rather than an optimistic local override. Demo badges clear in memory.

Thread panes send public receipts for the last visible reply in that thread;
they do not move the room's fully-read marker. Room timelines contain coalesced
thread activity, so their receipts are unthreaded and can advance the server's
read position across those replies. The separate local thread activity count
still requires viewing replies in the thread pane.

Room messages show **Read by N people** with Matrix IDs on hover, excluding the
current account. These positions include the SDK's implicit sender receipts;
they are read positions, not delivery or per-message acknowledgements. Thread
reader labels are omitted because this SDK's timeline-item receipt API does not
expose thread-specific readers. There is currently no receipt privacy setting.

Requests are serialized per view, time-limited, and cancelled when their viewport
is superseded, the window loses focus, an overlay obscures the conversation, or
the view closes. A request already received by the server cannot be undone.
Failures show an inline notice and retry on a new visible target or refocus;
there is no recurring retry/animation timer. A newer SDK event never becomes a
receipt target until the viewport reports it visible.

Automated coverage uses local mock servers and memory-only demo state for receipt
scope/payloads, manual-flag clearing requests, deduplication, failure/retry,
reader projection, stale generations, focus/overlay gating, empty rooms, and
saved scroll gating. Live homeserver and native desktop acceptance remain manual:

1. In an authorized test room, compare the unread badge and your read position
   in a second client before and after focusing the newest messages here.
2. Scroll up, switch away and back, and receive a message from the other client.
   Verify the restored position and unread badge persist until jumping to newest.
3. Repeat while unfocused and with Settings or the image viewer open; verify no
   new receipt until the conversation becomes visible and focused again.
4. Read a thread and compare its receipt in the other client; verify its receipt
   does not move this room's fully-read marker. Compare incoming room-reader
   labels, including a receipt-only sync update with no new message.
5. Test offline receipt failure, refocus after reconnecting, manual unread flags
   from another client, closing during a pending request, and unchanged idle CPU.

Demo Studio includes a sample reader label. Demo room badge clearing and receipt
labels do not access saved accounts or send network requests.

## Sending and room management

| Operation | Behavior and comparison points |
| --- | --- |
| Text and attachments | Room/thread sends use SDK encryption and local echoes. Queue admission clears the draft before delivery completes. Retry retains the original transaction. Captions, thumbnails, and thread relations can be compared in a second client. |
| Received attachments | Open launches the system handler; Save as uses a native dialog. Images, files, audio, and video download originals, including SDK decryption, in room/thread messages and thread originals. Image previews also retain their in-app viewer. |
| Reactions | Chips toggle reactions; the emoji picker adds a selection. Pending/error state belongs to its captured message, with stale results rejected after navigation. |
| Clipboard | Native file lists and images stage attachments; plain-text paths remain text. Multiple files leave the previous draft intact with an error. Host-selected paths need matching container access. |
| File drops | Only the visible conversation under the pointer stages the file. Sidebar, disabled panes, and overlays are excluded. Wayland delivery and macOS coordinates have separate native coverage; see [the Winit patch](../architecture/winit-file-drops.md). |
| Direct chats | People search accepts names and full Matrix IDs. Existing one-to-one DMs are reused; new chats invite the peer and enable encryption. Dismissal during creation prevents late navigation. |
| Room management | Invitations, invitation accept/reject, directory search/join, member actions, topic changes, and pin/unpin depend on server permissions and expose request errors. |
| Room creation | Public/private joining, history, directory visibility, and encryption become room state. Space permission denial fails before creation. A failed child link offers **Retry adding to space** and **Open room**; retry links the existing room. |

## Lifecycle and desktop integration

Network loss preserves loaded content and drafts; the SDK handles reconnection.
Failed sends retain explicit Retry. Additional lifecycle scenarios include offline
startup, revoked tokens, suspend/resume, and close during pending operations.
Normal close waits for writes and SDK shutdown. A shutdown error stays visible
for dismissal.

The [notification](notifications.md), [verification](verification.md), and
[idle-resource](idle-resources.md) references cover native delivery, trust, and
resource measurements. Notifications do not send read receipts. Recovery UI and
notification click navigation remain unfinished.

## Validation coverage

November 16, 2026 read-receipt validation passed formatting, workspace Clippy,
a locked desktop build, workspace tests, and the focused receipt and desktop
suites after the final changes. The receipt checks used mock servers and demo
state; no saved account or connected desktop was launched for this change.

Recorded Linux checks through November 15, 2026 include formatting, workspace
Clippy, locked builds, and mock protocol/UI tests. The room-creation change
recorded 95 library and 169 desktop tests passing, with two opt-in library tests
ignored.
Fixtures cover encrypted sending/retry, history, navigation, stale results,
runtime-safe teardown, offline restoration, reconnect, media, and room management.

Isolated Wayland/tiny-skia demo checks covered scrolling, quick switching, new
chat and room creation, header buttons, and 1000×720/640×480 layouts. Earlier
connected checks confirmed encrypted room/thread sending, historical-key
exchange with a trusted client, same-device resume, and Wi-Fi recovery.

Real-account acceptance of newer workflows, host clipboard/portal/drop behavior,
hardware-renderer performance, host IME behavior, and native macOS behavior remain
separate checks. Automated protocol success does not establish a live account's
trust or historical-key availability. Acceptance results identify the build,
platform, scenario, outcome, and remaining gaps.

## Formatted messages

Studio → **Message formatting** is a memory-only demo fixture for mixed styles,
headings, nested quotes/lists, code with preserved whitespace, image alternative
text, links, spoilers, emoji and bidirectional text. Launch only with
`cargo run --locked --bin interconnect -- --demo` for local experiments.

- Resize the room and thread panes; select across wrapped styles, paragraphs,
  code lines and adjacent plain messages. Copy preserves visible Unicode text
  and excludes markup and message controls.
- Hover links to inspect destinations. Dragging selects without opening them.
  Clicking opens an allowlisted destination through the system handler.
- Copy a hidden spoiler, reveal it, then copy again. Hidden text must not appear
  in the clipboard. Revealing must preserve the reading anchor.
- Open a thread on a formatted fixture and check the original uses the same
  renderer. Inline images reserve declared dimensions and use viewport-scoped
  authenticated media requests.
- Keep a draft and a scrolled reading position while switching spaces/rooms.
  Settled formatted messages must not schedule recurring redraws.

Automated coverage exercises parsing limits, entities/whitespace, combined
styles, code/image metadata, hidden spoiler text and nested links, narrow/wide
Unicode layout and hit-testing, stable draw/layout, cross-message copy, and idle
redraw behavior. Parser/cache tests and SDK projection tests distinguish plain
fallbacks from HTML. History fixtures include formatted root messages and verify
parsed-document reuse. They await timeline content separately from SDK page
completion, since those are distinct event-driven updates.

November 16, 2026: formatting checks, workspace Clippy with warnings denied, the
locked desktop build, and workspace tests passed (101 library and 176 desktop
tests; two opt-in library tests ignored). An isolated Linux Wayland/tiny-skia
demo at 1100×800 and 640×480 covered styled text, nested lists/quotes, code and
image alternative text, link-destination overlays, spoiler reveal with a stable
reading position, and formatted thread originals. Link activation is covered by
message-emission tests; the desktop check did not launch external handlers.

Real-account formatting, hardware-renderer appearance, native macOS behavior,
and measured idle resource use remain separate acceptance checks. Idle coverage
here verifies that settled widgets request no further redraw deadlines.

## Clickable message links

Studio → **Message formatting** includes a bare-link message and thread reply.
HTTP/HTTPS URLs and `www.` addresses work in ordinary text and formatted text;
`www.` opens with HTTPS. Hover shows the normalized destination. URL punctuation
is excluded while balanced parentheses and query parameters remain part of it.
Code spans/blocks stay literal and existing anchors retain their destinations.

Automated checks cover literal whitespace/markup, Unicode, punctuation, schemes,
input limits, document reuse/edit invalidation, plain/HTML source identity,
spoilers and inert anchors. Widget tests exercise both explicit and bare links:
clicks emit the opening action, selection drags do not, hover exposes an overlay,
and settled widgets request no recurring redraw deadline.

November 16, 2026: formatting, workspace Clippy with warnings denied, the locked
desktop build and workspace tests passed (127 library and 196 desktop tests;
two existing opt-in library tests ignored).

For native acceptance, launch `cargo run --locked --bin interconnect -- --demo`,
open the fixture and its thread, and click a sample URL to check the default
browser. Drag-select and copy the message, resize to 640×480, and switch rooms
with a draft and scrolled position. Browser/system-handler launch, native macOS
appearance, and measured idle resources remain manual acceptance checks. No
connected account or external handler was launched for the automated checks.

## Markdown composer

In demo mode, type `**bold**`, `_italic_`, `~~old~~`, a Markdown link, or a fenced
code block into either composer. Choose **Preview** to inspect formatting, then
send a sample locally. The source stays editable; changing it hides the old
preview. Preview appears only on request and has a bounded scroll area.

- Switch **Markdown** to **Plain text** and confirm syntax sends literally.
  Switching modes preserves source and selection; room/thread choices and drafts
  remain independent across navigation and space filtering.
- Check Ctrl+Enter, the configured Enter behavior, newlines, IME suppression,
  emoji insertion/completion, and the minimum 640×480 window with preview open.
- Simulate a format/queue error: source remains editable, and another draft is
  untouched. A stale preview result after edits, mode/account changes or closing
  the preview must not reopen it.
- Raw HTML is literal; unsafe Markdown links are inert and image syntax shows
  alternative text. Captions are explicitly plain text. No preview fetches links
  or images.

Automated parser tests compare preview documents and Matrix payloads. Mock
protocol checks cover formatted local echoes, encrypted/unencrypted room and
thread sends, and retries of the original transaction. No live-account test
messages are needed for these checks.

November 16, 2026: formatting checks, workspace Clippy with warnings denied,
the locked desktop build, and workspace tests passed (104 library and 180
desktop tests; two opt-in library tests ignored). Isolated Linux Wayland/tiny-skia
checks at 1100×800 and 640×480 covered source entry, on-demand styled previews,
local Markdown sends, literal-mode switching and local sends. The connected
client was not restarted and no test messages were sent to Matrix. Native macOS,
hardware-renderer appearance, live-account acceptance and measured idle resource
use remain separate checks.

## Inline images

Studio → **Inline images** in demo mode contains mixed text/images, different
widths/heights, narrow wrapping, a transparent local asset with baseline descent,
loading/failure rectangles, a hidden spoiler and bidirectional text. All fixture
pixels are in memory and no account/network access is required.

- Resize room and thread panes, including 640×480. Images must wrap as indivisible
  objects and enlarge their line boxes without overlapping neighboring lines.
- Compare declared-size loading, ready and failed states: the surrounding layout
  stays fixed. Intrinsic-size-only images may relayout when decoded; preserve the
  visible message anchor and bottom-follow behavior.
- Drag-select across text, images and adjacent messages in both directions. Copy
  must contain complete alternative text and no U+FFFC layout markers.
- Reveal the spoiler. Its image enters layout/demand then; hidden images must not
  produce requests. Settled images must not schedule redraw deadlines or animate.
- A local asset supplies RGBA pixels plus logical width/height/descent, without a
  Matrix URI. Formula rendering also uses this embedded-object layout path.

Automated checks cover exact dimensions, intrinsic aspect ratios, bounded extreme
sizes, baseline descent, bidi wrapping, object hit-testing, software-rendered
pixels, alt-text copying, viewport demand, stale document/generation rejection,
loading/failure stability, authenticated downloads and request deduplication.

November 16, 2026: formatting checks, workspace Clippy with warnings denied,
locked desktop builds and workspace tests passed (106 library and 188 desktop
tests; two opt-in library tests ignored). Isolated Wayland checks covered mixed
images/text and local transparent assets at 1100×800 and 640×480 with tiny-skia,
plus thread originals and a run configured for wgpu with Mesa's software Vulkan
ICD. The software-rendered pixel test checks the declared 80×40 rectangle, and
an integration test preserves the visible message anchor across intrinsic-size
arrival. A settled wgpu inline-image demo consumed zero CPU jiffies in a
five-second `/proc` sample at 100 ticks/second; this short sample does not measure
wakeups or power. Real-account media, hardware adapters and macOS remain separate
acceptance checks. The connected client was not restarted and no Matrix test
messages were sent.

## Mathematical messages

In `--demo`, open **Off the clock**, leave the composer in Markdown mode, and
preview/send this sample locally:

```text
Inline $\frac{a}{b}$ and $\sqrt{x^2+y^2}$.

$$\sum_{n=1}^{10}n$$
```

- Inline equations align to surrounding text; display equations have their own
  centered block. Resize to 640×480 and check wrapping and adjacent lines.
- Select across text and formulas: copy uses complete LaTeX sources, without
  U+FFFC layout markers. Formula sources are atomic selection objects.
- Code spans/blocks and Plain text mode keep dollar syntax literal. Unsupported
  commands such as `$\unsupported{x}$` retain a readable source fallback.
- Received Element messages use `data-mx-maths` attributes in formatted HTML.
  Plain incoming dollar-delimited text stays literal. Live Element exchange and
  native macOS appearance remain separate acceptance checks.

Automated checks cover SDK sanitization of Element-compatible math markup,
preview/wire serialization and escaping, inline/display layout, baseline metrics,
source copying, spoilers, document reuse, fractions, roots, integrals, matrices,
aligned equations, chemistry, malformed/recursive input, and source/pixel/count
limits. Rendering is local with bundled fonts and introduces no redraw timer.

November 16, 2026: formatting, workspace Clippy with warnings denied, a locked
desktop build and workspace tests passed (113 library and 189 desktop tests;
two existing opt-in library tests ignored). Isolated Wayland/tiny-skia demo checks
covered inline fractions/roots, centered sums, on-demand preview and local send
at 1100×800 and 640×480, plus dragging a selection across formulas. A settled
formula timeline consumed zero CPU jiffies during a five-second `/proc` sample
at 100 ticks/second; this does not measure wakeups or power. Clipboard export
was not confirmed in the isolated virtual-input session; canonical copy text is
covered by automated tests. Hardware rendering and live-account acceptance remain
untested for this change. No connected client was restarted and no Matrix test
messages were sent.

## Saving and opening received attachments

Automated coverage added November 16, 2026 exercises original-source extraction
for images/files/audio/video (including filenames separate from captions), exact
plain/encrypted download bytes against a mock authenticated media endpoint,
filename sanitization, save cancellation, stale navigation, cancellation tokens,
atomic destination replacement, and write failures. These fixtures do not launch
external applications or exercise native save dialogs.

Native acceptance remains required:

- In `--demo`, open Studio → Images. Save the sample PNG and `notes.txt`, cancel
  a save, and replace an existing scratch file. Open each using the default app.
  Repeat from the image viewer and a thread original; check controls at 640×480.
- With an authorized test account, save image/file/audio/video originals from an
  encrypted room and thread. Compare bytes and filenames to the sender's files,
  including a caption, a missing thumbnail, and an uncached original.
- Try offline, unavailable encryption keys, and an unwritable destination. Check
  the attachment shows an error and its controls allow another attempt. Confirm
  changing conversations while choosing a destination or downloading produces no
  late write or external launch; drafts and reading position must be preserved.
- Check Linux FileChooser portal and `xdg-open`, and macOS save dialog and `open`.
  Opened temporary files remain available until client exit and are then removed;
  saved files remain. Confirm settled UI has no additional recurring wakeups.
