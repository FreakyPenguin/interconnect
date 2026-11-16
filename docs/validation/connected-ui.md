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

## Sending and room management

| Operation | Behavior and comparison points |
| --- | --- |
| Text and attachments | Room/thread sends use SDK encryption and local echoes. Queue admission clears the draft before delivery completes. Retry retains the original transaction. Captions, thumbnails, and thread relations can be compared in a second client. |
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
resource measurements. Notifications do not send read receipts; recovery UI,
automatic read marking/receipts, and notification click navigation remain unfinished.

## Validation coverage

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
  renderer. Inline images remain alternative text with no download.
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
