# Development

## Build and checks

Open the repository in VS Code with **Dev Containers: Reopen in Container**.
The Debian-based image includes stable Rust, rustfmt, Clippy, Wayland, Mesa
Vulkan/EGL, SQLite, C/C++ tools, D-Bus utilities, and Noto Color Emoji.
See the [Dockerfile](../.devcontainer/Dockerfile) for native dependencies.
`rust-toolchain.toml` tracks stable; refresh with `rustup update stable`.
TLS uses Rustls and Secret Service uses Rust cryptography; OpenSSL and libdbus
development packages are unnecessary.

```sh
cargo build --locked --bin interconnect
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

For UI changes, use `cargo test --locked --bin interconnect`; shared/backend
changes require the workspace suite. Build before desktop validation. Ignored
native/stress tests are opt-in, documented in the validation guides.
Development/test profiles optimize tiny-skia to keep image redraws responsive.

[Linux CI](../.github/workflows/linux.yml) builds, formats, tests, and lints.
[macOS CI](../.github/workflows/macos.yml) builds an Apple Silicon release bundle,
checks its signature/icon, and tests session lifecycle without Keychain access.
CI builds do not establish native desktop acceptance.

CI caches dependencies separately per platform/profile; compiler, manifests,
lockfile, and environment affect reuse. Bump a workflow's cache key after
incompatible native setup changes. Keep Debian/macOS builds at two jobs for memory.
Measure cache reuse in Actions logs/timings, not local checks.

## Running the client

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo mode uses local data without saved accounts or a Matrix connection. Use it
for UI experiments and screenshots. Without `--demo`, the client resumes its
profile or opens sign-in. Use a dedicated test profile for connected validation;
see [profiles](settings.md#account-profiles) and [UI checks](validation/connected-ui.md).
Do not send test messages or restart an existing connected client without authorization.

Use `cargo run --locked --bin interconnect -- --help` to list startup options.
`--profile work` selects a named profile in the regular data directory;
`--profile-path /path/to/test-profile` selects an explicit directory.

### README screenshots

The images in `docs/images/` are captures of the running demo, using a 1440×960
window and the default light theme on Linux/Wayland. Capture only the app's
content, excluding desktop chrome, tooltips, and other windows. Keep each image
focused on a different workflow:

| Image | Demo setup |
| --- | --- |
| `demo.png` | Start in Studio → Design studio; Alt+T opens the latest thread beside the room. |
| `spaces.png` | Keep that room/thread open; use Home's tree to select Studio → Product → Design, then reopen the tree. The filtered sidebar and unchanged conversation should both be visible. |
| `formatting.png` | Ctrl+P → Message formatting; enter the draft below and click the preview eye. Focus the composer and use Page Up to show the beginning of the timeline with highlighted code. |
| `search.png` | Ctrl+Shift+F → search for `quiet` and submit to show matches across conversations. |

`keyboard-navigation.gif` is a paced sequence of actual demo captures with key
labels added below the app. Capture the following states in order, allowing the
UI to settle after every key press:

1. Initial Design studio room; Alt+T opens its thread.
2. Ctrl+Shift+P opens the space picker; type `Design`, Down selects Mobile,
   then Enter filters the sidebar while preserving the room/thread.
3. Ctrl+P opens the room picker; type `Design studio`, then Enter opens the room.
   Alt+T returns to its thread.
4. Ctrl+Shift+F opens account search; type `quiet`, Enter shows results,
   then Escape returns to the conversation.

Use the same 1440×960 capture area; resize to 1200×800 and append a 76-pixel
caption/key strip. The GIF has 14 frames held for 1.3–2.8 seconds (26 seconds in
total), a shared 256-color palette, and no transition effects. Keep source frames
and intermediate rendering files outside the repository. Verify every capture
contains only the demo window and inspect the decoded GIF before replacing it.

The formatting capture uses this unsent draft:

```text
**A useful identity**
$$\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}$$
```

These screenshots demonstrate local UI states, not connected-server acceptance
or draft persistence across restarts. Demo history is already fully included;
its search view does not exercise encrypted history downloads. See the
[connected UI guide](validation/connected-ui.md) for those checks.

## macOS builds

On a Mac with Rust and Xcode command-line tools:

```sh
./scripts/bundle-macos.sh --release
open target/Interconnect.app --args --demo
```

Omit `--release` for a debug build. Both profiles produce `target/Interconnect.app`
(or under `CARGO_TARGET_DIR`). The script builds and ad-hoc signs the bundle;
it does not launch it. The bundle identifier is `io.github.interconnect.desktop`.
Distribution signing and notarization are not configured.

A debug linker warning about `__eh_frame section too large` can affect unwind
performance; prefer a release build for desktop checks. The transitive Metal
`block` dependency can also emit a future-incompatibility warning; inspect it
with `cargo tree --locked --target aarch64-apple-darwin -i block@0.1.6`.

### Download a bleeding-edge macOS build

Open **Actions → macOS build**, select a successful run, and download
**Interconnect-macos-arm64-<commit>** from Artifacts. Extract the artifact ZIP,
then `Interconnect-macos-arm64.tar.gz`, and open `Interconnect.app`.
Artifacts require a GitHub login/read access and have limited retention.
These experimental Apple Silicon builds are ad-hoc signed and not notarized.

## Debian package automation

The [Debian workflow](../.github/workflows/debian.yml) builds an amd64 package
on Debian stable and checks installation of the same package on stable and
testing. Download **Interconnect-debian-stable-amd64-<commit>** from a successful
**Actions → Debian packages** run, extract the ZIP, and install with:

```sh
sudo apt install ./interconnect_VERSION_amd64.deb
```

This is a downloadable package, without an APT update repository. GitHub artifacts
require login/read access and expire. The package includes a desktop entry and
derived system-library dependencies; desktop portals and keyring providers need
a running desktop session. Installation checks do not launch an account.

### Tagged releases

A pushed version tag, optionally prefixed with `v`, must match `Cargo.toml`.
Tag pushes also call the macOS build workflow. After the macOS build/signature/session
checks and Debian installation checks pass, the workflow creates a GitHub Release
containing an amd64 `.deb`, `interconnect-VERSION-linux-x86_64.tar.gz`,
`Interconnect-macos-arm64.tar.gz`, and `SHA256SUMS` covering all three downloads.
Prerelease versions create prereleases and use Debian `~` ordering. Other runs
do not publish. The release must not already exist when the publishing job runs.

Download from **Releases** and verify with `sha256sum --check SHA256SUMS`.
The Linux tarball uses Debian stable's library baseline and does not bundle graphics
libraries, fonts, or desktop services. Extract the macOS archive to get
`Interconnect.app`; it is Apple Silicon only, ad-hoc signed, and not notarized.
The separate Linux CI workflow is not a publication dependency; confirm its checks
pass for the release commit before tagging.

To package an existing local release build:

```sh
cargo build --locked --release --bin interconnect
bash scripts/package-linux-release.sh v0.2.0 target/release/interconnect
```

Use the version matching the manifest. Outputs go to `target/linux-release/`
and `target/debian/`. Direct Debian packaging uses
`scripts/package-debian.sh stable VERSION BINARY`.

Version bumps must update `Cargo.toml`, the root entry in `Cargo.lock`, packaging
examples, and `CFBundleShortVersionString` in `packaging/macos/Info.plist` (numeric
version only, without the prerelease suffix).

## App logo and desktop icons

Edit [docs/logo.svg](logo.svg), then regenerate checked-in assets:

```sh
python3 scripts/generate-icons.py
```

The generator needs Python 3 and ImageMagick; normal builds do not. The Linux
application ID/desktop entry is `interconnect`; macOS uses the bundled icon.

## Renderer selection

Settings → Graphics applies on the next launch. GPU rendering defaults on,
trying wgpu with tiny-skia startup fallback. Disabling it forces CPU rendering
even if `ICED_BACKEND` requests wgpu. Low-power preference supplies
`WGPU_POWER_PREF=low`, or `high` when disabled; an explicit environment value
(`low`, `high`, `none`) wins. Preferences do not guarantee a device or power budget.

```sh
ICED_BACKEND=tiny-skia cargo run --locked --bin interconnect -- --demo
ICED_BACKEND=wgpu cargo run --locked --bin interconnect -- --demo
```

An explicit `wgpu` requires initialization to succeed. Unset `ICED_BACKEND` for
normal fallback, or use `wgpu,tiny-skia`. Demo ignores saved graphics settings.
`WGPU_BACKEND` chooses an API within wgpu; it does not select tiny-skia.
Device loss after startup does not switch renderers dynamically.

Use `RUST_LOG=iced_wgpu=info` to inspect the selected adapter. llvmpipe/lavapipe
with device type `Cpu` means software rendering through wgpu. Hardware access in
a container requires `/dev/dri` and suitable device/group permissions. Graphics
settings are applied before Tokio/Iced starts, using a Unix re-exec when needed.
See [idle measurements](validation/idle-resources.md) for resource comparisons.

## Running on the host Wayland desktop

For a local Linux host, merge these entries into `.devcontainer/devcontainer.json`:

```json
{
  "mounts": [
    "source=${localEnv:XDG_RUNTIME_DIR}/${localEnv:WAYLAND_DISPLAY},target=/tmp/wayland-host,type=bind"
  ],
  "containerEnv": {
    "XDG_RUNTIME_DIR": "/tmp/matrix-runtime",
    "WAYLAND_DISPLAY": "/tmp/wayland-host"
  },
  "postStartCommand": "install -d -m 700 /tmp/matrix-runtime"
}
```

Keep existing mounts/environment and ensure the container UID matches the host.
Recreate the container if the host replaces its Wayland socket. For GPU access,
add the `/dev/dri` device and appropriate render-group permissions separately.

If Mesa's device-selection layer hangs before opening a window, compare
`vulkaninfo` with `NODEVICE_SELECT=1`; that variable disables the implicit layer.
Try `ICED_BACKEND=tiny-skia` or `WGPU_BACKEND=gl` to isolate graphics issues.
Forwarding the display does not forward D-Bus, portals, credentials, or files.
Browser SSO also needs localhost port **8765** forwarded to the browser's host.

## Using the host Secret Service from the container

Linux account storage needs an unlocked Secret Service provider such as GNOME
Keyring, KWallet, or KeePassXC. Merge this mount/environment with the display setup:

```json
{
  "mounts": [
    "source=${localEnv:XDG_RUNTIME_DIR}/bus,target=/tmp/host-session-bus,type=bind"
  ],
  "containerEnv": {
    "DBUS_SESSION_BUS_ADDRESS": "unix:path=/tmp/host-session-bus"
  }
}
```

Match the host UID. This exposes the host session bus, including notifications
and other services, not only the keyring. No keyring-directory mount is needed.
Check reachability without modifying credentials:

```sh
dbus-send --session --print-reply --dest=org.freedesktop.secrets \
  /org/freedesktop/secrets org.freedesktop.DBus.Peer.Ping
```

## Native macOS account storage

macOS uses Keychain under service `Interconnect`; no D-Bus setup is needed.
Use the native bundle for prompts and notification identity. See
[account checks](validation/connected-ui.md#native-macos-account-storage).

## Attachments and desktop services

Linux file picking uses the desktop portal; macOS uses a native picker.
Container runs need host-selected paths mounted at the same paths inside the
container. See [composition](keyboard.md#composition) for attachment behavior.

External attachment opening uses `xdg-open` or macOS `open`; temporary files stay
until quit. Drops depend on the [Winit fork](architecture/winit-file-drops.md).
See [notifications](validation/notifications.md) and
[verification](validation/verification.md) for native checks.
