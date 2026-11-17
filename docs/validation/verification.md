# Device verification

## Use

Open the account menu → **Verify device…**, choose another device on the same
account, and accept on that client. For an incoming to-device SAS request, choose
**Verification requested → Accept request**. Requests never auto-accept or steal
focus. Other users' requests and QR verification are outside this flow.

Compare all seven emojis or all three numbers. Choose **They match** only when
the full comparison agrees; otherwise choose mismatch or cancel. Completion
requires the SDK's done state after both clients confirm. Escape closes the
dialog and cancels an active exchange. The open conversation and drafts survive.
Demo uses a local sample without accounts, trust changes, or Matrix requests.

Own-device trust is reported separately as Unknown, Unverified, or Verified.
A successful comparison does not promise cross-signing trust or historical keys.
Key backup, recovery-secret entry, and cross-signing setup/reset are missing;
room-key requests/forwarding remain SDK-owned.

## Lifecycle

One session owns an event-driven worker with at most one active exchange.
Discovery happens only on open/refresh. Discovery/request creation have 30-second
deadlines, an accepted/created exchange has three minutes, and best-effort cancel
has five seconds. Dialog close, session replacement, and shutdown cancel work.
An interrupted HTTP request may remain on the peer until its SDK timeout.

Session/flow revisions and displayed comparison values reject stale or duplicate
confirmations. The SDK performs key generation, MAC/signature checks, and trust
updates; the UI never marks a device trusted directly.

## Acceptance

Use a dedicated account with another trusted client online:

1. Confirm account/local device identity, select the intended peer by device ID,
   accept there, compare everything, confirm both sides, and wait for completion.
2. Initiate from the other client; check review and explicit acceptance.
3. Exercise mismatch, either side cancelling, timeout, close during request
   creation/exchange, and a late queued confirmation.
4. Check 640×480 layout, scrolling, missing emoji fonts/numeric fallback, Escape,
   draft preservation, and [settled resource use](idle-resources.md).
5. Open existing encrypted history. Report key availability separately from
   verification and reported own-device trust.

Automated fixtures exchange real SDK crypto messages between two in-memory
clients through local mock servers, including cancellation and stale commands.
They establish local SAS/MAC behavior, not live cross-signing, server
interoperability, historical-key recovery, native fonts, or CPU/wakeups.
