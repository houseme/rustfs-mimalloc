# 0.7.6 allocator and performance review — 2026-11-27

The remote project base `d927f57` carries 0.7.5 and one Unreleased section.
The workspace, inherited requirements, lockfile and current installation
examples now use 0.7.6. The pending notes are in a single 0.7.6 section.
The latest upstream dev3 source was already pinned. The source and PR
comparison is recorded in [UPSTREAM_REVIEW.md](UPSTREAM_REVIEW.md).

## Official Rust API synchronization

The official allocator exposes `Allocator` for `MiMalloc` and borrowed
heaps, with `MiMallocExcess` reporting native usable capacity. A Rust
1.100 stable compile probe and standard `Vec::new_in` tests confirm
availability. This workspace now exposes those three implementations.
Returning requested capacity avoids an extra size query for the ordinary
allocator. Returning usable capacity lets containers use size-class
headroom. The zeroed path explicitly initializes any returned tail beyond
the requested size. Fitting layouts can describe more bytes than the
original request, so trait deallocation uses general native free.

The official lower secure levels and five linked statistics exports are
also available. The strongest unified Cargo secure feature wins.
Source-level differences and the omitted linkless declaration are listed
in the official Rust audit.

The follow-up passed macOS arm64 workspace tests with default features,
`secure_basic`, `secure_checked` and all features. Combined lower-level
features selected level 2. API, ABI, Clippy, rustdoc and local package checks
passed for the unified 0.7.6 version. Cross-built Docker tests on Linux arm64
passed 70 allocator tests in each of the default and combined checked/no_profile
configurations, plus the raw API and 44 option cases in each configuration.

## Usable-capacity reallocation

`MiMalloc::realloc_aligned_with_usable_size` previously always used
`mi_urealloc_aligned`, which looks up the result's size after reallocation.
For alignment at most one machine word, the native implementation already
redirects aligned reallocations to ordinary reallocation if the original
address meets the requested alignment. The Rust wrapper now applies the
same guard and uses `mi_urealloc` to return size from that operation.
Offset pointers that fail the guard and over-aligned requests still use the
aligned entry point. Tests exercise null input, retained data, output
alignment, capacity reporting and preserved allocations after failed growth.
No workload-level speedup was measured.

## Further measurements

1. Compare usable-capacity reallocations for in-place shrink and moved
   growth, using ordinary, secure and over-aligned layouts. Track usable
   capacity and bytes copied alongside latency, with same-binary controls.
2. Compare cached ThreadHeap handles against repeated lookup under worker
   churn, including remote frees and idle-thread arena collection. Track
   heap retirement time, RSS and p99 allocation and purge latency.
3. Attribute the remaining profiling report costs to symbol resolution,
   serialization and live-map contention. Use the existing profiling
   diagnostics and verify sample counts and drops before drawing throughput
   conclusions.

The release workflow checks the official index before uploading and waits
60 seconds before fetching and verifying the published versions. That
workflow is included in the source preparation notes. This review covers
the source and local validation for 0.7.6.

## Validation

Stable Rust 1.100.0 on macOS arm64 passed default, secure/debug and
all-feature workspace tests, both isolated providers, the normalized
profiling archive, Clippy, rustdoc, release consistency and release-note
generation. The public API inventory, C ABI projection and eight controls
passed. Linux arm64 allocator test binaries were cross-built and run in
Docker, with 67 tests passing in each of the release default and
secure/debug/no_profile configurations.
