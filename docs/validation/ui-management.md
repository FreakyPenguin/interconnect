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
