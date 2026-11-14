# Interactive verification and encrypted-thread checks

This is a milestone 0 CLI feasibility harness, using the existing persistent
session. It does not mark device trust or thread support verified merely because
the commands compile. Keep the host Secret Service connected and unlocked.

```sh
cargo run --locked --bin spike -- session-live
```

An optional profile directory is the final argument. The process holds the profile
lock, validates the saved device identity, waits for a fresh room-list sync, and
stays online until `quit`, EOF, or Ctrl-C. It retains credentials on exit and uses
the bounded shutdown path. No browser login is required. It sends no chat messages
unless you issue `send-room` or `send-thread`.

## Verify against your second client

1. Keep your already trusted second client online and open its verification UI.
2. Type `devices` in the console. Identify that client's device ID (not the ID of
   this harness). Device names are shown as quoted text.
3. Type `verify DEVICE_ID`. The console requests SAS verification of that device
   on your own account. Accept the request in the second client.
4. Compare all seven emojis or all three numbers shown by both clients. Only if
   they match, enter the complete `match NUMBER NUMBER NUMBER` command printed
   by the console, and confirm in the second client. Generic `match`, an early
   confirmation, wrong numbers, or repeated confirmation will not confirm SAS.
5. On disagreement use `mismatch`; to abort use `cancel`. The exchange has a
   three-minute deadline after request creation. `quit` during verification
   cancels that exchange; type `quit` again to exit the console.
6. Wait for “SAS verification completed on both devices.” Then run `status` and
   record the SDK's device-trust state separately. A completed SAS exchange does
   not itself prove cross-signing trust or availability of historical room keys.

The console initiates verification; it does not automatically accept unsolicited
requests from other devices or users. No recovery secrets are requested here.

## Use a dedicated encrypted test room

Prepare a small encrypted room in the second client and invite/join the harness
account there if necessary. Avoid sending test messages into an unrelated room.

```text
rooms
room !YOUR_TEST_ROOM:SERVER
send-room milestone-0 root
```

The `room` command rejects an unencrypted room. It prints the explicit room
target, displays the last 20 loaded timeline items, and subscribes that room to
live sync. Local echoes show pending/failed/sent state. A queued send is not proof
of delivery; wait for its event ID/state and check decryption in the second client.
Do not manually resend an uncertain send merely because acknowledgement is slow.
The SDK send queue can retain pending messages across process restarts.

Copy the root's event ID from the console (or the second client's event details):

```text
thread $ROOT_EVENT_ID
send-thread milestone-0 threaded reply
page-thread
show
```

`send-thread` requires an open thread and delegates thread relationship creation
to its SDK thread-focused timeline. `send-room` always uses the room timeline and
creates a new ordinary message. This harness does not yet expose ordinary replies,
edits, reactions, redactions, receipts, or send retry controls.

Both views display event IDs, text, local send state, undecryptable-event markers,
and SDK thread summaries where present. Terminal output is local test evidence;
it can contain decrypted message text. Avoid recording private conversations or
posting raw output. Live snapshots and `show` display the latest 20 items and report duplicate
loaded event IDs. Pagination also prints the first 20 loaded items so that older
history can be inspected; `show-oldest` repeats that view. A latest-items snapshot
alone cannot establish whether older events were fetched. Pagination requests 20 older events per command; SDK pagination
may fetch additional batches internally. Verify ordering and completeness against
the other client; the duplicate check alone does not establish correctness.

Exercise these cases:

- Have the second client reply while the thread is open; check live arrival and
  decryption in both clients.
- Run `close-thread`, send a reply from the second client, then reopen the same
  root with `thread $ROOT_EVENT_ID`; check the reply and root summary.
- Create more than 20 replies, use `page-thread`, and check page overlap, root
  retention, ordering, and missing/duplicate events. Use `page-room` for room
  history and try a root outside the currently loaded room timeline.
- Type `quit`, restart `session-live`, and reopen the same room and thread. Check
  the same account/device, historical reply decryption, and newly arriving replies.
- Observe unavailable historical keys separately from failed login or failed
  device verification. This harness does not perform backup recovery.

Opening/restoring views does not explicitly send read receipts. Selection is
currently re-entered after restart; room/thread navigation state is not saved.
Record room/thread unread behavior in the second client without claiming this
CLI implements the final read-state UI.

One room view and one thread view are owned at a time. Changing rooms removes the
old room's explicit sync subscription; closing/replacing a view aborts its display
watcher. Account sync continues while the thread is closed. Display output is
limited, but SDK timeline/event-cache budgets are not yet configured: this is a
short-lived feasibility harness, not final bounded-cache acceptance.

## Evidence to report

Report only whether verification completed, the SDK trust state, and which test
cases passed or failed. No passwords, tokens, recovery keys, or private message
contents are needed. The recorded results below distinguish completed two-client checks from
remaining edge-case and read-state checks.

## Implementation checks — 2026-11-13

Locked binary build, formatting, Clippy with warnings denied, and all 14 tests
passed. SAS confirmation tests reject early/generic confirmation, mismatched
numbers, and repeated confirmation. Existing persistence and fresh-sync/shutdown
regressions continue to pass.

A live `session-live` smoke run against the existing profile executed only
`status` and `quit`: it restored the saved device, completed a fresh sync, reported
`Unverified`, stopped sync, and exited with status 0. No verification request or
chat message was sent, and no room/thread content was displayed. That smoke run did not exercise SAS comparison or encrypted messages; see the
subsequent user-run results below.


## User-run verification and messaging — 2026-11-13

The user reported successful device verification and encrypted message sending
and receiving both with and without threading using the interactive harness.
This is user-reported two-client evidence for:

- Device verification against the existing client.
- Ordinary encrypted room-message sending and receiving.
- Encrypted thread-reply sending and receiving.

The exact SDK `status` output was not supplied, so no particular cross-signing
state is inferred. These checks support the SDK's basic messaging/thread
feasibility; they do not establish complete timeline correctness.

The subsequent user confirmation covers all four requested follow-up steps:

- Exit and restart `session-live`, then reopen the same room/thread.
- Decrypt previous messages and receive new replies after restart.
- Close the thread, receive a reply from the second client while it is closed,
  and see that reply after reopening it.
- Paginate a thread with more than 20 replies and check ordering and duplicates.

Result: **pass**, reported by the user, for these scenarios. Together with the
previous send/receive results, this completes milestone 0's thread prototype
acceptance check. It does not complete the production messaging milestone.

Still unverified: roots outside loaded room history, receipts/unread behavior,
message navigation, historical keys unavailable to the existing device, and
broader cache/lifecycle stress cases. Exact page counts, event counts and raw
traces were not supplied; these are manual scenario results, not an exhaustive
ordering/deduplication proof. No private room IDs or message contents are needed.
