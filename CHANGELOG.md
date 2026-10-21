# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This is the shared changelog for `rustfs-mimalloc-sys`, `rustfs-mimalloc`,
and `rustfs-mimalloc-profiling`, which share one release version.

## [0.7.1] - 2026-10-21

### Changed

- Keep all three crate versions and internal requirements synchronized at 0.7.1;
  consolidate pending notes into this section. Build dependencies use `cc` 1.9.0
  and `find-msvc-tools` 0.1.15; the test-only `crc32fast` lock entry is 1.5.3.
- Advance mimalloc to V3.6.0 development commit `b5d94fe8`, 54 commits after
  `6f45cc52`. The numeric version remains 30600; the latest published upstream
  release remains v3.5.3. This pass includes nine merged PRs, detailed in
  `UPSTREAM_REVIEW.md`.
- Restore a single weak acquire-release CAS in remote free, replacing the
  earlier owned-page release-only branch. Inherit the private arm64 page-layout
  change, hashed small-page offsets and statistics-merge fixes.
- Correct native environment `K`/`KiB` scaling. Native profiling defaults are
  now actually 512 KiB per sample and 1 GiB per allocation snapshot interval;
  these are runtime-value fixes, not just comment corrections. Option IDs stay
  unchanged; hexadecimal integers work and zero-padded decimals remain invalid.
- Read Linux PMD size from sysfs. Mappings at least as large as detected pages
  above 8 MiB receive `MADV_NOHUGEPAGE`, independent of positive-advice alignment
  eligibility. Shorter mappings do not receive that negative advice.
- QSBR full pages follow `mi_option_page_full_retain`; delayed empty abandoned
  pages remain discoverable and can join a receiving same-heap context. Fresh
  allocation retries when reclaim releases an empty page. Reclaim-on-free policy
  is captured at heap creation; late TLS frees require runtime exclusion.

### Added

- Raw experimental `mi_heap_new_qsbr` FFI. Ordinary builds return null/ENOSYS;
  enabled builds need host C metadata and reader/grace-period hooks. There is no
  Cargo QSBR feature or owning safe Rust heap for this mode.
- `mi_option_unsafe_guarded_precise` at ID 31 while retaining the old Rust enum
  variant and glob imports. API/ABI gates cover 131 Rust function names, 45 option
  IDs, 122 extern signatures, nine callbacks and three public C layouts.
- Independent-process option coverage grows to 28 cases, including KiB suffixes
  and real native profiling defaults. Native callback tests reach dedicated heaps
  on the caller and a worker after lazy activation and exclude a nested callback
  allocation from sampling, including `no_profile` checks.
- Native C profiling/pprof tests join the focused CI suite. Linux also runs pprof
  with malloc override, including deterministic snapshot OOM and recovery tests.
- Document upstream `MI_NO_CXX_NEW_HANDLER` and `MI_NO_ALIGN_FALLBACK_WARNING`
  compile-time controls through target CFLAGS; default behavior is preserved.

### Fixed

- Inherit native pprof allocation-failure handling: abort an OOM snapshot before
  writing its output and release failed profiler/location allocations. Guard
  recursive allocation sampling during `on_alloc`, preserve sampling state on
  no-sample paths and avoid an oversized shift in exponential sampling.
- Native dedicated heaps/worker contexts are no longer permanently excluded from
  sampling. Activation is still lazy; the Rust sampling wrapper is independent.
- Remove the local Windows ImageBase patch now fixed by upstream PR #1443;
  dynamic CRT builds use upstream source. Retain static CRT bootstrap fixes and
  Linux/Android `no_thp` conditional compatibility, guarded against source drift.
- Inherit QSBR abandoned-page mapping, guarded sized-free, MSan syscall annotation,
  Emscripten builtin allocator, WASI preprocessing and relative-header fixes.
