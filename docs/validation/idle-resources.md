# Idle resources and lifecycle

The settled UI has no continuous redraw loop. Optional presence polling runs in
the account worker; SDK and network work still wake the process. CPU and scheduler
measurements capture activity that timer and subscription tests alone cannot
establish.

Clickable sender names reuse the existing DM task only on click. They add no
worker, polling, animation timer, or subscription. Rendered-widget click and
layout tests cover the controls; fresh production CPU/wakeup measurements were
not taken for this change (2026-11-17).

The optional Unread section filters the existing navigation snapshot during view
updates. It adds no SDK work, worker, subscription, polling, or timer. Unread
shortcuts reuse ordinary room navigation and read handling. Production CPU and
wakeup measurements were not repeated for this change (2026-11-17).

Optional unread-first ordering sorts the existing filtered room entries during
navigation/view updates. It adds no workers, SDK requests, subscriptions, polling,
or timers. Production CPU and wakeups were not measured for this option
(2026-11-17).

Space-rail unread dots derive from the same room snapshot and descendant-space
membership during view updates. They add no workers, SDK subscriptions, polling,
or animation timers. Rendered-widget coverage checks that settled rail redraws
request `Wait`; production CPU/wakeups remain unmeasured for this addition
(2026-11-17).

On-demand presence (2026-11-17) removes the extra classic-sync connection and
temporary SDK fork. Cached per-user reads run only when opening a DM/People page
or explicitly refreshing by default. Optional periodic refresh defaults off and
uses one deadline for all DM contacts, visible message senders, and People while focused.
The one-time
saved-cursor migration runs before SDK startup under the profile lock and retains
account/device keys. There is no extra sync stream, expiry timer, or recovery delay. See
[presence validation](connected-ui.md#user-presence).

Earlier connected diagnostics observed a shared-cursor retry loop at roughly one
CPU core; the temporary SDK patch recovered and a 45.71-second passive sample
used 0.45 CPU seconds. Those observations predate on-demand presence and are not
an idle-resource baseline for this implementation.

After the authorized on-demand launch and cursor migration, an 85.01-second
passive connected-process sample used 0.16 CPU seconds (0.19% of one core), with
no sync errors. Focus, incoming traffic, and user activity were uncontrolled;
scheduler wakeups and battery impact were not measured. This is a short functional
observation, not a general resource budget. Raw diagnostics stay outside the repo.

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

## Custom emoji — 2026-11-16

Pack metadata uses one additional SDK sliding-sync connection while a conversation
is selected. It requests only image-pack/canonical-parent state, with zero timeline
history, using normal SDK long polling. Cache projection is driven by SDK updates
and navigation; no periodic application refresh or animation subscription is added.
This adds network wakeups relative to previous captures; production CPU/wakeups
have not been remeasured. Picker pages, completion, previews and visible reaction
rows share the existing media slots and 96 MiB cache, with at most 80 additional
unique demands. GIF emoji stay static. Automated fixtures check a nonzero sync
long-poll timeout, no discovery media requests, shutdown cancellation and demand
release. Connected idle CPU, large-pack memory and macOS behavior remain unmeasured.

### Emoji management — 2026-11-16

The manager adds explicit account/state reads and writes, not another periodic
worker or sync connection. Its read/file tasks cancel on close/reload/account
replacement. Submitted writes have bounded deadlines and participate in pending
shutdown operations. Only the selected remote emoji enters media demand; local
file previews are decoded off the UI thread. New text fields disable cursor
blinking. Automated minimum-window redraw checks introduce no scheduled timer;
production CPU, native file-dialog cancellation and macOS remain unmeasured.

### Popup presentation — 2026-11-17

Composer picker alignment and border-only popup surfaces add no subscriptions,
workers, redraw loops or timers. Regression coverage checks outside-click capture
without a scheduled redraw; existing search/disabled-picker coverage checks idle
caret behavior. Tiny-skia damage-artifact acceptance and fresh process CPU/wakeup
measurements remain separate manual checks; no production resource claim is made.

### User presence — 2026-11-17

Presence uses explicit GET requests for at most 50 People members, unique visible
room/thread message senders (including the pinned original), and every
unambiguous navigator DM contact when periodic refresh is enabled, with
four in flight at once and five-second request deadlines. A 60-second memory
cache is checked only on new demand; it has no expiry timer. It retains up to
8192 users or the currently requested set, whichever is larger.
Manual Refresh bypasses cache age. Failures become unknown. With optional
periodic refresh disabled, there is no automatic retry; a settled worker waits
for selection changes and makes no requests.
Navigation/account replacement/shutdown cancel pending reads. UI status marks
remain static, and their tooltips identify last-checked snapshots.

The earlier filtered classic sync stream and SDK patch are removed. Cursor
migration uses the pinned SDK store API once per saved store, under the profile
lock, before any sync starts. Fixtures cover cancellation, stale-result rejection,
cache/refresh behavior, no idle/classic-sync requests, and stock-SDK reopening.
Production wakeups, large-account behavior, and battery impact remain unmeasured.

Optional periodic refresh (2026-11-17) is disabled by default, with a saved
60-second interval when enabled (15–3600 seconds). It arms a deadline only for
all navigator DM contacts, visible message senders, and People while focused, bypassing
cache age on each tick. Space filters, collapsed sections, and the open room do
not limit the navigator set; no room needs to be open. Contacts are deduplicated
and none are dropped by the 50-member page cap. Requests remain limited to four
in flight, so large accounts can take longer than the configured interval.
The deadline starts after a batch completes, preventing overlapping work and
catch-up bursts. Room/space changes do not restart a navigator-only batch or its
deadline. Disabling it, opening a modal, losing focus, or shutdown cancels periodic
work. Accounts without requested contacts and demo mode never poll. This adds
configured network/SDK wakeups when enabled, without an animation
or UI redraw timer. Production CPU, wakeups, and battery impact for enabled
polling have not been measured.

Visible-message presence reuses event-driven timeline viewport reports. It adds
no timer or subscription: new senders use the 60-second cache and the existing
four-request concurrency limit. Hidden room/thread panes, collapsed thread
summaries, activity rows, and own messages contribute no lookup demand. Scrolling,
navigation, and stale snapshots cancel superseded demand. When periodic refresh
is enabled, visible senders join its existing batch and deadline. Completed
reads update the cache as they arrive so a slow sender or viewport cancellation
does not discard completed navigator work. Unchanged status values produce no
additional model update. Automated regressions cover viewport changes, cache
reuse, thread cancellation, hidden
panes, demo isolation, and unchanged drafts/scroll positions; production CPU and
wakeups for this addition have not been measured.
