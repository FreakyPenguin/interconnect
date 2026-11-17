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

Timeline-label regression coverage checks a single rendered thread entry and actual
mouse routing to the expected thread in preview, missing-preview, search, and
event-focus views. Layout checks compare empty/loading/error status bounds
(including a missing snapshot and failed initial history), exercise the
initial-history retry action, and verify retained rows after an error; existing
history checks cover loading with rows and scroll preservation. Formatting,
workspace Clippy, and all 276 UI tests pass. Room/thread metadata and empty,
loading, and error layouts were checked in the isolated Sway/tiny-skia demo at
1280×900 and 640×480. Connected acceptance and macOS/wgpu remain untested; no
connected accounts were used.

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
| Navigation | Space selection filters the room list without switching the open conversation. Room and child-space rows stay 26 px high with no gaps between rows, including long names and unread badges. Long room, section, and space labels use a grapheme-safe ellipsis; hovering a room name reveals its full name after a short delay. Unread room names are semibold. Expanded section counts and zero-count Saved messages badges are hidden; collapsed sections retain nonzero room counts. Sections collapse independently; search filters rooms. |
| Unread section | Unread stays above Favorites and includes all unread rooms in the selected space/filter, including DMs, favorites, and custom sections. Rooms remain in their usual groups. Collapse it independently; search temporarily expands it. It hides when empty. **Settings → Navigation → Show unread section** defaults on; Save/Reload applies immediately, Cancel discards changes. Marking a room read removes its shortcut while preserving its usual row, open conversation, drafts, and timeline position. |
| Space unread dots | A small blue dot at the bottom right of a rail icon means its scope contains unread rooms or DMs, including manually marked unread rooms and nested spaces. All rooms covers every room; Direct messages covers DMs. The dot clears when the last unread room is read or removed. Indicators ignore room search, collapsed sections, and the Show unread section setting. In the demo, All rooms and Studio start with a dot for Release notes. Hover adds **Unread messages** to the space hint; clicking the dot still selects the space without switching the open conversation. |
| Presence | Opening a DM or People page checks status on demand; Refresh status rechecks the current view. Marks are last-checked snapshots, with a 60-second cache and no polling. Message-group avatars (rooms and threads) show an 8 px status mark; DM navigator rows reuse the peer’s checked status. Maya is online (green), Alex away (amber with a dash), Jamie offline (gray); other demo senders have no status mark. Hover a mark for its text label. Group continuations and room activity do not add marks. Switch spaces without changing the open conversation; check 26 px DM rows and minimum-window clipping. |
| Tooltips | Hover the account avatar, spaces, room-list toggle, header controls, composer actions, message timestamps/senders, and emoji choices. Hints use compact 11 px light text in a charcoal rounded surface with consistent padding, a subtle border/shadow, and a gap from the control. Check long topics and multiline read receipts wrap within the surface, and hints stay inside the window near its edges. Native rendering remains a manual check. |
| Global message search | **Search messages** above Saved messages or Ctrl+Shift+F (Cmd+Shift+F on macOS) opens a full-size panel. Enter or **Search** submits; typing cancels the previous request and clears its results. Results span all rooms regardless of the selected space and include room, sender, date, highlighted text, and **Go to message/reply**. Check pagination, duplicate suppression, no matches, server failure with local results, and switching to Saved messages or a room. Back/Escape restores the previous conversation, thread, drafts, and scroll; the panel sends no read receipts for hidden conversations. |
| In-room search | Header magnifier or Ctrl+F (Cmd+F on macOS) opens and focuses the bar. Search is a case-insensitive substring of loaded message bodies, including loaded replies; it does not query server history. Matches appear in timeline order, starting at the newest. Enter/↑ selects the previous match, ↓ the next, wrapping at either end. Every occurrence in visible plain and formatted message text has a yellow highlight while search is open; the selected message also has a row highlight and is scrolled into view. Check repeated terms, wrapped text, links, code, emoji, and RTL text. Clearing or closing search removes text highlights without changing selection/copy behavior or wrapping. Check no matches, Unicode text, edits, history pagination, and Escape/× closing without changing drafts or the open thread. Space filtering keeps search; switching rooms clears it. At narrow widths, opening search returns from the side pane to the room, retaining drafts. |
| Quick switcher | Ctrl+P searches across all rooms. Up/Down selects, Enter/click opens and focuses the room composer once its pane loads, and Escape/backdrop dismisses. The sidebar filter remains unchanged. |
| New chat | Ctrl+N or **+** opens people search. `Taylor` is a demo fixture; repeated selection reuses the same chat. Cancellation preserves the open thread and drafts. |
| Clickable usernames | Click another sender's name in room messages, thread originals/replies, pinned messages, or the People panel to open their DM. Hover highlights the name and shows their Matrix ID and **Open direct chat** hint. Own names stay plain. Return to the source room/thread and check drafts and reading positions; repeat the click to reuse the same demo DM. Space filters stay unchanged. |
| Room creation | Ctrl+Shift+N or the **Create room** icon opens the form, defaulting to the selected space or No space. Name is required. Joining, directory listing, encryption, and history options are independent where supported. Ctrl+Enter creates; Escape cancels. |
| Message deletion | Use the trash button in an eligible message’s hover popover or **Delete message…** in its context menu. Confirm explicitly; Escape/outside click cancels. Check own messages, media, thread originals/replies, permission loss, and server errors. |
| Message editing | Use the pencil button in a message’s hover action popover, right-click and choose **Edit message**, or press Up in an empty focused composer to edit the latest own message in that conversation. Save uses the configured send shortcut; Cancel or Escape restores the previous draft. Check room messages, thread originals/replies, and navigation away and back. |
| Composer | Text and toolbar occupy separate rows within one focus-highlighted surface, aligned with the reading column. Attachment/emoji controls sit together on the left; the format pill and eye icon control literal/Markdown mode and preview. Send is blue when enabled and muted when unavailable. Attachments use an inline chip with filename ellipsis; errors stay inside the surface. Check multiline editing/scrolling, focus, toolbar hit targets, staged captions, preview toggling, and keyboard shortcuts in room and thread panes at 640×480. |
| Drafts and scrolling | Room/thread drafts and reading positions survive navigation and dialogs. With the room composer focused, Page Up/Down scrolls room messages; with the thread composer focused, it scrolls thread replies. Paging leaves a small overlap and preserves draft text, caret, and selection. Modified paging keys retain editor behavior. Jump-to-newest resumes following incoming messages. |
| Context menus | Right-click a room entry for **Open room**, **Move to section…**, and **Copy room ID**. The section picker includes empty custom sections, Favorites, Low priority, and Default; the current section is checked. Messages in rooms, thread originals, and replies offer **Copy text**, reactions, and permission-gated pin/unpin; room messages also offer **Reply in thread**. Thread summaries offer **Open thread** and **Copy thread ID**. Menus stay inside the window, dismiss on Escape/outside click/focus loss, and preserve drafts and reading positions. An outside click dismisses without activating the item underneath. |
| Bookmarks | Hover a room message, thread original, or reply and use the outline/filled bookmark icon. **Saved messages** above the sections opens the account-wide view. Back/Escape restores drafts and reading positions. Demo bookmarks and previews stay in memory. |
| Panel resizing | Drag the divider to the right of the room list or to the left of a split thread/People panel. The cursor changes to horizontal resize and the divider highlights on hover. Room-list, thread, and People widths are remembered independently for the current session. Viewport-dependent limits preserve usable conversation space. Narrow windows keep the existing replacement view for thread/People, without a side divider. |
| Threads | The reply arrow or Alt+T opens a thread. Its original appears once above replies; long originals and replies scroll independently. |
| Pinned messages | Studio → Design starts with a compact **2 pinned messages** disclosure. Expand/collapse it to show bounded, scrollable previews and permission-gated unpin buttons. Previews use the conversation rich-text renderer: check emphasis, lists, quotes, code, clickable links, and concealed spoilers. Space filters and thread navigation keep the disclosure; a newly opened room starts collapsed. Check long pins, unavailable/loading pins, and live additions/removals without losing the timeline reading position or drafts. |
| Thread activity | Studio → Thread activity has interleaved replies. Each loaded preview provides one reply count and opens its latest reply; its root omits the duplicate count link. Roots without loaded previews, and raw search/event-focus views, retain the root reply link. Preview headings show the latest sender; hover preserves root context and timestamp. **New sample reply** updates the latest summary and new count for its thread. |
| Conversation states | Empty, opening, and initial-error messages occupy the same centered reading column above the composer. Failed initial history includes its retry action. Empty rooms omit **Beginning of conversation**. Loading or errors after messages arrive retain those rows and history controls. |
| Event layout | Message-group timestamps sit beside sender names; same-sender continuations reveal their time in the avatar gutter on hover. Sender names truncate safely on narrow panes, with identity/date tooltips retained. Message content is capped at 840 px including padding on wide windows; the scrollbar stays at the pane edge. Check group spacing, text selection/copy, formatting, reactions, search, and scroll anchors after resizing. Studio → Room activity contains compact activity rows and long topics. |
| Emoji | Studio → Emoji preview contains flags, skin tones, joined sequences, and text-presentation variants. Pickers insert glyphs; `:se` opens inline completion. |
| Images | Studio → Images has ready and failed fixtures. Escape closes the original viewer, preserving drafts and scroll positions. Originals also open from threads. |
| GIFs | Images open still. Play/Stop, once/duration settings, close, and focus loss control playback. Reopening stays still. |
| Attachments | Pick, paste, and drop stage one file per draft. The hover outline identifies the target; multiple files produce an error. Overlays block underlying drop targets. |
| Room header | The room name and search/People actions share a compact top row; the topic sits directly below. Room names and topics ellipsize at narrow widths. Click an editable topic to open its editor; empty editable topics offer Add a topic, while empty read-only topics leave no placeholder. Hover reveals the full topic and editing hint. Threads omit the generic subtitle. |
| Account utilities | The account avatar opens Settings, Find room, and device verification. Routine connection/device status lives in this menu; the normal bottom bar is hidden. Incoming verification requests, connection problems, settings/notification failures, media errors, and stopped-session Exit remain visible when applicable. An account attention dot also marks issues and graphics restart notices. Demo identity stays visible in the space rail. |
| Settings | The fixed header and footer keep Close, Cancel, and Save visible while the preferences body scrolls. Check 640×480 and ordinary desktop sizes: long descriptions, paths, and errors wrap within the card; controls use compact type and outlined fields. Save applies changes; Cancel/Close discards the dialog draft. Graphics changes show restart status. Demo settings stay in memory. |
| Control icons | Navigation, header, message actions, pins, and composer use shared static outline icons. Check consistent size/stroke, hover/disabled colors, recognizable pin/unpin and bookmark states, and action tooltips. User-authored emoji and reaction glyphs remain emoji. |