- Size the Rust profiler's temporary live-copy vector from retained occupancy.
  Stable empty snapshots reserve no vector storage; concurrent growth reserves
  outside the shard lock before copying. Retain realloc reservations and all
  sampled records without changing preallocated metadata or sampling defaults.
- Cover concurrent remote-free/content/reuse across 64 B, 4 KiB and 64 KiB blocks,
  and bounded native parser/PMD/advice controls. Clarify alignment obligations for
  unsafe option setters and caller-context scope for collection APIs.

## [0.7.0] - 2026-10-17

### Changed

- **Breaking (`rustfs-mimalloc-profiling`):** the default profiling core no
  longer selects an allocator. Install `SamplingAllocator<A>` around the
  application's `GlobalAlloc` provider. Both `mimalloc::MiMalloc` and
  `rustfs_mimalloc::MiMalloc` use the same recorder and pprof implementation.
- Keep the existing `SamplingMiMalloc::new()` and `Default` convenience API
  behind the explicit `rustfs-allocator` feature. Library adapters can depend
  on the core with `default-features = false`.
- Keep all three crate versions synchronized at 0.7.0 and refresh the locked
  libc resolution to 0.2.191. The pinned native allocator and ABI are unchanged.

- Restrict direct object parsing to uncompressed Linux ELF metadata, disabling
  unused format/compression features and removing the ruzstd decoder dependency.
  Backtrace symbolization keeps its existing features.

### Added

- Stateful-provider forwarding contracts for alignment, zeroing, realloc
  failure/in-place/move outcomes and cross-thread free, with deterministic
  live-record rollback checks independent of provider size classes.
- A shared behavior/release test suite run from independent Cargo workspaces
  for both providers. Provider requirements come from root workspace dependencies;
  resolved dependency graphs reject accidental inclusion of a second allocator.
- Default-core, optional compatibility API and dual-provider validation of the
  normalized published archive, plus stable/Rust 1.95 and platform CI checks.

## [0.6.1] - 2026-10-16

### Changed

- Manage all dependency version requirements in the root
  `workspace.dependencies`; member crates inherit them while retaining their
  feature selections and target-platform conditions.
- Raise the profiling test dependency `libflate` requirement from `2.1` to
  `2.3` and refresh the workspace lockfile, including `prost-derive`'s
  `itertools 0.14` resolution.
- Consolidate profiling release notes into this workspace changelog and link
  each crate README to the same source.

### Added

- Add a host C compiler ABI projection check for all 121 extern functions,
  nine callbacks and three exposed `repr(C)` layouts, complementing the
  function-name/option-ID inventory and runtime linkage tests. Negative
  controls reject return-type, callback-constness, layout-order and size-alias
  drift. CI runs the checks without adding allocator runtime overhead.
- Refresh the three-crate functional and performance review. The latest
  mimalloc `dev3` remains `6f45cc52`; no new upstream API requires another
  source or binding update in this pass.

### Fixed

- Validate release metadata and changelog on stable, pin every release job to
  the same source commit, and require locked dependencies. Validate the upstream
  Pyroscope CI baseline on Rust 1.95.0; run documentation and strict lint on stable.
- Preserve package archives before Cargo publication and verify official
  checksums when resuming an already uploaded version. Manual publication
  checks out the existing release tag for every validation and upload step.

## [0.6.0] - 2026-10-16

### Changed

- Pin mimalloc to upstream `dev3` commit `6f45cc52` (2026-10-16 review), after the
  **v3.5.4 interim** tag. The C source version remains 30504. This includes
  allocation/free improvements, adaptive full-page retention, cross-thread page
  reclamation, the arena/bitmap source reorganization, and bitmap rollback
  fixes through the 2026-10-16 review. Includes naturally aligned allocation
  size-class improvements, cache offsets, OS-page heap lifecycle/visiting fixes,
  and quiescent fork support. The large-range immediate-purge default is 32 MiB.
  Workload-specific throughput and RSS still require measurement.
