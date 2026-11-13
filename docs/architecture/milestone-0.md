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
