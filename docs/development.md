# Development container

Open the repository in VS Code with the Dev Containers extension and choose **Dev Containers: Reopen in Container**. The default configuration supports building, linting, and testing without requiring a display or GPU on the host. The Cargo project contains milestone 0 headless and Iced runtime probes; see [validation commands](validation/milestone-0.md). A separate `matrix-ui` binary connects the saved session to room and thread views; see [connected UI validation](validation/connected-ui.md).

The image uses Debian Bookworm and installs the latest stable Rust available when its Rust installation layer is built, together with rustfmt, Clippy, and rust-src. The repository's `rust-toolchain.toml` selects stable both inside and outside the container. To refresh an existing container, run `rustup update stable`; alternatively rebuild without the Docker cache. This intentionally tracks stable rather than pinning an exact compiler version. Pin a version when reproducible release builds are introduced.

Included native dependencies cover Wayland, X11 fallback, font availability, Vulkan/EGL with Mesa software rendering, OpenSSL, SQLite, D-Bus, and C/C++ builds. Selected Cargo dependencies and features are documented in [the milestone 0 ADR](architecture/milestone-0.md).

Run the repository checks:

```sh
cargo build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Design preview

Run `cargo run --locked --bin matrix-ui -- --demo` for a memory-only workspace
with spaces, sectioned room navigation, and room/thread conversations. It uses
no saved session and sends no Matrix messages. See the [design preview guide](validation/design-preview.md)
for shortcuts, fixtures, and known interaction gaps. Normal `matrix-ui` launches
use the same visual shell with the saved Matrix session.

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