Space unread dots (2026-11-17): rendered-widget coverage checks room/DM updates,
zero-count manual unread flags, nested/shared/cyclic space membership, empty and
removed rooms, fixed icon/dot bounds, and clicking the dot without changing the
conversation, draft, or saved scroll. Settled redraws request `Wait`. Formatting,
all 310 desktop tests, workspace Clippy with warnings denied, and the locked
desktop build pass. Native desktop and connected-account acceptance remain
manual; no connected client was restarted.

Unread section (2026-11-17): rendered-widget coverage checks the fixed top header,
unread favorites/DMs/custom rooms, manual unread flags with zero message counts,
independent collapse, filtering, empty-section hiding, and Save/Defaults behavior.
State assertions retain the open thread, draft, and saved scroll position;
settings tests cover cancellation, defaults for older files, and persistence.
All 309 desktop and 168 library tests pass (two library tests remain opt-in), as
does workspace Clippy with warnings denied. Native desktop and connected-account
acceptance remain manual; no connected client was restarted.

Composer paging (2026-11-17): rendered key-event tests cover room/thread routing,
repeated paging, and preservation of draft text, caret, selection, and the other
conversation's saved scroll. State tests cover scroll limits, blocked input, and
stale keys/view generations/metrics; binding checks cover focus, modifiers, and
IME composition. Formatting, workspace Clippy with warnings denied, and all 308
desktop tests pass. Native keyboard acceptance remains manual; no connected
client was restarted. This adds no subscription, polling, or animation timer.

Tooltip styling was visually checked in an isolated headless Sway demo with
tiny-skia at 1280×900: account name, header search, message timestamp near the
right edge, and room-list toggle. macOS, wgpu, long topics, and multiline read
receipts remain manual acceptance checks.

Clickable usernames (2026-11-17): rendered-widget tests click sender names in
rooms, thread originals/replies, People, and pins. State tests cover exact user
IDs, repeated-chat reuse, draft/scroll/filter preservation, and rejection of
obsolete room/thread/member/pin snapshots, changed accounts, self IDs, and busy
operations. The existing DM flow owns asynchronous requests, errors, dismissal,
and shutdown. Demo senders now have synthetic Matrix IDs; clicks remain offline.
Formatting, workspace Clippy, all 303 UI tests, and the locked desktop build pass.
Connected-account acceptance and native desktop rendering remain untested for
this change.

Popup placement and surfaces (2026-11-17): the composer emoji picker starts at
its toolbar button and clamps to the window; reaction pickers retain their end
alignment and context menus retain their captured cursor position. Picker,
context-menu, tooltip and message-action surfaces use opaque backgrounds and
borders without shadows, following the Settings workaround for tiny-skia damage
artifacts. Automated coverage checks start/end alignment, translated anchors,
viewport clamping, outside-click capture and absence of scheduled redraws;
existing picker tests cover selection, disabled controls, stale targets and drafts.
All 274 UI tests, formatting, and workspace Clippy pass. Composer placement and
opaque surfaces were checked in the isolated Sway/tiny-skia demo at 1280×900 and
640×480. Reaction-picker/menu scrolling, macOS/wgpu, connected acceptance, and
fresh resource measurements remain separate manual checks.

