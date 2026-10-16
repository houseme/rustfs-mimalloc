# Standalone profiling boundary

## Motivation

The mimalloc backend proposed in grafana/pyroscope-rs#572 combined a profiler
with a Pyroscope adapter. The existing jemalloc backend obtains encoded
profiles from `jemalloc_pprof`. This companion crate provides the equivalent
separation for mimalloc, without depending on Pyroscope.

## Ownership

| Component | Owner |
| --- | --- |
| Native allocator and FFI | Existing allocator/sys crates |
| `SamplingMiMalloc`, Poisson sampling, TLS rings and bounded recorder | Profiling crate |
| Pointer lifecycle, realloc rollback, live table and session generations | Profiling crate |
| Stack capture, symbol resolution, mappings, pprof types and encoder | Profiling crate |
| Config, diagnostics and exclusive `Profiler` session | Profiling crate |
| Backend trait, report scheduling, HTTP, auth, tenant and series labels | Pyroscope adapter |
| Allocator regressions, symbol quality and overhead workloads | Profiling crate tests/CI |
| Upload preservation and real-server interval/live queries | Pyroscope tests/CI |

The dependency direction is Pyroscope -> profiling -> allocator -> sys.
Applications selecting only the allocator do not depend on the profiler,
backtrace, object parser or protobuf encoder. Tests decode the standard pprof
wire format independently in both repositories; no shared Rust protobuf type
identity is required across the integration boundary.

## Public session API

`Profiler::start(config)` validates configuration, checks that the sampling
allocator has been observed, and claims one process recorder. `report()`
returns owned uncompressed pprof bytes. `stop()` releases ownership and pending
metadata; Drop provides the same cleanup. A stopped owner returns `Inactive`
from `report()` and cannot clear a subsequent session. Errors are typed and
independent of Pyroscope. Existing config, allocator and stats names can be
re-exported by adapters without duplicating state.

## Sampling and bounded recording

Allocation events and live objects share one inclusion-probability estimator:
for requested size S and byte interval R, p = 1 - exp(-S/R), and an included
sample represents 1/p objects and S/p bytes. This is the sampling correction
used by standard heap profiling consumers, including
[Go's pprof exporter](https://go.dev/src/runtime/pprof/protomem.go).
Fractions survive per-stack aggregation and are rounded once during encoding.
This avoids the object-count bias caused by per-event integer division.

Exponential memorylessness permits a fresh interval at the end of each sampled
allocation. Sampling therefore uses one RNG draw per hit, with no loop over
crossed intervals and no deterministic large-allocation fallback. Unsampled
allocations in the current generation update their TLS byte countdown without
entering the recursion guard; resets and captures retain suppression.

A TLS buffer must be registered before it accepts records. Registry contention
rejects the event with an explicit drop count; a later sampling hit can retry.
Thus an idle worker cannot retain accepted records invisible to the reporter.
Reports drain registered buffers directly, so allocation hooks no longer poll
a flush-request generation or participate in a separate handoff protocol.
Unavailable TLS during thread teardown is also counted as a dropped event.

The fixed TLS queue is a FIFO ring with head/length indices. A partial drain
moves only the records consumed and never compacts remaining stack records.
Global overflow shards absorb full-ring handoffs and thread-exit data. Fair
report budgets prevent either source from starving. Generation checks,
realloc rollback and reliable cross-thread free handling retain session and
pointer lifecycle correctness under one recorder admission rule.

Instruction addresses, source metadata and interval/live snapshot semantics
remain unchanged. The pprof sample label `__delta__="false"` identifies interval
samples for compatible consumers without depending on an uploader. Profiler
frame filtering preserves application functions that use profiler types.

## Release and allocator compatibility

All three crates inherit the workspace version and release together.
Dependency versions and release notes are managed in the root `Cargo.toml`
and `CHANGELOG.md`.
The profiling crate uses the allocator's `GlobalAlloc` methods; native
profiling APIs are not registered or used by this wrapper.

The `v<VERSION>` tag triggers `release.yml`. Pre-publication archive tests use
local patches for the not-yet-published dependencies. Publication follows sys,
allocator, then profiling, verifying each official index entry and uploaded
archive checksum before continuing. The normalized profiling archive is then
tested against the exact registry allocator/sys versions without local patches.
Consumers switch from Git only after all three packages are visible.

## Validation

Run formatting, all-target/all-feature Clippy, workspace tests and docs. Run
standalone behavior tests with concurrent allocation, cross-thread frees,
realloc failure, session restart and missing-allocator rejection. Exercise
stripped and source-enabled optimized profiles on Linux/macOS, and preserve
the quality-gated benchmark diagnostics. The Pyroscope consumer must pass
default/all-feature tests, raw HTTP payload/auth/tenant checks, and both pinned
server ingestion paths. Record local and CI results separately.

## Sampling review evidence

A deterministic population test on the previous implementation (`63b53f2`)
used 500,000 allocations of 768 bytes with a 1 KiB interval. The object estimate
was 394,752 (21.05% low), although the byte estimate was within 0.1%. The unified
estimator returns approximately 499,813 objects for the same population and
seed. Tests cover multiple sizes and intervals, fractional aggregation, actual
allocator-to-pprof population reconstruction, and FIFO wraparound.

A same-binary control followed by frozen-binary A/B/B/A comparisons ran on two
fixed Linux VM CPUs with the same compiler/build configuration and workspace
allocator 0.6. These historical measurements are specific to that workload
and build configuration. The accepted
per-workload comparisons passed 5% drift and 1% record-drop gates:

| Workload | Window | Throughput change |
| --- | ---: | ---: |
| Allocation, 1 MiB interval, one worker | 8 s | +0.23% |
| Allocation, 1 MiB interval, two workers | 8 s | -1.01% |
| Allocation, 1 KiB interval, fixed 64 KiB objects | 8 s | +8.83% |
| Live heap, 1 MiB interval, one worker | 20 s | -0.43% |

The ordinary workloads are near measurement noise; the clear local benefit is
in dense sampling. This does not establish production overhead or universal
optimality. The first shorter control had excessive baseline drift and was
rejected. The final live case also exceeded candidate drift in its 8-second
run; the longer 20-second retry passed. Rejected runs are retained but are not
used for attribution.

`scripts/bench_profiling_abba.py` reproduces the comparison from two frozen
executables, retains raw outputs and hashes locally, and rejects unstable or
lossy comparisons. Run the same executable for both arguments as a control
before comparing different builds. Use `--case` to rerun a failed workload
without repeating valid cases. Configure CPU affinity and resource isolation
outside the driver and use a fresh output directory for every run.
