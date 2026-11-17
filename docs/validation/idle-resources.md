# Idle resources and lifecycle

## Expected behavior

The UI has no global tick or background animation loop. SDK/network events drive
updates. Presence polling is optional and off by default; typing expiry, pending
operation deadlines, dirty-draft debounce, and explicitly playing GIFs schedule
work only while needed. Carets are static. Stopping playback, closing a view, or
ending a session must cancel its associated work.

Selected timelines are session-owned and asynchronously cancelled. Stale results
must not restore old panes; SDK handles must be dropped within a live runtime.
See [architecture](../architecture/client.md) for ownership and bounded media work.

## Repeatable checks

```sh
cargo test --locked --workspace
cargo test --locked --lib repeated_navigation_releases_timelines -- --ignored
```

The ignored fixture reopens a mock encrypted room/thread 100 times. It checks
at most one active presentation future per pane, one SDK timeline after thread
close, and none after shutdown and snapshot release. Weak references observe
ownership without extending it. This does not bound allocator RSS, SDK event
caches, room subscriptions, or drafts across many distinct rooms.

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
repeat navigation, offline/reconnect, and suspend/resume. Test both repeated
visits to the same conversation and many distinct conversations. For GIFs compare
unopened, open/still, playing, completed, stopped, closed, and unfocused states.
Compare presence polling off/on, verification closed/pending, and dirty/clean drafts.

Keep build, renderer/adapter, fonts, window geometry, focus, logging, and incoming
traffic comparable. Exclude compilation/startup from idle captures. Hiding a
window leaves Matrix sync running. Headless/software results do not establish
native GPU memory, battery use, or macOS behavior; unit tests do not measure
CPU or wakeups. Store capture output as local artifacts, not baseline run logs
in this document.
