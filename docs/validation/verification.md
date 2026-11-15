# Desktop device verification

## Scope

The footer's **Verify device** action opens a dialog for SAS verification with
another device on the same Matrix account. Choose your existing trusted client
from the device list and accept its request. Alternatively, initiate verification
on that client, then select **Verification requested** and **Accept request** in
Interconnect. Incoming support covers to-device SAS requests from your own
account; other users' requests and QR verification are outside this flow.

Compare all seven emojis, or all three numbers, with the other client. Select
**They match** only when the complete comparison agrees. Otherwise choose
**They don't match** or **Cancel verification**. Escape closes the dialog and
cancels an active exchange. Confirmation is disabled after a local match while
waiting for the other client; completion requires the SDK's done state on the
exchange. Device IDs are shown to help identify the intended peer.

The dialog reports the SDK's own-device trust independently as **Unknown**,
**Unverified**, or **Verified**. A completed SAS comparison does not by itself
promise cross-signing trust or historical message-key availability. Key backup,
recovery-secret entry, cross-signing setup/reset, and account recovery are not
implemented. Existing automatic room-key requests/forwarding remain SDK-owned;
see [the earlier two-client historical-key exchange](interactive.md#user-run-historical-key-exchange--2026-11-14).

## Lifecycle and resource behavior

One session owns one verification worker and a scoped SDK event handler. Trust
and incoming requests arrive through SDK streams/events. Device discovery runs
only on explicit opening/refresh; no periodic query or animation is added.
Incoming requests are bounded and never accepted automatically. One active
exchange is allowed at a time; additional requests are not presented while busy.

A verification has a three-minute deadline after request creation/acceptance.
Device discovery and outbound request creation have 30-second deadlines.
Cancellation sends a best-effort SDK cancel with a five-second deadline. Closing
the dialog cancels its active exchange; application close and session replacement
cancel and join the worker before sync is stopped. The SDK handler is removed
with the service. A request whose creation HTTP operation was interrupted may
remain on the peer until its SDK timeout; it is never automatically confirmed.

Commands carry session/flow revisions, and match commands also carry the
comparison's displayed numbers. Obsolete, premature, mismatched, duplicate, and
post-cancellation confirmations are rejected. The SDK handles key generation,
MAC exchange, signature validation, and device trust; the UI does not mark a
device trusted directly.

The modal captures pointer input and blocks background navigation/composition.
It preserves the open conversation, drafts, and scroll positions. Escape closes
and cancels the dialog, including during input-method composition. General
keyboard button traversal/focus restoration remains pending in the UI.
Conversation notification suppression is suspended while the dialog covers it.
The footer updates when a request arrives but does not steal focus or open a
modal automatically.

Demo mode offers a local sample device and comparison through the same footer.
Its completion is explicitly labeled as a preview and never changes device trust,
accesses saved accounts, or sends Matrix requests.

## Automated evidence — 2026-11-15

The protocol fixture runs two independent matrix-sdk clients with real crypto
stores in memory. Local mock homeservers serve their signed device keys and route
verification events between them; no real account or homeserver is accessed.
It covers outgoing requests, explicit incoming acceptance, equal seven-emoji and
three-number comparisons, and completion with the peer devices locally verified.
Each side sends exactly one MAC despite duplicate confirmation attempts.

Negative checks cover stale revisions, wrong numbers, early/duplicate
confirmation, mismatch, cancellation followed by a queued/late confirmation,
immediate close after starting from an older UI snapshot, closed controls after
shutdown, and idle services making no HTTP requests. Mismatch and cancellation
send no MAC and do not verify the peer. GUI checks cover demo isolation, draft
and scroll preservation, modal input blocking, notification visibility, and all
phases at the minimum 640×480 window size.

Validation passed: `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace` (73 library and 111 GUI tests passed; one
existing stress test ignored), and `cargo build --locked --bin interconnect`.
The six verification protocol/lifecycle tests also passed after the final
confirmation guards were tightened.

These checks exercise cryptographic SAS exchange and the SDK's local peer-device
trust. They do not establish a live account's cross-signing status, historical-key
recovery, server interoperability, native emoji rendering, or actual idle CPU and
wakeups. No connected client was launched or restarted, no verification was sent
to a real device, and no chat messages were sent.

## Manual acceptance

Use an authorized test account with another trusted Matrix client online:

1. Launch Interconnect with the test profile. Open **Verify device**, confirm
   account/local device identity, and choose the intended peer by device ID.
2. Accept on the other client. Compare the complete emojis/numbers, confirm on
   both, and wait for completion. Record the reported own-device trust separately.
3. Initiate a fresh request from the other client. Check the footer prompt and
   explicit acceptance. Exercise mismatch, cancellation, peer cancellation,
   timeout, and application close during a pending exchange.
4. Open an existing encrypted test conversation and check whether historical
   keys arrive from the trusted client. Report unavailable keys separately from
   verification failure. No new test message is needed.
5. Check minimum-window layout, emoji labels/numbers with missing emoji fonts,
   Escape, and draft/scroll preservation. Compare settled idle
   resources with [the existing procedure](idle-resources.md), including a closed
   dialog and a pending request. Do not save comparisons or private messages in
   tracked validation artifacts.

Native desktop and two-client acceptance remain pending for this desktop flow.
