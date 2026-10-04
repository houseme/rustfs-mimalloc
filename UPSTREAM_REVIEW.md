# Upstream and wrapper review — 2026-10-04

## Current result

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

## Newly merged upstream changes

| PR | Merge and effect on this crate |
| --- | --- |
| [#1420](https://github.com/microsoft/mimalloc/pull/1420) | Merged into dev3 on October 3 UTC. Clears cached thread-heap ownership during heap teardown, preventing a destroyed heap address reused by a new heap from matching a stale cache. Also collects cross-thread free lists before zeroing a destroyed page's used count. Both changes apply to the exposed heap API. |
| [#1419](https://github.com/microsoft/mimalloc/pull/1419) | Merged into dev3 on October 3 UTC. Clang targeting MSVC in C mode now uses C11 atomics. This corrects ARM64EC ordering and removes unnecessarily strong ARM64 operations. Our cc build compiles C, so the fix applies when clang-cl is selected. A Windows clang-cl job was added; ARM64/ARM64EC runtime performance is not measured locally. |
| [#1416](https://github.com/microsoft/mimalloc/pull/1416) | Merged into dev, then carried into dev3. The existing `mi_malloc_size` and `mi_malloc_usable_size` implementations are no longer conditional on malloc override. Their Rust declarations already existed, but linking a caller failed under the default feature set before this update. |
| [#1418](https://github.com/microsoft/mimalloc/pull/1418) | Merged into dev, then carried into dev3. Makes internal bitmap.h self-contained. No Rust public API change. |

## API audit

The public headers `mimalloc.h`, `mimalloc-profile.h`, and `mimalloc-stats.h`
are byte-identical to the previous v3.5.4 pin. This round adds **no new upstream
public functions, option values, or ABI fields**. Existing bindings for the two
usable-size functions become usable in default builds after the upstream fix.

The wrapper's sys API additionally exposes three previously omitted, already
existing C functions: `mi_register_error`, `mi_register_output`, and
`mi_register_deferred_free`. They are unsafe process-wide registrations with
explicit concurrency, lifetime and no-unwind contracts. There is no safe global
callback wrapper, since a Rust-side lock cannot synchronize unrelated C calls.

## Findings and changes

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

## Performance conclusions and next steps

The sync mainly fixes correctness and platform-specific code generation; it does
not justify changing GlobalAlloc to a local-only free or changing global THP,
NUMA or purge defaults. Keep the existing alignment and cross-thread contracts.

Prioritize Linux producer/consumer and burst/idle tests with throughput, p95/p99,
RSS and page-fault measurements. Compare the two fixed upstream commits under
the same compiler and resource conditions before claiming a release speedup.
For ARM64/ARM64EC, validate actual clang-cl code generation and runtime behavior
on the target. Fix upstream profiler activation before relying on its worker
samples for those performance conclusions.

## Current validation and benchmark evidence

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

## Previous review — 2026-10-02

### Baseline and upstream provenance

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

### Public API delta

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

### Review findings and disposition

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

### Performance work to prioritize next

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

### Validation results

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
