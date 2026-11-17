# Client architecture

## Dependencies and rendering

The client uses Iced 0.14 and matrix-sdk/matrix-sdk-ui 0.19.1, pinned in
[Cargo.toml](../../Cargo.toml) and Cargo.lock. Iced enables Wayland, Tokio, wgpu,
tiny-skia, advanced input handling, image handles, and canvas geometry for the
shared outline icons. Icon geometry is cached per widget and follows its inherited
color, including hover and disabled states; it adds no timers or subscriptions.
The SDK owns sync,
SQLite persistence, encryption, and automatic room-key requests/forwarding under
its trust policy. Native sliding sync is required.

On Linux, credential storage uses Secret Service 5.2 with Tokio and its Rust
cryptography backend. macOS uses Security.framework generic-password items through
`security-framework` 3.7, with no Secret Service dependency. Secret Service
AES/CBC/HKDF dependencies share the versions already used by the Matrix SDK;
system OpenSSL and libdbus development packages are unnecessary for the selected
dependency graph. Desktop D-Bus services remain Linux runtime requirements.

wgpu is the default renderer, with tiny-skia fallback on initialization failure.
Device loss does not switch an already running renderer. Graphics settings are
read before Iced/Tokio initialization; a Unix re-exec applies environment values
without mutating the environment of a threaded process. See
[renderer selection](../development.md#renderer-selection).

A small [Cosmic Text extension](../../vendor/cosmic-text/PATCH.md) supplies
embedded-object metrics for inline images, including application-generated
assets. The pinned dependency version is unchanged.

The local Iced widget patch disables recurring editor caret blinking; its changes
are documented in [PATCH.md](../../vendor/iced_widget/PATCH.md).
Wayland file drops use a [pinned Winit fork](winit-file-drops.md).

Presence uses the stock SDK's per-user status endpoint on demand. Opening a DM,
viewing room/thread messages, or opening People requests their peers, unique
visible senders, or visible members. Existing viewport reports drive message
lookups; self, activity rows, collapsed thread summaries, hidden panes, and stale
generations are excluded. The pinned thread original is included. Explicit Refresh
bypasses the 60-second memory cache. Optional periodic refresh defaults off, with a 60-second
interval when enabled. While focused, it refreshes every unambiguous DM contact
across the navigator, plus visible message senders and the People page. Room and
space navigation does not interrupt navigator-only demand. Four concurrent requests bound network
work; batches never overlap. There is no second sync stream or UI animation tick.
See [presence validation](../validation/connected-ui.md#user-presence) for limits
and the one-time migration from older shared sync cursors. All Matrix SDK crates
remain pinned to the unmodified 0.19.1 release.

Formatted room and thread messages use a bounded native document renderer; see
[message formatting](message-formatting.md) for selection and extension points.

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
The selected room and bookmark source rooms are combined into a single SDK room
subscription set. The SDK replaces that set on each update; setting bookmark
subscriptions separately would unsubscribe the open room and stop live pin state.
SDK diffs are applied in order before snapshots are coalesced. Navigation and
command results have separate lifetimes; generation/account checks reject obsolete
completions. Closing a thread releases its presentation without stopping the
room. Cancellation interrupts pending work and shutdown joins owned workers.

## Persistent accounts

Credentials, SDK store encryption keys, retained keys, and the device fingerprint
live together in the native credential store: Secret Service on Linux, Keychain
on macOS. Both use the same versioned JSON record. macOS uses the `Interconnect`
service name; Linux uses the `interconnect` application attribute with the display
label `Interconnect session`. Linux searches both this namespace and the legacy
`chatclient-milestone-0` namespace, rejecting multiple matching items. A validated
legacy record is renamed in place on read; saves migrate and update an existing
item in its original collection. Migration preserves the secret, profile identity,
and additional attributes. Metadata errors stop the operation without deleting
the record; a later attempt can retry. Logout removes entries under both names.
The canonical profile path (or `keyring-profile` identity override) selects the
record. On macOS it is the generic-password account attribute, and the namespace
is the service attribute.
The private profile contains the encrypted SDK stores, encrypted text drafts,
and navigation state. See [draft storage](drafts.md) for the draft envelope,
account isolation, serialized writer and close behavior.
There is no plaintext fallback.

Keychain calls run on blocking workers, serialized per session. A worker retains
both its serialization guard and a duplicate profile lock until the native call
returns, including after cancellation or a runtime cleanup timeout. A cancelled
caller cannot publish a late result; an in-flight native write may still finish.
A retry waits for it before reading or writing. Only `errSecItemNotFound` means
missing credentials; denied access, user cancellation, and locked/unavailable
keychains remain errors. Saves create or update without deleting the old item
first; local deletion is idempotent and follows successful server logout.
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
from the bottom in memory. Text drafts, composition mode and custom emoji selections survive exit in the
[encrypted draft store](drafts.md). Scroll positions and staged attachments
remain in memory.

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

Received-media actions use `download::Attachment` metadata for original sources
and filenames. The UI serializes explicit Open/Save operations, cancels obsolete
requests, rejects stale native-dialog/download results, and holds runtime-owned
timelines through SDK requests. File writes run on blocking workers; a temporary
sibling replaces a save destination only after writing completes. External opens
retain private temporary directories until exit so lazy readers keep access.

Attachment preparation reads bounded bytes and builds optional thumbnails away
from the UI thread. Clipboard reads occur only on paste; file drops use the
native event loop. Demo mode keeps its data in memory without account access.

Settled UI has no global animation tick. Deadlines exist for optional presence
polling, pending operations, typing expiry, and explicitly playing GIFs; stopped,
hidden, or closed content releases its visual timers. Resource measurements and their limits live
in [idle-resource validation](../validation/idle-resources.md).

## Home scope

Home is a synthetic navigation scope placed before All rooms. Its default
filter complements the child-room IDs of joined spaces and always includes DMs,
following Element Web's `showInHomeSpace`. A read-only import of Element's
`Spaces.allRoomsInHome` account setting can expand it to all rooms. Navigation
snapshots carry that flag alongside rooms, spaces and section definitions, so
existing account-data notifications refresh it without extra SDK workers. The
room list and unread indicator share the same scope filter. Home affects only
the navigator and is persisted through the ordinary navigation preferences.
