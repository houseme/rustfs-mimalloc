#!/usr/bin/env bash
set -euo pipefail

output_dir="${MIMALLOC_BENCH_OUTPUT_DIR:-target/mimalloc-benchmark}"
report_path="${MIMALLOC_BENCH_REPORT:-${output_dir}/mimalloc-benchmark-report.md}"
history_dir="${MIMALLOC_BENCH_HISTORY_DIR:-${output_dir}/history}"
history_path="${MIMALLOC_BENCH_HISTORY:-${history_dir}/mimalloc-benchmark-history-v4.csv}"
history_header="timestamp_utc,run_id,run_attempt,commit,scenario,sample_interval_bytes,mib_per_sec,allocations_per_sec,overhead_vs_baseline_pct,recorded_samples,flushes,dropped_samples,report_elapsed_ms,encoded_pprof_bytes,pprof_encode_elapsed_us,allocation_latency_p50_ns,allocation_latency_p95_ns,allocation_latency_p99_ns,status,latency_sampling_policy,allocation_latency_samples,allocation_latency_min_size,allocation_latency_max_size,report_interval_ms,ring_capacity,report_drain_limit,reports,reported_samples,periodic_reports,buffered_samples,dropped_live_samples,allocation_record_drop_pct,quality_status,report_max_elapsed_us,stack_capture,workers"
enforce_thresholds="${MIMALLOC_BENCH_ENFORCE_THRESHOLDS:-0}"
enforce_quality="${MIMALLOC_BENCH_ENFORCE_QUALITY:-0}"
input_dir="${MIMALLOC_BENCH_INPUT_DIR:-}"

: "${MIMALLOC_BENCH_DURATION_MS:=3000}"
: "${MIMALLOC_BENCH_BATCH_SIZE:=1024}"
: "${MIMALLOC_BENCH_MIN_SIZE:=64}"
: "${MIMALLOC_BENCH_MAX_SIZE:=65536}"
: "${MIMALLOC_BENCH_SIZE_STEP:=64}"
: "${MIMALLOC_BENCH_LATENCY_SAMPLE_INTERVAL:=1024}"
: "${MIMALLOC_BENCH_LATENCY_SAMPLE_LIMIT:=4096}"
: "${MIMALLOC_BENCH_INACTIVE_MAX_OVERHEAD_PCT:=2}"
: "${MIMALLOC_BENCH_ACTIVE_1M_MAX_OVERHEAD_PCT:=5}"
: "${MIMALLOC_BENCH_MAX_DROP_PCT:=1}"
: "${MIMALLOC_BENCH_STEADY_REPORT_INTERVAL_MS:=50}"
: "${MIMALLOC_BENCH_STEADY_RING_CAPACITY:=16384}"

if ! awk -v value="$MIMALLOC_BENCH_MAX_DROP_PCT" 'BEGIN { exit !(value ~ /^[0-9]+([.][0-9]+)?$/ && value <= 100) }'; then
    echo "invalid MIMALLOC_BENCH_MAX_DROP_PCT: expected 0..100" >&2
    exit 1
fi
if [[ ! "$MIMALLOC_BENCH_STEADY_REPORT_INTERVAL_MS" =~ ^[0-9]+$ ]] || [ "$MIMALLOC_BENCH_STEADY_REPORT_INTERVAL_MS" -eq 0 ]; then
    echo "steady report interval must be a positive integer" >&2
    exit 1
fi

export MIMALLOC_BENCH_DURATION_MS
export MIMALLOC_BENCH_BATCH_SIZE
export MIMALLOC_BENCH_MIN_SIZE
export MIMALLOC_BENCH_MAX_SIZE
export MIMALLOC_BENCH_SIZE_STEP
export MIMALLOC_BENCH_LATENCY_SAMPLE_INTERVAL
export MIMALLOC_BENCH_LATENCY_SAMPLE_LIMIT
# The bare allocator baseline is single-worker; do not compare it to a
# multi-worker profiler run. Multi-worker A/B runs use the overhead example directly.
export MIMALLOC_BENCH_WORKERS=1

