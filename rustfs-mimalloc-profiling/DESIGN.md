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

## Preserved behavior

This extraction retains sampling weights, queue limits, fair drains, generation
checks, cross-thread free handling, realloc rollback, symbol metadata and
interval allocation/live snapshot semantics. The pprof sample label
`__delta__="false"` is emitted for interval samples for compatible consumers;
this requires no dependency on an uploader. The profiler-frame filter follows
the new crate/module symbol paths and still preserves application generics.

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
