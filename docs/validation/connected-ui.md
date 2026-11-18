# UI and account validation

Use the [development checks](../development.md#build-and-checks), including a
locked desktop build, before desktop validation. Run UI tests with
`cargo test --locked --bin interconnect`; backend changes need the workspace suite.
The checks below are acceptance procedures, not a history of completed runs.

## Demo checks

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo uses local sample data without saved credentials, account stores, or Matrix
traffic. Explicit file pickers, local attachment preparation, and the Settings
sample notification may use native desktop services. No demo action sends a
Matrix message or uploads media.

Check at 1400×900, a narrower window, and the minimum 640×480 size. Verify both
wgpu and tiny-skia where available; a headless software-rendered capture does not
establish native GPU, macOS, font, or window-manager behavior.

| Area | Check |
| --- | --- |
| Navigation | Home precedes All with a house icon; spaces and Direct filter without changing the open conversation. |
| Sections | Favorites, Unread, custom sections, collapse/order, and room filter compose correctly. |
| Timelines | Room/thread resize, history loading, newest/older positions, send status, retry, and Unicode emoji remain usable. |
| Composition | Separate room/thread drafts, Plain/Markdown, preview, math, custom emoji completion, edit/cancel, and confirmed deletion. |
| Media | Inline dimensions, image viewer, explicit GIF playback, save/open, picker, paste, and drops into the intended pane. |
| Discovery | Quick switcher, people/member list, topics, pins, invitations, join/create/leave, saved messages, and both searches. |
| Dialogs | Settings, verification, and account actions preserve conversation/drafts; long content scrolls while actions stay reachable. |
| Keyboard | Exercise the [shortcut reference](../keyboard.md), including completion and dialog precedence. |

Screenshots in the README use only demo data. Capture an ordinary conversation
with a thread and a separate account-wide search view; keep window size and
renderer consistent, and check the exported images for clipping or private data.

### Received mentions

In the demo's **Message formatting** room, inspect the user mentions at wide and
narrow widths. Check soft accent backgrounds, stronger labels, emoji, long labels,
and RTL text. Drag/copy across mentions and adjacent text; the clipboard should
contain the displayed labels. Hover to inspect the destination. Hidden spoiler
mentions must stay concealed until revealed. Plain `@maya`/`@room` remain literal.
Click Maya's mention to open the existing demo DM. Click Jules's mention to open
a prefilled direct-chat dialog; Escape must leave the conversation and drafts
intact, while **Open chat** or Enter creates a memory-only demo DM. A subsequent
click reopens that DM. Dragging over a mention must select text without opening
anything. Check room/thread drafts and scroll positions survive navigation.

Automated fixtures cover Matrix user URI recognition, spoofed/unrelated links,
label/style preservation, spoiler privacy, wrapping, hit testing, activation,
selection drags, DM reuse, creation confirmation/dismissal, modal/lifecycle guards,
and stable layout after drawing. Desktop appearance and native system-opener
behavior still require manual acceptance; do not send connected test messages without
authorization.

### Room links and completion

In demo mode, type `#gen` in both a room composer and a thread reply. Check the
joined-room suggestions, including alias details, then accept with Enter, Tab,
and mouse. Acceptance inserts a teal `#General` label without sending or
navigating. Search by canonical alias too (`#general:example.org`). Completion
uses the existing room snapshot without member loads or network searches.

Shorten a room label, delete it entirely, undo, and switch conversations. Check
that each surviving label keeps its original room target in preview and after
editing a sent demo message, in both composition modes. Check mixed user and
room labels with the same visible text. Verify saved drafts retain both kinds
of label after restarting the demo with a disposable draft store. Existing user
draft bindings remain compatible. Room links must not introduce user IDs or a
room-wide notification flag into `m.mentions`.

In **Message formatting**, click the room ID and canonical alias links to open
General inside the app. Dragging must select text without navigating. The
unknown room link must open a prefilled Join dialog without submitting a join;
Cancel must retain the conversation draft. Check narrow wrapping, RTL text,
spoilers, and copying visible labels. Event permalinks remain ordinary links.
Only joined rooms in the current room snapshot are offered. Server-dependent
joining is a separate, authorized connected check.

### Room-wide mentions

In demo mode, type `@ro` in a room composer and a thread reply. Verify the
**@room — Everyone in this room** choice. Enter, Tab, and mouse acceptance
insert an amber `@room` without sending. Preview and local demo Send retain the
amber background; clicks must not navigate or open a DM. The demo's **Release
notes** room denies room notification permission and must omit that choice.

Edit a local message containing user mentions, room links, and `@room`. Check
that saving and editing again preserve all three. Remove the `@room` token and
verify preview drops its amber styling and the prepared notification flag.
Changing part of the token also removes its binding. Check room/thread draft
switches and encrypted draft restart with a disposable store. Keep unselected
literal `@room`, longer words, code, links, and image alternatives unbound.
In **Message formatting**, compare ordinary `@room` text with the metadata-backed
room-wide fixture; check wrapping, RTL text, copying, and concealed spoilers.

Automated fixtures cover completion permission changes while open, plaintext and
HTML received styling, metadata-aware document reuse, payload flags, edit
addition/removal without losing unrelated user IDs or legacy metadata, and
backend rejection after a power-level downgrade. Real homeserver delivery and
recipient push behavior remain separate authorized connected checks.

### User mention completion

In demo mode, type `@ma` in a room composer and in a thread composer. Check the
popup shows Maya's name and full ID. Use Up/Down, Enter, Tab, and mouse selection;
accepting must insert the full display name without sending. The name should
have a soft blue background and underline immediately in the composer. Delete only the
surname and confirm the surviving first name remains a mention in preview and
local demo Send. Repeat with duplicate names and Unicode/emoji display names. Escape and outside clicks dismiss
completion without losing drafts or closing the thread. Type an email address or
URL containing `@`; neither should open suggestions. Check IME, focus changes,
rapid room/thread navigation, both composers, and narrow popup placement.

Wrap a long name, scroll the composer, and try RTL/emoji labels: the decoration
must follow the name and stay inside the text viewport without shifting text or
caret positions. Select the entire name and press Delete/Backspace, then try
character-by-character deletion. Both must remove the visual indication and
binding; preview/send must not retain a deleted mention. Ordinary unbound names
stay unhighlighted. Existing selection, word deletion, and whole-draft deletion
must remain usable.

Preview a chosen mention in both Markdown and Plain text modes: it must use the
same mention styling and chosen name as a received user mention. Demo Send
is local; verify it renders the same way. Put a chosen name in code or a Markdown
link and verify it stays literal there. Replacing the entire name with unrelated
text must remove the binding. Restore encrypted room/thread drafts in a
dedicated test profile and verify selected identities and shortened labels survive restart.

While editing a room message and a thread reply, add a user with completion and
preview/save. Existing full-ID and display-name mentions must retain their labels
and targets, including duplicate display names for different IDs. Repeat the edit,
switch Plain/Markdown, and shorten a mention label without changing its target.
Plain text messages with literal Markdown delimiters around mentions must keep
them literal when reopened. Cancel must restore the original unsent draft and its
mention selections. For
server-dependent acceptance, verify `m.new_content` keeps old mentions and adds
the newly selected user. Remove the last occurrence of one existing mention and
save again: its user ID must leave `m.new_content.m.mentions` while surviving
mentions remain. Do not send unrelated messages.

Automated fixtures cover trigger boundaries, ranking/duplicate names, replacement
at the caret, keyboard and stale-event handling, selection persistence, payload
HTML/notification metadata, composer raster/geometry and deletion checks,
edit/save/cancel and repeat-edit preservation, literal
contexts, and joined-member loading. Real
homeserver notification delivery and native IME behavior require connected/native
acceptance; do not send connected test messages without authorization.

## Desktop login

Use a dedicated test account/profile and a homeserver with native sliding sync.
Confirm native credential access before password/SSO login. Browser SSO listens
on localhost **8765**; containers need port forwarding to the browser's host.
See [profile paths](../settings.md#account-profiles) and
[container setup](../development.md#using-the-host-secret-service-from-the-container).

1. Log in, record the account/device identity, and open an encrypted conversation.
2. Quit and resume the same profile. Confirm the device identity and crypto store
   are reused, including cached startup while offline.
3. Deny/lock credential access and test interrupted login. Errors must preserve
   existing stores and credentials; a competing process must fail the profile lock.
4. Test revoked-token handling. Explicit reauthentication creates fresh device
   state while retaining previous encrypted stores and recoverable text drafts.
5. Sign out through the account menu. Confirmation must stop session work and
   revoke the server session before removing local drafts/credentials. Remote
   failure permits cancellation; local cleanup failure offers retry without
   repeating revocation. Retained SDK stores must not resume a revoked session.

Do not restart the user's connected client, revoke its session, or send test
messages without authorization. Use disposable accounts for destructive checks.

## Native macOS account storage

Build and launch the [native bundle](../development.md#macos-builds). Check first
login, Keychain allow/deny/locked states, access after rebuilding the app, quit and
resume, offline cached identity, sign-out, and retry after local cleanup failure.
No Keychain prompt should be required merely to open demo mode or an empty login
form. Cancelling UI work must not release the profile lock while native credential
work is still running. Keep private credential details out of validation artifacts.

The isolated native fixture is opt-in and creates temporary test records:

```sh
cargo test --locked --lib session::credentials::macos::tests::native_keychain_round_trip -- --ignored
```

Run it on macOS. Linux credential fixtures use a private Secret Service and do
not establish native Keychain prompts or policy. CI lifecycle tests avoid Keychain.

## Home navigation

Compare the same account with Element Web. By default, Home includes joined
rooms outside joined spaces and all DMs, including DMs belonging to a space.
`Spaces.allRoomsInHome = true` in `im.vector.web.settings` makes Home include all
rooms. All remains unfiltered. Check missing/updated space-child state, membership
changes, and account-data updates. Switching scopes must preserve the open room,
thread, draft, and reading position; restoring a profile must retain its scope.

## Navigation and timelines

- Rapidly switch rooms/threads, then paginate or send. Late results must not
  replace the newly selected pane or act on another conversation.
- Return to an older position and confirm it stays anchored when new events
  arrive. Newest-following panes should continue following new events.
- Load multiple history pages, resize panes, hide/show the room list, and switch
  spaces. The open conversation must retain selection, drafts, and reading position.
- Check encrypted history with available and missing keys, reconnect, and retry.
  Historical gaps and unavailable keys are distinct from device trust failure.
- Create/rename/remove custom sections and reorder by handle/menu. Compare with
  Element, including empty sections, duplicate labels, and sections hidden by a filter.
- Open saved messages and search results from another room/thread; return using
  Back/Escape. Loading an original event may need intervening history and must
  remain cancellable. Test removed/inaccessible saved sources and storage errors.

Reading positions are per-session. Timeline memory grows with loaded history;
these checks do not establish virtualization or complete offline history.

## Read receipts and unread clearing

A focused, unobscured newest room pane sends public read and fully-read updates.
A pane reading older history must not clear newly arrived unread activity.
Thread reading updates the thread receipt without advancing the room's fully-read
marker. Incoming reader labels show room receipts, not thread-specific readers.

Check focused/unfocused windows, covered panes, old/newest positions, thread open
and closed, and incoming events while scrolled up. Exercise manual **Mark unread**
and reopening the room. Offline failures must remain visible/retryable through
later activity without a recurring retry timer. Arrival alone must not imply read.

## Draft recovery across restarts

Use an isolated connected profile. Create distinct room/thread drafts with
Markdown/plain mode and explicit emoji choices; restart and verify restoration.
Editing a sent message must preserve the original unsent draft on cancel/restart.
Attachments and scroll positions do not persist. Demo drafts stay in memory.

Quit while a save is pending; the newest snapshot should flush. With a controlled
unwritable/malformed draft store, verify the error, Retry, Keep open, and explicit
Close without saving. Preserve the invalid file. Sign-out must remove drafts
before deleting their keys; failed remote sign-out must retain them. See
[draft storage](../architecture/drafts.md) for encryption and durability boundaries.

## Sending and room management

Use a second client and dedicated rooms for server-dependent checks:

- Send text/emotes and attachments in rooms/threads; test offline queue, retry,
  encryption, duplicate prevention, and closing during pending work.
- Preview Markdown/math, send plain text, edit an owned message, cancel an edit,
  and confirm deletion. Permission changes must reject unauthorized writes.
- React, pin/unpin, update a topic, and invite a member. Verify resulting state in
  the other client and handle server failures without discarding current drafts.
- Start a DM, create a room, join by ID/alias/Matrix link, accept/decline an invite,
  and confirm leaving. Failed join/leave must not remove another open conversation.
- Save/remove a bookmark, page through saved messages, and navigate to originals.
  Test encrypted/inaccessible sources and malformed/shared bookmark storage.
- Manage personal/room emoji packs, prepare a local image, Save, and resolve a
  conflicting external edit with Reload. Removing references does not delete media;
  emoji uploads are unencrypted even in encrypted rooms.

## User presence

Presence is fetched on demand for visible senders, DM contacts, and People,
using a 60-second cache and bounded requests. Optional periodic refresh defaults
off and pauses when the navigator is unfocused. Verify disabled/enabled settings,
manual refresh, unavailable endpoints, stale results after navigation, and no
classic-sync connection. Presence failures must not block conversation use.

## Lifecycle and desktop integration

Check close during login, pagination, upload, bookmark navigation, verification,
and draft saves. Repeat navigation and inspect released timelines using the
[idle-resource procedure](idle-resources.md). Include offline/reconnect, hidden
windows, suspend/resume, and rapid dialog open/close.

On native desktops, check icons/app identity, file picker permissions, native
clipboard formats, drag coordinates for room/thread targets, external attachment
opening, [notifications](notifications.md), and [verification](verification.md).
Host-selected file paths must be mounted/accessible in container runs.

## Validation scope

Automated fixtures exercise UI state, local mock homeservers, crypto/session
lifecycle, and isolated native-service protocols. Demo screenshots exercise sample
layout. Neither establishes real homeserver interoperability, native prompts,
GPU behavior, or idle resource use. Record platform, build, renderer, and observed
failures with any acceptance result; keep run logs and private artifacts out of
this reference.

## Conversation creation failures and cancellation

Room and DM creation send `createRoom` once per explicit submission, with a
30-second request timeout and no automatic replay, including on server errors or
rate limits. Other SDK requests retain their existing retry policy. Rooms become
available to SDK views through sync; a confirmed ID is shown provisionally in the
room list. DM IDs are cached before updating `m.direct` to prevent recreation if
that follow-up fails or is cancelled.

Using an authorized test account and a controlled homeserver/proxy, exercise a
failed response after accepting creation, a 500, a 429, and a stalled response.
Verify one creation request, an actionable error, and no further rooms unless
explicitly submitted again. Cancel using Escape, the backdrop, or the dialog's
Cancel button; quit during a stalled creation and verify shutdown completes
without waiting for the network. Cancellation cannot undo a request already
accepted by the homeserver: check the room list before manually retrying.
Space-link failure should retain the created room and offer retry of the link.

Automated fixtures cover single-attempt room/DM errors, malformed success and
rate-limit responses, cancellation signals on dismissal/quit, pending-operation
release, and DM reuse before sync. These do not establish native desktop behavior
or a particular homeserver's behavior after a lost response.