mkdir -p "$output_dir"
mkdir -p "$history_dir"
history_timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
history_run_id="${GITHUB_RUN_ID:-local}"
history_run_attempt="${GITHUB_RUN_ATTEMPT:-1}"
history_commit="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
if ! git diff --quiet --ignore-submodules -- || ! git diff --cached --quiet --ignore-submodules --; then
    history_commit="${history_commit}-dirty"
fi
if [ -n "$input_dir" ]; then
    # Replayed inputs do not establish the revision that produced measurements.
    history_commit="replay-unknown"
fi

ensure_history_header() {
    if [ -f "$history_path" ]; then
        if ! IFS= read -r existing_header < "$history_path" || [ "$existing_header" != "$history_header" ]; then
            echo "incompatible benchmark history schema: $history_path; use a new v4 history file" >&2
            exit 1
        fi
        return
    fi

    echo "$history_header" > "$history_path"
}

metric() {
    awk -F= -v key="$2" '$1 == key { print $2; found = 1; exit } END { if (!found) exit 1 }' "$1"
}

metric_or_default() {
    awk -F= -v key="$2" -v default_value="$3" \
        '$1 == key { print $2; found = 1; exit } END { if (!found) print default_value }' "$1"
}

overhead_percent() {
    awk -v baseline="$1" -v current="$2" 'BEGIN {
        if (baseline <= 0) {
            print "nan";
        } else {
            printf "%.2f", ((baseline - current) / baseline) * 100.0;
        }
    }'
}

is_over_threshold() {
    awk -v value="$1" -v threshold="$2" 'BEGIN {
        if (value == "nan") exit 1;
        exit !(value > threshold);
    }'
}

run_baseline() {
    cargo run --locked --release --quiet --example mimalloc_baseline -p rustfs-mimalloc-profiling \
        > "${output_dir}/baseline.env"
}

run_inactive() {
    MIMALLOC_BENCH_MODE=inactive \
        cargo run --locked --release --quiet --example mimalloc_overhead -p rustfs-mimalloc-profiling \
        > "${output_dir}/inactive.env"
}

run_active() {
    local name="$1"
    local interval_bytes="$2"
    local mode="${3:-active}"
    local report_interval="${4:-0}"
    local ring_capacity="${5:-${MIMALLOC_BENCH_RING_CAPACITY:-512}}"

    MIMALLOC_BENCH_MODE="$mode" \
    MIMALLOC_BENCH_SAMPLE_INTERVAL="$interval_bytes" \
    MIMALLOC_BENCH_REPORT_INTERVAL_MS="$report_interval" \
    MIMALLOC_BENCH_RING_CAPACITY="$ring_capacity" \
        cargo run --locked --release --quiet --example mimalloc_overhead -p rustfs-mimalloc-profiling \
        > "${output_dir}/${name}.env"
}

record_drop_percent() {
    awk -v reported="$1" -v dropped="$2" -v pending="$3" 'BEGIN {
        if (reported !~ /^[0-9]+$/ || dropped !~ /^[0-9]+$/ || pending !~ /^[0-9]+$/) { print "nan"; exit }
        total = reported + dropped + pending;
        if (total == 0) print "nan";
        else printf "%.6f", 100 * dropped / total;
    }'
}

