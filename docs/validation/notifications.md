# Desktop notifications

## Behavior

Settings → Notifications controls native alerts/previews (both default on).
Previews off uses category-only text without room/sender/body/device details,
including when entries are collected for the app's notification history. Preview
changes apply when entries arrive; enabling previews later does not reconstruct
previously hidden text. Send sample notification uses fixed text, works in demo
with alerts off, and has no navigation target. Delivery errors appear in Settings;
OS acceptance does not guarantee a visible banner.

Message alerts follow SDK push rules independently of open timelines/spaces. Own
messages, muted rooms, edits, reactions, redactions, and ordinary state events stay
quiet. Undecryptable messages use “Encrypted message” when rules request an alert;
late decryption does not update it. A focused, uncovered room suppresses native
alerts for ordinary messages; thread replies suppress only with that thread open.
Older scroll position does not change suppression. Unfocused/covered panes can
notify. These native suppression rules do not prevent history collection.

Invitations and supported incoming device-verification requests are observed from
session-owned SDK snapshots. Cached invitations enter history silently, including
while offline. Invitations discovered by the first successful room-list sync also
form a silent baseline; later invitation transitions can produce native alerts.
Metadata changes do not re-alert; reinvitations update the same stable history
entry with a newer timestamp. Verification requests enter history and can alert
immediately, except while the verification dialog is already visible. Resolved or
superseded requests cannot activate an old native verification target.

The worker retains the newest 256 entries, with stable IDs by message event,
invited room, or verification flow/device. History collection continues with native
alerts disabled, a focused conversation, pending desktop permission, or native
delivery failure. It runs separately from serial desktop delivery, with no polling
or idle timer. History titles and bodies are bounded plain text; diagnostics omit
their contents. Read flags and durable history belong to the app UI/storage layer.

Clicks on message alerts open the exact event in its room/thread, request
focus/unminimize, and preserve the space filter, drafts, and prior reading positions.
Older message destinations load asynchronously. The destination receives a static
three-second tint and outline after it becomes visible. Expiry removes only the
visual marker. Legacy entries recover event IDs from their exact stored message
identity where available; room-only targets retain conversation navigation.
Invitation clicks review the current invitation; verification clicks review the
current flow, without accepting or confirming it. Modals and pending pickers defer
the latest click. Removed targets/session teardown invalidate clicks. Only the
newest 64 alerts retain observers; older banners may remain inert. Preview opt-out
still supports clicks; destination IDs stay in-process. Native actions use the
category-independent “Open notification” label.

Message events before worker registration or outside a two-minute age/future
window are discarded. A 2048-ID cache deduplicates messages/verification flows;
room-list sync requests 20 events/room; large gaps/clock skew can lose entries or
alerts. The 64-event SDK queue drops overflow; the native pending queue retains
only the newest 64 alerts while delivery is slow. Serial delivery has a 10-second
deadline; macOS permission has 60 seconds. Recheck settings, focus, request validity,
age, and shutdown after permission. No polling, inline reply, sound policy, or
cold-start navigation; OS-handed requests cannot be recalled.

## Notification screen and history

The bell immediately left of the account avatar opens **Notifications**. The badge
counts unread entries across Messages (including threads), Invitations, and
Verification. **Unread** is the default; **History** includes read entries.
A category selector narrows either view. Opening the screen alone does not mark
anything read. Open an entry, mark it read/unread, or use **Mark all read**.
Expired invitations and verification requests remain in history with unavailable
navigation; their read controls still work. Native clicks on active invitations
put that invitation first in the review dialog.

History and read flags are local to the profile/device, share encrypted draft
storage, and survive a normal restart. The newest 256 entries are retained; this
is observed notification history, not a homeserver-wide or cross-device archive.
Messages follow SDK push rules even when native alerts are disabled. Reading the
latest visible conversation clears matching older message notifications; reading
older history does not. Inbox read controls do not send Matrix read receipts.
Opening the inbox suspends the underlying conversation widgets and inline GIFs,
preserving room/thread drafts and live scroll positions without hidden input,
read receipts, media demand, or animation timers. Closing it returns to the same
conversation. Demo contains only local sample history and performs no delivery.

Automated UI/store tests cover category filters, read controls, unavailable and
stale-account activation, small-window layout, keyboard dismissal, idle redraws,
mounted timeline preservation, encrypted restart/read-state restoration, and
history arriving during sign-out or shutdown. Manually check native clicks on
Linux/macOS, toolbar placement at different scales, and connected history across
restart using an authorized isolated account. No connected desktop acceptance is
claimed by the automated tests.

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
  positions. Verify exact event targeting and highlight, including already-open
  conversations and older encrypted events. Wait during history loading: the full
  three-second marker starts on visibility. Repeated clicks restart feedback;
  navigation/account changes reject old expiry callbacks.
- Click with unsaved Settings, then close it: defer navigation. After leaving room,
  signing out, or quitting, clicks must not restore stale state. Sample must not navigate.
- Restore cached pending invitations offline and connect with existing invitations:
  collect history silently. Receive a fresh invitation after initial sync; check the
  native banner and click to review without joining. Decline/withdraw/reinvite and
  rename the room: only a new invitation transition should alert.
- Request supported device verification from another own device, with the dialog
  open/closed and previews on/off. Click to review; no accept/confirm command may be
  sent by a click. Cancel/replace the flow before clicking its old banner: ignore it.
- While desktop permission is pending or denied, receive messages, invitations, and
  verification requests: history must still update. Check generic previews in both
  native payloads and newly collected history. Native alerts off and focused-room
  suppression must leave collection active.

Automated policy/history/queue/observer/lifecycle tests and a private D-Bus daemon
fixture cover payloads, actions, mismatched IDs, dismissal, escaped previews,
rejection, silent invitation baselines, reinvitations, superseded verification
flows, session leases, preview opt-out, and collection during pending permission.
They do not establish native banners/focus, macOS permission UI, or connected delivery.
An isolated Apple Silicon adapter compile/lint check does not establish full app
build/runtime acceptance. Measure idle behavior [separately](idle-resources.md).
