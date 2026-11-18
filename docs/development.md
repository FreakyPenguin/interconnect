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

### CI dependency caching

All three build workflows use the commit-pinned
[Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) action to retain Cargo
downloads and compiled dependencies in `target/`. Linux checks, Debian release
packaging, and macOS release builds use separate cache keys for their native
build environments and profiles. The action also keys caches by Rust compiler,
Cargo manifests/lockfile, and build environment variables. It can restore a
previous dependency cache after a lockfile change; Cargo rebuilds what changed.
Workspace application artifacts are excluded, so application code still builds
on each run. The action respects the Debian job's custom `CARGO_HOME`.

The first successful run in each environment populates its cache. Later runs
should reuse compatible dependencies; a new stable compiler or an evicted cache
requires a cold build. Toolchain installation, native package installation,
application compilation/linking, tests, and packaging still take time. Keep the
existing two-job limits on Debian/macOS to stay within runner memory limits.

To validate this in Actions, compare two successful runs with unchanged Rust
version, dependencies, and build settings. Check the cache restore/save logs and
confirm the second Cargo build reuses dependencies. Repeat after an application
source edit and a dependency update to check reuse and selective recompilation.
These timings and hosted cache behavior must be verified on GitHub; local YAML
validation does not establish a measured speedup. If the native build setup
changes incompatibly, bump that workflow's explicit cache `key`.

## Running the client

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo mode uses local data without saved accounts or a Matrix connection. Use it
for UI experiments and screenshots. Without `--demo`, the client resumes its
profile or opens sign-in. Use a dedicated test profile for connected validation;
see [profiles](settings.md#account-profiles) and [UI checks](validation/connected-ui.md).
Do not send test messages or restart an existing connected client without authorization.

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
bash scripts/package-linux-release.sh v0.1.1-dev target/release/interconnect
```

Use the version matching the manifest. Outputs go to `target/linux-release/`
and `target/debian/`. Direct Debian packaging uses
`scripts/package-debian.sh stable VERSION BINARY`.

Development builds currently use `0.1.1-dev`. When bumping the package version,
update `Cargo.toml`, the root package in `Cargo.lock`, and the packaging example
above. Keep `CFBundleShortVersionString` in `packaging/macos/Info.plist` at the
matching numeric version (`0.1.1`), without the prerelease suffix.

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

macOS uses Security.framework Keychain directly, under service `Interconnect`;
it needs no D-Bus forwarding. Use a native app bundle for prompts and notification
identity. Credential calls run on serialized blocking workers, with the profile
lock retained until native work completes even if its UI request is cancelled.
See [account checks](validation/connected-ui.md#native-macos-account-storage).

## Attachments and desktop services

Linux file picking uses the desktop portal; macOS uses a native picker. Pasted
files and picker results must be accessible from the client environment: mount
host-selected paths at the same paths when using a container. Paste prefers file
lists, then images, then text. One file up to 50 MiB may be attached per send.

Open attachments through `xdg-open` on Linux or `open` on macOS; temporary files
remain until quit so the external application can read them. File drops use the
[pinned Winit fork](architecture/winit-file-drops.md). See
[notifications](validation/notifications.md), [verification](validation/verification.md),
and [keyboard shortcuts](keyboard.md) for desktop behavior and checks.
