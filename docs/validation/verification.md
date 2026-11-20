# Device verification

## Use

After an explicit password or browser sign-in, verification opens once as soon as
the session's verification worker is ready, unless Matrix already reports Verified.
The dialog explains how to compare with another signed-in Matrix app. **Verify
later** or Escape dismisses it; subsequent updates/reconnects do not reopen it.
Restoring a saved session and demo startup do not open verification automatically.
Unknown trust does not delay this sign-in step; the status reminder still requires
an explicit Unverified state from Matrix.

Account menu → Verify device… selects another device on the same account. For
incoming SAS, explicitly Accept request; requests never auto-accept or steal focus.
Other users and QR verification are unsupported.

While Matrix reports this session as Unverified and verification is available,
an amber **Unverified session · Verify device…** reminder appears in the status
banner and the account avatar shows its attention indicator. The reminder opens
the existing verification dialog without changing conversations. Incoming requests
take priority; Unknown/Verified sessions have no reminder. Matrix trust updates
clear it, rather than comparison completion alone. No timer or polling is added.

Compare all seven emojis or three numbers before **They match**; mismatch/cancel
otherwise. Completion requires SDK done after both confirmations. Escape cancels;
conversation/drafts survive. Demo is local and changes no trust.

Own-device Unknown/Unverified/Verified status is separate from cross-signing and
historical keys. No backup/recovery/cross-signing setup UI; SDK owns key forwarding.

## Lifecycle

One event-driven worker/exchange per session. Discovery runs on open/refresh.
Deadlines: discovery/request 30 seconds, exchange three minutes, best-effort cancel
five seconds. Close/session replacement/shutdown cancels; interrupted HTTP may
remain on the peer until SDK timeout. Flow/session revisions and displayed values
reject stale/duplicate confirmations. SDK alone checks crypto and changes trust.

## Acceptance

Use a dedicated account with a trusted second client:

1. Confirm device IDs; initiate each direction, accept explicitly, compare fully,
   confirm both sides, and wait for completion.
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

UI fixtures cover the one-time sign-in handoff, delayed worker readiness, dismissal,
failure/retry, saved-session restoration, verified sessions, and shutdown. They do
not perform live sign-in or verification.

Crypto fixtures exchange real SDK messages between in-memory clients via mock
servers. They establish local SAS/MAC/cancellation behavior, not live cross-signing,
server interoperability, key recovery, native fonts, or CPU/wakeups.
