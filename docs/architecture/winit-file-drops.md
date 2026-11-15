# Winit file-drop dependency

Cargo consumes the [FreakyPenguin Winit fork](https://github.com/FreakyPenguin/winit)
directly through the root `[patch.crates-io]`; no submodule or local Winit vendor
copy is required. The override applies to Iced's transitive dependency too.

- Branch: `interconnect/wayland-file-drop-0.30.13`.
- Pinned commit: [`752d80ad`](https://github.com/FreakyPenguin/winit/commit/752d80adf83bca01e2992f7e2ff6c7baa78444a4).
- Base: upstream Winit 0.30.13, commit
  `e9809ef54b18499bb4f2cac945719ecc2a61061b`.
- Fork notes: [INTERCONNECT_PATCH.md](https://github.com/FreakyPenguin/winit/blob/752d80adf83bca01e2992f7e2ff6c7baa78444a4/INTERCONNECT_PATCH.md).

The manifest pins the full commit hash, rather than following the branch tip.
Cargo fetches/caches the public HTTPS repository and `Cargo.lock` records the Git
source/revision. Update the manifest and lockfile together when changing the
backport; normal builds use `--locked`.

The fork keeps `dpi` on crates.io, matching the published Winit package. The
upstream Git workspace otherwise resolves its older local `dpi` member instead
of the application's locked registry version. This is the only manifest
adjustment beyond enabling Winit's existing `percent-encoding` dependency for
Wayland; no dependency versions are upgraded.

Winit 0.30.13 does not implement Wayland file-drop events. The application already
uses Wayland, so composer-only handling would leave its primary Linux backend
without drop support. `src/platform_impl/linux/wayland/data_device.rs` binds the
existing SCTK data-device implementation on the window's own connection and seats.
It accepts only copy actions and `text/uri-list`, prefetched asynchronously through
a nonblocking pipe in the existing calloop event loop. It never opens/stat files
or reads/changes clipboard contents. URI metadata is bounded to 256 KiB and 256
paths, rejects remote hosts/URLs, query/fragment strings and NULs, and decodes
percent-encoded local paths without requiring UTF-8 filenames. Application
validation runs separately off the UI thread.

Enter/motion/drop deliver native pointer coordinates in physical pixels before
file events, so Iced can hit-test separate room/thread panes. EOF readiness marks
`dispatched_events`, ensuring a completed transfer wakes the application even if
no further Wayland event follows. Offers and pipe sources are destroyed on
completion, cancellation, replacement, seat removal and window close. There is
no periodic timer or added polling loop. The manager is optional on compositors
without the protocol. The Wayland Cargo feature enables the existing optional
`percent-encoding` dependency.

macOS drag enter/update/drop also forwards content-view coordinates; successful
conclusion clears file-hover state. This path is pending native-host acceptance.
The application's Linux window backend remains Wayland; X11 was not enabled or
changed by this patch. Wayland native acceptance and
application routing/stale-result tests are recorded in
[connected UI validation](../validation/connected-ui.md) under file drops.

When updating Interconnect's Iced/Winit integration, compare/remove this backport
once it can consume upstream Wayland drops with usable coordinates.
