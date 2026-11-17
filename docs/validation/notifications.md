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
pane-level policy. Alerts do not change selection, drafts, or receipts.

Events before worker registration or outside a two-minute age/future window are
discarded; offline catch-up and clock skew can therefore lose alerts. A bounded
2048-event cache deduplicates IDs. Room-list sync requests 20 events per room,
but larger server gaps can still miss messages. The 64-event delivery queue drops
overflow; serial delivery has a 10-second deadline and macOS permission a separate
60-second deadline. Settings/focus/age/shutdown are rechecked after permission.

There is no alert polling timer, click-to-room navigation, inline reply, or sound
policy. The app must be running. Requests already handed to the OS cannot be recalled.

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

Use the [app bundle](../development.md#macos-builds), then send the Settings sample
and accept the UserNotifications permission prompt. Check Notification Center,
denial/retry via **System Settings → Notifications**, and relaunch of the same
bundle. Bare `cargo run` lacks the bundle identity and reports an error. Demo
sample delivery needs neither a saved account nor Matrix access.

## Acceptance

Use a dedicated account and second client to check focused/unfocused panes,
closed/open threads, other spaces, muted rooms, preview opt-out, duplicates,
history pagination, reconnect, encrypted messages, pending permission, and shutdown.

Automated tests cover policy, bounded queues/deduplication, and cancellation.
A private D-Bus fixture checks protocol payloads, app identity, and rejection;
it requires `dbus-daemon` and does not call the host daemon. Native Linux banners,
macOS prompts/banners, and connected delivery require desktop acceptance.
