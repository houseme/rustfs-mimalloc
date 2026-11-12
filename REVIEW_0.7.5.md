# 0.7.5 functional and upstream review — 2026-11-12

The clean project base after `git pull --ff-only` is `0c9a24a`. That revision
contains 0.7.4 with no Unreleased/0.7.5 section. This source preparation
synchronizes the three crates and their internal requirements to 0.7.5 and
collects the current changes into one shared changelog section.

## Upstream and API coverage

The native pin advances by 110 commits from `ea136f74` to
[`07db29aa986bb7cf57077e7017596afb6380572e`](https://github.com/microsoft/mimalloc/commit/07db29aa986bb7cf57077e7017596afb6380572e).
It is a v3.6.2 development snapshot, numeric C version 30602. The latest official
upstream release checked for this review is v3.6.1. The vendored tree stays
pristine. [Upstream release](https://github.com/microsoft/mimalloc/releases/tag/v3.6.1)

Read all four public-header diffs and the updated PR metadata. PR #1487 is
merged into dev3. PR #1489 closed without a GitHub merge, with its backtrace
capability check incorporated directly in `23fced33`. The full audit and key
native commit references are in [UPSTREAM_REVIEW.md](UPSTREAM_REVIEW.md).

| Surface | Result |
| --- | --- |
| Thread heap release | Add raw `mi_thread_heap_release` and unsafe `Heap::release_current_thread`. It permanently disables the caller's future allocations in that heap while preserving blocks for frees. Discard cached aliases first. |
| Precise guarded control | Add `mi_option_deprecated_unsafe_guarded_precise` at ID 31 while retaining older Rust spellings. Native precise placement is selected through MI_UNSAFE_GUARDED_PRECISE at build time. |
| Native profiler | Attachment generations delimit free callbacks. Stop ends sampling while on_free continues until detachment. Tests now free retained samples after stopping, before detaching. |
| Secure environment | MI_SECURE_ENV defaults on with secure. Environment options are restricted to show_stats, verbose and allow_thp. Tests verify defaults and direct programmatic configuration. |
| Native controls | 138 Rust function names, 46 option IDs, 129 FFI signatures, nine callbacks, three layouts and seven ABI/option controls pass. The reviewed unbound-export set contains 96 entries. |

Upstream also tightens non-null first-class heap handles. Correct the stale raw
debug-fill documentation that advertised a null main-heap selector. Rust Heap
handles already use NonNull and obtain the main heap through mi_heap_main.

## Functional assessment

| Area | Assessment and action |
| --- | --- |
| Global allocation | Rust forwards layout size/alignment through native aligned allocation and realloc. The updated native regular-block alignment path can benefit these calls. Existing content/alignment/zeroing contracts pass. |
| Heap lifecycle | New worker release supports explicit producer shutdown before heap deletion with remaining frees. Tests cover used and unused worker contexts, idempotence, permanent caller allocation failure, another thread's successful allocation, and retained block contents after heap deletion. |
| Arenas, purge and commit accounting | Inherit fixes for multi-chunk tails, purge-expiration races, committed statistics, page-map initialization and arena metadata indices. Native arena/bitmap tests and Rust reservation/query contracts pass. |
| QSBR | Inherit singleton-page retention and empty abandoned-page reclamation changes. The existing raw constructor and generated standalone header correction remain covered by native QSBR and C/C++ tests. Host reader protocol design remains application-owned. |
| Native profiling | Inherit attachment-generation, concurrent stop/snapshot and stdio-reentrancy fixes, Linux mapping offsets, sample weighting/countdown corrections and documented callback lifetimes. Programmatic profiling remains available in secure builds. |
| Rust profiling | The generic SamplingAllocator provider, bounded recorder, fallible initialization, stop cleanup and single-lock live snapshots retain their current contracts. Both allocator providers and release pressure tests pass with the updated native source. |
| Build/targets | Cargo's target metadata already selects ARM architecture for the cc build. CMake CI adopts the new backtrace capability variable. The Windows static-CRT source anchors and entropy probes pass on the host. Native Windows/Linux execution is covered by the pushed CI matrix. |

## Reproduced compatibility issue

With pristine upstream `07db29aa`, MI_PROFILE=0 and an explicitly attached raw
profiler, the native probe records **64 callbacks**. The existing Rust no_profile
integration test independently rejected the same behavior. The raw opt-out
contract requires zero allocation/free samples while explicit snapshots remain
available.

The no_profile build now creates a guarded sampler copy in OUT_DIR, forcing
profile sample rate to zero and making enabled-profiler lookup return null.
Constant conditions allow optimized compilation to eliminate the disabled
lookup path. Static source generation composes with the Windows CRT correction.
The patch validates both source anchors and the aggregator include, and an
anchor-drift test requires review when upstream changes. Cargo's sys archive
explicitly includes the helper. An extracted archive compiles and passes the
raw API suite with no_profile enabled.

The native probe and failure output are saved in
`target/no-profile-upstream-probe.c` and
`target/review-0.7.5-native-no-profile-before.log`. The corrected runtime and
archive evidence is in `target/review-0.7.5-no-profile.log` and
`target/review-0.7.5-archived-no-profile.log`.

## Performance priorities

| Priority | Current change or opportunity | Measurement needed |
| --- | --- | --- |
| 1 | Native aligned allocation routes guaranteed-aligned regular blocks directly through ordinary allocation and can use naturally aligned size classes. Rust GlobalAlloc reaches this path directly. | Compare small and over-aligned alloc/zeroed alloc with fixed layouts and build flags. Include resident memory and requested/usable sizes. |
| 1 | Realloc retains ordinary blocks with at most 12.5% waste. Eligible huge singleton blocks can retain at least half and reset their unused tail. | Separate shrink/grow, ordinary/huge and in-place/moved outcomes. Record bytes copied, latency, RSS and faults. |
| 2 | Chunkmap hints skip unavailable prefixes in huge searches. Native bitmap updates and committed-slice counting reduce writes, and environment parsing scans once on supported platforms. | Measure concurrent arena churn, fragmentation, startup work and purge behavior with stable workload/option provenance. |
| 2 | Use the new release API when a worker is permanently done with a dedicated heap. Reuse cached ThreadHeap contexts during active work. | Measure worker churn, heap retirement latency and remote frees. Temporary idle workers can use quiescent abandonment. |
| 2 | Active Rust profiling still pays stack capture, bounded live-table maintenance and report symbolization costs. Existing complete-report and encoding-phase timers support attribution. | Compare sample rate, queue capacity and report interval using recorded, reported, pending and dropped counts. Preserve profile quality alongside throughput. |

Throughput effects have yet to be measured for this pin. Use reserved resources,
frozen binaries and a same-binary control before A1/B1/B2/A2 comparisons. The
existing tools reject attribution above their drift and record-quality limits.
The 0.7.4 diagnostic runs remain historical evidence for their own configuration.

## Validation

Local validation uses stable Rust 1.99.1 on macOS arm64 with serialized libtest
execution. Dedicated concurrency tests create their own workers.

- Workspace default and secure/debug tests pass, along with no_profile,
  secure/debug/no_profile, no_hooks and macOS secure/debug/override combinations.
- API/ABI controls, C/C++ standalone headers/linkage, parser/THP probes,
  Windows source/entropy controls and the source-drift check pass.
- Eight native tests pass for API, API-fill, arena, bitmap, fork, QSBR, native
  profile and pprof. Four secure tests pass for API, arena, QSBR and the new
  native memory-protection suite.
- Both isolated providers pass in debug and release. Both ignored release
  pressure suites pass. Benchmarks compile successfully.
- All-target/all-feature Clippy, rustdoc with warnings denied, formatting and
  diff checks pass. Normalized profiling archive validation, sys/allocator
  packaging and the extracted sys no_profile integration pass.
- The 0.7.5 release consistency check passes. The lockfile changes only the
  four workspace package versions. Source preparation proceeds through branch
  commit/push.

Logs and API inventories remain under ignored `target/review-0.7.5-*` and
`target/api-0.7.5.json`. Linux unwinder fallback, Windows static CRT/clang-cl,
musl and Rust 1.95 execution depend on the current CI run. Hardware MTE/PAC,
32-bit and production performance require their respective target environments.
