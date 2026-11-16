# Features and scope

The prototype supports one account per process, with Linux/Wayland as the primary
platform. It requires a homeserver with native sliding sync. macOS build automation
and native notification support exist; saved accounts still depend on Linux
Secret Service. See [development](development.md) for running and packaging.

## Available functionality

- Password and browser SSO login, encrypted session storage, same-device restoration,
  cached startup, and SDK-managed reconnection.
- Encrypted and unencrypted room timelines, automatic history loading, separate
  thread panes, text sending, local send states, and explicit retry.
- Joined and nested spaces, shared room membership, automatic room-list sections,
  unread indicators, direct messages, filtering, and a quick room switcher.
- Profile-local navigation restoration; independent in-memory room/thread drafts
  and scroll positions. Changing spaces filters navigation without switching the
  open conversation.
- People search, direct-chat creation, room creation, invitations and membership
  actions, room topics, and pinned messages.
- Unicode emoji rendering, reactions, emoji pickers, and inline shortcode completion.
- Image viewing, click-to-play GIFs, and attachments from a file picker, clipboard,
  or file drop.
- Own-device SAS verification and configurable desktop notifications.
- TOML-backed settings and GPU rendering with software startup fallback.
- An account-independent demo for UI development.

These describe implemented functionality. [Validation](validation/connected-ui.md)
distinguishes automated coverage, desktop checks, and remaining live acceptance.

## Current limits

- Recovery/key-backup UI, automatic read marking and receipts, ordinary reply
  composition, editing/redaction actions, and notification click navigation
  remain incomplete.
- Keyboard shortcuts cover common workflows, but full button traversal and focus
  restoration are unfinished.
- Drafts and scroll positions are transient. Complete offline history, timeline
  virtualization, and configurable storage retention are unfinished.
- Native macOS account support, multi-account switching, legacy sync, and calls
  are not yet implemented.

SDK work runs asynchronously, cleanup retains the originating runtime, and the
settled UI is event-driven. [Idle resources](validation/idle-resources.md) documents
resource measurements and coverage across visible, hidden, and connected states.
