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

See the focused [preference/dialog](ui-preferences.md),
[management](ui-management.md), and [discovery/media](ui-discovery.md) checks.
Also verify the separate sidebar chat/room creation icons, sidebar actions menu,
thread room identity/back action, composer
shortcut hints, and conversation-level retry. Open invitations and sign-out
over an older reading position, then cancel: the same live timeline offset and
draft must remain. Covered conversations must not publish viewport/read demand
or accept focus/input. Check nested forms and application close with unsaved
changes: each confirmation must be visible in the topmost active dialog.

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

## Network proxies

With an authorized dedicated profile and a controlled proxy, use the
[proxy environment variables](../settings.md#network-proxies) before launch:

- Verify HTTP/HTTPS proxying and SOCKS5 proxying cover discovery/sign-in, resumed
  sync, and media downloads. Inspect proxy activity without retaining credentials
  or private message content.
- With `socks5h://`, use a homeserver hostname resolvable only by the proxy and
  verify discovery succeeds. With `socks5://`, destination DNS resolves locally.
- Check `HTTP_PROXY`/`HTTPS_PROXY` override `ALL_PROXY` and `NO_PROXY` bypasses
  the proxy for a controlled host. Check the lowercase environment forms too.
- Make the proxy unavailable: requests must fail without silently connecting
  directly; cancellation and shutdown must remain responsive.
- Configure the browser separately for SSO and preserve the localhost callback.
  Demo must still work without contacting the proxy or opening an account.

These are manual acceptance checks. Dependency-feature inspection and workspace
tests alone do not establish live proxy or DNS behavior.

## Desktop login

During browser/password sign-in, cancel while discovery, browser approval, and
credential persistence are pending. Cancellation must return to the form only
after the session owner acknowledges it. Admitted credential writes must finish;
an already authenticated session must finish persistence and may still sign in.
Check that a retry cannot overlap the previous attempt, and that a queued older
submission cannot create another device. If browser approval raced cancellation,
inspect the dedicated account's devices in another client. Registration remains
subject to its existing partial-account and challenge-cancellation behavior.
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
- Successful registration should open the account and
  [key backup and recovery setup](recovery.md), including on a new verified device.
  Quit/resume the dedicated profile and verify account/device/crypto identity is
  reused. If account creation succeeded but session saving failed or the response
  was lost, return to sign-in rather than creating another account.
- Check scrolling/tab navigation at 640×480 and with long errors. Static form
  and token/browser-wait states must settle without periodic redraws.

OAuth-only/delegated account creation is not implemented by this native
registration form. Use the homeserver's own account creation page, then return
to sign in using its supported method.

### Account profile

Settings → Account loads the signed-in name/avatar. Save each field independently
and confirm it in another client. Check empty/Unicode/long names, avatar selection
and removal, and conflict detection after another device edits the same field.
Only Save uploads/publishes; keep unrelated drafts. Deny/stall/lose a response:
show an error, retain the draft, and require Reload before retrying. Close/reopen
or switch account during reads/writes; no stale result may replace another form.
Avatar upload/publication is not atomic and may leave unused media.

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
- For existing subspaces, navigate to Design and use **Sidebar actions → Add this
  space to another space…** to add Community as a second parent. Studio → Product
  must remain linked. Navigate through Community → Design, then **Remove this
  subspace from parent**; only Community’s direct link is removed. The breadcrumb
  finds another route and the current conversation, draft, and scroll remain.
- Self/duplicate links and descendants that would introduce a cycle are excluded.
  Shared parents remain eligible; permission loss, changed links, account changes,
  and route changes reject stale operations. Root spaces have no unlink action.
  Graph checking uses known joined-space state, not an unjoined hierarchy crawl,
  and cannot make remote concurrent edits atomic. Check the tree after timeouts.

Mock homeserver and UI tests cover joined subspaces, parent-only add/remove writes,
shared-parent preservation, duplicates, self/cycle rejection, permissions, stale
routes/completions, and conversation/draft/scroll preservation. Connected server
writes and native Light/Dark/compact acceptance remain manual with an authorized
test account; these automated fixtures do not establish that acceptance.

## Navigation and timelines

Long loaded room/thread timelines mount only the viewport plus overscan; short
histories retain exact full layout. Automated fixtures exercise prefix-height
updates, bounded mounting, offscreen selection/copy/spoilers, deep targets,
prepending, row-height/width changes, newest navigation, cache release, and a
settled widget without redraw/layout deadlines. The headless performance fixture
reports mounted versus loaded counts; see [long-history measurements](long-history.md).

For connected desktop acceptance, load a long test history and check wheel,
trackpad, scrollbar dragging, paging, jump-to-newest, and distant reply/search/
saved/notification targets. Unknown row heights can adjust scrollbar geometry,
but the visible event and its offset must remain stable through switching rooms,
closing/reopening threads, pagination,
resize/interface scaling, edits/redactions, media dimensions, and moving thread
summaries. In particular, read the first loaded message with its text
partly above the viewport, then fetch an older page from the same sender/day.
The date divider and sender heading may regroup, but the text must stay at the
same pixel offset. Repeat across the 192-row virtualization threshold and in a
thread. The automated room fixture covers short histories, the threshold
transition, and already virtualized histories; connected/native checks remain
manual. Drag a selection across multiple virtual windows in both directions,
then copy after both endpoints leave the viewport. Revealed spoilers stay revealed;
hidden ones copy placeholders. Covering/unfocusing panes must retain existing
read-receipt/media suppression: overscan never counts as actually viewed.
After a scrollbar jump beyond the mounted window, reading-position reports must
wait for an actually visible event. Close a thread over an older room position:
the room must retain its event-relative offset as it widens, with newest-following
checked separately. Automated regressions cover the transient offscreen-report
case and event-anchor restoration on thread close; native reflow remains a desktop
acceptance check.

Native GPU latency, IME/focus, very long individual messages, and live homeserver
interoperability remain manual checks. Loaded SDK history has no new retention cap.

- Use Alt+Shift+Up/Down for unread rooms within the current space/text filter;
  add Ctrl (Command on macOS) to search the account. Check both directions,
  wrapping, collapsed sections, a current room outside the filter, and no other
  unread room. Preserve space/filter, room/thread drafts, saved positions, and
  back/forward history. Modals, IME, pending native pickers, and shutdown must
  prevent navigation. Automated demo tests cover ordering, wrapping, guards,
  history, and scope; live SDK unread changes and OS shortcut conflicts remain
  desktop checks. Notification-inbox read flags do not determine this action.

- In account-wide search, combine a joined room, full Matrix sender ID, and
  From/Through dates (inclusive UTC days). Check duplicate sender display names,
  midnight boundaries, malformed dates/user IDs, and reversed ranges. Changing
  any filter cancels local/server/history work, clears results and continuation,
  and requires a new submission; late pages must not restore old results.
  Verify filtered results and navigation in demo without network access.
- Repeat against a test homeserver with local cached messages, server pagination,
  and explicit older encrypted history. Room/sender restrictions are sent to
  server search; dates are checked on returned events, so a page can have no
  matches and still offer Load more. History scans only the selected encrypted
  room, if any; unavailable keys retain the ordinary scan diagnostics.
  Automated fixtures cover shared filter semantics, server request fields,
  history filtering, stale pages, and local filtering before the result cap.
  Live interoperability and desktop dropdown/focus behavior remain manual.

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

Reading anchors persist encrypted per room/thread; loaded history can grow memory
without virtualization. Quit/reopen over older room and thread messages, including
a changed window size: restore the same event and its viewport offset. Check
newest-following separately. Unavailable/deleted events, missing keys, and offline
history failures must fall back to the ordinary timeline without a retry loop.
Switch rooms or follow another event while restoration is loading: stale results
must not move the replacement conversation. These connected restore checks remain
manual; automated tests cover encrypted snapshots, anchor layout, and fallback state.

Back/forward history has automated coverage for nested navigation, room/thread
selection, draft/offset preservation, missing rooms, account changes, and bounded
stacks. In a connected profile, follow reply/search/saved-message destinations,
then go back and forward after history loads: check event-relative restoration,
thread composer focus, and unchanged space filter. Desktop acceptance remains
manual. Confirm new navigation after Back clears Forward and dialogs/IME prevent
history shortcuts from changing the covered conversation.

Explicit message jumps from reply previews, message/file permalinks, notifications,
saved messages, search results, and pins use the same static tint/outline. Check
both room and thread panes, text and attachment-only messages, and light/dark
appearance. The marker lasts three seconds after the loaded target first becomes
visible; it does not animate. Repeated jumps replace the prior expiry, and obsolete
callbacks cannot clear a newer target. Returning to a saved reading anchor does
not start a marker. Expiry must preserve layout, drafts, text selection, and scroll.
Automated tests cover target routing, delayed visibility/loading, stale expiry,
light/dark rendered pixels with stable geometry, and file-message link styling.
Desktop acceptance and connected unavailable-key/history cases remain manual.

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
- Connected **Copy room link** prefers the primary room alias, falling back to the
  room ID when no valid alias is set. **Copy room alias** appears with a valid
  primary alias; **Copy room ID** always copies the internal ID. **Copy message
  URL** uses the room ID and exact event, including thread replies. Synthetic demo
  IDs and pending events without server IDs omit link actions. Unit tests cover
  alias preference, ID fallback, and preserving conversation state. Check native
  clipboard and opening in another client manually.

### Deferred composer focus

- Follow account-search and saved-message results to an unloaded room. Wait for
  its matching pane and type: the destination composer must receive input once,
  preserving the old draft and scroll position. Repeat after returning from
  Search, Saved messages, or Notifications while the original pane is loading.
  Tab/F6, IME, another dialog/native picker, unfocus, and navigation must cancel
  delayed focus. Room settings and draft-close/sign-out prompts also cover focus.
- Tab from account-search input through Search to the room selector; use arrows
  to change its selection. In-room search exposes Previous, Next, and Close to
  keyboard traversal. Existing widget typing tests plus new delayed-result,
  cover-return, interruption, and dropdown tests exercise the automated path;
  actual desktop/IME timing and macOS behavior remain manual.

Open a thread with Alt+T while its messages are still loading, then type after
arrival: input must reach the thread composer and leave the room draft intact.
Before arrival, switch rooms, close the thread, press F6/Tab, focus the room filter,
open Settings, or unfocus the window: the late pane must not take focus back.
Close Saved messages and dismiss room/thread message menus, then type: the
corresponding composer should receive input. Automated tests cover delayed pane
focus and interruption routing; connected latency and desktop focus remain manual.

## Read receipts and unread clearing

Focused, uncovered newest room panes send read/fully-read updates (public by
default, private when Privacy → Share read receipts is off). Older,
unfocused, or covered panes must not clear new activity. Threads update their
receipt without advancing room fully-read; reader labels are room-scoped.
Test arrivals, manual Mark unread, reopen, and offline failure/recovery without a
recurring retry timer.

Privacy automation uses mock homeservers: default/partial TOML and round-trip
persistence; Save/Cancel/Defaults and demo isolation; disabling active outgoing
typing; unchanged-event public/private mode changes; private thread scope and
preserved room markers. No live account writes are part of these tests.

Connected acceptance remains manual with an authorized isolated test account:
turn typing off while composing and confirm a peer stops seeing the indicator;
ensure incoming indicators still appear. With sharing off, read room/thread
messages and check unread counts on another own device, while a peer sees no new
public receipt. Restore sharing at the same viewport and check the public receipt
advances. Check Save/Cancel/Defaults, account changes, loss of focus, offline
errors, and cancellation during navigation. Existing public receipts remain.

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
do not persist; event-anchored reading positions share encrypted draft storage.
Demo drafts and reading positions stay in memory.

Quit during a save and confirm the newest snapshot flushes. With an unwritable or
malformed store, check Retry, Keep open, and explicit Close without saving while
preserving invalid data. Successful sign-out removes drafts before keys; failed
remote sign-out retains them. See [storage boundaries](../architecture/drafts.md).

## Composition and formatting

- Replaced rooms show a persistent notice in place of both room and thread
  composers, including during loading, pagination failure, or timeline setup
  failure. Existing drafts, attachments, and reading positions remain associated
  with the old conversation. Space filtering must not switch the conversation.
  Following the replacement opens a joined room or offers the ordinary Join form
  with federation routing hints. Dismissing it must leave the old room open.
  Redacted tombstones without a destination still disable composition and explain
  that the replacement is unavailable. Remote upgrades stop typing/completion and
  reject stale composer actions; already admitted writes may finish.
- In room and thread composers, type/paste words wider than the field, including
  long identifiers and combining Unicode characters, in Plain text and Markdown
  modes. They should wrap within the field; resize narrower/wider, scroll through
  the draft, and select/copy it without losing text or adding hard line breaks.
- Toggle themes with pending drafts/older positions. Check Save/Cancel/Defaults,
  persistence/Reload in a test profile, legacy missing theme, and invalid theme
  retaining the last valid settings. Demo never writes settings.
- Inspect code, lists, links, spoilers, math, images/placeholders, mentions, and
  selection in room/thread messages and previews. Test Unicode/bidi and copy
  across runs/blocks; hidden spoilers copy placeholders.
- Preview Markdown lists using `*`, `-`, `+`, and numbered markers, both with
  consecutive items and blank lines between them. Each marker should share a
  line with its first paragraph; continuation paragraphs and nested lists keep
  their block boundaries. Automated composition/document tests cover these
  structures and received HTML; desktop rendering still needs manual inspection.
  Include date-like items such as `* 12. April`: CommonMark treats the number as
  a nested list marker, which should remain on the same line as the bullet.
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
- Activate received reply previews with mouse or keyboard. Loaded originals and
  older unavailable previews should navigate to the original with a highlighted
  row. Known thread replies open in their thread; a reply to its thread root
  opens the room original. Missing originals/keys and timeout show an error;
  leaving the conversation cancels loading. Verify drafts remain intact and old
  source generations/changed reply targets cannot navigate. Demo exercises local
  routing only; remote history, encryption, and cancellation need connected checks.
- Connected text/attachment sends carry `m.in_reply_to.event_id`. Room composer
  replies have no `m.thread`; thread replies retain the root with explicit-reply
  `is_falling_back: false` (or omitted). Check mentions, local echo, encryption,
  retry, and the other client's rendering.

### Cancelling queued sends

- Queue text and attachments while offline, including in a thread. Cancel send…
  opens an inline confirmation without changing drafts. Keep sending retains the
  queue entry; Confirm removes the unsent echo. Failed sends offer both Retry and
  Cancel, with overlapping retry/cancellation disabled.
- Cancel while an upload is active: its dependent media message must not be sent;
  subsequent queued messages must continue. Already uploaded bytes may remain on
  the server. If an event send is already in flight, the SDK may finish sending
  then redact it; confirmation must explain that others may see it first.
- Race sync/send completion with confirmation. Sent rows offer Delete message,
  and cancellation that arrives too late reports that the message left the queue.
  Check storage errors, room/thread switches, account changes, and shutdown during
  cancellation. Old results cannot alter another account or any composer draft.
  Cancellation errors stay in a dismissible footer through later SDK snapshots,
  including when the local echo has disappeared or navigation is hidden.
- Automated local-server tests cover pending/failed text, active upload abort,
  already-sent handles, and the in-flight-send/redaction race. Desktop offline,
  encrypted media, restart, and homeserver visibility remain connected checks;
  demo has no real send queue.

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
- In Ctrl+N / Command+N, enter a room alias, ID, or Matrix room link: Enter or
  **Open or join room…** opens a joined room or prefills Join without joining.
  Check malformed addresses, routing hints, cancellation, switching back to people
  search, and late people results after entering a room. Preserve conversation,
  drafts, and scroll positions when cancelling.
- Save/remove/page bookmarks and open originals, including unavailable encrypted
  sources and invalid storage. Manage personal/room emoji packs; only Save uploads,
  conflicts require Reload, removal keeps media, and uploads are unencrypted.

### Room and space settings

Save name/topic/access/history independently; check per-target demo state, discard
unsaved edits on close, and Reload/conflict/permission errors. Special access rules
stay read-only. Room notification overrides sync across clients; returning to
account defaults works. History visibility must not promise old keys. Hidden
conversation panes must not issue read receipts or demand visible-message media.

Room settings also manage **Published Addresses** and **Local Addresses**:

- Both address inputs show a fixed `#` prefix outside the text box, with no hash
  in the placeholder. Typing or pasting a leading hash must not duplicate it.
- Add a local address using a localpart or full alias. The suffix comes from the
  account's Matrix server name, including any port, rather than its API URL.
  Reject foreign-server aliases, empty/invalid input, and duplicates. Check an
  occupied address, server policy denial, and offline failure with a test server.
- Publish an existing alias, choose/clear the main address, and unpublish it.
  Publishing resolves every retained address to this room and checks fresh
  canonical-alias permissions/state before writing. Preserve other published
  aliases when changing the main address. An alias on another server must first
  be created there; publishing never creates or deletes an alias mapping.
- Toggle this account server's public directory listing independently of join
  rules, history visibility, and aliases. Server policy authorizes directory and
  local-alias writes; canonical-alias power levels authorize publication.
- Unpublish a local address before removing it. Removal requires an explicit
  confirmation explaining that existing links will stop working. Cancel must not
  delete anything. Recheck that the alias still resolves to this room before
  deletion. Matrix offers no compare-and-swap for alias mappings or room state;
  changes between the check and write remain possible.
- Deny alias/directory reads: unavailable controls show an error while other room
  settings remain usable. Close/switch during reads and saves; late results must
  not replace another panel. Keep unrelated unsaved settings and conversation
  drafts/scroll positions. Address writes have a 30-second deadline and disable
  automatic retries; after an uncertain response, Reload before retrying.
- Demo: add `friendly` locally, publish it, select it as the main address, toggle
  the directory, and reopen. Changes persist per room for the demo session only.
  Unpublish, then confirm local removal. Demo rejects publishing an address that
  has not been created in its in-memory local list and performs no network calls.

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

## Device and session management

Automated mock-homeserver tests cover own-device listing, current-device labels,
rename validation and exact request bodies, current-device revocation rejection,
password/bad-password UIAA, multi-stage browser fallback acknowledgements, bounded
dummy authentication, challenge identity/target binding, and fixed error messages.
UI tests cover explicit target confirmation, demo isolation, preserved conversation
and drafts/scroll, stale prompts/accounts/completions, covered/closed prompt cleanup,
write bookkeeping, refresh after ambiguous failures, and secret Debug redaction.
These tests do not make real account changes or launch a browser.

Manual acceptance needs an authorized isolated account with a spare device:

- Open Settings → Devices and compare names/IDs/last-seen times with another client.
  Verify the current device is labelled and cannot be revoked. Rename it and a
  spare device; Refresh and another client should show the new names.
- Cancel a spare-device revocation; it must remain signed in. Confirm it, exercise
  the server's password or browser UIAA (including rejection/retry), then verify
  only that device loses its session. Keep this client's session and conversation.
- Check a multi-stage flow if supported. UIAA fallback pages are server-dependent;
  OAuth-native approvals are not implemented. Unsupported flows must not bypass
  server authentication, and another client may be required.
- Close/switch/cover the panel during prompts and pending requests, then reopen and
  Refresh. Passwords/prompts must clear; admitted requests may already have applied.
  Check offline/errors and account changes without publishing stale results.
- Check Light/Dark, keyboard traversal, long device names/IDs, and 640×480 layout.
  Native acceptance and real cross-device/server behavior remain manual.

## External Matrix activation and profile selection

The package advertises `matrix:` only; ordinary HTTPS ownership is unchanged.
Do not change the user's default handler or open real account profiles for these
tests. Use demo for UI and private fake endpoints for IPC/native acceptance.

Automated tests cover validated/case-normalized CLI links, explicit profile
selection, zero/one/many active-profile routing, actual Unix-socket delivery and
deduplication, queue/frame limits, stale process/account identity, endpoint locks
and permissions, stalled live owners versus stale sockets, startup/sign-in and
covered-dialog deferral, preserved drafts/scroll/event focus, shutdown rejection,
sign-out ownership, and stale picker completions. Native queue admission and
picker exit are atomic; accepted links are retained in the bounded source queue
until the UI can route them. No account store is opened by discovery or picker.

For an isolated native fixture, run the built library test executable with
`--exact activation::tests::fixture_process --nocapture` and
`INTERCONNECT_ACTIVATION_FIXTURE=/tmp/SHORT_PRIVATE_DIR` (mode 0700). The helper
publishes two fake profiles under that directory, writes `ready.json`, and records
fake deliveries in `received.jsonl`. Optional
`INTERCONNECT_ACTIVATION_FIXTURE_SINGLE=work` exposes one fake profile. Set only the
test client's `XDG_RUNTIME_DIR` to the fixture directory, with an absolute
`WAYLAND_DISPLAY` if needed, so it cannot discover the real client's sockets.
The fixture creates no profile/account directories. Never publish fake profiles
in the user's real activation directory. Keep all artifacts outside the repo.

- One fake profile: opening a link must forward and exit before creating a UI or
  account store. Test with renderer re-exec variables absent as well.
- Two profiles: the SDK-free picker shows both account identities, supports
  Tab/Shift+Tab, Enter, Escape/Cancel, refresh, and 640×480 layout. Choosing one
  must deliver only there. An explicit `--profile-path ... --open ...` bypasses it.
- Close/restart/change the selected fake target before choosing; the picker must
  report failure and require refresh, without choosing another account. An
  unresponsive locked endpoint must not be interpreted as zero active profiles.
- Inspect Light/Dark and GPU/software rendering. With a real authorized test
  profile, hold a link through sign-in, settings/edit/confirmation dialogs, IME
  composition, and account restoration. It must navigate when safe, retaining
  drafts/scroll and join confirmation. Signing out cancels previous-account links.
- On macOS, test installed bundle cold and warm URL delivery with zero, one, and
  several active profiles, including a receiver originally launched explicitly
  for another profile. The no-URI welcome must retain the native handler until
  GURL delivery; pending links must not be replaced by later URLs. macOS bridge
  sources were compiled/linted in an isolated aarch64-apple-darwin harness on
  Linux, which does not establish native packaged AppleEvent acceptance.

Focus/attention is requested, but compositor policies (especially Wayland) decide
whether a background window can be raised. Activation ACKs indicate bounded
in-memory admission, not durable navigation or server-side completion.

Linux/Wayland native acceptance for this change used the real locked debug
binary, an on-screen nested Sway desktop, and isolated fake profile endpoints.
Intel ARL IntegratedGpu / Vulkan / Mesa 25.0.7 and forced tiny-skia both rendered
the 640×480 picker cleanly in Light/Dark. GPU Tab/Enter delivered only to Work;
software mouse selection delivered only to Personal, while Cancel,
Escape, and native window close delivered nothing and exited. Refresh sent no
link. Killing the fake endpoints produced a visible failure, then an empty
refreshed list without automatic default-profile/store creation. Single-profile
handoff, renderer re-exec, explicit-profile bypass, focus-only launch, and invalid
URI diagnostic redaction passed without a display or account initialization.
The software picker added 0.0 procfs CPU seconds over a five-second idle sample
(quantized measurement; scheduler wakeups were not measured).

These native checks did not touch saved accounts, send Matrix messages, change
the default handler, or establish live-server joins/sign-in. Packaged macOS
AppleEvents, fresh default-profile credential prompts, and OS handler selection
remain native/manual acceptance. Run artifacts are outside version control:
`/tmp/interconnect-activation-native-results.json` and the corresponding
`interconnect-activation-wgpu-*` / `interconnect-activation-tiny-skia-*` captures.
