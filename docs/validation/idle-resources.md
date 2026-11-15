# Connected UI lifecycle and idle resources

## Repeatable checks

Run the normal suite and the longer SDK fixture separately:

```sh
cargo test --all-targets
cargo test --lib repeated_navigation_releases_timelines -- --ignored
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
CHATCLIENT_METRICS=1 RUST_LOG=off target/debug/matrix-ui .matrix-spike
```

Once the initial `{"metrics":true,...}` JSON report appears on stderr, send
`kill -USR1 <pid>` to report counters on demand. Do not signal an ordinary run:
SIGUSR1 handling is only installed in instrumented runs. Counters contain no room
IDs, account IDs or message contents. Array order is `[room, thread]`.

- `view_starts`, `view_stops`, `active_views`, `peak_views`: presentation futures.
- `live_timelines`: surviving registered SDK timeline objects, including those
  retained by UI snapshots or pending sends after the view future stops.
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

Compare settled visible and genuinely hidden compositor states. Keep input,
window geometry, logging and incoming traffic controlled, and record whether an
editor has focus. Cursor blink may cause visible redraws. Hide only the target
window; avoid changing the whole desktop. Hidden UI should continue Matrix sync.

## Host run, 2026-11-14

Host: Intel Core Ultra 7 255U (14 logical CPUs), x86_64, Linux
7.1.13+deb14-amd64, Sway 1.11, rustc 1.100.0 (2026-11-09). Development build
(unoptimized, debug info disabled), Iced 0.14 tiny-skia/Wayland, Matrix SDK 0.19.1.
Worktree based on `0877eb5` plus the diagnostics/sampler changes. Restored existing
host profile, connected to the real server, logging off, diagnostics enabled.
Raw measurements are local ignored files under `artifacts/idle-2026-11-14/`.

Validation: 26 regular tests passed; explicit 100-cycle stress test passed in
16.24 s; Clippy with warnings denied passed. Sampler smoke check verified resource
fields and rejection of NaN, infinite, zero and negative sample durations.

During the initial CPU-only run, the installed `perf`
unprivileged tracepoint probe failed with permission denied; no scheduler wakeup
or battery/power conclusion follows from CPU-only captures.

The room-list captures showed no delivered redraws or UI updates. They still
consumed approximately 4% of one CPU core in this development build. A separate
`strace -c` diagnostic showed frequent `fdatasync` calls. This is consistent with
the SDK's default multiprocess store-lock heartbeat: pinned SDK 0.19.1 source
`matrix-sdk-common/src/cross_process_lock.rs` defines
`EXTEND_LEASE_EVERY_MS = 50`, and the SQLite crypto store renews a lease with a
write transaction. This is a hypothesis about the observed background CPU cost,
not a CPU-stack attribution. No lock settings were changed in this measurement.

A useful follow-up is to audit the application's exclusive profile lock and all
client construction paths, then evaluate SDK single-process store configuration
if that exclusivity covers the complete client lifetime. Compare measured
wakeups, flushes and CPU before adopting it; do not simply disable lease renewal
or change database durability settings.

| Scenario (30 s each) | CPU seconds | One-core CPU | RSS KiB, start → end | Redraws | UI updates |
| --- | ---: | ---: | ---: | ---: | ---: |
| visible-room-list | 1.09 | 3.63% | 119532 → 119608 | 0 | 0 |
| hidden-room-list | 1.18 | 3.93% | 113572 → 113660 | 0 | 0 |
| visible-focused-thread | 1.28 | 4.27% | 124972 → 125052 | 63 | 0 |
| hidden-focused-thread | 1.09 | 3.63% | 125052 → 125084 | 0 | 0 |

All four captures had 48 threads and stable endpoint thread sets. File-descriptor
counts were stable within each capture (108 for the first hidden room-list
capture, 114 for the later visible room-list capture, 116 with room/thread open).
The room-list samples used different window geometries (960×1164 hidden,
964×887 visible); the focused-composer pair retained 960×1133. Each is one short
sample, not a statistical benchmark. No typing or sends occurred. Incoming server
traffic was not suppressed. Samples overlapping navigation or strace attachment
were excluded from this table.

