# Client architecture

## Dependencies and rendering

The client uses Iced 0.14 and matrix-sdk/matrix-sdk-ui 0.19.1, pinned in
[Cargo.toml](../../Cargo.toml) and Cargo.lock. Iced enables Wayland, Tokio, wgpu,
tiny-skia, advanced input handling, and image handles. The SDK owns sync,
SQLite persistence, encryption, and automatic room-key requests/forwarding under
its trust policy. Native sliding sync is required.

wgpu is the default renderer, with tiny-skia fallback on initialization failure.
Device loss does not switch an already running renderer. Graphics settings are
read before Iced/Tokio initialization; a Unix re-exec applies environment values
without mutating the environment of a threaded process. See
[renderer selection](../development.md#renderer-selection).

The local Iced widget patch disables recurring editor caret blinking; its changes
are documented in [PATCH.md](../../vendor/iced_widget/PATCH.md).
Wayland file drops use a [pinned Winit fork](winit-file-drops.md).

## Runtime and session ownership

One Tokio runtime lives through final Iced teardown. `UiExecutor` borrows its
handle; UI-facing SDK capabilities use `RuntimeOwned` to enter the originating
runtime on final drop. This matters for SQLite destructors that schedule
blocking cleanup, including handles released outside Iced's executor context.
Runtime cleanup has a five-second bound.

Each session owns one SDK sync service and its account-scoped workers. The SDK
owns offline probing and reconnection. Token rejection stops the service and
returns to sign-in. UI updates stay synchronous; network, file preparation,
decoding, and persistence run asynchronously or on bounded blocking workers.

Room/thread presentation workers publish replaceable full-state snapshots.
SDK diffs are applied in order before snapshots are coalesced. Navigation and
command results have separate lifetimes; generation/account checks reject obsolete
completions. Closing a thread releases its presentation without stopping the
room. Cancellation interrupts pending work and shutdown joins owned workers.

## Persistent accounts

Credentials and SDK store encryption keys live in Secret Service. The private
profile contains the SDK stores, local device fingerprint, and navigation state.
Desktop restore checks local device identity and can present cached room data
without a network identity request. CLI session commands retain a `whoami` check.
Login, verification trust, and historical key availability are separate states.
See [account profiles](../settings.md#account-profiles) for paths and migration.

Persistent SDK clients use `CrossProcessLockConfig::SingleProcess`. Before
opening SQLite, the builder duplicates the OS profile lock into a process-lifetime
registry. It covers late SDK clones, construction failures, UI teardown, and
cleanup timeouts. The kernel releases it at process exit. A used profile cannot
be reopened by another session until then. In-process account switching is limited
by this lifetime. Store exclusion relies on all programs honoring the profile lock.
SQLite durability settings are unchanged.

New sign-in attempts use separate store directories. Earlier stores and their
keys are retained; there is no recovery/import UI. A session token alone cannot
replace the crypto store. Deleting a profile also removes its local device and key
state, rather than just clearing cached content.

## Conversation state and writes

Drafts are keyed by room and optional thread root. They retain text and staged
attachments across navigation, and clear on successful SDK queue admission.
Admission is distinct from delivery: SDK local echoes show pending/sent/failed
states. Retry unwedge uses the original send handle and transaction ID.
Shutdown waits for outstanding admission/retry operations.

Room and thread scrolling are independent. Open panes follow new messages only
while already at the bottom; otherwise a visible message anchor holds position
across arrivals and pagination. Closed-pane positions retain pixel distances
from the bottom in memory. Neither drafts nor scroll positions survive exit.

People search is abortable and generation-scoped. Explicit room/direct-chat
creation continues against its captured account after dismissal and participates
in shutdown. Late results cannot navigate another form/account. Event-driven
navigation refresh and temporary retention of created entries prevent older
snapshots from erasing a newly opened conversation.

## Media and resource use

Account-scoped media workers use SDK authenticated requests, encrypted attachment
handling, and disk caching. Downloads and decodes are bounded and deduplicated;
viewport demand and generation checks cancel obsolete work. Avatar and attachment
decoded-pixel caches have separate budgets. The image viewer requests originals
explicitly and suspends background timeline media demand while open.

Attachment preparation reads bounded bytes and builds optional thumbnails away
from the UI thread. Clipboard reads occur only on paste; file drops use the
native event loop. Demo mode keeps its data in memory without account access.

Settled UI has no global polling or animation tick. Deadlines exist for pending
operations, typing expiry, and explicitly playing GIFs; stopped, hidden, or closed
content releases its visual timers. Resource measurements and their limits live
in [idle-resource validation](../validation/idle-resources.md).
