# Discovery and media interface checks

Run the locked desktop build and UI tests before these demo checks. Use
`cargo run --locked --bin interconnect -- --demo`; do not use a connected account
for sample content. Check light and dark themes at 1400×900 and 640×480.

## Emoji and conversation pickers

- Open the composer or reaction emoji picker. Common emoji and the first custom
  pack entries appear initially; searching from this category searches all emoji.
  Choosing a named category limits the search to that category.
- Check the complete rightmost emoji column at narrow widths. Scroll and page
  through All emoji, search an exact shortcode or skin-tone sequence, and select
  without moving focus to an underlying composer or sending a message.
- Open Emoji packs, filter the selected pack, and change pages. Thumbnail demand
  is bounded to the current 48 entries and is released on close. Filtering and
  paging must not schedule animation or polling timers.
- Add/edit an emoji: Choose image is the primary path; existing MXC URLs are in
  an explicit optional section. A prepared file is uploaded only when saved.
  Check cancellation, read-only packs, errors, and an invalid image URL.
- Change a pack name, new-pack name, or emoji description/shortcode/image. Close
  or Reload must offer Keep editing / Discard changes. Escape cancels that
  confirmation; explicit Cancel in the editor discards the edit. An unchanged
  existing emoji closes directly. Closing during file preparation cancels the
  read and rejects its late result while retaining the entered fields.
- Check room/space quick-open and New conversation with empty results, one
  result, many results, and long names. Short results use a compact card; longer
  lists scroll within the available window. Arrow/Enter navigation and Escape
  dismissal retain the conversation's draft and reading position.

Automated regressions exercise emoji-column bounds, exact-key search/selection,
filtered thumbnail demand, stale picker results, and existing account/cancellation
behavior. The steps above are manual acceptance procedures, not a completed
native macOS or connected-server run record.

## Search, saved messages, pins, and menus

- Open New in the sidebar. Use arrows, Tab/Shift+Tab, and Enter/Space to choose
  New chat, Create room, or Create space/subspace. Escape returns to the trigger.
  Removing space-management permission while the menu is open must reject an
  outdated Create subspace action.
- Right-click a message and use Save message, then Remove saved message. Verify
  the actions have the same permission/pending checks as the message toolbar.
- Search results, saved messages, and pins provide a labeled Go to message/reply
  action without making the selectable text itself navigate. Open an older pin
  and a pinned thread reply; reject a stale room-generation action.
- Saved messages show the time they were saved and a persistent range and
  Previous/Next controls. Remove the last item on the last page: pagination
  should clamp to the remaining entries rather than show an empty stale page.
- Create/rename/remove a sidebar section. Inputs and action styling match other
  forms, and keyboard focus stays inside the popup.

Automated tests cover pin target validation and draft retention, creation-menu
permission changes, saving/removing from the context menu, and existing search
and saved-message navigation behavior.

## Image inspection and attachment actions

- Open a still image and an explicitly played GIF. Zoom in to 2× and 4× the fit
  size, pan with wheel/scrollbars in both directions, then choose Fit. Reopening
  the viewer starts fitted again. Zoom controls and Close remain visible with
  long filenames at 640×480.
- Tab through viewer controls and return to the conversation with Escape. Check
  that panning cannot scroll the covered conversation.
- Open and Save as have visible labels on attachments and in the viewer. Native
  file selection/opening needs a desktop service; demo captures do not validate
  host portal access or native macOS file behavior.
- Zooming/panning a still image must not add an animation subscription or a
  periodic redraw deadline. GIF playback continues to use only its existing
  explicit Play/Stop deadlines.

Automated regressions cover bounded zoom, reset on reopening, minimum-window
layout, backdrop scroll capture, existing download cancellation, and GIF timing.

Application Close/Exit must review the topmost dirty emoji editor before any
covered application Settings form. Keep editing cancels the transition. After
explicit discard, remove the emoji dialog before replaying the captured exit so
that a parent Settings confirmation is visible. Clean dialogs close immediately;
already-admitted writes keep their independent shutdown bookkeeping. Buffered
Keep/Discard actions from replaced or reopened confirmations must be ignored.
