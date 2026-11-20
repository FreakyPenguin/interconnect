# Preferences and account dialogs

These are demo/manual acceptance procedures, not a record of connected or native
macOS validation. Build with the locked desktop command before launching
`cargo run --locked --bin interconnect -- --demo`.

## Preferences

- At 1400×900 and 640×480, open Settings. General exposes appearance, sending,
  and room-list preferences; Notifications, Media & emoji, and Advanced are
  directly selectable. Only the body scrolls; the categories and actions remain
  available. Each category must fit horizontally in Light and Dark.
- Edit values in several categories. Switching categories must retain every edit
  and numeric input, including invalid input. An invalid value keeps Save disabled
  and offers a link to the category needing attention.
- Close using X, Escape, or the application close/exit action with an edited form:
  keep editing or explicitly discard. Old confirmation actions must not dismiss
  a replacement confirmation or a newly opened form.
  Cancel explicitly discards the form. Clean forms close immediately. Preserve the
  open room/thread, their drafts, and an older reading position throughout.
- Advanced → Reload file warns before discarding edited values. Reload fills the
  form; Save applies it. Cancel after reload leaves running preferences unchanged.
  Startup options still apply only on the next launch.
- Disable desktop notifications: message preview becomes unavailable while its
  saved choice is retained. Sample-notification failures are visibly identified
  beside the sample action. Do not send sample notifications during unattended
  review unless desktop-service interaction is intended.
- Focus the GIF-duration input. Its steady caret must not schedule periodic
  redraws. Static controls must remain event-driven.

Headless regressions cover category draft retention, dirty-close/reload choices,
reload staging, and the 640×480 notification-body/action geometry. Desktop
appearance, native notification permissions, and CPU/wakeups need manual checks.

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