The sidebar pass was visually checked in an isolated headless Sway/tiny-skia demo
at 1280×900 and 640×480: fixed-height room rows, long-name ellipsis and hover
tooltips, unread emphasis, hidden empty badges, collapsed section counts, and
hover-only drag handles. Dragging a collapsed custom section below Rooms retained
the open conversation. Automated checks cover Unicode grapheme boundaries,
resize/rename updates, row dimensions with unread badges, and the existing
reorder/cancellation behavior. macOS, wgpu, and native RTL rendering remain manual
acceptance checks. Label shaping is cached; tooltips use a single hover delay,
without recurring timers or background polling.

Navigator spacing (2026-11-17): room rows were reduced from 32 px plus a 2 px
gap to 26 px with no gap. Child-space rows, section headers, utility links, and
the gaps around the search field also use tighter vertical spacing; text sizes
are unchanged. The existing layout check confirms 26 px room bounds with long
names and unread badges. All 298 UI tests, formatting, and workspace Clippy pass.
This spacing change has not been visually checked on a desktop; in `--demo`,
check row alignment, badge clipping, hover/selection, and section drag handles
at ordinary and minimum window sizes.

The conversation layout pass was visually checked in the same isolated demo at
1280×900 and 640×480, including room messages, continuation-time hover, split
threads, and the narrow thread replacement. Automated UI checks cover adjacent
author/time bounds with long Unicode names, preserved body width for grouped
messages, the 840 px reading-column cap, and the full-width scroll viewport.
Existing selection, reactions, search, draft, and scroll-preservation tests also
pass. macOS and wgpu rendering remain manual acceptance checks.

The composer pass was visually checked in an isolated Sway/tiny-skia demo at
1280×900 and 640×480: empty/focused drafts, disabled/enabled Send, the format pill,
eye preview control, formatted preview, and room/thread layouts. Automated checks
cover editor focus tracking without recurring redraws, separate editor/toolbar hit
areas, focus retention when preview appears, and usable Send bounds with a long
preview and long thread original in a minimum-size pane. Existing draft, shortcut,
IME, emoji, paste, attachment, and cancellation checks pass. Attachment-chip native
rendering, macOS, wgpu, and connected-account acceptance remain manual checks.

Composer gap (2026-11-17): room and thread timelines now have 8 px of bottom
padding, followed by 4 px above the composer. The typing-status slot collapses
when empty, reducing the ordinary gap from 44 px to 12 px; active typing uses
an additional 18 px line. A stable outer container retains the editor widget
when that line changes height. Headless coverage checks both panes through
empty/active/empty typing updates, including timeline height, reading position,
editor focus, and draft preservation. All 299 UI tests, formatting, and workspace
Clippy pass. Desktop appearance remains unverified for this change; use `--demo`
to check spacing with typing, previews, and attachments at ordinary and minimum
window sizes.

The composer resize/scrollbar extension was checked in the isolated demo at
1280×900 and 640×480 with a long wrapped draft. Drag the centered top grip upward
to enlarge the editor or downward to shrink it; the toolbar remains below the
editor. The grip uses an open-hand cursor on hover and a closed hand while dragged. Heights are remembered per room/thread draft for the current session and
clamped to available pane space. Overflow shows a muted draggable scrollbar;
wheel scrolling and caret movement continue through the native editor. Automated
checks cover full-view grip routing, resize limits/deduplication/cancellation,
per-draft sizing, stale/modal rejection, wrapped overflow, scrollbar endpoints,
and preservation of text and selection. Offscreen paragraphs are lazily laid out,
so scrollbar proportions refine as paragraphs are visited. No timer or background
worker was added. Formatting, workspace Clippy, all 261 UI tests, and the desktop
build pass. Native macOS/wgpu and connected-account checks remain manual.

Room content now shares a centered 840 px column, including channel/thread
headers, pins, search controls, messages, typing status, formatting preview, and
the composer. **Settings → Appearance → Limit room content width** is enabled by
default; disabling it and saving uses the full pane width without restarting.
The timeline scrollbar stays at the panel edge. The same widget structure is
retained across the setting change. Automated checks cover centered header/editor
bounds, capped and unrestricted message widths, full-width scroll viewports,
Save/Cancel draft and scroll preservation, and settings file round trips. Visual
checks used the isolated Sway/tiny-skia demo at 1280×900 for both modes and
640×480 for the unrestricted layout. Formatting, workspace Clippy, all workspace
tests (159 library and 261 UI passed; two environment-dependent tests ignored),
and the desktop build pass. Connected-account and macOS/wgpu rendering were not
exercised.

Horizontal panel resizing was checked in an isolated Sway/tiny-skia demo at
1280×900 for the room list, thread pane, and People panel, and at 640×480 for
viewport clamping and the People replacement view. Dividers have 5 px hit areas
and horizontal resize cursors. Preferred widths stay in memory for the session:
room list 180–480 px, thread/People 300–640 px, with tighter limits when needed to
leave at least 440 px for a split conversation or 320 px for a single view.
Shrinking a window clamps displayed widths without overwriting preferred sizes;
thread and People widths are independent. The split/replacement breakpoint takes
the room-list width into account. Release, window exit/unfocus/resize, disabled
controls, and changed side-panel generations cancel the drag. Automated tests
exercise all three handles through full-view rebuilds, direction/limits, duplicate
motion, preserved drafts/saved scroll positions, and stale/modal rejection.
Formatting, workspace Clippy, all 263 UI tests, and the desktop build pass. No
polling or animation subscription was added. macOS/wgpu, connected-account
behavior, and production idle-resource measurements remain untested for this pass.

The utility-clutter pass moves routine Settings, Find room, and verification
actions into the account-avatar menu, with connection status in its heading.
Normal connected/demo views have no bottom status bar. Incoming verification
requests, connection problems, settings/notification errors, media errors, and
stopped-session Exit use a conditional bottom banner. The avatar shows an
attention dot for these states and graphics restart notices; the menu explains
the restart notice. Demo mode retains a small rail label. Existing shortcuts,
verification handling, and native dialog behavior are unchanged. Automated checks
cover menu dispatch/dismissal, stale modal actions, alert visibility/recovery,
draft/saved-scroll preservation, and the live timeline reading position as the
banner changes height. Visual checks used the isolated Sway/tiny-skia demo at
1280×900 and 640×480, including the avatar menu and Settings entry. Connected
alerts were exercised only as fixtures; real incoming requests, macOS/wgpu, and
production resource measurements remain manual acceptance checks. Formatting,
workspace Clippy, all 266 UI tests, and the desktop build pass. This pass adds
no subscription or timer.

