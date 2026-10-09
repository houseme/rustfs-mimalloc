# rustfs-mimalloc

[![Crates.io](https://img.shields.io/crates/v/rustfs-mimalloc.svg)](https://crates.io/crates/rustfs-mimalloc)
[![Documentation](https://docs.rs/rustfs-mimalloc/badge.svg)](https://docs.rs/rustfs-mimalloc)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](../LICENSE)

High-performance [mimalloc](https://github.com/microsoft/mimalloc) V3 global allocator for Rust.

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

| Feature | Description |
|---------|-------------|
| `secure` | Guard pages and encoded/randomized free lists (MI_SECURE=4) |
| `debug` | mimalloc debug checks |
| `debug_in_debug` | Auto-enable `debug` in Cargo debug builds |
| `override` | Override system `malloc`/`free` on non-Windows targets |
| `local_dynamic_tls` | Use local-dynamic TLS model |
| `no_thp` | Disable Transparent Huge Pages |
| `no_profile` | Compile out allocation profiling and automatic pprof startup |

## API

### Allocator

`MiMalloc` implements `GlobalAlloc`; allocations and reallocations use aligned
APIs, while deallocation uses the general `mi_free` path.

### Stats & Options

```rust
use rustfs_mimalloc::MiMalloc;
use rustfs_mimalloc_sys::mi_option_t;

let version = MiMalloc::version();       // 30504 = V3.5.4 interim source line
let json    = MiMalloc::stats_json();    // stats as JSON
let text    = MiMalloc::stats_print();   // stats as text
let info    = MiMalloc::process_info();  // ProcessInfo struct

// Configure before starting other threads.
unsafe { MiMalloc::option_set(mi_option_t::mi_option_purge_delay, 0); }
```

### Small Allocation And Free Fast Paths

`MiMalloc::malloc_csize`, `zalloc_csize`, `wmalloc_small`, `wzalloc_small`,
`free_csize`, `free_csize_nonnull`, `free_small`, and `free_small_nonnull`
expose mimalloc V3's small, word-size, and constant-size fast paths. These
APIs are unsafe: the pointer must come from mimalloc, and the caller must
preserve the original allocation size contract.

`MiMalloc::free_csize_aligned` and `free_csize_aligned_nonnull` additionally
preserve the correct free path when the original allocation was over-aligned.

### Migration from 0.5

The submodule is pinned to upstream **post-v3.5.4 interim (`a28efddd`)**. Runtime option setters
and toggles are now unsafe because upstream storage is not atomic; configure
before starting threads or exclude all concurrent mimalloc access. The raw
profiler ABI adds `on_snapshot` and changes callback heap pointers to mutable.

`free_small_local{,_nonnull}` requires a small page still owned by the calling
thread. `Heap::zalloc_aligned` and `Heap::realloc_aligned` add aligned heap
operations. `good_size`, `options_print` and clamped/default option access add
diagnostics and tuning support.

`MiMalloc::arenas_purge()` processes pending arena purges in the main
subprocess immediately. `mi_option_arena_purge_immediate_size` can release
large freed arena ranges without making all purges immediate; by default, the
threshold is 4 MiB. `mi_option_stats_merge_threshold` tunes when thread-heap
stats merge into the parent heap; its upstream default is 512 KiB.

The new raw pprof APIs are experimental. The interim upstream implementation
can leave new thread heaps with profiling disabled, so dedicated-heap and
worker-thread sampling are not reliable. See the repository's
[review](https://github.com/houseme/rustfs-mimalloc/blob/main/UPSTREAM_REVIEW.md).

### Threadpool Hint

```rust
rustfs_mimalloc::set_current_thread_in_threadpool();
```

Call this from custom thread-pool worker threads to set mimalloc's current-thread threadpool marker.

### Heap & Arena

```rust
use rustfs_mimalloc::heap::Heap;

let heap = Heap::new().unwrap();
unsafe {
    let ptr = heap.malloc(128);
    rustfs_mimalloc_sys::mi_free(ptr as *mut core::ffi::c_void);
}
heap.delete();
```

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

## Rust Toolchain Compatibility

Cargo manifests intentionally omit `rust-version`. CI checks stable Rust and
Rust 1.96.0 compatibility; no minimum Rust version is declared by these crates.

## License

Apache-2.0.
