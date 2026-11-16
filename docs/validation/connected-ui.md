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
| Custom sections | Use the room-list **⋯** to create/manage sections, or right-click a custom header to rename/remove it. Studio → Design groups Engineering and Research under an Element-format custom section. Collapse it, search for a room, and switch spaces; grouping follows the filtered list without switching the conversation. |
| Navigation | Space selection filters the room list without switching the open conversation. Sections collapse independently; search filters rooms. |
| Quick switcher | Ctrl+P searches across all rooms. Up/Down selects, Enter/click opens, and Escape/backdrop dismisses. The sidebar filter remains unchanged. |
| New chat | Ctrl+N or **+** opens people search. `Taylor` is a demo fixture; repeated selection reuses the same chat. Cancellation preserves the open thread and drafts. |
| Room creation | Ctrl+Shift+N or **#+** opens the form, defaulting to the selected space or No space. Name is required. Joining, directory listing, encryption, and history options are independent where supported. Ctrl+Enter creates; Escape cancels. |
| Drafts and scrolling | Room/thread drafts and reading positions survive navigation and dialogs. Jump-to-newest resumes following incoming messages. |
| Context menus | Right-click a room entry for **Open room**, **Move to section…**, and **Copy room ID**. The section picker includes empty custom sections, Favorites, Low priority, and Default; the current section is checked. Messages in rooms, thread originals, and replies offer **Copy text**, reactions, and permission-gated pin/unpin; room messages also offer **Reply in thread**. Thread summaries offer **Open thread** and **Copy thread ID**. Menus stay inside the window, dismiss on Escape/outside click/focus loss, and preserve drafts and reading positions. An outside click dismisses without activating the item underneath. |
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

## Element custom sections

