# Connected Iced prototype

The `matrix-ui` binary connects Iced to the existing persistent session. It restores
credentials without browser login, lists joined rooms, and displays SDK room and
thread timelines. Opening a room explicitly requests an initial history page of
20 events through SDK back-pagination while continuing to display live updates.
Room and thread text composers are implemented. Thread-pagination controls,
verification UI, recovery UI, receipts, and automatic read marking remain pending.
Use an already verified profile with access to the test room's keys.

Close `session-live` or any other process using the profile first. Keep the host
Secret Service available and unlocked, and provide a Wayland display as described
in [development.md](../development.md).

```sh
cargo run --locked --bin matrix-ui
```

An optional profile-directory argument follows the binary name. The default is
`.matrix-spike`; keep its canonical path unchanged from login. This command does
not create a new session, replace a device, or log out. Room names and decrypted
messages appear in the window.

## Interactive acceptance

1. Restore the existing profile. Confirm the room list appears without browser
   login. Select a dedicated encrypted test room; an unencrypted room should show
   an explicit error.
2. Confirm existing messages load without sending a new message. Then send
   messages from the second client and verify arrival, ordering, sender, and
   decryption in the room timeline. Use **Load older** to request additional pages.
   A page containing only hidden thread replies may add no visible rows; continue
   until older messages appear or **Beginning of history reached** is shown.
   The button disables while loading; failures offer **Retry loading history**.
   Missing keys should appear as decryption
   failures rather than a login failure.
3. Select **Open thread** under a known thread root. Confirm its root and loaded
   replies appear, then send another reply from the second client.
4. Close the thread. Send another reply from the second client and reopen it.
   Confirm the reply appears. Room synchronization should remain active.
5. Rapidly switch rooms and threads, including switching while a timeline is
   loading. Old snapshots must not appear under a new selection. Repeatedly
   selecting a room is also a way to retry a failed timeline load.
6. Close the window. It should wait for SDK sync to stop, then exit. A shutdown
   error stays visible; close again or use **Exit** to dismiss it. Restart and
   confirm the same session still works. This prototype does not save navigation.

For presentation-lifetime traces (without SDK event logging):

```sh
RUST_LOG=chatclient=debug cargo run --locked --bin matrix-ui
```