The focused-composer pair had two live timelines throughout; hiding preserved both
views but stopped delivered redraws. UI updates stayed at zero during all listed
samples. RSS changes are small within each sample, but this is not a long-run
memory bound. A separate 3-second syscall trace attributed 51 `fdatasync` calls
to `matrix-sdk-crypto.sqlite3-wal`, strengthening the store-heartbeat hypothesis;
the tracing interval is excluded from the CPU measurements.

Scheduler wakeups were unmeasured in the initial run: lowering
`perf_event_paranoid` to -1 did not make the root-only tracefs mount readable.
The diagnostic was permission denied at
`/sys/kernel/tracing/events/sched/sched_wakeup`. The follow-up below resolves
that measurement gap for the idle room-list scenario. Actual power draw, suspend/resume, and extended
navigation across many distinct live rooms remain unmeasured.

Closing the live thread reduced counters from two active views/timelines to one;
the remaining room stayed open. Closing the measurement GUI then exited with no
panic marker in stderr. No live send was performed during these measurements.


## Scheduler follow-up, 2026-11-14

After the user made tracefs directories traversable and the `id`/`format` files
for `sched_wakeup` and `sched_wakeup_new` readable, unprivileged captures worked.
The same development binary was launched with `CHATCLIENT_METRICS=1 RUST_LOG=off`
and the saved host profile. The UI showed Connected, with no room selected.
Two alternating visible/hidden pairs used `scripts/measure-idle.py` without
`--cpu-only`, requesting 30 seconds per capture. Sway scratchpad hid only this
measurement window. Window visibility was checked at both endpoints. Diagnostic
signals were sent outside the measured windows; no messages were sent.

| Scenario | Elapsed seconds | Target wakeups | Wakeups/s | CPU seconds | One-core CPU |
| --- | ---: | ---: | ---: | ---: | ---: |
| visible-room-list-1 | 30.273 | 5191 | 171.5 | 1.08 | 3.57% |
| hidden-room-list-1 | 30.300 | 5108 | 168.6 | 1.10 | 3.63% |
| visible-room-list-2 | 30.327 | 5136 | 169.4 | 1.06 | 3.50% |
| hidden-room-list-2 | 30.250 | 5128 | 169.5 | 1.07 | 3.54% |

All four captures had zero redraws and zero UI updates. Both tracepoints were
captured successfully; `sched_wakeup_new` was zero throughout. The target's 42
threads had identical endpoint TID sets, and file descriptors stayed at 90.
RSS increased from 107076 to 107748 KiB over the sequence, without establishing a
long-run memory bound. Sway reported 1028×770 while visible and 1028×787 in the scratchpad
(including decoration differences).

Counts are filtered to the client threads in the event's target `pid` field,
not all system activity. A separate two-second capture of a sleeping control
process returned zero for both events. Rates above use the helper's elapsed
wall time, which includes perf setup/teardown (about 0.3 seconds beyond the
requested 30 seconds); they are approximate. CPU is measured over that same
wall-time interval. Raw JSON, endpoint counters, and perf output are in the
ignored local directory `artifacts/scheduler-2026-11-14/`.

The visible and hidden rates are similar: about 170 scheduler wakeups per second
with no UI redraws, and about 3.5–3.6% of one core. This confirms background work
persists when hidden. It does not establish causal attribution to the 50 ms SDK
lock heartbeat or measure hardware idle exits/battery consumption. Auditing the
exclusive profile lock and comparing SDK single-process store configuration
remains the next investigation. Focused-composer scheduler counts were not
repeated in this follow-up; their earlier redraw/CPU observations remain above.


## Stack-profile investigation, 2026-11-14

An idle connected room-list run of the same development binary was sampled with:

