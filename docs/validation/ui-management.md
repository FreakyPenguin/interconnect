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
  while forms scroll. Feedback remains visible; Reload stays with the fixed room toolbar (or space footer).
- In Find rooms to join, scroll a long list at 640×480. Its title, Close, and
  Reload must stay visible while the directory body scrolls. Changing directory
  tabs and query/pagination cancellation retain their existing behavior.
- Save a multiline topic while another field is edited: only the topic is saved,
  the editor retains its line breaks, and the footer still reports unsaved changes.
- Confirm error and save messages remain beside the fixed actions rather than
  disappearing below the form. Read-only fields retain readable values.

## Members and destructive actions

- In the channel People roster at 300–360 px pane widths, names and Matrix IDs
  occupy the full identity row beside the avatar. Permission controls appear
  below; long Unicode names and IDs ellipsize without touching the role control.
  Hover either identity line to read the full value. Check editable roles,
  read-only administrators, and pending invitations, then widen and narrow again.
- Invite is beside search, and Refresh checks members/roles and online status
  without resetting the filter. A buffered refresh for an old roster must do
  nothing. No empty feedback area reserves space above search.
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

- Start a metadata/avatar save, close the panel, then reopen it while the admitted
  write is pending. Its completion must start one fresh load for the matching
  reopened room panel, without a timer or manual Reload. A duplicate old avatar
  completion must not reload or overwrite a new draft.

- Repeat the pending-write close/reopen check for Space settings and Room members,
  including metadata, avatar, and member changes. Completion may reload only a
  matching active account/target form with no loaded settings; a loaded draft,
  directory browser, exit confirmation, or other account must remain untouched.

## Channel settings polish

- In the room settings dialog, confirm Room details, Notifications, Security &
  access, Room addresses, and Membership have distinct headings and spacing.
  Access and history controls precede the longer address management tools.
- Edit each field independently. Its full-width value remains readable, the
  save icon has a contextual tooltip (for example, Save topic), and saving retains the other
  drafts. Unchanged fields show a Saved status rather than a disabled action.
- At 640×480 and desktop sizes, verify the bounded dialog, fixed Close and Reload,
  scrollable body, and readable feedback. Room Reload uses the header toolbar;
  an unchanged room form reserves no empty footer. Inspect edited fields and dropdowns,
  including the longest history option, without horizontal overlap.
- Published addresses, Local addresses, and Directory listing form separate
  groups. A long homeserver name wraps below the local input and directory
  checkbox; it must not consume the input's typing area. Publish/Create actions
  still apply immediately, and removing an unpublished local address still
  requires confirmation.

Focused geometry regressions exercise full-width field values with both Save and
Saved states, plus address input widths with a long server name at 240px and
560px body widths. Another regression checks the compact room header, body space,
and conditional feedback footer at 320×480 and 640×480. Run these with the locked UI suite; desktop appearance and
native dropdown placement still require the demo acceptance checks above.

## Room topic disclosure

- In demo mode, click Design studio's topic or its caret beneath the room name.
  It expands downward within the header; click again to collapse. The collapsed
  snippet retains bold, italic, code, strikethrough, and link styling, with
  grapheme-safe ellipsis at narrow widths. Check both light and dark themes.
- At 640×480 and desktop sizes, scroll a long expanded topic. The caret and
  Copy Markdown/Edit actions stay visible. The timeline and composer remain
  usable. Select text and copy with Ctrl+C/Command+C; Copy Markdown copies the
  original source, including line breaks and link destinations.
- External links retain expansion. The General room link uses normal internal
  Matrix navigation and clears the old room's expansion. Unknown Matrix rooms
  and users offer the existing join/new-chat flow.
- Choose Edit: the larger multiline dialog retains original Markdown. Save,
  clear, cancel, and dismiss with Escape; retain message drafts and timeline
  reading positions. Check wrapping of long save errors beside the fixed save
  control. The room-settings topic field also accepts Markdown source.
- Read-only rooms support expansion, selection, copy, and links; Edit is absent.
  Remote topic changes update both views. Room/account changes, permission loss
  during editing, and shutdown reject stale formatting/editor results. Space
  filtering preserves expansion. Escape collapses it after other dialogs dismiss.
- Raw HTML stays literal and image syntax shows alternatives. Excessive
  formatting falls back to plain text. Topics must not download media/previews,
  poll, or start animation/caret timers.

Automated regressions cover safe Markdown, limits, cache reuse/stale results,
read-only access, routing, original source, draft/scroll retention, styled snippet
truncation in both themes, disclosure toggling, minimum-pane timeline/composer
space, editor geometry, and idle redraw deadlines. Native clipboard/browser,
connected server writes, and macOS behavior require separate acceptance.

Local validation on 2026-11-20: all 550 locked UI tests, formatting, workspace
Clippy with warnings denied, and the desktop build passed. An isolated headless
Wayland demo session confirmed the styled collapsed snippet, inline disclosure,
responsive topic height, usable composer, and Escape collapse at 1400×900 and
640×480. Host clipboard/browser integration, connected writes, and native macOS
behavior were not exercised.
