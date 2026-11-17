# Saved messages

Use a message's bookmark action to save/remove it. The toolbar bookmark icon opens
account-wide saved messages, 20 per page; its count badge is hidden at zero.
Back/Escape returns without changing the conversation's draft or reading position.
**Go to message** opens the original room/thread and loads intervening history
as needed; normal newest/history navigation remains available.

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

A runtime-owned SDK service loads previews only for the visible page, with at most
four setups in flight and no polling. Original-event navigation uses normal room
or thread timelines, so memory can grow with intervening history. Cancellation
and source subscriptions must preserve the separately open conversation.

See [UI acceptance](validation/connected-ui.md#sending-and-room-management).