```sh
perf record -o cpu.data -e cpu-clock -F 499 --call-graph dwarf,16384 -p <pid> -- sleep 45
perf script -i cpu.data > cpu-stacks.raw
c++filt -s rust < cpu-stacks.raw > cpu-stacks.txt
```

The binary has a symbol table and unwind information despite `debug = 0`.
No rebuild or SDK configuration change was made. The capture contained 653 CPU
samples and reported zero lost samples. Analysis counted each sample once per
matching stack category, including generic task/closure frames that identify
which operation owns the work:

| CPU stack category | Samples | Share |
| --- | ---: | ---: |
| Crypto-store `try_take_leased_lock` | 422 | 64.6% |
| Cross-process-lock frames without a crypto lease frame | 40 | 6.1% |
| Other stacks | 191 | 29.2% |
| `fdatasync` anywhere in stack (overlaps categories above) | 210 | 32.2% |

Of the 210 `fdatasync` samples, 208 also contain crypto lease frames. A captured
stack connects the work directly (shown caller to callee, with wrappers omitted):

```text
SqliteCryptoStore::try_take_leased_lock closure
  → SqliteAsyncConnExt::with_transaction
  → rusqlite::Transaction::commit
  → sqlite3_step / sqlite3VdbeExec / sqlite3VdbeHalt
  → sqlite3PagerCommitPhaseOne / sqlite3WalFrames
  → fdatasync
  → btrfs_sync_file / btrfs_log_dentry_safe
```

This directly attributes most sampled idle CPU work to crypto-store lease
transactions. The pinned SDK's renewal loop sleeps 50 ms between lease attempts;
the source and CPU stacks now support the heartbeat diagnosis much more strongly
than the previous flush-count inference. It does not yet assign every scheduler
wakeup to that loop. Remaining stacks include Tokio timer processing, parking,
and scheduling. Some library frames lack symbols: 17 samples have an unknown
leaf, and additional stacks have unknown intermediate frames; percentages are
sample shares, not exact elapsed-time costs or an optimized-build benchmark.

Local artifacts are in `artifacts/stacks-2026-11-14/` (ignored): raw perf data,
demangled stacks, a machine-readable count summary, and representative stacks.
Raw stack captures are kept local because they can contain process memory.

A separate 30-second `sched_wakeup` stack recording filtered the event target
`pid` to the client's thread IDs. It recorded 4953 samples with stable endpoint
TIDs, but `perf script` rejected it as “broken or missing trace data”. Tracefs
`events/header_page`, `events/header_event`, `printk_formats`, and `saved_cmdlines`
were still unreadable. Wakeup counts alone worked with readable event `id` and
`format` files; decoding stack recordings additionally needs tracing metadata.
This first recording is not used for wakeup-stack conclusions.

Before changing to SDK `SingleProcess`, strengthen/audit ownership of the
exclusive profile guard: `RestoredSession` holds it through sync shutdown, but
GUI composer/timeline handles can outlive the backend future. The guard must
cover those handles and SDK cleanup too. This investigation changes no locking
or database durability settings.


After tracing metadata permissions were corrected, a fresh 30-second
`sched_wakeup` recording decoded successfully: 4952 events, stable endpoint thread
IDs. It exposed an important attribution limit: 3914 events (79.0%) contain
`sched_ttwu_pending`, meaning activation was deferred to the destination CPU.
Those callchains mostly show an idle CPU processing queued wakeups, not the
original requester. Another 908 (18.3%) contain `hrtimer_wakeup`; 130 fall outside
those two categories. Thirty-two callchains retain the SDK cross-process-lock
task and Tokio machinery. These counts explain why recording `sched_wakeup`
alone is insufficient for complete requester attribution on this host.

Use `sched_waking` for the follow-up requester stacks, applying the same target
TID filter. Its event `id` and `format` metadata require read access too. CPU-stack
attribution above is independent of this tracepoint limitation.


### Original requester stacks (`sched_waking`)