- **Breaking:** match the experimental profiler ABI: add `on_snapshot` and use
  mutable heap pointers in allocation/free callbacks.
- **Breaking:** make runtime option setters/toggles unsafe because upstream
  option storage is non-atomic. Configure before starting threads or exclude
  all concurrent mimalloc accesses.
- Remove `rust-version` from the workspace and member manifests as requested;
  retain Rust 1.96.0 as an explicit CI compatibility check, not a Cargo MSRV.

### Added

- Add the independent `rustfs-mimalloc-profiling` companion crate, extracted
  from grafana/pyroscope-rs#572. It owns the sampling allocator, bounded event
  recorder, opt-in live allocation tracking, symbol/mapping resolution and
  pprof encoder. `Profiler` provides an exclusive recording session with explicit
  stop and Drop cleanup, returning owned profile bytes without an uploader or
  dependency on Pyroscope. The wrapper uses the matching allocator's
  `GlobalAlloc` interface; direct native allocations are outside its coverage
  and bounded recorder pressure can omit samples.
- Move allocator behavior, optimized profile quality, stress and benchmark
  validation alongside the profiling implementation. Keep the base allocator
  dependency graph free of profiling dependencies.

- Bound all four new upstream public APIs: `mi_thread_id_of`,
  `mi_heap_set_debug_fill_preserve`, `mi_theap_delete`, and
  `mi_theap_quiescent_abandon`. Added `MiMalloc::thread_id_of`,
  `Heap::set_debug_fill_preserve`, and unsafe `ThreadHeap::quiescent_abandon`.
  Context deletion remains raw FFI because it invalidates all cached aliases.
- Added a runnable custom allocation-tracking hook example for upstream #1396,
  enabled with `CFLAGS=-DMI_TRACK_CUSTOM`, with no default hot-path overhead.
- Added owner-query, quiescent foreign-context cleanup, context deletion,
  zero-initialization and direct-OS-page lifecycle regression checks.
- Added `MiMalloc::{expand,rezalloc,rezalloc_aligned}`, heap zero-allocation
  and reallocation helpers, and cached `ThreadHeap::{calloc,malloc_csize,
  zalloc_csize,realloc,rezalloc}`. Added 13 previously unbound public C
  allocation APIs; zero-growth, alignment and overflow behavior are tested.
- Added a public-header API inventory and option-ID parity check to CI,
  covering 130 Rust function names and 45 option IDs at the current pin.
- Opt-in `no_profile` builds to remove allocation profiling sampling and automatic
  `MIMALLOC_PROFILE` startup. Statistics, alignment, security/debug checks and raw
  profiler control ABI remain available; explicit snapshots have no allocation samples.
- `MiMalloc::profiling_enabled()` and `MI_PROFILE_ENABLED` capability queries that
  respect the linked sys crate's unified Cargo features.
- Default/no-profile ABBA driver and tests for disabled sampling, environment
  startup, feature unification, and retained secure/debug/statistics behavior.
- `Heap::thread_local()` and borrowed `ThreadHeap` allocation methods to avoid
  repeated TLS lookups when a worker alternates between heaps. The view cannot
  outlive its parent or be sent/shared across threads; allocations may still be
  freed on other threads. Global allocator and retention defaults are unchanged.
- Single-heap and alternating-heap benchmarks plus an A/B/B/A driver that
  checks baseline drift and candidate repeatability before reporting a speedup.
- Alignment, zeroing, cross-thread free, parent-deletion and compile-fail tests
  for cached heap views.
- Raw callback registration: `mi_register_error`, `mi_register_output`, and
  `mi_register_deferred_free`, with lifetime/concurrency contracts.
- Cross-thread free batch benchmarks and a Windows clang-cl C11 test job.
- Raw `mi_profiler_snapshot`, `mi_pprof_profiler_new`, and
  `mi_pprof_profiler_delete`; all four profiling interval/sample-rate options.
