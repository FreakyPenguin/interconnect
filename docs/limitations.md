# Current limitations

Interconnect supports one account per process and requires native sliding sync.
Saved accounts require Linux Secret Service or macOS Keychain.

- Recovery/key-backup UI, account switching, cross-platform account import, legacy sync, and calls are missing.
- Room settings cover name/topic, account push-rule preferences, invitation/public
  access, and history visibility. Restricted/knock/custom access remains read-only;
  avatars, aliases, permission editing, encryption changes, bans, upgrades, timed
  mutes and per-room notification sounds are not implemented.
- Space management supports adding joined rooms to writable spaces and removing
  direct room links. Subspaces can be created inside writable spaces. Creating
  root spaces, editing spaces, and managing
  unjoined children are not implemented. Link changes leave room membership
  and child-side parent metadata intact.
- Deletion requires a synced message-like event with an event ID. Cancelling unsent
  local echoes and redacting room-state/activity events are unsupported.
- Reply previews use loaded timeline/embedded context; originals outside it show an
  unavailable label. Fetching or jumping to a quoted original is not implemented.
- Editing covers your text messages and emotes; media captions, notices, and
  arbitrary HTML round-tripping are unsupported.
- Receipt labels show room readers, not thread-specific readers. Thread unread
  badges are local to the process; receipt privacy preferences are missing.
- Formatting omits full tables, visual rich-text editing,
  inline-image authoring, and formatted media captions. Excerpts use plain text.
- Code highlighting uses declared languages from the bundled syntax collection;
  missing/unknown languages, blocks over 16 KiB, and lines over 2048 bytes stay
  plain monospace. Language detection and composer-source highlighting are absent.
- User mention completion suggests up to six joined room members from a snapshot
  of at most 4096 users. Full display names insert as editable text with per-occurrence
  identities and visual highlighting. Room link completion via `#` searches only
  joined rooms in the current snapshot, with canonical aliases (not alternate
  aliases or a server directory). `@room` is an explicit completion choice,
  offered only when cached room permissions allow notifications; sending rechecks
  permission. Received room-wide styling requires `m.mentions.room` metadata.
- Custom emoji packs support management and uploads, but not standalone stickers,
  bulk import/export, or animation. Received emoji retain sender dimensions because
  the pinned SDK strips the emoticon marker.
- [Keyboard shortcuts](keyboard.md) cover common workflows; complete button
  traversal and focus restoration remain unfinished.
- Text drafts persist encrypted. Attachments, unfinished message edits, and scroll
  positions do not survive restart. Complete offline history, timeline virtualization,
  and configurable storage retention remain unfinished.

See [UI validation](validation/connected-ui.md) for repeatable desktop checks and
validation boundaries.
