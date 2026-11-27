use core::{
    alloc::{AllocError, Allocator, GlobalAlloc, Layout},
    ptr::NonNull,
};

use crate::{MiMalloc, heap::Heap, ordinary_alloc_covers};
use rustfs_mimalloc_sys as sys;

/// A mimalloc allocator that exposes the full usable size of each allocation.
///
/// With Rust's [Allocator] API, containers can use the extra capacity in
/// mimalloc size classes before growing.
#[derive(Clone, Copy, Debug, Default)]
pub struct MiMallocExcess;

type Block = Result<NonNull<[u8]>, AllocError>;

#[inline]
fn block(ptr: *mut u8, len: usize) -> Block {
    NonNull::new(ptr)
        .map(|ptr| NonNull::slice_from_raw_parts(ptr, len.min(isize::MAX as usize)))
        .ok_or(AllocError)
}

#[inline]
fn empty(layout: Layout) -> NonNull<[u8]> {
    NonNull::slice_from_raw_parts(layout.dangling_ptr(), 0)
}

#[inline]
unsafe fn free(ptr: NonNull<u8>, layout: Layout) {
    // Allocator accepts a fitting layout. A sized native free requires the
    // original allocation size, which may differ for MiMallocExcess.
    if layout.size() != 0 {
        unsafe { sys::mi_free(ptr.as_ptr().cast()) };
    }
}

#[inline]
unsafe fn clear_tail(block: NonNull<[u8]>, initialized: usize) {
    if initialized < block.len() {
        unsafe {
            block
                .cast::<u8>()
                .as_ptr()
                .add(initialized)
                .write_bytes(0, block.len() - initialized);
        }
    }
}

#[inline]
unsafe fn resize<A: Allocator>(
    allocator: &A,
    ptr: NonNull<u8>,
    old: Layout,
    new: Layout,
    native: unsafe fn(&A, *mut u8, Layout) -> Block,
) -> Block {
    if old.size() == 0 {
        return allocator.allocate(new);
    }
    if new.size() == 0 {
        unsafe { free(ptr, old) };
        return Ok(empty(new));
    }
    unsafe { native(allocator, ptr.as_ptr(), new) }
}

macro_rules! resize_methods {
    ($native:path) => {
        #[inline]
        unsafe fn grow(&self, ptr: NonNull<u8>, old: Layout, new: Layout) -> Block {
            unsafe { resize(self, ptr, old, new, $native) }
        }

        #[inline]
        unsafe fn grow_zeroed(&self, ptr: NonNull<u8>, old: Layout, new: Layout) -> Block {
            let allocation = unsafe { resize(self, ptr, old, new, $native) }?;
            unsafe { clear_tail(allocation, old.size()) };
            Ok(allocation)
        }

        #[inline]
        unsafe fn shrink(&self, ptr: NonNull<u8>, old: Layout, new: Layout) -> Block {
            unsafe { resize(self, ptr, old, new, $native) }
        }
    };
}

#[inline]
unsafe fn resize_global(_: &MiMalloc, ptr: *mut u8, layout: Layout) -> Block {
    // Allocator permits changing alignment when growing or shrinking.
    let resized = unsafe { sys::mi_realloc_aligned(ptr.cast(), layout.size(), layout.align()) };
    block(resized.cast(), layout.size())
}

// SAFETY: The native allocator keeps live allocations valid across allocator
// instances. Positive sizes follow the GlobalAlloc allocation contract.
unsafe impl Allocator for MiMalloc {
    #[inline]
    fn allocate(&self, layout: Layout) -> Block {
        if layout.size() == 0 {
            return Ok(empty(layout));
        }
        block(unsafe { GlobalAlloc::alloc(self, layout) }, layout.size())
    }

    #[inline]
    fn allocate_zeroed(&self, layout: Layout) -> Block {
        if layout.size() == 0 {
            return Ok(empty(layout));
        }
        block(
            unsafe { GlobalAlloc::alloc_zeroed(self, layout) },
            layout.size(),
        )
    }

    #[inline]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { free(ptr, layout) };
    }

    resize_methods!(resize_global);
}

#[inline]
unsafe fn resize_excess(_: &MiMallocExcess, ptr: *mut u8, layout: Layout) -> Block {
    let (resized, usable) =
        unsafe { MiMalloc::realloc_aligned_with_usable_size(ptr, layout.size(), layout.align()) }
            .ok_or(AllocError)?;
    block(resized.as_ptr(), usable)
}

// SAFETY: The native usable-size APIs report the accessible extent.
// Zeroed methods initialize the entire extent returned to Rust.
unsafe impl Allocator for MiMallocExcess {
    #[inline]
    fn allocate(&self, layout: Layout) -> Block {
        if layout.size() == 0 {
            return Ok(empty(layout));
        }
        let mut usable = 0;
        let ptr = unsafe {
            if ordinary_alloc_covers(layout.align(), layout.size()) {
                sys::mi_umalloc(layout.size(), &mut usable)
            } else {
                sys::mi_umalloc_aligned(layout.size(), layout.align(), &mut usable)
            }
        };
        block(ptr.cast(), usable)
    }

