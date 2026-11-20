# Current limitations

One account per process; native sliding sync and Linux Secret Service or macOS
Keychain are required. Linux/Wayland is the main development platform; macOS
builds are experimental.

## Accounts and protocol

- No account switching/import, legacy sync, password changes/reset, account
  deactivation, or renaming/revoking other devices.
- No voice/video calls, polls UI, location sending/maps, voice recording, or in-app
  audio/video playback. Received calls/polls/locations/stickers use text fallbacks;
  audio/video attachments can be sent and opened externally.
- [Verification](validation/verification.md) supports SAS with your own devices;
  no QR or verification of other users. [Recovery](validation/recovery.md) supports
  key creation/import/replacement, but no destructive identity reset or manual
  room-key export/import.
- No ignore/block-user, abuse reports, receipt privacy controls, general push-rule
  editor, timed mutes, or per-room sounds.

## Rooms and spaces

- No custom power-rule editing, encryption changes, initiating upgrades, bans/
  unbans, or join-request management. Restricted/knock/custom access is read-only.
  Replaced rooms offer successor navigation; histories remain separate.
- Local room addresses can be created/removed and existing aliases published;
  creating aliases on other servers is unsupported. See [settings](settings.md).
- Member roles are Member/Moderator/Admin; no custom numeric role entry or
  self-role changes. Membership/permissions are synced snapshots.
- Space links accept joined rooms, not existing subspaces or unjoined children.
  Disband is partial/retryable, not atomic or a purge; child rooms/history/aliases/
  external references remain.
- Directory search covers the connected homeserver. Space search covers loaded
  results (1,000 per tab). No knocking, hidden-room discovery, remote-homeserver
  directory selection, or email/phone invitations.

## Conversations and storage

- Deletion needs a synced event ID; no unsent-echo cancellation or state/activity
  redaction. Editing covers owned text/emotes, not captions/notices or arbitrary
  HTML round-tripping. Reply previews need loaded/embedded context; opening an
  original explicitly loads history, subject to available keys and server access.
- Reader labels are room-scoped; thread unread badges are process-local.
- No full tables, visual rich-text editor, inline-image authoring, formatted media
  captions, language detection, or composer-source highlighting. Code highlighting
  needs a bundled declared language; blocks over 16 KiB or lines over 2048 bytes
  stay plain. Excerpts are plain text.
- User completion shows six matches from at most 4096 joined members; room links
  use joined rooms/canonical aliases. Room notifications require explicit
  completion and permission; received styling needs mention metadata.
- No standalone sticker sending, bulk emoji import/export, or animated custom
  emoji. Received emoji retain sender dimensions after SDK sanitization.
- Keyboard traversal/focus restoration remains incomplete; see [shortcuts](keyboard.md).
- Text drafts and event-anchored reading positions persist encrypted. Attachments
  and unfinished edits do not survive restart. Restoring an old position requires
  available history/keys; unavailable anchors fall back to the newest timeline. No complete offline
  history, timeline virtualization, or configurable storage retention.

See [ideas](todo.md) and [validation boundaries](validation/connected-ui.md#validation-scope).
