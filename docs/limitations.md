# Current limitations

The client supports one account per process and requires native sliding sync.
macOS builds and notifications exist, but saved accounts still depend on Linux
Secret Service.

- Recovery/key-backup UI, automatic read marking and receipts, ordinary reply
  composition, editing/redaction actions, and notification click navigation
  remain incomplete.
- Formatted messages support common Matrix HTML. Inline images, code syntax
  highlighting, full tables, formatted media captions, and visual rich-text editing
  remain incomplete. Excerpts continue to use the plain fallback.
- Keyboard shortcuts cover common workflows, but full button traversal and focus
  restoration are unfinished.
- Drafts and scroll positions are transient. Complete offline history, timeline
  virtualization, and configurable storage retention are unfinished.
- Native macOS account support, multi-account switching, legacy sync, and calls
  are not yet implemented.

See [UI validation](validation/connected-ui.md) for automated coverage, desktop
checks, and remaining live acceptance.
