# Connected Iced prototype

Desktop notification setup, behavior, test evidence, and native acceptance steps
are documented in [notification validation](notifications.md).

The `interconnect` binary restores an existing persistent session or offers login
when the profile is missing or signed out. It lists joined rooms and displays SDK room and
thread timelines in encrypted and unencrypted rooms. History loads automatically
in 50-event batches, filling the viewport and an older-message buffer while
continuing to display live updates.
Room and thread text composers and own-device SAS verification are implemented.
See [desktop verification](verification.md) for the comparison flow, SDK trust
reporting, automated protocol evidence, and pending native acceptance. Recovery
UI, receipts, and automatic read marking remain pending.
For encrypted rooms, use an already verified profile with access to the test
room's keys.

Close `session-live` or any other process using the profile first. Keep the host
Secret Service available and unlocked, and provide a Wayland display as described
in [development.md](../development.md).

```sh
cargo run --locked --bin interconnect
```

An optional profile-directory argument follows the binary name. The default is
`$XDG_DATA_HOME/interconnect/profiles/default`, or
`~/.local/share/interconnect/profiles/default` when XDG_DATA_HOME is unset or
relative. See [account profiles](../settings.md#account-profiles) to move an
existing `.matrix-spike` profile while retaining its keyring identity. This
command restores a saved account without creating a device. When no login is
available, the window offers password or browser sign-in; completing sign-in
creates a new device. Room names and decrypted messages appear in the window.

The package and GUI binary are now named `interconnect`, with application settings
under the `interconnect` configuration directory; see [settings](../settings.md)
for reusing an existing settings file. The legacy Secret Service application
attribute (`chatclient-milestone-0`) is retained for saved-login compatibility.
Existing profiles can be used at an explicit path or moved using the instructions
above without a new login.
The window, notification app name, and macOS bundle are branded **Interconnect**.
The macOS bundle identifier is now `io.github.interconnect.desktop`; check its
notification permission on the native host after rebuilding the bundle.

Rename validation on Linux (2026-11-15): formatting, workspace Clippy with
warnings denied, workspace tests (58 library and 100 GUI tests passed; one
existing stress test ignored), and `cargo build --locked --bin interconnect`
passed. Locked Cargo metadata resolves the renamed library and GUI binary;
dependency versions are unchanged. Bundle plist structure, bundle script syntax,
and container JSON syntax also passed checks. Desktop appearance, macOS execution,
and live saved-session restoration were not exercised for this rename.

XDG profile validation on Linux (2026-11-15): formatting, workspace Clippy with
warnings denied, workspace tests (62 library and 101 GUI tests passed; one
existing stress test ignored), and the locked GUI build passed. Tests cover
XDG/HOME resolution, explicit profile arguments, nested private-directory
creation, moved-profile keyring attributes and locking, and invalid identity
file rejection. The documented move commands were exercised against temporary
store/navigation fixtures with unset, empty, relative, and absolute XDG_DATA_HOME,
plus existing-directory and dangling-symlink destination refusal. No real account
was moved, no connected client was restarted, and live keyring restoration and
macOS behavior remain untested for this change.

## Desktop login — 2026-11-15

A missing profile opens a login form without contacting the network or system
keyring. Enter the homeserver URL, username/Matrix ID, and password, then select
**Sign in**. For SSO, enter the homeserver and select **Continue in browser**,
then **Open browser** (or **Copy sign-in link**). The homeserver must support
native sliding sync; browser login also requires Matrix SSO. Registration,
OAuth/OIDC-native login and key recovery are not provided. After login, use
**Verify device** to compare with an existing trusted client; see
[desktop verification](verification.md).

The callback listens on localhost port 8765. In a development container, forward
that port to the same port on the browser's host. The sign-in attempt has a
three-minute deadline; a failed attempt returns to the form for retry. Closing
the window cancels pending login or restoration. Browser launch runs away from
the UI thread; if desktop integration is unavailable, copy the link instead.

Tokens and store encryption keys are saved in Secret Service. Password inputs
are masked, cleared after submission, held in zeroizing application values, and
redacted in message/request Debug output. Passwords are not saved. A successful
login writes a complete session and local device fingerprint before starting
sync, then opens conversations in the same window. Restart restores the same
device locally, including when offline. Keyring, profile-lock, and damaged-store
errors are shown explicitly; they do not automatically replace a saved device.

Missing credentials, logged-out records, and interrupted pre-session attempts
allow a fresh login. A revoked token stops the existing sync service before
returning to the form; no replacement device is created until sign-in is
submitted. Each attempt uses a new `store-<random id>` directory. Earlier stores
remain on disk, and their encryption keys remain in the keyring record's
`retained_store_keys`; these are retained artifacts, with no recovery/import UI.
Legacy records continue to use `store`. Explicit CLI logout deletes the profile's
keyring record, including retained keys. If an attempt fails after browser
approval or password acceptance, remove its abandoned Interconnect device in
another client before retrying. New devices need separate verification/recovery
for historical encrypted messages.

Per-conversation drafts and scroll positions survive reauthentication as the
same account. Signing in as a different account clears those in-memory drafts
and positions. Demo mode bypasses profile restoration and authentication, and
continues to use only local sample conversations.

Automated checks cover the missing-profile form and shutdown without network or
keyring access, password login against a mock homeserver without starting sync,
redacted credential diagnostics, single-flight submission, retry and close,
stale browser results, minimum-window layout, account isolation, legacy/new store
records, store path validation, offline device restoration, and revoked-token
classification. Live password/SSO login, system keyring writes, browser opening,
callback forwarding, relaunch after login, idle-resource measurement, and macOS
acceptance remain manual checks. No connected client was launched or restarted.

Validation passed: `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace` (67 library and 108 GUI tests passed; one
existing stress test ignored), and `cargo build --locked --bin interconnect`.

Manual acceptance with a dedicated test profile:

1. Launch `interconnect /path/to/new-test-profile` and confirm the form appears.
2. Test password sign-in or browser SSO. Confirm the account opens in the same
   window, then close and relaunch to check same-device restoration.
3. With another fresh test profile, test invalid credentials, an unavailable
   homeserver, browser timeout, and closing during sign-in. Retry where offered.
4. Revoke the test device from another client. Confirm sign-in returns after
   sync stops; complete login and check that old stores were retained.
5. Check long errors and keyboard navigation at 640×480, and compare idle
   wakeups with the existing resource-validation procedure. No test messages
   are needed.

## Account avatar in the space rail — 2026-11-15

The account badge occupies the top of the space rail in place of the “m” mark;
there is no duplicate badge below the room-list toggle. It uses the signed-in
user's avatar, with the same colored initials placeholder as room timeline
avatars while unavailable. Hover shows the display name (Matrix ID fallback).
Demo mode shows Jamie Davis's initials without reading an account or network.

Profile loading runs asynchronously alongside sync, reads the SDK's cached own
profile, fetches once for servers without the profiles extension, and listens
for SDK own-profile updates. It adds no polling or animation timer. Servers
without profile updates require a later launch to pick up external changes.
Avatar thumbnails share the existing bounded media service and remain requested
across conversation/space changes. Shutdown drops profile work with the connection.

Automated coverage checks profile fetches with/without an avatar when no room
is open, avatar demand across room changes/removal, and demo media isolation.
The existing shared avatar layout test checks equal image/placeholder sizes.
Validation passed: `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace` (63 library and 102 GUI tests passed; one
pre-existing library test ignored), and `cargo build --locked --bin interconnect`.
Desktop acceptance remains pending: in demo, check the top JD placeholder and
bottom toggle at narrow widths; in an authorized connected session, compare an
account with an avatar and one without, hover for identity, and switch spaces
and conversations while checking draft/scroll preservation. No connected client
was launched or restarted for this change.

## GPU passthrough startup stall — 2026-11-15

After exposing `/dev/dri`, the Intel render node was accessible to the container
user, and Wayland registry discovery succeeded. Default demo startup stopped
before adapter selection; standalone `vulkaninfo --summary` also exceeded an
eight-second timeout. A trace showed a successful X11 DRI3 open reply on the
forwarded `DISPLAY=:0` without the expected descriptor, followed by a blocked
read. The installed Mesa 22.3.6 device-selection layer was implicated by the
following comparisons:

- `NODEVICE_SELECT=1 vulkaninfo --summary` completed and enumerated Intel RKL GT1
  integrated graphics and llvmpipe.
- With `NODEVICE_SELECT=1`, the isolated demo selected the Intel Vulkan adapter,
  acknowledged surface configuration, attached buffers, and received Wayland
  frame callbacks. It remained running until the ten-second test timeout.
- Removing `DISPLAY` also allowed Intel Vulkan initialization. Selecting
  `WGPU_BACKEND=gl` selected the Intel OpenGL adapter. Forced tiny-skia remained
  running until its test timeout.

The container configuration now sets `NODEVICE_SELECT=1`, matching the disable
switch in its installed Vulkan layer manifest. GPU access and X11/Wayland
forwarding remain enabled. See [development setup](../development.md#running-on-the-host-wayland-desktop)
for the immediate launch command and rebuild instructions. The workaround was
tested through per-process environment overrides; a rebuilt container has not
yet been checked. Build and JSON syntax checks passed. No account was opened or
existing connected process stopped. Pixel appearance, hardware power use, and
native-host behavior remain unvalidated.

## Graphics settings — 2026-11-15

**Settings → Graphics** now exposes **Use GPU rendering** and **Prefer a low-power
GPU**, both defaulting to true and applying on the next launch. Disabling GPU
rendering forces `tiny-skia`, even over an `ICED_BACKEND=wgpu` environment value.
The low-power control retains its value while disabled with GPU rendering off.
When GPU rendering is enabled, low-power selects `low` and unchecked selects
`high`; an explicit `WGPU_POWER_PREF` still takes precedence.

The startup path reads the selected `--config`/default file before Iced and Tokio
initialize, applies renderer environment via safe re-exec if needed, and hands
the loaded settings/store to the UI. There is no initial UI-side file read. Demo
launches skip config access, including with `--config`. Invalid files retain
defaults, surface a Settings error, and keep Save blocked until repaired and
reloaded. Existing settings files without graphics keys retain their defaults.

Saving/reloading graphics values marks the Settings button **restart required**
when they differ from launch values. Cancel or a failed save retains current
values. Reloading launch values clears the notice. No renderer is replaced and
no connected session is stopped by a settings edit/save. See
[settings documentation](../settings.md) for the schema and environment precedence.

Local regression coverage includes typed defaults/invalid values, persistence
and comment/unknown-key preservation, pre-window startup, error lockout, demo
isolation, disabled-control behavior, save failure/retry, restart status, and
unchanged conversation selection. Subprocess tests exercise configured high/
low preference, explicit power overrides, GPU opt-out over a GPU-only environment,
and demo isolation through the re-exec path. The existing minimum-window modal
and timeline scroll-preservation tests include the added controls.

Validation: `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`, and
`cargo test --locked --workspace` passed (48 library and 64 GUI tests; one
pre-existing library test ignored). `cargo build --locked --bin interconnect` also
passed. No connected account was opened or restarted.

Desktop acceptance remains pending: save both graphics options, confirm the
restart notice, close/relaunch manually, and compare renderer adapter diagnostics.
With GPU disabled there should be no wgpu initialization, including with a GPU
environment override. Re-enable GPU, choose either preference, and check the
selected hardware adapter on the native host. Power use remains unmeasured.

## wgpu with software fallback — 2026-11-15

The GUI now enables both Iced renderer features. With no `ICED_BACKEND` override,
Iced attempts wgpu first and tiny-skia if initialization fails. `interconnect`
defaults to `WGPU_POWER_PREF=low` while preserving explicit `low`, `high`, or
`none` values. Graphics settings above now control these defaults and can force
software rendering. The environment is applied by re-executing the Unix process before
starting Tokio/Iced, preserving PID, arguments, working directory, and inherited
environment without mutating it while other threads run.

Force `ICED_BACKEND=tiny-skia` to select software directly. Force
`ICED_BACKEND=wgpu` to require wgpu and expose initialization failures, or use
`ICED_BACKEND=wgpu,tiny-skia` for an explicit ordered fallback. This fallback
operates during startup, not after a running GPU device is lost. See
[renderer selection](../development.md#renderer-selection) for demo commands and
adapter diagnostics. No account stores, media budgets, synchronization tasks,
or application redraw timers are changed.

Widget/layout tests construct the software variant of Iced's combined renderer,
keeping the suite independent of GPU/display access. The real rasterization
regression still runs through tiny-skia at 1× and 2× scale. A subprocess test
checks that unset power preference becomes `low` and explicit overrides survive
startup. GPU visual output and performance require separate desktop acceptance.

The locked dependency graph adds the necessary graphics/API packages without
changing any existing package versions. The direct test dependency
`iced_tiny_skia` uses the already pinned 0.14.1 release; vendored Iced files are
unchanged.

Local validation: all 48 regular library tests and 61 GUI tests passed, along
with formatting, Clippy with warnings denied, and the locked GUI build. The
opt-in 100-cycle stress test was not run. Demo checks use only in-memory sample
data. The container has no `/dev/dri`; its Vulkan/GL adapter is llvmpipe (CPU), so
hardware GPU performance, idle power, suspend/resume, and native-host visual
acceptance remain unmeasured. No connected profile was opened or restarted by
these validation runs.

Demo smoke evidence: the default launch logged selection of the Vulkan
llvmpipe CPU adapter and exited with status 0. Forced `ICED_BACKEND=tiny-skia`
stayed running for eight seconds without wgpu initialization logs. With
`ICED_BACKEND` unset and `WGPU_BACKEND=dx12` (unavailable on this Linux setup),
wgpu reported no adapters and the application stayed running through software
fallback for eight seconds. The latter two runs were stopped by timeout
(status 124); they establish startup behavior, not graceful closing or visual
acceptance.

Host acceptance: launch the demo with each renderer, inspect the wgpu selected
adapter, open/close images repeatedly, scroll rooms/threads, select/copy text,
resize, hide/unhide, and suspend/resume. Compare settled idle CPU/wakeups and
active image/scroll behavior using [idle validation](idle-resources.md).

## Click-to-play GIFs — 2026-11-15

GIF previews and avatars stay static. Opening a GIF loads a still original;
**Play GIF** or clicking the displayed image starts playback. **Stop** or clicking
the image freezes it. **Settings → GIF playback** offers **Once** (the default,
one loop) or **For a duration** (1–300 whole seconds, default 5). Timed playback
repeats independently of the file's embedded loop count. Replay starts at frame
zero; closing, redaction/source replacement, shutdown, or loss of window focus
cancels playback. Reopening and regaining focus never resume automatically.

Composited GIF frames, transparency, and disposal are handled by the existing
`image` GIF decoder on the bounded blocking load workers, after the SDK decrypts
the original. Frames start within 512×512 and shrink uniformly to fit the pixel budget
(at most two reductions, to a 128px bounding box). Playback limits are 512 frames,
64 MiB retained animation pixels, and 128 Mi source canvas pixels processed
(with at most one extra frame decoded to detect the boundary). Frame delays
have a 20 ms floor. Malformed or excessive animations retain the decoded still
with a visible playback-unavailable explanation. The 96 MiB attachment LRU
accounts for the still image and all retained animation frames. Decoder working
memory, SDK download buffers, renderer copies, and old snapshots are additional.

The UI subscribes only to the next frame boundary or playback deadline while
playing; there is no idle animation polling. Late ticks skip expired frames
using wall time and never extend the configured duration. Per-play tokens reject
stale ticks after replay or reopening. Frame changes do not refresh SDK media
demand or disturb conversation drafts and scroll positions.

Automated coverage includes GIF pixels/delays, static previews, frame-limit and
truncated-file fallback, LRU pixel accounting, plain/encrypted mock-server loads
and offline reopening, precise one-loop/timed boundaries, an actual subscription
tick, explicit playback, stale ticks, focus/close/redaction cancellation, and
settings validation/persistence. See the demo fixture instructions in
[design preview](design-preview.md). Validation passed: 52 library tests and 68
GUI tests, workspace Clippy with warnings denied, formatting, and the GUI build.
The existing 100-cycle SDK stress test remains explicitly ignored. Desktop visual
checks, live homeserver GIF playback, extended resource measurements, and macOS
behavior have not been validated.

For a dedicated connected profile, inspect existing plain and encrypted GIFs
without sending test messages. Check room previews and the pinned thread
original stay still, then open/play/stop/replay them with both settings. Close,
unfocus, and redact an opened GIF from another client; compare idle CPU/redraws
before and after playback using [idle validation](idle-resources.md). Include a
large or damaged GIF to check the static fallback. Do not restart the user's
connected client without authorization.

## Image messages — 2026-11-15

### Image-viewer responsiveness correction

The user reported roughly two-second cached-image opening and CPU-heavy closing,
with both **Close** and Escape slow. A headless test that actually rasterizes a
2048×2048 RGBA image reproduced expensive UI-thread work in the development
software renderer: initial layout was about 240 ms and full redraws about 350 ms
at a 1000×800 physical viewport. Optimizing `iced_tiny_skia` and `tiny-skia` at
level 2 in the development/test profile reduced these observations to about
17 ms initial layout and 36 ms warm redraws (about 76 ms first redraw). These are
local fixture measurements, not timing guarantees or connected desktop evidence.

Attachment snapshots now retain ready handles from the bounded 32 MiB LRU when
the viewer closes. Reopening an image still in that cache starts with its decoded
handle available, without an empty/loading/ready snapshot exchange. Eviction
still removes entries within the existing budget; original network requests
still require explicit opening. Snapshot comparison uses handle identity rather
than comparing pixel buffers. Avatar snapshot behavior is unchanged.

Plain and encrypted SQLite fixtures repeatedly close/reopen the same original,
assert the decoded handle ID stays stable, and observe no extra media requests
or redundant snapshots. A real widget/render test exercises a 2048px image at
1× and 2× scale and retains the Close button's press across redraws and an
incoming model/caption update; one release emits one Close action. The previous
layout-only checks did not cover actual rasterization. `tiny-skia` is now a
test-only direct dependency at its existing version, with default features off;
no crate versions or vendored code changed.

Validation: all 48 regular library tests and 59 GUI tests passed. Native desktop
confirmation of the reported stall and idle-resource measurements remain
pending. Apply the rebuilt binary on the next user-controlled launch; this work
did not restart or open the connected profile.

### Preview and viewer behavior

Room and thread timelines display image previews beneath their existing text
caption/filename. Visible image rows request static 512×384 thumbnails; a fixed
180px preview frame (up to 320px wide) preserves row geometry while loading or
showing a failure. Clicking the preview opens the original in an in-app viewer.
The pinned thread original has an **Open image** action too. Escape or **Close**
returns to the conversation. The viewer blocks navigation, composer actions,
and backdrop scrolling while preserving drafts and reading positions.

For encrypted media, a supplied encrypted thumbnail is fetched as its own file
and decrypted by the SDK. Without a supplied thumbnail, the image shows **Open
image to load**, and no original is requested automatically. Plain images use
the SDK thumbnail endpoint. Originals are requested only after a click. Failed
originals show **Retry**; reconnect also retries current failed media. While the
viewer covers the timeline, its original is the only demanded media, cancelling
background avatar/preview downloads. Closing it restores viewport demand.
Redaction, source replacement, removal of the conversation, and session shutdown
close an obsolete viewer. SDK caption updates refresh its title.

Avatars and attachments share four load/decode permits. Attachments have a
separate 96 MiB decoded-pixel LRU; viewport demand is capped at 16 previews plus
one opened original. Preview input is limited to 2 MiB and resized within
512×384; originals accept up to 20 MiB and are displayed within 2048×2048.
Both reject dimensions over 4096px and use a 128 MiB decoder allocation limit.
The SDK buffers and decrypts downloads before these application checks; decoder
working memory, renderer copies, and old snapshots are outside the retained
pixel budget. PNG/JPEG/WebP stay static; GIF originals support explicit playback
as described above.
SDK disk retention and encryption settings remain as described below. No cache
file migration or account/key clearing is performed.

Temporary SQLite/mock-server tests verify plain and encrypted thumbnail loading,
no original endpoint request before demand, original decryption, offline disk
reuse after reopening, rejection of corrupt encrypted content, and the combined
four-request concurrency bound. SDK projection retains image metadata alongside
thread relations and sender profiles. UI checks cover stale viewport/click
generations, preview dimensions across ready/loading/failed states, explicit
retry, modal input isolation, preserved drafts/scroll positions, redaction, and
the pinned original. The demo **Studio → Images** room uses only generated
in-memory pixels, including ready and failed-preview examples.

Validation passed: all 48 regular library tests and 58 GUI tests, formatting,
workspace Clippy with warnings denied, and the locked GUI build. The opt-in
100-cycle stress test was not run. No saved account was opened or live-server
messages sent.

Live-server, native desktop, and idle-resource measurements remain pending.
With a dedicated profile, inspect existing plain/encrypted images in a room and
thread, open/close originals, and restart offline to check previously loaded
previews and originals. Verify a missing-thumbnail encrypted image makes no
original request before clicking. Check narrow-window layout, rapidly navigate
while previews load, redact an opened image from another client, and confirm
shutdown cancels pending downloads. Do not send validation messages from the
user's connected account without authorization. Image uploads, file export,
zoom controls, video playback, and custom emoji remain future work.

## Cached avatars — 2026-11-15

Room and thread message headers now display the sender avatar from the SDK
profile. Compact membership/activity rows use the same image at 16px. Missing,
unavailable, unsupported, and malformed images retain the existing initials.
Thread summaries retain their arrow; room-list and space icons remain text.

Viewport reports request only avatars actually shown in message/activity rows,
deduplicated across both panes. Reports reject stale view generations and do not
schedule redraw timers. Profile URI changes in SDK snapshots update demand even
when the viewport is stationary. Navigation cancels obsolete requests. Failed
visible loads are retried after reconnect or after leaving and re-entering the
viewport, without a retry timer. Demo mode never starts this service or requests
media, including if sample rows contain an avatar URI.

The account-scoped service requests static 64px thumbnails through the SDK's
authenticated media API with disk caching enabled. PNG, JPEG, WebP, and GIF are
enabled; GIF uses its first frame. Decoding and resizing run on blocking workers,
with four load/decode slots, at most 256 demanded URIs, and an 8 MiB LRU budget
for retained RGBA pixels. The service rejects encoded input above 2 MiB and
dimensions above 1024px. The SDK buffers the response before that size check;
renderer copies, old UI snapshots, and decoder working memory are outside the
retained-pixel budget.

SDK media retention is explicitly configured to its default 400 MiB content
target, 20 MiB maximum cached item, and 60-day last-access expiry. Cleanup is
checked on media activity at most daily, not by an application timer. This is a
retention target, not a hard SQLite file-size limit. Protected pending uploads
remain exempt under SDK policy. Crypto, room state, and message stores are not
cleared. These limits are fixed defaults for now; settings and explicit clearing
controls remain future work.

Temporary-store tests cover deduplication, decoded-memory reuse, disk reuse after
reopening with media endpoints unavailable, preservation of the device key,
bounded downloads, cancellation, reconnect retries, decoding limits, and LRU
eviction. UI tests cover stale visibility, profile URI changes, demo isolation,
room/thread viewport reports without repeated idle reports, and stable avatar
layout. Existing SDK projection tests check the sender profile's avatar URI.

Validation passed: all 43 regular library tests and 53 GUI tests, formatting,
workspace Clippy with warnings denied, and the locked GUI build. The opt-in
100-cycle lifecycle stress test was not run. No saved account was opened or
messages sent to a live server.

Desktop and host idle-resource acceptance remain pending. On a dedicated test
profile, inspect room and thread avatars, scroll through history, change rooms
rapidly, and check cached avatars after an offline restart. Change a member's
avatar from another client and confirm the new URI is displayed when the SDK
updates its profile snapshot. Check missing avatars, fixed row positions,
offline shutdown, and settled CPU/wakeups using [idle validation](idle-resources.md).

## Cached startup — 2026-11-15

The desktop restores its saved SDK stores and checks the local device signing
key against the keyring record without waiting for `whoami`. Cached navigation
and room messages can appear before synchronization succeeds. The existing
timeline presentation publishes cached rows while history requests are pending;
a slow or failed history request does not hide those rows. The SDK owns offline
availability checks and reconnects; no application polling timer was added.

An unlocked, reachable Secret Service and the original profile/store are still
required. Missing stores, missing or mismatched device fingerprints, wrong store
passphrases, and logged-out records remain errors. Sync reports rejected tokens
as requiring sign-in. The CLI resume/live commands retain their explicit online
identity check.

Temporary-store regressions close all first-client timeline handles and reopen
SQLite with sync returning HTTP 503 and history responses delayed for 60 seconds.
Both encrypted and unencrypted room names and cached message bodies appear
within five seconds of starting presentation. The encrypted variant additionally
decrypts the original ciphertext using the reopened crypto store. Restoring the
sync endpoint delivers another message without duplicating the cached event,
and shutdown cancels the pending history request. A separate session test exercises
the desktop's local restoration against an unreachable homeserver and checks
device-key preservation and rejection of damaged or incomplete saved state.

This establishes cached room reading, not a complete offline archive: only SDK
retained content is available. Uncached history, thread roots, and missing room
keys can still require network access. Message retention limits, non-image
attachments, custom emoji, configurable budgets, and cache-clearing controls
are future work. Avatar and image loading are described above. Existing SDK stores
stay in place; this change introduces no migration or eviction.

Desktop acceptance remains pending: with the client closed and Secret Service
available, disable networking, launch the existing profile, and confirm its saved
room opens with cached text. Check drafts and room switching, restore networking,
and confirm Connected and subsequent updates without duplicates. Close while
offline to check shutdown. Compare settled idle resources using
[the idle-resource procedure](idle-resources.md); no host measurements are claimed
for this change. Automated tests use only temporary profiles and local servers.

Validation passed: formatting, workspace Clippy with warnings denied, all 89
regular workspace tests (including 49 GUI tests), and the locked GUI build.
The opt-in 100-cycle lifecycle stress test was not run. No saved account was
opened or live-server messages sent.

## Unencrypted rooms — 2026-11-15

The desktop no longer rejects rooms without encryption. Both room types use the
same SDK timelines, history pagination, composers, and retry handling. The SDK
chooses the outgoing event type from the room's encryption state; the application
does not override it or fall back to plaintext after an encryption error.

The local mock-server regression opens an unencrypted room and thread, paginates
their history, sends ordinary messages and thread replies as `m.room.message`,
and retries a failed send using the same transaction URL. It also checks repeated
navigation and shutdown release the timelines. The existing encrypted variant
still checks ciphertext on the wire and decrypts it to verify bodies and thread
relationships. Live-server and desktop acceptance for unencrypted rooms remains
pending; repeat the interactive checks below with a dedicated unencrypted room.
The CLI encryption-feasibility harness continues to require an encrypted room.

Validation passed: all 77 regular workspace tests (including 43 GUI tests),
formatting, workspace Clippy with warnings denied, and the locked GUI build.
The existing opt-in 100-cycle lifecycle stress test was not run. No saved account
was opened or messages sent to a live server.

## Timeline event text — 2026-11-15

Room and thread timelines now use SDK-parsed event content instead of the generic
`[non-text event]` placeholder. Membership summaries distinguish joins, leaves,
invitations, kicks, bans, and requests to join. Moderation actions name the
affected member, including their Matrix ID; available reasons are appended.
Profile changes describe display-name and avatar updates. Room changes describe
names, topics, encryption, permissions, pinned messages, and other settings.
Stickers use their description, polls use the supplied fallback text or question,
and calls and live-location shares have text labels. These are plain-text
summaries; video playback, poll voting, and call controls are not implemented.
Image messages now have previews and a viewer as described above.
Ordinary message bodies, including attachment filenames/captions, still come
from the SDK unchanged. The feasibility console shares this wording.

Removed messages and redacted state events have explicit removal text.
Decryption failures remain distinct from unsupported or malformed events; the
latter identify the event type without exposing parser diagnostics. Summaries
are computed when SDK snapshots arrive, with no extra requests or timers.

Local SDK/mock-server tests cover membership transitions and reasons, profile
changes, room name/topic changes and removal, encryption, pins, stickers, polls,
live-location descriptions, custom events, redacted content, and preservation of
text and file-message bodies. Fallback tests cover decryption, parse failures,
and call labels. Connected desktop acceptance remains pending: inspect existing
events in a room and thread, then change a test room's topic or a member's
display name from another client and verify the wording updates. Confirm invites
and moderation events name the affected member, and inspect a removed message.
No real-account mutations are needed for the automated tests.

Validation passed: formatting, workspace Clippy with warnings denied, all 63
regular workspace tests (including 32 GUI tests), and the locked GUI build. The
existing opt-in 100-cycle lifecycle stress test was not run for this text change.

Membership, profile, room-setting, and call events now carry an explicit activity
kind in snapshots. The UI renders them as compact muted rows with a small initial
badge and inline sender, without the ordinary author header or reply hover
panel. Timestamps remain available on hover. Consecutive activity rows keep close
spacing; messages after activity regain their author header. Classification
comes from SDK content, so ordinary messages containing activity-like wording
retain the message layout. Stickers, polls, attachments, decryption failures, and
removed messages retain the message layout too. Room and thread panes use the
same activity presentation and selection behavior; stable event IDs still anchor
scrolling. See [the demo checks](design-preview.md) for the mixed activity fixture.

## Recent thread activity — 2026-11-15

The room timeline retains SDK thread replies and projects one compact activity
row per thread at its most recent loaded reply's position. Interleaved room
messages keep their order; older activity rows for the same thread are
coalesced into the latest one. Each row shows the latest author, original-message
excerpt when loaded, latest-reply preview, and reply count. The count uses the
SDK total when available, otherwise the number of loaded replies. A thread with
an original outside loaded history uses “a thread”; displaying its activity
does not fetch that original or open another SDK timeline.

Click an activity row to open its thread at the newest replies. Clicking again
on an already open thread jumps to newest without replacing that SDK view.
Ordinary reply links still restore the thread's remembered reading position.
Drafts and the room's reading position are preserved. When a summary moves,
scroll anchoring uses a surviving message rather than following the summary
down the history. Inside threads, replies retain their full message layout.

Blue new-reply counts appear on activity rows and their loaded originals' reply
links. These are **local session indicators** for incoming sync replies observed
while the room view is active, after its initial snapshot. Initial history,
pagination, edits, and own replies do not create new counts. Decryption can
resolve an incoming placeholder into a thread reply without duplicating it.
Changing rooms or spaces preserves existing counts during the process lifetime;
activity received while a room is not being watched is not counted as new on
return. Counts are not persisted or synchronized with other clients.

Room activity does not acknowledge a thread. Only actual reply rows visible in
the focused thread pane clear their local new indicators. Opening a thread,
loading history, restoring a scroll offset, and background/unfocused windows
alone do not clear them. Geometry and visible-ID changes trigger reports; there
is no polling or idle timer. This does not send Matrix receipts or mark rooms
read. SDK work retains the existing view cancellation, timeouts, and runtime
ownership rules.

Automated checks cover coalescing across interleaved threads, latest previews,
edits/removals, unavailable originals, Unicode excerpts, historical/own/duplicate
events, decryption and delivery ordering, per-room state, focus and stale-view
rejection, visible-only acknowledgement, scroll restoration, moving-summary
anchors, and repeated geometry reports without timers. An SDK/mock-server test
receives a live reply with no loaded root, verifies that presenting it does not
fetch the root, then loads the original through the thread timeline. Existing
pagination tests verify that historical thread replies carry non-live metadata.

Live-account acceptance remains pending: receive two replies in one thread and
one in another from a second client, confirm summaries and latest previews, then
open one thread. Scroll away from newest and receive another reply; its count
should remain until that reply is visible. Repeat with the app unfocused and
with an original outside the room's loaded history. Automated fixtures and the
demo send no real Matrix messages.

Validation passed: formatting, workspace Clippy with warnings denied, 76 regular
workspace tests (43 GUI tests), and the locked GUI build. Native demo visual
observations and remaining desktop checks are recorded in the
[design preview guide](design-preview.md#local-validation--2026-11-15).

## Connected navigation — 2026-11-15

The visual shell now projects joined spaces and rooms from SDK metadata. The
rail includes All rooms, Direct messages, and every joined space. A space shows
its joined child spaces plus joined conversation rooms reachable through them.
Clicking a space or Direct messages filters navigation without opening a room.
The current room, thread, and drafts stay open even when outside the selected
space. Only an explicit room choice changes the conversation.
Shared rooms can appear in multiple spaces; traversal deduplicates rooms and
handles cycles. Removed/redacted child links are excluded. Unjoined spaces and
rooms are not discovered or joined, so paths through unjoined spaces are not
expanded. Space ordering and room ordering within a section are alphabetical
with room IDs as stable tie-breakers; `m.space.child.order` is not yet applied.

Sections are exclusive, with precedence Favorites → Direct messages → Unread →
Rooms. Favorites keep their unread badge. The Direct messages rail entry includes
all direct rooms, including favorites. The unread badge uses the SDK's local
unread-message count; a dot represents a marked-unread room or SDK unread
notifications without a positive message count. Counts are not a complete thread
unread implementation. Selecting a room does not send receipts or mark it read.

Room updates and notable SDK metadata updates trigger coalesced snapshots; there
is no polling timer and navigation does not open timelines. Names, membership,
favorites, direct status, unread state, and child links update independently of
room-list service lifecycle transitions. Store errors preserve the last good
snapshot and show a navigation error until a later update succeeds. UI selection
uses stable room IDs and clears a conversation/thread when the room is no longer
joined. Space-link or sidebar-filter changes do not close the conversation. Collapsed sections are scoped to each navigation context.

Local SDK/mock-server tests cover nested and shared spaces, child-link removal,
room rename, favorite/direct/unread account data, leaving a room, and metadata
updates without a `/messages` request. GUI tests cover stable space selection
across reordering and clearing removed rooms. Live-account acceptance remains
pending: compare spaces and sections with the other client, rename/tag/change
unread state there, remove a test child link, and confirm navigation updates
without selecting each affected room. Use dedicated test rooms for mutations.

The [design preview](design-preview.md) still runs with `--demo`, now through the
same navigation projection as the connected app.

## Navigation restoration

Connected mode saves the selected navigation context, selected room, and collapsed
sections to `navigation.json` inside the locked account profile. The versioned
file contains IDs and UI state, not credentials or message text. One owned writer
coalesces changes, performs filesystem work off the UI thread, uses a private
atomic replacement file, and flushes the latest state during graceful shutdown.
There is no debounce/polling timer. A failure is shown in the navigation sidebar;
future navigation changes retry the write. A malformed/unknown-version file falls
back to the default view and is replaced only after preferences actually change.

Restoration waits for the saved scope and room to exist in SDK metadata; the
room does not have to belong to the saved sidebar scope. Until
then, the ordinary navigation remains usable; any explicit space/room/section
choice cancels pending restoration. Unavailable saved selections therefore cannot
force a room open or overwrite a user's new choice. Threads, drafts, searches,
scroll offsets, and arbitrary section order are not restored across process restarts. Section ordering is
currently fixed. Opening the restored room still does not send receipts.
Demo mode neither reads nor writes these preferences.

Local tests cover file round-trip, file permissions, malformed/unknown versions,
latest-value flushing on shutdown, late metadata arrival, and explicit navigation
overriding an unavailable saved room. Live check: choose a space and room,
collapse a section, close normally, restart, and verify the same context returns
without marking the room read in the other client.

## Timeline scrolling — 2026-11-15

A room or thread opened for the first time starts at the bottom. Returning to a
previously opened conversation restores its own in-memory scroll position after
initial history loading completes. Room and thread positions are independent.
Each pane shows a floating down-chevron centered near the bottom of its message
area only while scrolled away from the bottom. Its tooltip is **Jump to newest
message**; clicking returns that pane to the bottom and hides the button. No read receipt is sent by the
button. Scroll positions are not written to the profile or restored after exit.

Iced's bottom anchor handles messages arriving after the initial empty view.
A small widget wrapper gives scroll state the lifetime of the selected view;
remembered positions are then restored explicitly. History status occupies a
stable slot inside the scrolling content; changing loading/ready/end states
preserves the visible message anchor. Scroll notifications and jump actions carry the view generation,
so queued events from a previous room cannot change the current position. There
is no scroll polling or animation timer.

While a pane remains open, incoming messages follow only if it was already at
the bottom (with a one-pixel rounding tolerance). When scrolled up, the viewport
anchors the first visible message ID and its position relative to the viewport.
New arrivals, prepended history, and changed row heights above that message do
not move it. If it disappears, the nearest surviving successor, then predecessor,
is used; if no IDs survive, the old top pixel offset is retained within bounds.
Clicking the chevron resumes following. The same logic applies to room and thread
panes and adds no timer or periodic task.

Positions remembered across room/thread closures are still pixel distances from
the bottom, rather than saved message-ID anchors. History changes while a pane is
closed can therefore affect restoration. Positions are not persisted across
process restarts; timeline virtualization remains future work.

Validation: all 16 GUI tests, locked GUI build, formatting, and Clippy with
warnings denied passed. Tests exercise real Iced layout through empty-to-loaded
history, loading-control changes, and view replacement, plus independent saved
positions, delayed restoration, and stale-event rejection. A native Wayland demo
check confirmed first-open bottom position, scrolling up, switching to another
room and back, position restoration, and the chevron returning to the latest
message. The demo closed normally (exit 0). Local screenshots and the smoke
script are in `artifacts/scroll-2026-11-15/` (ignored). This desktop check did not
send messages or restart the user's connected app.

## Interactive acceptance

1. Restore the existing profile. Confirm the room list appears without browser
   login. Select a dedicated test room. Repeat these checks for encrypted and
   unencrypted rooms; both should open with a working timeline and composer.
2. Confirm existing messages load without sending a new message. Then send
   messages from the second client and verify arrival, ordering, sender, and
   decryption in the room timeline. Scroll upward to request additional history;
   there is no **Load older** link. The app keeps three viewport heights of older
   content buffered, with a 1,200-pixel minimum, and continues through pages that
   contain only thread replies, coalescing them into recent activity. At the start it shows **Beginning of
   conversation**. Failures preserve loaded messages and offer **Retry loading
   history** at the top of the loaded history.
   Missing keys should appear as decryption
   failures rather than a login failure.
3. Use a message's reply arrow or existing reply-count link to open its thread.
   Confirm the original appears once beneath the header, with its author and
   timestamp, and stays visible while replies scroll. Long originals scroll
   independently in a bounded area. Then send another reply from the second client.
4. Close the thread. Send another reply from the second client and reopen it.
   Confirm the reply appears. Room synchronization should remain active.
5. Rapidly switch rooms and threads, including switching while a timeline is
   loading. Old snapshots must not appear under a new selection. Repeatedly
   selecting a room is also a way to retry a failed timeline load.
6. Close the window. It should wait for SDK sync to stop, then exit. A shutdown
   error stays visible; close again or use **Exit** to dismiss it. Restart and
   confirm the same session still works. Navigation restores as described above; threads and drafts remain transient.

For presentation-lifetime traces (without SDK event logging):

```sh
RUST_LOG=interconnect=debug cargo run --locked --bin interconnect
```

Each started presentation view should stop when replaced or closed. Closing a
thread should stop only its view, leaving the room view running. A room switch
stops both old views. Trace entries do not contain room IDs or message bodies.
No recurring application timer is installed. Verify settled/hidden-window
behavior and scheduler wakeups using [the measurement procedure](milestone-0.md#early-cpu-and-wakeup-measurements);
absence of app timers is not evidence of zero SDK/network wakeups or redraws.

## Implementation and evidence — 2026-11-14

- Iced's Tokio executor runs restoration, the single `SyncService`, and view
  futures. Room and thread futures own their timeline subscriptions and are
  dropped independently on navigation. No detached presentation tasks are spawned.
- Desired navigation and UI snapshots use latest-value watch channels. The Iced
  output channel has capacity one. Snapshots contain SDK-ordered timeline items;
  raw timeline diffs are never reordered or coalesced by the app. The SDK remains
  responsible for applying diffs. Generation checks reject queued stale panes.
- A slow UI cannot block the backend on snapshot delivery. Snapshot delivery is
  independent of sync stop and view cancellation. This transport is for replaceable
  state only; future send commands/results will need separate delivery semantics.
- Persistent client construction retains the exclusive profile lock until process
  exit, covering late GUI handles and runtime cleanup, and shares
  the CLI's local device-key check. The desktop relies on ongoing authenticated
  sync for token rejection; see cached startup above. SDK stop has a 15-second bound.
  The GUI owns one Tokio runtime and lends its handle to Iced. After UI teardown,
  runtime cleanup has a five-second bound and reports incomplete cleanup on timeout.
  In-progress queue admission still has no total deadline.
- The local mock-server integration test uses actual SDK room/thread timelines,
  checks repeated navigation and thread closure, and shuts down without draining
  UI snapshots. Its room has encryption state but plaintext fixture events: this
  test proves integration, not cryptography or live-server encrypted delivery.
- UI tests cover stale snapshot rejection and clearing the thread on room change.
  Existing CLI persistence, verification, and shutdown tests remain applicable.

Local results: `cargo build --locked --bins`, formatting, Clippy with warnings
denied, and all 17 tests passed. A five-second native Wayland smoke run with an
intentionally unavailable Secret Service remained running without logged errors;
the timeout harness then terminated it (exit 124). No real profile or live account
was opened. This checks startup, not visual correctness or graceful closing.

Live two-client GUI acceptance, visual correctness, keyboard/focus behavior,
hidden-window behavior, and scheduler measurements remain **pending**. No complete
product feature or milestone is claimed by these implementation checks. The room
list is a basic joined-room list refreshed on room-list service state changes;
spaces, unread metadata, full incremental room-list presentation, cache budgets,
and viewport virtualization remain later work.


### Initial history correction — 2026-11-14

The user reported that the host CLI could load an encrypted room's messages,
while the GUI initially showed none; newly sent messages did appear in the GUI.
The prototype had relied on live sync/cache contents and never requested room
history. It now issues an initial SDK back-pagination request when opening a room.
Live snapshots remain active during the fetch. A one-shot 30-second deadline
bounds history loading; failures remain visible alongside live messages. Selecting
the room again retries the load. Navigation and shutdown cancel the view-owned
history future.

The mock-server regression supplies an empty sync timeline with older messages
available only through `/messages`. It failed before the fix and passes after it.
A second regression checks shutdown while initial history is pending. Live-server
confirmation of this correction remains pending.


### Manual room pagination — 2026-11-14 (superseded)

The user confirmed that repeated CLI pagination reveals the pre-login messages.
The GUI now provides **Load older**, using a bounded, view-owned request channel
and SDK back-pagination on the existing timeline. Live updates continue during a
fetch. Switching rooms or closing the app cancels the pending view operation.
Only the SDK's reached-start result ends pagination; a page without new visible
rows does not disable further requests. Errors preserve loaded messages and allow
retry. Thread pagination controls remain outside this change.

The mock-server integration covers an initial history page, a hidden-thread-reply
page, and an older visible message on a subsequent page, followed by the end of
history. UI tests reject stale-room clicks and immediate duplicate requests.

### Automatic history loading — 2026-11-15

Rooms start with a 50-event page. After layout, the UI measures the actual
message viewport and loads more until the visible area plus three viewport
heights of older messages is available (the buffer is at least 1,200 pixels).
Scrolling toward the top, resizing, and new page results replenish this buffer.
The buffer is a prefetch target, not a limit on retained/rendered messages.
Only the SDK's reached-start result ends pagination; hidden-only pages continue.

Threads eagerly paginate to the start, stopping at approximately 500 events or
ten pages before switching to the same viewport-driven loading. Each page asks
for 50 events; server-side page sizes and non-message events can affect counts.
This bounds initial work for long threads. The pinned original is excluded from
reply measurements and appears only once. Saved scroll positions wait for enough
history and buffer before restoration, or restore within available content after
reaching the start or a fetch error.

Viewport reports are emitted only when geometry or pagination state changes.
There is no polling, animation, or idle retry timer. Generation and page-revision
checks reject stale/duplicate requests; the view owns the bounded request channel
and fetch future. Live updates continue during each page, each fetch retains its
30-second timeout, and navigation/shutdown cancel pending view work. Failures
stop automatic requests until explicit retry, preserving loaded content. Existing
message-ID anchoring keeps the reading position steady while history is prepended.

Validation passed: 59 workspace tests (32 GUI tests), formatting, workspace
Clippy with warnings denied, and the locked GUI build. One existing 100-cycle
stress test remains opt-in. Real-widget tests cover changed viewport/page reports
without timer scheduling, buffer fill/scroll/resize decisions, delayed saved
position restoration, failed-fetch retry gates, stale/duplicate requests, and
stable visible anchors through history-status changes. SDK mock-server tests
cover hidden-only room pages, a fully loaded three-page thread, a 600-reply thread
that stops after ten initial pages and completes on demand, and cancellation,
send/retry, reconnection, and shutdown. Native/connected desktop acceptance and
new idle-resource measurements remain pending.


## GUI text sending — 2026-11-14

The user confirmed historical decryption after enabling SDK room-key requests;
see [the key-exchange result](interactive.md#user-run-historical-key-exchange--2026-11-14).

Each pane has its own multiline composer and a target label. Enter inserts a
newline, Ctrl+Enter sends from the focused composer, and **Send** is also available.
IME preedit state suppresses sending; test this with the host input method before
claiming full IME acceptance. Opening panes does not focus the editor automatically.
The stock editor may blink its caret while focused; the earlier empty-window idle
baseline does not describe an actively focused editor.

Drafts are retained separately per room and per thread across navigation and pane
closure, in memory only. They are not saved across process restarts. While the SDK
accepts a submission into its persistent send queue, that draft is temporarily
locked. Queue acceptance clears only that draft; an admission error preserves it.
Subsequent server delivery is shown by the timeline's **Pending**, **Sent**, or
**Failed** state. A failed local echo offers **Retry**, which reuses its existing
SDK transaction and resumes the room's queue (including other waiting messages).
It does not create another message from the text.

Finite Iced tasks own their captured timeline/queue handles until admission or
retry completes, even if the view closes. At most eight such operations may be
outstanding. Their results arrive as individual Iced messages, not replaceable
snapshot updates. Window closure waits for those operations before asking the
session to stop; queued network sends retain the SDK's persistence behavior.
Total shutdown is still subject to the previously documented database/runtime
cleanup limitations.

### Two-client acceptance (partially verified)

1. Send a multiline room message; confirm it appears once in the other client,
   with no thread relationship. Check Pending then Sent locally.
2. Open a known thread, send a reply, and confirm it belongs to that thread in the
   other client. The room composer must still create an ordinary room message.
3. Write different room/thread drafts. Switch rooms and close/reopen the thread;
   confirm each draft returns to its own target.
4. Send and immediately navigate away. Confirm the message keeps its original
   target and the acknowledgement does not clear a different draft.
5. Exercise Enter, Shift+Enter, Ctrl+Enter, and the active IME's composition and
   confirmation keys. No send should occur during composition or in an unfocused
   editor; ordinary editing/paste should work.
6. With connectivity disabled, send a test message and allow the SDK to report a
   failure (it may retry first). Restore connectivity and use Retry. Verify only
   one event is created. Stop the UI while a message is pending, restart, and
   inspect the existing queue before attempting any manual resend.

The local mock test exercises real SDK encryption and room/thread relationship
construction, an HTTP send failure, and retry using the same transaction URL.
It decrypts captured test ciphertext to verify the intended message target.
Live-server delivery, desktop composition behavior, and offline/restart acceptance
remain separate checks.


Local sending checks: all 21 workspace tests passed, including real SDK encrypted
queue/retry checks and draft/shortcut tests. Formatting, locked binary build, and
Clippy with warnings denied passed. This does not replace the pending desktop and
two-client acceptance above.


### User-run GUI sending — 2026-11-14

The user reports that sending messages in both the room and thread composers
works. This is user-reported live evidence for basic GUI room-message and
thread-reply sending, following the earlier confirmations of message loading
and historical decryption. No message contents or room identifiers are recorded.

Multiline formatting, explicit Pending/Sent transitions, duplicate checks,
draft retention, navigation during queue admission, keyboard/IME behavior,
failed-send retry, and offline/restart scenarios were not specifically confirmed.
Those acceptance cases remain pending; this result does not complete the full
messaging milestone or ordinary-reply support.


## Composer teardown correction — 2026-11-14

The user reported an abort when closing the UI after sending. The supplied trace
runs from `App -> Model -> Pane -> Composer -> Timeline -> Client -> SQLite pool`
to `deadpool_sync::SyncWrapper::drop`, which calls Tokio `spawn_blocking` with no
current runtime. A second panic during destructor cleanup aborts the process.
Stopping sync alone did not release the SDK handles retained by UI snapshots.

Composer and retry handles now wrap their SDK-owned values with the originating
Tokio handle. Their final drop enters that runtime context before releasing those
values. This covers the app model, widget/message copies, and discarded snapshots,
without depending on a particular final UI message being processed. It uses the
existing runtime and does not wait for SQLite I/O inside those destructors.

The GUI now owns its one Tokio runtime in `main`; a small Iced executor adapter
borrows its handle. The runtime remains entered on the UI thread until Iced and
its application state have been destroyed, even if Iced drops its executor first.
Afterward the existing five-second runtime-cleanup helper drains blocking work,
reporting a timeout as an error. No second Tokio runtime is created.

Regression tests release the last shared handle on a non-runtime thread and verify
blocking cleanup completes, and release a real SQLite-backed SDK client outside
`block_on` before shutting down the runtime. A GUI-lifecycle regression drops the
Iced executor before a SQLite client and confirms background cleanup completes
before the UI runtime wrapper returns. Existing encrypted send/retry and
navigation checks also remain applicable. This fixes the missing-runtime destructor
path and bounds final runtime cleanup; it does not establish a total deadline for
queue-admission operations before sync shutdown.

Desktop follow-up: open a room and thread, send, then close the window; also close
with a failed-send retry button present. Confirm the process exits without a panic
and that restarting restores the same session. Live confirmation remains pending.


Local teardown checks: all 24 workspace tests, locked binary build, formatting,
and Clippy with warnings denied passed. No live account was opened or modified
for these regressions; the SQLite tests use temporary stores.


## Wi-Fi loss and automatic reconnection — 2026-11-14

The user reported that losing Wi-Fi caused a terminal synchronization error and
forced the UI to exit. The GUI now enables `SyncService::with_offline_mode()`.
The SDK retains ownership of synchronization, probes server availability while
offline, and restarts sync after connectivity returns. No additional app polling
loop or second sync service was introduced.

The GUI displays an offline/reconnecting status and retains timelines, composers,
and drafts. Room-list metadata refreshes no longer overwrite connection status.
The GUI also stops using the finite CLI initial-sync success/failure gate after
session restoration: SDK startup and recovery states are handled in its ongoing
session loop. `UnknownToken` notifications remain a sign-in-required failure,
rather than retrying a revoked session indefinitely.

A message can be queued while offline. If the SDK marks it Failed, use **Retry**
after reconnecting; this resumes the room queue with the existing transaction.
Synchronization recovery does not automatically retry every failed send. Messages
queued behind a failed send may wait for that room queue to be resumed.

Manual check: open an encrypted room, disable Wi-Fi, and wait for the offline
banner. Confirm drafts remain editable and the window stays open. Queue a message,
restore Wi-Fi, and wait for Connected. Retry any failed message, then send a new
message. Confirm each appears once in the other client. Repeat the outage and
close the UI while offline to verify cancellation of the SDK availability checks.

The mock-server regression switches sync and availability endpoints to HTTP 503,
checks that the same view can queue a message, restores the endpoints, retries the
failed send, sends another message, then shuts down during a second outage.
Physical Wi-Fi and live-server confirmation remain pending.

At this stage, starting entirely offline was still blocked by `whoami` during
saved-session restoration. The cached-startup change documented above removes
that gate for the desktop while retaining the local device-key check.


Local recovery checks: all 26 workspace tests, formatting, Clippy with warnings
denied, and locked binary build passed. The HTTP-503 outage/recovery regression
passed; a separate HTTP-401 `M_UNKNOWN_TOKEN` test confirms that revoked sessions
are reported as requiring sign-in rather than entering endless network recovery.

### User-run Wi-Fi recovery — 2026-11-14

The user confirmed that the Wi-Fi loss/reconnection correction works. This is
user-reported live evidence that the UI remains usable and can resume operation
after connectivity returns, without requiring an application restart.

The confirmation does not separately establish failed-send deduplication, shutdown
while offline, repeated suspend/resume, or startup without connectivity. Those
checks retain their existing validation status.


### Lifecycle and idle-resource follow-up — 2026-11-14

See [idle resource measurements](idle-resources.md) for the 100-cycle SDK timeline
ownership check, opt-in diagnostics, and host visible/hidden CPU, memory and
redraw captures. No stale timeline ownership was observed in the fixture. Hidden
windows delivered zero redraws in the sampled intervals; background CPU and
crypto-store flush activity were investigated further. After tracefs permissions
were corrected, two visible/hidden room-list capture pairs measured about 170 client scheduler wakeups/s with zero redraws or UI
updates. Background CPU remained about 3.5–3.6% of one core. Subsequent CPU
stacks attribute 70.8% of samples to crypto lease/lock work;
`sched_waking` stacks show worker handoffs and filesystem/flush completions.
The SDK lease-renewal path is the main optimization target, with profile-guard
lifetime auditing required before a single-process configuration experiment.
See the linked report for methodology and limits.


### Single-process store fix — 2026-11-14

Persistent clients now use SDK `SingleProcess` store mode, protected by the OS
profile lock retained until process exit. Late GUI handles and runtime-cleanup
timeouts cannot release that lock early. Reopening a used profile in the same
process is deliberately unsupported; restart the application. SQLite durability
and existing session/device keys are unchanged.

All 27 regular tests, the 100-cycle navigation stress test, Clippy, formatting,
and GUI build passed. The subprocess regression verifies exclusion through late
SDK handles and cleanup timeout, followed by acquisition after owner exit.
SDK outage/reconnect and encrypted sending fixtures run in the new mode.

The host A/B check reduced initial-thread-set wakeups from approximately 170/s
to 1.6/s, with CPU dropping from 3.6–3.8% to 0.17% of one core. A follow-up
that accounts for transient SQLite worker threads measured about 3 wakeups/s
for the whole client, mostly in sync-response bursts every 30 seconds. Separate three-second
traces found 51 crypto-store flushes before and none after. See
[the measurement report](idle-resources.md#single-process-fix-and-ab-check--2026-11-14)
for capture conditions and limits. No live messages were sent during this check.


### Static composer caret — 2026-11-15

Both room and thread composers now use a steady visible caret while focused.
Iced 0.14.2's text editor hard-codes its 500 ms blink timer, so the pinned
`iced_widget` crate is locally overridden with a small `cursor_blinking(bool)`
option, defaulting to upstream behavior. The composers explicitly disable it.
This suppresses the timer's scheduled redraws and keeps the focused caret drawn;
normal input, selection, and window-focus behavior remain in the widget.
See [the dependency patch notes](../../vendor/iced_widget/PATCH.md).

A real-widget regression delivers redraw events across multiple blink phases:
enabled blinking schedules a future redraw, while disabled blinking requests no
redraw after focus styling settles. All 28 regular tests, GUI build, formatting,
and application Clippy passed. The vendored pick list also removes its legacy `std::f32` import so
`f32::INFINITY` uses the associated constant without a deprecation warning.

Live host check: restored the existing session, selected an encrypted room,
focused its composer, and typed an unsent test draft. The caret was visibly
present, and the next 30 seconds showed zero redraw events and zero UI updates.
The draft was cleared afterward without sending a message. Earlier blinking
behavior produced 63 redraw events in 30 seconds in a focused thread composer;
these are redraw observations, not whole-process scheduler-wakeup counts. Raw
local evidence is in `artifacts/static-caret-2026-11-15/` (ignored).

### Connected-navigation implementation checks — 2026-11-15

All 39 regular workspace tests passed; the existing 100-cycle SDK lifecycle
stress test remains explicitly ignored. The locked GUI build, formatting, Clippy
with warnings denied, and diff whitespace checks passed. After boxing the larger
UI snapshot message, the 13 GUI tests were rerun successfully. A native
Sway/Wayland demo smoke check rendered the shared navigation shell and exited
normally on a window-close request (exit 0). Its local screenshot/log are in
`artifacts/connected-navigation-2026-11-15/` (ignored).

This pass did not open the real saved account or send Matrix messages. The
SDK-backed navigation scenarios used a local mock server. Live space/unread and
restart acceptance, and resource measurements on a representative account,
remain pending. Metadata refresh currently scans the joined-room cache and
stored space links per coalesced update; larger-account costs still need to be
measured before claiming a navigation performance budget.

The chevron placement was refined to a bottom-center overlay. A native demo
check confirmed it appears when scrolled up, survives a room round-trip, and
hides after jumping to the bottom. It does not reserve layout space or move the
message viewport when appearing. All 16 GUI tests, build, formatting, and Clippy
passed. Screenshots are in `artifacts/scroll-chevron-2026-11-15/` (ignored).


### Follow-newest and reading-position validation — 2026-11-15

Real Iced widget-layout tests cover append-at-bottom following, append-while-up
anchoring, explicit jump resuming follow, prepended history, edits above the
visible message, viewport resizing, removal of the anchor message, repeated
stable layouts, and resetting view identity. The wrapper measures row geometry
only during layout; input and explicit scroll operations refresh just the scroll
metrics. No application messages or redraw requests are generated by the
anchoring operation itself. Existing viewport notifications keep the chevron and
session scroll-position cache current.

All 18 GUI tests passed, along with the locked GUI build, formatting, and Clippy
with warnings denied. The native Wayland demo regression confirmed that a
scrolled-up position survives switching rooms, the floating chevron remains
visible there, and clicking it returns to the bottom and hides it. The demo
closed normally (exit 0). Screenshots are in `artifacts/scroll-anchor-2026-11-15/`
(ignored). Incoming-content cases were exercised through widget-layout tests;
this check sent no Matrix messages and did not restart the connected app.

Message text selection (2026-11-15): room and thread bodies support dragging,
word/line selection, keyboard selection, and the platform copy shortcut. Bodies
remain read-only, have no caret or input-method request, and schedule no blink
redraws. Wheel events continue to scroll the conversation. This initially selected within one message; the timeline-wide implementation
below now preserves selection by message ID across layout/slot changes. The widget regression
exercises partial selection/copy, rejected typing/cut/paste, identity reset, and
idle redraw behavior. The demo scroll smoke still passes with selectable bodies.

Message presentation (2026-11-15): sender headers use the SDK profile display
name, falling back to the Matrix ID; ambiguous names include the ID and every
sender header exposes it in a tooltip. Local timestamps have a full date/time
and UTC-offset tooltip, with date separators in the timeline. Consecutive
messages group only for the same sender ID and display name, on the same local
date, with ordered timestamps at most five minutes apart. Unknown timestamps
never group. Grouping retains each message's scroll-anchor ID. Demo data includes
a consecutive-message group. UI regressions cover grouping boundaries and the
existing real-layout scroll-follow/anchor behavior.

Quieter message actions (2026-11-15): the per-message reply link is now a hover
panel with a reply-arrow control at the message's top right. The panel overlays
the message; no right gutter is reserved. Hovering leaves wrapping and row height
unchanged. The tooltip identifies **Reply in thread**.
Existing threads retain a visible reply-count link supplied by the SDK summary.
Successful sends no longer repeat “Sent”; pending/error states and retry remain
visible. The demo has one existing thread and updates its count for local replies.
A real-widget regression selects and copies part of a message inside a scrolled
hover row and opens its reply action. The full workspace suite passed (the
existing 100-cycle lifecycle stress test remains opt-in), as did the demo scroll
smoke. Desktop capture confirmed partial selection; automated desktop clipboard
verification was interrupted by a focus change. Controlled widget tests verify
clipboard contents. No actual-account messages were sent or client restarted.
Hover actions do not yet provide a complete keyboard navigation workflow.

The overlay follow-up passed formatting, workspace Clippy with warnings denied,
all 27 GUI tests, and the locked GUI build. The scrolled-row regression now uses
the production action panel and verifies full-width text, partial selection/copy,
and exactly one thread-open event from its button. Desktop inspection of the
overlay remains pending; the desktop observations above predate this follow-up.


Cross-message selection (2026-11-15): drag in either direction from a message
body to select partial endpoints and complete intermediate messages. Copying
within one message returns only the selected source text. Copying across messages
adds the current speaker name above each selected fragment, with a blank line
between messages:

```text
Alice:
selected end of the first message

Bob:
  selected beginning of the next message
```

Names are plain text, not literal Markdown bold markers: Iced's clipboard API
currently exposes text rather than HTML/rich-text formats. Avatars, dates,
timestamps, status text, reply counts, and actions never enter the clipboard.
Source indentation, blank lines, Unicode, and line endings are preserved; soft
wrapping adds no newlines. Labels flatten line breaks in display names.

Selection belongs to each room/thread timeline and uses message IDs plus logical
text positions. Appending messages, prepending history, and reflow preserve the
selected range. Navigation, an outside click, or a shell focus/navigation shortcut
clears it. Editing/removing an endpoint clears selection instead of retaining
stale text offsets. Keyboard extension across message boundaries is not yet
implemented; ordinary within-message keyboard selection remains available.

Dragging near the top/bottom edge scrolls through already-loaded history. Only
an active edge drag schedules frame deadlines; release, window focus loss, or
reaching the boundary stops them. This does not fetch additional history.
Real-widget regressions exercise forward/backward copy, body-only sources,
partial endpoints, Unicode/whitespace fidelity, stable IDs across append/prepend,
focus/navigation invalidation, and both auto-scroll directions including their
idle boundaries. No new SDK requests or subscriptions are involved.

## Unicode emoji — 2026-11-15

Read-only message/activity text, both composers, and thread excerpts explicitly
select an installed color emoji font for recognized emoji graphemes. The app
prefers Noto Color Emoji on Linux, Apple Color Emoji on macOS, and Segoe UI Emoji
on Windows, trying the other names when available. If none is installed, normal
Iced fallback remains in use. Font coverage still limits supported emoji.
The `emojis` and `unicode-segmentation` dependencies reuse versions already in
the lockfile. Custom Matrix image emoji remain separate work.

The text stays Unicode: modifiers, flags, keycaps, and joined sequences receive
a single font span; explicit VS15 requests are left in text presentation.
Thread-excerpt limits now count graphemes so they cannot cut those sequences.
Ordinary text keeps its existing typography. The small vendored editor API
applies font spans before layout measurements, avoiding a post-layout change in
wrapping, selection coordinates, or timeline scroll metrics. Line-change
tracking caches the formatting work; there are no new timers or subscriptions.

Automated checks cover complete byte ranges, text presentation, missing-font
behavior, clearing spans after edits, color glyph rasterization and single-glyph
shaping for common sequences, stable first-layout height at 75/300 logical pixels,
software-rendered colored pixels, and exact Unicode selection/copy. Font-dependent
raster checks skip with a diagnostic when no supported emoji font is installed;
they ran with Noto Color Emoji in this container. Existing draft, timeline-scroll,
and idle-caret regressions are retained.

Native Wayland demo captures were inspected at 636×1162 with tiny-skia and wgpu
on Intel RKL integrated graphics via Vulkan/Mesa 22.3.6. Faces/hearts, joined
replies, pinned originals, and the populated room composer rendered in color.
The wgpu launch used the documented `NODEVICE_SELECT=1` workaround. Captures and
temporary capture hooks are local validation artifacts, excluded from source.
No connected session was opened or restarted. macOS/Windows rendering, connected
desktop acceptance, interactive drag-selection with emoji, and new idle-resource
measurements remain pending.

## Message reactions — 2026-11-15

Room messages, pinned thread originals, and thread replies display reaction
chips beneath their content. Each chip shows the exact annotation
key and the SDK's unique-sender count; your own reactions use a blue tint.
Badges wrap at narrow widths, support Unicode emoji sequences and ordinary text
keys, and disappear when the last reaction is removed. Clicking a chip toggles
your own reaction; the picker below adds other emoji. Neither adds polling or an
animation subscription.

The pinned original uses current-generation room reactions when its message is
loaded there. The pinned SDK routes live root reactions to the room timeline,
leaving the thread's copy stale. If the original is outside loaded room history,
only reactions present in the SDK thread snapshot can be shown; live root
reaction updates remain limited until that original is loaded into the room.
Thread replies receive live reaction updates directly.

Automated coverage uses a local mock Matrix server for cached reactions when
opening a thread, live additions/removals in both timelines, exact Unicode/text
keys, distinct sender counts, and own-reaction state. Real Iced widget checks
cover room/original/reply placement, room-root updates overriding a stale thread
snapshot, narrow wrapping, read-only clicks, and absence of scheduled redraws.
Formatting, locked workspace Clippy with warnings denied, all 133 regular
workspace tests (58 library and 75 GUI), and the locked GUI build passed. The
existing opt-in lifecycle stress test was not run.
Native desktop acceptance, live-account checks, and reaction-specific resource
measurements remain pending. No connected client was restarted or real messages
sent for this change.

For manual acceptance, run `cargo run --locked --bin interconnect -- --demo`, open
**Studio → Design studio**, and open the last message's thread. Check badges on
the welcome message, thread original, and first reply, including blue own-reaction
tints and color emoji. Resize and hide the sidebar to inspect wrapping. In a
separately authorized connected run, observe another client's reaction additions
and removals on a loaded room message and a thread reply; verify counts update
while drafts and a scrolled reading position remain intact.

## Adding reactions and the emoji picker — 2026-11-15

Hover a message and choose **Add reaction** (the smiley-plus beside **Reply in
thread**). Thread replies and the pinned original have the same add control.
The picker searches emoji names, shortcodes (with or without surrounding
colons), and exact glyphs, including skin-tone sequences. Categories and paged
results keep the widget grid bounded to 48 choices at a time. Enter chooses the
first visible result. Escape, ×, or a click outside dismisses the popup; backdrop
scrolling and conversation shortcuts are captured while it is open. Resizing
to a layout that hides its room anchor dismisses it too.

Picking an existing own reaction leaves it in place. Connected sends capture
their room/event and originating SDK timeline, run asynchronously, and wait for
the homeserver response (up to 30 seconds). The picker disables selection while
sending, then closes on success; SDK sync provides the badge. Errors stay in the
picker with retry guidance. Dismissing the picker lets an admitted send finish;
stale results cannot change another picker, and closing the app waits for the
bounded request before stopping sync. Removed/redacted targets, activity rows,
undecrypted messages, and local echoes without an event ID cannot receive a new
reaction. Pending sends are bounded by the existing operation limit. Picker
selections stay add-only; chips toggle as described below. Custom emoji remains
future work.

The picker and anchored popup are reusable generic components, separate from
the Matrix reaction adapter. See [development notes](../development.md#emoji-picker)
for the catalog/selection boundary and future custom-emoji extension points.
Demo selections update in-memory room/thread copies and never access accounts
or the network.

Automated checks cover catalog search/categories/paging, independent catalog
keys, mouse/Enter selection, busy controls, static-caret idle behavior, popup
clamping beyond a narrow pane, outside-click/scroll capture, actual hover-button
selection, demo add-once behavior, draft preservation, IME suppression, target
invalidation, stale events/results, failure recovery, and shutdown with a pending
send. The mock Matrix fixture checks `m.annotation` requests to a thread reply
and original in an encrypted room, add-only handling of existing reactions,
server-denied sends, and invalid targets/keys.

Formatting, locked workspace Clippy with warnings denied, all 58 regular library
tests and 84 GUI tests, and the locked GUI build passed. The final GUI run also
checks that a popup Escape produces no second global shortcut that closes its
thread. The existing opt-in lifecycle stress test was not run.

A software-rendered demo widget capture at 360×640 was inspected for picker
layout, layering, and color emoji. It is a widget raster check, not native
desktop acceptance. Capture hooks were removed; the PNG remains under ignored
`artifacts/reaction-picker/`. Native pointer/keyboard/IME acceptance, connected
account checks, macOS, and new resource measurements remain pending.

Manual checks: in `--demo`, add an emoji to a room message, thread original,
and reply; search `:rocket:` and paste `👍🏽`, browse categories/pages, then
choose the same reaction twice. Verify the count increases once and drafts and
the reading position stay intact. At the minimum window size, check all popup
controls, outside-click dismissal, Enter, and Escape. In a separately authorized
connected run, check successful delivery and a denied/offline request without
switching the target or clearing drafts.

## Reaction chip toggling — 2026-11-15

Click a reaction chip to add your own reaction when it is unselected, or remove
your reaction when it is blue/selected. This works in rooms, on pinned originals,
and on thread replies, including exact Unicode sequences and text annotation
keys. Removing the final sender's reaction removes the chip; other senders'
reactions remain. Picker selections still only add.

Chip requests share the picker's operation bound and shutdown accounting.
The same room/event/key is disabled across both panes while a request runs.
Each click captures its intended selected state, so replaying an old remove
does not add a reaction back. Removal looks up cached annotation events and
redacts only matching events sent by the signed-in user, including duplicate
own annotations. The entire asynchronous removal is bounded to 30 seconds;
success updates arrive through SDK sync. There are no optimistic count changes
to roll back on a failure or timeout. Errors appear beside that message's chips;
clicking again retries. Navigation invalidates old errors and click targets,
while admitted requests finish against their captured conversation.

Automated widget clicks exercise room/original/reply toggles, own highlighting,
counts, final-chip disappearance, replayed clicks, and preservation of drafts,
conversation selection, and saved scroll offsets. State checks cover pending-click rejection, retry errors, stale
completions, and shutdown. The mock Matrix fixture checks exact own-event
redactions (including duplicates and text keys), preservation of other senders,
denied removal without a count change, and sync updates in both timelines.
The prior SDK limit for live root reactions outside loaded room history still
applies. Native desktop and connected-account acceptance remain pending.

Formatting, locked workspace Clippy with warnings denied, all 144 regular
workspace tests (58 library and 86 GUI), and the locked GUI build passed. The
existing opt-in lifecycle stress test was not run.

Manual acceptance in `--demo`: click selected/unselected chips in Design studio
and its thread. Repeat clicks on shared chips and remove the single own thumbs-up
on the pinned original. Confirm counts and blue state, disappearance of an empty
chip, and intact drafts and reading position. In a separately authorized
connected run, add/remove a shared reaction and verify another sender remains;
check a denied/offline request and retry. No real messages or reactions were
sent and no connected client was restarted during this change.

## Immediate picker opening — 2026-11-15

The thread original's timestamp tooltip overlapped the Add reaction button.
Iced's hover wrapper preferred that body tooltip over the picker in its action
panel, hiding the picker until the pointer left the timestamp. The popup host
now surrounds the whole hover panel and gives the picker priority while it is
open. An inner-container anchor keeps it positioned beside the action button,
including scroll translation, without changing message layout or adding timers.

A regression on the actual thread pane hovers/redraws the pinned original,
clicks Add reaction, then checks that picker choices exist immediately with no
mouse movement or synthetic leave event. It failed before the fix and passes
afterward. A separate widget check verifies the inner anchor and scroll offset.
Native desktop confirmation remains pending. No connected client was restarted
or real Matrix events sent.

Validation passed: formatting, locked workspace Clippy with warnings denied,
all 88 GUI tests, and the locked GUI build. Backend behavior was unchanged;
the library suite and opt-in lifecycle stress test were not repeated.

## Composer emoji insertion — 2026-11-15

Room and thread composers have a muted smiley button immediately left of Send,
with an **Insert emoji** tooltip. It opens the same searchable/category-based
picker used by reactions, anchored above the button when space below is short.
Picking an emoji inserts the full Unicode sequence at the saved caret or
replaces the saved selection, then restores focus to that composer. Enter in
picker search selects the first result; it never sends the draft, including
when Enter-to-send is enabled. Escape, ×, or an outside click cancels without
changing the draft or selection.

The adapter captures the draft key and view generation. Background editing,
sending, and navigation shortcuts are blocked while the popup is open; stale
events cannot insert into another conversation. A draft awaiting send, settings, the
image viewer, a reaction picker, and active IME composition cannot open the
composer picker. A removed/failed pane, shutdown, or a responsive layout hiding
its room composer invalidates it. Selection during IME composition is ignored.
The existing static caret and event-driven popup add no polling or animation
subscription. Demo insertion stays entirely in memory and sends no events.

Automated checks cover complete emoji sequences, caret insertion, selection
replacement, independent room/thread drafts, cancellation retaining selection,
stale/blocked input, hidden/missing panes, picker exclusivity, actual button
placement/clicks at a 300×480 pane size, and mouse/Enter insertion with
Enter-to-send enabled. Existing picker, scrolling, and caret regressions remain.
Software-rendered room/thread captures at 300×480 were inspected for the muted
button and action-row spacing. Temporary capture hooks were removed; the PNGs
are ignored under `artifacts/composer-picker/`. Native desktop/IME and
connected-account acceptance remain pending.

Formatting, locked workspace Clippy with warnings denied, all 92 GUI tests, and
the locked GUI build passed. Backend behavior was unchanged; the library suite
and opt-in lifecycle stress test were not repeated.

Manual acceptance in `--demo`: open both room and thread drafts, position a
caret in the middle of text or select a word, then use the smiley to insert an
emoji. Search a name or shortcode and choose by mouse and Enter. Check that
only the intended draft changes, typing resumes after the inserted emoji, and
the conversation's reading position stays intact. Cancel a selection, resize
with a picker open, and repeat with Enter-to-send enabled; choosing an emoji
must not send. No connected client was restarted or real Matrix events sent.

## Inline emoji completion — 2026-11-15

Room and thread composers display up to six shortcode suggestions after `:`
and two ASCII shortcode characters at the caret. The token must start a line
or follow whitespace/an opening bracket or quote; URLs, room IDs, escaped
colons, and times do not activate it. Letters, digits, underscores, `+`, and `-`
are accepted, with case-insensitive matching. Exact shortcode matches rank
first, then prefixes, then substring matches. Each emoji appears once.

Continue typing or deleting to update the list without leaving the composer.
Up/Down changes the highlighted result; plain Enter or Tab accepts it, and a
click chooses a visible row. Acceptance replaces the whole shortcode token,
including its suffix and optional closing colon when editing in the middle.
Complete Unicode sequences are inserted and typing resumes after the emoji.
Plain Enter accepts before Enter-to-send; explicit send shortcuts retain their
ordinary behavior. Typing a closing colon alone leaves the literal shortcode
unchanged rather than automatically expanding it.

Escape dismisses suggestions without deleting the typed text or closing a
thread. The same token stays dismissed until text/caret changes. Outside
clicks dismiss and still reach their target, so navigation/editing needs one
click. No matches, a selection, focus loss, navigation, IME composition,
pending draft sends, hidden/removed panes, or another picker/dialog hide the
list. Stale results cannot replace a changed token or another conversation's
draft. Existing drafts, message scroll behavior, and the full emoji picker are
preserved. Updates are event-driven, with no timer or SDK operation.

Automated checks cover trigger boundaries and threshold, prefix/exact ranking,
catalog keys independent of display text, complete-token replacement, Unicode,
room/thread isolation, stale results, dismissal, IME, focus loss, missing panes,
and modal exclusion. Real composer widgets exercise continued typing through
an open popup, Up/Down, Enter and Tab with Enter-to-send, typing after insertion,
Escape without a second global action, mouse selection, narrow layout, and
absence of scheduled redraws. Popup checks verify input/IME/outside-click
forwarding. A 500×640 software-rendered demo capture was inspected for the
list/header/highlight above the composer; temporary capture hooks were removed
and the PNG stays ignored under `artifacts/emoji-completion/`.

Validation passed: formatting, locked workspace Clippy with warnings denied,
all 100 GUI tests, and the locked GUI build. Backend behavior was unchanged;
the library suite and opt-in lifecycle stress test were not repeated.

Native desktop/IME and connected-account acceptance remain pending. Manual
acceptance in `--demo`: type `:se` in both drafts, continue narrowing it, choose
with mouse/Enter/Tab, and dismiss with Escape. Try editing `:rocket:` in the
middle, typing times/URLs, and changing rooms/spaces or opening the full picker.
Verify Enter never sends while accepting a suggestion and subsequent typing
stays in the intended draft. No connected client was restarted or real Matrix
events sent.

## Room topics — 2026-11-15

Room headers show the synced topic in a single line, with the full topic in a
hover tooltip. Empty topics show **No topic**. A pencil opens an inline multiline
editor when the current user's SDK power levels permit `m.room.topic` state
changes. **Save** can also clear the topic; **Cancel** or Escape discards edits.
The thread subtitle is unchanged.

Topic and permission changes use the existing room metadata subscription. Missing
cached permission data disables editing without hiding the room. Saving uses a
captured, runtime-owned room target, checks current membership and permissions,
and has a 30-second timeout. Errors retain the editor text. Duplicate saves are
disabled while pending; room changes or lost permissions dismiss the editor, and
late results cannot change a newer editor. Closing waits for pending operations
through the existing shutdown path. The editor uses a static caret.

Automated coverage includes SDK topic/permission updates, an actual state PUT to
a local mock server, refusal after permission revocation, demo save/clear/cancel,
read-only rooms, stale completions, draft/scroll preservation, and a long topic's
edit button fitting inside a 240-pixel header. Demo changes are memory-only.

Native visual and connected-account acceptance remain untested. For manual
acceptance, use `cargo run --locked --bin interconnect -- --demo`: edit **Design
studio**, clear its topic, cancel another edit, and check **Release notes** has
no pencil. Check a long and multiline topic at narrow window widths. In an
authorized test room, confirm another client's topic/permission changes update
the header, saving reaches that client, and denied/offline saves retain the
editor text. Do not change a real room topic without authorization.

## Pinned messages — 2026-11-15

Rooms with pins show a strip directly below the topic and above the message
history, without a section title or count. Each message has a pin icon on its
left; with pinning permission it is an **Unpin message** button, otherwise it
remains a static icon. The icon is followed by author, timestamp (with the existing full-time tooltip), and
message text, including multiline text and emoji. Media
uses the existing textual event description in this first display-only pass.
The section has its own scroll area capped at 144 pixels; it stays above the
conversation as history scrolls. Rooms without pins reserve no space. Thread
panes keep their original-message section and do not duplicate the room's pins.
The message hover panel offers **Pin message** when the user can change the
room's pinned messages, and **Unpin message** for messages already pinned.
Jump, media-opening, and manual-reload controls remain pending.

The selected room owns an asynchronous pins presentation using the SDK's
`TimelineFocus::PinnedEvents`. This loads pins outside the visible history,
uses SDK decryption/edit/redaction handling, and subscribes to changes. Pins
never cause room-history pagination. Empty rooms wait for metadata changes
without constructing an extra timeline. Room changes and shutdown drop the
application's pins setup/subscription future; generation checks reject stale
snapshots. SDK event-cache tasks and cached data retain the SDK's session-owned
lifecycle. No SDK client or timeline handle is placed in a pins UI snapshot.

Loading has a one-shot 30-second deadline, with an unavailable status if entries
are still missing; later SDK updates can still populate them. There is no polling
or animation timer. The SDK currently loads at most 128 pins per room with up to
eight requests in parallel. Missing/inaccessible events show a partial-result
status. SDK 0.19.1 can fail to populate a malformed pin list containing duplicate
event IDs; that also reaches the unavailable status. This change does not patch
the SDK or attempt to rewrite room state.

Automated coverage uses a local Matrix mock server to fetch pinned events absent
from room history, follow additions/removals and redaction, hide the section when
all pins are removed, and reject publication after cancellation. The test makes
no message/state PUTs or history requests. Headless UI tests cover stale
selection generations, preservation of room pins across space/thread changes,
message drafts and saved scroll positions, live timeline reading-position
preservation when pins arrive/disappear, and scrolling long pins within a
bounded strip. Existing workspace tests cover shared message projection.

Native visual and connected-account acceptance, including encrypted pins and
edits made by another client, remain untested. For a network-free visual check,
run `cargo run --locked --bin interconnect -- --demo`: **Design studio** has two
sample pins, including a multiline note; **General** has none. Check narrow
window sizes and scrolling the pin strip independently of the conversation.
For an authorized connected test, use a room with an older pinned event outside
the loaded history and compare the display to another Matrix client. Check live
pin/unpin, edit, redaction, and encrypted-key availability changes there. Do not
modify a real room's pins without authorization.


### Pin and unpin message actions — 2026-11-15

The message hover panel places a pin button beside **Add reaction** and **Reply
in thread**. It also works on thread replies and the thread's original message.
It changes to **Unpin message** (a pin with a minus sign) for already-pinned
messages. Both actions are hidden unless the current SDK power levels allow
`m.room.pinned_events`, and are hidden for local messages without event IDs,
unsupported content that is not already pinned, stopped/closing sessions, and
rooms with a pin write in flight. Redacted pinned messages can still be unpinned. Topic-editing permission does not determine pinning permission.
Permission and pinned-list changes use the existing metadata subscription.

A click captures the room, event, pane, generation, and requested pinned state.
Repeated pin/unpin clicks are idempotent. The UI rejects stale
clicks and modal-background actions, bounds pending operations, and serializes
pin writes per room. The asynchronous operation reads the latest server pin
list, rechecks membership and permissions, adds the event if absent or removes
all matching IDs for unpin, and sends `m.room.pinned_events` only if changed,
with a 30-second deadline. Reading fresh state
preserves earlier local pins even before sync catches up; Matrix's state API
still permits races with simultaneous edits from other clients. Sync supplies
the connected pinned strip. Failures appear by the originating message and
allow retry; late failures cannot move to another conversation. Shutdown waits
for the existing bounded pending-operation path. Demo pins stay in memory.

Automated checks cover the actual hover-button click and placement, permission
hiding/revocation, pin/unpin state and stale targets, thread pin/unpin, per-room
serialization, duplicate completions, error scoping, shutdown, and draft/scroll
preservation. The local Matrix server test checks existing-pin preservation,
consecutive pins before sync, pin/unpin idempotence and preservation of other
pins, and refusal to pin or unpin after permission revocation. The UI test clicks
both buttons and checks the pinned strip. Native visual checks and real-account
pin/unpin writes remain untested.
Use **Design studio** in `--demo` to pin another message and see it appear above
the timeline; **Release notes** has no pin permission and shows no pin button.


The pinned-panel icon reuses the same captured unpin action, per-room operation
limit, asynchronous save, and permission recheck as the timeline hover panel.
It works for pins outside the loaded history by validating against the current
pins snapshot; it waits for a current room send target before becoming active.
While an operation is pending or after shutdown, icons are static. Errors appear
beside the pin, and a failed removal can be retried. Generation checks reject
queued clicks after navigation. No new subscription, timer, or SDK handle is
added to UI snapshots.

Automated panel checks click an icon for a pin absent from room history and
verify only that pin disappears, permission denial renders no unpin button,
repeated clicks are idempotent, pending operations disable the action, scoped
errors remain visible without history, and stale clicks cannot affect the room
left behind. Native/connected-account checks remain untested.

## Sending attachments — 2026-11-15

The paperclip left of the emoji button opens a native file picker in both room
and thread composers. Pick one file (up to 50 MiB), optionally add a caption,
then use the normal send button/shortcut. The staged filename/size and **×**
remove action appear above the composer. Picking another file replaces it;
cancelling preserves it. Drafts, including attachments, stay with each
conversation. Clipboard files/images now use the normal paste shortcut (see the
clipboard section below); local files can also be dropped onto either pane.

Automated coverage includes file validation and missing-file rechecks, image
MIME detection/dimensions, room/thread draft isolation, picker cancellation,
obsolete picker result rejection after navigation/account changes, and draft
retention on queue failure. Local mock homeserver tests inspect outgoing image
captions, filenames, size/dimensions, ordinary room vs thread relationships,
and encrypted media/event requests. These fixtures do not prove interoperability
with a real homeserver or a second client.

Native acceptance (use `--demo` for picker and draft behavior; sending on a real
account requires authorization):

1. Open each composer and confirm the paperclip is immediately left of emoji.
   Pick a PNG/JPEG, a PDF or other file, and a filename long enough to wrap.
   Check filename/size, optional caption, remove, replacement, and cancellation.
2. Stage separate room/thread files and captions. Switch rooms, reopen the
   thread, change spaces, resize to the narrow thread view, and check drafts and
   reading positions. Close/cancel a picker after navigation; obsolete results
   must not attach to another conversation.
3. Pick an oversized file or delete a staged file before sending. Check the
   inline error and that the caption/file stay available for correction.
4. In an authorized encrypted test room, send an image with a caption and a
   generic file both to the room and a thread. Verify decryption, filename,
   caption, image dimensions/preview and thread membership in a second client.
   Check disconnected upload/send retry and that successful queue admission
   clears only the originating draft. Do not manually resend uncertain sends.
5. Check Linux portal opening/cancellation and native macOS dialogs independently.
   Confirm idle/hidden-window behavior using [idle resources](idle-resources.md).

Live homeserver delivery, second-client decryption, macOS dialogs, and resource
measurements have not been exercised for this change.

Validation result: `cargo fmt --all -- --check`, locked workspace Clippy with
warnings denied, `cargo test --locked --workspace` (79 library and 127 GUI tests
passed; one existing lifecycle stress test ignored), and the locked desktop
build passed. A native Wayland/wgpu demo rendered the paperclip in the requested
position and opened the XDG portal's **Attach a file** dialog. The dialog and demo
were closed normally; the demo exited 0. Native selection/send acceptance remains
pending: the host picker could not resolve this container's fixture path, and
virtual-keyboard input was unreliable. The demo screenshot is under ignored
`artifacts/attachments-2026-11-15/`. No saved account was opened, no real messages
were sent, and the user's connected client was not restarted.


### Attachment previews and local echoes — 2026-11-15

The initial sender uploaded originals without thumbnail metadata. For encrypted
images the existing image policy deliberately shows **Open image to load** when
no separate thumbnail exists; a peer could receive the image while this client
had no inline preview. Image sends now generate an optional PNG thumbnail fitting
512×384 on the file worker, with decoder dimensions limited to 4096×4096 and
allocations limited to 128 MiB. Small images are not enlarged. Unsupported,
damaged, and larger images still send without thumbnails. Both original and
thumbnail encryption remain SDK-owned. Existing messages without thumbnails are
unchanged and can be opened explicitly.

Explicit thumbnail sources now use `MediaFormat::File`, including plaintext
local-upload URIs. Requesting a resized server thumbnail bypassed the SDK's cached
thumbnail file and could not serve a queued local source. Generated thumbnails
remain within the existing 2 MiB preview decode budget; no new background timer,
polling, or automatic original download is introduced.

The mock-server attachment tests now run the real room/thread presentation
workers and assert a sent image row reaches each UI snapshot without navigation.
They decode each sent preview from the SDK cache with no media GET, check bounded
thumbnail dimensions and separate original/thumbnail upload URIs, and check that
encrypted uploads do not expose the original bytes. Preparation tests cover the
static thumbnail's bounds, small images, invalid image bytes, and dimension-limit
fallback. The reported first-attempt missing row was not reproduced by these
fixtures; the user reported that the original image reached the peer and that
sending after restarting worked locally. These are user-reported observations;
live acceptance of the new thumbnails still requires an authorized account send.

Preview follow-up validation: formatting, locked workspace Clippy with warnings
denied, the locked workspace suite (80 library and 127 GUI tests passed; one
existing stress test ignored), and the locked desktop build passed. After refining
the protocol fixture to cover an image-only room send as well as a captioned
thread send, both attachment fixtures and Clippy passed again. No connected
client was restarted and no account message was sent during this follow-up.


## Clipboard files and images — 2026-11-15

Ctrl+V (Cmd+V on macOS) in the room/thread composer can stage a copied file or
clipboard image. Images become `clipboard.png` in memory, preserving alpha, and
get the same thumbnail and encrypted send treatment as picked images. One file
can be staged at a time; multiple-file paste shows an error rather than silently
losing files. File lists take priority over image/text alternatives. Text paths
and URLs remain plain text. The existing attachment/caption survives invalid or
unsupported paste data. No message is sent by the paste action itself.

Ordinary text paste still replaces the current selection. Native clipboard
failure falls back to Iced's text clipboard. An empty clipboard is a no-op.
Results are rejected after navigation, account change, shutdown, or IME
composition; stale text results additionally check the original text and cursor.
Typing a caption can continue while media is being read. Clipboard access is
single-flight and only runs on demand; no polling, monitor, animation or
persistent clipboard history was added.

Automated tests use fixture readers for file/image/text priority, multiple-file
and remote-file refusal, text URI preservation, empty clipboard, invalid RGBA and
image limits. A memory-image round trip checks alpha, cloning, metadata and
thumbnail generation. UI tests cover room/thread targeting, caption preservation,
selection replacement, native-provider text fallback, blocked send while reading,
and obsolete results after navigation/account changes or intervening typing.
Real Iced room/thread editor tests also verify the paste key emits exactly one
attachment action and does not insert clipboard text or send synchronously.
Regular tests never access the system clipboard.

The ignored `clipboard::tests::native_wayland_files_images_and_text` fixture is
only for a **private, headless Sway compositor**. It requires
`INTERCONNECT_CLIPBOARD_FIXTURE_RUNTIME` to match a dedicated `XDG_RUNTIME_DIR`
containing `interconnect-clipboard-`, a `WAYLAND_DISPLAY` socket owned by that
compositor, and no `DISPLAY` fallback. It sets synthetic pixels, a temporary file
with spaces and `#` in its name, plain-text URI content, and multiple files, then
reads them through the actual native adapter. Do not run it on a user's display.
The local harness is under ignored `artifacts/clipboard-2026-11-15/`.

Once inside that isolated compositor environment, run:

```sh
INTERCONNECT_CLIPBOARD_FIXTURE_RUNTIME="$XDG_RUNTIME_DIR" \
  cargo test --locked --lib clipboard::tests::native_wayland_files_images_and_text \
  -- --ignored --nocapture
```

Native acceptance still required separately:

1. In `--demo`, copy a PNG/JPEG file from a file manager and paste into each
   composer. Check its name/size, replacement, **×**, caption, and cancellation
   by navigating away while a clipboard owner is slow. Copy a directory or
   several files and verify the inline error preserves the existing draft.
2. Copy screenshot/image pixels, including transparency, and paste. Check
   `clipboard.png`, remove/retry, distinct room/thread drafts and demo send.
   An image above 4096 pixels in either dimension should produce a size error.
3. Paste Unicode/multiline text over a selection, including a literal `file://`
   URL. Check that it inserts once, keeps ordinary editor focus/IME behavior,
   and does not turn the text into a file attachment. Try paste in search and
   Settings fields to check their ordinary text behavior.
4. Check clipboard data-control support on the target Linux compositor, X11
   fallback and native macOS file/image paste independently. Containers must
   expose both the clipboard connection and any copied file's path.
5. With authorization in a dedicated encrypted room/thread, send a pasted image
   and a copied file; verify caption, alpha, preview and decryption with a peer.
   A pasted image must remain sendable after the source clipboard is replaced.

Live account sends, macOS/X11 native paste, file-manager MIME variations, and
resource measurements remain untested for this change.

Validation result: formatting, locked workspace Clippy with warnings denied,
`cargo test --locked --workspace` (83 library and 132 GUI tests passed; the
existing stress test and native clipboard fixture were ignored in the regular
run), and the locked desktop build passed. The native clipboard fixture also
passed when run explicitly on a separate headless Sway compositor, including
native image alpha, file URI decoding, text and empty clipboard, and multiple-file
refusal. The private compositor was terminated after the check. The user's
clipboard and connected client were not changed; no account messages were sent.


## File drops — 2026-11-15

Dropping a local file onto the room or thread pane stages one attachment in that
conversation. A blue outline identifies the hovered destination; multiple-file
hover uses a red outline and drop shows **Drop one file at a time.**, keeping the
existing attachment/caption. Dropping onto the sidebar or a disabled/hidden pane
has no effect. A narrow thread view targets the thread. Nothing is uploaded or
sent until the normal send action; attachment validation and retry behavior are
shared with the picker. Raw image data, remote URLs and virtual/non-file drag
formats are not imported by this path.

GUI tests lay out both real room/thread panes side by side and verify only the
pane under the pointer emits a drop action. Other cases cover multiple-file
rejection, cancellation, outside/disabled targets, narrow hidden-room refusal,
caption preservation, single-flight import, and stale validation after
navigation. Hover is widget-local, adds no application messages, and schedules
no timer. Existing scroll, draft, modal and clipboard tests remain applicable.

Wayland requires the documented [pinned Winit fork](../architecture/winit-file-drops.md):
upstream 0.30.13 emits no file-drop events there. The backend accepts copy-only
local `text/uri-list` offers, reads URI metadata through the existing event loop's
nonblocking FD source, and forwards drag coordinates before file events. It
bounds metadata to 256 KiB/256 paths, rejects remote/query/fragment/NUL URIs, and
cleans up offers/FD sources on leave, completion, replacement, seat removal and
window close. No file content/stat work, worker thread or periodic wakeup is
added to the native event loop. macOS forwards native content-view drag
coordinates as well; that path is not native-host validated.

A private headless Sway compositor, synthetic Wayland virtual pointer and GTK
file-drag source exercise the real native path with `--demo`. The source offers
fixture URI lists (including spaces and `#`) without using the user's clipboard.
The local harness, screenshots and logs are under ignored
`artifacts/drop-2026-11-15/`. Source and demo processes are closed before the
private compositor is terminated. This is independent of saved accounts and
never sends Matrix messages.

Remaining native acceptance:

1. Drop PNG/JPEG and generic files onto both panes, remove/replace them, and
   retain distinct captions while switching rooms/spaces and resizing.
2. Try directories, missing and oversized files, multiple-file drags, leaving
   the target without dropping, and drops while Settings/verification/image
   overlays, a picker, paste import or send are active. Existing drafts must
   survive rejected input. Check reading positions while a chip appears.
3. Check native macOS drag coordinates, cancellation and multiple files, plus
   different Linux file-manager URI offerings and HiDPI output scaling.
4. With explicit authorization in a dedicated room/thread, send a dropped image
   and generic file and verify preview, caption and decryption in a peer.

Live Matrix delivery from drop, native macOS, HiDPI and idle-resource measurements
remain untested for this change.

Validation result: formatting, locked workspace Clippy with warnings denied,
`cargo test --locked --workspace` (83 library and 136 GUI tests passed; the
existing stress and isolated clipboard fixtures ignored), and the locked desktop
build passed. The real Wayland fixture confirmed room PNG staging, a two-file
rejection preserving that attachment, independent PNG staging in the thread,
and generic-file replacement in the visible thread at 640-pixel window width.
Hover outlines and staged filename/size were inspected in the captured demo.
Percent-encoded spaces and `#` decoded correctly. All four native drag gestures
completed, and the demo exited normally (0). No saved account was opened, no
Matrix messages were sent and the user's connected client was not restarted.

### Winit fork source — 2026-11-15

The drop backend now comes directly from the FreakyPenguin Winit fork at pinned
commit `752d80adf83bca01e2992f7e2ff6c7baa78444a4`, on branch
`interconnect/wayland-file-drop-0.30.13`. Cargo fetches it over public HTTPS;
no submodule or `vendor/winit` copy is required. The patch record moved to
[the dependency note](../architecture/winit-file-drops.md), with fork source links.
The fork's runtime sources were compared byte for byte with the tested local
copy before removal. All locked package versions are unchanged, including the
registry `dpi` dependency; only Winit's source changes for this migration.

Formatting, locked workspace Clippy with warnings denied, locked workspace tests
(83 library and 136 GUI passed; the same two fixtures ignored), and the locked
desktop build passed against the fetched Git revision. The pushed branch's
remote SHA was verified. Native Wayland acceptance above exercised the identical
runtime sources before the source migration; no additional desktop/account
session was launched for this dependency-source change.

## Room membership — 2026-11-15

The room header shows a joined-member count. Click the people badge to open
People in the thread sidebar; opening a thread replaces People. At narrow
widths, People occupies the conversation area, matching thread behavior. Close
or Escape returns to the room. Changing spaces keeps the open conversation and
People panel; choosing another room closes the panel. Room and thread drafts
and saved timeline positions remain conversation-owned.

People lists joined users and pending invitations in separate sections, with
names, Matrix IDs, and avatars (or initials). Joined users show the SDK's suggested
Creator/Admin/Moderator role; invitees show Invited. Departed and banned users
are excluded. The header badge counts joined members only. Search matches names
and Matrix IDs without case sensitivity. Lists are paged in groups of 50; avatar
demand is bounded to the current page and uses the existing media cache. Room
invitations are available through the Invite people button described below;
changing roles or removing members remains outside this panel’s scope.

The badge uses the SDK's cached joined-member summary, without fetching a roster
for unopened panels. Opening People loads complete membership asynchronously,
including lazy-loaded members, with a 30-second timeout and a Retry button on
failure. Membership/profile, power-level, creation, and redaction updates refresh
an open panel. No polling or animation timer is added. Closing, navigation, and
shutdown drop the worker future; request generations reject old snapshots.

Automated coverage includes a local Matrix fixture for lazy member loading,
joined/invited filtering, invitation transitions, roles, avatars, departure
updates, and cancellation. UI
state tests cover mutual exclusion with threads, retained drafts/scroll offsets,
space filtering, room changes, Escape, search, pagination, and stale snapshots.
Real-server acceptance and macOS validation remain pending.

Manual acceptance:

1. Run `cargo run --locked --bin interconnect -- --demo`. Click the header badge
   and check five sample members and one pending invitation, roles, Matrix IDs, and initials. Search a name
   and a Matrix ID; clear search and close with both × and Escape.
2. Write separate room/thread drafts and scroll each timeline. Open People over
   the thread, reopen the thread, then close it. Check drafts and scroll offsets.
3. Resize to a narrow window. Open/close People and confirm the room returns.
   Change space filters while People is open, then choose another room.
4. In an independently authorized connected session, compare the badge and list
   with another client. Check joins, departures, profile and power-level changes
   from that client. Test a room larger than 50 members, page/search avatar
   loading, network failure/Retry, and navigation during loading.

Validation passed: locked workspace tests (84 library tests; two existing
opt-in tests ignored), all 139 final GUI tests, workspace Clippy with warnings
denied, formatting, and the locked desktop build. The focused People search
regression verifies no scheduled redraw across caret blink phases. Native
Wayland demo checks covered the badge, People list, Matrix-ID search, a
760-pixel-wide panel, and Escape returning to the room; final outline-icon and
panel screenshots were inspected at 1400 pixels wide. Both demo launches closed
normally. Captures/logs remain ignored under `artifacts/members-2026-11-15/`;
the explicit demo config path remained absent. No saved account or connected
client was opened or restarted. Real-server, macOS, and measured idle-resource
acceptance remain pending.

## Room invitations and directory search — 2026-11-15

People includes an **Invite people** button when the account has permission to
invite. The inline form searches the account's homeserver user directory by name
or Matrix ID. Results show display names, full Matrix IDs, and cached avatars or
initials. Selecting a result fills the recipient; **Send invitation** (or Enter
in the recipient field) explicitly sends. A full `@name:server` can also be typed
or pasted, even when the directory fails or omits that user. Directory search
covers people known to the homeserver, not an exhaustive federated user list.

Search starts after two characters and a 300 ms pause, returns up to 20 ranked,
deduplicated results, and prompts for a more specific query when truncated.
Editing, selection, form closure, panel closure, navigation, loss of permission,
and shutdown cancel obsolete search tasks. Request and form generations reject
late results. Search and invite inputs use static carets; no polling is added.
Directory errors have a Retry search button and leave direct-ID entry available.

Invitations recheck joined-room status and cached SDK power levels before
sending; the homeserver also enforces authorization. Cached joined, invited, or
banned recipients are rejected. Sends serialize per room and participate in
shutdown's pending-operation accounting. Closing or navigating away after Send
does not undo an already-submitted invitation; it finishes against the captured
room, and its result cannot overwrite another form. A send has a 30-second
deadline; timeout feedback warns that the invitation may already have been sent.
Success keeps the joined-member count unchanged until that person actually joins.

Demo mode provides local directory matches for Taylor Brooks, Jordan Lee, and
Jamie Davis. Invitations show simulated success without opening an account,
using the network, or adding a joined member.

Manual acceptance:

1. Run `cargo run --locked --bin interconnect -- --demo`. Open People → Invite
   people. Search `Taylor`, choose the result, then Send invitation. Check the
   success feedback and unchanged member count. Try an unknown full Matrix ID,
   a malformed ID, and an already-joined member.
2. Type quickly, clear the input, close/reopen the form, change rooms, and replace
   People with a thread. Old search results and invite feedback must not appear
   in the new form. Escape first closes the form, then the People panel.
3. At narrow widths, verify result selection, sending, feedback, and closing fit.
4. In a separately authorized connected session, search by a real display name
   and Matrix ID. Send only an intended invitation and verify receipt in the
   other client. Check directory failure/retry, permission revocation, server
   errors, and navigation/shutdown during requests. Real-server invitation,
   macOS, and measured idle-resource acceptance remain pending.

Validation passed: locked workspace tests (86 library tests; two existing opt-in
tests ignored), all 143 final GUI tests, workspace Clippy with warnings denied,
formatting, and the locked desktop build. Local HTTP fixtures check directory
query/limit, rank/profile preservation, duplicate suppression, errors and
cancellation; the room fixture checks the invitation body, invalid/existing
recipients, and permission revocation. UI tests check selected/raw recipients,
stale results, navigation, duplicate submissions, permissions, unchanged drafts
and joined counts, and a focused search with no scheduled caret redraws.
Native Wayland demo checks covered searching Taylor, selecting a result, explicit
simulated sending, raw-ID sending, invalid-ID feedback, Escape returning to the
member list, and a 760×480 requested window. Captures/logs are ignored under
`artifacts/invites-2026-11-15/`. The demo closed normally, its explicit config
path remained absent, and no saved account, connected client, or real invitation
was used.

## Pending room invitations — 2026-11-15

People shows joined members and **Pending invitations** as separate sections,
with a summary of both counts. Pending invitees have names, full Matrix IDs,
avatars/initials, and an Invited label. The room-header badge still counts only
joined members. Name/ID search and 50-person pagination cover both sections;
section labels repeat when a group continues on another page. Pending invites
are visible even when the current account cannot invite people.

The existing cancellable roster loads JOIN and INVITE membership together.
Membership updates move accepted invitations into Members and remove declined,
revoked, or banned invitations. After a successful invite, the UI restarts the
roster worker with a fresh generation, retaining the form's success feedback.
Old snapshots cannot restore the preceding roster. A completion for a different
room cannot refresh or change the currently open room's roster. There is no
polling, new animation timer, or invitation-management action.

Demo mode includes Casey Nguyen as a pending invite in ordinary rooms. Newly
simulated invitations retain their selected profile (or raw ID), appear in
Pending invitations, and survive closing/reopening People or changing rooms
within that demo run. Already-invited people cannot be invited again. The joined
member count stays unchanged.

Manual acceptance:

1. Run `cargo run --locked --bin interconnect -- --demo`. Open People and check
   five members, one pending invite, the separate sections, and a header count
   of five. Search Casey by name and Matrix ID.
2. Invite Taylor through directory search and another user by full Matrix ID.
   Close the invite form. Both should appear as Invited without increasing the
   header count. Close/reopen People, switch rooms and return, and confirm the
   simulated invitations persist. Attempt a duplicate invite.
3. In an independently authorized connected session, inspect an existing pending
   invite and an intended new invitation. From the other client, accept or
   decline it; verify the section and header count follow server membership.
   Check invitations sent/revoked by another room admin and navigation while a
   refreshed roster is loading. Connected acceptance, macOS, and measured idle
   resources remain pending.

Validation passed: locked workspace tests (86 library tests; two existing opt-in
tests ignored), all 145 GUI tests, workspace Clippy with warnings denied,
formatting, and the locked desktop build. The local Matrix fixture covers
lazy-loaded invitations, excluded left/banned users, acceptance, renewed invites,
declines, and cancellation. UI tests cover searching/paging invitees, simulated
invite persistence, duplicate prevention, fresh roster generations, and stale
successful sends after room navigation. Native Wayland demo checks covered the
pending section, search, a new raw-ID invitation appearing after form closure,
persistence after reopening People, the unchanged joined-member badge, and the
760×480 narrow layout. The demo closed normally; its explicit config path stayed
absent. Local captures/logs are ignored under
`artifacts/pending-invites-2026-11-15/`. No real invitation was sent and no saved
account or connected client was opened or restarted.
