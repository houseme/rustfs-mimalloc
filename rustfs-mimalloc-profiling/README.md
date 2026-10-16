# rustfs-mimalloc-profiling

Sampled allocation and optional live heap profiles for Rust programs using
mimalloc. This companion crate owns the sampling allocator, recorder, live
allocation lifecycle, symbolization, executable mappings, and pprof encoder.
The base `rustfs-mimalloc` crate does not depend on it.

## Usage

```toml
[dependencies]
rustfs-mimalloc-profiling = "0.6.0"
```

The profiling crate shares version 0.6.0 with the allocator and sys crates.
It uses the allocator's `GlobalAlloc` interface and adds no dependencies to
programs that use only the base allocator.

```rust
use rustfs_mimalloc_profiling::{MimallocConfig, Profiler, SamplingMiMalloc};

#[global_allocator]
static ALLOC: SamplingMiMalloc = SamplingMiMalloc::new();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut profiler = Profiler::start(MimallocConfig {
        live_heap_tracking: true,
        ..MimallocConfig::default()
    })?;
    let retained = vec![0_u8; 2 * 1024 * 1024];
    std::hint::black_box(&retained);
    let bytes = profiler.report()?;
    profiler.stop();
    std::fs::write("heap.pprof", bytes)?;
    Ok(())
}
```

Only one `Profiler` may be active per process. `stop()` is idempotent and Drop
also stops the session. Stopping discards pending samples; collect a report
first to retain them. A stopped owner cannot report or affect a later session.
Applications schedule reports and provide their own storage or uploader.
`pprof::Profile` exposes standard protobuf types for `prost::Message` decoding.

## Profile semantics

- Default reports contain interval `alloc_objects/count` and `alloc_space/bytes`.
- `live_heap_tracking` adds `inuse_objects/count` and `inuse_space/bytes`; live
  samples survive reports, and `inuse_space` becomes the default sample type.
- Allocation and live counters use the same inclusion-probability weights,
  retaining fractional counts until stack aggregation. They estimate requested
  bytes and objects, rather than process RSS or allocator usable sizes.
- Reports are uncompressed pprof protobufs with timestamps and duration. The
  `__delta__="false"` sample label tells compatible consumers, including
  Pyroscope, that allocation counters already describe intervals. Other pprof
  consumers can ignore this label.
- `mimalloc_stats()` exposes report, drained-record, buffering, allocation-drop,
  live-drop, and metadata-size counters. Repeated live snapshots do not count
  as newly reported allocation events.

## Scope and constraints

Only Rust allocations through `SamplingMiMalloc` after initialization are
observed. Using `rustfs_mimalloc::MiMalloc` alone does not enable this profiler;
direct C allocations are outside its coverage. The Rust wrapper is independent
of the experimental native mimalloc profiler. The 0.6 allocator's `no_profile`
feature disables native sampling, but does not disable this wrapper.

Allocation event buffers and the live table are bounded. Registry/queue
contention can omit new samples, counted explicitly. Accepted events are in
reporter-visible buffers even while their thread is idle; tracked frees remove metadata before addresses can be reused. Live
removals may wait for a shard lock. Reports copy live shards individually, so
snapshots do not freeze the application at one global instant.

Portable backtrace capture is the default. `MimallocStackCapture::Native` is
an opt-in Linux GNU collector and is rejected elsewhere. Mapping and build-ID
collection supports Linux and Apple targets; other targets retain addresses
and available local symbols. Source filenames and lines require debug data.
Unresolved addresses remain eligible for offline symbolization. Sampled code
must remain loaded through reporting. On Apple platforms, avoid concurrent
library loading or unloading during reports.

Reporting and cleanup suppress self-sampling on the calling thread. A session
uses no background threads or network services. Performance depends on sample
interval, workload, capture engine and record drops; no production overhead or
whole-process jemalloc parity is guaranteed.

## Validation and diagnostics

```sh
cargo test -p rustfs-mimalloc-profiling
cargo test -p rustfs-mimalloc-profiling --test profiling -- --ignored
cargo test -p rustfs-mimalloc-profiling --release --test mimalloc_release
CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_STRIP=none \
  MIMALLOC_TEST_EXPECT_SOURCE=1 \
  cargo test -p rustfs-mimalloc-profiling --release --test mimalloc_release
bash scripts/test_mimalloc_benchmark_report.sh
MIMALLOC_BENCH_DURATION_MS=1000 MIMALLOC_BENCH_ENFORCE_QUALITY=1 \
  bash scripts/mimalloc_benchmark_report.sh
```

The baseline and overhead examples compare plain mimalloc, an inactive
wrapper, allocation sampling, and live tracking. Periodic-report scenarios
check dropped and pending records. Benchmark history preserves the sampling
policy, capture engine, worker count, and quality status. Lossy pressure rows
must not be presented as lossless profiling cost. Performance attribution
requires resource-isolated repeated runs with drift checks.

To export an example's final report after timing ends:

```sh
MIMALLOC_BENCH_MODE=live MIMALLOC_BENCH_PPROF_PATH=target/heap.pprof.gz \
  cargo run -p rustfs-mimalloc-profiling --release --example mimalloc_overhead
go tool pprof -sample_index=inuse_space -top target/heap.pprof.gz
```

The export contains the final report, not merged allocation intervals. See
[DESIGN.md](DESIGN.md) for the extraction boundary and [NOTICE](NOTICE) for origin.
