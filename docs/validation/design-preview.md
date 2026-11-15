# Iced design preview

The next feasibility slice tests whether a restrained desktop interface is
practical in Iced. The direction is a light, neutral surface, a tinted space rail
and sidebar, bundled Inter typography, small blue accents, and generous message
spacing. Native window management stays with the desktop. The visual direction
is intended to suit Linux and a future macOS port; this is not macOS platform
validation (the application still has Linux-specific integration).

## Run

```sh
cargo run --locked --bin matrix-ui -- --demo
```

Demo mode never starts the SDK connection, opens a profile, or sends network
messages. Messages and drafts exist only in memory and disappear on exit. It
uses the same shell, timeline presentation, and composers as the connected app.
Running without `--demo` still restores the real saved session and sends real
messages; see [connected UI validation](connected-ui.md).

**Studio → Emoji preview** contains faces, hearts, flags, skin tones, keycaps,
joined profession/family sequences, and explicit text-presentation examples.
Open the first message's thread to compare its pinned original and replies.
Select/copy the wrapping example and paste it into each composer. Check at
narrow widths with both renderers: recognized emoji should be colored while
VS15 text-presentation examples stay text, ordinary digits/punctuation stay in
Inter, and copying preserves the original Unicode sequences. Thread excerpts
use the same emoji fonts and truncate at grapheme boundaries.

**Studio → Images** contains a ready preview, a failed-preview example, and a
click-to-play animation fixture. Open
each image, close it with Escape or **Close**, and check the viewer at narrow
window sizes. Open the first image's thread and use the pinned original's
**Open image** action. Compare the fixed preview space in ready, loading, and
failed states; drafts and reading positions should survive opening and closing
the viewer. The ready image is a deterministic pixel fixture generated once at
startup; no image files, SDK clients, saved profiles, or network requests are used.
The animation fixture also uses generated in-memory frames. Open it and confirm
it stays still until **Play GIF** or an image click. Check **Stop**, replay from
the beginning, and automatic stopping after one loop. Close the viewer, choose
**Settings → GIF playback → For a duration**, enter a duration, and save. Reopen
the fixture and verify it loops for the chosen time only after Play. Escape,
Close, and switching window focus stop playback; reopening/regaining focus must
stay still. Repeat in its thread, at 640×480, and with both renderers. Settings
remain in memory in demo mode. The fixture tests playback/UI behavior; decoder
and encrypted download coverage uses generated GIFs in automated tests.

