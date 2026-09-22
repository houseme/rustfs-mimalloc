# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.5...HEAD
[0.5.5]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/houseme/rustfs-mimalloc/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/houseme/rustfs-mimalloc/compare/v0.3.0...v0.5.0
[0.3.0]: https://github.com/houseme/rustfs-mimalloc/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/houseme/rustfs-mimalloc/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/houseme/rustfs-mimalloc/releases/tag/v0.1.0
