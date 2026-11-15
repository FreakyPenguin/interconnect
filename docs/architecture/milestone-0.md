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
The UI SDK has defaults disabled and shares that TLS selection. The `automatic-room-key-forwarding` feature is explicitly enabled. It controls
both automatic missing-key requests and answering incoming room-key requests
under the SDK trust policy; see the historical-key follow-up below.

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

Use one Tokio runtime in the application; do not nest a second runtime. The GUI
now owns it through final UI teardown and gives Iced an executor adapter borrowing
its handle (see the teardown correction below). The headless executable has its own runtime because it is a separate
process. `runtime-ui` exercises a finite `Task::perform` on Iced's runtime.

The session will own one SDK `SyncService`. Views own timeline subscriptions and
presentation workers. `ViewWorker` is the first isolated lifecycle experiment:
commands and results use bounded channels, cancellation interrupts blocked
sends, explicit close joins completion, and drop requests cancellation. Results
are not silently dropped on channel saturation. The Iced probe opens/closes a synthetic event subscription with a view-owned
worker and rejects stale generation IDs. The separate `matrix-ui` prototype now connects actual SDK subscriptions to Iced
using replaceable full-state snapshots; see the connected UI follow-up below.
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

## Interactive verification and thread harness

`session-live` extends the persistent profile with one ongoing SDK `SyncService`,
a command console, and optional room/thread timeline views. Threading is explicitly
enabled in the SDK client; experimental thread subscription extensions remain off
(the target server did not advertise MSC4306). Explicit selected-room subscriptions
ensure room events are synced independently of the visible room-list range.

Verification uses the released `Device::request_verification_with_methods`,
`VerificationRequest` changes, and SAS changes/confirm/mismatch/cancel APIs. Only
self-verification initiated by a user command is supported. A comparison-specific
confirmation is required; there is no automatic trust grant. Account login,
SDK device trust and historical decryption remain separate outputs.

The console uses a bounded input channel and a dedicated stdin thread owning no
SDK state, so blocked terminal input does not hold Tokio's shutdown open. Views
own their timeline display tasks, which abort on drop. Each stream notification
renders the SDK's current timeline snapshot; it does not apply or reorder raw
timeline diffs. No polling timer is used. SDK cache budgets, full event navigation,
ordinary reply controls, receipts, edits, reactions and redactions remain future
harness/product work. See [interactive validation](../validation/interactive.md).


On 2026-11-13 the user confirmed successful device verification and encrypted
room/thread sends and receives with the second client. This establishes basic
SDK thread-messaging feasibility. The user subsequently confirmed multi-page
thread pagination with ordering/duplicate checks, closed-view updates on reopen,
and historical message decryption plus new replies after restart. Use SDK
thread-focused timelines for the initial implementation. Read-state/navigation
gaps and unloaded-root edge cases still need assessment. Exact SDK cross-signing status
was not reported. See the interactive validation record for the evidence scope.


## Connected Iced follow-up — 2026-11-14

`matrix-ui` restores the existing profile with a lifetime-owned profile lock and
runs one `SyncService` on Iced's Tokio executor. The backend owns independently
cancellable room/thread futures. Dropping a view future releases its timeline
subscription; no separate presentation task needs an asynchronous join.
Room selection manages explicit SDK room subscriptions. Closing a thread leaves
the room view and account synchronization running.

A latest-value channel carries desired navigation, another carries complete UI
snapshots, and a capacity-one channel forwards snapshots into Iced. Generation
IDs prevent queued old pane snapshots from appearing after navigation. The SDK
applies timeline diffs; the app reads ordered snapshots and coalesces only those.
This is not a command-result transport for future sending workflows.

A mock-server integration test exercises actual SDK room/thread views and repeated
navigation. Desktop encrypted delivery and idle/lifecycle measurements remain
pending. See [the connected UI procedure](../validation/connected-ui.md) for scope,
shutdown limitations, and acceptance steps. The original `runtime-ui` probe remains
available as the synthetic comparison baseline.


## Historical key requests — 2026-11-14

After pagination exposed pre-login events, the new host device could not decrypt
them. Inspection of the pinned 0.19.1 sources and `cargo tree -e features` found
that our `default-features = false` configuration omitted
`automatic-room-key-forwarding`. Despite its name this feature also gates the
creation of outgoing requests after missing-key/unknown-message-index errors.
The feature is now enabled explicitly for both CLI and GUI.

In `matrix-sdk-crypto/src/machine/mod.rs`, `decrypt_room_event` queues a key request
on those failures only with this feature. In `gossiping/machine.rs`,
`should_request_key` requires at least one verified own device and suppresses
requests already recorded for the session. Requests target our own account's
devices. `receive_supported_keys` accepts a forwarded key only for a matching
request and `should_accept_forward` requires a verified device on our account.
The SDK event-cache redecryptor observes received keys and retries loaded events.
No custom crypto, polling, automatic verification, or trust bypass was added.

The same feature enables outgoing key forwarding: the SDK can share its earliest
known session with a verified own device. For other users' devices, its policy
requires that the outbound session was originally shared with that device and
restricts the shared message index accordingly. This is not unrestricted key
sharing. See `GossipMachine::should_share_key` in the pinned dependency.

A second, distinct route is secret sharing after verification: the peer can send
the backup recovery secret via `m.secret.send`, without user entry of a recovery
key. Our SDK `EncryptionSettings` currently uses the default `Manual` backup
strategy, so automatic backup downloads are not configured. That route is not
changed here. Peer support and successful receipt of historical keys still need
live validation; enabling the feature alone is not evidence of recovery.


## GUI sending — 2026-11-14

