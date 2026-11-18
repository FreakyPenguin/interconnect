# Desktop notifications

## Behavior

Settings → Notifications controls alerts and previews, both on by default.
Previews off uses “New Matrix message” without room, sender, or message text.
**Send sample notification** explicitly sends fixed text, including in demo mode
and with automatic alerts disabled. Delivery errors appear in Settings; OS
acceptance does not guarantee a banner under Do Not Disturb or desktop policy.

Alerts follow SDK push rules independently of open timelines/selected spaces.
Muted rooms, own messages, edits, reactions, redactions, and state events stay
quiet. Sync-decrypted messages use text; undecryptable events use “Encrypted
message” if push rules request an alert. Late decryption does not update alerts.

A focused, uncovered room suppresses its ordinary messages; a thread reply is
suppressed only when that thread is open too. Other threads can notify. Unfocused
windows and covered panes do not suppress. Scrolling up does not change this
pane-level policy. Delivery does not change selection, drafts, or receipts.
Clicking an alert opens its room and, for thread replies, the thread, then requests window focus and
unminimizes the window. The selected space filter stays unchanged. Existing
drafts and scroll positions survive; clicking the already open conversation
keeps its timeline. Read receipts follow the usual visible-pane policy.

Clicks wait while a modal dialog or pending picker covers the conversation,
then apply after it closes. Only the latest pending click is retained. Rooms
that have left the room list are ignored. Session teardown/sign-out invalidates
queued clicks. The newest 64 alerts retain click observers; older banners can
remain visible but no longer navigate. Disabling previews still allows clicks;
room/thread IDs stay in process and are not passed to the desktop service.

Events before worker registration or outside a two-minute age/future window are
discarded; offline catch-up and clock skew can therefore lose alerts. A bounded
2048-event cache deduplicates IDs. Room-list sync requests 20 events per room,
but larger server gaps can still miss messages. The 64-event delivery queue drops
overflow; serial delivery has a 10-second deadline and macOS permission a separate
60-second deadline. Settings/focus/age/shutdown are rechecked after permission.

There is no alert polling timer, inline reply, or sound policy. The app must be
running; cold-start navigation is unsupported. Requests already handed to the OS
cannot be recalled. The Settings sample has no conversation target and does not
navigate.

## Linux

Delivery uses `org.freedesktop.Notifications` on the session bus, without a
libnotify dependency. Containers need the [host D-Bus mount](../development.md#using-the-host-secret-service-from-the-container)
and a host notification daemon; forwarding the display alone is insufficient.
Check reachability without displaying an alert:

```sh
dbus-send --session --print-reply --dest=org.freedesktop.Notifications \
  /org/freedesktop/Notifications org.freedesktop.Notifications.GetServerInformation
```

Run demo, send the Settings sample, and inspect the banner/notification center.
An absent daemon should produce an actionable Settings error without blocking UI.

## macOS

A small application-owned adapter uses Apple's UserNotifications API through
upstream `objc2` bindings. Native calls run on blocking workers and return through
asynchronous callbacks. Response registrations are removed when a click arrives
or an observer is dropped, including delivery cancellation and session teardown.
There is no vendored notification library or notification polling timer.

Use the [app bundle](../development.md#macos-builds), then send the Settings sample
and accept the UserNotifications permission prompt. Check Notification Center,
denial/retry via **System Settings → Notifications**, and relaunch of the same
bundle. Bare `cargo run` lacks the bundle identity and reports an error. Demo
sample delivery needs neither a saved account nor Matrix access.

## Acceptance

Use a dedicated account and second client to check focused/unfocused panes,
closed/open threads, other spaces, muted rooms, preview opt-out, duplicates,
history pagination, reconnect, encrypted messages, pending permission, and shutdown.

Click ordinary and thread alerts while minimized, in another space, and with an
unsent draft. Check that the correct room/thread opens, the space stays selected,
and drafts/scroll survive a round trip. Click while Settings has unsaved changes,
close Settings, and check deferred navigation. Click a banner after leaving its
room, signing out, or quitting: it must not reopen stale account state. Check
preview-disabled alerts and the target-free Settings sample. Linux focus policy
(particularly Wayland) may deny the focus request; verify with the target desktop.

Automated tests cover policy, bounded queues/deduplication, click observer bounds,
session invalidation, room/thread draft/scroll preservation, and cancellation.
A private D-Bus fixture checks protocol payloads, default actions, mismatched IDs, dismissal, app identity,
and rejection;
it requires `dbus-daemon` and does not call the host daemon. Native Linux banners,
macOS prompts/banners, and connected delivery require desktop acceptance.

## Implementation validation (2026-11-18)

On Linux, formatting, Clippy with warnings denied, and the workspace suite passed
(245 library tests and 398 UI tests; two opt-in library tests ignored). The D-Bus
fixture uses its own daemon and exercised immediate default clicks, unrelated
notification IDs, dismissal, escaped previews, sample actions, and rejection.
No connected client or host notification service was used.

The response registry is tested for clicks, dismissal, unrelated and repeated
responses, and dropping both unpolled and pending observers. An isolated Apple
Silicon cross-check compiled and linted the native adapter and checked the async
permission/delivery paths' `Send` bounds. This is not a full macOS app build or
runtime acceptance. Native banners, window activation, macOS permission prompts,
and connected end-to-end behavior remain manual checks.
