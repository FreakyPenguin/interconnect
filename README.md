# Interconnect

<a href="docs/logo.svg">
  <img src="docs/logo.svg" alt="Interconnect processor speech-bubble logo" width="160" height="160">
</a>

Interconnect is a native desktop [Matrix](https://matrix.org/) client written in
Rust with [Iced](https://iced.rs/) and the
[Matrix Rust SDK](https://github.com/matrix-org/matrix-rust-sdk). Home, spaces,
custom room sections, and side-by-side threads keep conversations organized in a
minimal interface. Encrypted local text drafts survive restarts; message search,
saved messages, and keyboard shortcuts help you find your way around busy accounts.
Event-driven updates aim to keep idle CPU use and wakeups low.

This project is fully AI-coded for the developer's own use, and shared in case
it's useful to anyone else. It's a working prototype, with Linux/Wayland as the
primary development platform and experimental Apple Silicon macOS builds.

![Interconnect showing the Home house icon above All, an invitation shortcut, unread rooms, custom sections, and a conversation with a side-by-side thread](docs/images/demo.png)

*Home and space navigation, invitations, and a side-by-side thread. All names and messages are local demo data.*

## Current features

- Password and browser SSO sign-in, saved sessions, end-to-end encrypted messaging,
  device verification, and in-app sign-out.
- Home for rooms outside spaces and all DMs, following Element's account preference,
  plus spaces, custom room sections, favorites, unread indicators and ordering,
  and quick room switching. Space filters preserve the open conversation.
- Resizable room and thread panes with history loading, send status, and retry.
- Encrypted local room and thread text drafts, including composition mode and custom
  emoji selections, restored after restart. Reading positions remain per-session.
- In-room search and account-wide message search, with explicit scans of older
  encrypted history using available keys.
- Saved messages with room and thread previews and navigation to the original.
- Edit your own text messages and emotes; delete messages with confirmation and
  room permission checks.
- Formatted messages with styled text, lists, quotes, code, links, spoilers, and
  inline images with explicit dimensions.
- Matrix-compatible inline and display LaTeX math, rendered locally with RaTeX.
- Markdown composition, on-demand previews, and a per-conversation plain-text mode.
- Start direct chats, create rooms, accept or decline invitations, join by room ID,
  alias or Matrix link, and leave rooms with confirmation.
- People search, member lists, editable room topics, and pinned messages.
- Reactions, Unicode and Matrix custom emoji pickers/completion, and personal and
  room emoji pack management with image uploads.
- Image viewing and click-to-play GIFs.
- Save original attachments or open them in the default application, including
  encrypted images, files, audio, and video in rooms and threads.
- Attachments through file picking, clipboard paste, and drag-and-drop.
- On-demand presence snapshots, with optional periodic refresh disabled by default.
- Desktop notifications, keyboard shortcuts, and configurable navigation,
  composition, media, and graphics settings.

![Account-wide demo search with the updated Home and space rail, an invitation shortcut, and highlighted results from multiple rooms and threads](docs/images/search.png)

*Account-wide search finds room messages and thread replies across spaces, with highlighted matches and navigation to the original.*

## Try it

With Rust and the [native build dependencies](docs/development.md) installed:

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo mode uses local sample data without a saved account or Matrix connection.
Run without `--demo` to sign in; the homeserver needs native sliding sync.

Experimental downloads are available from successful GitHub Actions runs:
[Debian amd64 packages](docs/development.md#debian-package-automation) and
[Apple Silicon macOS builds](docs/development.md#download-a-bleeding-edge-macos-build).
These are bleeding-edge builds; the macOS app is signed ad hoc and not notarized.

See the [documentation](docs/README.md) for setup, packaging, configuration,
[keyboard shortcuts](docs/keyboard.md), and [current limitations](docs/limitations.md).

## License

[MIT](LICENSE). Third-party code and assets retain their own licenses; see
[third-party notices](THIRD_PARTY_NOTICES.md).
