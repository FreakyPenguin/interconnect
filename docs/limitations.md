# Current limitations

One account per process; native sliding sync and Linux Secret Service or macOS
Keychain are required.

- No account switching/import, legacy sync, or calls.
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

## Missing protocol features

The main Matrix feature gaps below describe what Interconnect exposes, even where
the SDK can handle the underlying events. This is not an exhaustive protocol
compliance checklist.

- **Calls:** no voice/video calls or group-call participation. Received call events
  appear as activity text only.
- **Polls:** no creation, voting, ending polls, or results UI. Received polls show
  fallback text or the question.
- **Locations:** no location sending, maps, or live-location tracking. Received
  locations have text-only presentation.
- **Media:** no voice-message recording or in-app audio/video playback. Audio/video
  files can still be sent as attachments and downloaded/opened externally.
  Standalone sticker events have text-only presentation; no sticker sending.
- **Encryption and trust:** no destructive encryption-identity reset or manual
  room-key import/export UI. Recovery supports key creation, import, and replacement;
  see [key backup and recovery](validation/recovery.md).
  Verification supports SAS with your own devices only;
  no QR verification or verification of other users. See
  [device verification](validation/verification.md).
- **Account and sessions:** no password changes/reset, account
  deactivation, profile name/avatar editing, or renaming/revoking other devices.
  Login supports password and browser SSO; legacy sync is unsupported.
- **Room administration:** no alias management, custom power-rule editing,
  encryption changes, room upgrades, or bans/unbans. Restricted/knock access rules
  are read-only; no knocking or join-request management UI.
- **Discovery and invitations:** no remote-homeserver directory selection or
  email/phone invitations. Space linking does not cover existing subspaces or
  unjoined children. See [room and space management](settings.md).
- **Moderation and privacy:** no ignore/block-user or abuse-reporting UI, receipt
  privacy controls, or general push-rule editor. Per-room notification presets
  are supported, but timed mutes are missing.

See [ideas](todo.md) and [validation boundaries](validation/connected-ui.md#validation-scope).
