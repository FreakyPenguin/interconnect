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

### README screenshots and tours

Use demo data only; capture app content without desktop chrome or unrelated
windows. Install Noto Color Emoji and a CJK font for multilingual samples. Set
Light/Dark in Settings and let each state settle before capturing.

| Asset | Theme | Workflow |
| --- | --- | --- |
| `demo.png` | Light | Studio → Design studio; Alt+T opens a thread. |
| `spaces.gif` | Light | Space picker → Design, then Mobile; keep the conversation open. |
| `keyboard-navigation.gif` | Dark | Room picker → General; Back; account search → quiet; Escape; Forward. |
| `formatting.gif` | Dark | Message formatting; show code and toggle a Markdown/math draft preview. |
| `authentication-verification.gif` | Light | Local sign-in fixture, device selection, SAS comparison, simulated completion. |

Capture at 1440×960; resize GIF app content to 1200×800 with captions below
(1200×876 total). Hold steps for 2–4 seconds, SAS comparison for 5. Use a shared
256-color palette and inspect decoded frames. Keep fixtures/raw captures outside Git.
Existing media predates current code/table controls; see [the guide](keyboard.md#code-blocks).

For the authentication tour, use a temporary capture-only build outside the
repository: render `Authentication::SignIn` over demo, keep SDK subscriptions off,
and intercept `SubmitPassword` to clear the password and open demo verification.
Use `https://matrix.example.org` and `@jamie:example.org`. Keep the demo's trust/
completion labels and a persistent **LOCAL SIMULATION · NO SERVER CONNECTION**
caption. Restore the normal build afterward. This illustrates UI, not live sign-in,
trust changes, or key retrieval.

Example unsent formatting draft:

```text
**A useful identity**
$$\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}$$
```

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
`block` 0.1.6 dependency emits an `uninhabited_static` future-incompatibility
warning. It needs an upstream fix or reviewed backport before Rust rejects it.
Inspect the dependency with
`cargo tree --locked --target aarch64-apple-darwin -i block@0.1.6` and the native
build report with `cargo report future-incompatibilities --package block@0.1.6`.

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
Tag pushes run the Linux format/test/Clippy checks and macOS build/signature/session
checks. After these and Debian installation checks pass, the workflow creates a GitHub Release
containing an amd64 `.deb`, `interconnect-VERSION-linux-x86_64.tar.gz`,
`Interconnect-macos-arm64.tar.gz`, and `SHA256SUMS` covering all three downloads.
Prerelease versions create prereleases and use Debian `~` ordering. Other runs
do not publish. The release must not already exist when the publishing job runs.

Download from **Releases** and verify with `sha256sum --check SHA256SUMS`.
Published Linux builds use Debian stable's library baseline. Graphics libraries,
desktop services, and fallback Unicode/emoji fonts come from the system. Extract the macOS archive to get
`Interconnect.app`; it is Apple Silicon only, ad-hoc signed, and not notarized.

To package an existing local release build:

```sh
cargo build --locked --release --bin interconnect
bash scripts/package-linux-release.sh v0.5.0 target/release/interconnect
```

Use the version matching the manifest. Outputs go to `target/linux-release/`
and `target/debian/`. Direct Debian packaging uses
`scripts/package-debian.sh stable VERSION BINARY`.

Version bumps must update `Cargo.toml`, the root entry in `Cargo.lock`, packaging
examples, and both `CFBundleShortVersionString` and `CFBundleVersion` in
`packaging/macos/Info.plist` (numeric version only, without the prerelease suffix).

### Before tagging

- Run the locked format, build, workspace tests, and Clippy checks above.
- Inspect demo screenshots and Light/Dark layouts at desktop and 640×480 sizes;
  check both renderers where available and measure settled visible/hidden idle use.
- Upgrade a disposable 0.2.0 profile in place: retain device identity, drafts,
  room organization, and available encrypted history. Test restart/offline resume.
- Register and recover encrypted history on fresh dedicated profiles; check the
  recovery key with a second client. Exercise verification in both directions.
- Test native Linux/macOS credential prompts, notifications, clipboard, and file
  picking. Build/fixture success does not establish these platform integrations.
- Package the release binary, inspect archive contents/version/notices, and verify
  checksums. Keep raw validation artifacts and private account data outside Git.

## App logo and desktop icons

Edit [docs/logo.svg](logo.svg), then regenerate checked-in assets:

```sh
python3 scripts/generate-icons.py
```

The generator needs Python 3 and ImageMagick; normal builds do not. The Linux
application ID/desktop entry is `interconnect`; macOS uses the bundled icon.
Keep the SVG background and message-route cutouts transparent. After regeneration,
check transparency in all eight PNG sizes and the PNGs embedded in the macOS ICNS.

## Renderer selection

Settings → Advanced → Graphics applies on the next launch. GPU rendering defaults on,
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
