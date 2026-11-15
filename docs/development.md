# Development container

Open the repository in VS Code with the Dev Containers extension and choose **Dev Containers: Reopen in Container**. The default configuration supports building, linting, and testing without requiring a display or GPU on the host. The Cargo project contains milestone 0 headless and Iced runtime probes; see [validation commands](validation/milestone-0.md). A separate `matrix-ui` binary connects the saved session to room and thread views; see [connected UI validation](validation/connected-ui.md).

The image uses Debian Bookworm and installs the latest stable Rust available when its Rust installation layer is built, together with rustfmt, Clippy, and rust-src. The repository's `rust-toolchain.toml` selects stable both inside and outside the container. To refresh an existing container, run `rustup update stable`; alternatively rebuild without the Docker cache. This intentionally tracks stable rather than pinning an exact compiler version. Pin a version when reproducible release builds are introduced.

Included native dependencies cover Wayland, X11 fallback, font availability, Vulkan/EGL with Mesa software rendering, OpenSSL, SQLite, D-Bus, and C/C++ builds. Selected Cargo dependencies and features are documented in [the milestone 0 ADR](architecture/milestone-0.md).

The container installs `fonts-noto-color-emoji` for Unicode emoji fallback
alongside DejaVu. Rebuild the container to apply this addition. Confirm the font
is installed with `fc-list ':family=Noto Color Emoji' family file`. The UI selects
the color emoji font for recognized emoji sequences in messages, composers, and
thread excerpts; see [emoji validation](validation/connected-ui.md#unicode-emoji--2026-11-15)
for coverage and platform limits.

Run the repository checks:

```sh
cargo build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Development/test builds optimize `iced_tiny_skia` and `tiny-skia` at level 2.
Raster conversion, compositing, and scaling execute on the UI thread; leaving
these crates unoptimized made large image views stall input during redraws.
Application code retains the normal development profile, and release settings
are unchanged. Rebuilding picks up these package-specific profile settings.

## Design preview

Run `cargo run --locked --bin matrix-ui -- --demo` for a memory-only workspace
with spaces, sectioned room navigation, and room/thread conversations. It uses
no saved session and sends no Matrix messages. See the [design preview guide](validation/design-preview.md)
for shortcuts, fixtures, and known interaction gaps. Normal `matrix-ui` launches
use the same visual shell with the saved Matrix session.

## Emoji picker

`src/bin/matrix-ui/emoji_picker.rs` owns the reusable catalog, search/category/
page state, and generic `view`/`Event` API. Build `Catalog::new(entries)`, create
`State::new(&catalog)`, and map its events into the embedding surface's message
type. `State::update` returns a selected catalog key; the caller decides whether
to react, insert into a composer, or do something else. The host supplies the
available size and may disable selection while its own operation runs. Result
scroll state resets when a query/category/page changes, without resetting search
focus. The built-in Unicode catalog is cached once from the pinned `emojis`
dependency; only one page creates widgets. No picker subscription is required.

Entry keys are separate from display names, aliases, categories, and glyphs.
Custom emoji can later add an image-backed visual in `entry_view` and merge
room/workspace catalog entries without changing selection events or reaction
targeting. Media loading and caching belong to the embedding application's
existing asynchronous media path. This change does not discover, upload, or
render custom Matrix emoji.

`popover.rs` hosts arbitrary generic content beside an anchor, clamps it to the
window, and captures backdrop input. The host surrounds the complete message's
hover/tooltip wrappers, while an optional inner container ID anchors the popup
to its action button. This gives an open picker priority over underlying
timestamp and action tooltips; anchor measurement runs only while open.
`reaction_picker.rs` alone owns Matrix
targets, pending sends, errors, and demo behavior. Reuse the picker directly in
another surface, or pair it with the same popup host. Search uses a static caret;
emoji-name tooltips use Iced's ordinary one-shot hover delay.

## Renderer selection

`matrix-ui` builds both Iced renderers. **Settings → Graphics** controls the
renderer on the next launch: **Use GPU rendering** defaults on and tries
**wgpu** first with **tiny-skia** startup fallback. Turning it off forces the CPU
renderer even when `ICED_BACKEND` requests wgpu. **Prefer a low-power GPU** defaults
on, supplying `WGPU_POWER_PREF=low`; off supplies `high`. An explicit power
preference environment value (`low`, `high`, or `none`) wins. These preferences
do not guarantee a particular device or lower power use. See
[application settings](settings.md) for TOML keys, persistence, and restart status.

To compare the renderers with isolated sample data:

```sh
# Force CPU rendering without attempting wgpu.
ICED_BACKEND=tiny-skia cargo run --locked --bin matrix-ui -- --demo

# Require wgpu; initialization errors are reported instead of falling back.
ICED_BACKEND=wgpu cargo run --locked --bin matrix-ui -- --demo
```

With GPU rendering enabled, unset `ICED_BACKEND` for the normal wgpu-first/
software-fallback behavior, or set it to `wgpu,tiny-skia` explicitly. Saved GPU
opt-out always takes priority. Demo launches use default graphics settings and
ignore config files, so environment overrides remain useful for isolated
comparisons. This is a startup choice; device loss
after initialization does not dynamically switch the running renderer.
`WGPU_BACKEND=vulkan` selects a graphics API within wgpu, rather than selecting
Iced's software renderer.

For adapter diagnostics, use
`RUST_LOG=iced_wgpu=info cargo run --locked --bin matrix-ui -- --demo` and inspect
the `Selected: AdapterInfo` record. An adapter named llvmpipe/lavapipe with device
type `Cpu` uses software rendering through wgpu. In a container, expose `/dev/dri`
and the appropriate device permissions to try actual hardware acceleration.
Normal rendering remains event-driven; enabling wgpu adds no application frame
timer. Native-host checks are required to compare GPU memory, battery use,
hidden-window behavior, and suspend/resume.

Graphics values are read from the selected settings file before the window
and runtime exist. Invalid files use defaults and show a Settings error, retaining
the save lockout until repaired. Renderer environment is applied by a Unix
re-exec when needed before Tokio/Iced starts, preserving PID, working directory,
arguments, and inherited environment. This
avoids changing process environment variables while threads may read them.
The same settings apply to direct `target/debug/matrix-ui` launches.

## Settings

The desktop has a TOML-backed settings dialog. See [application settings](settings.md)
for paths, `--config`, defaults, manual editing/reload, and extending the schema.
Demo settings remain in memory and never read the saved configuration.

## Desktop notifications

Linux notifications use `notify-rust` over desktop session D-Bus. The existing
host Secret Service bus mount also carries notification requests. The macOS
backend uses UserNotifications with a bundled app; `./scripts/bundle-macos.sh`
builds a local development bundle on a Mac. Use **Settings → Notifications →
Send sample notification** in `--demo` for an account-independent check. See
[notification validation](validation/notifications.md) for permissions, automated
evidence, native acceptance checks, and macOS account-port limitations.

## Running on the host Wayland desktop

For a local Linux Docker host, launch VS Code from a terminal with `XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY` set. Add the following top-level properties to `.devcontainer/devcontainer.json` and rebuild:

```json
"mounts": [
  "source=${localEnv:XDG_RUNTIME_DIR}/${localEnv:WAYLAND_DISPLAY},target=/tmp/wayland-host,type=bind"
],
"containerEnv": {
  "XDG_RUNTIME_DIR": "/tmp/matrix-runtime",
  "WAYLAND_DISPLAY": "/tmp/wayland-host"
},
"postStartCommand": "install -d -m 700 /tmp/matrix-runtime"
```

The absolute Wayland socket path lets the application connect while its runtime directory remains private. UID matching is enabled for the container user so the host compositor can authenticate it. Check the connection with `wayland-info`, then run the runtime probe with `cargo run --bin runtime-ui`. This setup requires a local Wayland socket; remote Docker hosts and non-Linux hosts need a different display arrangement. If the compositor restarts and replaces its socket, recreate the container.

Mesa software rendering is available without GPU passthrough. For hardware rendering on a Linux host with `/dev/dri`, add `"runArgs": ["--device=/dev/dri"]`. If device access is denied, include `"--group-add=<host-render-group-gid>"` in that array, using the numeric group owning the render device. Proprietary NVIDIA setups may require additional host-specific configuration.

The checked-in container sets `NODEVICE_SELECT=1` to disable Mesa's implicit
Vulkan device-selection layer. With GPU passthrough and VS Code's forwarded
`DISPLAY=:0`, this layer was observed hanging on an X11 DRI3 reply before the
first window appeared, even though the application uses Wayland. Disabling the
layer keeps both display variables available and lets wgpu select the adapter
using its configured power preference. This is a container workaround; native
launches do not set it automatically. The installed
`/usr/share/vulkan/implicit_layer.d/VkLayer_MESA_device_select.json` declares this
disable switch.

Rebuild the container to apply the environment change. In an existing terminal,
test it immediately without accessing a saved account:

```sh
NODEVICE_SELECT=1 cargo run --locked --bin matrix-ui -- --demo
```

If startup stalls before an adapter is selected, compare `vulkaninfo --summary`
with `NODEVICE_SELECT=1 vulkaninfo --summary`. Removing `DISPLAY` for a Wayland
launch (`env -u DISPLAY ...`) also bypassed the observed stall, as did
`WGPU_BACKEND=gl`. Startup fallback only handles an initialization error; it
cannot recover while a driver call remains blocked.

Only the display socket is shared by these instructions. Desktop notifications, portal-based browser opening for SSO, and secret-service integration also need a deliberate host session D-Bus setup or native-host testing. A browser SSO callback must be reachable from the host browser once its port and return mechanism are chosen. Do not assume that displaying a window validates those integrations.

Use native-host runs with hardware rendering for representative battery and suspend/resume measurements; container software rendering is a functional development aid.

## References

- [Rust development container image](https://github.com/devcontainers/images/tree/main/src/rust)
- [Rust toolchain overrides](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file)
- [Iced platform dependencies](https://github.com/iced-rs/iced/blob/master/DEPENDENCIES.md)

## Using the host Secret Service from the container

On a local Linux Docker host, the persistent-session spike can use the desktop
keyring via the host's session D-Bus socket. A Secret Service provider such as
GNOME Keyring, a compatible KWallet service, or KeePassXC with Secret Service
integration enabled must be running on the host. Unlock prompts appear there.

Launch VS Code from the desktop environment with `XDG_RUNTIME_DIR` set. Add the
following mount to the existing `mounts` array in `.devcontainer/devcontainer.json`
and merge the environment variable into any existing `containerEnv`:

```json
"mounts": [
  "source=verifynet-codex,target=/home/vscode/.codex,type=volume",
  "source=${localEnv:XDG_RUNTIME_DIR}/bus,target=/tmp/host-session-bus,type=bind"
],
"containerEnv": {
  "DBUS_SESSION_BUS_ADDRESS": "unix:path=/tmp/host-session-bus"
}
```

Rebuild the container. Keep `updateRemoteUserUID: true`; the container process UID
must match the host desktop user's UID for normal D-Bus authentication. This
assumes the host session socket is `$XDG_RUNTIME_DIR/bus` and Docker is local.
Check the host's actual `DBUS_SESSION_BUS_ADDRESS` if it uses a different path.
Recreate the container if the host login session replaces its bus socket.

Check service reachability inside the rebuilt container without reading secrets:

```sh
dbus-send --session --print-reply --dest=org.freedesktop.secrets \
  /org/freedesktop/secrets org.freedesktop.DBus.Peer.Ping
```

A socket mount exposes the **host session bus**, including other services allowed
by its policy; it is not restricted to Secret Service. Use this with a trusted
development container. A filtering D-Bus proxy is an option when service-level
isolation is required. No host keyring files need to be mounted.

The application uses Secret Service over D-Bus with an encrypted service session.
It does not fall back to plain-text credentials if the service is unavailable.
This mount is opt-in so headless/remote builds still work. See
[session validation](validation/milestone-0.md#persistent-session-check).

References: [Secret Service specification](https://specifications.freedesktop.org/secret-service/latest-single/),
[Dev Container bind mounts](https://code.visualstudio.com/remote/advancedcontainers/add-local-file-mount).