append_row() {
    local scenario="$1"
    local file="$2"
    local baseline_mib_per_sec="$3"
    local threshold="$4"
    local status="$5"
    local sample_interval
    local mib_per_sec
    local allocations_per_sec
    local overhead
    local recorded_samples
    local flushes
    local dropped_samples
    local report_elapsed_ms
    local encoded_pprof_bytes
    local pprof_encode_elapsed_us
    local allocation_latency_p50_ns
    local allocation_latency_p95_ns
    local allocation_latency_p99_ns
    local latency_sampling_policy
    local allocation_latency_samples
    local allocation_latency_min_size
    local allocation_latency_max_size
    local report_interval ring_capacity report_drain_limit reports reported_samples periodic_reports
    local buffered_samples dropped_live_samples record_drop_pct quality_status report_max_elapsed_us stack_capture workers

    sample_interval="$(metric_or_default "$file" sample_interval_bytes "-")"
    mib_per_sec="$(metric "$file" mib_per_sec)"
    allocations_per_sec="$(metric "$file" allocations_per_sec)"
    recorded_samples="$(metric_or_default "$file" recorded_samples "-")"
    flushes="$(metric_or_default "$file" flushes "-")"
    dropped_samples="$(metric_or_default "$file" dropped_samples "-")"
    report_elapsed_ms="$(metric_or_default "$file" report_elapsed_ms "-")"
    encoded_pprof_bytes="$(metric_or_default "$file" encoded_pprof_bytes "-")"
    pprof_encode_elapsed_us="$(metric_or_default "$file" pprof_encode_elapsed_us "-")"
    allocation_latency_p50_ns="$(metric_or_default "$file" allocation_latency_p50_ns "-")"
    allocation_latency_p95_ns="$(metric_or_default "$file" allocation_latency_p95_ns "-")"
    allocation_latency_p99_ns="$(metric_or_default "$file" allocation_latency_p99_ns "-")"
    latency_sampling_policy="$(metric "$file" latency_sampling_policy)"
    allocation_latency_samples="$(metric "$file" allocation_latency_samples)"
    allocation_latency_min_size="$(metric "$file" allocation_latency_min_size)"
    allocation_latency_max_size="$(metric "$file" allocation_latency_max_size)"
    report_interval="$(metric_or_default "$file" report_interval_ms "-")"
    ring_capacity="$(metric_or_default "$file" ring_capacity "-")"
    report_drain_limit="$(metric_or_default "$file" report_drain_limit "-")"
    reports="$(metric_or_default "$file" reports "-")"
    reported_samples="$(metric_or_default "$file" reported_samples "-")"
    periodic_reports="$(metric_or_default "$file" periodic_reports "-")"
    buffered_samples="$(metric_or_default "$file" buffered_samples "unknown")"
    dropped_live_samples="$(metric_or_default "$file" dropped_live_samples "-")"
    report_max_elapsed_us="$(metric_or_default "$file" report_max_elapsed_us "-")"
    stack_capture="$(metric_or_default "$file" stack_capture "unknown")"
    workers="$(metric_or_default "$file" workers "1")"
    record_drop_pct="-"
    quality_status="N/A"
    if [ "$scenario" != "baseline" ] && [ "$scenario" != "inactive" ]; then
        record_drop_pct="$(record_drop_percent "$reported_samples" "$dropped_samples" "$buffered_samples")"
        quality_status="PASS"
        if [ "$record_drop_pct" = "nan" ] || [[ ! "$reports" =~ ^[0-9]+$ ]] || [ "$reports" = "0" ] || [[ ! "$dropped_live_samples" =~ ^[0-9]+$ ]]; then
            quality_status="UNKNOWN_OR_EMPTY"
        elif [ "$buffered_samples" != "0" ]; then
            quality_status="PENDING"
        elif is_over_threshold "$record_drop_pct" "$MIMALLOC_BENCH_MAX_DROP_PCT"; then
            quality_status="LOSSY"
        elif [ "$dropped_live_samples" != "0" ]; then
            quality_status="LIVE_DROPS"
        elif [[ "$scenario" == steady-* ]]; then
            if [[ ! "$periodic_reports" =~ ^[0-9]+$ ]] || [ "$periodic_reports" = "0" ] || [[ ! "$report_interval" =~ ^[0-9]+$ ]] || [ "$report_interval" = "0" ]; then
                quality_status="NO_PERIODIC_REPORTS"
            fi
        fi
    fi

    if [ "$scenario" = "baseline" ]; then
        overhead="0.00"
    else
        overhead="$(overhead_percent "$baseline_mib_per_sec" "$mib_per_sec")"
    fi

    if [ -n "$threshold" ] && is_over_threshold "$overhead" "$threshold"; then
        if [ "$enforce_thresholds" = "1" ]; then
            status="FAIL"
            failures=$((failures + 1))
        else
            status="WARN"
        fi
    fi
    if [ "$overhead" = "nan" ]; then
        status="INVALID_BASELINE"
        if [ "$enforce_thresholds" = "1" ]; then
            failures=$((failures + 1))
        fi
    fi
    if [ "$quality_status" != "PASS" ] && [ "$quality_status" != "N/A" ]; then
        if [[ "$scenario" == steady-* ]] && [ "$enforce_quality" = "1" ]; then
            status="FAIL_QUALITY"
            failures=$((failures + 1))
        elif [ "$status" != "FAIL" ] && [ "$status" != "INVALID_BASELINE" ]; then
            status="QUALITY_WARN"
        fi
    fi

    printf '| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n' \
        "$scenario" \
        "$sample_interval" \
        "$mib_per_sec" \
        "$allocations_per_sec" \
        "$overhead" \
        "$recorded_samples" \
        "$flushes" \
        "$dropped_samples" \
        "$report_elapsed_ms" \
        "$encoded_pprof_bytes" \
        "$pprof_encode_elapsed_us" \
        "$allocation_latency_p50_ns" \
        "$allocation_latency_p95_ns" \
        "$allocation_latency_p99_ns" \
        "$record_drop_pct" \
        "$quality_status" \
        "$status" >> "$report_path"

    printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
        "$history_timestamp" \
        "$history_run_id" \
        "$history_run_attempt" \
        "$history_commit" \
        "$scenario" \
        "$sample_interval" \
        "$mib_per_sec" \
        "$allocations_per_sec" \
        "$overhead" \
        "$recorded_samples" \
        "$flushes" \
        "$dropped_samples" \
        "$report_elapsed_ms" \
        "$encoded_pprof_bytes" \
        "$pprof_encode_elapsed_us" \
        "$allocation_latency_p50_ns" \
        "$allocation_latency_p95_ns" \
        "$allocation_latency_p99_ns" \
        "$status" \
        "$latency_sampling_policy" \
        "$allocation_latency_samples" \
        "$allocation_latency_min_size" \
        "$allocation_latency_max_size" \
        "$report_interval" "$ring_capacity" "$report_drain_limit" "$reports" "$reported_samples" \
        "$periodic_reports" "$buffered_samples" "$dropped_live_samples" \
        "$record_drop_pct" "$quality_status" "$report_max_elapsed_us" "$stack_capture" "$workers" >> "$history_path"
}