Each started presentation view should stop when replaced or closed. Closing a
thread should stop only its view, leaving the room view running. A room switch
stops both old views. Trace entries do not contain room IDs or message bodies.
No recurring application timer is installed. Verify settled/hidden-window
behavior and scheduler wakeups using [the measurement procedure](milestone-0.md#early-cpu-and-wakeup-measurements);
absence of app timers is not evidence of zero SDK/network wakeups or redraws.

## Implementation and evidence — 2026-11-14

- Iced's Tokio executor runs restoration, the single `SyncService`, and view
  futures. Room and thread futures own their timeline subscriptions and are
  dropped independently on navigation. No detached presentation tasks are spawned.
- Desired navigation and UI snapshots use latest-value watch channels. The Iced
  output channel has capacity one. Snapshots contain SDK-ordered timeline items;
  raw timeline diffs are never reordered or coalesced by the app. The SDK remains
  responsible for applying diffs. Generation checks reject queued stale panes.
- A slow UI cannot block the backend on snapshot delivery. Snapshot delivery is
  independent of sync stop and view cancellation. This transport is for replaceable
  state only; future send commands/results will need separate delivery semantics.
- Persistent client construction retains the exclusive profile lock until process
  exit, covering late GUI handles and runtime cleanup, and shares
  the CLI's device-key and server-identity checks. SDK stop has a 15-second bound.
  The GUI owns one Tokio runtime and lends its handle to Iced. After UI teardown,
  runtime cleanup has a five-second bound and reports incomplete cleanup on timeout.
  In-progress queue admission still has no total deadline.
- The local mock-server integration test uses actual SDK room/thread timelines,
  checks repeated navigation and thread closure, and shuts down without draining
  UI snapshots. Its room has encryption state but plaintext fixture events: this
  test proves integration, not cryptography or live-server encrypted delivery.
- UI tests cover stale snapshot rejection and clearing the thread on room change.
  Existing CLI persistence, verification, and shutdown tests remain applicable.

Local results: `cargo build --locked --bins`, formatting, Clippy with warnings
denied, and all 17 tests passed. A five-second native Wayland smoke run with an
intentionally unavailable Secret Service remained running without logged errors;
the timeout harness then terminated it (exit 124). No real profile or live account
was opened. This checks startup, not visual correctness or graceful closing.

Live two-client GUI acceptance, visual correctness, keyboard/focus behavior,
hidden-window behavior, and scheduler measurements remain **pending**. No complete
product feature or milestone is claimed by these implementation checks. The room
list is a basic joined-room list refreshed on room-list service state changes;
spaces, unread metadata, full incremental room-list presentation, cache budgets,
and viewport virtualization remain later work.


### Initial history correction — 2026-11-14

The user reported that the host CLI could load an encrypted room's messages,
while the GUI initially showed none; newly sent messages did appear in the GUI.
The prototype had relied on live sync/cache contents and never requested room
history. It now issues an initial SDK back-pagination request when opening a room.
Live snapshots remain active during the fetch. A one-shot 30-second deadline
bounds history loading; failures remain visible alongside live messages. Selecting
the room again retries the load. Navigation and shutdown cancel the view-owned
history future.

The mock-server regression supplies an empty sync timeline with older messages
available only through `/messages`. It failed before the fix and passes after it.
A second regression checks shutdown while initial history is pending. Live-server
confirmation of this correction remains pending.


### Manual room pagination — 2026-11-14

The user confirmed that repeated CLI pagination reveals the pre-login messages.
The GUI now provides **Load older**, using a bounded, view-owned request channel
and SDK back-pagination on the existing timeline. Live updates continue during a
fetch. Switching rooms or closing the app cancels the pending view operation.
Only the SDK's reached-start result ends pagination; a page without new visible
rows does not disable further requests. Errors preserve loaded messages and allow
retry. Thread pagination controls remain outside this change.

The mock-server integration covers an initial history page, a hidden-thread-reply
page, and an older visible message on a subsequent page, followed by the end of
history. UI tests reject stale-room clicks and immediate duplicate requests.


## GUI text sending — 2026-11-14

The user confirmed historical decryption after enabling SDK room-key requests;
see [the key-exchange result](interactive.md#user-run-historical-key-exchange--2026-11-14).

Each pane has its own multiline composer and a target label. Enter inserts a
newline, Ctrl+Enter sends from the focused composer, and **Send** is also available.
IME preedit state suppresses sending; test this with the host input method before
claiming full IME acceptance. Opening panes does not focus the editor automatically.
The stock editor may blink its caret while focused; the earlier empty-window idle
baseline does not describe an actively focused editor.

Drafts are retained separately per room and per thread across navigation and pane
closure, in memory only. They are not saved across process restarts. While the SDK
accepts a submission into its persistent send queue, that draft is temporarily
locked. Queue acceptance clears only that draft; an admission error preserves it.
Subsequent server delivery is shown by the timeline's **Pending**, **Sent**, or
**Failed** state. A failed local echo offers **Retry**, which reuses its existing
SDK transaction and resumes the room's queue (including other waiting messages).
It does not create another message from the text.

Finite Iced tasks own their captured timeline/queue handles until admission or
retry completes, even if the view closes. At most eight such operations may be
outstanding. Their results arrive as individual Iced messages, not replaceable
snapshot updates. Window closure waits for those operations before asking the
session to stop; queued network sends retain the SDK's persistence behavior.
Total shutdown is still subject to the previously documented database/runtime
cleanup limitations.

### Two-client acceptance (partially verified)

1. Send a multiline room message; confirm it appears once in the other client,
   with no thread relationship. Check Pending then Sent locally.
2. Open a known thread, send a reply, and confirm it belongs to that thread in the
   other client. The room composer must still create an ordinary room message.
3. Write different room/thread drafts. Switch rooms and close/reopen the thread;
   confirm each draft returns to its own target.
4. Send and immediately navigate away. Confirm the message keeps its original
   target and the acknowledgement does not clear a different draft.
5. Exercise Enter, Shift+Enter, Ctrl+Enter, and the active IME's composition and
   confirmation keys. No send should occur during composition or in an unfocused
   editor; ordinary editing/paste should work.
6. With connectivity disabled, send a test message and allow the SDK to report a
   failure (it may retry first). Restore connectivity and use Retry. Verify only
   one event is created. Stop the UI while a message is pending, restart, and
   inspect the existing queue before attempting any manual resend.

The local mock test exercises real SDK encryption and room/thread relationship
construction, an HTTP send failure, and retry using the same transaction URL.
It decrypts captured test ciphertext to verify the intended message target.
Live-server delivery, desktop composition behavior, and offline/restart acceptance
remain separate checks.


Local sending checks: all 21 workspace tests passed, including real SDK encrypted
queue/retry checks and draft/shortcut tests. Formatting, locked binary build, and
Clippy with warnings denied passed. This does not replace the pending desktop and
two-client acceptance above.


### User-run GUI sending — 2026-11-14

The user reports that sending messages in both the room and thread composers
works. This is user-reported live evidence for basic GUI room-message and
thread-reply sending, following the earlier confirmations of message loading
and historical decryption. No message contents or room identifiers are recorded.

Multiline formatting, explicit Pending/Sent transitions, duplicate checks,
draft retention, navigation during queue admission, keyboard/IME behavior,
failed-send retry, and offline/restart scenarios were not specifically confirmed.
Those acceptance cases remain pending; this result does not complete the full
messaging milestone or ordinary-reply support.


## Composer teardown correction — 2026-11-14

The user reported an abort when closing the UI after sending. The supplied trace
runs from `App -> Model -> Pane -> Composer -> Timeline -> Client -> SQLite pool`
to `deadpool_sync::SyncWrapper::drop`, which calls Tokio `spawn_blocking` with no
current runtime. A second panic during destructor cleanup aborts the process.
Stopping sync alone did not release the SDK handles retained by UI snapshots.

Composer and retry handles now wrap their SDK-owned values with the originating
Tokio handle. Their final drop enters that runtime context before releasing those
values. This covers the app model, widget/message copies, and discarded snapshots,
without depending on a particular final UI message being processed. It uses the
existing runtime and does not wait for SQLite I/O inside those destructors.

The GUI now owns its one Tokio runtime in `main`; a small Iced executor adapter
borrows its handle. The runtime remains entered on the UI thread until Iced and
its application state have been destroyed, even if Iced drops its executor first.
Afterward the existing five-second runtime-cleanup helper drains blocking work,
reporting a timeout as an error. No second Tokio runtime is created.

Regression tests release the last shared handle on a non-runtime thread and verify
blocking cleanup completes, and release a real SQLite-backed SDK client outside
`block_on` before shutting down the runtime. A GUI-lifecycle regression drops the
Iced executor before a SQLite client and confirms background cleanup completes
before the UI runtime wrapper returns. Existing encrypted send/retry and
navigation checks also remain applicable. This fixes the missing-runtime destructor
path and bounds final runtime cleanup; it does not establish a total deadline for
queue-admission operations before sync shutdown.

Desktop follow-up: open a room and thread, send, then close the window; also close
with a failed-send retry button present. Confirm the process exits without a panic
and that restarting restores the same session. Live confirmation remains pending.


Local teardown checks: all 24 workspace tests, locked binary build, formatting,
and Clippy with warnings denied passed. No live account was opened or modified
for these regressions; the SQLite tests use temporary stores.


## Wi-Fi loss and automatic reconnection — 2026-11-14

The user reported that losing Wi-Fi caused a terminal synchronization error and
forced the UI to exit. The GUI now enables `SyncService::with_offline_mode()`.
The SDK retains ownership of synchronization, probes server availability while
offline, and restarts sync after connectivity returns. No additional app polling
loop or second sync service was introduced.

The GUI displays an offline/reconnecting status and retains timelines, composers,
and drafts. Room-list metadata refreshes no longer overwrite connection status.
The GUI also stops using the finite CLI initial-sync success/failure gate after
session restoration: SDK startup and recovery states are handled in its ongoing
session loop. `UnknownToken` notifications remain a sign-in-required failure,
rather than retrying a revoked session indefinitely.

A message can be queued while offline. If the SDK marks it Failed, use **Retry**
after reconnecting; this resumes the room queue with the existing transaction.
Synchronization recovery does not automatically retry every failed send. Messages
queued behind a failed send may wait for that room queue to be resumed.

Manual check: open an encrypted room, disable Wi-Fi, and wait for the offline
banner. Confirm drafts remain editable and the window stays open. Queue a message,
restore Wi-Fi, and wait for Connected. Retry any failed message, then send a new
message. Confirm each appears once in the other client. Repeat the outage and
close the UI while offline to verify cancellation of the SDK availability checks.

The mock-server regression switches sync and availability endpoints to HTTP 503,
checks that the same view can queue a message, restores the endpoints, retries the
failed send, sends another message, then shuts down during a second outage.
Physical Wi-Fi and live-server confirmation remain pending.

Starting the application entirely offline remains a separate limitation: the
existing saved-session restoration still validates identity with `whoami` before
entering this recoverable sync lifecycle. No change to that check was made here.


Local recovery checks: all 26 workspace tests, formatting, Clippy with warnings
denied, and locked binary build passed. The HTTP-503 outage/recovery regression
passed; a separate HTTP-401 `M_UNKNOWN_TOKEN` test confirms that revoked sessions
are reported as requiring sign-in rather than entering endless network recovery.

### User-run Wi-Fi recovery — 2026-11-14

The user confirmed that the Wi-Fi loss/reconnection correction works. This is
user-reported live evidence that the UI remains usable and can resume operation
after connectivity returns, without requiring an application restart.

The confirmation does not separately establish failed-send deduplication, shutdown
while offline, repeated suspend/resume, or startup without connectivity. Those
checks retain their existing validation status.


### Lifecycle and idle-resource follow-up — 2026-11-14

See [idle resource measurements](idle-resources.md) for the 100-cycle SDK timeline
ownership check, opt-in diagnostics, and host visible/hidden CPU, memory and
redraw captures. No stale timeline ownership was observed in the fixture. Hidden
windows delivered zero redraws in the sampled intervals; background CPU and
crypto-store flush activity were investigated further. After tracefs permissions
were corrected, two visible/hidden room-list capture pairs measured about 170 client scheduler wakeups/s with zero redraws or UI
updates. Background CPU remained about 3.5–3.6% of one core. Subsequent CPU
stacks attribute 70.8% of samples to crypto lease/lock work;
`sched_waking` stacks show worker handoffs and filesystem/flush completions.
The SDK lease-renewal path is the main optimization target, with profile-guard
lifetime auditing required before a single-process configuration experiment.
See the linked report for methodology and limits.


### Single-process store fix — 2026-11-14

Persistent clients now use SDK `SingleProcess` store mode, protected by the OS
profile lock retained until process exit. Late GUI handles and runtime-cleanup
timeouts cannot release that lock early. Reopening a used profile in the same
process is deliberately unsupported; restart the application. SQLite durability
and existing session/device keys are unchanged.

All 27 regular tests, the 100-cycle navigation stress test, Clippy, formatting,
and GUI build passed. The subprocess regression verifies exclusion through late
SDK handles and cleanup timeout, followed by acquisition after owner exit.
SDK outage/reconnect and encrypted sending fixtures run in the new mode.

The host A/B check reduced initial-thread-set wakeups from approximately 170/s
to 1.6/s, with CPU dropping from 3.6–3.8% to 0.17% of one core. A follow-up
that accounts for transient SQLite worker threads measured about 3 wakeups/s
for the whole client, mostly in sync-response bursts every 30 seconds. Separate three-second
traces found 51 crypto-store flushes before and none after. See
[the measurement report](idle-resources.md#single-process-fix-and-ab-check--2026-11-14)
for capture conditions and limits. No live messages were sent during this check.


### Static composer caret — 2026-11-15

Both room and thread composers now use a steady visible caret while focused.
Iced 0.14.2's text editor hard-codes its 500 ms blink timer, so the pinned
`iced_widget` crate is locally overridden with a small `cursor_blinking(bool)`
option, defaulting to upstream behavior. The composers explicitly disable it.
This suppresses the timer's scheduled redraws and keeps the focused caret drawn;
normal input, selection, and window-focus behavior remain in the widget.
See [the dependency patch notes](../../vendor/iced_widget/PATCH.md).

A real-widget regression delivers redraw events across multiple blink phases:
enabled blinking schedules a future redraw, while disabled blinking requests no
redraw after focus styling settles. All 28 regular tests, GUI build, formatting,
and application Clippy passed. The vendored pick list also removes its legacy `std::f32` import so
`f32::INFINITY` uses the associated constant without a deprecation warning.

Live host check: restored the existing session, selected an encrypted room,
focused its composer, and typed an unsent test draft. The caret was visibly
present, and the next 30 seconds showed zero redraw events and zero UI updates.
The draft was cleared afterward without sending a message. Earlier blinking
behavior produced 63 redraw events in 30 seconds in a focused thread composer;
these are redraw observations, not whole-process scheduler-wakeup counts. Raw
local evidence is in `artifacts/static-caret-2026-11-15/` (ignored).
