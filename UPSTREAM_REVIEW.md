# 0.7.1 upstream, API and performance review — 2026-10-19

## Result and version provenance

The latest `git pull --ff-only` reported the clean repository current at
`f21aaa1`, the preceding 0.7.1 update. All three crates and internal requirements
remain at **0.7.1**. The single 0.7.1 changelog section is
dated 2026-10-19, with no separate Unreleased content. The tagged 0.7.0 and
older release notes are preserved. This delivery commits/pushes code only;
no tag or publication is requested.

Mimalloc advances from
[`6f45cc52`](https://github.com/microsoft/mimalloc/commit/6f45cc52f01413f2d86fe3c7c3ed6c6a86bdd13a)
to [`b4f183bb`](https://github.com/microsoft/mimalloc/commit/b4f183bb1bbcccfade6f98fba30ef276cf1eb5b8),
**17 commits** later, including two since `f21aaa1`'s `1d03ecbd` pin. The source
still identifies itself as **V3.6.0 / 30600**.
This is a fixed development commit, not a published v3.6.0 release: the latest
GitHub release is still [v3.5.3](https://github.com/microsoft/mimalloc/releases/tag/v3.5.3),
and the live tag-prefix query returns no v3.6 tags. The parent revision's
[CI passed all 48 jobs](https://github.com/houseme/rustfs-mimalloc/actions/runs/39154616201).
New-commit CI is checked separately after push.

## Upstream changes and PR status

The live merged-PR search since October 16 returns no new PRs. The QSBR branch
merge below is a Git commit, not a GitHub PR merge. Earlier PRs #1396, #1428
and #1429 are already present in the starting pin; their evidence is retained
in the historical reviews. [PR #1431](https://github.com/microsoft/mimalloc/pull/1431)
is open and unmerged. The latest native commit references it but implements a
different approach; neither that reference nor its newer unmerged patch counts
as a merged PR. The live PR discussion also documents the detector limitation
reproduced below.

| Commit / area | Actual change and relevance |
| --- | --- |
| [a7cc8442](https://github.com/microsoft/mimalloc/commit/a7cc8442b3757825e609f054333a5bd5bae9c7cc) | Hashes each small page's first-block offset to spread allocations across cache sets. Alignment tests remain important; payload traversal and allocation-only latency measure different effects. |
| [bd99378d](https://github.com/microsoft/mimalloc/commit/bd99378d8f389c93aeba7a1c329868e701ed1cfa), `c4a9f1ad`, [92984051](https://github.com/microsoft/mimalloc/commit/929840510ff880e4076cd5a5807c4c0cf2f2a9a7) | Introduce and merge runtime-oriented QSBR page-release primitives, with ordinary builds disabled by default. Fix 32-bit test support in `6a5e50bb`. |
| [e83c5605](https://github.com/microsoft/mimalloc/commit/e83c560535a32e2323f9f7bee2fb643bdd2ed1fe) | Reduces statistics merging work when source current/peak values are zero. Total counters still merge; the change is not removal of statistics. |
| [12e775db](https://github.com/microsoft/mimalloc/commit/12e775dbf7914929aa26aa2e5c08fff7a1cb2a56), [1d03ecbd](https://github.com/microsoft/mimalloc/commit/1d03ecbd19a07d8cbef4b93b4d08746f397bb59e) | Remote-free CAS uses release ordering while page ownership is already held; ownership transitions retain acquire-release. The explicit branch selects ordering from the observed ownership bit on every retry. |
| [4f3151bb](https://github.com/microsoft/mimalloc/commit/4f3151bb7b5e44c117fc3ab5108c30365c522ac2) | Adds prefetch macro definitions. No call sites exist in this pin, so this is not an enabled runtime-prefetch optimization. |
| [21d7e2ef](https://github.com/microsoft/mimalloc/commit/21d7e2ef7ae24cc40a799324b38fa25617b2832f) | Adds an initial `contrib/rust` crate with allocator, heap and sys APIs. It is distinct from the registry `mimalloc` 0.1.52 provider tested by this project. This update vendors C changes and retains the existing stable `GlobalAlloc` API/provider model; it does not substitute that new crate or add nightly allocator features. |
| [55fbfeb0](https://github.com/microsoft/mimalloc/commit/55fbfeb0896bec480997e4b86c8315710d809efa) | Replaces CRT `strtol`/`errno` in environment option initialization with internal `_mi_strtol`. This reduces CRT dependencies at startup, accepts hexadecimal and rejects zero-padded decimal numbers. It does not change a per-allocation algorithm or add public FFI. |
| [b4f183bb](https://github.com/microsoft/mimalloc/commit/b4f183bb1bbcccfade6f98fba30ef276cf1eb5b8) | Attempts to detect large OS pages from `/proc/meminfo` and disable THP advice above 8 MiB. The fixed read commonly misses the field, which also describes hugetlb rather than PMD THP size. No RSS improvement is established. |

## Follow-up findings and startup compatibility

**[src/prim/unix/prim.c:177] Large-page detection misses normal meminfo input.**
Severity: Major. The complete detector reads at most 512 bytes, then requires a
strictly shorter read before looking for `Hugepagesize`. A longer input always
keeps the 2 MiB fallback, leaving the new oversized-page guard inactive. An
exact-source C probe with fake file I/O returns 512 MiB for a short size-field
fixture but 2 MiB when the same field follows 1,024 bytes. The live PR author
reports the corresponding behavior on a 64 KiB-page Linux machine. Moreover,
the field is the hugetlb default, not THP's PMD size. The appropriate follow-up
is bounded parsing of the PMD sysfs value with explicit failure handling and
validation on the affected kernel. This update preserves upstream source;
it does not backport the still-open PR or claim to repair its detector.

Until then, affected applications can evaluate existing `no_thp` builds or
`MIMALLOC_ALLOW_THP=0` before startup. Verify RSS and latency on the deployment
kernel. Disabling allocator advice alone does not disable kernel `THP=always`;
the existing allow-THP opt-out also attempts Linux process-level `prctl`.
The synthetic probe validates parsing/fallback only, not a physical PMD or RSS.

**[src/util/options.c:709] Native K/KiB suffix omits scaling.**
Severity: Minor. This pre-existing parser branch advances past `K` without
multiplying the value. Independent child processes show
`MIMALLOC_STATS_MERGE_THRESHOLD=32KiB` produces 32 bytes and
`MIMALLOC_ARENA_PURGE_MIN_SIZE=32KiB` produces 1,024 bytes after native KiB
rounding. Both should describe 32,768 bytes under the header's documented
units. This issue already existed at `1d03ecbd`; it is not introduced by the
new integer parser. Use an unsuffixed byte count or `M`/`MiB` until corrected
upstream. Numeric Rust option setters are separate APIs and retain their
native option-specific units. README guidance now makes the limitation explicit.

The new `options_env` executable installs each environment before native startup
and checks 21 independent processes. Coverage includes signed/hexadecimal values,
native-long MIN/MAX on LP64 and LLP64 targets, overflow, malformed input,
zero-padded decimal rejection, MiB units and byte-to-KiB rounding. No test mutates
the running process's environment after allocator initialization. The new parser
and THP helper are internal; public headers, option IDs, callbacks and layouts
are unchanged by these last two commits. Previously exposed QSBR and renamed
option APIs remain available.

## Public API and configuration audit

`mimalloc-profile.h` and `mimalloc-stats.h` are unchanged; MI_STAT_VERSION is
still 6. The profiler callback table, public heap-area layout and option IDs
remain unchanged. The root `mimalloc.h` changes the numeric version and renames
one option. The new experimental factory is in **`mimalloc/qsbr.h`**, which is
now included in both API and ABI audits.

| Surface | Exposure and constraints |
| --- | --- |
| `mi_heap_new_qsbr()` | Added raw sys FFI. Default MI_QSBR=0 builds report ENOSYS and return null. The integration executable proves the symbol actually links and verifies the disabled result through the error callback. |
| Four QSBR runtime hooks | `mi_heap_qsbr_init`, `mi_page_qsbr_clear`, `mi_page_qsbr_is_clear`, `mi_page_qsbr_should_delay` are supplied by a host runtime. They are not allocator-exported functions to declare or call as ordinary Rust FFI. |
| `mi_option_unsafe_guarded_precise` | New associated constant at ID 31. The old `mi_option_guarded_precise` enum variant is retained, so qualified references and existing glob imports still compile. Audit validation follows the alias to the C name and rejects numeric drift. |
| `mi_option_page_reclaim_on_free` | Now copied into a heap during creation. Later option writes affect newly created heaps, not existing heaps or their future thread contexts. The main heap may be created before Rust main, so use the environment before process startup to configure its creation policy. |
| Guarded/profile documentation | Header comments clarify guarded sampling as requested bytes and native profile defaults. Corresponding option implementations already used these units/defaults at the old pin; this is not a new sampling algorithm in this interval. |

The factory is deliberately not a safe `Heap` constructor. Enabled QSBR builds
need host-defined, zero-initialization-compatible C metadata embedded in the
single-source build, a reader/grace-period implementation, and callbacks that
do not allocate, recurse or wait for readers under allocator locks. Protected
heaps reject normal `mi_heap_delete`, even when empty; terminal destruction
requires all readers and mutators to have stopped. The stable domain must
outlive its abandoned pages. QSBR delays page release, not block reuse, and does
not make arbitrary reads of freed Rust objects valid. There is no Cargo QSBR
feature or Rust reader algorithm in this delivery.

The upstream `test-qsbr` binary supplies its own fake runtime, compiles an enabled
adapter separately, and passes. It validates the primitives, not a production
Rust or CPython reader implementation. Ordinary crate layouts and fast paths
keep QSBR disabled.

API parity passes for **131 Rust function names / 45 option IDs**. The C compiler
projection passes **122 extern signatures / nine callbacks / three exposed C
layouts**. Seven positive/negative controls include legacy option spelling and
renamed-option numeric drift. These probes do not replace actual linking,
platform execution or the host runtime's safety proof.

## Functional and concurrency review

| Area | Finding / disposition |
| --- | --- |
| Remote-free ordering | Read the complete modified CAS loop and its ownership transition. Release publishes the freed block's list link; acquire-release remains when ownership changes. No Rust recorder atomics or general free routing were weakened. This is source reasoning plus regression coverage, not a formal weak-memory proof. |
| Contended free/reuse | Added eight-consumer free batches for 64 B, 4 KiB and 64 KiB blocks, checking payload and subsequent zero allocation. An additional two-producer/four-consumer test overlaps allocation, remote frees and owner-thread exit, drains 1,536 blocks and retains the parent heap throughout. It records corruption until after draining to avoid stranding producers on a failed data assertion. |
| Collection scope | `mi_heap_collect` in `src/heap/theap.c` resolves the calling thread's context; it does not walk every worker. Rust documentation now makes that scope explicit for both collect APIs. Calling collect on a coordinator is not a complete idle-worker/RSS reclamation policy. |
| Heap/context safety | Ordinary owned/borrowed heap lifetime behavior, !Send/!Sync cached views, aligned allocation, cross-thread free, context deletion and direct-OS-page tests remain enabled. QSBR handles stay raw because normal owning Drop would use the rejected delete operation. |
| Unsafe options | Setter docs explicitly require values to preserve active allocation contracts, in addition to excluding concurrent access to upstream non-atomic options. Precise guarded placement disables normal minimal rounding; ordinary aligned dispatch and defaults remain unchanged. |
| Native profiling | Empty native thread contexts still inherit `profile_disabled=true`. Main-heap callback tests pass; no claim of reliable worker/dedicated-heap native sampling is made. The independent Rust profiler does not use that activation path. |

## Profiling provider review

The predecessor 0.7.0 introduced `SamplingAllocator<A>`. Its constructor retains
the supplied instance, and each `GlobalAlloc` operation forwards the exact
pointer/layout/new-size contract to that instance. It uses static dispatch;
there is no provider selection or virtual dispatch added to allocation calls.
`SamplingMiMalloc::new()` remains behind the additive `rustfs-allocator` feature.
Default profiling dependencies remain allocator-neutral.

Both generated consumer workspaces pass shared behavior contracts and optimized
profile tests independently:

- RustFS provider selects only `rustfs-mimalloc` and `rustfs-mimalloc-sys` at 0.7.1,
  using this new C source pin.
- Registry provider resolves `mimalloc` 0.1.52 / `libmimalloc-sys` 0.1.49, with no
  RustFS allocator reachable. It does not consume the vendored development pin
  or the newly added upstream `contrib/rust` crate.

The stateful-provider tests cover alignment, zeroing, failure, in-place/moving
realloc and cross-thread free independently of provider size classes. Session
generations, visible TLS admission, live realloc reservation/rollback and
bounded drop reporting remain unchanged. Default-core, compatibility API and
normalized archive/provider checks pass at the new pin. Explicit pressure tests
and source-enabled optimized profiles remain evidence from the preceding pass.
Native `no_profile` does not disable Rust wrapper
sampling. No profiler algorithm or default sampling policy changes in this pass.

## Native performance diagnostics at the preceding 1d03ecbd pin

The preceding pass's two frozen release executables compare `6f45cc52` and
`1d03ecbd` with the same
Rust 1.99.1 compiler, locked dependencies and default features. Each A1/B1/B2/A2
round runs in a new process with 50 samples, one-second warm-up, two-second
measurement and one Rayon analysis thread. No task-owned build or benchmark
runs concurrently. The shared macOS arm64 host is **not resource-isolated**.
These retained numbers do not measure the subsequent `b4f183bb` Linux startup
change and must not be relabelled as benchmarks of the latest pin.

| Case | A1 | B1 | B2 | A2 | Unit | A / B drift | Conclusion |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `alloc_dealloc/mimalloc/64` | 3.387 | 3.420 | 3.426 | 3.509 | ns | 3.60% / 0.18% | No attribution |
| `alloc_dealloc/mimalloc/4096` | 8.792 | 8.909 | 9.062 | 9.228 | ns | 4.96% / 1.72% | No attribution |
| `alloc_dealloc/mimalloc/65536` | 10.538 | 11.490 | 11.531 | 10.906 | ns | 3.49% / 0.36% | No attribution |
| `aligned_alloc/mimalloc/256` | 3.840 | 3.897 | 3.904 | 3.888 | ns | 1.24% / 0.19% | No attribution |
| `aligned_alloc/mimalloc/4096` | 8.634 | 9.066 | 9.019 | 8.921 | ns | 3.33% / 0.53% | No attribution |
| `cross_thread_free_batch/mimalloc/4096` | 13.095 | 13.331 | 13.411 | 13.362 | us / 256 blocks | 2.04% / 0.60% | No attribution |

All six cases meet the 5% repeatability screen, but the resource-isolation gate
is not met. Every attribution/improvement field remains false/null. The 64 KiB
allocation case has higher point estimates at the new pin and deserves focused
controlled investigation. These loops do not measure multi-object payload
cache behavior, RSS or application throughput. Neither a production speedup nor
a regression attribution is established; no favorable retry replaces the data.
Raw logs, intervals and frozen-build provenance remain in `target/upstream-abba-071/`.
Repeat on isolated resources with a same-binary control before candidate
attribution. Cache-sensitive 64 KiB traversal and allocator-only latency should
be measured separately.

## Profiling quality diagnostics and optimization priorities

The preceding pin's supplied strict report-classifier controls pass. Its
one-second-per-row RustFS-provider diagnostic uses `1d03ecbd`. As in current CI,
quality enforcement is disabled for the real pressure run; rows retain explicit
PASS/LOSSY classification rather than treating dropped work as low overhead.

| Steady 1 MiB-interval scenario | Drained records | Record drops | Pending | Live drops | Periodic reports | Max report time |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Allocation | 84,551 | 0.047286% | 0 | 0 | 16 | 1,125 us |
| Live heap | 83,028 | 0.014451% | 0 | 0 | 17 | 1,082 us |

These observed steady rows pass the quality classifier; the allocation row
still raises the configured overhead warning. Small-recorder post-workload
rows are lossy and are not sustainable-overhead evidence. Live-table payload
is 15,826,944 bytes (about 15.1 MiB), excluding control bytes and headers.
No short-run overhead ratio is promoted to a release or production guarantee.
Raw diagnostics and history are in `target/review-071-profiling/`.

Prioritize further work in this order:

1. Correct PMD-size detection and validate THP/RSS on 16/64 KiB-page Linux
   systems before relying on the latest upstream guard. Audit deployed size
   environment values for the KiB suffix issue and zero-padded decimals.
2. Recheck the 64 KiB allocation point estimates on isolated Linux/macOS resources,
   then measure cache-sensitive payloads, RSS, faults and tail latency. Cache
   spreading can target payload conflicts while changing allocation-only costs.
3. Arrange collection/abandonment on the appropriate worker contexts before
   tuning retention, THP and purge policy for burst/idle services. Account for
   heap-creation-time reclaim defaults and measure OS-call/RSS tradeoffs.
4. Profile Portable versus Native capture on supported Linux GNU systems and
   tune sample interval/depth against accuracy and p99 budgets. Keep both
   dropped-record and live-drop counters in every cost comparison.
5. Investigate bounded stack interning/compact live entries and reporting lock
   hold times before changing defaults. Maintain realloc rollback, address-reuse
   exclusion, loader identity validation and visibility of idle worker records.

## Validation and boundaries

Stable Rust 1.99.1 / macOS arm64 local evidence at `b4f183bb`:

- Default workspace and secure/debug/no-profile/all-feature combinations pass.
  The final allocator suite has 44 cases; the added overlap case passes four
  configurations. The profiler has 69 unit cases, shared behavior contracts,
  missing-allocator rejection, stateful-provider and compatibility API tests.
- Both normally ignored pressure tests were explicitly executed in the preceding
  pass; those results are not a rerun at the new pin. Both isolated
  provider graphs pass behavior and optimized profile checks. Test-case
  serialization is not cited as race coverage: the contracts explicitly spawn
  their worker threads.
- Formatting, all-target check, all-target/all-feature Clippy with warnings denied,
  rustdoc with warnings denied, API/ABI parity and seven controls pass.
- All three package builds pass, including normalized profiling archive tests
  with local allocator/sys patches and both provider graphs. This does not claim
  that matching 0.7.1 registry packages were published.
- Upstream Debug/C `test-api`, `test-api-fill`, `test-arena`, `test-bitmap`,
  `test-fork` and enabled `test-qsbr` pass 6/6. CI now includes the QSBR target.
- Default and secure/debug/no-profile custom-tracking examples passed with actual
  hooks active in the preceding pass; the current pin is checked in CI.
  Runtime metadata and numeric version agree at 30600.
- Native option parsing passes all 21 child processes in every local feature
  configuration. The exact-source THP probe reproduces the documented fallback;
  separate diagnostic child processes reproduce the existing KiB suffix issue.
- Python 3.12 runs the provider/archive tools (which require 3.11+). No historical
  toolchain was installed for upstream's new Rust crate. Rust 1.95 compatibility,
  Linux/musl and Windows execution at this pin are checked by the matching CI.

All three Cargo versions and internal requirements match 0.7.1. This follow-up
changes neither the manifests' version requirements nor Cargo.lock. New-commit
CI results are reported after push. No release workflow
or package publication is requested.

## Historical reviews

Earlier pins, provider boundaries and measurements below are dated evidence,
not current validation or throughput conclusions.

## Three-crate review and 0.6.1 readiness — 2026-10-16

### Result and provenance

`git pull --ff-only` reported the repository current at `1fb36d2`. The starting
workspace was clean. All three crates remain **0.6.1**, with edition 2024,
matching workspace dependency requirements and no declared Cargo MSRV.
The empty Unreleased section is consolidated into the existing 0.6.1 section;
published 0.6.0 release notes are preserved. This pass commits/pushes code only,
without creating a tag or publishing packages.

The live upstream `dev3` head remains
[`6f45cc52`](https://github.com/microsoft/mimalloc/commit/6f45cc52f01413f2d86fe3c7c3ed6c6a86bdd13a),
which is already pinned. Fetch and public-header comparison show no source,
function, option or callback-layout delta. The latest upstream release remains
[v3.5.3](https://github.com/microsoft/mimalloc/releases/tag/v3.5.3), and the fixed
source remains on the post-v3.5.4 interim development line, version **30504**.
No submodule movement or new library API is warranted in this pass.

The merged-PR search since October 15 returns no additions. Refreshing the
October 14 onward search still returns [#1396](https://github.com/microsoft/mimalloc/pull/1396)
(custom allocation hooks), [#1428](https://github.com/microsoft/mimalloc/pull/1428)
(array-reallocation tests), and [#1429](https://github.com/microsoft/mimalloc/pull/1429)
(Windows console lifetime fix). Their changes are already incorporated and
covered by the dated upstream review below. All four recently introduced native
functions remain exposed: `mi_thread_id_of`, `mi_heap_set_debug_fill_preserve`,
`mi_theap_delete`, and `mi_theap_quiescent_abandon`.

The prior revision's [CI passed](https://github.com/houseme/rustfs-mimalloc/actions/runs/38897811238).
The new commit's CI result is checked separately after push. Rust 1.95.0 is an
explicit repository compatibility job, not a Cargo MSRV declaration; local
validation in this pass uses stable Rust 1.99.1.

### ABI verification improvement

The existing inventory passes for **130 Rust function names and 45 option IDs**,
but names and option values alone cannot detect signature/layout drift.
`python3 scripts/check_upstream_abi.py` now compiles C projections of the Rust
bindings against the pinned public headers, using strict compiler diagnostics:

- All **121 extern functions** have compatible parameter and return types in
  the supported C-compatible subset. The nine inline Rust helper functions
  remain covered by source review and runtime tests, not this C probe.
- All **nine callback aliases** are projected and checked against their C
  typedefs. The deferred-free heartbeat's C `unsigned long long` spelling is
  handled explicitly, with width/alignment checks against the fixed-width type.
- The heap-area, profiler sample-data and profiler callback-table projections
  match C size, alignment and every field offset for the host compiler.
- Unsupported aliases/calling conventions and unparsed declarations fail
  rather than being silently skipped.

`python3 scripts/test_upstream_abi.py` passes five controls: matching bindings
compile; a wrong return type, changed callback constness, reordered callback
fields and a changed size alias are rejected. The C compilation uses temporary
files and syntax checking only. CI runs both scripts; production allocator
code and hot paths gain no checks or branches.

This is a deliberately scoped C projection checker, not a Rust parser or proof
of every target ABI. It relies on the bindings' supported `repr(C)`/extern-C
forms and standard platform C aliases. It does not link symbols, validate inline
Rust helpers or allocator safety, exercise custom C compiler flags, or replace
actual Rust/C runtime integration. Local execution checks the macOS arm64 host;
CI also checks the Linux host compiler. Windows runtime and compatibility checks
remain in the existing matrix.

### Functional review

| Area | Evidence and conclusion |
| --- | --- |
| Allocator/sys | General aligned `GlobalAlloc` dispatch and cross-thread free behavior are retained. Cached `ThreadHeap` views borrow their heap and remain !Send/!Sync. Raw delete invalidates aliases; abandonment has explicit owner/quiescence requirements. Existing lifecycle, direct-OS-page, zeroing and alignment tests pass. |
| Independent Rust profiler | `SamplingMiMalloc` samples the Rust `GlobalAlloc` boundary, uses one exclusive `Profiler` owner, and returns owned pprof bytes. It is usable with the allocator's `no_profile` feature because it does not register the experimental native profiler. Direct C allocations remain outside this Rust wrapper's coverage. |
| Sampling and reporting | The byte Poisson sampler has one countdown fast path and one RNG draw per sampling hit. Allocation/live estimates share inclusion weights, preserve fractions until aggregation and retain physical addresses for offline symbolization. Statistical population tests pass. |
| Session/concurrency | Stop/restart generation checks, reporter-visible TLS admission, reentry suppression, fair drain budgets, cross-thread free removal and realloc rollback were traced with their tests. Both ignored pressure tests were explicitly executed; no stale worker live allocations were found. |
| Live metadata | Insertions are bounded/preallocated; removals can wait for a shard lock to avoid fabricated live entries. The membership filter is only a negative lookup shortcut; it is not a replacement for metadata removal. Reports copy shards separately, so they do not represent one stop-the-world instant. |
| Symbols/pprof | Frame resolution is shared within a report; mapping identity is revalidated on Linux. Optimized and source-enabled macOS tests preserve physical frames, source metadata when requested, live-object removal and standard profile sample types. Linux native capture is an explicit platform-specific mode. |
| Release boundaries | Dependency requirements live at the workspace root; every member inherits version 0.6.1. Three package archives were verified, including normalized profiling-package tests with matching local allocator/sys dependencies. Registry publication is outside this task. |

No new concrete allocator/profiler runtime defect was established in this pass.
The implemented improvement strengthens ABI maintenance coverage. Source-level
performance opportunities below need measurement before changing defaults.

### Performance diagnostics and priorities

The supplied benchmark-report controls passed. A one-second-per-scenario run
used portable capture, 64 B–64 KiB allocations and a shared macOS arm64 host.
The quality gate was enabled; throughput thresholds were diagnostic only.

| Scenario | Drained records | Record drop rate | Pending records | Periodic reports | Max report time | Sampled allocation p99 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Steady allocation, 1 MiB interval | 81,051 | 0.006169% | 0 | 17 | 975 us | 6,083 ns |
| Steady live heap, 1 MiB interval | 80,251 | 0.024916% | 0 | 17 | 1,049 us | 6,125 ns |

Both steady scenarios pass the record-quality gate, with zero live drops. The
allocation scenario still triggers the configured overhead warning; passing a
quality check does not mean that a 5% runtime-overhead target was met. The small
512-event post-workload recorder scenarios drop roughly 99% of allocation
records and are correctly marked LOSSY/QUALITY_WARN. Their apparent low cost
must not be used as sustainable profiling overhead evidence.

These are short diagnostics, not isolated A/B performance conclusions. There
is no runtime implementation change to compare in this pass, and the Linux-only
ABBA driver requires two isolated CPUs unavailable on this host. Its resource
gate was not bypassed. No speedup or production overhead percentage is claimed.
Raw output and the v4 history are in `target/review-061-quality/`; reproduce with
`MIMALLOC_BENCH_DURATION_MS=1000 MIMALLOC_BENCH_ENFORCE_QUALITY=1
bash scripts/mimalloc_benchmark_report.sh` and a fresh output directory.

Prioritized next work:

1. **Sampling cost under realistic traffic.** The steady diagnostics capture
   around 80,000 events/second. Compare sample intervals and stack depths under
   application accuracy/p99 budgets; on Linux GNU, compare Portable and Native
   with isolated same-binary controls and loss/drift gates. Keep the portable
   default until cross-platform evidence supports another policy.
2. **Live metadata budget.** The default 16,384-entry configuration reports
   15,826,944 bytes (about 15.1 MiB) of live-table payload, excluding hash-control
   bytes and headers. Entries retain fixed-depth stack keys, so metadata can be
   substantial even with few current live samples. Tune `max_live_samples` with
   drop counters first; investigate bounded stack interning or compact storage
   only with lifecycle/collision/concurrency and memory evidence.
3. **Reporting contention and phase costs.** Portable unwinding, reporter ring
   locks, live shard removal and loader/file metadata work are distinct costs.
   `last_pprof_encode_elapsed_micros` excludes aggregation and stack resolution.
   Measure whole-report latency and lock hold times before caching mappings or
   moving buffer allocations; retain fresh image identity and visible idle-thread
   records. The global recorder budget also excludes per-worker fixed TLS rings.
4. **Native allocator workload fit.** Continue Linux burst/idle, cross-thread
   producer-consumer, THP and cache-sensitive 64 KiB payload experiments with
   throughput, p95/p99, peak/idle RSS, page faults and reclaim costs. Native
   context abandonment is an opt-in tool, not a default memory-release policy.

### Validation

Local stable Rust 1.99.1 / macOS arm64 evidence:

- Default workspace tests pass: 42 allocator tests, 68 profiler unit tests,
  11 profiler behavior tests, optimized profile checks, missing-allocator
  rejection, standalone sys/startup tests, Windows source checks and doctests.
- `secure,debug`, `no_profile`, `secure,debug,no_profile` and all-feature tests
  pass. Both normally ignored profiler pressure tests pass when explicitly run.
- Optimized profile tests pass in ordinary release and source-enabled release
  (`CARGO_PROFILE_RELEASE_DEBUG=1`, no strip, source assertions enabled).
- Formatting, all-target checking, all-target/all-feature Clippy with warnings
  denied, rustdoc with warnings denied, source/API inventory, ABI projection and
  negative controls pass.
- Sys and allocator package builds pass. The normalized profiling archive passes
  all-feature tests and optimized profile validation using matching local path
  patches; this does not claim that 0.6.1 registry dependencies were published.
- All three metadata versions and internal requirements match 0.6.1. The single
  shared changelog is validated; the existing release workflow and tag behavior
  are unchanged.

### Historical reviews

Earlier results below belong to their original pins and package structures.
The former two-crate/native-profiler notes do not describe the independent Rust
profiling crate's capabilities or this pass's validation counts.

### Upstream and wrapper review — 2026-10-16

#### Result and scope

Both crates remain **0.6.0** with no declared Cargo MSRV. `git pull --ff-only`
was current and the starting workspace was clean. The cumulative changelog is
one `0.6.0` section dated 2026-10-16; there is no separate Unreleased section.
This delivery commits and pushes code only, without a tag or package publication.
The previous update's [full CI passed](https://github.com/houseme/rustfs-mimalloc/actions/runs/38589448844),
including Windows static CRT and clang-cl coverage.

The source advances from
[`4e64fce6`](https://github.com/microsoft/mimalloc/commit/4e64fce6248cb7247f66d900ee35c1b9e8ecc465)
to [`6f45cc52`](https://github.com/microsoft/mimalloc/commit/6f45cc52f01413f2d86fe3c7c3ed6c6a86bdd13a),
**31 commits** later. The latter was committed October 15 UTC / October 16 in
Asia/Shanghai. It is a fixed `dev3` development snapshot after the v3.5.4 interim
tag. The C version remains **30504**; the latest published upstream release is
still [v3.5.3](https://github.com/microsoft/mimalloc/releases/tag/v3.5.3).

#### Merged PRs and direct development

The live merged-PR search since October 14 returned three PRs. Closed proposals
were inspected separately; a direct incorporation is not reported as a merge.

| Item | Verified status and relevance |
| --- | --- |
| [#1396](https://github.com/microsoft/mimalloc/pull/1396) | Merged into dev3 October 14 UTC as `7e2e23fb`. Adds application-defined allocation/free hooks under `MI_TRACK_CUSTOM`. A runnable Rust example now verifies requested size, usable size and matching free metadata. |
| [#1428](https://github.com/microsoft/mimalloc/pull/1428) | Merged into dev3 October 14 UTC as `95572b93`. Tests array-reallocation growth, zero count and overflow preservation; also corrects the C++ test include. These are upstream test additions, not a new reallocation API. |
| [#1429](https://github.com/microsoft/mimalloc/pull/1429) | Merged into dev October 14 UTC. The equivalent dev3 fix is `097bde14`: diagnostics no longer attach GUI/detached processes to the parent console; the missing-stderr fallback uses `OutputDebugStringA`. |
| [#1408](https://github.com/microsoft/mimalloc/pull/1408) | Closed without GitHub merge status. Direct dev3 commits `5a9a79a4` and `a7875866` incorporate/refine natural-alignment size-class improvements. Unpadded zero-offset requests can avoid excess backing allocation. |
| [#1339](https://github.com/microsoft/mimalloc/pull/1339) | Closed without GitHub merge status. Direct commits `cf9c4e2b` and `a0c58a2f` add cache offsets and move 64 KiB blocks to large pages. This targets cache-set conflicts; the author's measurements are not measurements of this crate or host. |
| Windows RNG | `0c23e36d` and `80d7962f` adjust legacy loading and use documented ProcessPrng on Windows 8+, with RtlGenRandom fallback; UWP/Xbox keeps linked BCryptGenRandom. Existing CRT compatibility remains guarded by source-drift tests and CI. |
| Heap lifecycle / walking | Fix direct-OS-page deletion/destruction and make owned OS pages visible during heap walking. Track OS page count to skip unnecessary scans. Rust integration now forces direct OS allocation and checks live-block preservation on heap deletion. |
| Quiescent fork cleanup | Registers an atfork child handler to abandon pages belonging to vanished threads. Other threads must already be quiescent outside mimalloc. This is not unrestricted allocator use after arbitrary multithreaded Rust forks. |
| Inline linkage | `cee18e1c` fixes the upstream static-in-inline diagnostic seen at the prior pin. The new Cargo C build no longer emits those arena warnings. |

#### Public API delta and exposure

`mimalloc.h` adds exactly four public functions. `mimalloc-profile.h` and
`mimalloc-stats.h` are unchanged. Option IDs, the profiler callback table, heap
area layout and the numeric C source version are unchanged.

| New upstream API | Rust exposure and contract |
| --- | --- |
| `mi_thread_id_of` | Raw FFI and unsafe `MiMalloc::thread_id_of`. Accepts null or a live allocation; returns zero for null/abandoned/detached pages. The value is a page-owner snapshot, not a Rust `ThreadId`, synchronization primitive or proof that local-only freeing is valid. |
| `mi_heap_set_debug_fill_preserve` | Raw FFI and safe `Heap::set_debug_fill_preserve`. The setting is atomic and clamped to 1 MiB. Debug fills skip the prefix, but zero allocation, padding checks and free-list writes still apply. This does not extend object lifetime or permit freed-object reads. |
| `mi_theap_delete` | Raw unsafe FFI only. Owner-thread deletion abandons pages and preserves live allocations/the parent heap. Every cached context alias must be treated as invalid; reacquire through the heap. The upstream main-context special case retains its backing context. A method consuming one borrowed view would not invalidate other borrowed views, so no wrapper deletion method is added. |
| `mi_theap_quiescent_abandon` | Raw FFI and unsafe `ThreadHeap::quiescent_abandon` for the calling thread's view. Live blocks and context remain usable. Foreign-thread cleanup is raw-only and requires an owner quiescent outside mimalloc, a live parent heap and no concurrent heap/context deletion. It need not return memory immediately to the OS. |

The declarations and implementations were read together. In particular,
`src/heap/theap.c:513` temporarily clears `theap->tld` while abandoning pages;
its locks do not make concurrent owner allocation safe. The wrapper keeps
`ThreadHeap` lifetime, !Send and !Sync properties and documents the remaining
unsafe conditions. `src/heap/heap.c:31` stores the debug setting atomically,
which supports the safe setter on a borrowed live heap.

`python3 scripts/check_upstream_api.py --json target/upstream-api-audit-20261016.json`
passes for **130 Rust function names and 45 option IDs**. All four newly declared
C APIs have bindings. The inventory still records 98 header-declared functions
outside the binding scope; name parity alone does not verify C ABI or linkage.
Structured statistics, legacy aliases, subprocess migration and experimental
arena persistence remain outside the wrapper scope. Actual calls verify the
new functions' linkage and behavior.

#### Custom tracking support

`rustfs-mimalloc/examples/custom_tracking.rs` supplies the exact two C symbols
required by upstream's internal tracking header. Enable them explicitly with:

```sh
CFLAGS=-DMI_TRACK_CUSTOM cargo run --release -p rustfs-mimalloc --example custom_tracking
```

The example was run in release/default mode and in debug with
`secure,debug,no_profile`. Both observed the allocation/free metadata and fail
if tracking is inactive. Hooks use atomics only, including during allocator
startup. Custom tracking requires no new Cargo feature or default hot-path
registration/dispatch. Applications must supply both hook symbols in every
final binary built with this C flag.

These hooks can run concurrently and must not allocate, panic or unwind.
Aligned requests can report the backing block instead of the adjusted user
pointer, and heap destruction does not emit one free hook per block
(`MI_TRACK_HEAP_DESTROY=0`). A hook event stream alone is not a complete leak
or Rust-object lifetime model. CI now runs the example on Linux/macOS/Windows
in both supported configurations.

#### Functional review and remaining optimization work

No new wrapper memory-safety defect was established in the reviewed global
allocator, owned/borrowed heaps or cached contexts. The main boundary risks in
this update are lifecycle aliases, quiescence and misleading owner-ID checks;
the APIs retain explicit unsafe contracts and regression coverage.

| Priority | Finding / next action |
| --- | --- |
| P1, covered | Raw context deletion invalidates aliases (`rustfs-mimalloc-sys/src/lib.rs:467`). Keep it out of the borrowed wrapper. The integration test makes it the default context, deletes it, checks default reset/live-block survival, and reacquires a replacement. |
| P1, covered | Foreign abandonment needs more than a lock (`src/heap/theap.c:513`). The regression uses a release/acquire handshake and an atomics-only owner wait, checks owner ID becomes zero, then resumes allocation with the same context. |
| P2, covered | Direct OS pages were missed by upstream heap lifecycle paths. A serial integration test temporarily disables arena allocation, verifies heap-delete survival and runs heap-destroy without reusing dead pointers. It restores the process-global option before later tests. |
| P1, known upstream concern | The static empty context still has `profile_disabled=true`; initialization copies it without clearing the flag. This pass verifies main-heap samples, not reliable worker/dedicated-heap profiling. Reproduce and repair worker activation before using pprof for multi-thread attribution; no new worker-profiling performance claim is made. |
| P2, measurement priority | Measure 64 KiB/cache-sensitive payload traversal after the cache-offset change, with working sets crossing cache levels. The included microbenchmarks measure allocation/free overhead and do not establish application cache benefits. |
| P2, measurement priority | Compare burst/idle producer-consumer workloads under different page-retention, purge and Linux THP policies using p95/p99, peak/idle RSS, faults and OS calls. Context abandonment is now an explicit tool for idle workers; measure its latency/reclaim costs before automatic use. |
| P2, API maintenance | Header name/option parity is useful but does not validate callback signatures or layout. Keep exercised ABI/link tests, and add targeted C/Rust layout/signature probes when structured ABI surfaces are introduced. |

The global allocator's Rust dispatch, alignment guarantees, general free path,
feature defaults, retention/purge policy and CPU architecture baseline are
unchanged. Imported upstream C behavior, including cache offsets, does change.

#### Performance experiment and rejected attribution

Frozen release executables compare the old and new pins with the same Rust
1.99.1 compiler, lockfile and default features. A1/B1/B2/A2 run in fresh processes,
50 samples, 1 second warm-up and 2 seconds measurement per case, with
`RAYON_NUM_THREADS=1`. No other task-owned build or benchmark ran concurrently.
The shared macOS arm64 host was not resource-isolated.

| Case | A1 | B1 | B2 | A2 | Unit | A / B drift | Decision |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `alloc_dealloc/mimalloc/64` | 15.870 | 8.450 | 7.628 | 8.145 | ns | 48.68% / 9.72% | Rejected |
| `alloc_dealloc/mimalloc/4096` | 38.003 | 20.842 | 29.292 | 28.336 | ns | 25.44% / 40.54% | Rejected |
| `alloc_dealloc/mimalloc/65536` | 33.660 | 24.946 | 44.310 | 43.782 | ns | 30.07% / 77.62% | Rejected |
| `aligned_alloc/mimalloc/256` | 24.794 | 7.686 | 10.438 | 19.953 | ns | 19.52% / 35.81% | Rejected |
| `aligned_alloc/mimalloc/4096` | 27.095 | 27.955 | 37.862 | 31.720 | ns | 17.07% / 35.44% | Rejected |
| `cross_thread_free_batch/mimalloc/4096` | 33.640 | 44.399 | 64.428 | 41.085 | us / 256 blocks | 22.13% / 45.11% | Rejected |

**All six comparisons fail the 5% repeatability gate.** Every improvement field
is null. The failures are retained; this round makes no throughput improvement
or regression attribution and does not rerun measurements to select a favorable
result. Source-level reduction of excess aligned backing allocation and cache
index changes are included; their workload benefit still needs controlled
measurement. Previous dated measurements below remain historical.

Raw intervals, logs, frozen-build provenance and decisions are under
`target/upstream-abba-20261016/`. Reproduce by freezing the `alloc` release
benchmark executable at each exact pin, then running identical filters in
A/B/B/A order with the parameters above and a 5% gate on both A1/A2 and B1/B2.
Do not compare timings collected while rebuilding or from different build modes.

#### Validation

On stable Rust 1.99.1 / macOS arm64:

- Default, `secure,debug`, `no_profile`, `secure,debug,no_profile` and all-feature
  tests passed: 42 wrapper tests, 6 doctests, 4 Windows source-compatibility tests,
  standalone startup and upstream API executables.
- Added lifecycle checks exercise owner IDs, cached context reuse after abandon,
  owner-thread delete/default reset, foreign-thread cleanup with explicit
  quiescence, and heap deletion/destruction with direct OS pages. Zero allocation
  remains zero with debug-preserve values 0, 16 and `usize::MAX`.
- All-target check, formatting, all-feature/all-target Clippy with warnings denied,
  rustdoc with warnings denied, both offline package build verifications and
  source parity passed. Wrapper packaging uses the temporary unpublished-sys
  path patch. Both packages still resolve to 0.6.0.
- Custom tracking example passed both runtime configurations with actual hooks
  active, in a separate target directory.
- Upstream CMake Debug/C tests `test-api`, `test-api-fill`, `test-arena`,
  `test-bitmap` and `test-fork` passed **5/5**. The fill test exercises prefix
  preservation directly; the fork test uses explicitly quiesced worker threads.
  These tests are added to Linux/macOS CI.

The prior commit's CI is confirmed green. The new commit's CI result is reported
separately after push; local checks do not establish Windows, Linux, musl or
older-toolchain execution at the new pin. No release/tag workflow is requested.

#### Historical reviews

The following sections retain earlier pins, results and limitations at their
original dates; they are not current validation or performance conclusions.

#### Upstream and wrapper review — 2026-10-14

##### Result and provenance

The workspace stays at **0.6.0**. The former Unreleased changelog is consolidated
under `0.6.0` dated 2026-10-14. This delivery commits and pushes code only; it
creates no release tag and publishes no package. `git pull --ff-only` reported
that the main repository was current. Existing `cc`/lockfile dependency updates
were retained and validated with Rust 1.99.1 stable. Cargo still declares no MSRV.

The source advances from
[`a28efddd`](https://github.com/microsoft/mimalloc/commit/a28efddd3658084937d285858772481f676c4045)
to [`4e64fce6`](https://github.com/microsoft/mimalloc/commit/4e64fce6248cb7247f66d900ee35c1b9e8ecc465),
34 upstream commits later. The new commit was recorded on October 14 UTC.
This is a fixed `dev3` development snapshot after the v3.5.4 interim tag;
`MI_MALLOC_VERSION` remains **30504**. The latest published upstream release
remains [v3.5.3](https://github.com/microsoft/mimalloc/releases/tag/v3.5.3),
and `main3` still points to its release merge at the time of this review.

##### Upstream changes and merged PR audit

| Item | Verified effect and disposition |
| --- | --- |
| [dc0bfd76](https://github.com/microsoft/mimalloc/commit/dc0bfd763b561f00ea715658b2ef60fea00c66c4) | Restore the conservative chunk-map bit after temporarily claiming bitmap bits, including failed claims. This prevents available ranges from disappearing from later searches under contention. Newly included. |
| [0e853a07](https://github.com/microsoft/mimalloc/commit/0e853a07c2c0b36bb0a3d344e59279eccf4c983d) | Republish the failing chunk during a multi-chunk huge-allocation rollback, as well as previously claimed chunks. Newly included. |
| [4e64fce6](https://github.com/microsoft/mimalloc/commit/4e64fce6248cb7247f66d900ee35c1b9e8ecc465) | Restrict the AVX2 bitmap path to 64-bit. Our build does not force AVX2 or an upgraded CPU baseline. Newly included. |
| Internal source reorganization | Allocation, arena, heap and utility code now occupy separate directories. Cargo still builds upstream's single `src/static.c`; Windows compatibility generation now reads/includes `heap/theap.c`. Newly included. |
| Immediate arena purge threshold | The default increases from 4 MiB to 32 MiB. This may reduce OS purge calls while retaining more memory; application throughput and RSS require measurement. Newly included. |
| [#1412](https://github.com/microsoft/mimalloc/pull/1412) | Merged October 5 UTC; adds the stats-merge threshold control. Already included in the old pin and already bound. |
| [#1420](https://github.com/microsoft/mimalloc/pull/1420) | Merged October 3 UTC; invalidates stale cached thread heaps on heap destruction and collects pending remote frees before resetting page usage. Already included; regression tests remain enabled. |
| [#1419](https://github.com/microsoft/mimalloc/pull/1419) | Merged October 3 UTC; clang C builds targeting MSVC use C11 atomics. Already included; Windows clang-cl CI remains enabled. |
| [#1416](https://github.com/microsoft/mimalloc/pull/1416), [#1418](https://github.com/microsoft/mimalloc/pull/1418) | Merged October 3 UTC into dev and carried into dev3; default-build usable-size linkage and self-contained bitmap headers. Already included. |

The latest merged-PR list was read independently of the commit history. There
are no newly merged PRs after the old pin among the recent upstream results;
the 34 added commits are development/refactor changes, not new release PRs.

##### Public API audit and implementation

`mimalloc.h` and `mimalloc-stats.h` are unchanged between the two pins.
`mimalloc-profile.h` changes only formatting and a documentation link.
There are **no newly added public C functions, option IDs or profiler ABI fields**
in this interval. The existing ABI's `on_snapshot` and mutable heap arguments
remain correctly represented.

The audit found useful existing allocation capabilities missing from Rust:

| Rust surface added | Behavior and contract |
| --- | --- |
| `MiMalloc::expand` | Reuse usable capacity without moving or copying. Null/failure preserves the original allocation. Padding-enabled builds reject expansion; added bytes are not initialized. |
| `MiMalloc::rezalloc`, `rezalloc_aligned` | Preserve initialized content and zero growth for originally zero-initialized allocations. Aligned variants retain the original power-of-two alignment. |
| `Heap::calloc`, `rezalloc`, `rezalloc_aligned`, `recalloc` | Checked array multiplication and zero-initialized growth. Failure, including overflow, leaves the original block valid. |
| `ThreadHeap::calloc`, `malloc_csize`, `zalloc_csize`, `realloc`, `rezalloc` | Extend existing cached worker contexts without repeated TLS lookup. Borrowed parent lifetime and thread affinity are retained; allocation methods remain unsafe. |
| 13 raw C bindings | `mi_expand`, `mi_rezalloc`, `mi_recalloc`, `mi_rezalloc_aligned`, `mi_heap_mallocn`, `mi_heap_malloc_small`, `mi_heap_zalloc_small`, `mi_heap_reallocn`, `mi_heap_rezalloc`, `mi_heap_recalloc`, `mi_heap_calloc_aligned`, `mi_heap_rezalloc_aligned`, `mi_heap_recalloc_aligned`. |

`python3 scripts/check_upstream_api.py --json target/upstream-api-audit.json`
checks **126 Rust function names and 45 numeric option IDs** against the pinned
public headers. CI runs it on every change. The JSON inventory also lists
98 C exports outside the current binding scope. This checks source parity;
it does not independently prove C signatures, struct layout or linkability.
New allocation signatures were reviewed against the C declarations, and actual
calls exercise linkage, alignment, content preservation, zero growth and overflow.

Legacy libc/C++ compatibility aliases, subprocess migration/destruction,
experimental arena persistence/callback-managed memory and structured statistics
are still outside the wrapper scope. Thread-local aligned reallocation helpers
are **static internal C functions**, so they are not declared as exported FFI.
The changing `mi_stats_t` layout remains unmirrored; JSON/text diagnostics avoid
that extra ABI dependency. There is no new safe profiler owner while sampled
allocations and callbacks can outlive registration.

##### Review findings and optimization priorities

| Priority | Evidence and disposition |
| --- | --- |
| P1, fixed | Upstream removed `src/theap.c` and changed the static aggregator include. The prior Windows static-CRT generator would fail reading/patching it. Generator paths and source-drift tests now follow `heap/theap.c`; both CRT-mode source checks pass locally. Actual Windows startup is covered by CI. |
| P2, fixed | Sys constant-size free docs permitted over-aligned small blocks without naming their restriction. Heap alignment/reallocation and managed-arena contracts also omitted obligations. Documentation now states aligned-free routing, exclusive ownership, failure semantics, accurate flags and OS-region lifetime. |
| P2, improved | Manually copied FFI/option values had no automated full name/option inventory. The new header parity gate detects renamed/removed binding names and numeric option drift. Runtime tests still provide independent behavior evidence. |
| P1, open upstream | `_mi_theap_empty.profile_disabled` remains true and `_mi_theap_init` copies that template without clearing the flag. Dedicated-heap and worker sampling remains unreliable. Keep profiling experimental; first repair activation and prove worker sample coverage before using pprof to guide concurrency optimizations. |
| P2, next measurement | Linux producer/consumer and burst/idle workloads should compare purge thresholds, retention and THP using throughput, p95/p99, peak/idle RSS and page faults. The new 32 MiB purge threshold makes this more relevant. No Linux policy tuning is inferred from macOS allocator microbenchmarks. |
| P2, next measurement | Benchmark naturally aligned allocation routes against the current always-aligned `GlobalAlloc` route, and add cross-thread/large-alignment/realloc cases before changing dispatch. Never replace general frees with local-only frees: same-thread allocation does not prove current page ownership. |
| P3, caller optimization | Reuse `ThreadHeap` for workers alternating heaps; use constant-size helpers where the size is known, and try `expand` where capacity reuse avoids copying. Keep explicit opt-ins and validate application payload work; short single-heap loops need not benefit. |

No new safe-code memory defect was established in the reviewed global allocator
or heap ownership implementation. Raw-pointer operations remain subject to their
unsafe contracts. Windows, Linux, musl and 32-bit runtime behavior is not proven
by macOS host tests.

##### Performance evidence

Two release benchmark executables were frozen before measurement. A uses the
old upstream pin; B uses the new pin, the same stable compiler, lockfile,
default features and unchanged `GlobalAlloc` implementation. A1/B1/B2/A2 each
run in a fresh process with 50 samples, 1 second warm-up and 2 seconds measurement
per case. Statistical analysis uses one Rayon thread. No other build or benchmark
was run during these measurements. Both baseline and candidate repeatability
must remain within 5%; the macOS arm64 host is shared, not resource-isolated.

| Workload | A1 | B1 | B2 | A2 | A / B drift | Exploratory result |
| --- | --- | --- | --- | --- | --- | --- |
| 64 B alloc/free | 3.593 ns | 3.495 ns | 3.498 ns | 3.577 ns | 0.43% / 0.07% | 2.46% lower mean time |
| 4 KiB alloc/free | 8.473 ns | 7.939 ns | 8.310 ns | 8.567 ns | 1.11% / 4.68% | 4.65% lower mean time |
| 64 KiB alloc/free | 11.229 ns | 10.986 ns | 10.872 ns | 11.395 ns | 1.48% / 1.04% | 3.39% lower mean time |
| 4 KiB remote-free batch, 256 blocks | 13.034 us | 12.407 us | 12.103 us | 11.846 us | 9.11% / 2.45% | Baseline drift failed; no attribution |
| Two alternating heaps, cached, 64 B | 5.125 ns | 4.897 ns | 5.111 ns | 5.070 ns | 1.08% / 4.36% | 1.84% lower mean time |
| Two alternating heaps, lookup, 64 B | 14.078 ns | 14.557 ns | 14.458 ns | 14.401 ns | 2.29% / 0.68% | 1.88% higher mean time |

These small point-estimate differences are diagnostic, not a statistically
established release-wide speedup. The inconsistent cross-thread case is retained
with a null improvement; it was not rerun to select a favorable result. No RSS,
tail-latency, full-buffer or production workload benefit is claimed. The added
allocation APIs were not invoked by these existing benchmark loops, so this
experiment does not measure their individual benefit.

Raw logs, intervals, frozen-build provenance and the drift decisions remain in
`target/upstream-abba/{A1,B1,B2,A2}.log` and `target/upstream-abba/results.json`.
To repeat, build `cargo bench -p rustfs-mimalloc --bench alloc --no-run --locked`
at each pin, copy the emitted executable before switching pins, then run the
frozen executables in A/B/B/A order with `RAYON_NUM_THREADS=1`, `--sample-size 50`,
`--warm-up-time 1`, `--measurement-time 2`, `--noplot` and identical exact case
filters. Time reduction is `1 - (B1 + B2) / (A1 + A2)` only when both drift gates
pass. The benchmark directory is ignored and is not included in the package.

##### Validation

On stable Rust 1.99.1 / macOS arm64:

- Baseline tests passed before editing. After synchronization, default,
  `secure,debug`, `no_profile`, `secure,debug,no_profile` and all-feature tests
  passed: 40 wrapper unit tests, 6 doctests, 4 Windows source-compatibility tests,
  standalone startup and upstream API integration executables.
- Existing heap-destruction checks observed and validated 32 address replacements
  in each tested configuration. Main-heap profiler callbacks, pprof snapshots,
  disabled automatic profiling, malloc override and THP defaults remain covered.
- Formatting, all-target checking, all-feature Clippy with warnings denied,
  documentation with warnings denied, API parity and diff whitespace checks passed.
- Both 0.6.0 crates passed offline package build verification; wrapper verification
  used the temporary local sys dependency patch required before publication.
- Upstream CMake debug/C builds passed `test-api`, `test-arena`, and `test-bitmap`
  (3/3). These exercise the refactored arena/bitmap implementation independently
  of the Rust wrappers. The CMake build uses separate translation units; Cargo
  uses upstream's static aggregator.
- The Cargo C compiler emits two upstream `-Wstatic-in-inline` diagnostics in
  `arena.c`. Both builds and tests pass; upstream CMake explicitly suppresses
  this warning. No vendored source or CPU baseline was changed to hide it.

Cross-platform CI is triggered by the code push. Local success does not establish
Windows runtime startup, Linux/musl execution or older-toolchain compatibility;
those results must be read from the matching CI run. No tag or publication is
part of this delivery.

##### Historical reviews

The following sections retain prior evidence at their original pins and dates.
Their measurements and validation counts are historical, not current results.

##### Upstream and wrapper review — 2026-10-04

###### Current result

The crate version remains **0.6.0**, as requested. The three existing
`rust-version` removals are included; both workspace packages report no Cargo
Rust-version requirement. The compatibility CI job still tests Rust 1.96.0.
`git pull --ff-only` was up to date. The previous 0.6.0 commit's full CI run
[passed](https://github.com/houseme/rustfs-mimalloc/actions/runs/37008507453).

The submodule advances from v3.5.4 interim (`f8401bef`) to the fixed
[dev3 commit 8bd60cf0b8d0ff9086be5519b18d9843bf2ceeff](https://github.com/microsoft/mimalloc/commit/8bd60cf0b8d0ff9086be5519b18d9843bf2ceeff),
13 commits later. This is a fixed development commit, not a newly released
mimalloc version. `MI_MALLOC_VERSION` remains 30504. The latest full GitHub
Release and main3 still reference v3.5.3 at review time.

###### Profiling-free build follow-up

Added the opt-in `no_profile` feature, keeping default behavior unchanged. It
sets `MI_PROFILE=0` while retaining allocation alignment, zeroing, statistics,
error/output/deferred-free callbacks, and secure/debug checks. Guarded debug
allocations may still use sampling. `MiMalloc::profiling_enabled()` reads the
sys crate's `MI_PROFILE_ENABLED`, so it reports the linked mode even when another
dependency enables the sys feature through Cargo feature unification.

Raw profiler control/snapshot symbols remain available. Allocation/free hooks
produce no samples in this mode; explicitly invoked snapshot callbacks still
execute. Automatic `MIMALLOC_PROFILE` startup is disabled and tested in a child
process. Applications requiring profiling must leave `no_profile` off.

The initial `-DMI_PROFILE=0` probe failed to compile: the pinned upstream stub
for `mi_pprof_profiler_snapshot` takes `mi_profiler_t*`, while its forward
declaration takes `mi_pprof_profiler_t*`. The feature sets
`MI_PROFILE_USE_BUILTIN=1` to retain the compatible implementation; `MI_PROFILE=0`
still disables allocation sampling and automatic startup. No vendored source
was modified. Tests exercise both sampling modes instead of simply skipping
profiler coverage under the new feature.

`python3 scripts/bench_profile_abba.py --offline --output target/profile-abba-run1`
builds and freezes two release executables before A1/B1/B2/A2 measurement. It
uses 50 samples, 3 seconds warm-up, 5 seconds measurement, one statistical
analysis thread and 5% baseline/candidate drift limits. Raw logs remain in
`target/perf-profile-off/feature-abba`; the failed compile and exploratory screen
preceded this formal feature-based run and are not used as speedup evidence.

A = default; B = no_profile. Point estimates on the macOS aarch64 host:

| Workload | A1 | B1 | B2 | A2 | Baseline / candidate drift | Result |
| --- | --- | --- | --- | --- | --- | --- |
| 64-byte alloc/free | 3.5717 ns | 3.6817 ns | 3.6911 ns | 3.5047 ns | 1.88% / 0.26% | 4.2% slower |
| 4 KiB alloc/free | 8.1380 ns | 7.7050 ns | 9.1241 ns | 8.3321 ns | 2.39% / 18.42% | Invalid comparison; no speedup claim |
| 64 KiB alloc/free | 10.852 ns | 9.4248 ns | 9.7034 ns | 11.175 ns | 2.98% / 2.96% | 13.2% lower time |
| 4 KiB cross-thread batch (256 blocks) | 15.368 us | 15.003 us | 15.077 us | 15.536 us | 1.09% / 0.49% | Point estimate 2.7% lower, overlapping intervals; no clear win established |

The single-thread cases do not touch the allocation payload; the cross-thread
case touches only its first byte. This is allocator-overhead evidence rather
than application or full-buffer processing throughput.

This is a build-mode tradeoff, not a universal speedup. The driver exits with a
failed gate for the inconsistent 4 KiB case and records its improvement as null;
that failure is retained. No default build or memory-retention policy changed.
Workloads dominated by tiny allocations should keep the default unless their
own measurements favor the new mode. Larger-allocation users may evaluate it
when profiling is unnecessary. Linux/Windows application throughput and RSS
have not been measured in this experiment.

Validation includes zero allocation callbacks with no_profile, retained explicit
snapshot callbacks, empty allocation snapshots, ignored environment startup,
statistics and secure/debug mode reporting, and direct sys-feature unification.
The existing cross-thread, heap destruction, alignment, zeroing and ThreadHeap
lifetime checks also run in the new mode. CI and release gates now cover the
feature, including secure/debug combinations and clang-cl/musl builds. Local
default, no_profile, secure/debug/no_profile, all-feature and release/no_profile
runs passed (36 unit tests, 6 doctests and the sys integration executable).
Clippy, all-feature rustdoc and both no_profile package builds also passed;
wrapper packaging used a temporary local registry patch for the sys crate.

###### Performance implementation follow-up

Added `Heap::thread_local()` and the borrowed `ThreadHeap` view. It caches the
upstream per-thread heap context, eliminating repeated TLS/cache lookups when
one worker alternates between heaps. The view occupies one pointer plus a
zero-sized lifetime marker, is neither Send nor Sync, and cannot outlive its
parent heap. Its allocation methods remain unsafe and preserve upstream
alignment/zeroing/OOM contracts. Allocations can be freed on another thread;
the view itself must not be used during thread-local destruction or after
mimalloc thread finalization.

The accepted experiment uses the public Rust APIs, two alternating heaps, and
two aligned allocation/free pairs per measured iteration. Build: release,
default features, the fixed upstream pin above. Each round runs in a fresh
process with 50 samples, 3 seconds warm-up and 5 seconds measurement per size.
Criterion statistical resampling uses one Rayon thread, outside the measured
loop. This avoids its default parallel analysis heating all cores between
neighboring measurements. The host is shared, so the driver requires both
baseline and candidate repeatability within 5%.

| Size | A1: Heap (ns) | B1: ThreadHeap (ns) | B2: ThreadHeap (ns) | A2: Heap (ns) | A drift | B drift | Time reduction |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 64 bytes | 13.512 | 4.7772 | 4.8020 | 13.716 | 1.51% | 0.52% | 64.8% |
| 4096 bytes | 25.249 | 16.302 | 16.112 | 26.100 | 3.37% | 1.17% | 36.9% |

Time reduction compares the mean of B1/B2 with the mean of A1/A2. These are
microbenchmark estimates for two alloc/free pairs, not production throughput
or the previous producer/consumer workload. Single-heap tests did not show a
stable advantage; callers should cache views where multiple heaps are active.
The default `GlobalAlloc` remains unchanged.

Reproduce with `python3 scripts/bench_heap_abba.py --offline --output
target/heap-abba-run1` after dependencies are cached. The script writes logs
and JSON, preserves earlier output directories and rejects a speedup conclusion
when either drift gate fails. Raw measurements for this run remain under the
ignored `target/perf-2026-10-04` directory.

The earlier attempts are retained as negative evidence:

- Increasing `MIMALLOC_PAGE_FULL_RETAIN` to 16 reduced observed 4 KiB batch times,
  but baseline drift was 7.5%, outside the 5% gate. No policy change or validated
  speedup follows from that run. Higher retention also needs peak/idle RSS tests.
- The first public-API ABBA run with parallel statistical analysis failed: 64-byte
  baseline drift was 5.74%, and 4 KiB candidate drift was 9.14%. Its speedup
  fields are null. The accepted run above changed the measurement protocol
  explicitly; it did not discard these failures.
- External CPU sampling of the existing 4 KiB cross-thread batch identified
  `mi_page_queue_find_free_ex`, `mi_arenas_try_alloc` and fresh arena/page
  allocation paths. This is consistent with page lifecycle churn, beyond the
  cost of merely passing pointers through channels. It motivates isolated
  Linux RSS/latency experiments rather than a global retention-default change.

Correctness validation covers alternating heap ownership, small and over-aligned
allocation, zeroing, cross-thread freeing, and live allocations after parent
heap deletion. Compile-fail doctests verify parent lifetime, !Send and !Sync;
a runnable doctest checks the intended usage. Default, secure/debug, all-feature
and release validation passed (36 unit tests, 6 doctests, plus the sys integration
executable). Clippy, rustdoc and wrapper package verification also passed. The
version remains 0.6.0.

###### Newly merged upstream changes

| PR | Merge and effect on this crate |
| --- | --- |
| [#1420](https://github.com/microsoft/mimalloc/pull/1420) | Merged into dev3 on October 3 UTC. Clears cached thread-heap ownership during heap teardown, preventing a destroyed heap address reused by a new heap from matching a stale cache. Also collects cross-thread free lists before zeroing a destroyed page's used count. Both changes apply to the exposed heap API. |
| [#1419](https://github.com/microsoft/mimalloc/pull/1419) | Merged into dev3 on October 3 UTC. Clang targeting MSVC in C mode now uses C11 atomics. This corrects ARM64EC ordering and removes unnecessarily strong ARM64 operations. Our cc build compiles C, so the fix applies when clang-cl is selected. A Windows clang-cl job was added; ARM64/ARM64EC runtime performance is not measured locally. |
| [#1416](https://github.com/microsoft/mimalloc/pull/1416) | Merged into dev, then carried into dev3. The existing `mi_malloc_size` and `mi_malloc_usable_size` implementations are no longer conditional on malloc override. Their Rust declarations already existed, but linking a caller failed under the default feature set before this update. |
| [#1418](https://github.com/microsoft/mimalloc/pull/1418) | Merged into dev, then carried into dev3. Makes internal bitmap.h self-contained. No Rust public API change. |

###### API audit

The public headers `mimalloc.h`, `mimalloc-profile.h`, and `mimalloc-stats.h`
are byte-identical to the previous v3.5.4 pin. This round adds **no new upstream
public functions, option values, or ABI fields**. Existing bindings for the two
usable-size functions become usable in default builds after the upstream fix.

The wrapper's sys API additionally exposes three previously omitted, already
existing C functions: `mi_register_error`, `mi_register_output`, and
`mi_register_deferred_free`. They are unsafe process-wide registrations with
explicit concurrency, lifetime and no-unwind contracts. There is no safe global
callback wrapper, since a Rust-side lock cannot synchronize unrelated C calls.

###### Findings and changes

- **P1, fixed by upstream sync:** stale heap caches and pending cross-thread frees
  during heap destruction. The Rust integration executable now exercises these
  paths and reports whether the destroyed heap address was actually reused.
- **P1, fixed by upstream sync:** declared usable-size FFI functions missing from
  the default library. The new regression reproduced undefined symbols on the
  old source and passes with the new pin. Merely compiling the sys crate did not
  test unused extern declarations, so CI now calls both functions explicitly.
- **P1, still open upstream:** v3.5.4's `profile_disabled` initialization defect
  described in the previous review remains unchanged. Dedicated-heap and worker
  sampling remain unreliable. Profiling tests establish main-heap behavior only.
- **P2, coverage improved:** add clang-cl C11 CI coverage and a producer/consumer
  benchmark with explicit System and MiMalloc allocators. Each iteration frees
  a batch on another thread and reuses its pointer buffer. The result includes
  channel handoff and memory-touch costs; it is not isolated free-call latency.
- **Documentation corrected:** Cargo no longer declares an MSRV; release templates
  and READMEs no longer instruct maintainers to restore the removed field.

###### Performance conclusions and next steps

The sync mainly fixes correctness and platform-specific code generation; it does
not justify changing GlobalAlloc to a local-only free or changing global THP,
NUMA or purge defaults. Keep the existing alignment and cross-thread contracts.

Prioritize Linux producer/consumer and burst/idle tests with throughput, p95/p99,
RSS and page-fault measurements. Compare the two fixed upstream commits under
the same compiler and resource conditions before claiming a release speedup.
For ARM64/ARM64EC, validate actual clang-cl code generation and runtime behavior
on the target. Fix upstream profiler activation before relying on its worker
samples for those performance conclusions.

###### Upstream-sync validation and benchmark evidence

On macOS aarch64 / Rust 1.99.0, formatting, all-target checking, default,
`secure,debug`, all-feature and release tests, Clippy with warnings denied and
rustdoc all passed. The 33 wrapper tests and 2 doc-tests are supplemented by
standalone callback, usable-size and heap-destroy checks. Every tested
configuration observed and validated 32 destroyed-heap address replacements;
platforms that do not reuse an address explicitly report that test as
inconclusive. The old source failed to link the new usable-size regression,
providing negative-baseline evidence. Both crates also passed offline package
build verification, with the wrapper using a temporary local registry patch for
the sys crate.

Short Criterion cross-thread batches (256 allocations per iteration, reused
pointer buffer, one warm-up second, one measurement second, 30 samples):

| Object size | System batch time | MiMalloc batch time |
| --- | --- | --- |
| 64 bytes | 6.7423–6.8578 microseconds | 5.3677–5.5008 microseconds |
| 4096 bytes | 8.2873–8.4458 microseconds | 12.057–13.422 microseconds |

These intervals include allocation, first-byte writes, channel synchronization,
consumer reads and cross-thread frees. The 4 KiB mimalloc run had four outliers
among 30 samples. It motivates repeatable workload analysis rather than a
universal speedup claim. This comparison is against System on this host, not
an old-versus-new mimalloc measurement. No default allocator policy was changed.

###### Previous review — 2026-10-02

####### Baseline and upstream provenance

The workspace was clean and `git pull --ff-only` reported it up to date.
The previous submodule was v3.5.3 (`d4881d338125e1cb7c47ba4cfb398d6f7c0c8d45`).
This update pins [v3.5.4](https://github.com/microsoft/mimalloc/tree/v3.5.4),
commit `f8401befa675adb13b98decababd7fdc59572477`, which is 58 commits ahead.
The annotated tag says **interim**, dated September 30. At review time
[`main3` and the latest full GitHub Release](https://github.com/microsoft/mimalloc/releases/tag/v3.5.3)
still reference v3.5.3. The new pin matches `dev3` at review time.

Merged PRs and direct commits were checked separately:

| Upstream item | Verified status and relevance |
| --- | --- |
| [#1407](https://github.com/microsoft/mimalloc/pull/1407) | Merged into main3 September 17; releases v3.5.3, already in the old source tree. |
| [#1399](https://github.com/microsoft/mimalloc/pull/1399) | Merged into dev3 September 15; fixes process initialization with a TLS recursion guard, already included. |
| [#1401](https://github.com/microsoft/mimalloc/pull/1401) | Merged September 14; fixes over-aligned small frees, already included. |
| [#1384](https://github.com/microsoft/mimalloc/pull/1384) | Merged September 12; CMake multi-configuration handling, not this crate's cc build path. |
| [#1404](https://github.com/microsoft/mimalloc/pull/1404) | Closed without GitHub merge status; its exclusive child-arena fix was incorporated directly as d4881d33. Do not call this a merged PR. |
| v3.5.3 → v3.5.4 | Most new work is direct development commits: pprof, local small frees, allocation/free code generation, adaptive full-page retention, and cross-thread reclamation enabled by default. |

####### Public API delta

Comparing `mimalloc.h` and `mimalloc-profile.h` across the tags found exactly
five added C functions; all five now have Rust FFI declarations:

| API | Exposure and constraints |
| --- | --- |
| `mi_free_small_local`, `mi_free_small_local_nonnull` | Raw FFI and unsafe `MiMalloc` wrappers. A small allocation's page must still belong to the calling thread. The nullable Rust wrapper guards null because upstream asserts non-null in debug mode. |
| `mi_profiler_snapshot` | Raw experimental FFI. Callback state must remain alive, no unwinding across C. |
| `mi_pprof_profiler_new`, `mi_pprof_profiler_delete` | Raw experimental FFI. No safe owner wrapper is offered while sampled allocations/callbacks can outlive a handle. |

Also synchronized: `mi_profiler_t.on_snapshot`, its callback alias, mutable heap
arguments for the three existing profiler callbacks, and option values 48–51
(`profile_alloc_interval`, `profile_inuse_interval`, `profile_time_interval`,
`profile_sample_rate`). The byte-valued options use KiB internally; use
`option_get_size` for bytes. The time interval is in seconds.

Additional previously omitted APIs are exposed for aligned heap operations,
thread-local allocation/reallocation, option defaults/clamping, NUMA affinity
(raw, requires exclusive access), and diagnostic printing. The raw stats
structure is still intentionally not mirrored: v3.5.4 adds a counter to that
layout without changing MI_STAT_VERSION. JSON/text stats avoid that ABI hazard.
This is an audit of the new upstream API delta, not a claim that every legacy,
platform compatibility, deprecated, or C++ helper is wrapped.

####### Review findings and disposition

| Priority | Finding | Disposition |
| --- | --- | --- |
| P1 | Runtime option setters write non-atomic upstream storage while allocator paths read it. Safe setters allowed unsynchronized mutation. | Setters/toggles are now unsafe with startup/exclusive-access requirements; part of the 0.6.0 migration. A wrapper mutex alone cannot protect C allocator reads. |
| P1 | `override` did not define `MI_MALLOC_OVERRIDE`; the existing CI feature jobs could pass without intercepting malloc. | Fixed for non-Windows targets. An integration check allocates through Rust `System` and verifies mimalloc ownership. Windows static CRT behavior is retained. |
| P1 | **Upstream interim profiler limitation:** `_mi_theap_empty.profile_disabled` is true (`src/init.c`); `_mi_theap_init` copies it without clearing it (`src/theap.c`); `mi_malloc_generic_admin` skips activation for such theaps (`src/page.c`). | Dedicated-heap sampling remained at zero even after forcing over 1000 generic allocations. Main-heap activation by `mi_profiler_start` succeeds. Worker-thread/dedicated-heap coverage remains unreliable; documented and not promoted to a safe API. Upstream source stays unmodified. |
| P2 | `no_thp` only disabled explicit huge-page advice, leaving the runtime allow_thp default and purge granularity unchanged. | Also define `MI_DEFAULT_ALLOW_THP=0`; verify the effective default. Linux OS-level THP behavior still needs Linux execution. |
| P2 | Cargo tracked build.rs but not all included C/header files; a submodule-only update could reuse stale C objects. | Track both source trees and derive the exported version from the header. Runtime and build metadata are checked together. |
| P2 | New pprof code requires target-specific stack capture and file-output facilities normally detected by CMake. | Configure GNU Linux/macOS/Windows facilities and BSD libraries. musl has no automatic external unwinder, so stack traces may be empty. |
| P2 | Small-free contracts omitted over-alignment/page ownership constraints; borrowed heap handles omitted the heap's required lifetime. | Strengthen safety contracts, keep GlobalAlloc's general free path, and test aligned zero/reallocation plus immediate local frees. |
| P3 | Diagnostic output copied an already owned UTF-8 buffer; benchmarks claimed System comparison while only using the global mimalloc allocator. | Reuse the buffer and add explicit System plus generic/local small-free benchmark paths. |

####### Performance work to prioritize next

1. Measure v3.5.3 versus this fixed v3.5.4 pin on the same compiler and machine,
   including producer/consumer cross-thread frees, short-lived workers, and
   burst-then-idle workloads. Track throughput, p95/p99 latency, RSS, page faults,
   and reclamation, because cross-thread reclaim and full-page retention changed.
2. Compare `no_thp` and purge policies on Linux under real object-store workloads.
   Change defaults only with throughput, tail-latency and RSS evidence. Immediate
   purging can save memory while increasing OS calls and latency.
3. Consider cached thread-local heap handles for runtimes only after measuring
   TLS lookup costs. A safe handle must borrow its owner, be neither Send nor Sync,
   and prevent heap deletion while the handle is alive.
4. Fix and revalidate upstream profiler activation before using pprof as evidence
   for a multi-threaded performance conclusion. Keep local-free APIs out of the
   general GlobalAlloc path because same-thread allocation does not guarantee
   current page ownership.

Local microbenchmarks are diagnostic only; no application-level performance
improvement percentage is claimed. Linux, Windows, musl, MSRV and architecture
coverage are reported separately from macOS host validation.

####### Validation results

On the macOS aarch64 host with Rust 1.99.0:

- Formatting, workspace/all-target check, all-feature Clippy with warnings denied,
  and documentation build passed.
- Default, `secure,debug`, all-feature and release tests passed: 33 wrapper unit
  tests, 2 doc-tests, plus a standalone integration executable. The executable
  verifies real allocation/free/snapshot callbacks and nonempty pprof samples;
  feature builds also check System interception and the no-THP default.
- Both crates passed offline package build verification. Wrapper verification
  used a temporary local crates.io patch for the unpublished sys version.
- The 64-byte allocation/free microbenchmark (30 samples, 1 second warm-up and
  1 second measurement) measured generic free at 3.7744–3.7986 ns and the local
  wrapper at 3.9151–3.9705 ns. The new local path was slower in this short host
  experiment, so it was not substituted into GlobalAlloc. These intervals are
  Criterion estimates, not a production throughput claim or an old/new release
  comparison.

A separate 64-byte aligned allocation/free comparison measured explicit Rust
`System` at 9.0860–9.3001 ns and `MiMalloc` at 3.7535–3.8591 ns under the same
short sampling settings. This is a single-thread hot-cache microbenchmark, not
a v3.5.3-to-v3.5.4 improvement measurement.

Cross-platform and MSRV execution remains the responsibility of the CI matrix;
local compilation does not verify those targets.
