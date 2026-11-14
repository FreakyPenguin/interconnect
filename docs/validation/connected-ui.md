# Connected Iced prototype

The `matrix-ui` binary connects Iced to the existing persistent session. It restores
credentials without browser login, lists joined rooms, and displays SDK room and
thread timelines. Opening a room explicitly requests an initial history page of
20 events through SDK back-pagination while continuing to display live updates.
It is read-only: no composer, thread-pagination controls, verification
UI, recovery, receipts, or automatic read marking are implemented. Use an already
verified profile with access to the test room's keys.

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
- Restoration retains the exclusive profile lock through sync shutdown and shares
  the CLI's device-key and server-identity checks. SDK stop has a 15-second bound.
  Iced owns final runtime teardown; unlike the CLI's runtime wrapper, total GUI
  process teardown is not independently bounded if blocking SDK database work stalls.
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
