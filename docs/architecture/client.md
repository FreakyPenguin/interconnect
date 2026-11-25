# Client architecture

## Dependencies and rendering

Iced 0.14 and matrix-sdk/matrix-sdk-ui 0.19.1 are pinned in
[Cargo.toml](../../Cargo.toml) and [Cargo.lock](../../Cargo.lock). The SDK owns
native sliding sync, encrypted SQLite persistence, E2EE, and room-key requests
under its trust policy. Linux uses Wayland; native sliding sync is required.

wgpu is the default, with tiny-skia fallback at initialization. Device loss does
not switch an already running renderer. Graphics settings are read before
Iced/Tokio initialization; a Unix re-exec applies environment changes safely.
See [renderer selection](../development.md#renderer-selection).

Shared outline icons cache canvas geometry without timers. Local patches provide
[static editor carets](../../vendor/iced_widget/PATCH.md),
[embedded-object metrics](../../vendor/cosmic-text/PATCH.md), and
[Winit file drops](winit-file-drops.md). Formatted messages use a bounded
[native document renderer](message-formatting.md).

## Runtime and session ownership

One Tokio runtime lives through final Iced teardown. `UiExecutor` borrows its
handle; `RuntimeOwned` enters the originating runtime on final SDK-handle drop,
including SQLite destructors that schedule blocking cleanup. Runtime cleanup
has a five-second bound.

Each session owns one sync service and account-scoped workers. SDK sync handles
offline probing/reconnection; token rejection returns to sign-in. UI updates
stay synchronous. Network, file preparation, decoding, and persistence run
asynchronously or on bounded blocking workers.

Room/thread workers apply every SDK diff before publishing replaceable full-state
snapshots. The selected room and bookmark sources share one subscription set:
SDK subscription updates replace that set. Generation/account checks reject
obsolete results. Navigation and write commands have separate lifetimes; closing
a pane cancels presentation without abandoning an already submitted write.
Shutdown cancels and joins owned workers.

## Persistent accounts

Linux uses Secret Service; macOS uses Security.framework Keychain. Credentials,
SDK store keys, retained keys, and device fingerprint share a versioned record,
selected by canonical profile path or `keyring-profile` override. Keychain uses
service `Interconnect`; Linux uses application attribute `interconnect` and label
`Interconnect session`. Legacy `chatclient-milestone-0` items migrate in place;
ambiguous matches and metadata failures are errors. There is no plaintext fallback.

Keychain calls run on serialized blocking workers. A native worker retains its
guard and duplicate profile lock until completion, even after caller cancellation
or cleanup timeout. Late results cannot publish; in-flight native writes may
still finish. Only item-not-found means missing credentials. Saves update/create
without deleting the previous item first. Local credential removal follows
successful server logout and draft deletion; cleanup is retryable.

The private profile contains encrypted SDK stores, conversation state, and
navigation state. A process-lifetime OS lock is duplicated before opening SQLite and covers late SDK clones and teardown. It releases at process exit,
limiting in-process profile reuse. All programs must honor the lock.

Desktop restore checks local device identity and can show cached data offline;
CLI session commands retain a network `whoami` check. New logins use separate
store directories and retain earlier stores/keys. Tokens cannot replace crypto
state. [Recovery](../validation/recovery.md) imports encryption secrets through the
SDK; account profiles cannot be imported across OS keyrings. See
[profiles](../settings.md#account-profiles).

## Conversations and navigation

Room/thread drafts and event-relative reading anchors persist in
[encrypted local storage](drafts.md); attachments remain in memory. Unavailable
anchors fall back once. Queue admission clears drafts independently of delivery;
local echoes expose pending/sent/failed states and retry reuses the transaction.
Shutdown waits for writes and flushes drafts.

[Virtual rows](timeline.md) bound mounted widgets, with model-backed selection and
a 64-conversation session cache for selection/spoilers/code state. They do not
bound loaded SDK history or full layout of plain prose/expanded code. Newest panes
follow arrivals; older panes retain event anchors. Search/creation capture account
and target identity; late results cannot navigate replacement forms. Temporary
retention protects newly created entries from older navigation snapshots.

The space breadcrumb bar replaces the rail. Peer menus retain the route used
through shared parents, expose expandable branches, and bound traversal of cycles.
Home is a virtual root: directly contained rooms have no joined-space link;
including descendants yields all rooms. DMs obey the same rule. Each space also
has direct/recursive filtering. These toggles, the selected route, and expanded
branches use local navigation persistence; Element account settings are not
written or used to override the toggle. Metadata events refresh membership and
unread indicators without opening timelines. Browsing preserves the conversation,
draft, and reading position. Focusable navigation controls and type-to-jump use
input events and widget operations without periodic work.

Custom sections use `RoomList.CustomSectionData` and
`RoomList.OrderedCustomSections` in `im.vector.web.settings`, with room `m.tag`
keys `element.io.section.<id>`. Edits serialize, read fresh state, and preserve
unrelated metadata; cross-client GET/PUT races remain possible.

## Presence and media

Presence uses per-user SDK endpoints for DM contacts, visible senders, and People.
Refresh bypasses a 60-second cache. Optional polling defaults off and only runs
while focused; batches never overlap and use at most four requests. There is no
second classic-sync stream. See [presence checks](../validation/connected-ui.md#user-presence).

Media workers bound/deduplicate authenticated downloads and decodes, use encrypted
attachment handling and SDK disk caching, and cancel obsolete viewport demand.
Avatars and attachments have separate pixel budgets; the image viewer explicitly
requests originals and suspends background timeline media demand.

Open/Save operations serialize native dialogs/downloads and reject stale results.
Blocking file writes atomically replace destinations after completion. External
opens retain private temporary directories until exit. Attachment preparation
runs off the UI thread; clipboard access occurs only on paste.

There is no global animation tick. Optional polling, typing expiry, dirty-draft
saves, pending operation deadlines, and explicit GIF playback schedule work only
as needed. See [idle resources](../validation/idle-resources.md) for measurement.
