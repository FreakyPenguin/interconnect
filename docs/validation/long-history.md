# Long-history performance

Measure before choosing timeline virtualization or a loaded-history limit.
This opt-in fixture uses only local demo data, without creating SDK clients,
opening profiles, starting a window, or making network requests.

```sh
python3 scripts/measure-long-history.py > /tmp/interconnect-long-history.jsonl
```

By default this builds the optimized release test binary and measures 500,
2,000, and 10,000 events, three fresh processes per size, with 20 samples per
operation. `--events`, `--runs`, and `--samples` adjust the workload; `--debug`
uses the ordinary test profile for fixture development, not release conclusions.
Keep generated output and machine-specific artifacts outside the repository.

The fixture uses a 1280×800 window at scale 1 with bundled Inter fonts. It warms
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

Each record also reports the maximum mounted row count observed across rendered
frames and the final loaded row count. They distinguish bounded widget rendering
from retained SDK/model history. Small conversations up to 192 projected rows
use exact full layout; larger histories mount an overscanned window.

Scroll samples now include the corrective relayout following the native scroll
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

This isolates the UI's loaded-history scaling. It does not measure SDK diff
processing, encrypted storage, history pagination, network latency, unique-media
cache growth, thread summaries, GPU presentation, native input latency, idle
CPU/wakeups, battery use, or macOS desktop behavior. Full software redraws differ
from damage-limited redraws and native wgpu. Do not extrapolate absolute frame
times into a GPU frame-rate claim.

Use results to identify whether construction/layout/retained widget state grows
with history despite a fixed viewport. If growth is material, prototype bounded
rendering or virtualization and rerun this fixture before adopting it. Preserve
event-relative anchors, pagination, text selection, replies, thread summaries,
and read-receipt visibility; add connected/native checks before release.

## Individual enormous messages

Run each workload separately, using `--events` as paragraph repetitions, code
lines, or repeated wide-line columns respectively:

```sh
python3 scripts/measure-long-history.py --workload long-text --events 1000 10000
python3 scripts/measure-long-history.py --workload many-line-code --events 1000 10000
python3 scripts/measure-long-history.py --workload wide-code --events 1000 10000
```

These start with exactly one message, report its source byte count and workload
units, then run the same append, resize, and scroll operations. They expose costs
that row virtualization cannot bound. Source generation/parsing precedes timing;
initial layout is timed. Very wide lines and many-line code are separate cases.
Results describe headless software frames, not native input or GPU acceptance.
