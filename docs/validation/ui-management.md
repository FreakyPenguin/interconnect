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

## Reporting messages

- In a room or thread reply, right-click a message and choose **Report message…**.
  This includes rooms reached through a space. The dialog shows the room, sender, and
  message preview, requires a reason (up to 4000 UTF-8 bytes), and sends only after
  **Send report**. Reports go to the account's homeserver administrators.
- Empty/oversized reasons cannot submit. Cancel/Escape preserve both composers,
  drafts, scroll positions, and content. Reporting does not redact or ban. Demo
  shows a simulated success and makes no network request.
- During submission, prevent duplicate reports for the same account/room/event.
  Closing or navigating does not retarget an admitted request. Late responses
  must not update a newer dialog/account; quitting waits for admitted writes.
  Errors preserve the reason and describe ambiguous delivery without auto-retry.
- Headless fixtures cover request paths/bodies for room and space events, ordinary
  member access, failure/cancellation, stale completions, drafts, small-window
  layout, and absence of idle redraw timers. With authorized dedicated profiles,
  verify report receipt in homeserver administration and permission/server errors.
  These live checks and native Linux/macOS acceptance remain manual.

Validation on 2026-11-25: locked workspace tests passed (388 library tests;
2 ignored); after the final UI refinements, all 727 binary tests passed (1
ignored). Formatting, workspace/all-target Clippy with warnings denied, and the
locked desktop build passed. An isolated Linux headless Wayland demo with
Tiny Skia/Light verified room ban/unban and banned-list scrolling at 1280×800 and
640×480, space ban scope and banned-list display, and report opening, preview,
reason entry, and simulated submission. Keyboard dismissal was also covered by
rendered-widget fixtures. No connected account was used; live moderation/report
receipt, Dark/GPU, and native macOS acceptance were not established by this run.

## Ban by Matrix ID

- Room member management and space settings offer **Ban by Matrix ID** to permitted
  moderators. Enter a full ID, choose **Review ban…**, then explicitly confirm.
  Departed people and IDs absent from the roster are supported. Invalid IDs,
  yourself, protected roles, and existing bans are rejected. Space scope remains
  limited to that space; conversations, drafts, and reading positions remain.
- Fixture coverage includes room/space requests for joined, departed, and unknown
  people, permission boundaries, cancellation, reload, and stale form input.
  Native desktop and live-server acceptance still require the dedicated checks.

## Moderation reasons and ban details

- Kick, invitation revocation, ban, unban, and message deletion accept optional
  reasons. Whitespace-only reasons are omitted; reasons are bounded to 4000 UTF-8
  bytes. Reasons are included in moderation events and may be visible in room
  history. Cancellation, failed requests, and unrelated conversations retain their
  own drafts. A new confirmation starts with an empty reason.
- Banned-member entries show the actor, server event time, and available reason.
  Missing/redacted details remain unavailable. A confirmed local ban shows its
  known actor/reason and requires Reload for an authoritative server timestamp.
  Demo details are local simulation data.
- Failed member writes retain the confirmation and reason for explicit retry.
  Deletion retries retain their transaction and original reason; the reason field
  becomes read-only after the first attempt. Enter in the reason field does not
  confirm deletion. Check room, thread, and space confirmations at compact sizes.
- Fixtures cover trimmed/omitted/oversized payloads, reason-bearing kick/ban/unban
  and redaction requests, server ban details, stale input, and retry preservation.
  Live-server event propagation and native platform acceptance remain manual.

## Moderate a message sender

- Right-click another person's room message or thread reply. **Remove sender…**
  and **Ban sender…** appear when the account has permission for that sender.
  Joined members/invitees can be removed; departed or unknown senders can still
  be banned. Yourself, protected roles, and already-banned senders have no ban
  action. Permission lookups run on demand and are cancelled with the menu.
- The confirmation names the sender and source room and accepts an optional
  reason. It targets that room even when browsing through a space. Cancel,
  Escape, navigation, and account changes cannot submit a stale prompt. Drafts,
  reading positions, and space filtering are preserved; messages are not deleted.
- Submit rechecks membership and permissions. Keep reasons on failure and require
  explicit retry. Prevent duplicate pending actions against the same account,
  room, and sender, including actions from a different message. Admitted writes
  finish independently of dismissal and release shutdown bookkeeping once.
- Fixtures cover room/space permission boundaries, lazy-read cancellation,
  demotion before submission, room/thread menu dispatch, stale lookup results,
  invalid reasons, drafts, duplicate writes, shutdown, compact layout, Escape,
  and no idle redraw deadline. Server enforcement and native platform behavior
  still need dedicated acceptance checks.

Validation on 2026-11-25 for these additions: the locked workspace suite passed
(396 library tests; two ignored), followed by the final binary suite (738 passed;
one ignored). Formatting, workspace/all-target Clippy with warnings denied, and
the locked desktop build passed. The permission fast path also has a fixture
asserting that a non-moderator's message menu does not request `/members`.
An isolated Linux headless Wayland demo in Light/Tiny Skia checked sender menus,
room-specific confirmation and simulated ban, reason entry, ban-by-ID form layout,
and ban detail presentation; sender confirmation remained usable at 1280×800 and
640×480. No saved account was used. Live server enforcement/propagation, complete
native keyboard/IME acceptance, Dark/GPU rendering, macOS, and resource measurements
remain outside this run.
