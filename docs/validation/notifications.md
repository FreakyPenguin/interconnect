# Desktop notifications

## Behavior

Settings → Notifications controls alerts/previews (both default on). Previews off
uses “New Matrix message” without room/sender/body. Send sample notification uses
fixed text, works in demo with alerts off, and has no navigation target. Delivery
errors appear in Settings; OS acceptance does not guarantee a visible banner.

Alerts follow SDK push rules independently of open timelines/spaces. Own messages,
muted rooms, edits, reactions, redactions, and state events stay quiet. Undecryptable
messages use “Encrypted message” when rules request an alert; late decryption does
not update it. A focused, uncovered room suppresses ordinary messages; thread
replies suppress only with that thread open. Older scroll position does not change
suppression. Unfocused/covered panes can notify.

Clicks open the room/thread, request focus/unminimize, and preserve space filter,
drafts, and saved positions. Already-open targets keep their timeline. Modals and
pending pickers defer the latest click. Removed rooms/session teardown invalidate
clicks. Only the newest 64 alerts retain observers; older banners may remain inert.
Preview opt-out still supports clicks; room/thread IDs stay in-process.

Events before worker registration or outside a two-minute age/future window are
discarded. A 2048-ID cache deduplicates; room-list sync requests 20 events/room;
large gaps/clock skew can lose alerts. The 64-event queue drops overflow. Serial
delivery has a 10-second deadline; macOS permission has 60 seconds. Recheck settings,
focus, age, and shutdown after permission. No polling, inline reply, sound policy,
or cold-start navigation; OS-handed requests cannot be recalled.

## Linux

Uses `org.freedesktop.Notifications` on the session bus. Containers need the
[host bus mount](../development.md#using-the-host-secret-service-from-the-container)
and notification daemon. Read-only reachability check:

```sh
dbus-send --session --print-reply --dest=org.freedesktop.Notifications \
  /org/freedesktop/Notifications org.freedesktop.Notifications.GetServerInformation
```

Send the demo Settings sample; inspect banner/notification center. Missing daemon
must produce a Settings error without blocking UI. Wayland may deny focus requests.

## macOS

Uses UserNotifications via upstream `objc2` on blocking workers/async callbacks.
Observers unregister on response/drop/cancellation/teardown. Use the
[native bundle](../development.md#macos-builds); bare `cargo run` lacks identity.
Send the demo sample, allow permission, check Notification Center, denial/retry in
System Settings → Notifications, and relaunch with the same bundle.

## Acceptance

Use a dedicated account and second client:

- Check focused/unfocused/covered panes, closed/open/other threads, other spaces,
  muted rooms, preview opt-out, duplicates, pagination, reconnect, encrypted messages,
  pending permission, and shutdown.
- Click room/thread alerts while minimized, in another space, and with drafts/older
  positions. Verify target and state preservation, including already-open targets.
- Click with unsaved Settings, then close it: defer navigation. After leaving room,
  signing out, or quitting, clicks must not restore stale state. Sample must not navigate.

Automated policy/queue/observer/lifecycle tests and a private D-Bus daemon fixture
cover payloads, actions, mismatched IDs, dismissal, escaped previews, and rejection.
They do not establish native banners/focus, macOS permission UI, or connected delivery.
An isolated Apple Silicon adapter compile/lint check does not establish full app
build/runtime acceptance. Measure idle behavior [separately](idle-resources.md).