- Raw `mi_arenas_purge` and safe `MiMalloc::arenas_purge()` to process pending
  arena purges in the main subprocess. Expose `mi_option_stats_merge_threshold`
  and `mi_option_arena_purge_immediate_size`, plus renamed purge/stat-merge
  options and compatibility aliases for the prior Rust option names.
- Unsafe `MiMalloc::free_small_local{,_nonnull}` and matching raw FFI. The
  allocation's small page must still be owned by the calling thread.
- Aligned heap zero-allocation and reallocation, additional thread-local FFI,
  capacity hints, option diagnostics, and default/clamped option access.
- Integration tests for profiler callback ABI, sampled pprof output, effective
  malloc override and THP defaults; benchmark comparisons against `System` and
  the generic/local small-free paths.

### Fixed

- Require reporter-visible TLS registration before accepting allocation events;
  count registration contention and unavailable TLS as drops. Remove the
  allocation-side flush notification protocol and its hidden pending buffers.
- Correct allocation object counts by sharing the inclusion-probability weights
  with live heap tracking and rounding after stack aggregation. The former
  per-event integer division underestimated object counts for allocation sizes
  near the sampling interval.
- Replace interval-crossing loops and their deterministic fallback with one
  memoryless sampling step. Keep the unsampled path to a TLS countdown and
  use a FIFO TLS ring so partial drains do not move retained stacks.

- Inherit upstream Windows console detachment, ProcessPrng/legacy RNG loading,
  naturally aligned allocation, direct-OS-page cleanup, and inline-linkage fixes.
  Upstream #1428 adds array-reallocation contract tests.

- Updated the Windows static-CRT compatibility source and drift tests for
  upstream moving `src/theap.c` to `src/heap/theap.c`.
- Clarified raw constant-size free alignment and arena memory ownership
  requirements, and heap reallocation failure/alignment contracts.
- Declare Windows `__ImageBase` with explicit `extern` in C builds. Upstream's
  C++ linkage macro expands to nothing in C and accidentally defined BSS storage,
  shadowing the linker-owned PE base. Static UCRT used that false base for RVA
  lookups and crashed in `__acrt_tls2_supported` before `main`. Verify the symbol
  matches the OS module handle and the PE header in the isolated startup test.
- Inherit upstream #1420: invalidate cached thread heaps on heap destruction
  and collect pending cross-thread frees before resetting a destroyed page.
- Inherit upstream #1416: make the already-declared `mi_malloc_size` and
  `mi_malloc_usable_size` available even without malloc override; test their
  actual linkage and values.
- Inherit upstream #1419: use C11 atomics for clang C builds targeting MSVC,
  fixing ARM64EC ordering and avoiding unnecessary ARM64 barriers.
- Inherit upstream fixes for `mi_free_size` size-mismatch checks, Windows
  forced-exit handling and UWP compilation, Android syscall avoidance, and
  profiling-disabled builds. Keep the built-in pprof implementation enabled in
  `no_profile` so explicit raw snapshots remain functional.
- Defer full Windows static CRT initialization and strong RNG reseeding from
  the pre-CRT TLS callback to `.CRT$XCT`, after UCRT initialization. Keep
  weak seeds for early allocation bootstrap and TLS thread-exit cleanup.
- Use one resolved CRT setting for MSVC compiler flags and startup compatibility;
  distinguish `+crt-static` from `-crt-static` and honor encoded Rust flags.
- Check static CRT startup ordering before `main` in an isolated regression
  executable, including secure/debug/no-profile and release CI coverage.
- Enable `MI_MALLOC_OVERRIDE` on non-Windows override builds.
- Set `MI_DEFAULT_ALLOW_THP=0` with `no_thp`; disabling only explicit huge-page
  advice did not disable the runtime THP policy or its larger purge granularity.
- Track vendored C sources/headers for Cargo rebuilds and derive build metadata
  from `MI_MALLOC_VERSION`, preventing stale C objects after submodule updates.
