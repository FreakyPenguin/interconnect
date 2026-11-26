# Idle resources and lifecycle

## Expected behavior

The UI has no global tick or background animation loop. SDK/network events drive
updates. Presence polling is optional and off by default; typing expiry, pending
operation deadlines, dirty-draft debounce, and explicitly playing GIFs schedule
work only while needed. Carets are static. Stopping playback, closing a view, or
ending a session must cancel its associated work.

Verified connected sessions make one bounded room-key prefetch pass after room
discovery and the checked completion-dialog choice. Unchecking skips it for the
session; already verified restores retain the automatic startup pass.
The pass reads one room at a time, up to 50 recent encrypted events each, with a
30-second room deadline. Completion schedules no polling or repeated pass;
trust loss/session teardown cancels remaining reads. Exclude this initial work
and subsequent SDK key exchanges from settled idle measurements. See
[verification](verification.md) for scope and connected acceptance checks.

Selected timelines are session-owned and asynchronously cancelled. Stale results
must not restore old panes; SDK handles must be dropped within a live runtime.
See [architecture](../architecture/client.md) for ownership and bounded media work.

## Repeatable checks

```sh
cargo test --locked --workspace
cargo test --locked --lib repeated_navigation_releases_timelines -- --ignored
```

The ignored fixture reopens a mock encrypted room/thread 100 times: at most one
presentation future/pane, one timeline after thread close, none after shutdown/
snapshot release. It does not bound RSS, SDK caches/subscriptions, or drafts
across distinct rooms.

The UI test `sqlite_cleanup_outlives_iced_executor_and_runs_before_return` opens a
temporary SQLite store without an account or network requests, then drops Iced's
executor before the SDK client. Its blocking cleanup probe acknowledges startup
and waits until runtime shutdown cancels a pending task before completing. This
checks that `run_ui` retains the runtime context through SDK teardown and waits
for already-running blocking cleanup before returning. It does not assert that
Tokio executes queued blocking tasks during shutdown; those may be discarded.

## Opt-in live diagnostics

```sh
INTERCONNECT_METRICS=1 RUST_LOG=off cargo run --locked --bin interconnect -- --demo
```

For SDK lifecycle counters, use an explicitly chosen test profile instead of
`--demo`. After the initial `{"metrics":true,...}` report on stderr, request a
snapshot with `kill -USR1 <pid>`. **Send SIGUSR1 only to an instrumented process:**
ordinary runs retain the signal's default termination behavior.

Counters contain no account/room IDs or message text; array order is `[room, thread]`.

| Counter | Meaning |
| --- | --- |
| `view_starts`, `view_stops`, `active_views`, `peak_views` | Presentation futures. |
| `live_timelines` | SDK timeline objects, including retained snapshots/pending sends. Pins can add one extra timeline. |
| `redraws` | Iced redraw events, not GPU presentations or compositor callbacks. |
| `ui_updates` | Messages processed by the UI update function. |

Reports are on demand, with no timer or redraw feedback loop. Sample counters
before/after a settled capture, outside its timing window. Concurrent work can
change counters while a report is read.

## External capture

```sh
python3 scripts/measure-idle.py <pid> --seconds 30 --label visible-room-list
```

Scheduler tracepoints may require running the capture with `sudo`. Use
`--cpu-only` when scheduler data is unavailable; wakeups are then unmeasured,
not zero. Scheduler capture filters target TIDs in system-wide tracepoints;
`perf -p` would observe the waker's context instead. A changed endpoint thread
set invalidates wakeup measurement; transient thread churn is a remaining limit.

CPU percent uses one fully occupied core as 100%. RSS/virtual memory, thread
count, and open descriptors are endpoint measurements; RSS is neither peak
memory nor an allocation/leak count.

## Measurement scenarios

Compare settled visible/hidden windows, focused composers, open/closed threads,
repeated/distinct-room navigation, offline/reconnect, suspend/resume, presence
off/on, verification closed/pending, poll forms open/closed and hidden/ended
poll cards, and drafts dirty/clean.

For inline/viewer GIFs compare unopened, still, playing, completed, stopped, closed,
and unfocused. Viewport departure/navigation must remove animation deadlines;
loading stays still and focus restoration must not restart playback.

Keep build, renderer/adapter, fonts, geometry, focus, logging, and traffic comparable;
exclude startup/compilation. Hiding keeps sync running. Headless/software captures
do not establish native GPU/battery/macOS behavior; unit tests do not measure
CPU/wakeups. Keep capture artifacts local, not in this reference.

For loaded-history append, resize, scroll-frame, and RSS scaling, use the
[long-history fixture](long-history.md). Those UI measurements are separate from
settled idle CPU and wakeups.
