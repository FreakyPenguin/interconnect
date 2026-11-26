# Desktop notifications

## Behavior

Settings → Notifications controls native alerts/previews (default on). Previews off
uses category-only text, including newly collected inbox entries; enabling later
cannot reconstruct hidden text. Send sample notification uses fixed text, works in
demo even with alerts off, and cannot navigate. Delivery errors appear in Settings;
OS acceptance does not guarantee a banner.

Message alerts follow SDK push rules independently of timelines/spaces. Own messages,
muted rooms, edits, reactions, redactions, and ordinary state events stay quiet.
Undecryptable alerts say “Encrypted message”; late decryption does not update them.
A focused, uncovered room suppresses ordinary alerts; thread replies suppress only
with that thread open. Scroll position does not affect suppression.

Cached invitations and the first successful room-list sync form a silent baseline.
Later invitation transitions can alert; metadata edits do not. Reinvites update the
same history entry. Supported incoming verification requests can alert immediately
unless the dialog is visible; resolved/replaced flows cannot activate old targets.

Clicks request focus/unminimize and open the exact room/thread event, loading older
history as needed while preserving filters/drafts/positions. A static three-second
highlight starts on visibility. Invitation/verification clicks review without
accepting. Modals/pickers defer the latest click; stale/session-ended targets are
ignored. Only the newest 64 alerts retain click observers. Preview opt-out retains
in-process navigation IDs; legacy room-only entries still open the conversation.

## Notification screen and history

The bell or Ctrl+I (macOS: Cmd+I) opens Notifications and focuses Unread;
the bell tooltip shows the shortcut. Its badge counts unread Messages, Invitations,
and Verification. Unread is the default; History includes read entries. Category filters
apply to either. Opening alone marks nothing read. Open an entry, mark read/unread,
or Mark all read. Expired invitation/verification entries retain read controls but
lose navigation. Native invitation clicks put the active invitation first for review.
Successfully accepting or declining an invitation marks its bell entry read.
Joined room/space snapshots also clear accepted invitations, including acceptance
on another device and history imported after joining. Later manual unread choices
remain until another explicit read action; a new invitation occurrence stays unread.

The newest 256 entries/read flags persist encrypted per profile/device with drafts.
Collection continues when alerts are disabled/suppressed, permission is pending,
or delivery fails. This is observed local history, not a cross-device/server archive.
Read controls send no Matrix receipts.

Reading the newest visible conversation clears matching older message entries.
Viewing one older event clears only it, including entries delivered after layout.
Session-local read observations also handle entries collected after scrolling past
their messages or leaving the conversation. These observations are bounded to 256
events and 256 conversation boundaries, reset on account changes, and preserve
existing manual read/unread choices when importing duplicate history. A newest
boundary uses messages actually reported visible, excluding arrivals awaiting layout.
Thread replies clear in their thread, not from a room summary. Covered, unfocused,
loading, or unsettled panes record no new reads. The inbox suspends underlying input,
receipts, media demand, and GIFs while retaining live room/thread offsets and drafts.
Demo uses local history without delivery.

## Worker limits

Collection is separate from serial native delivery, with no polling. Stable IDs
identify message events, invited rooms, and verification flow/devices. Titles/bodies
are bounded plain text and omitted from diagnostics.

Events before worker registration or outside a two-minute age/future window are
discarded. A 2048-ID cache deduplicates messages/verification; sync requests 20
events/room. The SDK queue holds 64 events; slow native delivery retains the newest
64 pending alerts. Large gaps, overflow, and clock skew can lose entries/alerts.
Delivery deadline: 10 seconds; macOS permission: 60 seconds. After permission,
recheck settings/focus/validity/age/shutdown. No inline reply, sound policy, cold-start
navigation, or recall after OS handoff.

## Linux

Uses `org.freedesktop.Notifications` on the session bus. Containers need the
[host bus mount](../development.md#using-the-host-secret-service-from-the-container)
and a notification daemon. Read-only reachability check:

```sh
dbus-send --session --print-reply --dest=org.freedesktop.Notifications \
  /org/freedesktop/Notifications org.freedesktop.Notifications.GetServerInformation
```

Send the demo sample; inspect banner/center. Missing services report a Settings
error without blocking UI. Wayland may deny focus requests.

## macOS

Uses UserNotifications via `objc2` on blocking workers/async callbacks; observers
unregister on response/cancellation/teardown. Use the [bundle](../development.md#macos-builds)
for identity. Test sample delivery, permission allow/deny/retry in System Settings,
and relaunch with the same bundle.

## Acceptance

In demo mode, check Ctrl+I (macOS: Cmd+I) from both composers,
sidebar/filter, breadcrumb, room/account search, and Saved messages. Verify Unread
receives focus without marking entries read, and Escape restores the conversation
with drafts, space/filter, and reading positions intact. Dialogs and active IME
composition must take precedence. These are manual checks, not a completed run.
With enough entries to overflow, press Page Down/Up immediately after the shortcut,
with the pointer outside the list. Check overlapping pages, repeated presses,
top/bottom limits, and retained Unread focus. Extra modifiers, an open category
dropdown, active IME, and covering dialogs must not page the underlying list.

Use a dedicated account and second client:

- Check focused/unfocused/covered rooms, open/closed/other threads, other spaces,
  mute, previews, duplicates, pagination, reconnect, encryption, and shutdown.
- Click while minimized/elsewhere/reading older history. Verify exact room/thread
  target, preserved drafts/filter, visibility-timed highlight, and stale expiry guards.
  Defer through unsaved Settings; sample/stale-account clicks cannot navigate.
- Restore cached invitations offline, then sync existing ones: collect silently.
  Receive a new invite and review without joining; decline/withdraw/reinvite/rename
  to distinguish invitation transitions from metadata changes.
  Accept room and space invitations directly from Invitations and on another device;
  their bell entries should become read. Declining locally also clears the entry;
  failed responses and new invitations stay unread. Check manual unread afterward.
- Receive own-device verification with the dialog open/closed and previews on/off.
  Click only reviews; cancelled/replaced banners cannot act.
- While permission is pending/denied or alerts disabled/suppressed, history still
  updates. Preview opt-out affects both native payloads and new history.
- Read specific older messages through navigation/search, including late-arriving
  history; offscreen entries stay unread. Repeat for threads and restart persistence.
- Read several DM messages, scroll past them, and switch rooms before notification
  history arrives. Their bell entries should arrive read; unseen messages, later
  arrivals, other rooms, and other threads should retain their unread state.

Automated policy/history/queue/lifecycle and private D-Bus fixtures cover payloads,
actions, baselines, stale targets, and encrypted persistence. They do not establish
native banners/focus/permission UI or live delivery. Measure [idle resources](idle-resources.md) separately.
History regressions also cover real DM layout visibility, delayed collection after
scrolling/navigation, bounded read observations, account replacement, and unseen
arrivals before their first layout. The connected checks above remain manual.
Invitation regressions cover successful room/space responses, failed/stale results,
joined snapshots, delayed history import, reinvites, and later manual unread choices.
