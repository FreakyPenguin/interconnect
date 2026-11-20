# Device verification

## Use

After an explicit password or browser sign-in, verification opens once as soon as
the session's verification worker is ready, unless Matrix already reports Verified.
The dialog explains how to compare with another signed-in Matrix app. **Verify
later** or Escape dismisses it; subsequent updates/reconnects do not reopen it.
Restoring a saved session and demo startup do not open verification automatically.
Unknown trust does not delay this sign-in step; the status reminder still requires
an explicit Unverified state from Matrix.

Registration opens [key backup and recovery](recovery.md) instead, allowing a new
account to save a recovery key without needing another device. The verification
dialog's **Recovery…** button also opens this screen; an existing recovery key can
restore secrets on a new device as an alternative to comparing with another app.

Account menu → Verify device… selects another device on the same account. For
incoming SAS, explicitly Accept request; requests never auto-accept or steal focus.
Reviewing an incoming request keeps its transaction and revision, skips automatic
device discovery, and hides device-selection/refresh/recovery actions until the
request is accepted or dismissed. An incoming request received during device
discovery remains available after discovery finishes. Stale device-row clicks
cannot start an outgoing request while an incoming request is pending.
Other users and QR verification are unsupported.

While Matrix reports this session as Unverified and verification is available,
an amber **Unverified session · Verify device…** reminder appears in the status
banner and the account avatar shows its attention indicator. The reminder opens
the existing verification dialog without changing conversations. Incoming requests
take priority; Unknown/Verified sessions have no reminder. Matrix trust updates
clear it, rather than comparison completion alone. No timer or polling is added.

Compare all seven emojis or three numbers before **They match**; mismatch/cancel
otherwise. Completion requires SDK done after both confirmations. Escape cancels
an active exchange; conversation/drafts survive. Demo is local and changes no trust.

Device trust and historical-key availability are separate. Use
[key backup and recovery](recovery.md) to import or manage recovery secrets.

The completion dialog offers **Request room keys in the background**, checked by
default. Close or Escape on that final dialog submits the choice. Interactive
verification does not start the pass before closing, even if Matrix already
reports Verified. Unchecking skips the pass for this session; ordinary on-demand
key requests still work. Stale/duplicate completion choices cannot reverse it.

After an enabled choice, Matrix must report Verified and the known joined/invited
room count must cover the server's room-list count. One background pass then reads
up to 50 recent encrypted events per joined conversation, most recently active
first. Already verified restored sessions retain the automatic startup pass.
The dialog choice is session-scoped, not a saved account setting. Unknown encryption
state is resolved first; unencrypted rooms, spaces, invitations, left rooms, and
bookmark storage are skipped.
The pass uses SDK decryption to queue missing session keys for the existing crypto
sync worker. It needs no recovery-key entry or configured backup; another verified
device must have the keys and process the requests. It does not guarantee recovery.

Only one room is read at a time, with a 30-second deadline per room. Failed rooms
are skipped; returned pagination tokens are discarded, including on empty pages.
This is a snapshot of the available rooms, not a scan of all historical messages;
rooms joined later and older sessions remain on demand. Cached room metadata can
contribute to the discovery count on restore. Reconnects do not restart the pass.
Completion releases the worker without polling or periodic retries. Loss of trust,
sign-out, and shutdown cancel remaining reads; SDK requests already queued remain
SDK-owned. No presentation timelines, subscriptions, read receipts, navigation,
drafts, or scroll positions are changed.

## Lifecycle

One event-driven worker/exchange per session. Discovery runs on open/refresh.
Deadlines: discovery/request 30 seconds, exchange three minutes, best-effort cancel
five seconds. Close/session replacement/shutdown cancels; interrupted HTTP may
remain on the peer until SDK timeout. Flow/session revisions and displayed values
reject stale/duplicate confirmations. SDK alone checks crypto and changes trust.
If both peers start SAS after acceptance, follow the SDK's selected exchange on
the existing transaction, including replacement of the superseded SAS handle.
Replacement clears the previous comparison and its confirmation gate; only the
current exchange's displayed values can be confirmed. This uses request-state
events without polling or additional timers.

## Acceptance

Use a dedicated account with a trusted second client:

1. Confirm device IDs; initiate each direction, accept explicitly, compare fully,
   confirm both sides, and wait for completion. Repeat between two Interconnect
   sessions; reviewing/accepting must retain the original request, without sending
   a request back or leaving both waiting. Also receive a request during device
   discovery and accept it after discovery finishes.
2. Test mismatch, either side cancelling, timeout, close during creation/exchange,
   and late confirmation.
3. Check 640×480, scrolling, missing emoji/numeric fallback, Escape, drafts, and
   [settled resources](idle-resources.md).
4. Check encrypted history, reporting keys separately from device trust/verification.
5. Check the reminder on an unverified session, including with the sidebar hidden;
   open/cancel verification and confirm it remains. Incoming requests should replace
   its wording; an SDK Verified update should remove it without losing drafts or
   the reading position. Check both themes and offline/reconnect states.
6. Sign in with password and browser separately. Verification should open once,
   with device discovery and a Verify later action, even if worker readiness is
   delayed. Dismiss and reconnect: it must stay closed. Retry a failed sign-in,
   restore a saved session, and quit while signing in; no stale dialog should open.
7. With a trusted second client online and no recovery key entered, finish
   verification. Check that the final checkbox starts checked and room reads wait
   for Close/Escape. Repeat with the checkbox cleared: no background pass should
   start for that session, while ordinary room opening still requests missing keys.
   With it checked, leave conversations unopened while the bounded background pass
   finishes. Open several encrypted conversations and check available recent keys;
   compare older history separately. Repeat with an already verified saved session,
   a large room list, offline/failing rooms, and the second client offline. Navigate
   and edit distinct drafts during the pass; preserve reading positions and unread
   state. Sign out/quit mid-read, and reconnect after completion: no remaining reads
   or repeated pass should run. Measure settled resources after prefetch and SDK
   responses finish, separately from the startup traffic.

## Automated coverage

The workspace suite uses real SDK cryptography against isolated mock servers.
It covers SAS/MAC completion and cancellation, simultaneous-start tie-breaking,
incoming requests during discovery, stale commands, and the final prefetch choice.
Background-key tests cover ordering, room filtering, bounded reads, missing-key
request deduplication, cancellation, and no repeated pass. UI tests cover one-time
onboarding, safe dismissal, minimum-window controls, and static redraw deadlines.
Live forwarding/recovery, server interoperability, native fonts, and CPU/wakeups
remain separate acceptance checks.
