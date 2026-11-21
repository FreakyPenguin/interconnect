# Encrypted local conversation state

`src/drafts.rs` stores room/thread text drafts, reading anchors, and recent
notification history in `drafts.json` inside the locked account profile. Drafts include the literal source, plain/Markdown composition
mode, explicit custom emoji bindings, and selected user mention identities/ranges. Attachment
data and paths, and unfinished message edits are excluded. Per-room/thread reading
anchors (synced event IDs and signed viewport offsets) share the encrypted snapshot
and serialized writer; older snapshots default to no anchors. While editing a sent
message, the saved value remains the original unsent conversation draft.

Recent notifications include their destination, timestamp, title/preview, stable
identity, and local read state. The snapshot retains at most 256 records. Older
snapshots default to an empty history. Notification previews and destination IDs
are encrypted with the drafts; no separate plaintext inbox file is created.

The file contains a versioned encrypted envelope using the SDK's StoreCipher.
Its exported cipher is protected by the current SDK store key from the native
credential store. The envelope names that key's store directory; retained store
keys permit the same account to recover drafts after reauthentication. Account
identity is checked inside the encrypted payload. Another account receives an
empty snapshot, including no notification history or read state. No plaintext
fallback exists.

Encryption, key derivation and file I/O run on blocking workers. Saves use a
private temporary file, file sync, atomic replacement and directory sync.
Opening a missing draft store creates no file. Invalid versions, keys,
permissions and malformed data preserve the original file and report an error.
Limits bound file size, total text, draft count, emoji selections, and user
mention selections (256 each), reading positions (4096), and saved notifications
(256). Notification identities are unique and limited to 2048 bytes, titles to
4096 bytes, and previews to 8192 bytes. Conversation/invitation destinations must
contain valid Matrix room IDs; optional thread roots must be valid event IDs.
Verification flow/device identities must be nonempty, bounded strings without
control characters. Named mention ranges must be valid UTF-8 boundaries and
non-overlapping; older files default to no named selections. Snapshot diagnostics
show counts, never draft text or notification previews.

Successful sign-out removes this file, including notification history and read
state, before deleting its encryption keys.
A failed remote sign-out retains drafts; local deletion failures can be retried
without revoking the server session again. Encrypted SDK stores are retained.

The UI owns a serialized writer and a cancellable 400 ms debounce that exists
only after draft, reading-anchor, or notification-history changes. It captures
source/composition changes, changed viewport anchors, notification arrivals and
local read-state changes, excluding caret movement and unchanged redraw/hover
events. A normal quit waits for
pending message writes and then flushes the newest draft snapshot. Save errors
keep the window open with retry, keep-open and explicit discard controls. Abrupt
process termination may lose edits within the debounce interval. Demo mode uses
neither draft storage nor a draft timer.
