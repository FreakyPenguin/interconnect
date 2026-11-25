# Virtual timeline rows

The existing Iced Scrollable retains input, clipping, scrollbar, and native
scroll operations. A repository-owned `virtual_rows::Rows` widget supplies a
virtual content height and positions only mounted children at their prefix
offsets. No Iced vendor changes or global update timer are required.

`timeline_projection::Cache` describes projected room/thread rows by source
indices and stable event/summary keys. It is reused while the SDK row snapshot,
generation, thread root, projection mode, and read boundary are unchanged.
Grouping, dates, thread previews, and unread decoration consult the full
projection, so a mount boundary cannot become a false conversation boundary.
A complete source-event index also resolves visible message actions, reply
previews, and thread originals without rescanning loaded history for each row.
Before a matching snapshot/generation has a cached index, lookups retain the
ordinary guarded fallback against current data.

Conversations of at most 192 projected rows use exact full layout. Longer
conversations mount the visible region plus roughly two viewport heights in
each direction. Unknown rows begin with estimated heights; a Fenwick prefix
index resolves pixel positions and incorporates measured corrections. Heights
survive source updates by key. Reflow measures only mounted rows; unmounted
measurements remain estimates until revisited. A bounded convergence loop fills
the overscan after estimates change. A populated window is reused until the
viewport approaches its buffer boundary, avoiding unnecessary cold shaping on
small scrolls. Rows share widget state by key while mounted and release heavy
state as they leave the window.

The timeline observer captures the first actually visible event and its screen
offset. After measurement corrections, prepend, or reflow, it restores that
logical anchor. Previously virtualized conversations also reopen by their saved
event-relative anchor when away from newest; old bottom pixel offsets refer to
estimates that are rebuilt with a new pane. Short histories keep exact in-session
pixel restoration; conversations with saved code-view state also prefer their
event-relative anchor when away from newest. Settled geometry updates the saved
scroll distance after reflow, so an old newest offset cannot override a code
expansion. An explicit newest position overrides a stale saved anchor. Bottom-following
remains separate. An explicit event target mounts its indexed neighborhood
immediately, then centers the measured target or restores its saved event-relative
offset. Native scroll operations request
their newly needed layout on the next update; they do not require constructing
intermediate rows. Viewport changes invalidate layout only as they approach a
mount boundary, with no repeating idle work.

`RowsVisible`, read-receipt visibility, persisted reading anchors, and jump
highlight activation use actual viewport intersection. Overscan and virtual
empty space do not count as viewed content. History demand continues to use
scroll metrics and existing generation/revision checks.

Selection retains logical message endpoints and gets canonical text from an
ordered model adapter. Mounted widgets provide hit-testing and painting; copy
can include unmounted intermediate messages. Canonical rich text preserves
hidden-spoiler placeholders, object alternatives, and formula source. A removed
or edited endpoint invalidates a selection rather than copying obsolete text.

A bounded LRU cache holds the last 64 room/thread conversations' logical
selections and sparse document-scoped semantics: spoiler reveals, code expansion,
and horizontal code offsets. These survive widget eviction and conversation
navigation during the session. Restoration can trigger a bounded reflow before
measuring anchors. Document changes reject stale semantics; account transitions
clear the cache. It retains no widget trees or SDK handles and is not persisted
across restarts. Persisted event anchors and drafts are separate.

The pinned thread original retains its own selection, code semantics, and
top-anchored scroll offset in that same conversation entry. It does not share
reply selection or bottom-following state. A transient loading view cannot erase
its retained range or scroll position. Sidebar/thread-header presses suspend
keyboard Copy while retaining logical ranges for navigation. Clicking message
text replaces the range; returning restores it without stealing Copy from another
focused field.

Code controls preserve selection and anchor the message body even when expansion
begins at newest. Corner icon overlays, the bottom Expand cue, and horizontal
scrollbars are renderer chrome, excluded from canonical text. Drag/track/wheel
input changes the same horizontal offset; release or focus loss ends capture.
These interactions add no idle timer. See [code rendering](message-formatting.md#bounded-code-previews).

Projection caches clear on pane navigation/close and account transitions. They
must not extend SDK snapshot/handle lifetimes. Widget virtualization bounds
heavy rendering state, not loaded event data, height metadata, or explicit copy
output. Collapsed code shapes at most 12 lines / 2 KiB; expansion opts into full
code layout, and collapse releases those paragraphs. Plain prose still lays out
as a whole. Offscreen height estimates mean scrollbar proportions can adjust
while exploring history.

See [performance validation](../validation/long-history.md) for the opt-in
mixed-history and individual-message workloads, and the distinction between
software frame costs, RSS, native GPU/input behavior, and idle measurements.
