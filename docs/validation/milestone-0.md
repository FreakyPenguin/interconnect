# Milestone 0 validation

## Local checks

```sh
cargo build --locked --bins
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo run --locked --bin spike -- lifecycle
cargo run --locked --bin spike -- probe https://matrix.mpi-sws.org
cargo run --locked --bin runtime-ui
```

The probe performs unauthenticated login and native sliding-sync discovery. It
uses the SDK’s `DiscoverNative` builder, which fails if native support is not
advertised. It does not authenticate,
verify SSO, or prove native sliding sync. The UI button checks that Iced enters
Tokio. Open/close the subscription repeatedly: each opening should deliver one
event, and closing should log a worker stop with tracing enabled. A successful
headless build does not verify Wayland rendering.

## Early CPU and wakeup measurements

Build first. Run `target/debug/spike idle 120` in one terminal. Wait for startup to
settle, then use its printed PID in another terminal:

```sh
mkdir -p artifacts
python3 scripts/measure-idle.py PID --seconds 30 > artifacts/runtime-idle.json
```

Run on a native Linux host with kernel-matched `perf` tooling and permission to
access scheduler tracepoints. The container may not expose tracefs or permit
system-wide tracing. Configure host tracing access deliberately; the script does
not elevate privileges or change kernel settings. An unavailable counter is a
failed/unmeasured capture, never zero. Explicit `--cpu-only` permits a limited
procfs CPU-time check where tracing is unavailable.

The script counts `sched_wakeup` and `sched_wakeup_new` for target thread IDs via
system-wide tracepoint filters. `perf -p PID` alone would filter the waking task,
not the target of the wakeup. Use a settled process with a stable thread set; the
script rejects changed endpoint thread sets, but cannot detect transient threads.
Task churn needs a separate full scheduler trace with fork/exit tracking. These
counts are not CPU hardware idle exits, and context switches are not wakeups.

Record machine/CPU, OS/kernel, compiler, commit or worktree state, build profile,
rendering backend, scenario, duration, logging configuration, and tool versions.
Compare multiple equal-duration windows with tracing disabled in the app. Repeat
with application tracing only when investigating a change; tracing has overhead.
For milestone 1 extend this to visible/hidden UI and connected idle. Do not infer
battery budgets from container or software-rendered results.

The runtime idle executable uses one end-of-run deadline and Ctrl-C handling. It
has no measurement polling timer. Lifecycle tracing can be inspected with:

```sh
RUST_LOG=chatclient=trace target/debug/spike lifecycle
```

## Live-server gate (pending)

Required inputs: target homeserver URL, dedicated account, second Matrix client,
and a host desktop for browser SSO/device verification. Do not put credentials,
session exports, message contents, or private room identifiers in this repository.

1. Discover advertised SSO and native sliding-sync support, then actually sign in
   and run `SyncService`; record server/SDK versions and flow used.
2. Verify the test device using the second client. Prepare an encrypted room with
   more replies than one pagination page and a root outside loaded room history.
3. Load root/replies; paginate with overlapping cached/live data. Assert unique
   event IDs, correct order, and preserved root. Include a page containing only
   already-cached replies before further history becomes available.
4. Send ordinary replies and thread replies; inspect targeting and decryption in
   the second client. Receive replies while the pane is open, closed and reopened.
5. Exercise edits/reactions/redactions in each view and inspect summaries while
   closed. Record SDK support and observed gaps separately.
6. Test room/thread receipts and unread behavior, then restart with the same
   session/store. Reopen historical threads without automatically marking read.
7. Navigate to room/thread/message links, including unloaded events. Record
   missing-key behavior separately from failed session restoration.

For each case record setup, commands/actions, expected result, actual result,
pass/fail, and sanitized evidence. No live scenario has passed merely because its
API is present or compiles. Authenticated thread harness work remains outstanding.

## Recorded local results — 2026-11-13

Environment: development container on Linux `7.1.3+deb14-amd64`, x86_64,
Intel Core i7-11700 (16 logical CPUs), Rust 1.100.0, Cargo 1.100.0. Uncommitted
milestone 0 worktree; exact dependencies are in Cargo.lock. Dev build with debug
symbols disabled, application tracing off, no account connected.