- Configure pprof backtrace and file-output facilities for supported targets.
- Correct TLS/security documentation, remove the dead Windows TLS feature hook,
  strengthen small-free and borrowed-heap safety contracts, and reuse the text
  diagnostic buffer instead of copying valid UTF-8 output.

### Known upstream limitation

- V3.5.4 interim copies `profile_disabled=true` into new thread heaps without
  clearing it. Dedicated-heap and worker-thread profiling can remain inactive.
  Profiling stays experimental raw FFI; the integration test proves main-heap
  sampling in an isolated single-threaded process only.

## [0.5.6] - 2026-09-22

### Fixed

- Honor Cargo's `crt-static` target feature for MSVC builds instead of forcing
  `/MD`, allowing a Windows binary and the vendored mimalloc library to use the
  same static CRT (`/MT`) without a runtime DLL dependency.

### Added

- Added a Windows static-CRT CI job using `-C target-feature=+crt-static`.

## [0.5.5] - 2026-09-22

### Changed

- Updated the mimalloc submodule to upstream v3.5.3 (commit `d4881d3`).
- Updated the exposed build metadata version to `30503`.

### Fixed

- Inherited mimalloc's fix for exclusive arena allocations that cross child
  arenas, including managed regions larger than 16 GiB.
- Inherited fixes for over-aligned small allocation frees and local-dynamic TLS
  process initialization.

### Added

- Exposed V3.5.3's aligned constant-size free helpers as unsafe
  `mi_free_csize_aligned{,_nonnull}` FFI mirrors and `MiMalloc` methods.
- Exposed raw `mi_theap_alloc_new`, `mi_theap_alloc_new_n`, and
  `mi_theap_alloc_new_nothrow` FFI. These retain their upstream C++ OOM-handler
  semantics and intentionally have no safe Rust wrapper.

## [0.5.4] - 2026-09-16

### Changed

- Updated the mimalloc submodule to upstream v3.5.2 (commit `636510a`).
- Updated the exposed build metadata version to `30502`.

### Added

- Exposed mimalloc V3.5.2's word-size small allocation APIs:
  `mi_wmalloc_small`, `mi_wzalloc_small`, `mi_wsize_from_size`,
  `mi_malloc_csize`, `mi_zalloc_csize`, `mi_theap_wmalloc_small`,
  `mi_theap_wzalloc_small`, `mi_theap_malloc_csize`, and
  `mi_theap_zalloc_csize`.
- Added unsafe `MiMalloc` wrappers for constant-size and word-size small
  allocation fast paths.
- Exposed `mi_option_collect_merges_stats` and
  `mi_theap_stats_merge_to_heap` through `rustfs-mimalloc-sys`.
- Exposed mimalloc's experimental profiling hooks in `rustfs-mimalloc-sys`
  as raw FFI only.

## [0.5.3] - 2026-09-04

### Fixed

- Avoid passing MSVC-only compiler flags to Windows GNU/LLVM targets such as
  `x86_64-pc-windows-gnullvm`.

### Changed

- Centralized build-script target detection around Cargo's structured
  `CARGO_CFG_TARGET_*` values to keep OS, ABI, architecture, and vendor checks
  distinct.

## [0.5.2] - 2026-09-03

### Changed

- Updated the mimalloc submodule to upstream v3.5.1 (commit `34fbd7e`).
- Updated the exposed build metadata version to `30501`.

### Added

- Exposed mimalloc V3.5.1's `mi_free_small_nonnull` binding.
- Added Rust inline mirrors for `mi_free_csize` and `mi_free_csize_nonnull`.
- Added unsafe `MiMalloc` wrappers for small and constant-size free fast paths.

## [0.5.1] - 2026-08-26

### Added

- Exposed mimalloc V3's `mi_thread_set_in_threadpool()` through `rustfs-mimalloc-sys` and added a safe `rustfs_mimalloc::set_current_thread_in_threadpool()` wrapper.