Room and thread panes expose captured SDK timeline handles for finite queue
admission operations. Iced `Task` completions carry individual results; these are
not coalesced through the snapshot channel. Drafts are keyed by room and optional
thread root, retained across navigation in memory, and cleared only on successful
queue admission. At most eight admission/retry operations can be outstanding.
Window closure waits for them before stopping sync. A captured handle may briefly
retain its timeline after navigation until the operation finishes.

SDK local echoes supply Pending/Sent/Failed states. Retry calls the original
`SendHandle::unwedge` and enables that room's send queue, preserving the transaction
ID rather than calling send again. A mock test verifies encrypted payloads, room
versus thread relations, and the unchanged transaction URL on retry. Desktop and
live-server acceptance are recorded in the connected UI guide.

Iced's `advanced` feature exposes input-method events for composer preedit tracking.
The multiline editor preserves normal Enter behavior; focused Ctrl+Enter sends only
outside composition. No global send-key subscription is used. Stock editor caret
blinking means focused-editor idle measurements must be recorded separately.


### Runtime context for SDK handles retained by the UI

Iced can drop application state and queued message/widget copies outside its
executor's entered Tokio context. Composer timelines and retry queue handles can
own the final reference to SQLite resources, whose destructors schedule blocking
cleanup. These UI-facing capabilities now use a shared `RuntimeOwned` guard that
enters the originating runtime for final drop. A SQLite-backed regression covers
release outside `block_on`; see the connected UI teardown correction. This replaces
the assumption that stopping sync was sufficient for safe UI-state destruction.


The GUI's `run_ui` wrapper owns the single Tokio runtime through Iced teardown.
Its `UiExecutor` adapter holds only a handle, so Iced dropping the executor cannot
prematurely destroy the runtime. The main thread remains entered until UI-owned
SDK resources are dropped; runtime shutdown then uses the existing five-second
cleanup deadline. This complements the per-capability drop guard for handles
released on other threads. A regression drops the Iced adapter before a real
SQLite-backed client and checks that blocking cleanup completes before return.


### Recoverable synchronization lifecycle — 2026-11-14

The GUI enables the SDK's offline mode and keeps presentation futures alive during
`SyncService::State::Offline`. The SDK owns availability probes and sync restart;
room-list refreshes update navigation metadata without changing connection status.
The GUI handles startup in the same ongoing loop rather than applying the CLI
probe's finite initial-sync deadline/error policy. Account-token rejection is
observed separately through session-change notifications and remains terminal.
Failed send queues retain explicit Retry semantics after connectivity returns.
This lifecycle change did not yet remove the restoration identity check that
blocked offline startup; see the cached-startup follow-up below.


## Single-process store ownership — 2026-11-14

Persistent session clients now use SDK `CrossProcessLockConfig::SingleProcess`.
The shared builder in `session.rs` covers persistent login, CLI resume/live/logout,
and GUI restoration. Capability discovery and ephemeral clients do not share the
persistent store. The pinned `matrix-sdk-common` dependency exposes this SDK
configuration type; no dependency version changed.

Before opening SQLite, the builder retains a duplicate of the already-locked
profile file descriptor in a process-lifetime registry. The session still owns
its profile, but dropping that owner cannot release the last lock descriptor.
This covers GUI timelines/composers, pending operations, errors/cancellation,
and runtime-cleanup timeout paths without relying on every SDK clone carrying an
application guard. The kernel releases the lock at process exit. No periodic
application task or SDK lease write is needed for this exclusive ownership.

This deliberately supports one account per process: once a persistent client
has been constructed (or construction attempted), that profile cannot be
reopened by another session until the process exits. Future in-process account
switching needs an explicit ownership/drain design before relaxing retention.
Other programs opening this store must honor the same profile lock; shared-store
multiprocess access is not supported. SQLite durability settings are unchanged.

A real subprocess regression creates a SQLite client, drops its session/profile
owner while retaining a late SDK clone, and exercises a runtime-cleanup timeout
with blocked work. A competing process cannot acquire the profile in these
phases or before owner exit, and can acquire it after exit. The SDK-backed
reconnect/history/send and navigation fixtures also run in single-process mode.
See [idle-resource validation](../validation/idle-resources.md) for before/after
host measurements.

## Cached desktop startup — 2026-11-15

Keep the SDK as the owner of persistent crypto, room state, event relationships,
and media bytes. `sqlite_store` already configures these stores under the existing
private profile; there is no application message database or store migration.
Device identity and room keys are durable account data and must remain outside
any future cache-eviction operation. Credential and store-key storage stays in
Secret Service.

Desktop restoration now validates the saved local device fingerprint without a
network identity request. The SDK restores room state from SQLite; navigation
reads that state, and room timelines publish cached events while pagination is
pending. The ongoing authenticated sync service handles network recovery and
token rejection. CLI resume/live retain their explicit `whoami` validation.
Profile ownership, cancellation, generation checks, and runtime cleanup are
unchanged; no new background timer is introduced.

The first caching milestone covers locally available room messages and retained
keys across store reopening. Complete history, offline access to uncached thread
roots, and recovery of missing historical keys are separate requirements. Next,
use SDK media storage with bounded, deduplicated asynchronous downloads and a
byte-bounded decoded-image memory cache for avatars, thumbnails, and custom emoji.
Media retention and explicit clearing should use SDK APIs and preserve pending
uploads. Verify safe event-history pruning in the pinned SDK before offering a
message budget; never clear the entire profile to free cached content. Custom
emoji pack metadata needs a separate format adapter while image bytes share the
media path. These media and retention decisions are proposed, not implemented.

See [cached-startup validation](../validation/connected-ui.md#cached-startup--2026-11-15)
for local evidence and remaining desktop checks.