ensure_history_header
if [ -n "$input_dir" ]; then
    for scenario in baseline inactive active-1m active-512k active-4k live-1m live-512k live-4k steady-active-1m steady-live-1m; do
        if [ "$input_dir" != "$output_dir" ]; then
            cp "${input_dir}/${scenario}.env" "${output_dir}/${scenario}.env"
        fi
    done
else
    run_baseline
    run_inactive
    run_active active-1m 1048576
    run_active active-512k 524288
    run_active active-4k 4096
    run_active live-1m 1048576 live
    run_active live-512k 524288 live
    run_active live-4k 4096 live
    run_active steady-active-1m 1048576 active "$MIMALLOC_BENCH_STEADY_REPORT_INTERVAL_MS" "$MIMALLOC_BENCH_STEADY_RING_CAPACITY"
    run_active steady-live-1m 1048576 live "$MIMALLOC_BENCH_STEADY_REPORT_INTERVAL_MS" "$MIMALLOC_BENCH_STEADY_RING_CAPACITY"
fi

failures=0
baseline_mib_per_sec="$(metric "${output_dir}/baseline.env" mib_per_sec)"

{
    echo "# Mimalloc Benchmark Report"
    echo
    echo "Generated by \`scripts/mimalloc_benchmark_report.sh\`."
    echo
    echo "## Workload"
    echo
    echo "- duration_ms: ${MIMALLOC_BENCH_DURATION_MS}"
    echo "- batch_size: ${MIMALLOC_BENCH_BATCH_SIZE}"
    echo "- min_size: ${MIMALLOC_BENCH_MIN_SIZE}"
    echo "- max_size: ${MIMALLOC_BENCH_MAX_SIZE}"
    echo "- size_step: ${MIMALLOC_BENCH_SIZE_STEP}"
    echo "- latency_sample_interval: ${MIMALLOC_BENCH_LATENCY_SAMPLE_INTERVAL}"
    echo "- latency_sample_limit: ${MIMALLOC_BENCH_LATENCY_SAMPLE_LIMIT}"
    echo "- enforce_thresholds: ${enforce_thresholds}"
    echo "- enforce_quality: ${enforce_quality}"
    echo
    echo "## Thresholds"
    echo
    echo "- inactive overhead <= ${MIMALLOC_BENCH_INACTIVE_MAX_OVERHEAD_PCT}%"
    echo "- steady-active 1 MiB overhead <= ${MIMALLOC_BENCH_ACTIVE_1M_MAX_OVERHEAD_PCT}%"
    echo "- steady allocation-record drops <= ${MIMALLOC_BENCH_MAX_DROP_PCT}%, zero pending records, zero live drops, and at least one periodic report."
    echo "- Post-workload reports are pressure diagnostics, not lossless profiling overhead."
    echo "- active 512 KiB and 4 KiB are diagnostic rows by default."
    echo
    echo "## Results"
    echo
    echo "| scenario | sample_interval_bytes | MiB/s | allocations/s | overhead_vs_baseline_% | recorded_samples | flushes | dropped_samples | report_elapsed_ms | encoded_pprof_bytes | pprof_encode_elapsed_us | allocation_latency_p50_ns | allocation_latency_p95_ns | allocation_latency_p99_ns | record_drop_% | quality | status |"
    echo "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |"
} > "$report_path"

