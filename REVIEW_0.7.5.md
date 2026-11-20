# 0.7.5 follow-up review — 2026-11-20

The clean base after `git pull --ff-only` is `6ee6142`. The workspace already
uses 0.7.5 and has one shared changelog section. The current follow-up is
collected there with a November 20 date. Native source advances from the
v3.7.0 release to post-release dev3 `cb534580`, with numeric C version 30700.
The release API still identifies v3.7.0 as the latest official release.
The commit/PR audit is in [UPSTREAM_REVIEW.md](UPSTREAM_REVIEW.md).

## Findings and fixes

| Area | Finding and delivered change |
| --- | --- |
| Native profiler lifetime | A safe attach could publish callbacks that outlive the handle's Drop. Mark attach unsafe and specify callback quiescence before deletion. A compile-fail example checks the boundary. |
| Profiler target ownership | A failed repeated attach discarded local ownership, custom detach targeted the main subprocess, and retargeting left previous registrations behind. Store one successful subprocess target, detach it before retargeting, stop native sampling on detach and release the same target in Drop. |
| Profiler state | Remove the redundant running flag and query native start/stop directly. Regression coverage includes repeated attachment, occupied main slots, independent custom slots, failed retargeting, custom Drop and main-slot preservation. |
| THP and reset policy | Document allow_thp modes 0/1/2 and the Linux default 2. The native implementation now advises THP independently of explicit huge-page permission, checks complete aligned-page coverage and honors huge-block purge/reset policy. Host-C probes execute the new native eligibility and purge-minimum functions. |
| Build compatibility | Numeric upstream Windows startup macros require numeric conditions in the generated static-CRT source. Refresh the checked anchors and tests. The parser source probe adopts the numeric MI_NO_GETENV guard. |
| Public API | Public function/option/layout additions in this range total zero. Verify 159 Rust function names, 46 IDs, 150 extern signatures, nine callbacks, three layouts and eight controls. |

Native pprof callers now use `unsafe { profiler.attach() }` after establishing
that all callback users will finish before teardown. Custom subprocesses must
outlive their attachments. Reattachment first releases the previous target,
and a competing profiler keeps its occupied slot. Raw consumers continue to
uphold the upstream lifetime and registration contracts.

## Performance assessment

The current project already implements ordinary-allocation dispatch, sized
GlobalAlloc frees, sampled-stack hashes, cold sampling tails and Linux ELF
mapping caches. The review evaluates those current paths alongside the new
native source.

1. Lazy abandoned-bin bitmaps reduce metadata header writes and committed
   pages for unused bins. Measure arena/heap count, OS page size and resident
   metadata under the actual workload. Native arena tests cover first
   abandonment, concurrent initialization and reclaim after thread exit.
2. Compare allow_thp=1 and 2 using page faults, RSS, huge-page residency and
   p99 allocation/purge latency. Mode 2 can retain larger resident ranges by
   avoiding splits. Explicit purge minimums and no_thp build selection belong
   in the benchmark configuration.
3. Measure huge shrink, remote free and disabled-abandonment workloads. The
   new native reset policy and fuller queue scanning trade reclamation work
   against memory retention. Record bytes retained, eager resets and latency.
4. Compare native pprof and Rust SamplingAllocator with their respective
   coverage and stack-quality requirements. The Rust report still constructs
   aggregation and frame caches per report. Reusable report capacity could
   reduce allocation churn while preserving dynamic-image symbol lifetimes.
5. Use cached ThreadHeap contexts where workers alternate between heaps.
   Measure the existing lookup/cached-context workloads and profile the live
   map's snapshot copy duration and contention before changing locking.

This refresh has functional and native regression evidence. Production
throughput/RSS effects require workload measurements. Earlier benchmark
numbers in the changelog describe their original builds and workloads.

## Validation

Local validation uses stable Rust 1.100.0 on macOS arm64 with serial libtest
execution. The following checks pass.