- `cargo build --locked --bins`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo test --locked --workspace`: three lifecycle tests passed, covering
  cancellation under output backpressure, drop cancellation, and repeated cycles.
- `spike lifecycle`: 100 open/deliver/close cycles passed.
- `cargo tree -i tokio`: one resolved Tokio version (1.54.1), shared by Iced and SDK.
- `runtime-ui`: remained running for five seconds via the provided Wayland socket,
  with no logged errors; terminated by the smoke harness. Button interaction,
  visual correctness and redraw behavior are not verified.
- Idle sample: two-second settling time, then 10.00007 seconds measured;
  process CPU time delta 0.00 seconds at procfs resolution, 17 threads at both
  endpoints. This is a functional measurement smoke check, not a battery budget
  or evidence of zero wakeups. Raw local output: `artifacts/runtime-idle-cpu.json`.
- Measurement script: CPU-only smoke check and explicit failure without perf
  passed. Actual scheduler capture remains unverified: perf and scheduler
  tracepoint access are unavailable in this container.
- Homeserver SSO, sliding sync, encrypted threads, receipts, and restart:
  **not run** at the initial scaffold checkpoint; see discovery follow-up below.

At this initial checkpoint, only the dependency-selection acceptance check was complete. Runtime integration
still needs real SDK stream testing; section rules remain proposed. No product
feature is marked complete from these feasibility probes.

## Target homeserver discovery — 2026-11-13

Target: `https://matrix.mpi-sws.org`. The user confirms an account and second
client are available. Unauthenticated requests succeeded:

- [Client versions](https://matrix.mpi-sws.org/_matrix/client/versions) advertises
  versions through v1.17 and `org.matrix.simplified_msc3575: true`.
- The compiled SDK probe selects `Version::Native` via `DiscoverNative` and finds
  password, SSO, and token login flows. The SSO flow advertises
  `oauth_aware_preferred: true` and no explicit identity-provider list.
- [Client discovery](https://matrix.mpi-sws.org/.well-known/matrix/client)
  points to the same homeserver and advertises authentication issuer
  `https://matrix.mpi-sws.org/auth/`. OAuth preference warrants checking the
  browser SSO flow in practice; it does not by itself prove SSO is unusable.

Discovery alone establishes advertisements. The subsequent user-run interactive
check below confirms browser login and initial authenticated native sliding sync.

### Interactive SSO and initial-sync check

```sh
cargo run --locked --bin spike -- sso https://matrix.mpi-sws.org
```

If running in VS Code's container, forward **8765 to localhost:8765** using the
Ports panel before opening the printed browser URL. Native host execution needs
no forwarding. The SDK binds the callback to localhost and handles the login token internally; do not paste callback URLs or
tokens into chat. Browser login has a three-minute deadline. If the port is busy,
stop the conflicting test or use a native host setup; this spike does not select
a different callback port automatically.

This deliberately short-lived check creates a named temporary device, waits up
to 60 seconds for the room list to reach `Loaded` through `SyncService`, stops
sync, and attempts logout. `Running` alone is not treated as successful sync.
It sends no chat messages and saves no credentials or crypto store. If the
process is killed, login times out after server-side device creation, or logout
fails, remove the named temporary device from the other client. Device trust,
encrypted messaging, historical keys and session restoration require the next
persistent-session harness; this command does not validate them.

For subsequent runs, report whether browser login and initial room-list loading
succeed, or a sanitized error.

After adding `sso-login` and the interactive command: locked binary build,
formatting, Clippy with warnings denied, and all three existing lifecycle tests
passed again. These checks compile the SSO/sync integration but do not substitute
for completing the interactive login flow.

### User-run result — 2026-11-13

The user reported the following output from the interactive check against
`matrix.mpi-sws.org`:

```text
SSO callback and login succeeded. Device trust and historical keys are not verified.
Initial room list loaded through SDK native sliding sync.
```

Result: **pass** for milestone 0 homeserver SSO and sliding-sync capabilities.
This is user-reported execution evidence, not an agent-observed login. Logout
completion was not included in the reported output and remains unconfirmed.
Device verification, historical key availability, encrypted threads, receipts,
and restart/session restoration remain unverified. The application's milestone 2
login UI and keyboard workflow are not completed by this CLI feasibility result.