On-demand presence (2026-11-17) replaces the extra classic sync stream with
per-user status lookups. This removes the shared-cursor collision that caused
repeated HTTP 400 responses for `extensions.to_device.since`, status flicker, and
high CPU. The temporary Matrix SDK patch and vendored source have been removed;
all SDK crates use their stock, pinned 0.19.1 releases.

Before restoring a saved account, a one-time migration runs under the existing
profile process lock. A native checkpoint saved by the temporary patch is copied
to the stock SDK cursor through its store API; otherwise only the old cursor is
removed so pending device events can replay. No device keys, identity, account,
or timelines are replaced. A small marker prevents later startups from resetting
native progress. Migration uses the already-pinned crypto store APIs, without
SQL modifications or an SDK fork. A malformed native checkpoint fails visibly
before sync starts and leaves the cursor unchanged.

Encrypted SQLite fixtures cover patched/unpatched cursors, malformed checkpoints,
idempotence, retained unrelated data and device signing keys, and reopening with
the stock SDK against a strict server expecting the native cursor. A separate
mock keeps `/versions` available while sync fails and then restores sync, checking
recovery during pending long polls. Connection status remains event-driven and
can be traced with `RUST_LOG=interconnect::connection=info` without account or
message content. The single-line connection banner retains full text on hover;
widget tests cover stable banner/timeline bounds at 640 and 1280 px.

Formatting, workspace Clippy with warnings denied, all 172 library and 314 desktop
tests (two existing opt-in library tests skipped), and the locked desktop build
pass. The authorized connected launch completed cursor migration and restored the
existing account with the stock SDK: Connecting → Connected, with no sync errors
through an 85.01-second passive sample. The store migration marker was present.
No test messages, new login, or device/key reset was performed. Native visual
acceptance of the refresh controls and other platforms remains manual; widgets
and lookup behavior were verified in fixtures. Logs remain outside the repository.

The compact header pass reduces vertical padding and aligns room actions with
the title row. The topic uses a cached single-line ellipsis label, with its full
text in a tooltip. An editable topic is itself the edit control, changing color
on hover; the detached pencil is removed. Empty editable rooms offer **Add a
topic…**; empty read-only topics are hidden. Room names ellipsize instead of
wrapping, and the thread header omits its generic subtitle. Visual checks used
an isolated Sway/tiny-skia demo at 1280×900 and 640×480, including opening and
canceling the inline topic editor and a long topic in the narrow layout.
Automated topic checks cover narrow click routing, editing permissions,
stale completion rejection, and draft/scroll preservation. Formatting, workspace
Clippy, all 266 UI tests, and the desktop build pass. Native macOS/wgpu and
connected topic edits remain untested for this pass.

Shared form controls now use the same rounded, outlined input/select treatment
and rounded primary actions as Settings. Room creation, people/invite search,
global/in-room search, emoji search/management, and verification use compact
13 px control text and consistent field padding. Settings reuses the shared
select/primary styles. This is presentation only: request, disabled-state,
permission, IME, and cancellation handling are retained. Formatting, workspace
Clippy, and the full UI test suite pass. The room-creation form was visually
checked in the isolated Sway/tiny-skia demo at 1280×900 and 640×480. Other native
forms, macOS/wgpu, and connected acceptance remain untested for this extension;
no connected account was used.

Emoji pack management now opens as one visible dialog. When launched from
Settings, the Settings widget tree remains mounted but skips drawing, input,
operations, and overlays; closing management restores its unsaved draft and
scroll position. Pack browsing is separate from explicit Rename, New pack, and
Add/Edit emoji forms. Header and actions stay fixed while the body scrolls.
Removal replaces the editor with a scoped explanation and a visible Keep/Remove
footer. Canceling an editor drops read/preparation tasks and rejects late file
callbacks; submitted writes retain their existing shutdown handling. Regression
checks cover each editor's minimum-window action bounds, read cancellation,
confirmation isolation, Settings draft/scroll retention, and suspended-layer
input/draw/overlay suppression. All 270 UI tests, formatting, and workspace
Clippy pass. Browse, Add Emoji, and removal confirmation were visually checked
in the isolated Sway/tiny-skia demo at 1280×900 and 640×480. Connected acceptance
and macOS/wgpu rendering remain untested for this extension.

Search and Saved messages now use the same centered reading column as room
content, respecting **Appearance → Limit room content width**. Their scrollbars
remain at the panel edge in either width mode. Results use light outlined rows
instead of filled gray blocks, with room, sender, and date separated from message
text. Search navigation uses a compact, tooltip-labelled open control; bookmark
navigation/removal and result text selection/highlighting retain their actions.
Automated layout coverage checks both pages at narrow and wide widths, restriction
on/off, viewport width, and real search-result click routing. Existing async,
stale-update, pagination, bookmark, draft/scroll, and idle tests remain in place.
Formatting, workspace Clippy, and all 271 UI tests pass. Search results and empty
Saved messages were checked in the isolated Sway/tiny-skia demo at 1280×900 and
640×480. Connected acceptance and macOS/wgpu rendering remain untested for this
extension.

The room list now collapses automatically below 800 px window width, leaving
only the 64 px space rail beside the conversation. **Show room list** or **Find
room** temporarily reveals it in a narrow window; choosing a room collapses it
again. Filtering by space does not switch the conversation or dismiss the list.
Returning to a wide window restores normal visibility and the preferred drag
width. Automatic collapse does not edit startup settings or the normal visibility
preference. Hidden navigation and its divider retain stable widget slots but do
not draw or receive input/operations/overlays; the side-panel replacement decision
uses effective displayed navigation width. Automated coverage checks temporary
visibility, search reveal, room/space behavior, and retained live timeline scroll,
editor focus, header action bounds, and unsent drafts through responsive and
manual transitions. All 273 UI tests, formatting, and workspace Clippy pass.
Native rendering and connected acceptance for this extension are recorded
separately.

The compact pins, shared control icons, and Settings layout were visually checked
in an isolated headless Sway/tiny-skia demo at 1280×900, 800×700, and 640×480.
Checks included expanding/collapsing pins, message-action icons, and scrolling
Settings to the bottom while Close/Cancel/Save remain visible. The card uses a
border rather than a large shadow: tiny-skia 0.14 shadows paint outside damage
regions and darken the card during partial redraws. Automated UI coverage checks
disclosure scope, stale/modal rejection, timeline scroll preservation, bounded
long pins, caption wrapping, and minimum-size action visibility. Native macOS,
wgpu, and connected-account acceptance remain untested for this visual pass.

Pinned preview formatting has automated coverage for rich-text rendering, link
activation, and concealed spoilers. Native visual acceptance of formatted pins
remains a manual check.

The selected room and bookmark source rooms share one complete sliding-sync
subscription set. A mock homeserver regression checks that the pin count and
previews advance from one to two to three while bookmark subscriptions are added
and removed. In connected acceptance, pin another message in the open room and
verify both the count and expanded list update after sync; repeat with Saved
messages open/closed and pin changes from another client. This regression has not
been validated against a saved account.

