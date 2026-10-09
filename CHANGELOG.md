# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Pin mimalloc to upstream `dev3` commit `a28efddd` (2026-10-08), after the
  **v3.5.4 interim** tag. The C source version remains 30504. This includes
  allocation/free improvements, adaptive full-page retention, cross-thread page
  reclamation, and subsequent upstream fixes. Workload-specific throughput and
  RSS still require measurement.
- **Breaking:** match the experimental profiler ABI: add `on_snapshot` and use
  mutable heap pointers in allocation/free callbacks.
- **Breaking:** make runtime option setters/toggles unsafe because upstream
  option storage is non-atomic. Configure before starting threads or exclude
  all concurrent mimalloc accesses.
- Remove `rust-version` from the workspace and member manifests as requested;
  retain Rust 1.96.0 as an explicit CI compatibility check, not a Cargo MSRV.

### Added

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

### Known upstream limitation

- V3.5.4 interim copies `profile_disabled=true` into new thread heaps without
  clearing it. Dedicated-heap and worker-thread profiling can remain inactive.
  Profiling stays experimental raw FFI; the integration test proves main-heap
  sampling in an isolated single-threaded process only.

### Fixed

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
- Enable `MI_MALLOC_OVERRIDE` on non-Windows override builds.
- Set `MI_DEFAULT_ALLOW_THP=0` with `no_thp`; disabling only explicit huge-page
  advice did not disable the runtime THP policy or its larger purge granularity.
- Track vendored C sources/headers for Cargo rebuilds and derive build metadata
  from `MI_MALLOC_VERSION`, preventing stale C objects after submodule updates.
- Configure pprof backtrace and file-output facilities for supported targets.
- Correct TLS/security documentation, remove the dead Windows TLS feature hook,
  strengthen small-free and borrowed-heap safety contracts, and reuse the text
  diagnostic buffer instead of copying valid UTF-8 output.

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
