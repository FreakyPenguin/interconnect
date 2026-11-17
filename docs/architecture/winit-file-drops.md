# Winit file-drop dependency

The root `[patch.crates-io]` consumes the
[FreakyPenguin Winit fork](https://github.com/FreakyPenguin/winit) directly,
including Iced's transitive dependency. There is no submodule/local vendor copy.

- Branch: `interconnect/wayland-file-drop-0.30.13`.
- Pinned revision: `752d80adf83bca01e2992f7e2ff6c7baa78444a4`.
- Base: Winit 0.30.13, `e9809ef54b18499bb4f2cac945719ecc2a61061b`.
- [Fork patch notes](https://github.com/FreakyPenguin/winit/blob/752d80adf83bca01e2992f7e2ff6c7baa78444a4/INTERCONNECT_PATCH.md).

Revision changes require both manifest and lockfile updates; build with `--locked`.
The fork retains crates.io `dpi` to match the lockfile and enables Winit's existing
optional `percent-encoding` dependency for Wayland, without dependency upgrades.

The Wayland backport uses SCTK on the existing connection/seats and a nonblocking
calloop pipe. It accepts copy actions and `text/uri-list`, bounded to 256 KiB and
256 paths. Remote hosts, query/fragment strings, and NULs are rejected; local
percent-encoded paths need not be UTF-8. It neither opens/stats files nor accesses
the clipboard. Application file validation happens separately off the UI thread.

Pointer coordinates in physical pixels precede file events so room/thread targets
can be hit-tested. EOF wakes event dispatch even without further Wayland events.
Offers/pipes clean up on completion, cancellation, replacement, seat removal,
and window close. No polling/timer is added; unsupported compositors omit the
optional manager. macOS forwards content-view coordinates and clears hover on
successful conclusion. Linux remains Wayland; this patch does not enable X11.

The fork is needed until Iced/Winit supports upstream Wayland drops with usable
coordinates. Use [native UI checks](../validation/connected-ui.md#lifecycle-and-desktop-integration)
for coordinate routing and platform acceptance.
