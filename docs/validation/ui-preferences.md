# Preferences and account dialogs

Run the locked build and UI tests, then use `cargo run --locked --bin interconnect -- --demo`.
Check Light/Dark/System at 1400×900 and 640×480. Connected checks need a dedicated profile;
see [account validation](connected-ui.md#desktop-login).

## Preferences

- Settings fills the workspace. General, Account, Notifications, Media & emoji,
  and Advanced fit the toolbar; compact windows place categories on a second row.
  Save/Close stay visible while only the body scrolls.
- Edit several categories, including invalid numeric input. Switching categories
  retains every edit. Invalid values disable Save and link to their category.
- Close, Escape, application exit, and Advanced → Reload ask before discarding
  edits. Escape from a discard prompt keeps editing. An old confirmation cannot
  dismiss a replacement form. Clean forms close immediately.
- Reload stages values; Save applies them. Discard after Reload leaves running
  preferences unchanged. Graphics/startup changes identify their restart requirement.
- Covered conversations retain room/thread drafts and the actual older scroll
  position. Empty-space clicks, scrolling, typing, focus, and viewport/read reports
  must not reach them.
- Disabling notifications retains the preview choice. Explicitly test sample
  notification feedback only when desktop-service interaction is intended.
- Focus numeric fields and leave Settings idle: carets remain static, with no
  periodic redraws. Measure [resources](idle-resources.md) separately.
- General → Interface size: stage each size, cancel, then save. Save must scale
  room/thread messages, formatted code, composer text, controls, and hit targets
  together; retain drafts and the selected conversation. Resize for larger sizes
  and verify Settings remains reachable to return to 100%. Without resizing the
  native window after Save, toggle the compact sidebar and drop files on each
  visible conversation pane; hidden panes must reject drops. Repeat after Reload
  and Save and after returning to defaults.
- Select System and change desktop appearance while reading and while Settings
  has unsaved edits. All message/code/topic and widget colors follow the native
  event without changing the draft. Explicit Light/Dark ignore desktop changes.
  Switch desktop appearance during an explicit app color choice, then choose
  System; it must adopt the current appearance and follow subsequent changes.
  Native window decorations continue following the desktop in every app mode.
  An unavailable desktop preference resolves to Light. Verify no polling appears
  in idle resource measurements.

## Account profile

- Account loads the display name/avatar on demand. Save display name and Save image
  publish independently; saving one retains the other draft. The toolbar Save is
  for preferences. Changes must update the account header without navigation.
- Clear the name, prepare/cancel/replace/remove an image, switch categories, and
  close/reopen. Only explicit field Save publishes. Dirty close/reload requires a
  choice. Demo changes persist only for that session.
- Connected: change the same field in another client before Save or during upload.
  Reject the conflict, preserve the draft, and require Reload. Test lost responses,
  denied writes, sign-out, and stale completions after account replacement.
- Check long Unicode names, errors, keyboard traversal, and image actions at 640×480.

## Dialogs and sign-in

- Join, invitations, leave, sign-out, verification, and recovery keep their header
  and final actions visible while long details scroll. Short forms remain compact.
- Tab/Shift+Tab stay in the active dialog and skip disabled controls. Enter/Space
  activate buttons/checkboxes; dropdowns and IME get first use of their keys.
  Escape closes an open dropdown before its dialog.
- Open/cancel dialogs while reading older room/thread history. Retain both drafts
  and scroll widgets. Nested confirmations appear in the topmost active form.
- Leave warns about last-admin/member/no-inviter consequences. Reviewing members
  does not leave; final Leave remains an explicit action.
- At 640×480, verification shows seven emojis/three numbers and persistent match/
  mismatch actions. Recovery retains its key until acknowledged.
- Sign-in/registration/token forms have labeled fields, visible focus, and static
  carets. Cancel during discovery, browser wait, and credential persistence;
  return to the form only after safe cleanup. An already authenticated attempt
  can finish saving. An old queued Cancel cannot affect the next attempt.
- Failure retains server/username and clears secrets. Cancelling registration or
  browser approval must not claim to delete an account/device created on the server.

Automated state/layout tests cover preference staging, theme resolution, native
appearance event handling, and idle deadlines. The minimum-size preferences
layout test loads bundled Inter Regular/SemiBold and selects Inter, matching
application startup so system sans-serif metrics do not change category wrapping.
Native appearance delivery and interface scaling still need the desktop checks
above. Native prompts, IME, clipboard, browser callbacks, and server writes require
desktop checks.

## Default Matrix link handler

Automated UI state tests cover demo simulation, independent preference staging,
panel reopening, action guards, and stale completions. Native associations require
these desktop checks; they are not established by the automated tests:

- Demo → General → Matrix links: Check again and Make default update simulated
  status. Cancel/reopen preserves the simulated default without touching OS settings.
- Installed Linux package: open Settings and compare status with
  `xdg-mime query default x-scheme-handler/matrix`. Explicitly choose Make default,
  check again, and verify the association. Restore the previous handler afterward.
- Running macOS bundle: compare with the desktop association, choose Make default,
  handle any native confirmation, and check again. Restore the prior handler.
- Run without the desktop package/app bundle, without xdg-utils, or with a denied
  desktop change. Verify actionable feedback and working retry controls.
- Close/reopen Settings during a check or change. Old results must not overwrite
  the new panel. A submitted change can finish after closing; Cancel does not undo it.
- At 640×480, check wrapping, keyboard navigation, and static idle behavior. No
  periodic checks should appear in resource measurements.
