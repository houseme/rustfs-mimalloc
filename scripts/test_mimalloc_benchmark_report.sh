#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/inputs"

fixture() {
    local scenario="$1" reported="$2" dropped="$3" pending="$4" periodic="$5" live_drops="$6"
    printf '%s\n' \
        'mib_per_sec=100' 'allocations_per_sec=1000' 'latency_sampling_policy=stratified_v1' \
        'allocation_latency_samples=10' 'allocation_latency_min_size=64' 'allocation_latency_max_size=65536' \
        'sample_interval_bytes=1048576' 'recorded_samples=1000' 'reports=4' \
        "reported_samples=$reported" "dropped_samples=$dropped" "buffered_samples=$pending" \
        "periodic_reports=$periodic" "dropped_live_samples=$live_drops" 'report_interval_ms=50' 'stack_capture=Portable' \
        > "$tmp/inputs/$scenario.env"
}

for scenario in baseline inactive active-1m active-512k active-4k live-1m live-512k live-4k steady-active-1m steady-live-1m; do
    fixture "$scenario" 1000 0 0 3 0
done
fixture active-1m 10 990 0 0 0
fixture active-512k 0 0 0 0 0
fixture active-4k 1000 0 locked 0 0
fixture live-1m 1000 0 5 0 0
fixture live-512k 1000 0 0 0 1
fixture live-4k 1000 0 0 0 0

run_report() {
    MIMALLOC_BENCH_INPUT_DIR="$tmp/inputs" MIMALLOC_BENCH_OUTPUT_DIR="$tmp/output" \
        MIMALLOC_BENCH_ENFORCE_QUALITY=1 bash "$root/scripts/mimalloc_benchmark_report.sh" > "$tmp/report.txt"
}

run_report
history="$tmp/output/history/mimalloc-benchmark-history-v4.csv"
awk -F, 'NF != 36 { exit 1 } END { if (NR != 11) exit 1 }' "$history"
awk -F, '$5 == "steady-active-1m" && $35 == "Portable" { good=1 } END { exit !good }' "$history"
awk -F, '$5 == "active-1m" && $19 == "QUALITY_WARN" && $32 == "99.000000" && $33 == "LOSSY" { good=1 } END { exit !good }' "$history"
for quality in UNKNOWN_OR_EMPTY PENDING LIVE_DROPS; do
    awk -F, -v expected="$quality" '$33 == expected { good=1 } END { exit !good }' "$history"
done
awk -F, '$4 == "replay-unknown" && $5 == "steady-active-1m" { good=1 } END { exit !good }' "$history"

fixture steady-active-1m 10 990 0 3 0
if run_report; then
    echo "lossy steady row incorrectly passed" >&2
    exit 1
fi
awk -F, '$19 == "FAIL_QUALITY" && $33 == "LOSSY" { good=1 } END { exit !good }' "$history"

fixture steady-active-1m 1000 0 0 unknown 0
if run_report; then
    echo "missing periodic report incorrectly passed" >&2
    exit 1
fi
awk -F, '$33 == "NO_PERIODIC_REPORTS" { good=1 } END { exit !good }' "$history"

printf '%s\n' 'old,v2,header' > "$tmp/old-history.csv"
if MIMALLOC_BENCH_HISTORY="$tmp/old-history.csv" run_report; then
    echo "old history schema incorrectly accepted" >&2
    exit 1
fi
test "$(head -n 1 "$tmp/old-history.csv")" = 'old,v2,header'
echo 'mimalloc benchmark quality classification tests passed'
