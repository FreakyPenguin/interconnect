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
no subtitle, leaving more height for rooms. Long room names can still wrap.

Room and thread composers share a compact 80-pixel frame, with a small send
arrow at the bottom right. The bottom action row stays clear of editable text
and leaves space for future controls. The room/reply label and shortcut footer
are removed; hover over the arrow for **Send message (Ctrl+Enter)** or
**Send reply (Ctrl+Enter)**. The arrow becomes an ellipsis while queueing.

Hover over a room message to reveal a small action panel at its top right.
The reply arrow opens its thread; the tooltip says **Reply in thread**.
The panel overlays the full-width message without changing wrapping or height.

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
