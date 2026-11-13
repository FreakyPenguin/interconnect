#!/usr/bin/env python3
"""External fixed-window CPU and scheduler-wakeup capture; no app sampling timer."""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import time


def process_stat(pid):
    raw = Path(f"/proc/{pid}/stat").read_text()
    fields = raw[raw.rindex(")") + 2:].split()
    return {"ticks": int(fields[11]) + int(fields[12]), "start": fields[19]}


def tids(pid):
    return sorted(int(p.name) for p in Path(f"/proc/{pid}/task").iterdir())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pid", type=int)
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--cpu-only", action="store_true", help="explicit fallback; wakeups stay unmeasured")
    args = parser.parse_args()
    if args.pid <= 0 or args.seconds <= 0:
        parser.error("PID and duration must be positive")
    if not args.cpu_only and not shutil.which("perf"):
        parser.error("perf is unavailable; install host kernel perf tools or explicitly use --cpu-only")

    before = process_stat(args.pid)
    initial_tids = tids(args.pid)
    started = time.monotonic()
    perf = None
    if args.cpu_only:
        time.sleep(args.seconds)
    else:
        # sched_wakeup executes in the WAKER's context: perf -p would measure
        # the wrong side. System-wide collection filters the target TID field.
        target_filter = " || ".join(f"pid == {tid}" for tid in initial_tids)
        perf = subprocess.run([
            "perf", "stat", "-a", "-x", ";",
            "-e", "sched:sched_wakeup", "--filter", target_filter,
            "-e", "sched:sched_wakeup_new", "--filter", target_filter,
            "--", "sleep", str(args.seconds),
        ], text=True, capture_output=True)
    elapsed = time.monotonic() - started
    after = process_stat(args.pid)
    final_tids = tids(args.pid)
    if before["start"] != after["start"]:
        raise RuntimeError("PID was reused during capture")
    stable_threads = initial_tids == final_tids
    counts = {}
    if perf is not None:
        for line in perf.stderr.splitlines():
            fields = line.split(";")
            if len(fields) >= 3 and fields[2].strip() in ("sched:sched_wakeup", "sched:sched_wakeup_new"):
                try:
                    counts[fields[2].strip()] = int(fields[0].strip())
                except ValueError:
                    pass
    valid_capture = perf is not None and perf.returncode == 0 and stable_threads and len(counts) == 2
    result = {
        "pid": args.pid, "kernel": platform.release(), "machine": platform.machine(),
        "elapsed_seconds": elapsed,
        "cpu_seconds": (after["ticks"] - before["ticks"]) / os.sysconf("SC_CLK_TCK"),
        "initial_tids": initial_tids, "final_tids": final_tids,
        "thread_set_stable_at_endpoints": stable_threads,
        "wakeup_status": "unmeasured" if perf is None else "captured" if valid_capture else "invalid",
        "wakeup_counts": counts if valid_capture else None,
        "wakeups_per_second": sum(counts.values()) / elapsed if valid_capture else None,
        "perf_output": None if perf is None else perf.stderr,
        "limitations": "Wakeup filter covers initial threads only. Use a settled process; endpoint checks cannot detect transient threads. Counts are scheduler wakeups, not hardware CPU idle exits. CPU time is quantized to procfs clock ticks.",
    }
    print(json.dumps(result, indent=2))
    if perf is not None and not valid_capture:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
