//! SamplingMiMalloc allocation workload for local overhead comparisons.
//!
//! ```sh
//! cargo run --release --example mimalloc_overhead -p rustfs-mimalloc-profiling
//! MIMALLOC_BENCH_MODE=active cargo run --release --example mimalloc_overhead -p rustfs-mimalloc-profiling
//! ```

#[path = "mimalloc_benchmark/support.rs"]
mod support;

use rustfs_mimalloc_profiling::{
    MimallocConfig, MimallocStackCapture, Profiler, SamplingMiMalloc, mimalloc_stats,
};
use std::{
    sync::{Arc, Condvar, Mutex, mpsc},
    time::{Duration, Instant},
};
use support::{WorkloadConfig, print_workload, run_workload};

#[derive(Default)]
struct ReportMetrics {
    elapsed: Duration,
    maximum: Duration,
    encoded_bytes: usize,
    periodic_reports: u64,
}

impl ReportMetrics {
    fn collect(&mut self, backend: &mut Profiler) -> rustfs_mimalloc_profiling::Result<Vec<u8>> {
        let start = Instant::now();
        let report = backend.report()?;
        let elapsed = start.elapsed();
        self.elapsed += elapsed;
        self.maximum = self.maximum.max(elapsed);
        self.encoded_bytes = self.encoded_bytes.saturating_add(report.len());
        Ok(report)
    }
}

#[global_allocator]
static ALLOC: SamplingMiMalloc = SamplingMiMalloc::new();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = WorkloadConfig::from_env();
    let mode = std::env::var("MIMALLOC_BENCH_MODE").unwrap_or_else(|_| "inactive".to_string());

    if mode == "active" || mode == "live" {
        run_active(config, mode == "live")?;
    } else {
        let result = run_workload(config);
        print_workload("sampling_mimalloc_inactive", config, result);
    }

    Ok(())
}