The default launch tries wgpu with a low-power adapter preference and software
fallback. Repeat the image/scroll/selection checks with
`ICED_BACKEND=tiny-skia cargo run --locked --bin matrix-ui -- --demo` to force
the CPU renderer, and `ICED_BACKEND=wgpu` to require wgpu. See
[renderer selection](../development.md#renderer-selection) for adapter logs and
overrides. Check that the selected wgpu adapter is a hardware device before
attributing performance or battery differences to GPU acceleration.

The rail contains All rooms, Studio, Community, and Home. Each sample space has
Favorites, Unread, Rooms, and/or Direct messages sections. Click section headings
to collapse them. Unread badges and membership are fixtures, not Matrix state.
Connected mode uses the same projection with real joined spaces and SDK room
metadata; see [connected navigation](connected-ui.md#connected-navigation--2026-11-15).

Search filters the selected space; Enter opens its first match and focuses the
room composer. Search temporarily expands matching sections. The rail's bottom
menu button toggles the room list. At narrower widths, an open thread replaces
the room timeline; at wider widths it sits beside it. Thread and room drafts
remain separate across these transitions and navigation.

The room list uses compact spacing: single-line room rows are about 31 pixels
apart (previously about 44), with the same 13-pixel room labels. Section headings,
child-space buttons, and search padding are tighter too. The sidebar heading has
no subtitle or workspace footer label, leaving more height for rooms. Long room
names can still wrap.

Room and thread composers share a compact 80-pixel frame, with a small send
arrow at the bottom right. The bottom action row stays clear of editable text
and leaves space for future controls. The room/reply label and shortcut footer
are removed; hover over the arrow for **Send message (Ctrl+Enter)** or
**Send reply (Ctrl+Enter)**. The arrow becomes an ellipsis while queueing.

Hover over a room message to reveal a small action panel at its top right.
The reply arrow opens its thread; the tooltip says **Reply in thread**.
The panel overlays the full-width message without changing wrapping or height.

Membership, profile, room-setting, and call events use compact activity rows:
a 16-pixel circular initial badge, smaller muted text, and the sender inline.
Consecutive activity rows keep close spacing; timestamps are available on hover.
Activity rows have no reply hover panel and break normal sender grouping.
Text remains selectable, including across activity rows and ordinary messages;
copying includes each activity's inline sender once. Messages, attachments,
stickers, polls, decryption failures, and removed-message placeholders retain
their ordinary message layout. Alt+T selects the newest message, skipping
activity rows. Existing reply links on activity events remain accessible.

Studio → Room activity mixes activity and messages, including a long topic change
for wrapping checks. Inspect it with the sidebar visible and hidden, then open
a message's thread. These fixtures use no account data or network requests.

Studio → Thread activity contains interleaved replies to two threads plus a reply
whose original is outside the fixture. Only the latest activity row for each
thread appears in the room. **New sample reply** adds an incoming fixture reply,
updating one summary's author/preview and blue new count. Click that summary to
open the latest replies; visible replies clear their new indicators while the
window is focused. Own demo replies also update the room summary. These
indicators are held in memory and do not send read receipts.

Thread panels keep the original message beneath the header, above the scrolling
replies. Its author, timestamp, and selectable text stay visible when opening a
busy thread at the newest reply. Long originals scroll independently within a
144-pixel area. The original appears once, and threads without replies show
**No replies yet**. The current room supplies the original while the thread loads.
The header's muted × button closes the thread; its tooltip is **Close thread**.

Connected history loads automatically, using the measured viewport plus three
screens of older content (at least 1,200 pixels of buffer). Scrolling up fetches
more. Threads initially fetch up to approximately 500 events or ten pages, then
use the same buffer. Only errors show a retry action. Demo histories remain
memory-only and already loaded; SDK mock-server tests exercise actual pagination.

Shortcuts:

- Ctrl+K: focus room search and select the current query (Command+K on macOS).
- Enter in search: open the first matching room.
- Alt+Up / Alt+Down: previous/next room within the selected space and query.
- Alt+T: open the last loaded message's thread and focus its composer.
- Escape: clear the search query first; otherwise close the thread and restore
  focus to the room composer.
- Tab / Shift+Tab: traverse the input controls. Stock buttons are not yet in
  the focus order; this is not complete keyboard accessibility.
- Ctrl+Enter in a composer: send; Enter: new line. IME composition suppresses
  sending and shell shortcuts. Host IME acceptance remains pending.

## Representative checks

Reaction fixtures are in **Studio → Design studio**: the welcome message and
the last message have multiple badges; opening the last message's thread shows
badges on its pinned original and first reply. Check own-reaction blue tints,
Unicode emoji, wrapping, and inert clicks. See
[reaction validation and SDK limits](connected-ui.md#message-reactions--2026-11-15).

1. In Studio, open Design studio and a message's thread. Type different drafts
   in each pane, change rooms/spaces, and return. Send a demo reply; verify it
   remains in that thread and does not enter another room or clear another draft.
2. Collapse Favorites, search for Design studio, and open it with Enter. Clear
   search, switch spaces, and verify the list and selected conversation agree.
3. Resize with a thread open from a wide window down to 780 pixels and the
   640-pixel minimum. Toggle the room list. Check wrapping and composer access.
4. Studio → Off the clock is empty. Research, references & tiny details contains
   a long paragraph, URL, and multilingual text. Community contains a 150-message
   timeline, a stable loading fixture, and an unavailable-conversation fixture.
5. Focus the room composer, thread composer, and search in turn; leave the window
   still. The existing opt-in diagnostics and measurement script apply to demo
   mode too. There is no animation or periodic UI timer. Search and composers use
   static visible carets.
6. Check the compact room list in All rooms and Studio at both the default window
   size and the 640×480 minimum. Verify selected rows and unread badges remain
   legible, the long room name wraps, and section collapse/search still work.
   Confirm there is no workspace label below the room list.
   This density-specific desktop check remains pending.
7. In both room and thread composers, type a multiline draft and scroll within
   it. Text, caret, and selection should stay above the send arrow. Check the
   tooltip, empty-draft disabled state, and sending by click and Ctrl+Enter in
   demo mode. Repeat at the 640×480 minimum. This composer-specific desktop
   check remains pending.
8. Hover short, grouped, and multiline messages, including the first/last visible
   row after scrolling. Check that the reply panel appears at the top right,
   remains clickable, and disappears on leaving. Wrapping and scroll position
   should stay fixed. Select/copy text away from the panel and open a thread
   through its arrow. Repeat at the minimum width. This overlay-specific desktop
   check remains pending.
9. Open threads with zero and several replies. Confirm the original appears once
   above the replies and stays put while they scroll. Open a thread on the long
   message and scroll its original independently; replies and the composer should
   remain accessible. Switch threads and check the original returns to its top.
   This thread-original desktop check remains pending.
10. In Studio → Room activity, check small badges, muted inline names, tight
    activity spacing, and the full message header after an activity by the same
    sender. Hover an activity for its timestamp; its row should stay still.
    Select/copy across messages and the wrapped topic event in both directions.
    Verify the sender appears once in each copied activity, and Alt+T skips the
    final leave event. Repeat with the sidebar hidden at a narrow width.
11. In Studio → Thread activity, click **New sample reply** twice. Confirm the
    Release checklist thread has one summary at its newest reply, with the last
    preview and **2 new**. Change spaces/rooms and return; the count should stay.
    Open the summary and confirm the latest replies are visible and the count
    clears. Type a thread draft, scroll upward, and add another sample reply;
    viewing room activity alone should leave it new. Jump to newest to clear it.
    Check the “a thread” fallback and summary wrapping with the sidebar hidden.

## Feasibility findings and scope

Room/thread scroll positions are remembered during the session; first-time opens
start at the bottom. A bottom-center floating chevron appears when scrolled up and jumps to the newest message. See the
[scrolling validation](connected-ui.md#timeline-scrolling--2026-11-15).

The shell uses standard Iced layout and styling APIs. No new custom layout,
drawing, or navigation widget is required for this visual treatment. A small timeline wrapper scopes widget state to each view; scrolling and layout
still use Iced. The existing
vendored caret option was extended to the search input to preserve idle behavior.

The important unresolved work is interaction depth: selecting/copying timeline
text, keyboard focus for message and navigation buttons, restoring message-ID
anchors across closed views, and virtualization for large histories. Open panes
now anchor the visible message when scrolled up and follow arrivals at the bottom. The
150-message fixture is a usability probe, not a performance budget or evidence of
bounded rendering. The presentation still renders every loaded row. Message
formatting, display-name resolution, timestamps, grouping consecutive senders,
media and complete unread/receipt semantics remain future work. Demo memberships and sections are intentionally presentation fixtures;
connected mode now supports joined nested spaces, SDK unread metadata, and
profile-local navigation restoration. Demo mode remains memory-only.

The prototype is ready for visual feedback; aesthetic acceptance is the user's
judgment. Completing this slice does not complete the Wayland-shell milestone or
establish a production-quality timeline.

## Local validation — 2026-11-15

Native Sway/Wayland inspection covered the wide shell, side-by-side thread,
780-pixel replacement-thread layout, search/Enter navigation, keyboard thread
opening/closing, and a local send. The default UI uses the same light palette
and bundled font on this host; no macOS build or desktop test was performed.

The workspace test run passed 30 tests with one existing SDK stress test ignored.
After adding the search-caret regression, all 10 GUI tests passed (31 regular
tests total across the workspace). Locked GUI build, formatting, and Clippy with
warnings denied passed. New behavioral tests cover space-filtered search and
keyboard navigation, plus draft/message isolation across room and thread changes.

With the thread composer focused, a 30-second demo capture recorded zero redraws,
zero UI updates, and 0.00 seconds of procfs CPU time; RSS stayed at 55,536 KiB.
This is a demo UI observation, not a connected SDK baseline or a scheduler-wakeup
measurement. The capture explicitly used `--cpu-only`; CPU time is quantized to
procfs ticks. Raw captures and screenshots live in the ignored local directory
`artifacts/design-preview-2026-11-15/`.

A second 30-second capture with search focused also recorded zero redraws, zero
UI updates, and 0.00 seconds of procfs CPU time. The 640-pixel minimum-width
thread layout was visually inspected as well: navigation scrolls independently
and the thread composer and close control remain available.

For the compact room-list follow-up, formatting, workspace Clippy with warnings
denied, all 27 GUI tests, and the locked GUI build passed. Desktop inspection of
the compact spacing remains pending; the earlier desktop observations above
predate this change.

For the compact composer follow-up, the same checks passed, including all 27 GUI
tests covering draft isolation, keyboard sending/IME guards, scroll restoration,
and static caret behavior. Desktop inspection of the new action-row layout and
tooltip remains pending.

For the pinned thread original, formatting, workspace Clippy with warnings
denied, all 29 GUI tests, and the locked GUI build passed. New tests exercise
current-generation lookup with room fallback, stale-source rejection, a bounded
long original, deduplication, and independent reply scrolling through real Iced
layout. Desktop inspection remains pending.

For activity styling, native Wayland demo screenshots were inspected at
1920×1200 for the room and side-by-side thread, and 636×1162 with the sidebar
hidden. Compact badges, inline muted text, wrapping, and full author headers
after activity were visible. The thread capture reused the mixed demo rows to
exercise activity among replies. Capture-only source hooks were removed after
the three demo processes exited normally; screenshots and the capture script
remain under ignored `artifacts/activity-rows/`. No connected client was
restarted and no Matrix messages were sent. Native pointer/tooltip and clipboard
acceptance remain pending; widget tests cover forward/backward copying through
a wrapped activity, selection after relayout, no selection timer while idle,
compact room/thread layouts, and keyboard thread selection.
Formatting, workspace Clippy with warnings denied, all 67 regular workspace
tests (35 GUI tests), and the locked GUI build passed. The existing opt-in
lifecycle stress test was not repeated. Idle resource measurements were not
repeated for this presentation change.

For recent thread activity, formatting, workspace Clippy with warnings denied,
all 76 regular workspace tests (43 GUI tests), and the locked GUI build passed.
The existing opt-in lifecycle stress test and idle-resource measurements were
not repeated. Native Wayland demo captures show two sample arrivals coalesced
into one latest-preview row with **2 new**, plus an unrelated thread and a
missing-original fallback. The focused thread capture shows the new count
cleared after its replies became visible. Wide captures are 1920×1200; the
sidebar-hidden capture is 476×1162 (the compositor tiled below the requested
minimum width). Temporary capture hooks were removed, and the ordinary binary
was rebuilt. Captures and the script are in ignored `artifacts/thread-activity/`.
All demo runs exited normally. No real messages were sent and the user's
connected client was not restarted. Native pointer/wheel interaction and
live-account acceptance remain pending; automated widget tests exercise the
same open/jump commands, focus gates, actual viewport IDs, and reading anchors.


## Settings infrastructure — 2026-11-15

Open Settings from the bottom bar or Ctrl+, (Command+, on macOS). Demo settings
remain in memory. The dialog offers startup room-list visibility and the send
shortcut, with Save, Cancel, and Use defaults. Escape closes it before acting on
search or threads. See [configuration details](../settings.md).

Manual acceptance:

1. With room and thread drafts and a scrolled timeline, open Settings. Try room
   navigation shortcuts and scroll over the backdrop; the conversation should
   stay in place. Cancel and verify drafts, thread, search, and scroll position.
2. Save Enter-to-send in demo mode. Enter should send a local sample in either
   composer, Shift+Enter should insert a newline, and Ctrl+Enter should still send.
   Cancel a later shortcut change and verify the saved choice remains active.
3. At 640×480, verify the card fits and scrolls to every control. Repeat with a
   long config path and an error. No setting introduces a periodic UI timer.
4. In a separately authorized connected run with a temporary `--config` path,
   Save to create TOML. Edit it externally, then Reload file. Check comments and
   unknown keys survive subsequent saves; external changes before Save should
   prompt a reload. Invalid TOML should retain active values and block saving
   until repaired and reloaded. Restart to check startup visibility.

Connected acceptance and native IME behavior remain pending; automated storage
and UI state/widget tests cover the persistence and modal behavior separately.

Validation passed: all 38 regular library tests and all 49 final GUI tests,
workspace Clippy with warnings denied, formatting, and the locked GUI build.
The existing opt-in lifecycle stress test was not run. New tests cover TOML
validation/defaults, comment and unknown-key retention, file permissions,
external-edit conflicts, failed saves, stale completions, shutdown during save,
modal input blocking, send shortcuts, and the actual timeline widget offset
across opening/closing the dialog.

Native Wayland demo screenshots were inspected at 1920×1200 and 636×1162,
including an error banner. The compositor tiled the requested 640×480 window to
the latter size; exact minimum-size layout/backdrop handling is tested through
Iced widget layout, not claimed as native desktop acceptance. Demo launches with
an explicit `--config` left that path absent. Temporary screenshot hooks were
removed and the ordinary binary rebuilt. Screenshots and capture scripts remain
under ignored `artifacts/settings/`. No saved account was opened, no real
messages were sent, and the user's connected client was not restarted.
Native pointer/keyboard acceptance, connected configuration acceptance, macOS,
and idle-resource measurements remain pending. The settings code adds no timer,
polling loop, or subscription.