Room composer focus (2026-11-17): opening a room from the sidebar, its context
menu, or Ctrl+P focuses the composer. Focus waits for the selected room's
SDK pane instead of running while only the opening placeholder is mounted.
Headless regressions cover delayed pane arrival, a single focus request, typing
into the resulting composer, and cancellation after navigation, dialogs, window
focus loss, or a failed pane. In connected acceptance, use Ctrl+P and Enter/click
to open an unloaded room, then type without clicking the composer. Repeat by
clicking a sidebar room or choosing **Open room** from its context menu, and while
quickly switching elsewhere or opening Settings during loading. Native desktop
acceptance has not been repeated for this change.

Ctrl+, opens Settings, Ctrl+K focuses the sidebar room filter, Alt+Up/Down selects adjacent
filtered rooms, and Escape clears search or closes a thread. macOS uses Command
for the primary shortcuts. Ctrl+Enter sends by default; Enter inserts a newline.
With Enter-to-send enabled, Shift+Enter inserts a newline. IME composition
suppresses sending and navigation shortcuts. Full button keyboard traversal and
focus restoration remain unfinished.

Composer word editing (2026-11-17): Ctrl+Left/Right moves by word,
Ctrl+Shift+Left/Right extends the selection, and Ctrl+Backspace/Delete removes
text to the previous/next word boundary. macOS uses Option for these actions.
An existing selection is deleted as-is. Check both room and thread composers,
including punctuation, accented/combining text, empty drafts, and crossing line
breaks; deletion at a document edge should leave the draft unchanged. Word
shortcuts yield during IME composition. Headless focused-widget regressions cover
these actions, draft/scroll isolation, and RTL document edges. Native keyboard
delivery and macOS acceptance remain manual checks; no connected client was used.

In-room search has automated coverage for matching, navigation, stale snapshots,
draft preservation, room changes, and modal shortcut blocking. Native rendering,
keyboard focus, and connected-history acceptance require the manual checks above.

Global search combines the homeserver's message-body search (newest first, 30
results per page) with a memory-only snapshot of recently loaded messages. The
local cache retains at most 32 room/thread panes and 500 recent messages per pane;
results are bounded to 200 and deduplicated by room/event ID. The initial query
searches encrypted history only within that local coverage. After submitting,
**Search older encrypted messages** loads history without opening the rooms.
Each click requests up to 20 pages of 50 events, rotating between encrypted
rooms (including rooms with unknown encryption state); bookmark storage is
excluded. The SDK decrypts pages using available keys. Progress counts checked
events, finished rooms, events missing keys, and unavailable rooms. Stop cancels
the current page; Continue resumes from the last finished page. Empty pages
with a new token remain resumable, while repeated/cyclic tokens stop pagination.
There is no plaintext search index or history-scan file. New queries reset the
scan; leaving the panel cancels work. Matching edits are applied to their
original event when found while scanning backwards; forged edits from another
sender and redacted/unreadable events are not used as message matches. Search runs on explicit submit,
with cancellable work and no background indexing or polling. Demo searches all
sample room histories without an account or network. Server search terms use
the homeserver's matching rules; yellow previews highlight literal,
case-insensitive occurrences of the submitted phrase.

Automated coverage includes the search endpoint's room filter, pagination,
unavailable/unjoined events, Unicode local matching, stale results, modal guards,
view navigation, and preservation of drafts/scroll. Native minimum-size layout,
button/focus behavior, encrypted-room coverage, and live homeserver acceptance
remain manual checks. For connected validation, use dedicated test rooms: submit
a distinctive existing phrase in two rooms, load the next page, follow both a
room hit and a thread reply, then confirm the original reading positions and
drafts. Repeat with an encrypted message loaded during this session and with
server search unavailable; the panel must communicate its partial coverage.
Then search for an older encrypted message in a room not opened in this session:
click **Search older encrypted messages**, check incremental matches and progress,
stop during a request, resume, and follow a reply. Check a room without historical
keys and an inaccessible room; both must be counted without preventing other
rooms from being searched. Finish one batch with more history remaining and
verify no further page is fetched until Continue. Change the query, close the
panel, and change accounts during scanning to check stale-page rejection.
Mock-homeserver tests cover ciphertext decryption, empty-page continuation,
missing keys, cyclic tokens, failures, and historical edits. UI tests cover
merging, stop/query/account/close rejection, and demo isolation; the connected
acceptance steps above remain untested until run with a test account.

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

Unread stays first when enabled and nonempty, followed by Favorites; Low priority
stays last. Unread duplicates matching rooms from their usual sections without
changing membership. Explicit tags win over inferred Direct messages/Rooms
grouping; multiple tags use the first displayed explicit section. Element's
`chats` order slot positions Rooms, and `im.vector.fake.direct` positions Direct
messages. Empty custom sections appear in their creation scope using Element's `spaceId` (`home-space`
for All rooms/Direct messages); legacy sections without a scope default there.
Search hides empty sections. Nonempty sections follow room membership in the
selected space regardless of their creation scope.

Right-click any joined room → **Move to section…** to move it to Favorites, a
custom section (including an empty one), Low priority, or Default. Default clears
explicit section tags, returning to inferred Direct messages/Rooms
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
Hover a movable section header to reveal its six-dot handle at the right, then
drag it to reorder the section. Handles also remain visible during a drag; their
space stays reserved so labels do not shift when they appear. The blue
line marks insertion above/below the target header. Favorites and Low priority
stay pinned, with Unread fixed above Favorites. Direct messages moves independently;
Rooms uses Element's `chats` order slot. Order is account-wide, including sections hidden
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
   Verify the insertion line, pinned endpoints, and that Unread cannot be moved.
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
| Received attachments | Open externally launches the system handler; Save as uses a native dialog. Images, files, audio, and video download originals, including SDK decryption, in room/thread messages and thread originals. Image previews also retain their in-app viewer. |
| Reactions | Chips toggle reactions; the emoji picker adds a selection. Pending/error state belongs to its captured message, with stale results rejected after navigation. |
| Clipboard | Native file lists and images stage attachments; plain-text paths remain text. Multiple files leave the previous draft intact with an error. Host-selected paths need matching container access. |
| File drops | Only the visible conversation under the pointer stages the file. Sidebar, disabled panes, and overlays are excluded. Wayland delivery and macOS coordinates have separate native coverage; see [the Winit patch](../architecture/winit-file-drops.md). |
| Direct chats | People search accepts names and full Matrix IDs. Click another user's name in messages, thread originals/replies, pins, or People to open their chat directly. Check both an existing one-to-one DM and a peer without one; existing DMs are reused, and new chats invite the peer and enable encryption. Errors appear in the chat picker. Dismissal during creation prevents late navigation. |
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
code block into either composer. Choose the **Preview message formatting** eye
icon to inspect formatting, then
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