- Workspace default, no_profile, secure/debug/no_profile,
  secure/debug/override and all-feature tests, plus the separate profiling
  no_hooks/rustfs-allocator suite.
- The pprof lifecycle integration in debug and release, and its unsafe-attach
  compile-fail example. The ordinary allocator, callback and visitor suites
  retain their current coverage.
- The 159-function/46-option inventory, 150-signature C ABI projection and
  all eight ABI controls. The C ABI probe also passes with Wundef enabled.
- Nine native debug C tests, four native secure tests and four Clang UBSan
  API/alignment/QSBR tests. The native arena suite includes lazy-bitmap first
  use, concurrent initialization and post-thread-exit reclamation.
- Updated host-C parser, THP eligibility/advice/purge-minimum probes and
  Windows startup/entropy source controls. The THP probes run with and without
  no_thp and MADV_NOHUGEPAGE support, retaining the dangling-else control.
- Both isolated providers in debug and release, plus the RustFS provider's
  two ignored release pressure tests.
- All-target/all-feature Clippy, rustdoc with denied warnings, benchmark
  compilation, formatting and diff checks.
- Normalized profiling archive verification, sys/allocator archive creation
  with local dependency patches, extracted sys no_profile API execution and
  0.7.5 version/changelog/release-note consistency checks.

