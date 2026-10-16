#!/usr/bin/env python3
"""Compare frozen profiler binaries on two Linux CPUs, with drift/loss gates.

Run a same-binary control first by supplying the baseline for both arguments.
The caller must isolate resources and retain the builds' source provenance.
Fresh output directories preserve rejected runs for inspection.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

CASES = {
    "alloc-default": {},
    "alloc-default-2": {"MIMALLOC_BENCH_WORKERS": "2"},
    "alloc-dense": {
        "MIMALLOC_BENCH_SAMPLE_INTERVAL": "1024",
        "MIMALLOC_BENCH_MIN_SIZE": "65536",
        "MIMALLOC_BENCH_MAX_SIZE": "65536",
    },
    "live-default": {"MIMALLOC_BENCH_MODE": "live"},
}
ORDER = ("A1", "B1", "B2", "A2")


def run_workload(binary, environment):
    return subprocess.run(
        [str(binary)], env=environment, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, text=True, check=True,
    ).stdout


def metrics(output):
    data = dict(line.split("=", 1) for line in output.splitlines() if "=" in line)
    reported = int(data["reported_samples"])
    dropped = int(data["dropped_samples"])
    pending = int(data["buffered_samples"])
    total = reported + dropped + pending
    drop_fraction = dropped / total if total else 1.0
    quality = (
        total > 0 and drop_fraction <= 0.01 and pending == 0
        and int(data["dropped_live_samples"]) == 0
        and int(data["periodic_reports"]) >= 2
    )
    return {
        "throughput": float(data["allocations_per_sec"]),
        "drop_fraction": drop_fraction,
        "quality": quality,
        "report_max_us": int(data["report_max_elapsed_us"]),
        "latency_p99_ns": int(data.get("allocation_latency_p99_ns", 0)),
    }


def compare(rounds):
    a1, b1, b2, a2 = (rounds[label]["throughput"] for label in ORDER)
    drift_a, drift_b = abs(a2 / a1 - 1), abs(b2 / b1 - 1)
    gate = (
        max(drift_a, drift_b) <= 0.05
        and all(row["quality"] for row in rounds.values())
    )
    return {
        "baseline_drift": drift_a,
        "candidate_drift": drift_b,
        "gate": gate,
        "throughput_change": (b1 + b2) / (a1 + a2) - 1 if gate else None,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--duration-ms", type=int, default=8000)
    args = parser.parse_args()
    if args.duration_ms <= 0:
        parser.error("--duration-ms must be positive")
    if not hasattr(os, "sched_getaffinity"):
        parser.error("run on Linux with affinity restricted to two available CPUs")
    affinity = sorted(os.sched_getaffinity(0))
    if len(affinity) != 2:
        parser.error("restrict affinity to two available CPUs")
    if any(key.startswith("MIMALLOC_") for key in os.environ):
        parser.error("remove allocator and workload environment overrides")
    binaries = {"A": args.baseline.resolve(), "B": args.candidate.resolve()}
    if not all(path.is_file() for path in binaries.values()):
        parser.error("both frozen executables must exist")
    if args.output.exists():
        parser.error("choose a fresh output directory to preserve prior evidence")
    args.output.mkdir(parents=True)
    base_env = {
        "MIMALLOC_BENCH_MODE": "active",
        "MIMALLOC_BENCH_DURATION_MS": str(args.duration_ms),
        "MIMALLOC_BENCH_SAMPLE_INTERVAL": "1048576",
        "MIMALLOC_BENCH_STACK_CAPTURE": "portable",
        "MIMALLOC_BENCH_WORKERS": "1",
        "MIMALLOC_BENCH_REPORT_INTERVAL_MS": "25",
        "MIMALLOC_BENCH_RING_CAPACITY": "65536",
        "MIMALLOC_BENCH_REPORT_DRAIN_LIMIT": "131072",
    }
    report = {
        "affinity": affinity, "environment": base_env, "cases": {},
        "binaries": {
            label: {
                "path": str(path),
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            }
            for label, path in binaries.items()
        },
    }
    passed = True
    for name, overrides in CASES.items():
        environment = {**os.environ, **base_env, **overrides}
        warmup = {**environment, "MIMALLOC_BENCH_DURATION_MS": "1000"}
        for binary in binaries.values():
            run_workload(binary, warmup)
        rounds = {}
        for label in ORDER:
            output = run_workload(binaries[label[0]], environment)
            (args.output / f"{name}-{label}.log").write_text(output)
            rounds[label] = metrics(output)
        comparison = compare(rounds)
        report["cases"][name] = {
            "overrides": overrides, "rounds": rounds, **comparison,
        }
        (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
        print(name, json.dumps(comparison), flush=True)
        passed &= comparison["gate"]
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