### Fixed

- Ensure GitHub workflows check out the mimalloc submodule recursively so CI builds can find `c_src/mimalloc/src/static.c`.

## [0.5.0] - 2026-08-22

### Changed

- Switched mimalloc source from embedded code to git submodule (`microsoft/mimalloc`).
- Updated mimalloc submodule to v3.5.0 (commit `cd69707`).
- Simplified submodule update process: `git fetch` + `git checkout` in submodule directory.
- Pinned `codeql-action` to major version `v4` in CI workflow.

## [0.3.0] - 2026-08-22

### Added

- Added `homepage` and `documentation` fields to all Cargo.toml packages.
- Added `README.md` to `rustfs-mimalloc-sys` and `rustfs-mimalloc` sub-crates.
- Added `readme` field to sub-crate Cargo.toml files for crates.io display.

### Changed

- Updated workspace version to 0.3.0.
- Updated all documentation to reflect current MSRV (1.96.0).

### Removed

- Removed dead `win_direct_tls` feature (unused in build.rs and source code).

## [0.2.0] - 2026-08-22

### Added

- Added `README.md` to `rustfs-mimalloc-sys` and `rustfs-mimalloc` sub-crates.
- Added `readme` field to both sub-crate Cargo.toml files.

### Changed

- Raised MSRV from 1.85.0 to 1.96.0, matching the rolling support window for the latest three stable Rust release trains.
- Reduced the feature surface to behavior-changing options only; removed the no-op `extended` feature and fine-grained `secure_level_1..5` aliases.
- Removed the unstable `nightly_allocator_api` feature so the crate remains fully verifiable on stable Rust.
- Kept `secure` as the single heap-encryption feature and mapped it directly to mimalloc's upstream default `MI_SECURE=4`.
- Consolidated profile output collection into a shared internal FFI helper with preallocated callback storage.
- Changed low-level FFI aliases to use `core::ffi` platform C types.
- Distinguished owned heap handles from borrowed heap handles so `Heap::main()` and `Heap::heap_of()` do not delete heaps they do not own.
- Updated CI and release workflows to test only stable feature combinations that exist.

### Removed

- Removed dead `win_direct_tls` feature (unused in build.rs and source code).
- Removed `extended` feature; all stats/options/heap APIs are always available.
- Removed `nightly_allocator_api` feature.
- Removed `secure_level_1..5` fine-grained features.

## [0.1.0] - 2026-08-22

### Added

- Initial release.
- `rustfs-mimalloc-sys`: low-level FFI crate that builds and links mimalloc V3 (v3.5.0).
- `rustfs-mimalloc`: safe global allocator wrapper implementing `GlobalAlloc`.
- Heap and arena management helpers for advanced allocation control.
- Process memory information APIs: `MiMalloc::process_info()`, `MiMalloc::stats_json()`, `MiMalloc::stats_print()`, `MiMalloc::process_info_print()`, `MiMalloc::stats_reset()`.
- Runtime option APIs: `MiMalloc::option_set()`, `option_get()`, `option_enable()`, `option_disable()`.
- Features: `secure`, `debug`, `debug_in_debug`, `override`, `local_dynamic_tls`, `no_thp`.
- CI: 3-platform test matrix (Linux/macOS/Windows), musl cross-compile, MSRV check, lint, docs.
- Release workflow: tag-triggered + manual dispatch, crates.io publish, GitHub Release.
- 22 unit tests + 2 doc-tests + allocation benchmarks.

[0.6.0]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.6...v0.6.0
[0.5.6]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.5...v0.5.6
[0.5.5]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/houseme/rustfs-mimalloc/compare/v0.3.0...v0.5.0
[0.3.0]: https://github.com/houseme/rustfs-mimalloc/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/houseme/rustfs-mimalloc/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/houseme/rustfs-mimalloc/releases/tag/v0.1.0
