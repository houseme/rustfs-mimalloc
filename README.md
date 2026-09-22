# rustfs-mimalloc

[![Crates.io](https://img.shields.io/crates/v/rustfs-mimalloc.svg)](https://crates.io/crates/rustfs-mimalloc)
[![Documentation](https://docs.rs/rustfs-mimalloc/badge.svg)](https://docs.rs/rustfs-mimalloc)
[![CI](https://github.com/houseme/rustfs-mimalloc/actions/workflows/ci.yml/badge.svg)](https://github.com/houseme/rustfs-mimalloc/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.96.0-orange.svg)](#minimum-supported-rust-version)

High-performance [mimalloc](https://github.com/microsoft/mimalloc) V3 global allocator for Rust.

## Overview

`rustfs-mimalloc` provides safe, ergonomic Rust bindings to Microsoft's mimalloc V3 memory allocator (v3.5.3). Drop-in replacement for the system allocator with excellent multi-threaded performance.

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
rustfs-mimalloc = "0.5.5"
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
| `secure` | | Heap allocation encryption (MI_SECURE=4) |
| `debug` | | mimalloc debug checks |
| `debug_in_debug` | | Auto-enable `debug` in Cargo debug builds |
| `override` | | Override system `malloc`/`free` |
| `local_dynamic_tls` | | Use local-dynamic TLS model (fixes polars compatibility) |
| `no_thp` | | Disable Transparent Huge Pages on Linux/Android |

All stats, options, heap, and arena APIs are available without any feature flag.

## API

### Global Allocator

```rust
use rustfs_mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
```

Implements `GlobalAlloc` with `alloc`, `alloc_zeroed`, `dealloc`, `realloc` — all using `mi_malloc_aligned` for guaranteed alignment.

### Statistics & Diagnostics

```rust
use rustfs_mimalloc::MiMalloc;
use rustfs_mimalloc_sys::mi_option_t;

// Version: 30503 = V3.5.3
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
MiMalloc::option_set(mi_option_t::mi_option_purge_delay, 0);

// Read an option
let delay = MiMalloc::option_get(mi_option_t::mi_option_purge_delay);

// Toggle
MiMalloc::option_enable(mi_option_t::mi_option_show_errors);
MiMalloc::option_disable(mi_option_t::mi_option_show_errors);
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
by mimalloc. These mirror mimalloc V3.5.3's small, word-size, and constant-size
fast paths for language runtimes and other allocation-heavy systems.

Use `MiMalloc::free_csize_aligned` or `free_csize_aligned_nonnull` when both
the original size and alignment are known. They preserve the small-free fast
path only when the allocation is not over-aligned, avoiding incorrect routing
for small allocations with a larger alignment.

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
| mimalloc version | V3 only (v3.5.3) | V2/V3 (configurable) |
| Alignment | Always aligned | Conditional |
| TLS model | Configurable | Forced `initial-exec` |
| Stats API | JSON + text + struct | JSON only |
| Options API | Full get/set/enable/disable | Partial |
| Heap API | Full (create/delete/destroy/alloc) | Basic |
| Arena API | Full (reserve/manage) | Basic |
| MSRV | 1.96.0 | 1.46.0 |

## Design Decisions

### V3 Only

This crate exclusively targets mimalloc V3. Key improvements over V2:

- Metadata separated from heap objects (better cache behavior)
- Improved arena management with exclusive arenas
- Guard page support for debugging
- Theap (thread-local heap) API for fine-grained control
- Better multi-threaded performance

### Always Use Aligned Allocation

All `GlobalAlloc` methods call `mi_malloc_aligned` / `mi_realloc_aligned` internally. Previous implementations tried to skip aligned calls for small alignments, causing [alignment bugs](https://github.com/purpleprotocol/mimalloc_rust/issues/87) and [crashes](https://github.com/purpleprotocol/mimalloc_rust/issues/128). The overhead of always using aligned allocation is negligible.

### No TLS Model Override by Default

The `-ftls-model=initial-exec` flag [breaks compatibility](https://github.com/purpleprotocol/mimalloc_rust/issues/138) with some projects (e.g., polars). Use the `local_dynamic_tls` feature to opt-in.

## Platform Support

| OS | Architecture | Status |
|----|-------------|--------|
| Linux | x86_64, aarch64, arm, riscv64 | ✅ Tested in CI |
| macOS | x86_64, aarch64 | ✅ Tested in CI |
| Windows | x86_64, aarch64 | ✅ Tested in CI |
| FreeBSD | x86_64 | ✅ Should work |
| Linux (musl) | x86_64 | ✅ Tested in CI |

## Minimum Supported Rust Version

**Rust 1.96.0** (2026-05-28). This crate follows a rolling support window for the latest three stable Rust release trains. With Rust 1.98.0 as the current stable release, the supported window is 1.96.x through 1.98.x.

The MSRV is tested in CI and will not change without a minor version bump.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).

The mimalloc C library is licensed under the MIT License. See [rustfs-mimalloc-sys/c_src/mimalloc/LICENSE](rustfs-mimalloc-sys/c_src/mimalloc/LICENSE).
