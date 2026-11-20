# Device verification

## Use

After password/browser sign-in, verification opens once unless Matrix already
reports Verified. Verify later/Escape dismisses it until the next explicit login;
restoring a saved session does not reopen it. Registration offers
[key recovery setup](recovery.md) instead.

Account menu → Verify device… selects another device on the same account.
Incoming SAS requests need explicit acceptance and do not steal focus. Reviewing
one keeps its transaction, hides outgoing discovery/start actions, and cannot
send a reverse request. Requests arriving during discovery remain available.
Other-user verification and QR are unsupported.

Compare all seven emojis or three numbers before They match; otherwise mismatch
or cancel. Completion requires the SDK to finish after both confirmations.
Escape cancels an active exchange without losing drafts or the conversation.
Matrix's Unverified state shows an amber reminder/account indicator; only a trust
update clears it. Unknown/Verified states have no reminder. Device trust does not
guarantee historical-key availability; recovery can import the missing secrets.

## Background room keys

The final dialog's Request room keys in the background checkbox starts checked.
Close/Escape submits that session's choice; interactive verification starts no
pass before this choice, even if trust is already Verified. Unchecking skips the
pass; normal on-demand requests still work. Verified restored sessions start the
pass automatically. Duplicate/stale choices cannot change the decision.

After verification and initial room discovery, one pass reads up to 50 recent
encrypted events per joined conversation, most-recent first. It resolves unknown
encryption state and skips plaintext rooms, spaces, invitations, left rooms, and
bookmark storage. Reads run one at a time with a 30-second room deadline, without
pagination/retry. It discards content/tokens and changes no timeline, receipt,
navigation, draft, or scroll position.

SDK decryption queues missing-key requests for its existing crypto worker. No
recovery key/backup is required, but another verified device must have and share
the keys. Older history and later-joined rooms remain on demand. Reconnect does
not repeat the pass; trust loss/session teardown cancels remaining reads. Already
queued SDK requests remain SDK-owned. Exclude initial work/key responses from
settled [resource measurements](idle-resources.md).

## Lifecycle

One event-driven worker/exchange per session. Discovery/request deadline: 30 seconds;
exchange: three minutes; best-effort cancellation: five seconds. Close/session
replacement/shutdown cancels, though peer requests may survive until SDK timeout.
Revisions and displayed values reject stale/duplicate confirmations. Simultaneous
SAS starts follow the SDK's selected exchange on the same transaction; replacement
clears the old comparison/confirmation. No polling or animation timers.

## Connected acceptance

Use dedicated profiles and a trusted second client:

1. Confirm device IDs, initiate each direction, accept, compare, and confirm both
   sides. Repeat between two Interconnect sessions and receive during discovery;
   retain one transaction and never send a reverse request.
2. Test mismatch, peer cancellation, timeout, close while pending, and late clicks.
3. At 640×480 in both themes, check full comparisons, fixed actions, Escape, and
   unchanged drafts/older positions. Check missing-emoji numeric fallback.
4. Sign in via password and browser; dismiss onboarding and reconnect. It stays
   dismissed. Restore a saved session, retry failed login, and quit mid-login:
   no stale prompt opens. Check Unverified reminders with the sidebar hidden;
   incoming requests take priority, and actual trust updates clear the reminder.
5. Complete with prefetch enabled and disabled. Room reads wait for final Close/
   Escape; disabled still permits on-demand keys. Check recent versus old history,
   large room lists, offline/failing rooms/peer, verified restore, mid-read sign-out,
   and reconnect after completion. Navigate/edit drafts during the pass; preserve
   reading positions and unread state. Measure settled resources separately.

## Automated coverage

The workspace suite uses real SDK cryptography against isolated mock servers for
SAS/MAC completion/cancellation, simultaneous starts, incoming/discovery ordering,
stale commands, prefetch choice, room filtering/bounded reads, deduplicated requests,
and cancellation. UI tests cover onboarding, dismissal, minimum-size controls, and
redraw deadlines. These do not establish live interoperability or native behavior.