With `sched_waking/{id,format}` readable, a 30-second capture used the same
system-wide recording and target-TID filter as `sched_wakeup`, with
`--call-graph dwarf,16384`. This produced 5188 decoded samples, no lost samples,
and identical endpoint TID sets. These are wake requests, not a second count of
the earlier recording's completed wakeups; the captures ran separately.

| Requester stack category | Events | Share |
| --- | ---: | ---: |
| Timer expiry | 931 | 17.9% |
| SDK crypto lease or lock task | 1332 | 25.7% |
| Btrfs completion | 1796 | 34.6% |
| Block flush completion | 531 | 10.2% |
| Other (mostly runtime handoffs) | 598 | 11.5% |

Categories are mutually exclusive, assigned in this priority order: crypto
`try_take_leased_lock` or `cross_process_lock` frame; `hrtimer_wakeup`; Btrfs
module/function; `flush_end_io`; other. Most remaining events have futex/runtime
handoff frames. The source snippets and captured stacks establish this path:

```text
SDK lease renewal timer (50 ms in pinned source)
  → crypto-store lease transaction
  → Tokio spawn_blocking wakes a SQLite worker (futex)
  → SQLite WAL commit calls fdatasync
  → Btrfs writeback and block flush completions wake the waiting worker
  → completion wakes an async runtime worker (eventfd/epoll or futex)
```

A representative requester stack directly includes the lease closure,
`deadpool_sync::SyncWrapper::interact`, Tokio `spawn_blocking` / `spawn_task`,
and `futex_wake`. Filesystem examples include `end_bbio_data_write`,
`end_bbio_meta_write`, `orig_write_end_io_work`, and `flush_end_io` waking the
client's worker threads. Multiple scheduler wakes per renewal explain how a
50 ms application timer can lead to substantially more than 20 wakeups/s.

The CPU and requester profiles together identify crypto-store lease renewal as
the dominant sampled idle CPU cost and a major source of the wakeup chain. The
profiles do not prove every timer or filesystem wake belongs to this task; an
A/B run after correcting profile-guard ownership is still needed to quantify the
reducible wakeups and CPU. Profiling overhead also means these recordings should
not replace the earlier lightweight `perf stat` baseline. No SDK lock policy or
durability settings were changed during this investigation.


## Single-process fix and A/B check — 2026-11-14

