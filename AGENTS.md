# Project guidance

Rust desktop Matrix client using Iced and matrix-sdk. Aim for a sleek, minimal
UI that will also fit macOS, with spaces and room-list sections for organization.

- Keep idle CPU use and wakeups low. Prefer event-driven updates; avoid polling,
  unnecessary redraws, and background animation timers.
- Keep SDK work asynchronous and preserve cancellation, stale-update rejection,
  and runtime-safe cleanup. Never block the UI thread.
- Preserve per-conversation drafts and scroll behavior. Changing spaces filters
  the room list without switching the open conversation.
- Use `cargo run --locked --bin interconnect -- --demo` for UI experiments. Keep
  demo mode independent of saved accounts and network access. Do not send test
  messages or restart the user's connected client without authorization.
- Never commit credentials, account stores, private message content, or local
  validation artifacts.
- Keep changes focused and commits reviewable. Update relevant validation docs
  when behavior changes, and distinguish tested behavior from assumptions.
- Keep vendored Iced changes small and documented in `vendor/iced_widget/PATCH.md`;
  avoid unrelated formatting or dependency upgrades.

## Validation

Run checks appropriate to the change: `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`, and targeted
tests (UI: `cargo test --locked --bin interconnect`). Use
`cargo test --locked --workspace` for shared/backend changes and
`cargo build --locked --bin interconnect` before desktop validation.

See `docs/development.md`, `docs/validation/connected-ui.md`, and
`docs/validation/idle-resources.md` for setup and validation details.
