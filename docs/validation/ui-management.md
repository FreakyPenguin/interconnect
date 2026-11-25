# Management interface checks

Run the locked build and UI tests, then use demo mode. Check Light/Dark at
1400×900 and 640×480. Use [connected checks](connected-ui.md#sending-and-room-management)
for server permissions and writes.

## Editing and dismissal

- Edit room/space fields, prepare an avatar, or type an unfinished address.
  Close/Reload/member management/leave requires Keep editing or Discard changes.
  Escape from the prompt keeps editing; discard replays only the captured action.
- Save fields independently. An invalid unsaved name must not block a separate
  topic/access save; successful saves retain all unrelated drafts.
- Cancel/reopen confirmations and deliver an old action: only the current prompt
  can act. Late file results cannot restore discarded avatars.
- Start a write, close, and reopen the same room/space/member form. Completion
  reloads an unloaded matching form once, preserving an already-loaded draft.
  Other targets/accounts and exit/directory forms remain untouched.
- Close reads/preparation on navigation; admitted writes retain their target and
  finish shutdown bookkeeping once. Permission loss/account removal invalidates
  stale edits. Test errors and explicit retry without losing drafts.

## Layout and actions

- **Settings → Devices** lists local demo devices on demand. Rename and revoke
  confirmations preserve the conversation; the current device cannot be revoked.
  Check long names/IDs, Light/Dark, compact width, and keyboard controls. Password
  and browser UIAA require authorized connected acceptance; demo never opens them.

- **Sidebar actions** offers adding the selected joined space to another writable
  space. The picker fits at 640×480, supports search/Enter/Escape, and excludes
  self/duplicate/cyclic links. Unlink follows the selected breadcrumb parent;
  other parents and the open conversation remain. See the connected link checks.
- Room settings groups details, notifications, security/access, addresses, and
  membership. Values use the available width; Save icons have field-specific
  tooltips. Unchanged fields show Saved. Long history options must fit.
- Headers/Close/Reload and feedback remain visible while the form scrolls. The
  room browser keeps its tabs and final actions usable with long results.
- Published addresses, local addresses, and directory listing are distinct.
  Long server suffixes wrap below inputs, without reducing the typing area.
  Creating/publishing applies immediately; local removal requires confirmation.
- Avatar selection never uploads before Save image. Change/Remove/Discard have
  distinct actions, and errors retain other edited fields.
- Tab/Shift+Tab stay inside the active form. Escape from a field dismisses once;
  open dropdowns consume Escape first. Invitation requires a valid full Matrix ID
  or selected directory result, not a partial search.

## People and destructive actions

- At 300–360 px pane widths, names/Matrix IDs fit beside avatars; roles appear
  below. Long Unicode identities ellipsize and expose full values on hover.
  Invite sits beside search. Refresh retains the search and rejects stale targets.
- Role dropdowns show the current value, including custom roles. Selecting it is
  a no-op; changes/removal/revocation require explicit, person-specific confirmation.
  Filtering/paging cancels hidden confirmations.
- Leave/disband keep Back and final actions visible at 640×480. Disband requires
  the exact name and fresh permissions/counts. Cancel and Enter alone do not act.
  Verify partial failures against disposable spaces; see connected procedures.

## Markdown topics

- Click Design studio's topic/caret to expand beneath the header and collapse it
  again. Collapsed bold/italic/code/link/strike styles and Unicode ellipsis fit
  narrow widths. The expanded body scrolls without hiding Copy Markdown/Edit.
- Timeline/composers remain usable. Text selection copies displayed text; Copy
  Markdown preserves source and links. External links keep expansion; internal
  room navigation clears it. Space filtering retains it. Escape collapses after
  dismissing higher-priority dialogs.
- Edit retains source/line breaks in a separate dialog. Save, clear, Cancel, and
  Escape preserve message drafts and scroll positions. Read-only topics omit Edit.
- Remote changes refresh both views. Navigation/account/permission changes reject
  stale formatting/edit completions. Raw HTML stays literal; images show alt text;
  excessive formatting falls back to plain text. No media demand or timers.

UI fixtures cover layout, keyboard actions, stale results, drafts, and redraw
deadlines. Native clipboard/browser/pickers, live server writes, macOS, and resource
use need the dedicated desktop/connected checks.

## Basic moderation

- In room members, the room People sidebar, and space settings, Ban requires a
  person-specific confirmation. Cancel, filtering, and paging must not ban anyone.
  Members and pending invitees are eligible only when permissions allow; hide
  controls for yourself and peers above the permitted power boundary.
- A space ban affects the space alone. Child-room membership, the current
  conversation, drafts, and reading positions remain intact.
- Backend fixtures cover room/space ban and unban requests, permission boundaries,
  stale membership, and single-attempt server failures. UI fixtures cover ban
  confirmation/cancellation and conversation preservation. Live server behavior,
  native desktop layout, and macOS still require dedicated acceptance checks.

- Room member management and space settings expose a **Banned** tab with search,
  bounded pages, an empty state, Reload, and confirmed Unban. The People sidebar
  links to **Manage members & bans**. Unbanning removes the ban without sending
  an invitation or joining the person. Toggle tabs/search/page while confirming:
  hidden confirmations cannot act. Demo bans persist in memory across reopening.
- Fixtures verify separate banned/joined/invited projections and room/space unban
  cancellation, reload, and preservation of the open conversation. Check a
  dedicated server for remote bans/unbans, permission loss, and failed requests;
  these connected checks have not been run by the fixtures.
