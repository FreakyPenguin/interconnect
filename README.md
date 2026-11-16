# Interconnect

Interconnect is a native desktop [Matrix](https://matrix.org/) client written in
Rust with [Iced](https://iced.rs/) and the
[Matrix Rust SDK](https://github.com/matrix-org/matrix-rust-sdk). It aims to be
lightweight and responsive,
with a clean interface, strong conversation organization, and power-user features.
Spaces, room sections, threads, and keyboard shortcuts help keep busy accounts
manageable. Event-driven updates keep background work and idle power use low.

This project is fully AI-coded for the developer's own use, and shared in case
it's useful to anyone else. It's a working prototype, with Linux/Wayland as the
primary platform and best-effort macOS builds.

![Interconnect demo showing spaces, room sections, a conversation, and a thread](docs/images/demo.png)

*Demo mode with local sample conversations.*

## Current features

- Password and browser SSO login, saved sessions, encrypted messaging, and device
  verification.
- Spaces, room sections, direct messages, and quick room switching.
- Room and thread timelines with history loading, separate drafts and reading
  positions, send status, and retry.
- People search, room creation, invitations, room topics, and pinned messages.
- Reactions, emoji pickers and completion, image viewing, and click-to-play GIFs.
- Attachments through file picking, clipboard paste, and drag-and-drop.
- Desktop notifications, keyboard shortcuts, and configurable settings.

## Try it

With Rust and the [native build dependencies](docs/development.md) installed:

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo mode uses local sample data without a saved account or Matrix connection.
Run without `--demo` to sign in; the homeserver needs native sliding sync.

See the [documentation](docs/README.md) for setup, packaging, configuration, and
[current limitations](docs/limitations.md).

## License

[MIT](LICENSE). Third-party code and assets retain their own licenses; see
[third-party notices](THIRD_PARTY_NOTICES.md).
