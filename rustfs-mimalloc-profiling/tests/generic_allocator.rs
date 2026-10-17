use rustfs_mimalloc_profiling::SamplingAllocator;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

#[derive(Default)]
struct State {
    calls: AtomicU8,
    realloc_mode: AtomicU8,
}

struct StatefulAllocator(Arc<State>);

// SAFETY: Every successful allocation belongs to System. Deallocation uses its
// matching layout; forced moves copy the valid prefix before freeing the old block.
unsafe impl GlobalAlloc for StatefulAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.calls.fetch_or(1, Ordering::Relaxed);
        // SAFETY: Forward the caller's valid layout unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        self.0.calls.fetch_or(2, Ordering::Relaxed);
        // SAFETY: Forward the caller's valid layout unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        self.0.calls.fetch_or(4, Ordering::Relaxed);
        // SAFETY: Successful methods above return System allocations.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        self.0.calls.fetch_or(8, Ordering::Relaxed);
        match self.0.realloc_mode.load(Ordering::Relaxed) {
            1 => ptr::null_mut(),
            2 if size == layout.size() => pointer,
            3 => {
                let next_layout = Layout::from_size_align(size, layout.align()).unwrap();
                // SAFETY: The caller supplied a valid nonzero new size and alignment.
                let next = unsafe { System.alloc(next_layout) };
                if !next.is_null() {
                    // SAFETY: Distinct live allocations do not overlap; the prefix
                    // fits both blocks. The old block is released only after copying.
                    unsafe {
                        ptr::copy_nonoverlapping(pointer, next, layout.size().min(size));
                        System.dealloc(pointer, layout);
                    }
                }
                next
            }
            // SAFETY: Forward the live allocation and its matching layout unchanged.
            _ => unsafe { System.realloc(pointer, layout, size) },
        }
    }
}

#[test]
fn stateful_provider_preserves_alignment_zeroing_realloc_and_cross_thread_free() {
    let state = Arc::new(State::default());
    let allocator = SamplingAllocator::with_allocator(StatefulAllocator(Arc::clone(&state)));
    let layout = Layout::from_size_align(128, 128).unwrap();
    // SAFETY: Nonzero, valid layout. Ownership is retained until the final free.
    let pointer = unsafe { allocator.alloc_zeroed(layout) };
    assert!(!pointer.is_null());
    assert_eq!(pointer as usize % layout.align(), 0);
    // SAFETY: All 128 bytes are initialized by alloc_zeroed and remain live.
    unsafe {
        assert!(
            std::slice::from_raw_parts(pointer, 128)
                .iter()
                .all(|byte| *byte == 0)
        );
        ptr::write_bytes(pointer, 0x5a, 128);
    }
    state.realloc_mode.store(1, Ordering::Relaxed);
    // SAFETY: The original allocation is live; failure must leave it intact.
    assert!(unsafe { allocator.realloc(pointer, layout, 256) }.is_null());
    // SAFETY: Failed realloc preserves the original pointer and contents.
    assert!(
        unsafe { std::slice::from_raw_parts(pointer, 128) }
            .iter()
            .all(|byte| *byte == 0x5a)
    );
    state.realloc_mode.store(2, Ordering::Relaxed);
    // SAFETY: Same-size realloc preserves a valid block in this provider.
    let in_place = unsafe { allocator.realloc(pointer, layout, 128) };
    assert_eq!(in_place, pointer);
    state.realloc_mode.store(3, Ordering::Relaxed);
    // SAFETY: The allocation returned by the previous realloc is live.
    let moved = unsafe { allocator.realloc(in_place, layout, 256) };
    assert!(!moved.is_null());
    assert_ne!(moved, in_place);
    // SAFETY: Successful realloc preserves the initialized original prefix.
    assert!(
        unsafe { std::slice::from_raw_parts(moved, 128) }
            .iter()
            .all(|byte| *byte == 0x5a)
    );
    std::thread::scope(|scope| {
        let address = moved as usize;
        let allocator = &allocator;
        scope
            .spawn(move || {
                // SAFETY: Ownership is transferred to this worker, which frees the
                // replacement exactly once with its new layout and original provider.
                unsafe {
                    allocator.dealloc(
                        address as *mut u8,
                        Layout::from_size_align(256, 128).unwrap(),
                    )
                };
            })
            .join()
            .unwrap();
    });
    let layout = Layout::from_size_align(64, 256).unwrap();
    // SAFETY: Allocate and free a fresh block with its matching layout.
    unsafe {
        let pointer = allocator.alloc(layout);
        assert!(!pointer.is_null());
        assert_eq!(pointer as usize % 256, 0);
        allocator.dealloc(pointer, layout);
    }
    assert_eq!(state.calls.load(Ordering::Relaxed), 15);
}
