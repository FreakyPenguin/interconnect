# Current limitations

Interconnect supports one account per process and requires native sliding sync.
Saved accounts require Linux Secret Service or macOS Keychain.

- Recovery/key-backup UI, ordinary reply composition, notification click navigation,
  account switching, cross-platform account import, legacy sync, and calls are missing.
- Deletion requires a synced message-like event with an event ID. Cancelling unsent
  local echoes and redacting room-state/activity events are unsupported.
- Editing covers your text messages and emotes; media captions, notices, and
  arbitrary HTML round-tripping are unsupported.
- Receipt labels show room readers, not thread-specific readers. Thread unread
  badges are local to the process; receipt privacy preferences are missing.
- Formatting omits full tables, syntax highlighting, visual rich-text editing,
  inline-image authoring, and formatted media captions. Excerpts use plain text.
- Received Matrix user links have mention styling; completion, mention authoring,
  room-wide mention styling, and in-app member-link navigation are unsupported.
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
