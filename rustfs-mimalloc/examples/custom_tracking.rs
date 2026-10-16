//! Application-provided hooks from upstream PR #1396.
//!
//! Build with `CFLAGS=-DMI_TRACK_CUSTOM cargo run --release -p rustfs-mimalloc
//! --example custom_tracking`. The hooks below only use atomics: allocating or
//! formatting from a hook could recursively invoke the allocator.

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

const REQUEST: usize = 123_457;
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED: AtomicUsize = AtomicUsize::new(0);
static USABLE: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL: rustfs_mimalloc::MiMalloc = rustfs_mimalloc::MiMalloc;

/// Called concurrently by C; must never allocate, panic or unwind.
#[unsafe(no_mangle)]
pub extern "C" fn mi_track_malloc_hook(ptr: *const c_void, requested: usize, size: usize, _: bool) {
    if requested == REQUEST {
        REQUESTED.store(requested, Ordering::Relaxed);
        USABLE.store(size, Ordering::Relaxed);
        ADDRESS.store(ptr as usize, Ordering::Relaxed);
    }
}

/// A heap-destroy operation does not emit one hook for every freed block.
#[unsafe(no_mangle)]
pub extern "C" fn mi_track_free_hook(ptr: *const c_void, size: usize) {
    if ADDRESS.load(Ordering::Relaxed) == ptr as usize {
        FREED.store(size, Ordering::Relaxed);
    }
}

fn main() {
    // This unaligned path demonstrates the exact requested-size metadata.
    // Aligned allocations may report a backing block rather than the user pointer.
    unsafe {
        let ptr = rustfs_mimalloc_sys::mi_malloc(REQUEST);
        assert!(!ptr.is_null());
        let hooked = ADDRESS.load(Ordering::Relaxed) == ptr as usize;
        rustfs_mimalloc_sys::mi_free(ptr);
        if !hooked {
            eprintln!("Custom hooks inactive; rebuild with CFLAGS=-DMI_TRACK_CUSTOM");
            std::process::exit(1);
        }
    }
    assert_eq!(REQUESTED.load(Ordering::Relaxed), REQUEST);
    assert!(USABLE.load(Ordering::Relaxed) >= REQUEST);
    assert_eq!(
        FREED.load(Ordering::Relaxed),
        USABLE.load(Ordering::Relaxed)
    );
    println!("custom allocation/free hooks received matching allocation metadata");
}
