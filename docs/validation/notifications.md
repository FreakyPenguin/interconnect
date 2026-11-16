# Desktop notifications

`notify-rust` provides asynchronous Linux
Tokio/zbus delivery to `org.freedesktop.Notifications` on the session bus. macOS
selects the modern UserNotifications backend (`preview-macos-un`), with an app
bundle and asynchronous permission handling.

## Behavior and limits

**Settings → Notifications** controls alerts and previews. Both default on and
apply after Save/Reload. Turning previews off hides room names, sender IDs, and
message text behind “New Matrix message”. **Send sample notification** explicitly
sends fixed sample text, including in demo mode and when automatic alerts are
disabled. OS acceptance does not guarantee a banner: Do Not Disturb and desktop
policy still apply. Delivery failures appear in Settings and flag its button as
needing attention; the next successful automatic delivery clears that error.

Alerts use SDK push-rule notifications, independently of replaceable UI snapshots
and open timelines. Muted rooms stay quiet; rooms outside the selected space can
notify. Own messages, edits, redactions, reactions, and membership/state events
are excluded. Messages decrypted by sync use their text. Undecryptable events use
“Encrypted message” when their push actions request notification; late decryption
does not update an existing desktop alert.

A focused room pane suppresses its ordinary messages. Thread replies are
suppressed only when the matching thread pane is also open. Other threads still
notify. Unfocused windows, Settings, and image viewers do not suppress alerts.
This is a pane-level policy: scrolling up within a focused conversation still
suppresses its alerts. Notifications do not change drafts, scrolling, selection,
spaces, or read receipts.

Events timestamped before worker registration, more than two minutes old, or
more than two minutes in the future are discarded. This prevents startup/history
and reconnect floods but drops offline catch-up and events from substantially
skewed sender clocks. IDs are deduplicated in a bounded 2048-event cache,
including events suppressed by focus/settings.

Room-list sync requests up to 20 events per room instead of the SDK default one,
reducing missed mentions behind newer messages in unselected rooms. This adds
initial sync/cache work and cannot guarantee delivery across larger server
timeline gaps. The 64-event queue drops overflow rather than blocking sync.
Delivery is serial, with a ten-second service deadline and a separate sixty-second
macOS permission deadline. Current settings, focus, age, and shutdown are checked
again after permission completes. Requests already handed to the OS cannot be
recalled by disabling notifications.

There is no application polling timer, action waiter, sound policy, inline reply,
or click-to-room navigation in this first implementation. Closing the app stops
sync and delivery; this is not push while the application is quit.

## Linux and development containers

The [host-session D-Bus mount](../development.md#using-the-host-secret-service-from-the-container)
used for Secret Service also carries notification requests. A host notification
daemon is required. The Rust backend does not need `libnotify`.

Check service reachability without displaying anything:

```sh
dbus-send --session --print-reply --dest=org.freedesktop.Notifications \
  /org/freedesktop/Notifications org.freedesktop.Notifications.GetServerInformation
```

Run `cargo run --locked --bin interconnect -- --demo`, open Settings, and click
**Send sample notification**. Check the host banner or notification center.
A forwarded display alone does not forward notifications; the session bus must
also be accessible. An absent daemon produces a Settings error; delivery runs
asynchronously.

## macOS

On a Mac with Rust and Xcode command-line tools:

```sh
./scripts/bundle-macos.sh
open target/Interconnect.app --args --demo
```

The script builds the GUI, copies its binary/Info.plist into an `.app`, and
ad-hoc signs it for local development. It does not launch the application. With
`CARGO_TARGET_DIR` set, substitute that directory for `target`. The stable bundle
identifier is `io.github.interconnect.desktop`. Distribution signing/notarization
remain release work.

Send the sample in Settings, accept the permission prompt, and inspect
Notification Center. Denial appears as an actionable error. Enable Interconnect
in **System Settings → Notifications**, then retry and check again after
relaunching the same bundle. A bare `cargo run` launch reports a missing bundle
rather than invoking UserNotifications without a bundle identity. Iced
stays on the main thread, pumping AppKit while Tokio handles notification work.

This provides the native notification route, not a complete macOS account port:
the existing saved-account backend still needs Linux Secret Service. A macOS
Keychain backend is separate work. Demo/sample delivery needs no account store,
Matrix login, or network.

## Validation evidence and remaining acceptance

Recorded Linux automated checks cover notification policy, bounded deduplication,
privacy settings, focus propagation, muted-room filtering without open timelines,
and cancellation during pending permission. They do not establish native banner
appearance or connected-account delivery.

The D-Bus integration test delivers real freedesktop protocol calls to a mock
service on a private `dbus-daemon`, checking app identity, escaped body markup,
category, sample text, and desktop rejection errors. Only the
child process receives the private bus address;
the runner and host notification daemon remain untouched. This test requires
`dbus-daemon`, included in the development container.

Native Linux banner appearance, connected-account delivery, encrypted notification
delivery with real keys, and macOS compilation/permission/banner behavior remain
manual acceptance checks. Run macOS compilation and acceptance on a native host.

Connected acceptance uses a dedicated test room and a second client. Scenarios
include focused/unfocused panes, closed threads, other rooms/spaces, mute rules,
preview opt-out, duplicate events, history pagination, reconnect, and shutdown.