append_row "baseline" "${output_dir}/baseline.env" "$baseline_mib_per_sec" "" "BASELINE"
append_row "inactive" "${output_dir}/inactive.env" "$baseline_mib_per_sec" "$MIMALLOC_BENCH_INACTIVE_MAX_OVERHEAD_PCT" "PASS"
append_row "active-1m" "${output_dir}/active-1m.env" "$baseline_mib_per_sec" "" "PRESSURE"
append_row "active-512k" "${output_dir}/active-512k.env" "$baseline_mib_per_sec" "" "INFO"
append_row "active-4k" "${output_dir}/active-4k.env" "$baseline_mib_per_sec" "" "DIAGNOSTIC"
append_row "live-1m" "${output_dir}/live-1m.env" "$baseline_mib_per_sec" "" "DIAGNOSTIC"
append_row "live-512k" "${output_dir}/live-512k.env" "$baseline_mib_per_sec" "" "DIAGNOSTIC"
append_row "live-4k" "${output_dir}/live-4k.env" "$baseline_mib_per_sec" "" "DIAGNOSTIC"
append_row "steady-active-1m" "${output_dir}/steady-active-1m.env" "$baseline_mib_per_sec" "$MIMALLOC_BENCH_ACTIVE_1M_MAX_OVERHEAD_PCT" "PASS"
append_row "steady-live-1m" "${output_dir}/steady-live-1m.env" "$baseline_mib_per_sec" "" "DIAGNOSTIC"

{
    echo
    echo "## Raw Output"
    echo
    echo "Raw key-value outputs are saved next to this report:"
    echo
    echo "- baseline.env"
    echo "- inactive.env"
    echo "- active-1m.env"
    echo "- active-512k.env"
    echo "- active-4k.env"
    echo "- live-1m.env"
    echo "- live-512k.env"
    echo "- live-4k.env"
    echo "- steady-active-1m.env"
    echo "- steady-live-1m.env"
    echo "- history/mimalloc-benchmark-history-v4.csv"
    echo
    echo "Live heap raw outputs include live_samples, dropped_live_samples, and live_metadata_payload_bytes."
    echo "Payload bytes exclude hash control bytes, shard headers, and allocator bookkeeping."
    echo "Latency samples use stratified_v1: one pseudorandom allocation per window."
    echo "Raw outputs and v4 history include report cadence, queue/drain capacity, completed reports, drained records, pending records, live drops, quality status, and stack capture engine."
    echo "Drop rate is dropped / (reported + pending + dropped), measured after the workload joins; raw record counts are not weighted-byte coverage."
    echo "Replay mode uses MIMALLOC_BENCH_INPUT_DIR and does not run workloads."
    echo "Steady report duration and encoded bytes are totals over all reports; pprof_encode_elapsed_us remains the last report's encoding time."
    echo "Legacy history files are preserved; their fixed-cadence percentiles are not directly comparable."
} >> "$report_path"

cat "$report_path"

if [ "$failures" -gt 0 ]; then
    echo "mimalloc benchmark threshold failures: ${failures}" >&2
    exit 1
fi