Presentation refinement (November 17, 2026): received attachments use a bordered
card containing filename/type metadata, a thumbnail for image messages, and
**Open externally** / **Save as…** controls with preparation/result feedback.
Captions remain selectable message content; filename-only bodies appear in the
card. Filenames ellipsize to one line, with the complete filename on hover.
Thread originals use a compact **View image** control inside the same card.
The image viewer uses the attachment filename (or **Image** without filename
metadata), a persistent Close header, and one grouped action/playback toolbar.
GIF playback still starts only after an explicit click and has no idle timer.

Automated presentation coverage checks a long Unicode filename at a 220 px card
width, Save as control routing, a long viewer title and reachable Close at
640×480, plus existing playback, stale-download, cancellation and scroll tests.
All 278 UI tests, formatting, and workspace Clippy pass. Attachment cards and
the in-app GIF viewer were visually checked in the isolated Sway/tiny-skia demo
at 1280×900 and 640×480. External application launching, native save dialogs,
connected acceptance, and macOS/wgpu remain separate manual checks.

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

## Message bookmarks — 2026-11-15

The outline/filled bookmark action saves/removes a private bookmark from room timelines,
thread replies and the thread original. **Saved messages** above the sidebar
sections opens an account-wide view. Back/Escape restores the open conversation;
space changes still only filter navigation. The view shows 20 live source
previews per page, allows removal even when the source is unavailable, and opens
the source room/thread. **Go to message** uses the regular live room/thread
timeline, loads back to the saved event and centers it once after history settles.
Newer history remains below it and live updates continue. **Jump to newest**
returns to the latest messages.

See [bookmark protocol and lifecycle](../bookmarks.md) for experimental MSC4482
wire names, extensible-text fallback limits, storage recovery, and write/retry
semantics. No Matrix SDK or Ruma fork, vendoring, or dependency upgrade is used.

Automated evidence:

- A local mock homeserver exercises initial read-only discovery, encrypted
  invite-only room creation, waiting for synced metadata, and publishing the
  global account-data pointer. Cold discovery waits for a room outside the
  initial list range instead of creating a replacement.
- An actual SDK encrypted send is decrypted in the fixture to check the custom
  event/pointer and Matrix URI fallback. Source text is absent from the fallback.
  A failed send retries with the same transaction ID; acknowledged saves do not
  duplicate before their sync echo. Removal redacts every known duplicate.
- Paginated cache hydration resolves bookmark replacements, rejects other
  senders' edits, follows raw redactions, and cancels without late model updates.
  Unknown history is not exposed as unbookmarked. An active account-data room
  switch cannot send old bookmark IDs to the new room.
- Source preview timelines follow actual message edits and redactions. Dropping
  previews adds no idle requests. Initial empty discovery settles without room
  creation or periodic HTTP requests. Cold previews wait for source-room metadata
  outside the initial list range.
- UI tests click the real hover control, retain drafts and saved scroll offsets,
  keep space filtering separate from the saved view, reject obsolete actions and
  preview results, and wait for writes during shutdown. The saved view is laid
  out and CPU-rasterized at 1000×720 and 360×480, with settled redraws requesting
  `Wait`. Local inspection captures were written only under `/tmp`.

Validation passed: formatting, locked workspace Clippy with warnings denied,
locked workspace tests (102 library tests and 165 GUI tests passed; two existing
opt-in library fixtures ignored), and `cargo build --locked --bin interconnect`.

No native desktop or saved-account session was launched. Native fonts/rendering,
real homeserver interoperability, simultaneous first creation on two devices,
room-upgrade recovery, restart with real historical keys, and measured idle
resources remain acceptance work.

Manual acceptance:

1. Use `cargo run --locked --bin interconnect -- --demo`. Bookmark room messages,
   a thread reply and its original, then open Saved messages. Remove from both
   the view and the message action panel. Confirm room/thread drafts and reading
   positions survive Back, Escape and sidebar navigation.
   Check that the sidebar count follows saves/removals, includes bookmarks from
   other spaces and pages, and reaches zero when all saved messages are removed.
   In connected mode the count stays hidden until bookmark history is complete.
   Use **Go to message** on an older room message and thread reply. Confirm the
   saved event is centered where surrounding history permits, scroll both up and
   down past it to the latest messages, and check new incoming events remain live.
   Confirm manual scrolling is retained on redraw, drafts survive, and **Jump to
   newest** reaches the latest messages. Navigate away during loading
   and check late results cannot change the new conversation.
2. In an independently authorized connected session, bookmark an existing
   message. Check that exactly one private encrypted Bookmarks room is created
   lazily and that its account-data pointer survives restart. No test message
   needs to be sent to an ordinary room.
3. Inspect the room in another client. A client supporting extensible text should
   show a readable fallback with a working Matrix URI; a legacy client may show
   an unsupported event. Verify history on another own device, including keys.
4. With a compatible second client, add/remove/edit bookmarks, including an older
   one outside the initial history window. Check binary action state, duplicate
   removal, paging, and updates while Saved messages is closed and reopened.
5. Edit/redact a source message and inspect its open preview. Test inaccessible
   sources, missing keys and disconnected refresh. Check error feedback, retained
   drafts, same-operation retry and shutdown while a write is pending.

Rebase integration on November 16, 2026: formatting, locked workspace Clippy
with warnings denied, locked workspace tests (144 library and 219 desktop tests
passed; two existing opt-in library fixtures ignored), and the locked desktop
build passed against current main. The integration preserves custom section
controls and message context menus; a regression checks that Saved messages
dismisses conversation menus and rejects stale message menus while keeping room
sidebar menus available. No native desktop or connected session was launched;
the manual acceptance items above remain pending.

Saved-event navigation follow-up: the library suite passed 145 tests (two
existing opt-in fixtures ignored), and the final desktop suite passed all 221
tests. Formatting, locked workspace Clippy with warnings denied, and the locked
desktop build passed. Mock-server coverage loads room/thread targets outside
live history and cancels their views. Widget coverage waits for asynchronous
rows, jumps once, preserves manual scrolling, and resets on new navigation;
demo state coverage retains drafts and reading offsets and returns to live mode.
Native desktop and connected-account acceptance remain pending. No connected
client was restarted or Matrix test messages sent.

Regular-timeline correction: saved-event navigation now backfills the ordinary
live room/thread timeline and centers the target after loading settles. The
mock-server regression checks older and newer context, retained newest history,
and a subsequent incoming sync event. An actual room-pane layout regression
checks centering and scrolling from the saved event down to the newest message;
widget coverage also checks scrolling in both directions after the jump.
Formatting, locked workspace Clippy with warnings denied, the locked desktop
build, and workspace tests passed (145 library and 222 desktop tests; two
existing opt-in fixtures ignored). Native desktop and connected-account visual
acceptance remain pending. No connected client was restarted or messages sent.

## Custom emoji — 2026-11-16

