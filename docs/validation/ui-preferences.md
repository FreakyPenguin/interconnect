# Preferences and account dialogs

These are demo/manual acceptance procedures, not a record of connected or native
macOS validation. Build with the locked desktop command before launching
`cargo run --locked --bin interconnect -- --demo`.

## Preferences

- At 1400×900 and 640×480, open Settings. It fills the workspace with an opaque
  panel. General exposes appearance, sending, and room-list preferences;
  Notifications, Media & emoji, and Advanced are directly selectable. Wide windows
  use one toolbar row; compact windows put categories beneath the heading. Only
  the body scrolls, and no empty footer reserves space. In Light and Dark, check
  that Save and Close remain visible and category labels fit without truncation.
- Save uses a disk icon and Close uses an X, both with action tooltips and keyboard
  focus. Unsaved changes and an admitted save have a compact status beside the
  heading. Errors and links to invalid values appear immediately below the
  toolbar; confirmations retain explicit Keep editing and discard text actions.
- Edit values in several categories. Switching categories must retain every edit
  and numeric input, including invalid input. An invalid value keeps Save disabled
  and offers a link to the category needing attention.
- Close using X, Escape, or the application close/exit action with an edited form:
  keep editing or explicitly discard. Old confirmation actions must not dismiss
  a replacement confirmation or a newly opened form.
  Clean forms close immediately. Preserve the open room/thread, their drafts, and
  an older reading position throughout. Empty-space clicks and scrolling must not
  reach the retained conversation beneath Settings.
- Advanced → Reload (refresh icon, with tooltip) warns before discarding edited
  values. Reload fills the form; Save applies it. Closing and discarding after
  reload leaves running preferences unchanged. Advanced explains that graphics
  and room-list startup options apply only on the next launch.
- Disable desktop notifications: message preview becomes unavailable while its
  saved choice is retained. Sample-notification failures are visibly identified
  beside the sample action. Do not send sample notifications during unattended
  review unless desktop-service interaction is intended.
- Focus the GIF-duration input. Its steady caret must not schedule periodic
  redraws. Static controls must remain event-driven.

The headless suite includes category draft retention, dirty-close/reload choices,
reload staging, and a 640×480 panel fixture checking full bounds, compact toolbar,
remaining body height, icon hit areas, and empty-space input capture. The panel
fixture exercises the integrated full-panel layout. Desktop appearance, native
notification permissions, and CPU/wakeups still need the manual checks above;
headless geometry tests do not establish platform service behavior.

## Account and membership dialogs

- Joining, invitations, room leaving, sign-out, draft-close recovery, device
  verification, and key recovery use the shared fixed-header/fixed-action frame.
  Check both themes at 640×480 and desktop sizes. Short invitation lists should
  produce compact cards; long details scroll without moving the actions.
- Tab and Shift+Tab must stay inside the active dialog and skip unavailable
  controls. Enter/Space activates focused buttons and checkboxes. Dropdown arrows,
  Enter, Escape, and IME retain precedence; one Escape closes an ordinary form
  after its child dropdown is dismissed. A pending destructive operation retains
  its existing cancellation restrictions.
- The covered conversation stays visible but inert. It must not receive typing,
  focus traversal, viewport reports, or hover overlays. Open and close several
  account dialogs while reading older history: retain the actual scroll widget
  and independent room/thread drafts.
- Room leave details highlight the last-administrator, last-member, or no-inviter
  consequence when relevant. Review members & permissions opens member management
  without leaving the room. Confirming Leave remains an explicit separate action.
- Verification at 640×480 shows all seven emojis and all three numbers without
  scrolling during comparison. Identity is compact; comparison and persistent
  match/mismatch actions remain available. Recovery retains its key until the
  user acknowledges saving it, and focused recovery inputs have steady carets.

## Sign-in and registration

- Empty sign-in, registration, and registration-token forms have persistent field
  labels, shared input styling, visible keyboard focus, and non-blinking carets.
  Headless fixtures cover their geometry and focused-input redraw deadlines.
- With an authorized disposable profile, cancel browser/password sign-in during
  discovery/authentication. The UI stays in its waiting state until the connection
  owner reports cancellation or completed sign-in. Repeated Cancel requests are
  suppressed; a full command queue offers an explicit retry. A buffered Cancel
  from an earlier attempt must not cancel a subsequent attempt.
- Cancel after native credential persistence begins: cleanup/persistence finishes
  safely before a new sign-in form is admitted. An already-completed sign-in can
  succeed. Registration cancellation can leave a created account or a browser-
  approved device; the UI must not imply those were deleted.
- Check that failed sign-in preserves server/username input, registration secrets
  remain redacted, and returning to the form clears the cancellation status.
  Native keyring access and actual browser callback behavior require connected
  acceptance; demo/headless fixtures do not establish them.