Cargo emits one existing warning for the provider requirement consumed by
generated consumer workspaces. Evidence stays under ignored
`target/review-0.7.5-nov20-*` and `target/api-0.7.5-nov20.json`.
Current Windows static CRT/clang-cl, Linux THP/musl and Rust 1.95 execution
results come from CI for the pushed commit. Hardware MTE/PAC and production
performance require their respective environments.
The baseline commit's cross-platform CI completed successfully.
[Baseline CI](https://github.com/houseme/rustfs-mimalloc/actions/runs/44451013745)

---

# 0.7.5 follow-up review — 2026-11-15

`git pull --ff-only` confirms base `ad3dae6` is current. The workspace,
lockfile and internal requirements use 0.7.5. Its single changelog section
collects this follow-up with the prior pending notes and is dated November 15.
The native pin advances by six commits to `9548089b`, a v3.6.2 development
snapshot with C version 30602. The upstream audit and individual PR status
are recorded in [UPSTREAM_REVIEW.md](UPSTREAM_REVIEW.md).

## Functional and performance assessment

| Area | Reviewed result and follow-up |
| --- | --- |
| Ordinary allocation and free | The ARM64 page-metadata path now uses integer arithmetic. Native null-free and aligned allocation pass UBSan. GlobalAlloc still forwards allocation/reallocation layouts directly and uses `mi_free` for deallocation. Sized-free forwarding remains a candidate for measured optimization. |
| Heap lifecycle | The prior acquire-bitmap fix and four-worker release/deletion regression continue to pass. Heap/Theap remain opaque Rust handles across the private native state changes. Cached ThreadHeap continues to avoid repeated worker heap lookup. |
| Native QSBR | The heap-delayed singleton queue permits collection after owner exit through later huge allocation and periodic maintenance. The regression bounds retained pages at 16 after ten rounds of four worker exits, each allocating/freeing two 2-MiB singletons. It also covers a busy queue lock, unfinished head goals, mixed arena/OS pages, concurrent enqueue/collection and terminal destruction. This behavior applies to host-adapted native QSBR heaps. |
| Ordinary heap builds | MI_QSBR guards the shared queue implementation and storage. Existing default raw QSBR-constructor failure and ordinary heap allocation/free contracts pass. |
| Windows/build integration | Inherit ARM64EC CMake target/DLL selection. The sys crate builds a static library through cc with Cargo target metadata. Windows static-CRT source anchors and entropy controls continue to pass locally. ARM64EC execution requires its target environment. |
| Profiling | Existing native callback/no_profile contracts and Rust no_hooks, bounded live tracking, realloc rollback and session teardown remain covered. Current report aggregation and symbol caches are rebuilt per report. Live-map snapshot copying still holds each shard mutex during the copy. |
| API exposure | Four public API header diffs are empty. The 138-name/46-option inventory and complete C ABI controls pass. Private QSBR queue helpers are implemented inside the native library. |

The implemented memory-reclamation improvement is exercised by native QSBR
tests. Production throughput and RSS effects require workload measurements.
The PR's earlier-compiler instruction counts are upstream observations.

Further optimization priorities are ordered as follows.

1. Benchmark existing size/alignment-aware frees before changing GlobalAlloc
   deallocation. Cover ordinary and over-aligned sizes, remote frees,
   secure/debug, native profiling and custom tracking.
2. Measure cached ThreadHeap use for multi-heap workers with the existing heap
   ABBA runner, including aligned buffer growth and worker retirement.
3. Reduce profiling report allocation churn through reusable aggregation
   capacity. Keep symbol lifetime tied to current image mappings. Measure total
   report latency, metadata size, pending/dropped records and stack quality.
4. Measure live-map shard copy duration and contention under periodic snapshots
   before changing shard layout or locking. Track p95/p99, record loss and RSS.
5. For native QSBR integrations, measure singleton churn and grace-period
   progress together with retained pages, collection work and huge-allocation
   latency. Ordinary heap benchmarks exercise a separate configuration.

The detailed acceptance cases from the November 13 review remain applicable.
The existing ABBA tools enforce drift and profile-quality checks. A reserved
environment and same-binary control support attributable performance results.

## Validation

Local validation uses stable Rust 1.100.0 and Apple Clang 21 on macOS arm64,
with serial libtest execution. The following checks pass.

- Workspace default, no_profile, secure/debug, secure/debug/no_profile,
  secure/debug/override and all-feature suites. The profiling
  no_hooks/rustfs-allocator suite passes separately.
- The API inventory, C ABI projection and all seven mutation/option controls.
  Existing C/C++ header/linkage, native parser/THP, checked-source drift and
  Windows startup/entropy controls pass through the workspace suite.
- Nine native debug C tests, including the new no-retention QSBR configuration,
  and four native secure tests. Four Clang UBSan C++ tests cover API, API-fill,
  QSBR and QSBR no-retention.
- Both isolated allocator providers in debug and release. Both ignored release
  pressure tests pass with System and the updated RustFS provider. The
  four-worker heap deletion/remote-free regression also passes in release.
- All-target/all-feature Clippy and rustdoc with denied warnings, benchmark
  compilation, formatting and diff checks.
- Normalized profiling archive verification with local dependencies, sys and
  allocator archive creation, and extracted sys no_profile API execution.
  The allocator archive uses an explicit local sys dependency patch.
- 0.7.5 version/internal-dependency/changelog consistency and release-note
  extraction from the single version section.

Cargo 1.100 emits the two existing workspace-manifest warnings for the
dynamically consumed provider dependency and unused package description.
Logs, the full native diff and API inventory remain under ignored
`target/review-0.7.5-nov15-*` and `target/api-0.7.5-nov15.json`.

The updated native source passes UBSan API, API-fill, QSBR and no-retention
tests. A separate C probe checks null free and 64-byte aligned allocation.
Replacing only the current include tree's page-map header with the previous
pin's header produced no diagnostic with this compiler in C at O0/O1 or C++
at O1. The earlier Apple Clang 17 diagnostic is documented in PR #1491.
[Upstream reproducer](https://github.com/microsoft/mimalloc/pull/1491)

The previous commit's cross-platform CI completed successfully. Current Windows
static CRT, clang-cl, Linux/musl and Rust 1.95 results come from CI for the
pushed commit. ARM64EC, hardware MTE/PAC and production throughput/RSS require
their respective target environments.
[Baseline CI](https://github.com/houseme/rustfs-mimalloc/actions/runs/43436155219)

---

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
