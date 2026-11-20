# UI and account validation

Run the [development checks](../development.md#build-and-checks) and a locked desktop
build first. These are acceptance procedures, not completed-run records.

## Command-line startup

- Run `target/debug/interconnect --help` (or `-h`) and `--version` without a
  display, `HOME`, or XDG directories: exit successfully without reading settings,
  creating profiles, opening credential storage, or starting the UI.
- Check missing option values, unknown flags, multiple profile selectors, and
  extra positional arguments: fail before startup with a useful error.
- Named-profile resolution and invalid names are covered by headless unit tests:
  `--profile work` uses the platform profiles directory, retaining the XDG/HOME
  fallback and macOS Application Support behavior. Paths containing separators,
  empty names, `.` and `..` must not escape that directory.
- With an authorized test account, verify `--profile-path PATH` and the legacy
  positional directory select the same profile, including relative paths and
  paths with spaces. `--config FILE` must remain independent of profile selection.
- Demo must ignore profile/configuration paths and use sample data without
  resolving account storage, including when `HOME` and XDG directories are absent.

## Demo checks

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo uses sample data without credentials, account stores, Matrix traffic, or
uploads. Explicit file preparation/pickers and sample notifications can use native
services. Use only demo data in README screenshots.

Check Light/Dark at 1400×900, narrow widths, and 640×480; use wgpu and tiny-skia
where available. Across the checks below, preserve distinct room/thread drafts
and an older reading position. Check keyboard/IME/dialog precedence using the
[shortcut reference](../keyboard.md), scrolling, long labels, tooltips, and disabled
states. Static controls and focused fields must not schedule periodic redraws.

| Area | Check |
| --- | --- |
| Navigation | Breadcrumb peer/tree menus, shared/cyclic paths, Home/subspace filtering, picker, sidebar visibility, sections and ordering. |
| Timelines | Resize room/thread panes, load history, newest following versus anchored older positions, pending/failed/retry states. |
| Composition | Plain/Markdown preview, math/code/spoilers, completion, replies, edit/cancel, confirmed deletion, selection/copy. |
| Media | Inline dimensions, viewer, GIF play/stop, save/open, picker/paste/drop into each pane. |
| Discovery | Room/account search, saved messages, People, pins, invitations, room/space creation and discovery. |
| Management | Room/space settings, members, avatars, emoji packs, leave/disband previews and cancellation. |

## Shared lifecycle checks

For each asynchronous feature, close/reopen, navigate, switch accounts, and quit
during reads/preparation and writes. Cancel reads and reject stale results; admitted
writes retain their captured target and release shutdown bookkeeping once. Verify
errors preserve drafts/forms and require explicit retry where appropriate.

Management permissions/state must be rechecked before writes. Test remote changes
while a form or confirmation is open. Matrix changes are not atomic: after a lost
response or partial failure, let sync settle and Reload before retrying.

Use dedicated accounts/profiles and disposable rooms/spaces for connected or
destructive checks. Do not send messages, restart the user's connected client,
or revoke its session without authorization.

## Desktop login

Use a homeserver with native sliding sync and working native credential storage.
SSO requires localhost port **8765** forwarded to the browser's host. See
[profiles](../settings.md#account-profiles) and [container setup](../development.md#using-the-host-secret-service-from-the-container).

1. Log in and open encrypted history; record account/device identity privately.
2. Quit/resume the profile, including offline cached startup; identity and crypto
   store must be reused. A second process must fail the profile lock.
3. Lock/deny credential access or interrupt login: preserve existing stores/credentials.
4. Revoke the token; reauthenticate into fresh device state while retaining old
   encrypted stores and recoverable drafts.
5. Sign out: revoke remotely before deleting drafts/credentials. Remote failure
   permits cancellation; local cleanup retry must not revoke again. Retained SDK
   stores must not resume the revoked session.

### Registration

Headless mock-server tests cover direct/dummy registration, native registration
tokens and invalid-token retries, multiple browser stages, premature Continue,
cancellation/dropped attempts, malformed challenges, and common server errors.
UI tests cover confirmation, mode changes, token/password redaction and clearing,
duplicate/stale replies, and minimum-window idle layout.
These fixtures do not establish live server, browser, or native keyring acceptance.

With an authorized disposable account on a server offering native Matrix
registration and sliding sync, use a new, dedicated profile:

- Select **New to Matrix? Create an account**. Confirm mismatched passwords and
  a full Matrix ID prevent submission; switching back to sign-in clears passwords.
- Submit a username and matching passwords. Check unavailable registration,
  taken/invalid usernames, weak passwords, and errors retain homeserver/username
  while clearing passwords for an explicit retry.
- When the server requests a registration token, enter the administrator-provided
  token in the masked **Registration token** field and select **Continue**. This
  submits `m.login.registration_token` directly with the current registration
  session; it must not open a browser or require SSO. Check invalid/expired tokens
  show a retry prompt with an empty field. A later terms/browser step must retain
  the session without resending the token. Cancel, expiry, prompt replacement,
  and quit must clear the token; stale inputs/submissions cannot reach a new prompt.
- For CAPTCHA, email, or terms stages, open/copy the server's
  browser link, complete the step, and select **Continue**. Terms must require the
  user's acceptance. A premature Continue must show the unfinished step again;
  multiple steps must use the same registration session. No polling is used.
- Cancel at a token/browser step or quit while waiting; no later prompt reply may
  advance another attempt. Registration expires after fifteen minutes overall.
  Cancellation stops the local attempt; it cannot undo a server-created account.
- Successful registration should open the account and device verification flow.
  Quit/resume the dedicated profile and verify account/device/crypto identity is
  reused. If account creation succeeded but session saving failed or the response
  was lost, return to sign-in rather than creating another account.
- Check scrolling/tab navigation at 640×480 and with long errors. Static form
  and token/browser-wait states must settle without periodic redraws.

OAuth-only/delegated account creation is not implemented by this native
registration form. Use the homeserver's own account creation page, then return
to sign in using its supported method.

## Native macOS account storage

Use the [native bundle](../development.md#macos-builds). Check Keychain allow/deny/
locked states, access after rebuilding, resume/offline startup, and cleanup retry.
Demo and an empty login form must not prompt. Cancellation must retain the profile
lock until native work finishes.

Opt-in fixture (macOS; creates temporary test records):

```sh
cargo test --locked --lib session::credentials::macos::tests::native_keychain_round_trip -- --ignored
```

Linux private Secret Service fixtures and CI tests do not validate Keychain policy.

## Home navigation

- Home defaults to rooms outside joined spaces, including DMs; Include subspaces
  shows all rooms. Spaces toggle direct/recursive rooms without duplicates.
- Toggle each location, leave/return, then restart a test profile: retain route,
  expanded branches, and per-location toggles. Element's Home preference must not
  override or be written by the local toggle; legacy All rooms becomes recursive Home.
- Exercise Home's tree, breadcrumb peers, shared-child routes, cycles, removed
  links, and long-path overflow. Avatar arrival must not shift rows/focus.
- Use Ctrl+Shift+P / Command+Shift+P to search names/paths, wrap selection, switch
  to room search, and dismiss. Renamed/removed results must invalidate stale choices.
- Toggle Include subspaces by mouse, keyboard, and Ctrl+Shift+L / Command+Shift+L
  with sidebar hidden or composer focused. Preserve focus/query; key-repeat must
  not toggle repeatedly. Check F6 and [space keyboard navigation](../keyboard.md#space-navigation).

### Add and remove space links

- **Add room to space…** lists only joined writable spaces lacking a direct link;
  recursive visibility must not prevent adding a direct link. Check empty results,
  permission changes, duplicate links, errors retaining the picker/query, and retry.
- Demo: add General to Personal without removing Studio. Remove Design studio
  from Studio, then Design; Home should show it only after its last link is gone.
- Remove is available only for a direct link in the selected writable space.
  Connected add sends `m.space.child` with non-empty `via`; removal empties `via`.
  Neither changes child parent metadata, room membership, or other links.

## Navigation and timelines

- Rapidly switch/paginate/send: late results must not replace or act on another pane.
- Newest panes follow arrivals; older positions stay anchored through arrivals,
  pagination, resize, sidebar toggles, and space filtering.
- Check available/missing encryption keys, offline/reconnect, and retry separately
  from device trust.
- Create/rename/remove/reorder custom sections; compare with Element, including
  empty/duplicate labels and sections hidden by the current filter.
- Open saved/search originals across rooms/threads; Back/Escape preserves the
  source conversation. Older-event loading remains cancellable. Test inaccessible
  saved sources and malformed/shared storage.

Positions are per-session; loaded history can grow memory without virtualization.

### Links, pills, and permalinks

- Check authored user/room labels and semantic pills for raw Matrix destinations,
  including unknown IDs, split formatting, RTL/emoji, wrapping, and hidden spoilers.
  Rendering must not fetch profiles/media/events. Ordinary/code URLs stay literal.
- Drag-select without activation. Text copy preserves canonical text/URLs even
  when a pill has a short summary. **Copy link URL** copies the original destination,
  including queries; edit/navigation invalidates an open link menu.
- User links reuse a DM or prefill creation; self links do nothing. Known rooms
  open, joined spaces filter without changing conversation, unfamiliar rooms
  prefill Join without submitting. Event links load/scroll to room or thread events.
- Test encoded IDs, `via`, `matrix:` and matrix.to, invalid/lookalike hosts,
  unavailable/private/encrypted events, and cancellation or address edits after
  opening Join. No stale event navigation may follow a dismissed/replaced form.
- Connected **Copy room link** uses the room ID; **Copy message URL** uses the exact
  event, including thread replies. Synthetic demo IDs and pending events without
  server IDs omit these actions. Check native clipboard and opening in another client.

## Read receipts and unread clearing

Focused, uncovered newest room panes send public read/fully-read updates. Older,
unfocused, or covered panes must not clear new activity. Threads update their
receipt without advancing room fully-read; reader labels are room-scoped.
Test arrivals, manual Mark unread, reopen, and offline failure/recovery without a
recurring retry timer.

### Unread catch-up divider

Demo Release notes has a deterministic New messages boundary. Check sender/date
separation. Connected rooms capture the opening receipt and keep the divider there
as receipts/arrivals advance; reopen captures the new position. No guessed divider
for unloaded/missing/unrendered receipts, manual unread alone, or outgoing-only
activity. Loading older may reveal it; thread panes have no room divider.

## Draft recovery across restarts

Use an isolated connected profile to restore distinct room/thread drafts with
Plain/Markdown mode, emoji/mention bindings, and text or empty reply intent. Sent
message edits preserve the original unsent draft on cancel/restart. Attachments
and scroll positions do not persist; demo drafts stay in memory.

Quit during a save and confirm the newest snapshot flushes. With an unwritable or
malformed store, check Retry, Keep open, and explicit Close without saving while
preserving invalid data. Successful sign-out removes drafts before keys; failed
remote sign-out retains them. See [storage boundaries](../architecture/drafts.md).

## Composition and formatting

- Toggle themes with pending drafts/older positions. Check Save/Cancel/Defaults,
  persistence/Reload in a test profile, legacy missing theme, and invalid theme
  retaining the last valid settings. Demo never writes settings.
- Inspect code, lists, links, spoilers, math, images/placeholders, mentions, and
  selection in room/thread messages and previews. Test Unicode/bidi and copy
  across runs/blocks; hidden spoilers copy placeholders.
- Highlight Rust/TypeScript/TOML/Dockerfile blocks in both themes. Unknown/unset
  languages, blocks over 16 KiB, or lines over 2048 bytes stay plain monospace.
  Colors survive resize/theme changes; inline code stays plain. Stale preview
  preparation must not replace a newer source/conversation.

### Mentions and room links

- Accept `@` users and `#` rooms with arrows, Enter, Tab, and mouse without sending.
  Duplicate/Unicode names retain distinct targets. Email/URL/IME contexts must
  not spuriously open user completion; Escape dismisses before closing a thread.
- Shorten labels, delete partially/wholly, undo, preview/send/edit, switch drafts,
  and restore encrypted drafts. Surviving labels keep identity; whole replacement
  removes it. Decorations follow wrapping/scrolling without shifting text/carets.
- Plain/Markdown code, raw HTML, links, and image alternatives must not acquire
  inferred mentions. Room links add neither user IDs nor room notification flags.
- Explicit `@room` completion is permission-gated, amber, and non-navigable.
  Altering/deleting its token removes the binding. Literal tokens stay unbound;
  received styling requires `m.mentions.room`. Downgrade permission before Send.
- Edit mixed mention kinds repeatedly: retain surviving targets/metadata, remove
  the last explicitly represented user's ID or room flag, preserve unrepresented
  legacy metadata, and restore the original draft on Cancel.

### Ordinary replies

- Reply acts in the current composer; Reply in thread opens a thread. Check each
  on room messages, roots, and thread replies, including long/attachment excerpts.
- Choose/replace/cancel reply intent without losing source, cursor, bindings, or
  attachments. Escape cancels reply after completion; pending send disables changes.
- Queue admission clears reply intent; conversion/queue failure preserves it.
  Loaded/embedded originals reflect edits/redactions; missing context shows unavailable
  without background fetching.
- Connected text/attachment sends carry `m.in_reply_to.event_id`. Room composer
  replies have no `m.thread`; thread replies retain the root with explicit-reply
  `is_falling_back: false` (or omitted). Check mentions, local echo, encryption,
  retry, and the other client's rendering.

### Multiple attachments

- Pick/paste/drop batches into both panes, append more, remove individually, and
  check order/long names/scrolling. Cancel or invalid/inaccessible/>50 MiB input
  preserves the existing list/caption. Reject preparation results after navigation.
- Send one message per file, caption on first, reply relation on each. Remove the
  second file after staging: only queued files clear; remaining files stay for
  retry. Failure before the first admission retains the caption. Check encryption,
  offline queue/retry, pane routing, and shutdown during a batch.

## Sending and room management

Use a second client for server-dependent state and delivery:

- Send text/emotes/media in rooms/threads; test offline queue, retry, duplicate
  prevention, encryption, editing, confirmed deletion, reactions, pins, and topics.
- Start a DM, create/join by ID/alias/link, accept/decline invites, and leave.
- Save/remove/page bookmarks and open originals, including unavailable encrypted
  sources and invalid storage. Manage personal/room emoji packs; only Save uploads,
  conflicts require Reload, removal keeps media, and uploads are unencrypted.

### Room and space settings

Save name/topic/access/history independently; check per-target demo state, discard
unsaved edits on close, and Reload/conflict/permission errors. Special access rules
stay read-only. Room notification overrides sync across clients; returning to
account defaults works. History visibility must not promise old keys. Hidden
conversation panes must not issue read receipts or demand visible-message media.

### Conversation and space creation

- From Home, create private/public root spaces with no parent/link writes; from a
  writable space, create fixed-parent unencrypted subspaces. Preserve conversation
  and filter. Deny parent permission before submission; fail before creating.
- On parent-link failure, retry only the link to the created ID.
- With a controlled server/proxy, lose a creation response, return 500/429, or
  stall. Room/DM creation sends once per explicit submission (30-second timeout),
  without automatic replay. Check the room list before retrying ambiguous creation.
  DM reuse must survive a failed/cancelled `m.direct` update before sync.

### Members

Check People and room/space member editors with large rosters, search/paging,
pending invitations, directory/full-ID invites, denied/banned/invalid targets,
and preserved search on permission reload. Confirm removal, revocation, and
Member/Moderator/Admin changes; preserve unrelated power-level fields. Self,
peer, and protected-creator restrictions apply; admin promotion may be irreversible
by the actor. Space actions must not affect child membership; removal does not ban.

### Discovering rooms

- Browse nested/unjoined children; root metadata must not list itself. Search loaded
  names/topics/aliases/IDs and explicitly load pages. Check deduplication, repeated/
  invalid tokens, page errors, and the 1,000-room per-tab cap.
- Homeserver queries run only on submission, including empty search. Reload/Load
  more retain the submitted query. Tab switches cancel reads and retain separate
  query/result/continuation state; Current space disables outside a space.
- Join public, invited, and eligible/ineligible restricted rooms with routing hints
  for remote/opaque IDs. No enabled join for uninvited invitation/knock-only rooms.
  Joining does not navigate or add a space link; explicit Open may navigate.

### Leave and disband space confirmations

Use disposable spaces. Check fresh counts/permissions, exact-name confirmation
for Disband, and no Enter auto-submit. Change members/links/permissions after
preview: require review again. Leave/forget must retain children and other members.
Disband closes access, unlists locally, removes confirmed active memberships and
links, verifies completion, then leaves. Protected peers can block it. Inject
partial failures/new members; retain the actor until final checks pass and require
Reload/explicit retry. A failed forget retries only forget. Child rooms/history,
aliases, and external references remain; older snapshots must not restore the space.

### Room leave consequences

Check public/encrypted rooms, elevated/last admin, no remaining inviter, and final
member warnings. Opening/Enter alone must not leave; failed previews disable
Leave and offer Reload. Changed encryption/membership/capabilities invalidate the
confirmation. Leaving sends no forget/key deletion and preserves other conversations.
Review privileges/key recovery before destructive testing.

### Room and space avatars

Preview PNG/JPEG (EXIF rotation)/WebP; cancel, damaged/oversized input, or stale file
results preserve the draft. Only Save uploads. Check permissions independently
from name/topic, remote avatar changes and permission loss during upload, Reload,
removal without media deletion, and other unsaved fields surviving. Lost responses
require checking server state; upload/publication is not atomic.

## Inline GIF playback

Play animates the existing inline preview; outside-image clicks open the viewer.
Stop/completion retains the frame and permits replay. Test both playback settings,
rooms/threads, separate thumbnails, and encrypted GIFs. Only Play requests the
original for inline animation; loading stays still/cancellable. Scroll out, change
rooms, close thread, open viewer, or unfocus: stop and never auto-resume. Failures
must offer viewer details/retry. See [idle measurement](idle-resources.md).

## User presence

Check on-demand snapshots/refresh, the 60-second cache, optional polling off/on
and pause while unfocused, unavailable endpoints, and stale navigation results.
Failures must not block conversations; no classic-sync connection is added.

## Lifecycle and desktop integration

Check native app identity/icons, picker permissions, clipboard formats, room/thread
drop coordinates, external attachment opening, [notifications](notifications.md),
and [verification](verification.md). Container file paths must be accessible.
Include offline/reconnect, hidden windows, suspend/resume, and repeated navigation
with released-timeline and [resource measurements](idle-resources.md).

## Validation scope

Automated state/mock-server/crypto/native-protocol fixtures and demo screenshots
do not establish live-server interoperability, native prompts/fonts/IME/clipboard,
macOS behavior, GPU rendering, or CPU/wakeups. Record platform, build, renderer,
and observations with acceptance results; keep private data and run artifacts out
of this reference and version control.