Protocol references: Element's [section schema and ordering](https://github.com/element-hq/element-web/blob/develop/apps/web/src/stores/room-list-v3/section.ts),
[section membership](https://github.com/element-hq/element-web/blob/develop/apps/web/src/stores/room-list-v3/skip-list/filters/SectionFilter.ts),
[account settings storage](https://github.com/element-hq/element-web/blob/develop/apps/web/src/settings/handlers/AccountSettingsHandler.ts),
and [room tag changes](https://github.com/element-hq/element-web/blob/develop/apps/web/src/utils/room/tagRoom.ts).

Display reads `RoomList.CustomSectionData` and `RoomList.OrderedCustomSections`
from global `im.vector.web.settings` account data. Room `m.tag` keys of the form
`element.io.section.<id>` supply membership. Names are labels; tags identify
sections and local per-space collapse state. Valid definitions omitted from the
order are placed before Chats; invalid definitions and stale order entries are
ignored. Unknown/deleted tags leave rooms in their usual groups.

Favorites stays first and Low priority last. Explicit tags win over inferred
Direct messages/Unread grouping; multiple tags use the first displayed explicit
section. Element's `chats` order slot retains this client's Unread and Rooms
groups, and `im.vector.fake.direct` positions Direct messages. Empty custom
sections appear in their creation scope using Element's `spaceId` (`home-space`
for All rooms/Direct messages); legacy sections without a scope default there.
Search hides empty sections. Nonempty sections follow room membership in the
selected space regardless of their creation scope.

Right-click any joined room → **Move to section…** to move it to Favorites, a
custom section (including an empty one), Low priority, or Default. Default clears
explicit section tags, returning to inferred Direct messages/Unread/Rooms
grouping. These inferred groups are not independent move destinations. Choices
use stable IDs even when labels are duplicated. Selecting the current destination
keeps the room there rather than toggling it out.

An explicit move reads the server's current `m.tag` content, removes Favorites,
Low priority, and all custom section tags (including orphaned tags), then sends a
single replacement `m.tag` event. Unrelated tags, their metadata, and an existing
destination's metadata are preserved. Custom destinations are checked against
current server definitions. Reads that fail or return malformed tags abort the
write; a missing `m.tag` event is treated as empty. Same-account moves serialize
and have a 30-second deadline including the queue. Cross-client edits between
GET and PUT can still race; the protocol has no conditional replacement.

Submitted moves finish against the captured account after dismissal and
participate in shutdown. Connected grouping follows sync; demo moves only edit
memory. Pending choices are disabled, failures appear in the picker (or room-list
feedback after dismissal), and obsolete completions cannot close a newer menu.
Moving never selects the room or changes drafts/reading positions.

The **⋯ → New section…** menu beside the room-list heading creates sections. It
also lists every custom section, including empty sections and sections outside
the current space: select one to rename/remove it. Right-click a custom section
header for the same actions. Built-in sections cannot be renamed or removed.
Names are trimmed and must be nonempty; duplicate labels remain independent IDs.
Removal asks for confirmation and removes only the definition and its order
entries, leaving room tags and joined membership intact. Those rooms immediately
fall back to their remaining valid tags or inferred groups after sync.

Creation allocates an Element-format UUID v4 tag when the form opens and inserts
it before `chats`. Rename preserves that tag, scope, order, unknown definition
fields, and local collapse state. Edits read fresh global settings and update
both definition/order keys in one account-data PUT while preserving unrelated
settings. Malformed data aborts the write. Submitted edits share the room-move
serialization/deadline and shutdown accounting; dismissed/stale results cannot
close a newer menu. Retries reuse the creation tag to avoid duplicate sections.
Demo changes remain in memory. Cross-client GET/PUT races remain possible.
Drag the **⠿** handle at the right of a section header to reorder it. The blue
line marks insertion above/below the target header. Favorites and Low priority
stay pinned. Direct messages moves independently; Unread and Rooms move together
in Element's `chats` order slot. Order is account-wide, including sections hidden
by the current space/search, and keyboard room navigation follows the new order.
Right-click a header for **Move section up/down**; the same commands are available
for empty/off-screen custom sections through **⋯ → section name**.

Reordering changes only `RoomList.OrderedCustomSections`, using fresh server
settings and preserving definitions/metadata and unrelated settings. The saved
order normalizes duplicate/stale entries and retains all valid sections, including
those hidden in the current view. Relative before/after moves are idempotent on
retry. Deletion of either endpoint before the write returns an error. Room tags,
open conversations, drafts, scroll positions, and local collapse state are kept.
Pending reorders disable new drags, errors appear in the room list, and shutdown
waits for submitted writes using the existing 30-second settings-edit deadline.

A handle click without a drop on another section makes no change. Release outside
a valid header, Escape, right-click, wheel scrolling, focus loss, window resize,
space/search changes, account changes, or a modal cancels a drag. There is no
automatic scroll timer; scroll first or use the up/down commands for distant
sections. Hover updates occur only during a drag and only when its target changes.
Element's visibility/device expansion settings remain unimplemented.

Automated coverage includes schema validation, duplicate labels and order entries,
multiple-tag precedence, stale tags, room-tag sync, and settings-only syncs for
creation, rename, reorder, and deletion. UI state tests cover stable collapse
identity and space filtering. Move fixtures cover full tag replacement, preserved
metadata, repeated moves before sync, deleted destinations, malformed data,
read/write failures, stale accounts/menus, shutdown, and draft/scroll preservation.
Section-management fixtures cover create/rename/removal without intervening sync,
preserved remote settings and definition metadata, idempotent retries, deleted
destinations, malformed settings, HTTP failures, empty-section scope visibility,
confirmation/cancel, and conversation preservation. Reorder fixtures cover both
directions, Chats/People slots, pinned/stale endpoints, unchanged definitions,
retries before sync, clipping/hover capture without scheduled redraws, cancellation,
stale completions, shutdown, and unchanged conversation/draft/scroll/collapse state.
An isolated headless Sway demo with tiny-skia was visually checked at 1280×900:
right-clicking the unselected General room opened the menu, **Move to section…**
showed the checked Favorites destination, and choosing Design moved General
there while Design studio remained open. Section-management demo checks also
created Projects via **⋯**, entered its name and submitted with Enter, renamed
its empty header to Planning, and confirmed removal. The open Design studio
conversation stayed selected throughout. A subsequent reorder demo check verified
both drag directions, the before/after insertion line, moving Design across the
shared Chats slot and above Direct messages, and releasing outside all headers
without changing order. Design studio remained selected. These checks used
isolated headless Sway/tiny-skia at 1280×900. Live Element interoperability and
native desktop acceptance still require manual checks:

1. In Element on a dedicated test account, create two custom sections, move rooms
   (including a DM) into them, and reorder them around Chats. Open the same account
   here and compare names, membership, and order.
2. Rename, reorder, and delete sections in Element while this client runs. Check
   updates without navigating or reopening, and that a deleted section's rooms
   remain accessible. Confirm this client makes no section/tag writes until an
   explicit move or section-management save is chosen.
3. Check All rooms, Direct messages, and nested/shared spaces. Empty sections
   appear in their creation scope and stay available in the management menu;
   changing spaces preserves the conversation, thread, drafts, and scroll.
4. Collapse a section, rename it remotely, and confirm it stays collapsed. Search
   exposes matching rooms temporarily; clearing search restores collapse state.

5. Move a non-selected room here to each destination and compare in Element after
   sync. Include a DM, an empty custom section, a room with unrelated tags, and
   Default. Preserve an open room/thread draft and reading position throughout.
6. Disconnect before choosing a destination; check pending/error feedback and
   retry after reconnecting. Dismiss a pending move, open another menu, and close
   the app; the completion must not close a newer menu or skip shutdown cleanup.

7. Create sections here in All rooms and a space, including duplicate names;
   compare IDs/names/order/scope in Element. Rename a collapsed section and confirm
   collapse is preserved. Remove empty and populated sections; rooms stay joined
   and accessible. Cancel/escape must leave settings unchanged. Test read/write
   failures and dismissal/close during a pending save with a dedicated account.

8. Drag a custom section above and below another header, and move Direct messages.
   Verify the insertion line, pinned endpoints, and shared Unread/Rooms movement.
   Compare the resulting order in Element after sync/relaunch. Use up/down commands
   on empty custom sections via **⋯**. Try Escape, outside release, scrolling,
   focus loss, account/space changes, and remote deletion during a drag. No canceled
   drag should write or activate a room/header. Verify error feedback on a refused
   reorder and closure while a submitted reorder is pending.

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

The spaces bar uses a room-grid icon for All rooms and two people with a shared
speech bubble for Direct messages. These static vector icons inherit the button
color; their full names remain available in tooltips.
The spaces bar displays each space's configured avatar through the shared static
thumbnail cache. Missing, loading, or failed icons retain the initial fallback;
the button size, selection highlight, and tooltip stay the same. Icon changes and
removals follow account sync without a polling timer. Demo mode makes no icon
requests.

For connected acceptance, use another client to set, replace, and remove a space
avatar, and confirm the bar updates without switching the open conversation.
Check a space without an avatar and an unavailable image for the initial fallback.
These rendering and live-account checks remain manual. Automated coverage checks
SDK metadata updates/removal and deduplicated media demand across space selection,
icon replacement, space removal, and demo isolation.

November 16, 2026: space-icon validation passed formatting, workspace Clippy with
warnings denied, a locked desktop build, and workspace tests (127 library tests
passed, two ignored; 197 desktop tests passed). No live-account or desktop visual
acceptance was performed for this change.

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

## Context menu validation

Automated widget tests cover right-click capture, clipped targets, cursor-based
placement at window edges, Escape/outside-click capture, and the absence of
scheduled redraws. Demo state tests cover opening without navigation, room/thread
actions, permission changes, stale and dismissed choices, modal exclusion, and
draft/scroll preservation. Existing UI tests cover the reused SDK action handlers.

A Linux Wayland demo smoke check on 2026-11-16 verified room-list menus,
message menus positioned at the cursor in scrolled history, outside-click
dismissal, opening a thread from a message menu, and thread-reply menus. This
check used the GPU renderer at 1096×681; it did not validate native clipboard
contents, every action, minimum-size rendering, or connected accounts.

Room entries also offer **Mark as unread**, including for the open room. This
sets Matrix's explicit unread flag without moving receipts or inventing a message
count. The open room and its thread, drafts, and saved scroll positions stay in
place. Automatic read marking pauses for that conversation until the room is
opened again; space filtering and focus changes do not reset this pause. A
connected request shows progress, and failures remain in the menu for retry.
Demo changes stay in memory and never access an account or the network.

Automated coverage checks the account-data request and server refusal with a
mock server, plus room selection, drafts, scroll preservation, and the unread
pause across viewport updates in demo state. On November 16, 2026, formatting,
workspace Clippy with warnings denied, the locked desktop build, workspace tests
(134 library tests passed, two ignored; 214 desktop tests passed), and the final
desktop suite passed. No connected client was restarted or Matrix test messages
sent. For manual acceptance, mark an
unselected room and the open room unread, change spaces, refocus the window,
then reopen the room at the bottom and confirm it becomes read. Live account
sync, native menu appearance, and macOS acceptance remain separate checks.

For desktop acceptance, use `--demo`: right-click both an unselected room and a
message, copy to a separate editor, open a reaction picker, and pin/unpin. Open a
thread and repeat on the original and a reply. In **Thread activity**, right-click
a summary and open it. Check Escape, outside clicks, focus loss, and menus near
all window edges at 640×480. Confirm scrolling remains blocked while the menu is
open and resumes at the same position after dismissal. File drops must not stage
an attachment through an open menu. Connected and native macOS checks, including
secondary click and clipboard integration, require separate desktop acceptance.