Automated checks cover pack/usage validation, stable state overriding legacy packs
(including empty removals), the narrow SDK state subscription, live state arrival,
worker cancellation without media fetching, escaped payloads, code/raw-HTML
fallbacks, underscore shortcodes, source preservation, duplicate pack choices,
stale completion/picker actions, per-conversation bindings, MXC reaction keys and
picker demand release. Existing room/thread sending, encryption, selection and
renderer tests also run. These are local fixtures, not connected-account results.

Manual checks (not yet performed for this change):

- Build, then launch `cargo run --locked --bin interconnect -- --demo`. In
  **Emoji preview**, inspect the custom inline images, missing image fallback and
  image reaction. Choose either `wave` from the two demo packs via the composer
  picker and `:wa` completion. Open Preview; verify the selected image and fallback.
- Choose both packs in the same room/thread draft. Verify different tokens and
  images, keyboard/mouse selection, Plain text mode, code literals and navigating
  away/back. Space filtering must leave the open conversation and draft intact.
- Verify the 640×480 window and narrow thread pane, search/category/page changes,
  failed image placeholders, copy alternatives and settled redraw behavior.
- With an authorized connected session, use existing personal, global, room and
  canonical-space packs. Verify both stable and legacy packs and a pack update
  from another client. Confirm leaving a room/replacing an account cannot expose
  the previous room/account catalog. Unknown global rooms should be skipped.
- Send only with explicit authorization. Inspect inline HTML, plain token fallback,
  room/thread relations, encrypted queue/retry behavior and reactions in another
  client. SDK 0.19.1 removes the emoticon marker when sanitizing received/local-echo
  HTML, which leaves normal inline-image dimensions instead of the local 24 px
  override. GIF emoji intentionally stay static. Pack management/stickers are
  outside this release.

