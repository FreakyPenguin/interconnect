# Milestone 0: architecture decisions in progress

Status: provisional, 2026-11-13. Compilation is distinct from live-server feasibility.

## Dependencies

Select Iced **0.14.0**, Matrix SDK **0.19.1**, and matrix-sdk-ui **0.19.1**.
Direct UI/SDK versions are exact; Cargo.lock pins the complete application graph.
The SDK requires Rust 1.96 or later. The repository continues to track stable as
specified in development.md; compiler pinning remains a release-build decision.

Iced enables `tokio`, `wayland`, and `tiny-skia`, with defaults disabled. This
provides a small software-rendered feasibility window. Hardware rendering and
representative desktop battery measurements remain milestone 1 work; this is not
a final renderer decision. No X11 fallback is enabled in this spike.
Matrix SDK enables `e2e-encryption`, `sqlite`, `rustls-aws-lc-rs`, and `sso-login` explicitly.
The SSO feature enables the SDK-managed localhost callback server.
The UI SDK has defaults disabled and shares that TLS selection. The default
`automatic-room-key-forwarding` feature is not enabled pending trust-flow design.

The container supplies C/C++, CMake, pkg-config, Wayland, xkbcommon, fonts, and
SQLite build support. TLS uses Rustls/AWS-LC rather than OpenSSL. Existing GPU,
X11, OpenSSL and D-Bus packages are a development superset, not proof that every
package is necessary. A host compositor is needed for the UI; host browser,
D-Bus/secret-service access and callback routing require separate validation.

