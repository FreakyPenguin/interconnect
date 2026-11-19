# Current limitations

One account per process; native sliding sync and Linux Secret Service or macOS
Keychain are required.

- No recovery/key-backup UI, account switching/import, legacy sync, or calls.
- No room alias/power-rule editing, encryption changes, bans, upgrades, timed mutes,
  or per-room sounds. Restricted/knock/custom access is read-only.
- Space links support joined rooms, not existing subspaces/unjoined children.
  Directory search covers the connected homeserver only; space search covers
  loaded results (1,000 per tab). No knocking or hidden-room discovery.
  Disbanding is partial/retryable, not atomic or a data purge; children/history/
  aliases/external references remain. See [management](settings.md).
- Member roles are Member/Moderator/Admin; no custom numeric role entry, self-role
  changes, or bans/unbans. Membership/permissions are synced snapshots.
- Deletion requires a synced message event ID; no cancellation of unsent echoes
  or redaction of state/activity events. Editing covers owned text/emotes, not
  media captions, notices, or arbitrary HTML round-tripping.
- Reply previews need loaded/embedded context; no fetching/jumping to quoted originals.
- Reader labels are room-scoped; thread unread badges are process-local. No receipt
  privacy preferences.
- No full tables, visual rich-text editor, inline-image authoring, or formatted media
  captions. Excerpts are plain. Code highlighting needs a declared bundled language;
  unknown languages, blocks over 16 KiB, and lines over 2048 bytes stay plain.
  No language detection or composer-source highlighting.
- User completion shows six matches from at most 4096 joined members; room links
  search the joined snapshot/canonical aliases only. Room notifications require
  explicit completion and permission; received styling needs mention metadata.
- No standalone stickers, bulk emoji import/export, or animation. Received emoji
  retain sender dimensions because SDK sanitization strips the emoticon marker.
- [Keyboard traversal/focus restoration](keyboard.md) remains incomplete.
- Encrypted text drafts persist; attachments, unfinished edits, and scroll positions
  do not survive restart. No complete offline history, timeline virtualization,
  or configurable storage retention.

See [ideas](todo.md) and [validation boundaries](validation/connected-ui.md#validation-scope).
