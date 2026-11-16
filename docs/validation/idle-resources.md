# Idle resources and lifecycle

The settled UI has no global polling or continuous redraw loop. SDK and network
work still wake the process. CPU and scheduler measurements capture activity
that timer and subscription tests alone cannot establish.

## Message bookmarks — 2026-11-15

The account-wide bookmarks worker adds one storage-room cache subscription and
hydrates its full history. The Saved messages view additionally owns up to 20
source timelines and source-room sync subscriptions for its current page;
four setups run concurrently. These end on paging, closing or account changes.
Settled streams wait on SDK/cache updates. Only finite setup/write deadlines add
timers. No periodic refresh, frame subscription or automatic receipt was added.
Existing `live_timelines` diagnostics do not count these source timelines.
The CPU-rendered view test checks that settled redraws request `Wait`.
Production CPU, wakeups and large-history memory remain unmeasured; earlier
measurements below predate this feature. See [bookmark design](../bookmarks.md).

## Repeatable checks

Run the normal suite and the longer SDK fixture separately:

```sh
cargo test --locked --workspace
cargo test --locked --lib repeated_navigation_releases_timelines -- --ignored
```

The stress fixture reopens the same mock encrypted room and its thread 100 times.
It asserts at most one active presentation future per pane, one remaining SDK
`Timeline` after closing a thread, and zero remaining timelines after shutdown
and release of UI snapshots. Weak references observe ownership without keeping
SDK timelines alive. This does not establish a bound on SDK event caches, room
metadata subscriptions, allocator RSS, or drafts across many distinct rooms.
The existing fixture also checks encryption, send retry, pagination and shutdown;
all traffic in this test goes to the local mock server.

## Opt-in live diagnostics

```sh
INTERCONNECT_METRICS=1 RUST_LOG=off target/debug/interconnect
```

Once the initial `{"metrics":true,...}` JSON report appears on stderr, send
`kill -USR1 <pid>` to report counters on demand. SIGUSR1 handling is installed only
in instrumented runs; an ordinary run uses the signal's default behavior. Counters contain no room
IDs, account IDs or message contents. Array order is `[room, thread]`.

- `view_starts`, `view_stops`, `active_views`, `peak_views`: presentation futures.
- `live_timelines`: surviving registered SDK timeline objects, including those
  retained by UI snapshots or pending sends after the view future stops. A
  selected room with pins can add one pinned-events timeline; this does not add
  to the `[room, thread]` view counters. Rooms without pins add no timeline.
- `redraws`: Iced redraw events delivered to its subscription system, not GPU
  presentations or compositor frame callbacks.
- `ui_updates`: messages processed by the UI update function.

Diagnostics add no periodic timer. The redraw listener counts events and returns
no UI message, avoiding a redraw/update feedback loop. Reports run on SIGUSR1;
request before and after a capture, outside its timed window. Reports are intended
for settled observations; concurrent activity can change counters while read.
Normal runs do not install the raw redraw or signal subscriptions.

## External capture

```sh
python3 scripts/measure-idle.py <pid> --seconds 30 --label visible-room-list
# If scheduler tracepoints need elevated privileges:
sudo python3 scripts/measure-idle.py <pid> --seconds 30 --label visible-room-list
# Explicit fallback; never labels missing scheduler data as zero:
python3 scripts/measure-idle.py <pid> --seconds 30 --cpu-only --label visible-room-list
```

CPU seconds cover all process threads; CPU percent uses one fully occupied core
as 100%. Resource endpoints include resident/virtual KiB, thread count and open
file descriptors. Endpoint RSS is neither peak RSS nor an allocation/leak count.
Scheduler measurements filter target TIDs using system-wide tracepoints, not
`perf -p` (which would observe the waker's context). An endpoint thread-set change
invalidates the wakeup measurement; transient thread churn remains a limitation.

Comparable visible/hidden captures use the same input, window geometry, logging,
and incoming traffic conditions, including editor focus. The local editor patch
uses a static caret. Hiding only the target window isolates its visibility from
other desktop activity; Matrix sync continues while the window is hidden.

## Measurement scenarios

Measurement scenarios include settled visible and hidden room lists, focused
composers, open threads, offline/reconnected state, busy accounts, and suspend/resume.
A capture includes the build, renderer/adapter, host, geometry, focus, logging, and
incoming traffic. Compilation and startup activity can distort idle samples.

For GIFs, use demo **Studio → Images** and compare unopened, open/still, playing,
completed, manually stopped, closed, and unfocused states. Only explicit playback
schedules frame deadlines; stopping removes the playback subscription.

Verification uses SDK events and explicit discovery, notifications use a bounded
queue and async delivery, and typing uses selected-room events and pending-activity
deadlines. Their lifecycle tests check cancellation and idle subscriptions;
CPU/wakeup measurements have not been repeated for all these additions.
See [notifications](notifications.md) and [verification](verification.md).

## Recorded baseline and interpretation

On November 14, 2026, profiling a Linux/Sway development build identified SDK
multiprocess crypto-store lease renewal as the dominant idle cost. CPU stacks
showed SQLite lease transactions and `fdatasync`; requester stacks connected the
50 ms renewal loop to runtime and filesystem wakeups. Selecting SDK single-process
locking with a process-lifetime profile guard removed that renewal work without
changing SQLite durability. See [store ownership](../architecture/client.md#persistent-accounts).

The earlier idle baseline was about 170 wakeups/s and 3.5–3.8% of one core.
After the fix, a capture including transient workers observed 177 wakeups over
61.61 seconds, about 2.9/s, clustered around sync responses. Initial fixed-thread
captures reported about 1.6/s because they missed short-lived workers; those rates
cover only the initial thread set.

A November 15 follow-up on build `3e8f25c` measured the already-running connected
client without changing its conversation or sending messages:

| State | Elapsed | Wakeups including new workers | Wakeups/s | CPU seconds | One-core CPU |
| --- | ---: | ---: | ---: | ---: | ---: |
| Hidden | 31.663 s | 106 | 3.35 | 0.05 | 0.16% |
| Visible, focused, settled | 31.695 s | 102 | 3.22 | 0.06 | 0.19% |

Those captures used system-wide scheduler recording, target-TID filtering, and
FORK/EXIT metadata to include transient workers, with no lost records. Diagnostics
were disabled, so redraw and UI-update counts were unmeasured. Most events arrived
in short clusters about 30 seconds apart, consistent with sync processing; no
stacks were captured to attribute each event.

These are short development-build observations for one account, not release
budgets, long-run memory bounds, battery measurements, or guarantees for later
changes. Raw recordings remain in ignored local artifact directories. Comparisons
with later builds require fresh measurements. The helper's `initial_thread_set`
scope requires separate transient worker accounting for whole-process scheduler totals; stable endpoint TIDs alone
are insufficient.
