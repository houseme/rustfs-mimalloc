# rustfs-mimalloc

[![Crates.io](https://img.shields.io/crates/v/rustfs-mimalloc.svg)](https://crates.io/crates/rustfs-mimalloc)
[![Documentation](https://docs.rs/rustfs-mimalloc/badge.svg)](https://docs.rs/rustfs-mimalloc)
[![CI](https://github.com/houseme/rustfs-mimalloc/actions/workflows/ci.yml/badge.svg)](https://github.com/houseme/rustfs-mimalloc/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](#rust-toolchain-compatibility)

High-performance [mimalloc](https://github.com/microsoft/mimalloc) V3 global allocator for Rust.

## Overview

`rustfs-mimalloc` provides safe, ergonomic Rust bindings to Microsoft's mimalloc V3 memory allocator (post-v3.5.4 interim, commit `a28efddd`). Drop-in replacement for the system allocator with excellent multi-threaded performance.

### Why this crate?

- **V3 only** — exclusively targets mimalloc V3, no multi-version complexity
- **Always aligned** — uses `mi_malloc_aligned` for all allocations, preventing [alignment bugs](https://github.com/purpleprotocol/mimalloc_rust/issues/87)
- **Zero indirection** — `#[inline(always)]` hot path, no intermediate function calls
- **Cross-platform** — Linux, macOS, Windows, ARM, RISC-V, musl
- **Comprehensive API** — stats, options, heap/arena management
- **Issue-informed** — addresses [40+ known issues](https://github.com/purpleprotocol/mimalloc_rust/issues) from the reference implementation

## Quick Start

```toml
[dependencies]
rustfs-mimalloc = "0.6.0"
```

```rust
use rustfs_mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    let v = vec![1, 2, 3, 4, 5];
    println!("{:?}", v);
}
```

## Features

| Feature | Default | Description |
|---------|:-------:|-------------|
| `secure` | | Guard pages and encoded/randomized free lists (MI_SECURE=4) |
| `debug` | | mimalloc debug checks |
| `debug_in_debug` | | Auto-enable `debug` in Cargo debug builds |
| `override` | | Override system `malloc`/`free` on non-Windows targets |
| `local_dynamic_tls` | | Use local-dynamic TLS model (fixes polars compatibility) |
| `no_thp` | | Disable Transparent Huge Pages on Linux/Android |
| `no_profile` | | Compile out allocation profiling and automatic pprof startup |

All stats, options, heap, and arena APIs are available without any feature flag.

## API

### Global Allocator

```rust
use rustfs_mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
```

Implements `GlobalAlloc` with aligned allocation/reallocation and general
`mi_free` deallocation.

### Statistics & Diagnostics

```rust
use rustfs_mimalloc::MiMalloc;
use rustfs_mimalloc_sys::mi_option_t;

// Version: 30504 = V3.5.4
let version = MiMalloc::version();

// Stats as JSON
let json = MiMalloc::stats_json();

// Stats in human-readable text
let text = MiMalloc::stats_print();

// Process memory info (struct)
let info = MiMalloc::process_info();
println!("Peak RSS: {} bytes", info.peak_rss);

// Process memory info (text)
let text = MiMalloc::process_info_print();

// Reset stats
MiMalloc::stats_reset();
```

### Runtime Options

```rust
use rustfs_mimalloc::MiMalloc;
use rustfs_mimalloc_sys::mi_option_t;

// Return memory to OS immediately (default delay: 10ms)
// Configure before starting other threads.
unsafe { MiMalloc::option_set(mi_option_t::mi_option_purge_delay, 0); }

// Read an option
let delay = MiMalloc::option_get(mi_option_t::mi_option_purge_delay);

// Toggle
unsafe {
    MiMalloc::option_enable(mi_option_t::mi_option_show_errors);
    MiMalloc::option_disable(mi_option_t::mi_option_show_errors);
}
```

### Small Allocation And Free Fast Paths

```rust
use core::ptr::NonNull;
use rustfs_mimalloc::{MI_SMALL_SIZE_MAX, MiMalloc};

unsafe {
    let ptr = NonNull::new(MiMalloc::malloc_csize(64)).expect("allocation failed");
    assert!(64 <= MI_SMALL_SIZE_MAX);
    MiMalloc::free_csize_nonnull(ptr, 64);
}
```

Use `MiMalloc::malloc_csize`, `zalloc_csize`, `wmalloc_small`, `wzalloc_small`,
`free_csize`, `free_csize_nonnull`, `free_small`, and `free_small_nonnull` only
when the original allocation size contract is known and the pointer is managed
by mimalloc. These mirror mimalloc V3.5.4's small, word-size, and constant-size
fast paths for language runtimes and other allocation-heavy systems.

Use `MiMalloc::free_csize_aligned` or `free_csize_aligned_nonnull` when both
the original size and alignment are known. They preserve the small-free fast
path only when the allocation is not over-aligned, avoiding incorrect routing
for small allocations with a larger alignment.

### Upstream APIs and migration

The submodule pins [upstream commit a28efddd](https://github.com/microsoft/mimalloc/commit/a28efddd3658084937d285858772481f676c4045),
after the v3.5.4 interim tag. The source version remains 30504. This pin
includes upstream fixes through 2026-10-08, plus new statistics-merge and
arena-purge controls.

- `MiMalloc::free_small_local{,_nonnull}` exposes the new thread-local free fast
  path. These unsafe functions require a small allocation whose page is **still
  owned by the calling thread**. They are unsuitable for general cross-thread
  frees and are not used by `GlobalAlloc`.
- `mi_profiler_snapshot` and `mi_pprof_profiler_{new,delete}` are available as
  experimental raw FFI, together with all four `mi_option_profile_*` controls.
  The profiler ABI adds `on_snapshot`; allocation/free callbacks now receive
  `*mut mi_heap_t`. Update custom `mi_profiler_t` initializers accordingly.
- `Heap::zalloc_aligned` and `Heap::realloc_aligned` preserve requested alignment.
  `MiMalloc::good_size`, `options_print`, `option_get_clamp`, and default option
  setters support capacity planning and diagnostics.
- `MiMalloc::arenas_purge()` immediately processes pending arena purges for the
  main subprocess. It can issue OS memory calls and should be used where the
  release of delayed arena memory is worth the added latency. Configure
  `mi_option_arena_purge_immediate_size` to eagerly purge large freed ranges, or
  `mi_option_stats_merge_threshold` to tune thread-heap statistics merging.
- Option setters/toggles are now **unsafe**: upstream storage is not atomic.
  Configure before starting other threads or exclude all concurrent mimalloc
  access; a mutex around setters alone is insufficient.

To collect a built-in heap profile, start the application with
`MIMALLOC_PROFILE=/path/to/profile` (protobuf), or a name ending in `.heap`
(text). Sampling is inactive unless enabled.

**Interim limitation:** the pinned upstream code copies `profile_disabled=true`
into new thread heaps without clearing it. Dedicated-heap and worker-thread
profiling may therefore remain inactive. The integration test verifies actual
samples on the main heap in a single-threaded process; it does not establish
complete multi-thread profiling coverage. Custom profilers must remain alive
until sampling is stopped, the profiler is detached, all sampled allocations
are freed, and callbacks have finished. There is deliberately no safe RAII
profiler wrapper yet.

Backtraces are configured for macOS, GNU Linux and Windows. musl builds keep
profiling APIs but do not automatically link an external unwinder; their stack
traces may be empty. FreeBSD/DragonFly require `libexecinfo` and `libutil`.

### Diagnostic Callbacks

The sys crate exposes upstream's existing `mi_register_error`,
`mi_register_output`, and `mi_register_deferred_free` functions. These are raw,
unsafe APIs: register without concurrent allocator activity, keep callback state
alive until unregistration and completion of in-flight calls, and do not unwind
through C. Prefer `MiMalloc::stats_print` / `options_print` when a process-wide
callback is unnecessary.

### Threadpool Hint

```rust
rustfs_mimalloc::set_current_thread_in_threadpool();
```

Call this from custom thread-pool worker threads to tell mimalloc that the
current thread can run arbitrary tasks. This is a thin wrapper around mimalloc
V3's `mi_thread_set_in_threadpool()` API.

### Heap Management

```rust
use rustfs_mimalloc::heap::Heap;

// Create a custom heap
let heap = Heap::new().expect("failed to create heap");

// Allocate from it
unsafe {
    let ptr = heap.malloc(128);
    core::ptr::write_bytes(ptr, 0xAB, 128);
    rustfs_mimalloc_sys::mi_free(ptr as *mut core::ffi::c_void);
}

// Delete heap (moves live blocks to main heap)
heap.delete();
```

### Arena Management

```rust
use rustfs_mimalloc::heap;

// Reserve a memory arena
let arena = heap::reserve_os_memory(1024 * 1024 * 64, true, true, true)
    .expect("failed to reserve arena");

// Create a heap in the arena
let heap = heap::Heap::new_in_arena(arena).expect("failed to create heap");
```

## Comparison with `mimalloc` crate

| Aspect | `rustfs-mimalloc` | `mimalloc` crate |
|--------|-------------------|-------------------|
| mimalloc version | V3 only (post-v3.5.4 interim, commit `a28efddd`) | V2/V3 (configurable) |
| Alignment | Always aligned | Conditional |
| TLS model | Configurable | Forced `initial-exec` |
| Stats API | JSON + text + struct | JSON only |
| Options API | Full get/set/enable/disable | Partial |
| Heap API | Full (create/delete/destroy/alloc) | Basic |
| Arena API | Full (reserve/manage) | Basic |
| Cargo MSRV declaration | None; stable and 1.96.0 checked in CI | 1.46.0 |

## Design Decisions

### V3 Only

This crate exclusively targets mimalloc V3. Key improvements over V2:

- Metadata separated from heap objects (better cache behavior)
- Improved arena management with exclusive arenas
- Guard page support for debugging
- Theap (thread-local heap) API for fine-grained control
- Better multi-threaded performance

### Always Use Aligned Allocation

`GlobalAlloc` allocation and reallocation methods use mimalloc's aligned APIs. Previous implementations tried to skip aligned calls for small alignments, causing [alignment bugs](https://github.com/purpleprotocol/mimalloc_rust/issues/87) and [crashes](https://github.com/purpleprotocol/mimalloc_rust/issues/128). Its cost should be measured for the target workload before considering any fast-path change.

### Platform TLS Models

Linux and FreeBSD use `initial-exec` for performance by default. Enable `local_dynamic_tls` for dynamic-loading compatibility. macOS retains upstream's pthread TLS implementation; the feature does not switch it to compiler thread locals.

## Platform Support

| OS | Architecture | Status |
|----|-------------|--------|
| Linux | x86_64, aarch64, arm, riscv64 | ✅ Tested in CI |
| macOS | x86_64, aarch64 | ✅ Tested in CI |
| Windows | x86_64, aarch64 | ✅ Tested in CI |
| FreeBSD | x86_64 | ✅ Should work |
| Linux (musl) | x86_64 | ✅ Tested in CI |

### Windows Static CRT

For an MSVC binary that must not depend on `vcruntime140.dll` or `ucrtbase.dll`,
enable Rust's static CRT target feature for the final binary. The build script
passes the matching `/MT` setting to mimalloc through `cc`; do not add a
conflicting `/MD` flag manually.

```toml
# .cargo/config.toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

This is validated in the Windows CI matrix.

## Builds Without Allocation Profiling

Applications that do not need sampled heap profiles can opt out at build time:

```toml
[dependencies]
rustfs-mimalloc = { version = "0.6.0", features = ["no_profile"] }
```

This removes profiling sampling from allocation/free paths and disables
`MIMALLOC_PROFILE` automatic startup. Allocation alignment, zeroing, statistics,
error/output/deferred-free callbacks, and `secure`/`debug` checks remain
available. Debug builds may still sample guarded allocations for debugging.

Use `MiMalloc::profiling_enabled()` to query the linked allocator's capability.
Cargo features unify across dependencies: enabling `no_profile` anywhere disables
allocation profiling for that linked allocator. Leave it off when profiling is
required. It is off by default. Benchmark your workload before enabling it:
local tests improved 64 KiB allocations but slightly slowed 64-byte allocations;
no reliable 4 KiB single-thread gain was established.

Raw profiler control symbols remain available for ABI compatibility. Explicit
snapshot callbacks still run, but profiles contain no allocation samples. The
feature changes sampling capability; it does not change page-retention, purging,
NUMA or thread-local heap ownership policies.

Reproduce default versus `no_profile` measurements with
`python3 scripts/bench_profile_abba.py --offline --output target/profile-abba-run1`.
The driver freezes both builds before measuring A1/B1/B2/A2 and records per-workload
drift checks; omit `--offline` if dependencies are not cached.

## Cached Thread-local Heap Views

When one worker frequently alternates between several heaps, cache a view for
that worker to avoid repeating mimalloc's TLS lookup on each allocation:

```rust
use rustfs_mimalloc::heap::Heap;

let heap = Heap::new().expect("heap allocation failed");
let local = heap.thread_local().expect("thread context unavailable");
unsafe {
    let ptr = local.zalloc_aligned(4096, 64);
    assert!(!ptr.is_null());
    // Use the block, then free it normally (also permitted on another thread).
    rustfs_mimalloc_sys::mi_free(ptr.cast());
}
```

`ThreadHeap` borrows its parent heap and is neither `Send` nor `Sync`. Its
`malloc`, `zalloc`, `malloc_aligned` and `zalloc_aligned` methods return raw
pointers with the same allocation contracts as the parent heap. Keep the view
on its creating thread, create it outside hot loops, and do not use it from
thread-local destructors or after explicit mimalloc thread finalization. Dropping the view does
not free blocks; deleting its parent preserves live allocations in the main
heap. This API does not change `GlobalAlloc` or global page-retention settings.

The `heap_lookup` benchmark is a single-heap control; `heap_switching`
alternates two heaps. Run reproducible comparisons with:

```sh
python3 scripts/bench_heap_abba.py --output target/heap-abba-run1
```

The script records A1/B1/B2/A2 in fresh processes and rejects speedup conclusions
if either baseline drift or candidate drift exceeds 5%. Add `--offline` when
all Cargo dependencies are already cached. Results are local microbenchmarks,
not a guarantee of production throughput.

## Rust Toolchain Compatibility

The workspace and member manifests intentionally omit `rust-version`. CI
checks the stable toolchain and compatibility with Rust 1.96.0. A passing
compatibility job does not establish the lowest supported Rust version;
dependencies may have their own requirements.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).

The mimalloc C library is licensed under the MIT License. See [rustfs-mimalloc-sys/c_src/mimalloc/LICENSE](rustfs-mimalloc-sys/c_src/mimalloc/LICENSE).
