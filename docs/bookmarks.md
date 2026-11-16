# Message bookmarks (MSC4482)

Use the star in a room/thread message's hover action panel to bookmark it or
remove its bookmark. **Saved messages** in the room list opens an account-wide
view, independent of the selected space. It shows 20 messages per page, their
source rooms and senders, and removal/open-conversation actions. **Back** or
Escape restores the conversation; drafts and reading positions are retained.
The sidebar badge shows the total saved-message count, including zero, once
bookmark history is complete. It updates as bookmarks are added or removed.
**Go to message** opens the regular room timeline and centers the saved event,
loading older history as needed. Messages after it remain available below, and
live updates continue; scroll freely in either direction. Replies open their
regular thread when its metadata is available. Centering happens once, after
history settles, so subsequent scrolling stays under your control. **Jump to
newest** returns to the latest messages.
Unavailable events show a loading error; the bookmark remains removable.

The view has no composer. Saved message previews resolve the original events
through SDK event-focused timelines and follow edits/redactions while open.
Unavailable messages keep their bookmark and can be removed. No source message
body is copied into the bookmark's fallback text. Previews are text descriptions;
saved images/files use their message caption or filename rather than a second
attachment browser.

## Protocol and SDK boundary

This is an experimental implementation of
[MSC4482](https://github.com/matrix-org/matrix-spec-proposals/pull/4482), checked
against revision `cd24f359b241b9c0e77232d1612d6f9af23f8595`. The proposal is a draft
and has not assigned unstable wire names. `src/bookmarks.rs` isolates our names:

| Purpose | Written name |
| --- | --- |
| Room type | `org.matrix.msc4482.bookmarks` |
| Timeline event | `org.matrix.msc4482.bookmark` |
| Pointer content block | `org.matrix.msc4482.pointer` |
| Global account data | `org.matrix.msc4482.bookmarks_room` |

The account-data content is `{ "active_bookmarks_room_id": "!room:server" }`.
The pointer contains `room_id`, the original `event_id`, and `via`. The event
includes `m.text` and `org.matrix.msc1767.text` text blocks containing a Matrix
URI and source room name. Readers also accept the draft's `m.bookmarks`,
`m.bookmark`, `m.pointer`, and `m.bookmarks_room` names. This does not establish
interoperability with implementations choosing different experimental prefixes.
Fallback rendering requires another client to support extensible event text;
legacy clients may show an unsupported event instead.

The first explicit save creates an encrypted, invite-only room containing only
the account owner. The client waits asynchronously for its synced metadata
before validating it and publishing the account-data pointer. Opening the saved
view alone never creates a room. Ordinary room/space lists exclude both
experimental and draft-stable bookmarks rooms. Bookmarks remain ordinary Matrix
room events on the server and use the SDK's existing encrypted store/cache.
Neither matrix-sdk nor Ruma is vendored or modified for this feature.

The session-owned worker hydrates the complete storage-room event cache before
exposing binary bookmark state. It handles custom bookmark replacements with
same-sender/type validation, groups duplicate pointers, and removes all known
duplicates on unbookmark. The SDK owns event-cache gaps, pagination, encryption,
raw redaction and later decryption. Unknown history or missing keys disables
bookmark writes and provides loading/error feedback and explicit refresh.

Writes use the SDK's encrypted `Room::send_raw`/redaction APIs and await server
confirmation. They require a working connection; they do not enter the ordinary
offline message send queue. Failed operations retain transaction IDs for retry
within the running session, and acknowledged changes remain visible before sync
echoes arrive. Shutdown waits for admitted operations, bounded by their deadline.
Across a restart, discovery/history loading reconciles the server's actual state.

An inaccessible, unencrypted, shared, public, or upgraded storage room is never
silently replaced. A created room is retained if publishing account data fails;
one discoverable orphan can be reused on retry. Account-data writes and room
creation cannot be atomic across devices. The implementation rechecks the pointer
before publishing, preserves orphan rooms, and leaves ambiguous multiple-room
recovery to explicit handling. Automatic room-upgrade migration is deferred.

## Resource and validation scope

The storage watcher uses an SDK event-cache subscription. Initial hydration
loads all bookmark history, in batches of 100; first-load cost and retained
memory scale with history. There is no separate plaintext bookmark database.
Only the visible page has source timelines and explicit source-room sync
subscriptions, with four setups at a time and a 25-second setup deadline.
Paging, closing, navigation and account changes cancel old preview streams;
generation checks reject already queued results. Settled workers wait on SDK
events, with no polling, recurring animation timer, or automatic read receipts.
Saved-event navigation uses the normal SDK live/thread timeline and asynchronously
loads the intervening history from newest to the saved event, plus older context.
This initial navigation has a 60-second deadline and ends on navigation/shutdown;
its memory and rendering cost scale with the intervening history. No separate
context-only timeline or forward-pagination boundary is introduced.
Actual desktop CPU/wakeups and large-account memory have not been measured.

See [connected validation](validation/connected-ui.md#message-bookmarks--2026-11-15)
for automated evidence and the remaining two-client/native acceptance procedure.
