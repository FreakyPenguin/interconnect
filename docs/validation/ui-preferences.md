# Preferences and account dialogs

These are demo/manual acceptance procedures, not a record of connected or native
macOS validation. Build with the locked desktop command before launching
`cargo run --locked --bin interconnect -- --demo`.

## Preferences

- At 1400×900 and 640×480, open Settings. General exposes appearance, sending,
  and room-list preferences; Notifications, Media & emoji, and Advanced are
  directly selectable. Only the body scrolls; the categories and actions remain
  available. Each category must fit horizontally in Light and Dark.
- Edit values in several categories. Switching categories must retain every edit
  and numeric input, including invalid input. An invalid value keeps Save disabled
  and offers a link to the category needing attention.
- Close using X or Escape with an edited form: keep editing or explicitly discard.
  Cancel explicitly discards the form. Clean forms close immediately. Preserve the
  open room/thread, their drafts, and an older reading position throughout.
- Advanced → Reload file warns before discarding edited values. Reload fills the
  form; Save applies it. Cancel after reload leaves running preferences unchanged.
  Startup options still apply only on the next launch.
- Disable desktop notifications: message preview becomes unavailable while its
  saved choice is retained. Sample-notification failures are visibly identified
  beside the sample action. Do not send sample notifications during unattended
  review unless desktop-service interaction is intended.
- Focus the GIF-duration input. Its steady caret must not schedule periodic
  redraws. Static controls must remain event-driven.

Headless regressions cover category draft retention, dirty-close/reload choices,
reload staging, and the 640×480 notification-body/action geometry. Desktop
appearance, native notification permissions, and CPU/wakeups need manual checks.
