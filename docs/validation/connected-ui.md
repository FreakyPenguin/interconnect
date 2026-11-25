# UI and account validation

Run the [development checks](../development.md#build-and-checks) and a locked desktop
build first. This is an acceptance checklist, not a record of completed runs.
Use dedicated authorized profiles and disposable rooms/spaces for live checks;
do not send test messages or restart the user's connected client without authorization.

## Command-line startup

- `--help`, `-h`, and `--version` work without a display, `HOME`, or XDG directories
  and do not open settings, profiles, credentials, or UI.
- Invalid flags, missing values, and conflicting profile selectors fail before startup.
  Named profiles reject empty names, separators, `.` and `..`.
- `--profile-path PATH` and the legacy positional path select the same profile,
  including relative paths and spaces. `--config FILE` remains independent.
- Demo ignores configuration/profile paths and works without `HOME` or XDG directories.

## Demo checks

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo uses local data without saved accounts or Matrix traffic. Explicit file
pickers/preparation and sample notifications can use desktop services.
Check Light/Dark at 1400×900 and 640×480, with wgpu and tiny-skia where available.
Throughout, retain distinct room/thread drafts and an older reading position.
Check long labels, disabled states, keyboard/IME precedence, and static idle controls.

| Area | Checks |
| --- | --- |
| Navigation | Breadcrumbs/tree, shared/cyclic routes, Home/subspace filters, pickers, sections, back/forward. |
| Timelines | Room/thread resize, pagination, newest following versus older anchors, pending/failed sends. |
| Composition | Plain/Markdown preview, mentions, replies, edits, deletion, selection/copy. |
| Media | Viewer, GIF play/stop, save/open, picker/paste/drop into each pane. |
| Discovery | Search, saved messages, pins, invitations, room/space creation and discovery. |
| Management | Settings, members, avatars, emoji packs, leave/disband cancellation. |

Use the focused [preferences](ui-preferences.md), [management](ui-management.md),
and [discovery/media](ui-discovery.md) checklists for details. Covered conversations
must retain live offsets without accepting input, read receipts, or media demand.
Nested discard/exit confirmations must appear in the topmost active dialog.

## macOS keyboard checks

Run these checks in `--demo` after a locked desktop build. The macOS CI UI suite
exercises bindings and focused widgets without an account; it does not establish
native event delivery, IME behavior, or function-key settings on a physical Mac.
This section is a manual acceptance checklist, not a completed validation record.

- Compare Settings' send option, room/account search, room/space navigation, and
  poll-reorder tooltips with the [platform shortcut tables](../keyboard.md).
  Mac labels use Cmd and Option; Ctrl+Enter remains a send alias.
- Open app commands with Cmd+N/K/P/F/L/T/J and their documented variants from
  room/thread composers, sidebar/filter, breadcrumb, and search. Control-letter
  combinations must not open app commands. Ctrl+Tab/Ctrl+Shift+Tab still switch
  rooms; Cmd+Tab remains available to macOS.
- In a multiline room/thread draft and a topic editor, check word, paragraph,
  line, and document movement/selection, word deletion, and Cmd+Backspace.
  Option+Up/Down and Option+Shift+Up/Down must edit/select within the field without
  navigating rooms. Repeat movement/selection in plain and formatted messages.
  Check the documented Control editing keys, including Ctrl+O's retained cursor
  and Ctrl+K at paragraph end; do not expect kill-ring/yank support.
- Exercise account unread navigation with Cmd+J/Cmd+Shift+J and filtered unread
  navigation with Cmd+Option+J/Cmd+Option+Shift+J. Preserve the selected space,
  both conversation drafts, and older reading positions.
- In a new-poll form, Cmd+Option+Up/Down reorders an answer and retains its focus
  and selection. Option+Up/Down must not reorder. Closing retains the draft.
- In pickers and dialogs, plain arrows and Tab/Shift+Tab act once. Extra modifiers
  must not accidentally select an item, traverse the form, or submit it. An open
  dropdown or active IME composition takes precedence over the surrounding form.
- Check F6/Shift+F6 region traversal, including Fn when required by keyboard
  settings. Confirm keyboard events add no background animation or idle wakeups.

## Shared lifecycle checks

For asynchronous features, close/reopen, navigate, switch accounts, and quit during
reads and writes. Reads cancel; stale results cannot change replacement views.
Admitted writes retain their captured account/target and finish shutdown accounting
once. Errors preserve drafts/forms. Recheck permissions and remote state before
writes; after partial failure or a lost response, let sync settle and Reload before
explicit retry. Matrix changes are not atomic.

## Network proxies

With a dedicated profile and controlled proxy, check the
[proxy environment variables](../settings.md#network-proxies):

- HTTP/HTTPS and SOCKS cover discovery, sign-in, resumed sync, and media.
- `socks5h://` resolves destination DNS remotely; `socks5://` resolves locally.
- Protocol-specific variables override `ALL_PROXY`; `NO_PROXY` bypasses a controlled
  host. Check lowercase forms too.
- An unavailable proxy fails without direct fallback; cancellation stays responsive.
- Browser SSO uses its own proxy configuration and preserves the localhost callback.
  Demo makes no proxy/account requests.

## Desktop login

Use native sliding sync and working Secret Service/Keychain storage. Browser SSO
needs localhost port **8765** reachable from the browser; see
[container setup](../development.md#using-the-host-secret-service-from-the-container).

1. Sign in, open encrypted history, and record account/device identity privately.
2. Quit/resume, including offline cached startup: reuse device/crypto state.
   A second process must fail the profile lock.
3. Cancel during discovery, browser approval, and credential saving. Cleanup must
   finish before retry; authenticated attempts may still finish persistence/sign-in.
   Old submissions must not create another device. Inspect raced SSO in another client.
4. Deny/lock credential access or interrupt login: preserve existing stores/keys.
   Revoke the token, then reauthenticate with fresh device state while retaining
   earlier encrypted stores and recoverable drafts.
5. Sign out: remote revocation precedes draft/credential deletion. Remote failure
   allows cancellation; local cleanup retry must not revoke again. Retained stores
   must not resume the revoked session.

### Registration

Use a disposable account and fresh profile on a server supporting native registration.

- Reject mismatched passwords and full Matrix IDs as usernames. Errors retain
  server/username and clear passwords; switching to sign-in clears secrets too.
- Exercise `m.login.registration_token` directly in the masked field, including
  invalid/expired tokens. Later stages retain the session without resending it.
- Complete CAPTCHA/email/terms in the browser, then Continue. Premature Continue
  retains the unfinished stage; terms require acceptance. No polling.
- Cancel/expire/replace prompts and quit: stale replies cannot advance another
  attempt. The overall deadline is fifteen minutes. Cancellation cannot undo a
  server-created account; after ambiguous creation/save failure, return to sign-in.
- Success opens [recovery setup](recovery.md). Resume reuses account/device/crypto
  identity. Check keyboard/scrolling at 640×480 and settled token/browser-wait states.

OAuth-only account creation requires the homeserver's own page.

### Account profile

Save name/avatar independently and confirm in another client. Check empty/Unicode
names, image removal, remote conflicts, denied/lost responses, and stale completions.
Only explicit Save publishes; retain unrelated edits and require Reload after
uncertain writes. Avatar upload/publication can leave unused media.

## Native macOS account storage

Use the [bundle](../development.md#macos-builds). Check Keychain allow/deny/locked
states, access after rebuilding, offline resume, and cleanup retry. Demo and empty
sign-in must not prompt. Cancellation retains the profile lock until native work ends.
Opt-in Keychain fixture (creates temporary test records):

```sh
cargo test --locked --lib session::credentials::macos::tests::native_keychain_round_trip -- --ignored
```

## Home navigation

- Home shows rooms outside joined spaces; Include subspaces shows all rooms.
  Spaces toggle direct/recursive rooms without duplicates; DMs follow the same rule.
- Retain routes, expanded branches, and per-location toggles across navigation/restart.
  Element's Home setting stays untouched; legacy All rooms becomes recursive Home.
- Check shared children, cycles, removed links, long breadcrumbs, avatar arrival,
  picker rename/removal races, F6, and [space shortcuts](../keyboard.md#space-navigation).
  Include subspaces preserves focus/query and ignores repeated keydown toggles.

### Add and remove space links

- Add lists writable joined parents lacking a direct link; recursive visibility
  does not exclude a valid parent. Reject self/duplicate/known-cycle links.
- Add/remove writes only the parent's `m.space.child` (`via` populated/empty).
  Child metadata, membership, other parents, and the conversation stay intact.
- Demo: link General to Personal; unlink Design studio from Studio then Design.
  It reaches Home after its last link is removed. Add Community as a second parent
  of Design, navigate through it, then unlink: retain Studio's route.
- Recheck permissions/links/routes before submission. Root spaces cannot unlink.
  Unknown unjoined paths and concurrent remote edits cannot be checked atomically;
  inspect the tree after timeouts.

## Navigation and timelines

- Load a long history and check wheel/trackpad, scrollbar, paging, newest, and
  distant reply/search/saved/notification targets. See [performance fixtures](long-history.md).
- Retain the visible event and pixel offset through arrivals, prepends, resize,
  scaling, edits/redactions, media reflow, and thread close/reopen. Read partly above
  the first loaded message, then prepend the same sender/day: regrouped headings
  must not shift its text. Repeat below/across/above the 192-row virtualization threshold.
- Drag/copy selections across unmounted rows in both directions; retain spoiler
  reveals. Overscan/empty virtual space must not count as viewed for receipts,
  highlights, media, or saved reading anchors.
- Quit/reopen older room/thread positions with a changed window size. Restore the
  event-relative offset; unavailable/deleted anchors, keys, or history fall back
  once without retry loops. Navigation during restore rejects stale results.
- Check unread shortcuts in both directions, wrap/no-match cases, collapsed sections,
  and current rooms outside the filter. Account-wide actions preserve the filter;
  scoped actions obey it. Inbox flags do not determine room unread status.
- Exercise [navigation shortcuts](../keyboard.md#navigation) from composer, filter,
  sidebar, breadcrumb, and completion. Dialogs, IME, native pickers, and shutdown
  block covered navigation. Check native macOS Control/Command/Option behavior.
- Back/forward restores room/thread/drafts/positions without changing the space;
  new navigation after Back clears Forward. Sections preserve ordering/metadata
  across clients. Rapid switching/pagination must not retarget late results.
- Explicit event jumps show a static three-second tint/outline after visibility.
  Repeated jumps replace expiry; old callbacks cannot clear a new marker.
  Restored anchors stay unmarked; expiry changes no focus, layout, selection, or scroll.

Account search: combine room, full sender ID, and inclusive UTC dates; reject invalid
IDs/dates and reversed ranges. Filter changes cancel work and clear continuation
until resubmitted. Check cached results, server pages (including empty filtered pages
with continuation), and explicit encrypted-history scans. Verify missing-key errors,
stale pages, and navigation with a second client.

### Links, pills, and permalinks

- Check raw/labelled Matrix destinations, split formatting, RTL/emoji, wrapping,
  spoilers, and selection without activation. Rendering fetches no profiles/events.
  Copy link URL retains the original query; source changes invalidate menus.
- User links reuse a DM or prefill creation; self links do nothing. Known rooms open,
  joined spaces filter, unfamiliar rooms prefill Join without submission. Event
  links load the correct room/thread. Cancelled/replaced Join cannot navigate later.
- Check encoded IDs, `via`, `matrix:`/matrix.to, lookalike hosts, missing keys/access,
  and pending events. Room links prefer a valid primary alias, else ID; message URLs
  use the room ID and exact event. Check native clipboard and another client.

### Deferred composer focus

Open unloaded rooms/threads from search, saved messages, or Ctrl+T, then type after
arrival: focus the destination composer once and retain the old draft. Tab/F6, IME,
filter focus, another dialog/picker, unfocus, or navigation cancels deferred focus.
Dismiss menus/return from covered views and check composer routing. Search selectors,
match arrows, and Close must be keyboard accessible. Native timing/IME needs desktop checks.

## Read receipts and unread clearing

Focused, uncovered newest room panes advance read/fully-read state; older, covered,
or unfocused panes do not clear arrivals. Threads send thread receipts without moving
room fully-read. Check Mark unread, reopen, offline/reconnect, and no recurring retry.

Disable typing during composition: a peer stops seeing it; incoming indicators stay.
Disable public receipts: another own device updates unread state while a peer sees
no new receipt. Re-enable at the same viewport and check public advancement. Test
Save/Cancel/Defaults and account changes; previously sent receipts remain public.

### Unread catch-up divider

Demo Release notes has a fixed New messages boundary. Connected rooms retain the
opening receipt boundary as arrivals/receipts advance; reopening recaptures it.
Do not guess for missing/unloaded receipts, manual unread alone, or outgoing-only
activity. Pagination may reveal it; thread panes have no room divider.

## Draft recovery across restarts

Restore distinct room/thread text, Plain/Markdown mode, emoji/mention bindings,
reply intent, poll drafts, and reading anchors in a dedicated profile. Sent-message
edits retain the original unsent draft; attachments do not persist. Demo stays in memory.
Quit during save: flush the latest snapshot. Unwritable/malformed storage preserves
original data and offers Retry/Keep open/Close without saving. Sign-out deletes drafts
before keys, only after remote success. See [storage boundaries](../architecture/drafts.md).

## Composition and formatting

- Replaced rooms disable both composers, even during loading/errors; retain old
  drafts/attachments. Successor navigation opens or prefills Join. Missing tombstone
  destinations explain the block; remote upgrades reject stale composition.
- Long words/combining Unicode wrap in both composer modes without added line breaks
  or lost selection. Check code, lists, tables, math, links, spoilers, and image
  alternatives in both panes/previews/themes, including bidi and cross-block copy.
- List markers share a line with their first paragraph; nested/loose lists retain
  boundaries. Include `* 12. April`, which contains a nested CommonMark list.
- Highlight declared languages in both themes. Unknown languages, blocks over
  16 KiB, and lines over 2048 bytes stay uncolored but retain preview controls.
  Stale preparation cannot replace newer source. See [code](#long-technical-code-messages)
  and [tables](#markdown-tables).

### Mentions and room links

Accept `@`/`#` completion with arrows/Enter/Tab/mouse without sending. Check duplicate
names, Unicode, label edits/deletion/undo, preview/send/edit, and draft restoration.
Code, raw HTML, URLs, image alternatives, and literal tokens infer no mentions.
Room links never notify; explicit `@room` requires permission and metadata, and any
token edit removes it. Recheck permission at Send. Repeated edits retain surviving
identities, remove deleted represented metadata, and preserve unrepresented legacy metadata.

### Ordinary replies

Reply uses the current composer; Reply in thread opens a thread. Replace/cancel intent
without losing source, bindings, or attachments. Admission clears intent; failure retains it.
Previews reflect loaded edits/redactions without fetching missing context. Activating
one loads/highlights the original; thread replies route to threads, roots to the room.
Check cancellation/missing keys and cross-client relations: room replies have
`m.in_reply_to` without `m.thread`; thread explicit replies retain the root and do not
claim fallback-only status.

### Cancelling queued sends

Queue offline text/media in both panes; Cancel requires confirmation and keeps drafts.
Failed sends offer Retry/Cancel without overlap. Active-upload cancellation prevents
its dependent send while later entries continue; uploaded bytes may remain. In-flight
events may send before redaction, so warn that others can see them. Race sync/completion,
storage failure, navigation, and shutdown; late results cannot change another pane.
Cancellation errors remain dismissible even after the echo disappears. Demo has no real queue.

### Multiple attachments

Pick/paste/drop batches into each pane; append/remove and check order/long names.
Invalid/inaccessible/>50 MiB inputs and cancellation preserve staged files/caption.
Reject late preparation. Send one message per file, caption on first, reply on each.
Only admitted files clear; failure before first admission retains the caption.
Check encrypted/offline delivery, retry, partial admission, and shutdown.

## Polls

Use the [poll controls](../keyboard.md#polls) and a second client:

- Create all four disclosure/single/multi-choice modes in room/thread forms and the
  text-template path. Check 2–20 distinct answers, explicit multi-choice limit,
  independent message/reply/attachment drafts, form close/reopen/restart, offline
  echoes, failure retention, and clearing only the admitted poll draft.
- Reorder by grip and Alt+Up/Down (macOS: Cmd+Option+Up/Down): retain
  focus/selection, order, and settings in sent/restored drafts. Click-only/outside
  drops, Escape, scroll, unfocus, and stale
  forms cancel dragging. Check compact scrolling and dropdown-first Escape.
- Vote/revote up to the limit; pending guards span room/thread views. Hidden counts,
  voter names, bars, and Show votes stay hidden until the SDK end snapshot.
  Hover/popover uses cached names, handles missing users/empty answers, scrolls,
  and dismisses without timers. Redaction uses normal removed-message presentation.
- End only owned polls after explicit confirmation; cancellation sends nothing.
  Recheck ownership/permissions/content and retain the transaction on uncertain retry.
  Disable actions until the SDK end arrives; old confirmations cannot act.
- Edit only owned unanswered/unended polls, including thread roots. SDK eligibility
  applies even when visible counts are zero. Cancel retains other drafts; unchanged
  initial Save sends nothing. Recheck intervening responses/edits/permissions.
  Same normalized retry reuses the transaction; changed content gets a new one.
  Reverting after an uncertain Save still sends replacement. Await the SDK update
  with controls disabled; edits/votes/end cannot overlap across panes.
- The headless `poll_cards_wrap_at_narrow_widths` test checks wrapping at
  380/260/180 px. Inspect poll appearance in both themes in demo mode; the layout
  test does not establish rendered theme appearance.

Local fixtures do not establish encrypted cross-client races or native acceptance.

## Sending and room management

With a second client, check text/emotes/media, offline/retry, encryption, edits,
deletion, reactions, pins, topics, DMs, invitations, and joining by ID/alias/link.
Ctrl+N room entry opens known rooms or prefills Join; cancellation retains conversation.
Exercise bookmarks (including unavailable sources/invalid storage) and emoji packs;
only Save uploads, conflicts require Reload, and media is unencrypted.

### Room and space settings

Save fields independently; Reload/discard/conflicts retain unrelated drafts. Special
access stays read-only; history visibility supplies no old keys. Check notification
overrides/account defaults across clients. For addresses:

- Fixed `#` prefixes do not duplicate pasted hashes. Local aliases use the account's
  server name/port, reject foreign/invalid/duplicate aliases, and expose policy errors.
- Publication validates resolution and fresh permissions, preserves other aliases,
  and never creates/deletes mappings. Directory listing remains independent.
- Removal requires unpublishing and confirmation, then rechecks resolution.
  Denied reads leave other settings usable; uncertain writes require Reload.
- Demo create/publish/main-address/directory/remove persists per target in memory.

### Conversation and space creation

Home creates root spaces; writable spaces create fixed-parent subspaces. Recheck
parent permission before creation. Link failure retries only the created ID's link.
Room/DM creation sends once per submission with a 30-second deadline; inject lost
responses/500/429/stalls and inspect state before retry. DM reuse survives failed
`m.direct` updates. Preserve conversation/filter throughout.

### Members

Check large rosters, search/paging, directory/full-ID invites, pending invitations,
permissions, and confirmed removal/revocation/role changes. Preserve unrelated power
rules; protect self/peers/creator. Space actions affect no children. Use the focused
[moderation/reporting checklist](ui-management.md#basic-moderation).

### Discovering rooms

Check nested/unjoined results, deduplication, search, explicit pagination/errors,
and the 1,000-result cap. Directory queries submit explicitly; tabs retain separate
query/results/continuations and cancel reads when switching.
Link an alias/ID/Matrix URL or discovery result into a writable parent without joining.
Opaque IDs need `via`; writes affect only the parent's child link. Reject known
cycles/duplicates and recheck permission; unknown remote cycles remain possible.
Retain address/query/feedback on errors and across tabs; discard prompts include
unfinished addresses. Accepted writes may complete after closing. Join eligible
rooms with routing hints; joining neither navigates nor adds a link. Open navigates.

### Leave and disband space confirmations

Use disposable spaces: check fresh counts/permissions, exact-name Disband, and no
Enter auto-submit. Changed state requires review. Leave/forget retains children/others.
Disband closes access, unlists, removes memberships/links, verifies, then leaves.
Protected peers/new members/partial failures require Reload/explicit retry; retain
the actor until completion. Failed forget retries only forget. History/aliases/external
references remain; older snapshots cannot restore a disbanded space.

### Room leave consequences

Check encrypted/public, last-admin/inviter/member warnings, and changed-state guards.
Opening/Enter never leaves; failed preview disables Leave with Reload. No forget/key
removal; retain other conversations. Review recovery before destructive tests.

### Room and space avatars

Check PNG/JPEG orientation/WebP, bad/oversized files, stale preparation, and independent
permissions. Only Save uploads; remote changes/permission loss require review/Reload.
Remove clears the reference, not media. Upload/publication is not atomic.

## Inline GIF playback

Play loads/animates the original; other image clicks open the viewer. Loading stays
still/cancellable. Stop/completion permits replay; viewport departure, navigation,
thread close, viewer open, or unfocus stops without auto-resume. Test encrypted GIFs,
both playback settings, errors/retry, and [idle resources](idle-resources.md).

## User presence

Check on-demand refresh, the 60-second cache, optional polling off/on and pause while
unfocused, endpoint failures, and stale results. No second classic-sync stream.

## Lifecycle and desktop integration

Check native identity/icons, file picking, clipboard, pane drop coordinates, external
opening, [notifications](notifications.md), [verification](verification.md), and
[recovery](recovery.md). Include offline/reconnect, hidden windows, suspend/resume,
repeated navigation, released timelines, and [resource measurement](idle-resources.md).

## Validation scope

Automated state/mock-server/crypto fixtures cover guarded requests, cancellation,
stale updates, drafts/anchors, and layout invariants. They and demo captures do not
establish live interoperability, native prompts/fonts/IME/clipboard, macOS, GPU, or
CPU/wakeups. Record build/platform/renderer and observed limits separately; keep
private data and run artifacts outside Git and this reference.

## Device and session management

With an isolated account and spare device, compare Settings → Devices with another
client. Rename both devices; the current one stays labelled and cannot be revoked.
Cancel then confirm spare-device revocation; exercise password/browser/multistage
UIAA and errors, and confirm only that session ends. Unsupported flows must not
bypass authentication. Close/cover/switch during prompts: clear secrets, reject stale
results, and Refresh after uncertain writes. Check compact layout/keyboard controls.

## External Matrix activation and profile selection

Do not change the user's default handler or open real profiles for fixture checks.
Use `--demo --demo-picker` for UI. For isolated IPC/native checks, run the built
library test executable with `--exact activation::tests::fixture_process --nocapture`
and `INTERCONNECT_ACTIVATION_FIXTURE=/tmp/SHORT_PRIVATE_DIR` (0700). It publishes two
fake profiles, `ready.json`, and `received.jsonl`; optional
`INTERCONNECT_ACTIVATION_FIXTURE_SINGLE=work` exposes one. Point only the test client's
`XDG_RUNTIME_DIR` there, using an absolute `WAYLAND_DISPLAY` if needed. Never publish
fixtures in the real activation directory.

- One profile forwards/exits without UI/account storage, including renderer re-exec.
  Multiple profiles show an SDK-free picker; only the chosen endpoint receives.
  Explicit profile plus `--open` bypasses it. Cancel/Escape/close delivers nothing.
- Restart/change/stall the target: fail with Refresh, without another account or
  default-store fallback. Check keyboard and Light/Dark at 640×480 in both renderers.
- With an authorized real profile, defer links through sign-in, restore, dialogs,
  and IME. Preserve drafts/scroll/join confirmation; sign-out clears old-account links.
- On macOS test packaged cold/warm AppleEvents with zero/one/multiple active profiles,
  including a receiver launched with an explicit profile. Welcome retains the URL
  handler before delivery; pending links survive later arrivals.

Focus is compositor-controlled. ACK means bounded in-memory admission, not durable
navigation/server completion. Packages advertise `matrix:` only.

## Long technical code messages

Demo → Message formatting has a 100-line sample. Check collapsed 12-line/2-KiB
budgets, full expansion, released layout on collapse, and uncolored code controls.
HTML beyond parser limits falls back to plain text without previews.

- Copy/Save/Expand corner icons and bottom Expand cue stay clear of message actions
  at narrow widths. Tooltips/focus identify actions; Tab/Shift+Tab skip offscreen
  controls, Enter/Space acts once, Escape releases focus, and source edits clear it.
- Wide code scrolls horizontally by thumb/track/trackpad without moving overlays or
  changing hit-testing. Release/unfocus ends drag even outside the block; vertical
  scrolling stays with the conversation. No settled timer.
- Copy/Save includes full Unicode/whitespace/spoiler placeholders, excluding chrome.
  Check native clipboard, async picker cancellation/errors, and suggested `code.txt`.
- Retain selection, expansion, offsets, and event anchors across resize, size/theme
  changes, room return, and thread reopen/loading. The pinned original stays
  independent of replies. Session state is bounded to 64 conversations and clears
  on account/source changes; restart retains reading anchors, not code controls.

## Markdown tables

Inspect preview/sent/received grids, wrapping, equal widths, tallest-cell row height,
headers, left/center/right alignment, narrow panes, and theme/size reflow. Copy retains
tabs/newlines, empty cells, labels, Unicode, and hidden-spoiler placeholders, excluding
chrome. Check parser limits: 32 columns, nested-table plain fallback, flattened spans.
Confirm outgoing tables in another client.

## Independent message text sizing

Stage/cancel/Save 12/14/16/18 px independently of Interface size; test Reload/defaults
and missing-setting 14 px. Bodies/headings/code/tables/previews change; shell/composer
and authored media keep their own sizes. Retain copy, code state, and anchors in both
panes. See [preference checks](ui-preferences.md#preferences).
