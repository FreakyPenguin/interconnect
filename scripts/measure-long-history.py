#!/usr/bin/env python3
"""Run isolated local timeline measurements; output JSON lines to stdout only."""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--events", type=int, nargs="+", default=[500, 2000, 10000])
    parser.add_argument("--workload", choices=["mixed", "long-text", "many-line-code", "wide-code"], default="mixed",
                        help="events for mixed; repetitions/lines/columns for individual messages")
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--debug", action="store_true", help="use the unoptimized test profile")
    args = parser.parse_args()
    if not 1 <= args.samples <= 1000 or not 1 <= args.runs <= 100:
        parser.error("samples must be 1..1000 and runs must be 1..100")
    if any(not 1 <= count <= 100000 for count in args.events):
        parser.error("event counts must be 1..100000")
    root = Path(__file__).resolve().parent.parent
    version = subprocess.check_output(["rustc", "--version"], cwd=root, text=True).strip()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True))
    cpu = platform.processor()
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        cpu = next((line.split(":", 1)[1].strip() for line in cpuinfo.read_text().splitlines()
                    if line.startswith("model name")), cpu)
    print(json.dumps({"platform": platform.platform(), "machine": platform.machine(),
                      "cpu": cpu, "logical_cpus": os.cpu_count(),
                      "rustc": version, "revision": revision, "dirty": dirty,
                      "renderer": "tiny-skia, headless, full rasterization, scale 1",
                      "profile": "test" if args.debug else "release"}), flush=True)
    command = ["cargo", "test", "--locked"]
    if not args.debug:
        command.append("--release")
    command.extend(["--bin", "interconnect", "long_history::measure", "--",
                    "--ignored", "--exact", "--nocapture", "--test-threads=1"])
    for count in args.events:
        for run in range(args.runs):
            print(f"Measuring {count} events, run {run + 1}/{args.runs}", file=sys.stderr, flush=True)
            env = dict(os.environ, INTERCONNECT_HISTORY_EVENTS=str(count),
                       INTERCONNECT_HISTORY_SAMPLES=str(args.samples),
                       INTERCONNECT_HISTORY_WORKLOAD=args.workload)
            result = subprocess.run(command, cwd=root, env=env, text=True,
                                    stdout=subprocess.PIPE, check=False)
            if result.returncode:
                sys.stderr.write(result.stdout)
                raise SystemExit(result.returncode)
            lines = [line.split("LONG_HISTORY ", 1)[1] for line in result.stdout.splitlines()
                     if "LONG_HISTORY " in line]
            if len(lines) != 1:
                sys.stderr.write(result.stdout)
                raise SystemExit("Expected exactly one measurement from the fixture")
            record = json.loads(lines[0])
            record["run"] = run + 1
            print(json.dumps(record), flush=True)


if __name__ == "__main__":
    main()