fn run_active(config: WorkloadConfig, live_heap: bool) -> Result<(), Box<dyn std::error::Error>> {
    let report_interval =
        Duration::from_millis(read_env_u64("MIMALLOC_BENCH_REPORT_INTERVAL_MS", 0));
    let ring_capacity = read_env_usize("MIMALLOC_BENCH_RING_CAPACITY", 512);
    let report_drain_limit = read_env_usize("MIMALLOC_BENCH_REPORT_DRAIN_LIMIT", 1_000_000);
    let workers = read_env_usize("MIMALLOC_BENCH_WORKERS", 1);
    if !(1..=64).contains(&workers) || (workers > 1 && report_interval.is_zero()) {
        return Err(std::io::Error::other(
            "benchmark workers must be 1..64; multiple workers require periodic reporting",
        )
        .into());
    }
    let stack_capture = match std::env::var("MIMALLOC_BENCH_STACK_CAPTURE").as_deref() {
        Ok("native") => MimallocStackCapture::Native,
        Ok("portable") | Err(_) => MimallocStackCapture::Portable,
        Ok(_) => {
            return Err(std::io::Error::other(
                "unknown MIMALLOC_BENCH_STACK_CAPTURE; use portable or native",
            )
            .into());
        }
    };
    let mut backend = Profiler::start(MimallocConfig {
        sample_interval_bytes: read_env_u64("MIMALLOC_BENCH_SAMPLE_INTERVAL", 1024 * 1024),
        ring_capacity,
        report_drain_limit,
        stack_capture,
        live_heap_tracking: live_heap,
        max_live_samples: read_env_usize("MIMALLOC_BENCH_MAX_LIVE_SAMPLES", 16_384),
        ..MimallocConfig::default()
    })?;

    // Retain a working set through report generation while the timed workload
    // exercises allocations and frees. Its setup is outside throughput timing.
    let retained: Vec<Vec<u8>> = if live_heap {
        (0..read_env_usize("MIMALLOC_BENCH_LIVE_RETAINED_COUNT", 256))
            .map(|_| vec![0; read_env_usize("MIMALLOC_BENCH_LIVE_RETAINED_SIZE", 64 * 1024)])
            .collect()
    } else {
        Vec::new()
    };

    let mut metrics = ReportMetrics::default();
    let result = if report_interval.is_zero() {
        run_workload(config)
    } else {
        // The workload owns its TLS ring; this thread drains it concurrently,
        // just as the agent reporter does. Always join, including error paths.
        std::thread::scope(|scope| -> Result<_, Box<dyn std::error::Error>> {
            let (tx, rx) = mpsc::sync_channel(workers);
            let start_gate = Arc::new((Mutex::new(None::<bool>), Condvar::new()));
            let mut handles = Vec::with_capacity(workers);
            for index in 0..workers {
                let tx = tx.clone();
                let gate = Arc::clone(&start_gate);
                let handle = std::thread::Builder::new().spawn_scoped(scope, move || {
                    let mut config = config;
                    if index != 0 {
                        config.latency_sample_interval = 0;
                    }
                    let started = gate
                        .1
                        .wait_while(gate.0.lock().unwrap(), |state| state.is_none())
                        .unwrap();
                    if *started != Some(true) {
                        return;
                    }
                    drop(started);
                    let result = run_workload(config);
                    let _ = tx.send((index, result));
                });
                match handle {
                    Ok(handle) => handles.push(handle),
                    Err(error) => {
                        // Release already-created workers without running the
                        // workload; a failed spawn must not strand a barrier.
                        *start_gate.0.lock().unwrap() = Some(false);
                        start_gate.1.notify_all();
                        return Err(error.into());
                    }
                }
            }
            drop(tx);
            let start = Instant::now();
            *start_gate.0.lock().unwrap() = Some(true);
            start_gate.1.notify_all();
            let mut remaining = workers;
            let mut first = None;
            let mut allocations = 0_u64;
            let mut bytes = 0_u64;
            while remaining > 0 {
                match rx.recv_timeout(report_interval) {
                    Ok((index, result)) => {
                        allocations = allocations.saturating_add(result.allocations);
                        bytes = bytes.saturating_add(result.bytes);
                        if index == 0 {
                            first = Some(result);
                        }
                        remaining -= 1;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        metrics.collect(&mut backend)?;
                        metrics.periodic_reports += 1;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        for worker in handles {
                            worker.join().map_err(|_| {
                                std::io::Error::other("benchmark workload panicked")
                            })?;
                        }
                        return Err(std::io::Error::other("benchmark workload disconnected").into());
                    }
                }
            }
            let elapsed = start.elapsed();
            for worker in handles {
                worker
                    .join()
                    .map_err(|_| std::io::Error::other("benchmark workload panicked"))?;
            }
            let mut result =
                first.ok_or_else(|| std::io::Error::other("benchmark first worker missing"))?;
            result.allocations = allocations;
            result.bytes = bytes;
            result.elapsed = elapsed;
            Ok(result)
        })?
    };
    let mut report = metrics.collect(&mut backend)?;
    // Respect small drain limits while bounding a misconfigured final drain.
    for _ in 0..1024 {
        if mimalloc_stats().buffered_samples == Some(0) {
            break;
        }
        report = metrics.collect(&mut backend)?;
    }
    let stats = mimalloc_stats();

    std::hint::black_box(&retained);
    print_workload(
        if live_heap {
            "sampling_mimalloc_live"
        } else {
            "sampling_mimalloc_active"
        },
        config,
        result,
    );
    println!(
        "sample_interval_bytes={}",
        read_env_u64("MIMALLOC_BENCH_SAMPLE_INTERVAL", 1024 * 1024)
    );
    println!("recorded_samples={}", stats.recorded_samples);
    println!("reported_samples={}", stats.reported_samples);
    println!("reports={}", stats.reports);
    println!("periodic_reports={}", metrics.periodic_reports);
    println!("report_interval_ms={}", report_interval.as_millis());
    println!("ring_capacity={ring_capacity}");
    println!("workers={workers}");
    println!("latency_scope=first_worker");
    println!("stack_capture={stack_capture:?}");
    println!("report_drain_limit={report_drain_limit}");
    println!("flushes={}", stats.flushes);
    println!("flushed_samples={}", stats.flushed_samples);
    println!("dropped_samples={}", stats.dropped_samples);
    println!("live_samples={}", stats.live_samples);
    println!("dropped_live_samples={}", stats.dropped_live_samples);
    println!(
        "live_metadata_payload_bytes={}",
        stats.live_metadata_payload_bytes
    );
    println!(
        "buffered_samples={}",
        stats
            .buffered_samples
            .map(|samples| samples.to_string())
            .unwrap_or_else(|| "locked".to_string())
    );
    println!("report_elapsed_ms={}", metrics.elapsed.as_millis());
    println!("report_max_elapsed_us={}", metrics.maximum.as_micros());
    println!("encoded_pprof_bytes={}", metrics.encoded_bytes);
    println!(
        "pprof_encode_elapsed_us={}",
        stats.last_pprof_encode_elapsed_micros
    );

    backend.stop();
    if let Some(path) = std::env::var_os("MIMALLOC_BENCH_PPROF_PATH") {
        use std::io::Write;
        // Export after shutdown and outside both throughput and report timing.
        let file = std::fs::File::create(path)?;
        let mut gzip = libflate::gzip::Encoder::new(file)?;
        gzip.write_all(&report)?;
        gzip.finish().into_result()?;
    }
    Ok(())
}

fn read_env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn read_env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
