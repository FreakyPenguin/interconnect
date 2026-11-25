# Virtual timeline rows

Iced Scrollable owns input, clipping, scrollbars, and native scroll operations.
`virtual_rows::Rows` supplies virtual content height and mounts children at prefix
offsets, without vendor changes or a global timer.

## Projection and layout

`timeline_projection::Cache` keys rows by stable event/summary identity and reuses
projections while snapshot, generation, thread root, mode, and read boundary match.
Grouping/dates/thread previews/unread decoration use the full projection; mount
boundaries cannot create false conversation boundaries. An event index resolves
actions, replies, and originals, with a guarded fallback before the index is ready.

Up to 192 projected rows use exact full layout. Larger conversations mount the
viewport plus roughly two viewport heights each way. A Fenwick prefix index starts
with estimated heights and incorporates measurements by row key. Reflow measures
mounted rows; a bounded convergence loop fills overscan after corrections. Reuse
the populated window until near its boundary, sharing widget state by key and
releasing heavy state on unmount. Offscreen estimates can change scrollbar proportions.

## Anchors and visibility

The observer captures the first actually visible event and its screen offset.
Restore it after prepend, measurement corrections, or reflow. Virtualized and
code-state conversations reopen by event-relative anchor when away from newest;
short histories otherwise retain exact in-session pixel offsets. Settled geometry
updates saved distance so old offsets cannot undo expansion. Explicit newest wins
over stale anchors; bottom-following stays separate.

Event targets mount their indexed neighborhood, then center the measured event or
restore its saved offset. Native scroll operations request the next layout without
constructing intervening rows. Viewport updates invalidate layout near mount
boundaries, with no recurring idle work.

`RowsVisible`, receipts, persisted anchors, and jump highlights use actual viewport
intersection. Overscan/empty virtual space never counts as viewed. History demand
retains generation/revision guards.

## Selection and retained state

Logical selection endpoints copy canonical text from an ordered model adapter,
including unmounted rows, hidden-spoiler placeholders, object alternatives, and
formula source. Widgets provide hit-testing/painting. Edited/removed endpoints
invalidate selection instead of copying obsolete text.

A 64-conversation LRU retains logical selections, spoiler reveals, code expansion,
and horizontal offsets. Restoration may reflow before measuring anchors. Source
changes reject stale semantics; account transitions clear the cache. It holds no
widget trees/SDK handles and does not persist across restarts. Drafts and persisted
reading anchors are separate.

Pinned thread originals retain independent selection/code state/top-anchored scroll,
including through loading views. Navigation controls suspend keyboard Copy while
retaining ranges; returning cannot steal Copy from another focused field.

Code controls preserve selection and anchor the message body. Chrome is excluded
from canonical text; drag/track/wheel share horizontal offset and release/unfocus
ends capture. Collapsed code shapes at most 12 lines / 2 KiB; expanded code and plain
prose use full layout. See [code rendering](message-formatting.md#bounded-code-previews).

Projection caches clear on navigation/close/account changes without extending SDK
handle lifetimes. Virtualization bounds widgets, not loaded event data, height
metadata, or copy output. See [performance validation](../validation/long-history.md).
