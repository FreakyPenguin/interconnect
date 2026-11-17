# Development

Open the repository in VS Code with the Dev Containers extension and choose **Dev Containers: Reopen in Container**. The default configuration supports building, linting, and testing without requiring a display or GPU on the host. Run the `interconnect` desktop client with a saved account or use `--demo` for isolated UI development; see [UI validation](validation/connected-ui.md).

The image uses Debian Bookworm and installs the latest stable Rust available when its Rust installation layer is built, together with rustfmt, Clippy, and rust-src. The repository's `rust-toolchain.toml` selects stable both inside and outside the container. To refresh an existing container, run `rustup update stable`; alternatively rebuild without the Docker cache. This intentionally tracks stable rather than pinning an exact compiler version. Pin a version when reproducible release builds are introduced.

Included native dependencies cover Wayland, X11 fallback, font availability, Vulkan/EGL with Mesa software rendering, SQLite, D-Bus tools, and C/C++ builds. TLS uses Rustls/AWS-LC and Secret Service uses its Rust cryptography backend; neither needs system OpenSSL or libdbus development packages. A desktop session bus and its services are still needed for desktop integration. Selected Cargo dependencies and features are documented in [the client architecture](architecture/client.md).

The container installs `fonts-noto-color-emoji` for Unicode emoji fallback
alongside DejaVu. Rebuild the container to apply this addition. Confirm the font
is installed with `fc-list ':family=Noto Color Emoji' family file`. The UI selects
the color emoji font for recognized emoji sequences in messages, composers, and
thread excerpts; see [emoji validation](validation/connected-ui.md#demo-checks)
for coverage and platform limits.

Run the repository checks:

```sh
cargo build --locked --bin interconnect
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Dependency cleanup (2026-11-17): Secret Service 5.2 consolidates seven obsolete
crypto packages, reducing the lockfile from 756 to 749 packages. Formatting,
workspace Clippy with warnings denied, the locked desktop build, and workspace
tests pass (180 library and 320 desktop tests; two library tests remain opt-in).
The locked graph has no OpenSSL/libdbus crates, and `ldd` on the Linux binary
shows neither library. Container and CI package lists omit `libssl-dev` and
`libdbus-1-dev`. A clean-container build without those development packages has
not been run locally because Docker/Podman are unavailable; the existing
container already has them installed. Live keyring compatibility and native
macOS builds remain untested for this upgrade; no saved account or connected
client was accessed.

Development/test builds optimize `iced_tiny_skia` and `tiny-skia` at level 2.
Raster conversion, compositing, and scaling execute on the UI thread; leaving
these crates unoptimized made large image views stall input during redraws.
Application code retains the normal development profile, and release settings
are unchanged. Rebuilding picks up these package-specific profile settings.

## Linux continuous integration

[Linux CI](../.github/workflows/linux.yml) runs the checks above on Ubuntu 24.04
for pushes and pull requests, daily at 04:23 UTC, and via **Actions → Linux CI →
Run workflow**. Scheduled runs use the default branch and begin once the workflow
is on that branch. CI follows the repository's stable Rust toolchain, installs
the native build dependencies and test fonts, and caches Cargo downloads.

The build and default workspace tests run headlessly; ignored tests requiring
an isolated desktop fixture remain opt-in. A passing job does not validate
connected accounts, desktop integration, rendering, or idle resource usage;
continue using the relevant validation guides for those checks. The workflow
does not launch the client or publish binaries.

## macOS build automation

[macOS build](../.github/workflows/macos.yml) builds the desktop client on the
Apple Silicon `macos-15` runner for pushes, pull requests, manual dispatch, and
daily at 04:43 UTC on the default branch. It follows stable Rust and uses the
runner's Xcode command-line tools. Cargo downloads are cached, and compilation
is limited to two concurrent jobs to fit the runner's memory.

The workflow runs `scripts/bundle-macos.sh --release`, which builds the optimized
client with `--locked`, validates its Info.plist, and creates an ad-hoc-signed
`.app`; CI then verifies that signature and uploads a bleeding-edge download.
It does not launch the app or perform distribution signing/notarization. It also
runs the session library tests in the release profile, including native
Keychain error classification and blocking-worker cancellation. These tests do
not access Keychain; the native round-trip fixture is opt-in. The complete test
suite and Clippy remain in Linux CI. Intel builds and native account/desktop
validation are separate follow-up work.

The bundle script defaults to a debug build for local development; pass
`--release` to reproduce the CI build. Large unoptimized macOS binaries can exceed
the linker's 16 MiB DWARF offset limit for compact unwind tables, producing
`__eh_frame section too large`. This is a successful-link warning about unwind
performance, also tracked in [rust-lang/rust#159105](https://github.com/rust-lang/rust/issues/159105).
Optimizing the downloadable build is intended to reduce its unwind data; whether
it removes this warning needs confirmation in a native macOS CI run. Unwinding
and linker diagnostics remain enabled.

The separate future-incompatibility notice for `block 0.1.6` comes from the
macOS GPU dependency chain (`iced` → `wgpu` → `wgpu-hal`/`metal` → `block`),
as checked with `cargo tree --locked --target aarch64-apple-darwin -i block@0.1.6`.
It remains unresolved in the locked dependencies. The bundle build includes
`--future-incompat-report` so its actual compiler diagnostics appear in CI logs;
release optimization does not fix dependency compatibility. Recheck this chain
when updating Iced/wgpu, and verify that a dependency update or targeted upstream
fix removes the reported lint before considering it resolved.

### Download a bleeding-edge macOS build

On GitHub, open **Actions → macOS build**, select a successful run for the branch
and commit you want, then download **Interconnect-macos-arm64-&lt;commit&gt;** from
**Artifacts**. Downloads require a GitHub login and repository read access;
artifacts expire after 14 days. For the latest development build, choose the
newest successful default-branch run. Pull-request runs contain that PR's code.

Extract `Interconnect-macos-arm64.tar.gz` to get `Interconnect.app`, then move it
to Applications if desired. The archive preserves executable permissions.
These are experimental optimized builds for Apple Silicon only, signed ad hoc and
not notarized; macOS Gatekeeper may block opening them. They are not distribution
releases, and native account/desktop behavior still needs validation. To try the
UI without an account, run:

```sh
open /path/to/Interconnect.app --args --demo
```

## Debian package automation

[Debian packages](../.github/workflows/debian.yml) builds one amd64 `.deb`
in the official `debian:stable-slim` container, then checks installation of that
same package on both `debian:stable-slim` and `debian:testing-slim`.
Building on stable keeps the system-library baseline suitable for stable;
the testing check detects incompatible library or package transitions.
The package contains bleeding-edge application code. Builds run on pushes,
pull requests, manual dispatch, and
daily at 05:13 UTC on the default branch.

CI compiles with `cargo build --locked --release --bin interconnect`, then runs
`scripts/package-debian.sh stable VERSION BINARY`. The script installs the client
under `/usr/bin`, adds an application-menu entry, derives linked-library
dependencies with `dpkg-shlibdeps`, and declares dynamically loaded graphics
libraries and fonts explicitly. Portal and keyring services are recommended;
native desktop integration still needs a running session and suitable providers.
Version strings include the Cargo version, build timestamp, run number, commit,
and a `+stable` build-suite suffix. Separate install jobs use the same artifact
in clean containers for both suites, validate the desktop entry, and check
linked libraries without launching the client or opening an account.

To download, open **Actions → Debian packages**, choose a successful run, and
select **Interconnect-debian-stable-amd64-&lt;commit&gt;** under **Artifacts** for
either stable or testing. As with the
macOS builds, downloads require a GitHub login and repository read access and
expire after 14 days.
Extract the downloaded artifact ZIP, then install the `.deb` file with APT so
dependencies are resolved. CI also uses this named ZIP artifact to transfer the
package to both install jobs; raw uploads ignore the configured artifact
name.

```sh
sudo apt install ./interconnect_VERSION_amd64.deb
```

These are downloadable packages, not an APT repository with automatic updates.
Desktop behavior requires manual validation.

## App logo and desktop icons

[docs/logo.svg](logo.svg) is the editable source for the app logo. Regenerate the
checked-in PNG sizes and macOS ICNS after changing it:

```sh
# Requires Python 3 and ImageMagick (magick or convert).
python3 scripts/generate-icons.py
```

Normal builds and packaging use the assets in `assets/icons` and do not require
ImageMagick. The startup/profile-opening and sign-in views share one cached,
embedded logo with the window icon; the logo adds no animation or timer.

The Debian package installs `interconnect.desktop` and scalable/PNG icons under
the `hicolor` icon theme. The Linux window application ID and X11 class are
`interconnect`, matching the desktop entry and icon name so taskbars can group
the window with its launcher. Wayland uses the installed desktop entry for the
icon; an uninstalled `cargo run` may still show a generic taskbar icon. The
existing notification desktop-entry hint uses this same identity where honored
by the notification service.

On macOS, `scripts/bundle-macos.sh` installs `interconnect.icns` into the bundle
and declares it in `Info.plist` for Finder, the Dock, and the app switcher.
Launch the `.app` to use that identity; a bare `cargo run` does not have a bundle
icon. There is no separate system-tray/menu-bar item, and window-close behavior
is unchanged. Native icon appearance still requires platform validation; see
[the UI validation guide](validation/connected-ui.md#app-logo-and-desktop-icons).

## Running the client

Run `cargo run --locked --bin interconnect -- --demo` for a memory-only workspace
with spaces, sectioned room navigation, and room/thread conversations. It uses
no saved session and sends no Matrix messages. See [demo checks](validation/connected-ui.md#demo-checks)
for shortcuts and fixtures. Normal `interconnect` launches
use the same visual shell with the saved Matrix session, or show the login form
for a new or signed-out profile. The desktop supports password and browser SSO
sign-in; see [desktop login validation](validation/connected-ui.md#desktop-login).

## Device verification

The account-avatar menu’s **Verify device…** action opens own-device SAS
verification with another Matrix client. Incoming requests also show a review
banner without opening the dialog automatically.
The session-owned worker subscribes to SDK trust and incoming to-device requests,
loads devices on demand, and sends confirmation only for the current displayed
comparison. See [verification validation](validation/verification.md) for protocol
fixture coverage, cancellation, scope, and live acceptance. In `--demo`, the same
entry opens a memory-only sample comparison without an account or network.

## UI components

The desktop modules under `src/bin/interconnect/` separate reusable picker and
popover views from Matrix operations. `emoji_picker.rs` owns the catalog and
selection API; reaction and composer adapters decide what a selection does.
`emoji_completion.rs` shares catalog suggestions while keeping editor focus.
`context_menu.rs` adapts the shared popup host to right-click actions for room
entries, messages, and thread summaries. Actions are resolved again on selection
to reject stale conversations and permission changes; the menu adds no timers.
`message_edit.rs` temporarily replaces each conversation’s composer, retaining its
draft until Save/Cancel. The runtime-owned connected composer admits SDK replacement
events asynchronously and rechecks the target’s ownership/content.
`message_delete.rs` owns captured confirmation popups and account-scoped pending
redactions. The connected composer rechecks membership, content, and Matrix power
levels, then awaits the server using a stable transaction ID for explicit retry.
`people_picker.rs` provides reusable directory search for invitations and new
chats. Hosts namespace asynchronous results by account and form/view generation.
See [architecture](architecture/client.md) for ownership and lifecycle rules.

## Message search

The navigator's **Search messages** view uses Ctrl+Shift+F (Cmd+Shift+F on
macOS), separately from Ctrl+F in-room search and Ctrl+K room navigation. The
`search` service wraps the homeserver search endpoint with joined-room filtering,
a timeout, and runtime-safe client ownership. The desktop owns request/account
generations, cancellation, result pagination, and a bounded memory-only cache of
recently loaded message projections. Local matching runs off the UI thread.
There is no persistent search index or idle search worker. **Search older
encrypted messages** explicitly pages backwards through joined encrypted rooms
(and rooms whose encryption state is unknown), using the SDK to decrypt each
page. It rotates between rooms, processes up to 20 pages of 50 events per click,
and offers Stop/Continue with in-memory pagination state. Missing keys and
unavailable rooms are counted; selected UI timelines and read receipts are
untouched. See [search validation](validation/connected-ui.md#demo-checks).

## Renderer selection

`interconnect` builds both Iced renderers. **Settings → Graphics** controls the
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
ICED_BACKEND=tiny-skia cargo run --locked --bin interconnect -- --demo

# Require wgpu; initialization errors are reported instead of falling back.
ICED_BACKEND=wgpu cargo run --locked --bin interconnect -- --demo
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
`RUST_LOG=iced_wgpu=info cargo run --locked --bin interconnect -- --demo` and inspect
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
The same settings apply to direct `target/debug/interconnect` launches.

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
evidence and native acceptance checks.

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

The absolute Wayland socket path lets the application connect while its runtime directory remains private. UID matching is enabled for the container user so the host compositor can authenticate it. Check the connection with `wayland-info`, then run `cargo run --locked --bin interconnect -- --demo`. This setup requires a local Wayland socket; remote Docker hosts and non-Linux hosts need a different display arrangement. If the compositor restarts and replaces its socket, recreate the container.

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
NODEVICE_SELECT=1 cargo run --locked --bin interconnect -- --demo
```

If startup stalls before an adapter is selected, compare `vulkaninfo --summary`
with `NODEVICE_SELECT=1 vulkaninfo --summary`. Removing `DISPLAY` for a Wayland
launch (`env -u DISPLAY ...`) also bypassed the observed stall, as did
`WGPU_BACKEND=gl`. Startup fallback only handles an initialization error; it
cannot recover while a driver call remains blocked.

Only the display socket is shared by these instructions. Desktop notifications, portal-based browser opening for SSO, and secret-service integration also need a deliberate host session D-Bus setup or native-host testing. The SSO callback listens on localhost port 8765 and needs forwarding to the browser host. Display forwarding alone does not provide these integrations.

Use native-host runs with hardware rendering for representative battery and suspend/resume measurements; container software rendering is a functional development aid.

## References

- [Rust development container image](https://github.com/devcontainers/images/tree/main/src/rust)
- [Rust toolchain overrides](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file)
- [Iced platform dependencies](https://github.com/iced-rs/iced/blob/master/DEPENDENCIES.md)

## Native macOS account storage

Native macOS builds store the complete saved session and SDK encryption keys in
Keychain through Security.framework. No D-Bus service or container keyring mount
is needed on a Mac. The default profile is
`~/Library/Application Support/interconnect/profiles/default`; it contains the
encrypted SDK stores and local preferences. Launch the bundled app normally to
sign in, or pass a fresh explicit profile directory for a dedicated test account.
Demo mode continues to skip all account and Keychain access.

The first credential access may ask to unlock the login keychain or allow
Interconnect access. Approve access only for the intended app. Rebuilding or
replacing an ad-hoc-signed bundle may require access again; native acceptance
must check this. Denial/unavailability reports an error and retains existing
state, with no plaintext fallback. Security.framework calls run off the UI and
async runtime threads. An already-running native call can outlive caller
cancellation while retaining the profile lock; a retry waits for it to finish.

See [native account validation](validation/connected-ui.md#native-macos-account-storage)
for the opt-in fixture and manual checks. Native execution remains unvalidated in
the Linux development environment.

## Using the host Secret Service from the container

On a local Linux Docker host, the client can use the desktop
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

After rebuilding, `updateRemoteUserUID: true` aligns the container process UID
with the host desktop user's UID for normal D-Bus authentication. This
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
[login validation](validation/connected-ui.md#desktop-login).

References: [Secret Service specification](https://specifications.freedesktop.org/secret-service/latest-single/),
[Dev Container bind mounts](https://code.visualstudio.com/remote/advancedcontainers/add-local-file-mount).

## Attachments and desktop services

The paperclip opens a native file picker: Linux uses the XDG desktop portal and
needs a session bus and portal backend offering `FileChooser`; macOS uses its
native dialog. Picked host paths must be accessible at the same location inside
a development container. Sharing the session bus does not share files.

Received images, files, audio, and video offer **Open** and **Save as…** in room
and thread timelines, thread originals, and the image viewer. Both request the
original bytes through the SDK, including decryption. Save uses a native dialog
and replaces the destination only after the complete file is written. Open uses
`xdg-open` on Linux and `open` on macOS with a private temporary copy retained
until the client exits. These handlers need access to the same filesystem;
container display forwarding alone does not provide that access. Downloads are
explicit, asynchronous, and cancelled when their conversation becomes stale.
Studio → Images in demo includes generated PNG and text downloads without network
or account access.

Each room/thread draft stages one file, up to 50 MiB, with composer text as its
caption. Files are read at send time; successful queue admission clears the
draft, while later delivery failures use the timeline Retry action. The
homeserver may impose a smaller size limit. Demo sends stay in memory.

Ctrl+V (Cmd+V on macOS) prefers native file lists, then images, then ordinary
text. Multiple files show an error; plain-text paths stay text. Native clipboard
reads happen only on paste, without an observer. Linux prefers Wayland
data-control with X11 fallback; compositor support is required.

Drop one local file onto the destination room/thread pane. The hover outline
identifies the target; obscured panes, overlays, and the sidebar do not accept
drops. The [Winit fork](architecture/winit-file-drops.md) supplies Wayland support
and native coordinates. macOS drop routing needs native acceptance.

See [UI validation](validation/connected-ui.md#sending-and-room-management) for
attachment, clipboard, drop, people-search, invitation, and room-creation checks.