Sources: [Iced 0.14.0](https://docs.rs/iced/0.14.0/iced/),
[SDK 0.19.1](https://docs.rs/matrix-sdk/0.19.1/matrix_sdk/),
[UI SDK 0.19.1](https://docs.rs/matrix-sdk-ui/0.19.1/matrix_sdk_ui/).
API observations below were checked against the downloaded release sources.

## Runtime and ownership

Use Iced's Tokio executor in the application; do not nest a second Tokio runtime
inside it. The headless executable has its own runtime because it is a separate
process. `runtime-ui` exercises a finite `Task::perform` on Iced's runtime.

The session will own one SDK `SyncService`. Views own timeline subscriptions and
presentation workers. `ViewWorker` is the first isolated lifecycle experiment:
commands and results use bounded channels, cancellation interrupts blocked
sends, explicit close joins completion, and drop requests cancellation. Results
are not silently dropped on channel saturation. The Iced probe opens/closes a synthetic event subscription with a view-owned
worker and rejects stale generation IDs. Full-state coalescing and wiring actual
SDK subscriptions into Iced remain to be implemented and tested.
Never coalesce raw timeline diffs without retaining their ordering/application.

There is no recurring application timer. Optional `RUST_LOG=chatclient=trace`
records worker lifetime and event delivery. SDK tracing can be enabled separately
for investigation, but logs may contain account/event data; do not commit them.
External Linux scheduler tracing measures actual thread wakeups. App trace events
are causal breadcrumbs, not a substitute for scheduler counts.

## Thread feasibility inventory

| Capability | Released API observed | Validation remaining |
| --- | --- | --- |
| Thread loading | `TimelineBuilder::with_focus(TimelineFocus::Thread { root_event_id })` | Encrypted root outside loaded history; compiled helper in `src/lib.rs` |
| History | `Timeline::paginate_backwards` | Overlapping pages, ordering, complete root retention |
| Live updates | `Timeline::subscribe` | Open/closed/reopened views, deduplication |
| Send | `Timeline::send`, `send_reply` | Verify thread relation and encryption in a second client |
| Summaries | `ThreadSummary` | Count/latest activity correctness while thread closed |
| Read state | `send_single_receipt`, `mark_as_read`, own-receipt subscription | Thread vs room semantics; unread totals; no receipt on restore |
| Navigation | `TimelineFocus::Event` and thread focus | Resolve permalink to room/thread/event |
| Restart | SQLite store and SDK session restoration facilities | Secure session input, persisted cache, historical keys |

These are API findings, not passing feasibility checks. In particular, unread
semantics and notification behavior must not be inferred from receipt APIs.
Use the SDK timeline model if the live scenarios pass; only introduce an adapter
for a demonstrated gap. Required thread functionality remains an exit blocker.

## Proposed sections

Within a selected space assign each joined room exactly once: Favorites first,
then Unread, then Other rooms. Favorites keep their unread badge. Initially define
Unread using SDK-provided room unread state, with the precise treatment of manual
unread flags to be validated. Sort each section by normalized display name, with
room ID as a deterministic tie-breaker. Section order and collapse state will be
user-persisted. Shared rooms retain one room state across spaces; direct messages
also remain accessible through a dedicated entry point. Defer manually curated
sections for the initial release. These product rules are proposed, not confirmed.

## Target homeserver

`https://matrix.mpi-sws.org` advertises SSO and native sliding sync; the pinned
SDK confirms discovery. On 2026-11-13, the user confirmed successful browser SSO
callback/login and initial room-list loading through SDK native sliding sync.
An interactive `spike sso` command checks those using a temporary in-memory
session and logs out afterward. It is not the persistent-session design.
The user has an account and a second client for later verification/thread checks.
See the validation guide for the observations and callback forwarding procedure.

## Persistent CLI session design

The `session-login`, `session-resume`, and `session-logout` commands use Linux
Secret Service (`secret-service` 5.1.0 with Tokio/Rust crypto), plus SDK SQLite
storage. This replaces the contemplated passphrase-file approach; no such file
backend was implemented. Container access is opt-in via the host session bus,
as described in development.md.

One keyring item stores a versioned bundle of homeserver, Matrix session tokens,
a random 256-bit SDK store passphrase, and the local device signing-key
fingerprint. Attributes identify the application and canonical profile-directory
path; attributes are not secret. The bundle is never written to a local JSON
file. SDK store encryption is enabled via its passphrase API; this is not a claim
that every SQLite header/metadata byte or every local cache is encrypted.

A private (0700) profile directory contains the SDK store and a process lock.
Concurrent use of the same profile fails. Profile identity currently depends on
its canonical path: keep that path stable across container rebuilds. The default
`.matrix-spike` is ignored by git. Other profile paths must also stay out of git.

Login checks keyring availability and persists the random store key before
opening SSO. It saves the session immediately after login and before sync.
Closing the CLI preserves the device. Resume restores the same SDK store and
session, verifies the local device signing key against the saved fingerprint,
and calls `whoami` to confirm server account/device identity. The sync probe waits for a fresh `RoomListService` transition after a successful
network response; cached room-list availability alone is insufficient.

Network errors retain the saved credentials. There is no silent relogin or
replacement crypto store. This spike does not request refresh tokens or enable
automatic token refresh; expired/revoked sessions are reported and retained for
explicit handling. Durable refresh-token rotation remains a later requirement.
Logout revokes the server session first, then writes a logged-out tombstone and
deletes the keyring item. Network failure preserves credentials for retry. A
failure after the tombstone can be retried for local cleanup. The SDK store is
retained; a later login uses a new profile rather than reusing old crypto state.

There is no transaction spanning SSO/device creation and a keyring write. A crash
in that interval can leave an abandoned device and a reserved profile with no
saved session. The command refuses to overwrite it: remove any abandoned device
using the other client and use a new profile directory. A crash after server
logout but before tombstone persistence can likewise leave a revoked token in
the keyring; recovery for that boundary is not yet automated. Live abrupt-exit,
keyring failure and expired-session scenarios remain unverified.


## CLI shutdown correction

A user-run resume restored credentials and reached cached room-list availability,
but did not exit promptly. The old probe accepted `RoomListLoadingState::Loaded`, which can
come from SQLite before the newly started sync workers are ready, and then
awaited `SyncService::stop()` without a deadline. Inspection of the pinned SDK
shows stop requests use the sync streams' internal channels and await child tasks;
early shutdown and slow crypto-store work are relevant failure paths. The exact
SDK-internal cause of the observed hang has not been established. A short,
metadata-only syscall trace showed ongoing crypto-store WAL flushes.

The probe now requires a fresh `RoomListService` state transition (`SettingUp`,
`Recovering`, or `Running`), published after a successful sync response. SDK stop
has a 15-second deadline and handles Ctrl-C, with progress and completion messages.
The CLI gives Tokio runtime cleanup another five seconds; an overrun is a failure,
not a successful clean shutdown. Blocking database work cannot be safely cancelled
by dropping its future. On timeout the CLI exits with incomplete cleanup reported;
this is a CLI containment measure, not a demonstrated graceful production shutdown.
The saved credentials are retained. No polling loop was added.


The corrected command subsequently completed a live resume against the existing
profile and exited with status 0. The original process had already exited by the
time termination was attempted; no signal was sent. Thus the observed incident
was prolonged shutdown, not a proven permanent deadlock.
