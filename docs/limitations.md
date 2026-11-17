# Current limitations

The client supports one account per process and requires native sliding sync.
macOS builds and notifications exist, but saved accounts still depend on Linux
Secret Service.

- Recovery/key-backup UI, mark-unread controls, ordinary reply
  composition, redaction actions, and notification click navigation
  remain incomplete.
- Incoming receipt labels currently show room receipts only; the SDK does not
  expose thread-specific readers through timeline items. Thread activity badges
  remain local to this process. Receipt privacy preferences are not implemented.
- Message editing supports own text messages and emotes with event IDs. Media
  caption editing, notices, and arbitrary HTML round-tripping remain unsupported.
- Formatted messages support common Matrix HTML. Inline-image authoring, code syntax
  highlighting, full tables, formatted media captions, and visual rich-text editing
  remain incomplete. Excerpts continue to use the plain fallback.
- Custom emoji support includes personal/room pack management and uploads.
  Standalone stickers, bulk pack import/export and animated emoji playback remain
  unfinished. Received emoji keep
  sender dimensions because the pinned SDK strips the emoticon HTML marker.
- Keyboard shortcuts cover common workflows, but full button traversal and focus
  restoration are unfinished.
- Drafts and scroll positions are transient. Complete offline history, timeline
  virtualization, and configurable storage retention are unfinished.
- Native macOS account support, multi-account switching, legacy sync, and calls
  are not yet implemented.

See [UI validation](validation/connected-ui.md) for automated coverage, desktop
checks, and remaining live acceptance.
