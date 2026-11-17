//! High-performance [mimalloc](https://github.com/microsoft/mimalloc) V3 global allocator.
//!
//! ```rust
//! use rustfs_mimalloc::MiMalloc;
//!
//! #[global_allocator]
//! static GLOBAL: MiMalloc = MiMalloc;
//! ```

mod api;
pub mod callback;
mod ffi;

pub mod heap;

pub use api::{ProcessInfo, set_current_thread_in_threadpool};
pub use callback::{
    clear_deferred_free, clear_error, clear_output, register_deferred_free, register_error,
    register_output,
};
pub use rustfs_mimalloc_sys::{MI_SMALL_SIZE_MAX, MI_SMALL_WSIZE_MAX};

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;

/// The mimalloc global allocator.
///
/// Drop-in replacement for the system allocator. Ordinary allocations take
/// mimalloc's native fast path; only layouts that genuinely need more than
/// the native guarantees route to the aligned-allocation APIs, mirroring
/// upstream's own dispatch guards.
#[derive(Debug, Clone, Copy, Default)]
pub struct MiMalloc;

/// mimalloc's `MI_MAX_ALIGN_SIZE` (`max_align_t`): blocks of at least this
/// size are aligned to it by ordinary allocations.
const MI_MAX_ALIGN_SIZE: usize = 16;

/// Word alignment is guaranteed for every mimalloc block, however small.
const MI_WORD_ALIGN: usize = core::mem::size_of::<usize>();

/// Mirror upstream's `mi_theap_malloc_zero_aligned_at` fast-path guard:
/// ordinary allocation already provides the requested alignment when the
/// alignment fits the native guarantees.
#[inline(always)]
const fn ordinary_alloc_covers(align: usize, size: usize) -> bool {
    align <= MI_MAX_ALIGN_SIZE && (align <= MI_WORD_ALIGN || size >= align)
}

// ── GlobalAlloc: hot path — zero indirection ────────────────────────────────

