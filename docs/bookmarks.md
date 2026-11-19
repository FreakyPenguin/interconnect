# Saved messages

Use a message's bookmark action to save/remove it. The toolbar bookmark icon opens
account-wide saved messages, 20 per page. Back/Escape preserves drafts and reading
positions. **Go to message** opens the original room/thread, loading history as needed.

## Storage

The first explicit save creates an encrypted, invite-only self-room. Opening the
view alone creates nothing. Writes follow experimental
[MSC4482](https://github.com/matrix-org/matrix-spec-proposals/pull/4482), revision
`cd24f359b241b9c0e77232d1612d6f9af23f8595`. Stable `m.*` equivalents are also
accepted when reading. Experimental names may differ in other implementations;
legacy clients may show an unsupported event. Storage rooms are hidden from
ordinary room/space lists.

| Purpose | Event/account-data type |
| --- | --- |
| Storage room | `org.matrix.msc4482.bookmarks` |
| Bookmark event | `org.matrix.msc4482.bookmark` |
| Source pointer | `org.matrix.msc4482.pointer` |
| Account-data room selection | `org.matrix.msc4482.bookmarks_room` |

Account data uses `active_bookmarks_room_id`. Pointers contain `room_id`,
`event_id`, and `via`; extensible text holds a link/name without copying private
source message bodies into storage.

## Behavior and limits

Initial hydration loads all bookmark history; its cost scales with that history.
Writes require complete decrypted bookmark history and await server confirmation;
they do not use an offline send queue. Retries reuse stable transaction IDs within
a session. Unavailable source previews can still be removed. Visible sources
follow edits/deletion. Invalid, shared, public, upgraded, or inaccessible storage
is never silently replaced; uncertain room creation requires explicit recovery.

Previews load only for the visible page, with four setups at most and no polling.
Original navigation can grow timeline memory with intervening history; cancellation
and subscriptions must preserve the open conversation.

See [UI acceptance](validation/connected-ui.md#sending-and-room-management).
