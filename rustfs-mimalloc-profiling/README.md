# rustfs-mimalloc-profiling

Sampled allocation and optional live heap profiles for Rust programs using
mimalloc. This companion crate owns the sampling allocator, recorder, live
allocation lifecycle, symbolization, executable mappings, and pprof encoder.
The base `rustfs-mimalloc` crate does not depend on it.

## Usage

The profiler has no allocator dependency enabled by default. Applications select
one provider and install `SamplingAllocator<A>` as their global allocator. The
same recorder, live tracking, symbolization and pprof implementation serve both
providers; the wrapper uses static dispatch through `GlobalAlloc`.

For the upstream `mimalloc` provider:

```toml
[dependencies]
rustfs-mimalloc-profiling = { version = "0.7.1", default-features = false }
mimalloc = "0.1.52"
```

```rust
use rustfs_mimalloc_profiling::{MimallocConfig, Profiler, SamplingAllocator};

#[global_allocator]
static ALLOC: SamplingAllocator<mimalloc::MiMalloc> =
    SamplingAllocator::with_allocator(mimalloc::MiMalloc);

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

To use the RustFS provider directly, depend on `rustfs-mimalloc = "0.7.1"`
and substitute `rustfs_mimalloc::MiMalloc` for the provider above.

### Migration from 0.6

The old convenience name and zero-argument constructor remain available with an
explicit feature:

```toml
rustfs-mimalloc-profiling = { version = "0.7.1", features = ["rustfs-allocator"] }
```

```rust
use rustfs_mimalloc_profiling::SamplingMiMalloc;

#[global_allocator]
static ALLOC: SamplingMiMalloc = SamplingMiMalloc::new();
```

Enable this feature only for applications selecting the RustFS provider. Library
adapters should depend on the default allocator-neutral core and re-export
`SamplingAllocator`, leaving provider selection to their applications. Cargo
features are additive: enabling `rustfs-allocator` elsewhere in the dependency
graph also brings in the RustFS provider. Use each provider in its own binary.

The three workspace crates still share one release version. All dependency
requirements are managed in the root `workspace.dependencies`.

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

Only Rust allocations through `SamplingAllocator` after initialization are
observed. Installing a bare provider alone does not enable this profiler;
direct C allocations are outside its coverage. The Rust wrapper is independent
of the experimental native mimalloc profiler. The allocator's `no_profile`
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
cargo test -p rustfs-mimalloc-profiling -- --test-threads=1
cargo test -p rustfs-mimalloc-profiling --features rustfs-allocator --test compatibility
python3 scripts/test_profiling_providers.py --local-dependencies
cargo test -p rustfs-mimalloc-profiling --test profiling -- --ignored
cargo test -p rustfs-mimalloc-profiling --release --test mimalloc_release
CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_STRIP=none \
  MIMALLOC_TEST_EXPECT_SOURCE=1 \
  cargo test -p rustfs-mimalloc-profiling --release --test mimalloc_release
bash scripts/test_mimalloc_benchmark_report.sh
MIMALLOC_BENCH_DURATION_MS=1000 MIMALLOC_BENCH_ENFORCE_QUALITY=0 \
  bash scripts/mimalloc_benchmark_report.sh
```

The sampler is process-global. Run profiling test cases serially so test-harness
allocations cannot contaminate another case's exact counters; the concurrency
contracts create their own worker threads. Provider/archive drivers enforce this.

Provider and archive verification scripts require Python 3.11 or newer. The
provider driver generates separate Cargo workspaces from root dependency
requirements, runs shared contracts, and rejects unexpected allocator graphs.

The baseline and overhead examples compare plain RustFS mimalloc, an inactive
wrapper, allocation sampling, and live tracking. Periodic-report scenarios
check dropped and pending records. Benchmark history preserves the sampling
policy, capture engine, worker count, and quality status. Lossy pressure rows
must not be presented as lossless profiling cost. CI retains the real pressure
report as diagnostics and gates its strict classifier using replayed fixtures.
Set `MIMALLOC_BENCH_ENFORCE_QUALITY=1` to reject any nonzero live loss in steady
scenarios; snapshot contention can cause such loss by design. Performance attribution
requires resource-isolated repeated runs with drift checks.

To export an example's final report after timing ends:

```sh
MIMALLOC_BENCH_MODE=live MIMALLOC_BENCH_PPROF_PATH=target/heap.pprof.gz \
  cargo run -p rustfs-mimalloc-profiling --features rustfs-allocator --release --example mimalloc_overhead
go tool pprof -sample_index=inuse_space -top target/heap.pprof.gz
```

The export contains the final report, not merged allocation intervals. See
[DESIGN.md](DESIGN.md) for the extraction boundary and [NOTICE](NOTICE) for origin.

Release notes are maintained in the shared
[workspace changelog](https://github.com/houseme/rustfs-mimalloc/blob/main/CHANGELOG.md).
