# Upstream and wrapper review — 2026-10-14

## Result and provenance

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

## Upstream changes and merged PR audit

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

## Public API audit and implementation

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

## Review findings and optimization priorities

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

## Performance evidence

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

## Validation

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

## Historical reviews

The following sections retain prior evidence at their original pins and dates.
Their measurements and validation counts are historical, not current results.

## Upstream and wrapper review — 2026-10-04

### Current result

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

### Profiling-free build follow-up

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

### Performance implementation follow-up

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

### Newly merged upstream changes

| PR | Merge and effect on this crate |
| --- | --- |
| [#1420](https://github.com/microsoft/mimalloc/pull/1420) | Merged into dev3 on October 3 UTC. Clears cached thread-heap ownership during heap teardown, preventing a destroyed heap address reused by a new heap from matching a stale cache. Also collects cross-thread free lists before zeroing a destroyed page's used count. Both changes apply to the exposed heap API. |
| [#1419](https://github.com/microsoft/mimalloc/pull/1419) | Merged into dev3 on October 3 UTC. Clang targeting MSVC in C mode now uses C11 atomics. This corrects ARM64EC ordering and removes unnecessarily strong ARM64 operations. Our cc build compiles C, so the fix applies when clang-cl is selected. A Windows clang-cl job was added; ARM64/ARM64EC runtime performance is not measured locally. |
| [#1416](https://github.com/microsoft/mimalloc/pull/1416) | Merged into dev, then carried into dev3. The existing `mi_malloc_size` and `mi_malloc_usable_size` implementations are no longer conditional on malloc override. Their Rust declarations already existed, but linking a caller failed under the default feature set before this update. |
| [#1418](https://github.com/microsoft/mimalloc/pull/1418) | Merged into dev, then carried into dev3. Makes internal bitmap.h self-contained. No Rust public API change. |

### API audit

The public headers `mimalloc.h`, `mimalloc-profile.h`, and `mimalloc-stats.h`
are byte-identical to the previous v3.5.4 pin. This round adds **no new upstream
public functions, option values, or ABI fields**. Existing bindings for the two
usable-size functions become usable in default builds after the upstream fix.

The wrapper's sys API additionally exposes three previously omitted, already
existing C functions: `mi_register_error`, `mi_register_output`, and
`mi_register_deferred_free`. They are unsafe process-wide registrations with
explicit concurrency, lifetime and no-unwind contracts. There is no safe global
callback wrapper, since a Rust-side lock cannot synchronize unrelated C calls.

### Findings and changes

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

### Performance conclusions and next steps

The sync mainly fixes correctness and platform-specific code generation; it does
not justify changing GlobalAlloc to a local-only free or changing global THP,
NUMA or purge defaults. Keep the existing alignment and cross-thread contracts.

Prioritize Linux producer/consumer and burst/idle tests with throughput, p95/p99,
RSS and page-fault measurements. Compare the two fixed upstream commits under
the same compiler and resource conditions before claiming a release speedup.
For ARM64/ARM64EC, validate actual clang-cl code generation and runtime behavior
on the target. Fix upstream profiler activation before relying on its worker
samples for those performance conclusions.

### Upstream-sync validation and benchmark evidence

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

### Previous review — 2026-10-02

#### Baseline and upstream provenance

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

#### Public API delta

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

#### Review findings and disposition

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

#### Performance work to prioritize next

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

#### Validation results

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