The persistent-client builder now selects SDK `SingleProcess` store locking.
Before store construction, it duplicates the application's existing OS profile
lock into a process-lifetime registry. The kernel retains exclusion until the
process exits, including late SDK clones, GUI teardown, construction failures,
and timed-out runtime cleanup. This is deliberately conservative for the current
one-account-per-process executable: reopening a used profile requires restart.
No SQLite durability settings changed. See the
[ownership design](../architecture/milestone-0.md#single-process-store-ownership--2026-11-14).

Validation passed: 27 regular tests, explicit 100-cycle SDK navigation test,
Clippy with warnings denied, formatting, and GUI build. SDK fixtures use the new
mode for history, encrypted room/thread send and retry, network loss/recovery,
and shutdown. A subprocess test verifies competing-profile exclusion with late
handles and blocked cleanup, including the actual shutdown timeout helper, then
successful acquisition after process exit.

For the A/B comparison, a preserved copy of the previous GUI binary and the new
binary restored the same host profile sequentially. Both displayed Connected.
No messages were sent. The new binary settled for at least 15 seconds before
sampling. Compiler/test jobs had completed before captures. The same host,
logging-off settings, diagnostics, and 30-second filtered scheduler sampler
were used. Initial visible geometry was 640×1164 for both; Sway scratchpad
geometry was 964×904 for both hidden captures. Raw evidence is under the ignored
`artifacts/single-process-2026-11-14/` directory.


| Build / state | Target wakeups | Wakeups/s | CPU seconds | One-core CPU | Redraws | Threads |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| before-visible | 5153 | 170.23 | 1.14 | 3.77% | 0 | 48 |
| before-hidden | 5188 | 171.70 | 1.09 | 3.61% | 0 | 48 |
| after-visible | 335 | 11.08 | 0.27 | 0.89% | 8 | 16 |
| after-hidden | 49 | 1.62 | 0.05 | 0.17% | 0 | 16 |
| after-visible-settled | 50 | 1.65 | 0.05 | 0.17% | 0 | 16 |

All listed captures have stable endpoint TID sets and zero `sched_wakeup_new`
events. The first new visible sample included eight redraws, so the settled
repeat provides the quiet visible observation (964×887 after scratchpad restore).
An unsuccessful intermediate repeat was excluded. All listed samples had zero
UI updates. The matched hidden pair shows about 99.1% fewer wakeups to the initial thread
set (not the whole process; see the residual-activity correction below)
and 95.4% less sampled CPU time; the quiet new visible observation agrees.
These are short development-build measurements, not battery/power estimates.

Separate three-second `fdatasync` traces, outside the timed captures, showed 51
calls on `matrix-sdk-crypto.sqlite3-wal` before the change and zero calls after.
This A/B result confirms that removing SDK multiprocess lease renewal removes
most of the measured idle work while ongoing sync remains connected. Background
network/sync work remains; the goal is not zero wakeups under all conditions.


## Residual activity and counting correction — 2026-11-14

The approximately 1.6/s result above counts only threads present at the beginning
of each `perf stat` window. Stable endpoint TID sets missed transient SQLite
blocking workers: a new requester-stack capture recorded two workers being born
and exiting about 10.5 seconds later, both between the endpoint snapshots.
Therefore the earlier approximately 99% reduction applies to the fixed thread
set, not the whole process. `measure-idle.py` now labels its output scope as
`initial_thread_set` and explicitly warns that stable endpoints do not establish
whole-process coverage. CPU time from `/proc/<pid>/stat` still covers the process.

A 60-second requester-stack capture of those initial threads recorded 103
`sched_waking` events. They formed two bursts of 51 and 52 events, starting
30.077 seconds apart and lasting 0.47 and 0.57 seconds. Stacks show TCP receive,
Tokio scheduling/handoffs, SQLite key/value reads and writes, and crypto-store
`save_changes`. No `try_take_leased_lock` path appeared, and diagnostic counters
showed no redraws or UI updates. This is consistent with ordinary Matrix sync
responses and persistence, rather than a continuous 50 ms lock heartbeat.

To include transient workers, a second recording admitted target threads named
`tokio-rt-worker` plus the original client TIDs, then used recorded
`PERF_RECORD_FORK`/`EXIT` metadata to retain only targets belonging to this client,
including new threads while they existed. Other processes' events were excluded
from the client totals. An initial stack-rich broad recording reported lost data
and was not used for the definitive rate. A lower-overhead recording without
stacks, with larger perf buffers (`-m 1024`), captured both `sched_waking` and
`sched_wakeup` for a requested 60 seconds with no lost records:

| Measure | Result |
| --- | ---: |
| Client wakeups, including transient workers | 177 |
| Of those, targets in the initial thread set | 105 |
| Targets in transient workers | 72 |
| Wake requests (`sched_waking`) | 177 |
| Recorder wall time including setup/teardown | 61.61 s |
| Approximate full-process rate | 2.9/s (about 3/s) |

Of the 177 wakeups, 170 occurred in two bursts of 86 and 84 events, about 30.1
seconds apart. This reinforces sync-response processing as the main residual
source. The other seven events were scattered outside those bursts; not every
individual event has been causally assigned. Compared with the earlier roughly
170/s baseline, this is approximately a 98% reduction rather than the previously
quoted 99% full-process interpretation. These remain short observations under
this account's traffic, not a fixed rate guaranteed in all rooms or networks.

Raw evidence and summaries are in the ignored local directory
`artifacts/residual-idle-2026-11-14/`. The existing `perf stat` helper is useful for
fixed-thread comparisons, but transient-thread accounting is required before
calling its result a whole-process rate.
