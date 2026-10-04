#!/usr/bin/env python3
"""Compare default and no_profile builds in frozen A/B/B/A processes.

Estimates include full allocation/free operations. The cross-thread benchmark
also includes channel handoff and first-byte accesses. Reject conclusions for
any workload whose baseline or candidate drifts by more than the chosen limit.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

CASES = ["alloc_dealloc/mimalloc/64", "alloc_dealloc/mimalloc/4096",
         "alloc_dealloc/mimalloc/65536", "cross_thread_free_batch/mimalloc/4096"]
UNITS = {"ns": 1.0, "µs": 1000.0, "us": 1000.0, "ms": 1000000.0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("target/profile-abba"))
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--measurement-time", type=float, default=5)
    parser.add_argument("--warm-up-time", type=float, default=3)
    parser.add_argument("--sample-size", type=int, default=50)
    parser.add_argument("--drift-limit", type=float, default=0.05)
    args = parser.parse_args()
    if args.sample_size < 10 or min(args.measurement_time, args.warm_up_time) <= 0:
        parser.error("use at least 10 samples and positive measurement/warm-up times")
    if not 0 < args.drift_limit < 1:
        parser.error("drift-limit must be between 0 and 1")
    overrides = [k for k, v in os.environ.items()
                 if k.startswith("MIMALLOC_") or ("CFLAGS" in k and v)]
    if overrides:
        parser.error("start without allocator/C compiler overrides: " + ", ".join(overrides))
    root = Path(__file__).resolve().parents[1]
    output = (root / args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error("choose a fresh output directory to preserve previous evidence")
    report = {
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "mimalloc_commit": subprocess.check_output(["git", "-C", "rustfs-mimalloc-sys/c_src/mimalloc", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root)),
        "rustc": subprocess.check_output(["rustc", "--version"], cwd=root, text=True).strip(),
        "parameters": {"samples": args.sample_size, "warm_up_seconds": args.warm_up_time,
                       "measurement_seconds": args.measurement_time, "drift_limit": args.drift_limit,
                       "analysis_threads": 1},
        "builds": {}, "rounds": {},
    }
    executables = {}
    for label, features in [("A", []), ("B", ["no_profile"])]:
        command = ["cargo", "bench", "-p", "rustfs-mimalloc", "--bench", "alloc", "--no-run",
                   "--locked", "--message-format=json"]
        if args.offline:
            command.append("--offline")
        if features:
            command += ["--features", ",".join(features)]
        build = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, text=True, encoding="utf-8", check=True)
        artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
        artifact = next(a for a in artifacts if a.get("executable") and a.get("target", {}).get("name") == "alloc")
        destination = output / (label + (".exe" if os.name == "nt" else ""))
        shutil.copy2(artifact["executable"], destination)
        executables[label] = destination
        report["builds"][label] = {"features": artifact.get("features", []), "binary_bytes": destination.stat().st_size}
    env = os.environ.copy()
    env["RAYON_NUM_THREADS"] = "1"
    pattern = "^(" + "|".join(re.escape(case) for case in CASES) + ")$"
    for label in ["A1", "B1", "B2", "A2"]:
        result = subprocess.run([
            str(executables[label[0]]), "--bench", pattern, "--noplot",
            "--sample-size", str(args.sample_size), "--warm-up-time", str(args.warm_up_time),
            "--measurement-time", str(args.measurement_time),
        ], cwd=root, env=env, text=True, encoding="utf-8", stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (output / (label + ".log")).write_text(result.stdout, encoding="utf-8")
        if result.returncode:
            sys.exit("benchmark failed; inspect " + str(output / (label + ".log")))
        measured = {}
        for case in CASES:
            match = re.search(re.escape(case) + r"\s+time:\s+\[([0-9.]+) (ns|µs|us|ms) ([0-9.]+) (ns|µs|us|ms) ([0-9.]+) (ns|µs|us|ms)\]", result.stdout)
            if not match:
                sys.exit("missing estimate for " + case + "; inspect " + str(output / (label + ".log")))
            values = match.groups()
            measured[case] = dict(zip(("lower_ns", "estimate_ns", "upper_ns"),
                (float(values[i]) * UNITS[values[i + 1]] for i in (0, 2, 4))))
        report["rounds"][label] = measured
        print(label, measured, flush=True)
    report["comparisons"] = {}
    for case in CASES:
        a1, b1, b2, a2 = [report["rounds"][label][case]["estimate_ns"] for label in ["A1", "B1", "B2", "A2"]]
        baseline_drift, candidate_drift = abs(a2 / a1 - 1), abs(b2 / b1 - 1)
        passed = max(baseline_drift, candidate_drift) <= args.drift_limit
        report["comparisons"][case] = {
            "baseline_drift": baseline_drift, "candidate_drift": candidate_drift,
            "gate_passed": passed, "time_reduction": 1 - (b1 + b2) / (a1 + a2) if passed else None,
        }
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["comparisons"], indent=2), flush=True)
    if not all(v["gate_passed"] for v in report["comparisons"].values()):
        sys.exit("drift gate failed: do not attribute speedups for the failed workloads")


if __name__ == "__main__":
    main()
