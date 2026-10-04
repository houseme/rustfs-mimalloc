# rustfs-mimalloc-sys

[![Crates.io](https://img.shields.io/crates/v/rustfs-mimalloc-sys.svg)](https://crates.io/crates/rustfs-mimalloc-sys)
[![Documentation](https://docs.rs/rustfs-mimalloc-sys/badge.svg)](https://docs.rs/rustfs-mimalloc-sys)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](../LICENSE)

Low-level FFI bindings to [mimalloc](https://github.com/microsoft/mimalloc) V3, pinned to commit `8bd60cf0` after v3.5.4 interim.

For a safe, ergonomic wrapper, use [`rustfs-mimalloc`](https://crates.io/crates/rustfs-mimalloc).

## Usage

```toml
[dependencies]
rustfs-mimalloc-sys = "0.6.0"
```

```rust
unsafe {
    let ptr = rustfs_mimalloc_sys::mi_malloc_aligned(64, 8);
    // ... use ptr ...
    rustfs_mimalloc_sys::mi_free(ptr);
}
```

## Features

| Feature | Description |
|---------|-------------|
| `secure` | Guard pages and encoded/randomized free lists (MI_SECURE=4) |
| `debug` | mimalloc debug checks |
| `debug_in_debug` | Auto-enable `debug` in Cargo debug builds |
| `override` | Override system `malloc`/`free` on non-Windows targets |
| `local_dynamic_tls` | Use local-dynamic TLS model |
| `no_thp` | Disable Transparent Huge Pages |
| `no_profile` | Compile out allocation profiling and automatic pprof startup |

## Allocation Profiling Capability

`no_profile` sets `MI_PROFILE=0`. Check `MI_PROFILE_ENABLED` for the sys crate's
compiled capability (including Cargo feature unification). Allocation and free
callbacks produce no profiling samples, and `MIMALLOC_PROFILE` automatic startup
is disabled. Statistics, guarded debug allocations and secure checks are retained.
Raw profiler control/snapshot symbols remain linked: explicit snapshot callbacks
can still execute, but there are no sampled allocations to report.

At the pinned upstream revision, disabling the built-in profiler implementation
has a C signature mismatch. The build keeps its compatible implementation while
turning off sampling and automatic startup. Vendored source remains unmodified.

## Windows Static CRT

On `*-pc-windows-msvc`, configure the final Rust binary with
`-C target-feature=+crt-static`. The build script lets `cc` select `/MT` for
mimalloc from Cargo's `crt-static` target feature, so the C allocator and Rust
share one CRT mode. Without that target feature, the default remains `/MD`.

## What's Included

This crate vendors the mimalloc V3 C source and compiles it via the `cc` crate. No system mimalloc installation required.

- Static mimalloc linking; platform system libraries are linked as needed
- Platform-specific flags handled automatically (Windows libs, musl compat, TLS model)
- `links = "mimalloc"` — exports `DEP_MIMALLOC_INCLUDE` for downstream C/C++ crates
- V3.5.4 small allocation/free APIs, including `mi_wmalloc_small`,
  `mi_wzalloc_small`, the thread-local heap word-size variants,
  `mi_free_small_nonnull`, and inline Rust mirrors for `mi_malloc_csize`,
  `mi_zalloc_csize`, `mi_theap_malloc_csize`, `mi_theap_zalloc_csize`,
  `mi_free_csize`, `mi_free_csize_nonnull`, `mi_free_csize_aligned`, and
  `mi_free_csize_aligned_nonnull`
- Raw `mi_theap_alloc_new*` C++-semantics FFI; callers must not allow a foreign
  exception from an installed C++ new-handler to unwind into Rust
- Raw experimental profiling FFI from `mimalloc-profile.h`, including
  `mi_profiler_snapshot` and `mi_pprof_profiler_{new,delete}`. V3.5.4 adds
  `on_snapshot` to `mi_profiler_t` and changes callback heap pointers to mutable
- `mi_free_small_local{,_nonnull}` for small pages still owned by the calling thread
- Raw error, output and deferred-free callback registration APIs
- All four `mi_option_profile_*` controls, default/clamped option APIs, aligned
  heap reallocation, and thread-local aligned allocation/reallocation APIs

The source pin follows the **interim** tag and includes fixes from upstream
PRs #1416, #1418, #1419 and #1420. No public C API was added by those commits.
The inherited `profile_disabled` flag
can prevent sampling in new thread heaps; dedicated-heap and worker-thread
profiling are not reliable at this pin. Profiling callbacks must not unwind;
stop and detach a profiler, free sampled allocations, and quiesce callbacks
before deleting it. `mi_profiler_start`/`stop` return the previous running state.
On musl, backtrace capture needs an externally configured unwinder. GNU Linux
and macOS use `execinfo`, Windows uses `CaptureStackBackTrace`; FreeBSD/DragonFly
also link `execinfo` and `util`.

## Rust Toolchain Compatibility

Cargo manifests intentionally omit `rust-version`. CI checks stable Rust and
Rust 1.96.0 compatibility; no minimum Rust version is declared by these crates.

## License

Apache-2.0. The vendored mimalloc C library is MIT-licensed.
