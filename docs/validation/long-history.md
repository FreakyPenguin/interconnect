# Long-history performance

Measure virtualized timeline scaling and individual-message rendering before
choosing further optimizations or a loaded-history limit. This opt-in fixture
uses only local demo data, without creating SDK clients, opening profiles,
starting a window, or making network requests.

```sh
python3 scripts/measure-long-history.py > /tmp/interconnect-long-history.jsonl
```

By default this builds the optimized release test binary and measures 500,
2,000, and 10,000 events, three fresh processes per size, with 20 samples per
operation. `--events`, `--runs`, and `--samples` adjust the workload; `--debug`
uses the ordinary test profile for fixture development, not release conclusions.
Keep generated output and machine-specific artifacts outside the repository.

The fixture uses a 1280×800 viewport at scale 1 with bundled Inter fonts. It warms
the ordinary demo shell/font discovery, then replaces the selected room with
unique synthetic events: 60% plain multilingual wrapping text, 20% formatted
lists/quotes/links, 10% code blocks, and 10% image attachments. Formatted documents
have unique content; attachments reuse the same local demo image/cache entry.
Images are static. There is no background animation, polling, or media fetching.

Each frame measures view construction, widget-tree diff/layout, draw-command
generation, and full tiny-skia software rasterization using preallocated buffers.
The reported operations are:

- Initial frame after loading the generated history; fixture preparation is
  excluded, while first layout/shaping of that history is included.
- Append one event, then render, repeated for the requested sample count.
- Resize between widths 960 and 1280, keeping height 800, then render.
- Advance the bottom-anchored scroll offset by 400 pixels per sample, including
  the widget scroll operation and a full frame. These are scroll-frame costs,
  not measured input-to-display latency.

Each record reports `workload`, workload `units`, the initial event count, total
bytes in the initial message bodies (`message_bytes`), maximum mounted row count,
and final loaded row count (including appended samples). The mounted maximum
includes shell warm-up frames, so it can exceed the final loaded count in a
one-message workload. These counters distinguish widget rendering from retained
model history. Small conversations up
to 192 projected rows use exact full layout; larger histories mount an overscanned
window.

Scroll samples include the corrective relayout following the widget scroll
operation, matching the widget lifecycle needed for newly mounted rows. Older
captures without that relayout undercount scroll work and are not identical
workloads. Viewport/counter probes add a small bounded traversal as well.

Median, nearest-rank p95, and maximum are reported in milliseconds. The JSON
header records revision, dirty state, platform, Rust version, renderer, and
profile. Samples follow in separate JSON records; compare runs individually
and report variability rather than treating one maximum as a stable bound.

On Linux, RSS is sampled after warming the shell, after generating rows, after
initial layout/render, after all operations, and after dropping the app/widget
tree/renderer. These are process endpoint measurements, not allocation counts
or peak memory. Allocator and process-global font/render caches can retain
memory after drop. Other platforms report null RSS; use native memory tooling.

This isolates UI loaded-history scaling and single-message rendering. It does
not measure SDK diff processing, encrypted storage, history pagination, network
latency, unique-media cache growth, thread summaries, GPU presentation, native
input latency, idle CPU/wakeups, battery use, or macOS desktop behavior. Full software redraws differ
from damage-limited redraws and native wgpu. Do not extrapolate absolute frame
times into a GPU frame-rate claim.

Use results to compare changes to overscan, caching, and per-message layout.
Row virtualization bounds mounted widgets, not the cost of one mounted message.
Preserve event-relative anchors, pagination, selection, replies, thread summaries,
and read-receipt visibility; add connected/native checks before release.

## Individual enormous messages

Choose `--workload mixed` (the default) for many ordinary messages. Run the
individual-message workloads separately; `--events` means repeated text phrases,
code lines, or repeated wide-line chunks respectively:

```sh
python3 scripts/measure-long-history.py --workload long-text --events 1000 10000
python3 scripts/measure-long-history.py --workload many-line-code --events 1000 10000
python3 scripts/measure-long-history.py --workload wide-code --events 1000 10000
```

These start with one message, then run the same append, resize, and scroll
operations. Fixture preparation precedes timing; initial layout is timed. The
synthetic code documents deliberately bypass the incoming HTML byte limit so
very large code stays on the code renderer instead of becoming a plain fallback.
They construct structural documents without syntax-color preparation, exclude
parser/highlighter worker costs, and do not imply that such HTML is accepted from
a homeserver. Long text remains a single plain paragraph; many-line
and unusually wide code are separate cases.

Code workloads also report expand/collapse frame costs, including corrective
layout and rasterization. Collapsed shaping must stay within 12 lines / 2 KiB;
expansion explicitly opts into full code layout, and collapse releases it. Plain
prose still uses full layout. The current code UI includes corner icons, a
translucent bottom Expand cue, and an overflow scrollbar; captures before these
changes are different workloads. These measurements do not time native input,
clipboard round trips, file pickers, GPU presentation, or settled idle resources.
