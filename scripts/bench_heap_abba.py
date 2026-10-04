#!/usr/bin/env python3
"""Run fixed-order A/B/B/A heap lookup comparisons in separate processes.

A = Heap::malloc_aligned; B = ThreadHeap::malloc_aligned.
Each measured iteration alternates between two heaps (two alloc/free pairs).
A drift or B repeatability failure invalidates the performance conclusion.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="build from cached Cargo dependencies")
    parser.add_argument("--output", type=Path, default=Path("target/heap-abba"))
    parser.add_argument("--measurement-time", type=float, default=5)
    parser.add_argument("--warm-up-time", type=float, default=3)
    parser.add_argument("--sample-size", type=int, default=50)
    parser.add_argument("--drift-limit", type=float, default=0.05)
    args = parser.parse_args()
    if args.sample_size < 10 or min(args.measurement_time, args.warm_up_time) <= 0:
        parser.error("use at least 10 samples and positive measurement/warm-up times")
    if not 0 < args.drift_limit < 1:
        parser.error("drift-limit must be between 0 and 1")
    tuning = [key for key in os.environ if key.startswith("MIMALLOC_")]
    if tuning:
        parser.error("start without allocator tuning variables: " + ", ".join(tuning))

    root = Path(__file__).resolve().parents[1]
    output = (root / args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error("choose a fresh output directory to preserve earlier measurements")
    build = subprocess.run([
        "cargo", "bench", "-p", "rustfs-mimalloc", "--bench", "alloc", "--no-run",
        "--locked", "--message-format=json",
        *(["--offline"] if args.offline else []),
    ], cwd=root, text=True, stdout=subprocess.PIPE, check=True)
    artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
    artifact = next(item for item in artifacts if item.get("executable") and item.get("target", {}).get("name") == "alloc")
    exe = artifact["executable"]
    benchmark_env = os.environ.copy()
    # Criterion uses Rayon for statistical resampling, outside the measured loop.
    # Keep that analysis from heating all cores between neighboring benchmarks.
    benchmark_env["RAYON_NUM_THREADS"] = "1"
    report = {
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "mimalloc_commit": subprocess.check_output(["git", "-C", "rustfs-mimalloc-sys/c_src/mimalloc", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root)),
        "rustc": subprocess.check_output(["rustc", "--version"], cwd=root, text=True).strip(),
        "parameters": {"measurement_seconds": args.measurement_time, "warm_up_seconds": args.warm_up_time,
                       "samples": args.sample_size, "drift_limit": args.drift_limit, "analysis_threads": 1},
        "features": artifact.get("features", []),
        "rounds": {},
    }
    # Exact filters keep the same workload in every process. Do not run other
    # builds or measurements concurrently with this script.
    for label, variant in [("A1", "lookup"), ("B1", "cached"), ("B2", "cached"), ("A2", "lookup")]:
        command = [exe, "--bench", "^heap_switching/" + variant + "/(64|4096)$", "--noplot",
                   "--sample-size", str(args.sample_size), "--measurement-time", str(args.measurement_time),
                   "--warm-up-time", str(args.warm_up_time)]
        result = subprocess.run(command, cwd=root, env=benchmark_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (output / (label + ".log")).write_text(result.stdout)
        if result.returncode:
            sys.exit("benchmark failed; inspect " + str(output / (label + ".log")))
        results = {}
        for size in (64, 4096):
            match = re.search(r"heap_switching/" + variant + "/" + str(size) +
                              r"\s+time:\s+\[([0-9.]+) ns ([0-9.]+) ns ([0-9.]+) ns\]", result.stdout)
            if not match:
                sys.exit("missing nanosecond estimate; inspect " + str(output / (label + ".log")))
            results[str(size)] = dict(zip(("lower_ns", "estimate_ns", "upper_ns"), map(float, match.groups())))
        report["rounds"][label] = results
        print(label, results, flush=True)

    report["comparisons"] = {}
    for size in ("64", "4096"):
        a1, b1, b2, a2 = [report["rounds"][label][size]["estimate_ns"] for label in ("A1", "B1", "B2", "A2")]
        drift = abs(a2 / a1 - 1)
        repeat = abs(b2 / b1 - 1)
        passed = drift <= args.drift_limit and repeat <= args.drift_limit
        report["comparisons"][size] = {
            "baseline_drift": drift, "candidate_drift": repeat, "gate_passed": passed,
            "time_reduction": 1 - (b1 + b2) / (a1 + a2) if passed else None,
        }
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["comparisons"], indent=2), flush=True)
    if not all(item["gate_passed"] for item in report["comparisons"].values()):
        sys.exit("drift gate failed: keep the measurements, make no speedup claim")


if __name__ == "__main__":
    main()
