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
