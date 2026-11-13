# 0.7.5 follow-up review — 2026-11-13

The current base is `96ea497`, verified with `git pull --ff-only`. All published
crates and internal requirements already use 0.7.5. The existing 0.7.5 section
contains the pending release notes, and this follow-up consolidates its changes
there. The selected native source is now `80899607`, one commit after the
previous pin. It remains a v3.6.2 development snapshot with C version 30602.
The upstream/API audit is in [UPSTREAM_REVIEW.md](UPSTREAM_REVIEW.md).

## Functional review

| Area | Current result |
| --- | --- |
| GlobalAlloc | Allocation, zeroing and realloc forward size/alignment directly to native aligned APIs. The wrapper adds no runtime dispatch or per-allocation Rust state. Layout/content/zeroing contracts are exercised by the allocator suite. |
| First-class heaps | The native acquire-bitmap correction synchronizes deletion with concurrent page release. Add a 16-round, four-worker regression using 64-byte, 16-KiB and 2-MiB blocks. Release the allocating context before deletion, verify payload endpoints, and retain one block per size per worker until deletion completes. |
| Cached contexts | ThreadHeap's borrow prevents parent deletion while a safe cached view exists. Thread affinity and unsafe explicit-release requirements remain documented. The new regression transfers block ownership without sharing cached contexts. |
| Arena lifecycle | Arena reservations remain process-lifetime resources. Native API, arena and bitmap tests exercise allocation and page traversal after the update. The generated standalone QSBR header and C++ linkage remain covered. |
| Profiling | Native no_profile, custom callback ABI and Rust no_hooks contracts retain their coverage. The Rust live map keeps bounded admission, blocking removal and pending-realloc reservations. Reporting reserves replacement storage outside shard locks. |
| API exposure | Public headers are identical to the previous pin. Revalidate the existing 138-function inventory, 46 option IDs, 129 extern signatures, nine callbacks, three layouts and seven controls. |

## Performance improvements to pursue

| Priority | Evidence and proposed work | Acceptance measurements |
| --- | --- | --- |
| 1 | GlobalAlloc dealloc currently calls `mi_free` and discards Layout. Existing size/alignment-aware native frees offer a candidate fast path. Benchmark the existing wrappers against general free before changing forwarding. | Cover ordinary and over-aligned small objects, large/singleton blocks, remote frees, native profiling, secure/debug and custom tracking. Record throughput, tail latency and RSS. |
| 1 | `Heap::thread_local()` caches a native context. `ThreadHeap::{malloc_aligned,realloc_aligned}` reuse it across allocations and buffer growth. Use the existing heap ABBA runner to quantify the benefit for multi-heap workers. | Measure heap lookup versus cached contexts, worker scaling and retirement, with identical layout mix and allocator options. |
| 1 | Sampling hits call `StackKey::capture`, while each report creates stack aggregation and frame caches in `build_memory_samples_with_resolver`. Reusable report storage could reduce allocation churn. Symbol lifetime must account for dynamic image unloads. | Compare sample interval, stack depth and report frequency. Record total report time, pending/dropped samples, retained metadata and profile stack quality. |
| 2 | Live-map insertions use try_lock and may drop under snapshot contention. `snapshot_shard` copies a shard under its mutex. Shard sizing and copy time merit targeted measurement. | Compare throughput with live tracking on/off, snapshot p95/p99, live drops and metadata size under same-thread and remote-free workloads. |
| 2 | Native aligned paths, realloc retention and purge/reclaim policies influence copy volume and resident memory. They are already available through native APIs/options. | Separate shrink/grow and ordinary/singleton cases. Measure requested/usable bytes, in-place ratio, RSS, faults and latency with fixed option provenance. |

The bitmap update adds acquire semantics to page traversal. Performance effects
for this pin have yet to be measured. The existing ABBA tools enforce drift
and profile-quality checks. Measurements require reserved resources, frozen
binaries and a same-binary control before attributing throughput differences.

## Validation

Current local validation uses stable Rust 1.100.0 on macOS arm64 and serial
libtest execution. The following checks pass.

- Workspace default, no_profile, secure/debug/no_profile,
  secure/debug/override and all-feature suites, plus the profiling
  no_hooks/rustfs-allocator suite.
- The new four-worker heap-deletion regression in debug, release and
  secure/debug/no_profile configurations.
- The API inventory, ABI projection and all seven mutation/option controls.
  C/C++ header/linkage, Unix option parser, Windows source/entropy controls
  and checked-source drift tests run through the workspace suite.
- Eight native debug C tests covering API, API-fill, arena, bitmap, fork,
  QSBR, profile and pprof. Four native secure tests cover API, arena, QSBR
  and memory protection.
- Both isolated allocator providers in debug and release. Both ignored
  release pressure tests pass with System and the updated RustFS provider.
  The optimized profile lifecycle/quality suite also passes.
- All-target/all-feature Clippy with denied warnings, rustdoc with denied
  warnings, benchmark compilation, formatting and diff checks.
- Normalized profiling archive validation with local dependencies, sys and
  allocator archive creation, and extracted sys no_profile API execution.
  The allocator archive uses an explicit local sys dependency patch because
  the requested 0.7.5 source version is being prepared together.
- Version, edition, internal dependency and changelog consistency for 0.7.5.

Cargo 1.100 emits two workspace-manifest warnings for the dynamically consumed
provider dependency and unused workspace package description. Logs and API
inventory are retained under ignored `target/review-0.7.5-refresh-*` and
`target/api-0.7.5-refresh.json`.

The baseline `96ea497` CI run is successful. Current Windows static CRT,
clang-cl, Linux/musl and Rust 1.95 execution results come from CI for the pushed
commit. Hardware MTE/PAC and workload performance require target measurements.
[Baseline CI](https://github.com/houseme/rustfs-mimalloc/actions/runs/43358553143)

---

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