unsafe impl GlobalAlloc for MiMalloc {
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if ordinary_alloc_covers(layout.align(), layout.size()) {
                rustfs_mimalloc_sys::mi_malloc(layout.size()) as *mut u8
            } else {
                rustfs_mimalloc_sys::mi_malloc_aligned(layout.size(), layout.align()) as *mut u8
            }
        }
    }

    #[inline(always)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if ordinary_alloc_covers(layout.align(), layout.size()) {
                rustfs_mimalloc_sys::mi_zalloc(layout.size()) as *mut u8
            } else {
                rustfs_mimalloc_sys::mi_zalloc_aligned(layout.size(), layout.align()) as *mut u8
            }
        }
    }

    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // `mi_free_size_aligned` routes small frees to the page-metadata fast
        // path and degrades to plain `mi_free` for over-aligned allocations,
        // mirroring upstream's own sizing rules.
        unsafe {
            rustfs_mimalloc_sys::mi_free_size_aligned(
                ptr as *mut c_void,
                layout.size(),
                layout.align(),
            )
        };
    }

    #[inline(always)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            if ordinary_alloc_covers(layout.align(), new_size)
                && (ptr as usize).is_multiple_of(layout.align())
            {
                rustfs_mimalloc_sys::mi_realloc(ptr as *mut c_void, new_size) as *mut u8
            } else {
                rustfs_mimalloc_sys::mi_realloc_aligned(
                    ptr as *mut c_void,
                    new_size,
                    layout.align(),
                ) as *mut u8
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout};

    #[global_allocator]
    static GLOBAL: MiMalloc = MiMalloc;

    #[test]
    fn alloc_dealloc_roundtrip() {
        let layout = Layout::from_size_align(64, 8).unwrap();
        unsafe {
            let ptr = GLOBAL.alloc(layout);
            assert!(!ptr.is_null());
            GLOBAL.dealloc(ptr, layout);
        }
    }

    #[test]
    fn alloc_zeroed_is_zero() {
        let layout = Layout::from_size_align(256, 16).unwrap();
        unsafe {
            let ptr = GLOBAL.alloc_zeroed(layout);
            assert!(!ptr.is_null());
            assert!((0..256).all(|i| *ptr.add(i) == 0));
            GLOBAL.dealloc(ptr, layout);
        }
    }

    #[test]
    fn realloc_preserves_content() {
        let layout = Layout::from_size_align(64, 8).unwrap();
        unsafe {
            let ptr = GLOBAL.alloc(layout);
            core::ptr::write_bytes(ptr, 0xAB, 64);
            let new_ptr = GLOBAL.realloc(ptr, layout, 128);
            assert!(!new_ptr.is_null());
            assert!((0..64).all(|i| *new_ptr.add(i) == 0xAB));
            GLOBAL.dealloc(new_ptr, Layout::from_size_align(128, 8).unwrap());
        }
    }

    #[test]
    fn alignment_respected() {
        for align_pow in 0..=12 {
            let align = 1usize << align_pow;
            let layout = Layout::from_size_align(64, align).unwrap();
            unsafe {
                let ptr = GLOBAL.alloc(layout);
                assert!(!ptr.is_null(), "align={align}");
                assert_eq!(ptr as usize % align, 0, "align={align}");
                GLOBAL.dealloc(ptr, layout);
            }
        }
    }

    #[test]
    fn large_alignment_page_size() {
        // Regression: mimalloc_rust#87 — 4096 alignment crashed
        let layout = Layout::from_size_align(4096, 4096).unwrap();
        for _ in 0..100 {
            unsafe {
                let ptr = GLOBAL.alloc(layout);
                assert!(!ptr.is_null());
                assert_eq!(ptr as usize % 4096, 0);
                GLOBAL.dealloc(ptr, layout);
            }
        }
    }

    #[test]
    fn size_below_alignment_stays_aligned() {
        // size < align must take the aligned path: ordinary blocks smaller
        // than the alignment only carry word alignment.
        for &(size, align) in &[(1usize, 16usize), (4, 16), (1, 8), (2, 8), (8, 16)] {
            let layout = Layout::from_size_align(size, align).unwrap();
            unsafe {
                let ptr = GLOBAL.alloc(layout);
                assert!(!ptr.is_null(), "size={size} align={align}");
                assert_eq!(ptr as usize % align, 0, "alloc size={size} align={align}");
                let zeroed = GLOBAL.alloc_zeroed(layout);
                assert!(!zeroed.is_null());
                assert_eq!(
                    zeroed as usize % align,
                    0,
                    "zeroed size={size} align={align}"
                );
                assert!((0..size).all(|i| *zeroed.add(i) == 0));
                // Growing to or from below-alignment sizes keeps the contract.
                let grown = GLOBAL.realloc(ptr, layout, align * 2);
                assert_eq!(grown as usize % align, 0, "grow size={size} align={align}");
                let shrunk = GLOBAL.realloc(
                    grown,
                    Layout::from_size_align(align * 2, align).unwrap(),
                    size,
                );
                assert_eq!(
                    shrunk as usize % align,
                    0,
                    "shrink size={size} align={align}"
                );
                GLOBAL.dealloc(shrunk, layout);
                GLOBAL.dealloc(zeroed, layout);
            }
        }
    }

    #[test]
    fn vec_push_smoke() {
        let v: Vec<i32> = (0..10_000).collect();
        assert_eq!(v.len(), 10_000);
        assert_eq!(v[9999], 9999);
    }

    #[test]
    fn box_smoke() {
        let b = Box::new(42u64);
        assert_eq!(*b, 42);
    }

    #[test]
    fn concurrent_alloc_free() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let n = 8;
        let barrier = Arc::new(Barrier::new(n));
        let handles: Vec<_> = (0..n)
            .map(|_| {
                let b = barrier.clone();
                thread::spawn(move || {
                    b.wait();
                    let layout = Layout::from_size_align(128, 16).unwrap();
                    for _ in 0..1000 {
                        unsafe {
                            let ptr = GLOBAL.alloc(layout);
                            assert!(!ptr.is_null());
                            core::ptr::write_bytes(ptr, 0xCD, 128);
                            GLOBAL.dealloc(ptr, layout);
                        }
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }
}
