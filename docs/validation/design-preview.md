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

## Feasibility findings and scope

The shell uses standard Iced layout and styling APIs. No new custom layout,
drawing, or navigation widget is required for this visual treatment. The existing
vendored caret option was extended to the search input to preserve idle behavior.

The important unresolved work is interaction depth: selecting/copying timeline
text, keyboard focus for message and navigation buttons, scroll anchoring during
pagination and incoming messages, and virtualization for large histories. The
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
