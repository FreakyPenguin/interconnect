# Current limitations

One account per process; native sliding sync and Linux Secret Service or macOS
Keychain are required. Linux/Wayland is the main development platform; macOS
builds are experimental.

## Accounts and protocol

- No in-app account switching or account import/transfer. Separate accounts can
  use launch-time profiles via `--profile` or `--profile-path`; see
  [account profiles](settings.md#account-profiles). No legacy sync, password
  changes/reset or account deactivation. Settings → Devices supports device names
  and revoking other sessions; some servers require an unavailable UIAA fallback
  or OAuth-native approval flow and must be managed in another client.
- No voice/video calls, polls UI, location sending/maps, voice recording, or in-app
  audio/video playback. Received calls/polls/locations/stickers use text fallbacks;
  audio/video attachments can be sent and opened externally.
- [Verification](validation/verification.md) supports SAS with your own devices;
  no QR or verification of other users. [Recovery](validation/recovery.md) supports
  key creation/import/replacement, but no destructive identity reset or manual
  room-key export/import.
- No abuse reports. Privacy settings control outgoing typing and public read
  receipts; private read state still reaches your homeserver and other own devices.
  Room notification presets support account default, all
  messages, mentions/keywords, and mute; there is no general push-rule editor,
  timed mute, or per-room sound.
- Matrix links support startup and running-profile activation, with a picker for
  multiple active profiles. Packages advertise `matrix:`; Settings → General can check and request the
  default handler on Linux/macOS. Desktop policy may require confirmation, and
  `matrix.to` HTTPS links are not claimed.
  Delivery queues are bounded/in-memory; native focus is compositor-controlled.
  Packaged macOS AppleEvent delivery still needs native acceptance.

## Rooms and spaces

- No custom power-rule editing, encryption changes, initiating upgrades, bans/
  unbans, or join-request management. Restricted/knock/custom access is read-only.
  Replaced rooms offer successor navigation; histories remain separate.
- Local room addresses can be created/removed and existing aliases published;
  creating aliases on other servers is unsupported. See [settings](settings.md).
- Invitations, member removal, invitation revocation, and Member/Moderator/Admin
  role changes are supported; no custom numeric role entry or self-role changes.
  Membership/permissions are synced snapshots.
- Adding existing space children accepts joined rooms and subspaces, including
  shared parents. Self/duplicate links and cycles through known joined-space
  state are rejected; unknown/unjoined paths and concurrent server changes cannot
  be checked atomically. Unjoined children cannot be linked. Creating and linking
  new subspaces is supported. Disband is partial/retryable, not atomic or a purge; child rooms/history/aliases/
  external references remain.
- Directory search covers the connected homeserver. Space search covers loaded
  results (1,000 per tab). No knocking, hidden-room discovery, remote-homeserver
  directory selection, or email/phone invitations.

## Conversations and storage

- Deletion needs a synced event ID; no state/activity redaction. Queue cancellation
  stops pending uploads/unsent echoes; an in-flight send can finish before the SDK
  removes it, so others may see it first. Editing covers owned text/emotes, not
  captions/notices or arbitrary HTML round-tripping. Reply previews need loaded
  or embedded context; opening an original explicitly loads history, subject to
  available keys and server access.
- Reader labels are room-scoped; thread unread badges are process-local.
- In-room search checks loaded room messages. Account-wide search shows at most
  200 results; older encrypted history requires explicit scans of up to 1,000
  events per batch. Results depend on server search support, available history,
  and encryption keys; there is no complete persistent search index. See
  [search controls](keyboard.md#navigation).
- [Saved messages](bookmarks.md) use experimental MSC4482 event names, which may
  differ in other clients. Initial hydration loads all
  bookmark history; writes require complete decrypted bookmark history and do
  not use an offline send queue.
- No full tables, visual rich-text editor, inline-image authoring, formatted media
  captions, language detection, or composer-source highlighting. Code highlighting
  needs a bundled declared language; blocks over 16 KiB or lines over 2048 bytes
  stay plain. Excerpts are plain text.
- User completion shows six matches from at most 4096 joined members; room links
  use joined rooms/canonical aliases. Room notifications require explicit
  completion and permission; received `@room` styling requires `m.mentions.room`
  metadata. User and room-link styling uses validated Matrix anchors.
- No standalone sticker sending, bulk emoji import/export, or animated custom
  emoji. Inline HTML images and custom emoji remain static; received emoji retain
  sender dimensions after SDK sanitization. Outgoing attachments are limited to
  50 MiB per file. Avatar and custom emoji media is unencrypted even in encrypted
  rooms; see [media settings](settings.md#room-and-space-avatars) and
  [formatting limits](architecture/message-formatting.md#inline-images-and-local-embedded-assets).
- Dialog keyboard traversal/focus confinement and deferred composer focus are
  implemented; complete native keyboard/IME and focus behavior still needs desktop
  validation. See [shortcuts](keyboard.md) and
  [focus validation](validation/connected-ui.md#deferred-composer-focus).
- Text drafts and event-anchored reading positions persist encrypted in the local
  profile and do not sync across devices. At most 4,096 reading positions persist;
  see [local storage limits](architecture/drafts.md). Attachments and unfinished
  edits do not survive restart. Restoring an old position requires available
  history/keys; unavailable anchors fall back to the newest timeline. No complete
  offline history or configurable storage retention. Long timelines virtualize row widgets;
  scrollbar geometry uses estimated offscreen heights, and individual very long
  messages still lay out as a whole. Loaded SDK data remains in memory.

See [ideas](todo.md) and [validation boundaries](validation/connected-ui.md#validation-scope).
