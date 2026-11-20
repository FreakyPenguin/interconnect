# Discovery and media interface checks

Run the locked build and UI tests, then use demo mode. Check Light/Dark at
1400×900 and 640×480. Preserve room/thread drafts and older reading positions.

## Emoji and conversation pickers

- Initial emoji view shows common emoji/custom entries; search covers all emoji.
  Selecting a category limits search. Check the last column at narrow widths,
  paging, exact shortcodes, skin tones, keyboard selection, and Escape.
- Emoji packs filter/page with thumbnail demand limited to the visible 48 entries.
  Closing releases demand; browsing must not add animation/polling timers.
- Add/edit using Choose image or optional MXC URL. Only Save uploads. Test invalid
  URLs, read-only packs, preparation errors/cancellation, and late file results.
- Dirty Close/Reload/application exit asks Keep editing or Discard changes.
  Escape keeps editing; explicit Cancel discards the current editor. Nested emoji
  edits are reviewed before covered Settings. Old prompt actions cannot act again.
- Room/space pickers and New conversation fit empty, short, long, and Unicode
  results. Arrow/Enter/Escape preserve conversation state.

## Search, saved messages, pins, and menus

- New chat and Create room have separate sidebar icons, tooltips, and keyboard
  actions (Ctrl+N/Ctrl+Shift+N; Command on macOS). Space creation remains in (⋯).
  Permission loss invalidates an already-open Create subspace action.
- Context-menu Save/Remove saved message follows toolbar permissions/pending state.
  Search, saved messages, and pins use explicit Go to message/reply actions;
  selecting text must not navigate. Open older pins and pinned thread replies.
- Saved-message paging stays visible; removing the last item clamps the page.
  Section create/rename/remove keeps focus in its popup.

## Images and attachments

- Open a still/GIF, zoom to 2×/4× fit, pan both ways, then Fit. Reopen fitted.
  At 640×480, long filenames must not displace zoom and Close controls.
- Keyboard traversal and Escape return to the conversation. Viewer scrolling
  must not reach the covered timeline. Open and Save as have distinct tooltips.
- Still-image inspection adds no periodic deadline. Only explicit GIF Play uses
  animation deadlines; stop/close/unfocus/navigation cancel playback.
- Native file picking/opening, clipboard formats, and host portal paths need a
  desktop-service check; demo screenshots do not establish those integrations.

## Software redraw regression

`cargo test --locked --bin interconnect render_damage::` compares incremental
search-caption rendering with full redraws at 1400×900 and 640×480. It covers
translated/scaled text groups, empty cached icons replaced by text, and zero
unchanged-frame damage; see [the renderer patch](../../vendor/iced_tiny_skia/PATCH.md).
The comparison excludes a separate upstream antialias rounding difference on
input borders (at most 3/255 per channel). Native/GPU rendering needs its own check.

Routine save/reload/cancel/paging controls use contextual icon tooltips and keyboard
focus. Authentication, security decisions, creation, and destructive confirmation
submits retain explicit wording. Test actual activation as well as visible geometry.