See [formatting architecture](../architecture/message-formatting.md#custom-emoji)
for subscription/media bounds and the fact that pack media is unencrypted.

Validation performed for this change: `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace` (153 library and 244 UI tests passed; two
existing environment-dependent tests ignored), and
`cargo build --locked --bin interconnect`. A separate
`cargo run --locked --bin interconnect -- --demo` instance started successfully
on Wayland/wgpu and was terminated after a short startup smoke check. This did
not exercise visual interactions, a connected account, or idle resource metrics.

## Emoji pack management — 2026-11-16

Automated mock-server coverage exercises personal and room pack creation,
renaming/deletion, fresh-state conflicts, permission loss, stable-over-legacy
conflicts, preserving unknown/sticker metadata, global reference edits and
malformed global data. Local-file fixtures check size/type/decoding constraints,
preview preparation without uploads, and upload/save payloads. UI fixtures cover
demo creation/edit/removal/projection, confirmation cancellation, stale loads,
account replacement, duplicate save callbacks, earlier reaction completions,
draft/scroll preservation and 640×480 / 1280×900 layout without redraw timers.
These are isolated fixtures; no connected-account changes are performed.

Manual acceptance (not yet performed):

- Launch `cargo run --locked --bin interconnect -- --demo`. Open **Manage packs…**
  from both emoji pickers and Settings. Create a room pack, add/rename/remove an
  emoji, enable a room pack globally and navigate away/back. Inspect the picker
  and completion after each saved change; local drafts/scroll must survive.
- Try valid and invalid shortcodes, duplicate names, invalid MXC URLs, oversized
  and damaged files, canceling the native file chooser, Reload and modal Close.
  Inspect remote/local previews and all controls in the minimum window.
- With an authorized connected session, verify personal pack persistence across
  clients, room permission denial/demotion, stable and legacy edits, global
  enable/disable, and a concurrent edit from another client. Perform uploads or
  destructive changes only on explicitly authorized packs. Check timeout/reload
  behavior and that existing messages/drafts still display removed pack images.
- Close the app during a submitted save and confirm bounded shutdown; switch
  accounts during a load and reject its late result. Compare connected idle
  resources with the earlier emoji baseline. Management adds no periodic worker.

Bulk import/export, deleting media from the server, standalone stickers, live
interoperability and native macOS behavior remain outside this validation.

Validation performed for this extension: `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace`, subsequent targeted management/shutdown
regressions, and `cargo build --locked --bin interconnect` all passed. Two
existing environment-dependent library tests remain ignored. Desktop/manual,
connected-account interoperability and production resource measurements were
not performed; the user's connected client was not restarted or used for tests.

## Message editing — 2026-11-17

Room messages, thread originals, and thread replies offer a pencil **Edit message**
button in their hover action popover and an **Edit message** context-menu action
for own text messages and emotes that have an event ID. Up in an empty focused
composer selects the latest eligible own message; room Up skips thread replies.
Thread Up selects own replies; thread originals remain editable through their
hover action popover or context menu. Nonempty drafts, whitespace-only drafts, selection shortcuts, and
IME composition retain normal editor behavior. Attachments, notices, redactions,
and unsent messages without an event ID have no editing action.

Editing uses the existing composer with an **Editing message** banner,
**Save**, and **Cancel**. The configured send shortcut saves; Escape while the
editor has focus cancels. An earlier draft, staged attachment, authoring mode,
cursor, and composer height return after cancellation or queue admission.
Each conversation keeps its own edit across room/thread navigation, while space
filtering leaves the conversation open. Once saving starts, the captured edit
finishes against its original account/room; Cancel is disabled until admission
completes. Formatting/admission failures retain the edit and show an error.
Delivery after admission uses the SDK's existing send queue.

Plain messages start in Plain text mode. Formatted messages start from their
plain fallback in Markdown mode; Markdown authored by this client, emote type,
existing mentions, and custom emoji shortcode/image bindings are retained.
Arbitrary HTML formatting from other clients cannot be round-tripped through
this source editor. Review its preview before saving such a message. Edited
rows show a quiet **edited** label without changing event identity or timestamp.
Editing adds no polling subscription or animation timer.

Edit-composer spacing (2026-11-17): the edit header uses compact Cancel padding
and replaces the editor’s usual top inset. The editing resize gutter is 4 px,
and the header has no bottom inset, reducing the header-to-text gap from 14 px
to 6 px. The scrollbar rail and visible-height calculation use the same padding
and gutter as the native editor. A two-line Markdown regression preserves the
exact source, limits the gap to 6 px, and verifies that both lines fit without
overflow. Formatting, workspace Clippy, all 291 UI tests, and the locked desktop
build passed. An 800×600 software widget render was visually checked; the demo
compositor constrained its window to 211 px wide, so it could not verify the
intended desktop layout. Native macOS and connected-account checks remain open.

Automated coverage includes offline replacement payloads (`m.replace` and
`m.new_content`), emote type, mentions, thread identity, incoming edit projection,
redaction rejection, plain/Markdown authoring, custom emoji after SDK sanitizing,
draft/attachment/cursor restoration, navigation, errors, and stale account/edit
completions. Layout tests cover Save/Cancel placement at 640×480 and 1240×820 and
settled redraws returning `Wait`. On 2026-11-17, formatting, workspace Clippy with
warnings denied, workspace tests (161 library tests passed, two ignored), the
final desktop suite (290 passed), and the locked desktop build passed. A Wayland
software-renderer demo capture confirmed the edit banner and controls at the
compositor-provided 636×1162 viewport; requested window sizes were overridden.
The capture harness and images stayed outside the final repository changes.
Live-account editing, encrypted interoperability, native macOS, and manual
pointer/keyboard acceptance remain untested. No connected client was restarted
and no real Matrix test events were sent.

For manual acceptance in `--demo`, edit a Jamie Davis message in Design studio
using the hover popover’s pencil button and the context menu, first with a
nonempty draft and staged attachment. Cancel and verify the draft
returns. Save a multiline Markdown change, open its thread, and confirm the same
body appears in the original without an extra reply. Send a local thread reply,
then edit it with Up and with the context menu. Navigate to another room and
back during editing; switch spaces; verify each composer and reading position.
Repeat at 640×480 and check Save/Cancel and preview placement. Connected
acceptance with an authorized test account should confirm encrypted edits in
another client, offline queue delivery, server refusal/retry, and simultaneous
edits/redactions from another device. No real Matrix events are needed for demo
or automated fixture checks.

## Message deletion — 2026-11-17

The message hover action popover has a trash button, and the context menu offers
**Delete message…**. Both open the same captured confirmation popup for room
messages, thread originals, and thread replies, including attachments and other
message-like events. Room power levels determine whether own messages and other
senders’ messages can be deleted. State/activity events, already redacted rows,
and unsent local echoes without event IDs have no delete action.

Opening the popup sends nothing. **Delete message** or an unmodified,
non-repeated Enter key confirms the displayed request. Escape or an outside click
cancels without activating the background; normal composer shortcuts are
suppressed while the popup owns input. A pending request can be dismissed with
**Close** and continues against its captured account/room/event. Pending requests
count toward graceful shutdown. Failures stay in the open popup for explicit
retry, retaining the transaction ID; stale confirmations and late results cannot
confirm or replace a newer popup. Navigation, hidden panes, account changes,
permission loss, and remote redactions invalidate stale confirmations.

The runtime-owned composer reads current SDK membership and power levels,
rechecks the target, and awaits the homeserver redaction response with a 30-second
bound. No content is sent in the redaction body. A timeout may have succeeded;
the error asks the user to check before retrying. There is no automatic
application retry, deletion worker, idle poll, or animation timer.

The SDK sync projection supplies **Message removed** and releases the row’s
formatted document, attachment controls, and reaction actions. Event identity and
room placeholders preserve reading anchors. SDK 0.19.1 can remove a redacted reply
from its thread view because redaction strips its relation; the room placeholder
remains. Deleting a thread original leaves the other replies intact. Existing
composer drafts, staged attachments, and scroll positions are retained. If a
message being edited is deleted, its previous draft returns once any pending edit
finishes. Search caches and saved-message preview text are invalidated after a
successful deletion; bookmark/pin records themselves remain unchanged.

Offline protocol tests cover own-message permissions, moderator permissions,
permission changes, departure, plaintext redaction requests in an encrypted room,
server refusal, stable-transaction retry, and room/thread sync updates. UI tests
cover actual trash/confirmation click routing, popup clamping, cancellation,
unchanged drafts/scroll, deleted media, original/reply copies, edits invalidated
by local/remote deletion, stale account/request results, and shutdown while a
redaction is pending. Workspace tests passed (163 library tests, two existing
ignored checks); the final desktop suite passed all 298 tests. Formatting,
workspace Clippy with warnings denied, and the locked desktop build passed. A
360×480 software widget render was inspected for popup placement and an opaque
surface; temporary render artifacts stayed outside the repository. Native
appearance and connected-account acceptance remain separate checks. No real
Matrix messages were deleted and the connected client was not restarted.

For manual acceptance, use `--demo`: delete a Jamie Davis message from each entry
point, cancel first, and repeat on the original and a reply in a thread. Check
that another sender’s message has no delete action under the ordinary demo role.
In Images, delete Jamie’s attachment and verify its media/download controls go
away. Keep drafts in both composers and a scrolled reading position throughout.
Repeat at 640×480. With an explicitly authorized test account, verify redaction
in another client, revoked permissions, a server refusal, timeout/retry behavior,
and deletion while an edited message is open. Demo and fixture checks require no
saved account, network access, or real Matrix test messages.

## User presence

Opening a DM requests its unambiguous Matrix peer's current status. Opening People,
changing its search, or paging requests the visible joined members, excluding
self and invitees. Each demand is capped at 50 users with four concurrent requests
and a five-second per-request timeout. Responses use the standard
[per-user presence endpoint](https://spec.matrix.org/v1.19/client-server-api/#get_matrixclientv3presenceuseridstatus)
through the stock SDK. Presence never calls `/sync` or changes encryption cursors.

Results are last-checked snapshots, not continuous live presence. Opening another
view reuses results checked within 60 seconds. Cache age is checked only on view
demand; no timer expires entries or polls the server. **Refresh status** in a DM
header or People panel bypasses the cache for the current view; **Checking…**
disables duplicate refreshes. A failed or unsupported lookup hides that user's
mark and is cached as unknown until another eligible lookup or explicit refresh.
The memory cache retains at most 8192 users and is released with the account.

Online/away/offline marks retain their existing colors and dimensions. Tooltips
say **Last checked**; People explains the snapshot behavior. Unknown status has
no mark and is never presented as Offline. Room/thread avatars and DM navigator
rows can reuse previously checked users; opening a group conversation alone does
not request every sender. Space filtering preserves the open conversation and
its demand. Modal dialogs cancel demand. Demo fixtures stay in memory and issue
no presence requests, including when Refresh is clicked.

A superseding room, People view, page, or refresh cancels its pending batch.
Responses are checked against current demand before entering the cache, and a
session owns its own worker/cache. Lookup completion cannot replace conversation
selection, drafts, reading positions, or another account's data. Connection status
is independent of presence lookup failures. The existing SDK default for our own
presence remains unchanged.

Fixtures cover cache reuse, manual refresh, unknown/failed responses, bounded and
validated user demand, no idle/classic-sync requests, navigation/shutdown
cancellation, and rejection of late results. UI tests cover visible member pages,
self/invite exclusion, stale refresh controls, modal/draft/scroll preservation,
click routing, and demo isolation. Existing avatar tests cover fixed bounds and
idle redraw behavior. Cursor migration and stock-SDK reopening are covered above.

For manual acceptance, open a DM and People at ordinary and 640×480 window sizes.
Hover a status mark, change a test peer's status in another client, then click
Refresh and check the new snapshot. Navigate away during a pending request; check
that the new view's statuses and drafts stay intact. Test unsupported presence,
People search/paging, account replacement, and shutdown while a lookup is pending.
Use dedicated authorized test accounts for encrypted message/verification checks;
no test sends or new logins are necessary for ordinary demo/fixture validation.
