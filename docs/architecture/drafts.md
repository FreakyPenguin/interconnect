# Local text draft storage

`src/drafts.rs` stores room/thread text drafts in `drafts.json` inside the locked
account profile. Drafts include the literal source, plain/Markdown composition
mode, explicit custom emoji bindings, and selected user mention identities/ranges. Attachment
data and paths, scroll positions, and unfinished message edits are excluded. While editing a sent
message, the saved value remains the original unsent conversation draft.

The file contains a versioned encrypted envelope using the SDK's StoreCipher.
Its exported cipher is protected by the current SDK store key from the native
credential store. The envelope names that key's store directory; retained store
keys permit the same account to recover drafts after reauthentication. Account
identity is checked inside the encrypted payload. Another account receives an
empty draft set. No plaintext fallback exists.

Encryption, key derivation and file I/O run on blocking workers. Saves use a
private temporary file, file sync, atomic replacement and directory sync.
Opening a missing draft store creates no file. Invalid versions, keys,
permissions and malformed data preserve the original file and report an error.
Limits bound file size, total text, draft count, emoji selections, and user
mention selections (256 each). Named ranges must be valid UTF-8 boundaries and
non-overlapping; older files default to no named selections. Diagnostics show draft counts, never draft text.

Successful sign-out removes the draft file before deleting its encryption keys.
A failed remote sign-out retains drafts; local deletion failures can be retried
without revoking the server session again. Encrypted SDK stores are retained.

The UI owns a serialized writer and a cancellable 400 ms debounce that exists
only after draft changes. It captures changes to the source or composition mode,
not caret movement, scroll, redraw or hover events. A normal quit waits for
pending message writes and then flushes the newest draft snapshot. Save errors
keep the window open with retry, keep-open and explicit discard controls. Abrupt
process termination may lose edits within the debounce interval. Demo mode uses
neither draft storage nor a draft timer.
