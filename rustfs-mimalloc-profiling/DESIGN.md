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
entering the recursion guard; resets, handoffs and captures retain suppression.

The fixed TLS queue is a FIFO ring with head/length indices. A partial drain
moves only the records consumed and never compacts remaining stack records.
The existing global overflow shards, fair report budgets, generation checks,
realloc rollback and reliable cross-thread free handling preserve recorder
ownership and bounded storage. These mechanisms handle real concurrency and
pressure contracts and are kept together rather than replaced by extra modes.

Instruction addresses, source metadata and interval/live snapshot semantics
remain unchanged. The pprof sample label `__delta__="false"` identifies interval
samples for compatible consumers without depending on an uploader. Profiler
frame filtering preserves application functions that use profiler types.

The allocator dependency comes from this workspace's current main (0.6.0),
whereas the original Pyroscope implementation used 0.5.6. This dependency change
needs correctness and performance revalidation; old timings are not evidence
for the extracted crate. Native profiling is not enabled or registered by this
crate. Existing native APIs and their worker-thread/lifetime restrictions are
a separate implementation choice, not a prerequisite for extraction.

## Delivery

Keep the Pyroscope adapter small and retain service integration tests there.
Before a registry release, pin its Git dependency to a reviewed immutable
commit; local path overrides are validation-only. Do not commit machine paths.
The workspace release workflow includes this crate after the allocator package.

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
fixed Linux VM CPUs, with 8-second workload windows and the same compiler/build
configuration. All four comparisons passed 5% drift and 1% record-drop gates:

| Workload | Throughput change |
| --- | ---: |
| Allocation, 1 MiB interval, one worker | -0.05% |
| Allocation, 1 MiB interval, two workers | -1.91% |
| Allocation, 1 KiB interval, fixed 64 KiB objects | +11.31% |
| Live heap, 1 MiB interval, one worker | +2.84% |

The ordinary workloads are near measurement noise; the clear local benefit is
in dense sampling. This does not establish production overhead or universal
optimality. The first shorter control had excessive baseline drift and was
rejected. Its results are not used for attribution.

`scripts/bench_profiling_abba.py` reproduces the comparison from two frozen
executables, retains raw outputs and hashes locally, and rejects unstable or
lossy comparisons. Run the same executable for both arguments as a control
before comparing different builds. Configure CPU affinity and resource
isolation outside the driver and use a fresh output directory for every run.
