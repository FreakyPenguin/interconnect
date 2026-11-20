# Management UI validation

Run the locked UI tests and build before the demo acceptance checks in
[connected-ui.md](connected-ui.md). Use demo data only for local UI experiments.

## Unsaved settings

- Edit room or space metadata, prepare an avatar change, or type an unfinished
  room address. Close, Reload, open members, or begin leaving: require an explicit
  Keep editing or Discard changes choice before dropping those edits.
- Keep editing must retain all fields. Discard changes must replay only the
  captured transition. Saved fields remain saved when another field is discarded.
- Remote removal, account changes, and shutdown invalidation must still reject
  stale form actions. A write already admitted continues against its captured
  target and releases its bookkeeping once after the form closes.

The UI suite exercises draft preservation and discard transitions. Connected
server writes and native file-picker cancellation require desktop acceptance;
unit tests do not establish server or platform behavior.

## Management layout

- At 640×480 and desktop sizes, room and space titles/close controls stay fixed
  while forms scroll. Feedback and Reload remain visible in the footer.
- Save a multiline topic while another field is edited: only the topic is saved,
  the editor retains its line breaks, and the footer still reports unsaved changes.
- Confirm error and save messages remain beside the fixed actions rather than
  disappearing below the form. Read-only fields retain readable values.

## Members and destructive actions

- Member-role dropdowns display the current role as a value, including custom
  roles. Selecting the current role is a no-op; permitted changes require an
  explicit Change role confirmation.
- Remove/revoke confirmations appear beside the affected person and use specific
  action labels. Changing the member filter or page cancels a hidden confirmation.
- Avatar controls use labeled Choose/Change image and Remove image actions.
  Discard change and Save image appear only after preparation; selection never
  uploads until Save image is chosen.
- Leave/disband space pages keep Back and the final action visible while their
  consequences scroll, including at 640×480. Disband still requires the exact
  space name and the original asynchronous permission and partial-failure checks.

## Follow-up checks

- An invalid unsaved space name must not prevent saving a separate topic or
  access change; saving the invalid name itself must still report its validation
  error and retain the old saved name.
- A late Discard action from another panel request, or a Discard action without
  an active confirmation, must preserve the current form draft.

- Escape from a focused management field dismisses in one press. An open choice
  menu keeps Escape first; Escape from a discard prompt means Keep editing.
- Invitation buttons stay disabled for an empty or partial directory query.
  Choosing a result or typing a valid full Matrix ID enables invitation in both
  member-management forms and the People sidebar; backend permission and duplicate
  checks remain authoritative.

- Discarding an avatar draft must invalidate its picker serial even when a later
  recovery step keeps the panel open. A late picker result must not replace a new
  selection or restore a discarded preview.

- Cancel a discard prompt, reopen another prompt in the same settings panel, and
  deliver the earlier prompt's Keep/Discard action: only the current prompt token
  may act. Panel identity alone must not authorize a later confirmation.