    #[inline]
    fn allocate_zeroed(&self, layout: Layout) -> Block {
        if layout.size() == 0 {
            return Ok(empty(layout));
        }
        let mut usable = 0;
        let ptr = unsafe {
            if ordinary_alloc_covers(layout.align(), layout.size()) {
                sys::mi_uzalloc(layout.size(), &mut usable)
            } else {
                sys::mi_uzalloc_aligned(layout.size(), layout.align(), &mut usable)
            }
        };
        let allocation = block(ptr.cast(), usable)?;
        // Native zeroed allocation only promises the requested bytes.
        unsafe { clear_tail(allocation, layout.size()) };
        Ok(allocation)
    }

    #[inline]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { free(ptr, layout) };
    }

    resize_methods!(resize_excess);
}

#[inline]
unsafe fn resize_heap(heap: &&Heap, ptr: *mut u8, layout: Layout) -> Block {
    let resized = unsafe {
        sys::mi_heap_realloc_aligned(heap.as_ptr(), ptr.cast(), layout.size(), layout.align())
    };
    block(resized.cast(), layout.size())
}

// SAFETY: The heap borrow prevents handle destruction while the allocator is
// usable. Native heap allocation respects the layout and supports remote free.
unsafe impl Allocator for &Heap {
    #[inline]
    fn allocate(&self, layout: Layout) -> Block {
        if layout.size() == 0 {
            return Ok(empty(layout));
        }
        let ptr = unsafe {
            if ordinary_alloc_covers(layout.align(), layout.size()) {
                self.malloc(layout.size())
            } else {
                self.malloc_aligned(layout.size(), layout.align())
            }
        };
        block(ptr, layout.size())
    }

    #[inline]
    fn allocate_zeroed(&self, layout: Layout) -> Block {
        if layout.size() == 0 {
            return Ok(empty(layout));
        }
        let ptr = unsafe {
            if ordinary_alloc_covers(layout.align(), layout.size()) {
                self.zalloc(layout.size())
            } else {
                self.zalloc_aligned(layout.size(), layout.align())
            }
        };
        block(ptr, layout.size())
    }

    #[inline]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { free(ptr, layout) };
    }

    resize_methods!(resize_heap);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise_resize(allocator: impl Allocator) {
        let empty_layout = Layout::from_size_align(0, 4096).unwrap();
        let start = Layout::from_size_align(73, 16).unwrap();
        let large = Layout::from_size_align(8193, 4096).unwrap();
        let empty = allocator.allocate(empty_layout).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.cast::<u8>().as_ptr().addr() % 4096, 0);
        unsafe {
            let block = allocator
                .grow_zeroed(empty.cast(), empty_layout, start)
                .unwrap();
            assert!(block.len() >= start.size());
            assert!(block.as_ref().iter().all(|&byte| byte == 0));
            block.cast::<u8>().as_ptr().write_bytes(0x5a, block.len());
            let block = allocator.grow_zeroed(block.cast(), start, large).unwrap();
            assert!(block.len() >= large.size());
            assert_eq!(block.cast::<u8>().as_ptr().addr() % 4096, 0);
            let bytes = block.as_ref();
            assert!(bytes[..start.size()].iter().all(|&byte| byte == 0x5a));
            assert!(bytes[start.size()..].iter().all(|&byte| byte == 0));
            let block = allocator.shrink(block.cast(), large, start).unwrap();
            assert_eq!(block.cast::<u8>().as_ptr().addr() % 16, 0);
            assert_eq!(
                core::slice::from_raw_parts(block.cast::<u8>().as_ptr(), start.size()),
                &[0x5a; 73]
            );
            let block = allocator.shrink(block.cast(), start, empty_layout).unwrap();
            assert!(block.is_empty());
            allocator.deallocate(block.cast(), empty_layout);
        }
    }

    #[test]
    fn resize_preserves_alignment_and_initialized_contents() {
        exercise_resize(MiMalloc);
        exercise_resize(MiMallocExcess);
        let heap = Heap::new().unwrap();
        exercise_resize(&heap);
    }

    #[test]
    fn excess_accepts_fitting_layout_and_zeros_reported_capacity() {
        let requested = Layout::from_size_align(73, 8).unwrap();
        let allocation = MiMallocExcess.allocate_zeroed(requested).unwrap();
        unsafe {
            assert_eq!(
                allocation.len(),
                MiMalloc::usable_size(allocation.cast::<u8>().as_ptr())
            );
            assert!(allocation.as_ref().iter().all(|&byte| byte == 0));
            let fitting = Layout::from_size_align(allocation.len(), requested.align()).unwrap();
            MiMallocExcess.deallocate(allocation.cast(), fitting);
        }
    }

    #[test]
    fn stable_collections_can_use_all_allocator_modes() {
        let heap = Heap::new().unwrap();
        let mut regular = Vec::new_in(MiMalloc);
        let mut excess = Vec::new_in(MiMallocExcess);
        let mut local = Vec::new_in(&heap);
        for n in 0..1024 {
            regular.push(n);
            excess.push(n);
            local.push(n);
        }
        assert_eq!(regular, excess);
        assert_eq!(regular, local);
        assert!(unsafe { heap.contains(local.as_ptr().cast()) });
    }
}
