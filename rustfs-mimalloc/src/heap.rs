//! Heap and arena operations for advanced memory management.

use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr::NonNull;

// ── Error type ──────────────────────────────────────────────────────────────

/// Error returned by arena operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaError {
    /// The OS call failed (e.g., out of memory, invalid parameters).
    Failed,
}

// ── Heap ────────────────────────────────────────────────────────────────────

/// A handle to a mimalloc heap.
///
/// Allocations from a heap can be freed from any thread.
/// Dropping a `Heap` moves its live blocks to the main heap (via `mi_heap_delete`).
pub struct Heap {
    ptr: NonNull<rustfs_mimalloc_sys::mi_heap_t>,
    owned: bool,
}

// SAFETY: V3 first-class heaps support allocation and collection from any thread.
// Destruction consumes the owned handle; borrowed handles require the caller of
// `heap_of` to uphold the heap lifetime and synchronization contract.
unsafe impl Send for Heap {}
unsafe impl Sync for Heap {}

/// A cached, thread-local view of a [`Heap`].
///
/// Create one view per heap on each worker and reuse it to avoid repeated TLS
/// lookups when alternating between heaps. Allocations may still be freed on
/// any thread using `mi_free`. Dropping this view does not free allocations.
/// Do not use cached views from thread-local destructors: mimalloc's own thread
/// teardown can invalidate its context before the parent heap is deleted.
///
/// The view cannot outlive its borrowed heap:
/// ```compile_fail
/// use rustfs_mimalloc::heap::Heap;
/// let heap = Heap::new().unwrap();
/// let local = heap.thread_local().unwrap();
/// heap.delete();
/// unsafe { local.malloc(64); }
/// ```
///
/// A view cannot be moved to another thread:
/// ```compile_fail
/// use rustfs_mimalloc::heap::Heap;
/// let heap = Heap::new().unwrap();
/// let local = heap.thread_local().unwrap();
/// std::thread::scope(|scope| {
///     scope.spawn(move || unsafe { local.malloc(64); });
/// });
/// ```
///
/// It cannot be shared with another thread either:
/// ```compile_fail
/// use rustfs_mimalloc::heap::Heap;
/// let heap = Heap::new().unwrap();
/// let local = heap.thread_local().unwrap();
/// let shared = &local;
/// std::thread::scope(|scope| {
///     scope.spawn(move || unsafe { shared.malloc(64); });
/// });
/// ```
#[must_use]
pub struct ThreadHeap<'heap> {
    // NonNull deliberately prevents Send and Sync. The borrow prevents safe
    // deletion of the parent while upstream's per-thread pointer is cached.
    ptr: NonNull<rustfs_mimalloc_sys::mi_theap_t>,
    _heap: PhantomData<&'heap Heap>,
}

impl ThreadHeap<'_> {
    /// Allocate from this heap without another TLS lookup.
    ///
    /// # Safety
    /// Check the result for null before access and free successful allocations
    /// with a compatible mimalloc free function. No Rust type alignment is implied.
    /// The creating thread's mimalloc context must remain initialized: do not use
    /// the view from thread-local destructors or after an explicit `mi_thread_done`.
    #[inline]
    pub unsafe fn malloc(&self, size: usize) -> *mut u8 {
        unsafe { rustfs_mimalloc_sys::mi_theap_malloc(self.ptr.as_ptr(), size).cast() }
    }

    /// Allocate zeroed memory without another TLS lookup.
    ///
    /// # Safety
    /// The same pointer lifetime and alignment requirements as [`Self::malloc`] apply.
    #[inline]
    pub unsafe fn zalloc(&self, size: usize) -> *mut u8 {
        unsafe { rustfs_mimalloc_sys::mi_theap_zalloc(self.ptr.as_ptr(), size).cast() }
    }

    /// Allocate with a power-of-two alignment without another TLS lookup.
    ///
    /// # Safety
    /// `alignment` must be a nonzero power of two. Check the result for null
    /// before access and free it with `mi_free` or an appropriate aligned free.
    /// The cached-context lifetime requirement in [`Self::malloc`] also applies.
    #[inline]
    pub unsafe fn malloc_aligned(&self, size: usize, alignment: usize) -> *mut u8 {
        unsafe {
            rustfs_mimalloc_sys::mi_theap_malloc_aligned(self.ptr.as_ptr(), size, alignment).cast()
        }
    }

    /// Allocate zeroed memory with a power-of-two alignment.
    ///
    /// # Safety
    /// The same requirements as [`Self::malloc_aligned`] apply.
    #[inline]
    pub unsafe fn zalloc_aligned(&self, size: usize, alignment: usize) -> *mut u8 {
        unsafe {
            rustfs_mimalloc_sys::mi_theap_zalloc_aligned(self.ptr.as_ptr(), size, alignment).cast()
        }
    }
}

impl Heap {
    /// Create a new heap. Returns `None` on OOM.
    pub fn new() -> Option<Self> {
        NonNull::new(unsafe { rustfs_mimalloc_sys::mi_heap_new() }).map(Self::owned)
    }

    /// Cache this heap's allocation context for the calling thread.
    ///
    /// The returned view borrows this handle and is neither `Send` nor `Sync`.
    /// Create it once outside an allocation loop. Returns `None` if upstream
    /// returns a null context; individual allocations may still fail separately.
    ///
    /// ```
    /// use rustfs_mimalloc::heap::Heap;
    /// let heap = Heap::new().unwrap();
    /// let local = heap.thread_local().unwrap();
    /// unsafe {
    ///     let ptr = local.zalloc_aligned(4096, 64);
    ///     assert!(!ptr.is_null());
    ///     rustfs_mimalloc_sys::mi_free(ptr.cast());
    /// }
    /// ```
    pub fn thread_local(&self) -> Option<ThreadHeap<'_>> {
        // SAFETY: this handle keeps the heap alive. The returned pointer is used
        // only on this thread and cannot outlive the borrowed heap handle.
        NonNull::new(unsafe { rustfs_mimalloc_sys::mi_heap_theap(self.ptr.as_ptr()) }).map(|ptr| {
            ThreadHeap {
                ptr,
                _heap: PhantomData,
            }
        })
    }

    /// Create a heap that allocates exclusively from the given arena.
    pub fn new_in_arena(arena_id: ArenaId) -> Option<Self> {
        NonNull::new(unsafe { rustfs_mimalloc_sys::mi_heap_new_in_arena(arena_id.0) })
            .map(Self::owned)
    }

    /// Get the main heap.
    pub fn main() -> Self {
        let ptr = unsafe { rustfs_mimalloc_sys::mi_heap_main() };
        Self::borrowed(NonNull::new(ptr).expect("mi_heap_main returned null"))
    }

    /// Get the heap that owns `ptr`.
    ///
    /// # Safety
    /// `ptr` must be a valid mimalloc-allocated pointer. The owning heap must
    /// remain alive for the returned handle's entire lifetime, and must not be
    /// deleted or destroyed concurrently with any operation on this handle.
    pub unsafe fn heap_of(ptr: *const u8) -> Option<Self> {
        NonNull::new(unsafe { rustfs_mimalloc_sys::mi_heap_of(ptr as *const c_void) })
            .map(Self::borrowed)
    }

    /// Check if this heap contains `ptr`.
    ///
    /// # Safety
    /// `ptr` must be valid.
    pub unsafe fn contains(&self, ptr: *const u8) -> bool {
        unsafe { rustfs_mimalloc_sys::mi_heap_contains(self.ptr.as_ptr(), ptr as *const c_void) }
    }

    /// Allocate `size` bytes from this heap.
    ///
    /// # Safety
    /// The returned pointer must be freed with `mi_free` (cross-heap frees are allowed).
    pub unsafe fn malloc(&self, size: usize) -> *mut u8 {
        unsafe { rustfs_mimalloc_sys::mi_heap_malloc(self.ptr.as_ptr(), size) as *mut u8 }
    }

    /// Allocate zero-initialized memory from this heap.
    ///
    /// # Safety
    /// The returned pointer must be freed with `mi_free`.
    pub unsafe fn zalloc(&self, size: usize) -> *mut u8 {
        unsafe { rustfs_mimalloc_sys::mi_heap_zalloc(self.ptr.as_ptr(), size) as *mut u8 }
    }

    /// Allocate aligned memory from this heap.
    ///
    /// # Safety
    /// The returned pointer must be freed with `mi_free`.
    pub unsafe fn malloc_aligned(&self, size: usize, alignment: usize) -> *mut u8 {
        unsafe {
            rustfs_mimalloc_sys::mi_heap_malloc_aligned(self.ptr.as_ptr(), size, alignment)
                as *mut u8
        }
    }

    /// Allocate zeroed memory with a power-of-two alignment.
    ///
    /// # Safety
    /// Check the result for null; free a successful allocation with `mi_free`.
    pub unsafe fn zalloc_aligned(&self, size: usize, alignment: usize) -> *mut u8 {
        unsafe {
            rustfs_mimalloc_sys::mi_heap_zalloc_aligned(self.ptr.as_ptr(), size, alignment).cast()
        }
    }

    /// Reallocate memory while retaining its power-of-two alignment.
    ///
    /// # Safety
    /// `ptr` must be null or a live mimalloc allocation with the given alignment.
    /// On failure the original allocation remains valid. Free the successful
    /// result with `mi_free`; do not use the old pointer after success.
    pub unsafe fn realloc_aligned(
        &self,
        ptr: *mut u8,
        new_size: usize,
        alignment: usize,
    ) -> *mut u8 {
        unsafe {
            rustfs_mimalloc_sys::mi_heap_realloc_aligned(
                self.ptr.as_ptr(),
                ptr.cast(),
                new_size,
                alignment,
            )
            .cast()
        }
    }

    /// Reallocate memory from this heap.
    ///
    /// # Safety
    /// `ptr` must be a valid mimalloc pointer. The returned pointer must be freed with `mi_free`.
    pub unsafe fn realloc(&self, ptr: *mut u8, new_size: usize) -> *mut u8 {
        unsafe {
            rustfs_mimalloc_sys::mi_heap_realloc(self.ptr.as_ptr(), ptr as *mut c_void, new_size)
                as *mut u8
        }
    }

    /// Delete this heap, moving live blocks to the main heap.
    /// Consumes `self` without running `Drop`. Borrowed heap handles are left untouched.
    pub fn delete(self) {
        if self.owned {
            unsafe { rustfs_mimalloc_sys::mi_heap_delete(self.ptr.as_ptr()) }
        }
        core::mem::forget(self);
    }

    /// Destroy this heap, freeing all live blocks.
    ///
    /// # Safety
    /// All pointers from this heap become dangling. Borrowed heap handles are left untouched.
    pub unsafe fn destroy(self) {
        if self.owned {
            unsafe { rustfs_mimalloc_sys::mi_heap_destroy(self.ptr.as_ptr()) };
        }
        core::mem::forget(self);
    }

    /// Force garbage collection on this heap.
    pub fn collect(&self, force: bool) {
        unsafe { rustfs_mimalloc_sys::mi_heap_collect(self.ptr.as_ptr(), force) }
    }

    /// Allocation statistics for this heap as JSON. Returns empty string on failure.
    pub fn stats_json(&self) -> String {
        unsafe {
            crate::ffi::owned_mimalloc_string(rustfs_mimalloc_sys::mi_heap_stats_get_json(
                self.ptr.as_ptr(),
                0,
                core::ptr::null_mut(),
            ))
        }
    }

    /// Allocation statistics for this heap in mimalloc's human-readable text format.
    pub fn stats_print(&self) -> String {
        crate::ffi::collect_mimalloc_output(|out, arg| unsafe {
            rustfs_mimalloc_sys::mi_heap_stats_print_out(self.ptr.as_ptr(), out, arg);
        })
    }

    /// Raw pointer to the underlying `mi_heap_t`.
    pub fn as_ptr(&self) -> *mut rustfs_mimalloc_sys::mi_heap_t {
        self.ptr.as_ptr()
    }

    #[inline]
    fn owned(ptr: NonNull<rustfs_mimalloc_sys::mi_heap_t>) -> Self {
        Self { ptr, owned: true }
    }

    #[inline]
    fn borrowed(ptr: NonNull<rustfs_mimalloc_sys::mi_heap_t>) -> Self {
        Self { ptr, owned: false }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        if self.owned {
            unsafe { rustfs_mimalloc_sys::mi_heap_delete(self.ptr.as_ptr()) }
        }
    }
}

// ── Arena ───────────────────────────────────────────────────────────────────

/// Arena identifier for managing memory regions.
#[derive(Debug, Clone, Copy)]
pub struct ArenaId(rustfs_mimalloc_sys::mi_arena_id_t);

unsafe impl Send for ArenaId {}
unsafe impl Sync for ArenaId {}

/// Reserve OS memory as an exclusive arena.
pub fn reserve_os_memory(
    size: usize,
    commit: bool,
    allow_large: bool,
    exclusive: bool,
) -> Result<ArenaId, ArenaError> {
    let mut id = core::ptr::null_mut();
    let rc = unsafe {
        rustfs_mimalloc_sys::mi_reserve_os_memory_ex(size, commit, allow_large, exclusive, &mut id)
    };
    if rc == 0 {
        Ok(ArenaId(id))
    } else {
        Err(ArenaError::Failed)
    }
}

/// Manage an existing memory region as an arena.
///
/// # Safety
/// `start` must point to at least `size` valid bytes that outlive the arena.
pub unsafe fn manage_os_memory(
    start: *mut u8,
    size: usize,
    is_committed: bool,
    is_pinned: bool,
    is_zero: bool,
    numa_node: i32,
    exclusive: bool,
) -> Result<ArenaId, ArenaError> {
    let mut id = core::ptr::null_mut();
    let ok = unsafe {
        rustfs_mimalloc_sys::mi_manage_os_memory_ex(
            start as *mut c_void,
            size,
            is_committed,
            is_pinned,
            is_zero,
            numa_node,
            exclusive,
            &mut id,
        )
    };
    if ok {
        Ok(ArenaId(id))
    } else {
        Err(ArenaError::Failed)
    }
}

/// Minimum alignment for arena allocations.
#[inline]
pub fn arena_min_alignment() -> usize {
    unsafe { rustfs_mimalloc_sys::mi_arena_min_alignment() }
}

/// Minimum size for arena allocations.
#[inline]
pub fn arena_min_size() -> usize {
    unsafe { rustfs_mimalloc_sys::mi_arena_min_size() }
}

/// Maximum object size for arena allocations.
#[inline]
pub fn arena_max_object_size() -> usize {
    unsafe { rustfs_mimalloc_sys::mi_arena_max_object_size() }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_create_delete() {
        let heap = Heap::new().expect("heap::new failed");
        heap.delete();
    }

    #[test]
    fn cached_views_keep_alternating_heaps_and_alignment_distinct() {
        let first = Heap::new().unwrap();
        let second = Heap::new().unwrap();
        let first_local = first.thread_local().unwrap();
        let second_local = second.thread_local().unwrap();
        for size in [1, 64, 4096] {
            for alignment in [1, 8, 64, 4096, 16 * 1024] {
                for (heap, local) in [(&first, &first_local), (&second, &second_local)] {
                    unsafe {
                        let ptr = local.malloc_aligned(size, alignment);
                        assert!(!ptr.is_null());
                        assert_eq!(ptr as usize % alignment, 0);
                        assert!(heap.contains(ptr));
                        core::ptr::write_bytes(ptr, 0xA5, size);
                        rustfs_mimalloc_sys::mi_free(ptr.cast());
                        let ptr = local.zalloc_aligned(size, alignment);
                        assert!(!ptr.is_null());
                        assert_eq!(ptr as usize % alignment, 0);
                        assert!(heap.contains(ptr));
                        assert!((0..size).all(|index| *ptr.add(index) == 0));
                        rustfs_mimalloc_sys::mi_free(ptr.cast());
                    }
                }
            }
        }
    }

    #[test]
    fn cached_heap_allocations_can_be_freed_on_another_thread() {
        let heap = Heap::new().unwrap();
        let local = heap.thread_local().unwrap();
        for _ in 0..8 {
            let mut addresses = Vec::with_capacity(256);
            for _ in 0..256 {
                let ptr = unsafe { local.malloc(4096) };
                assert!(!ptr.is_null());
                unsafe { core::ptr::write_bytes(ptr, 0x5A, 4096) };
                addresses.push(ptr as usize);
            }
            std::thread::spawn(move || {
                for address in addresses {
                    // Exclusive allocation ownership is transferred to the worker;
                    // the allocating thread and parent heap stay alive until join.
                    unsafe {
                        let ptr = address as *mut u8;
                        assert_eq!(*ptr, 0x5A);
                        assert_eq!(*ptr.add(4095), 0x5A);
                        rustfs_mimalloc_sys::mi_free(ptr.cast());
                    }
                }
            })
            .join()
            .unwrap();
        }
        unsafe {
            let ptr = local.zalloc(128);
            assert!(!ptr.is_null());
            assert!((0..128).all(|index| *ptr.add(index) == 0));
            rustfs_mimalloc_sys::mi_free(ptr.cast());
        }
    }

    #[test]
    fn cached_allocations_survive_view_and_parent_deletion() {
        let heap = Heap::new().unwrap();
        let ptr = {
            let local = heap.thread_local().unwrap();
            let ptr = unsafe { local.malloc(128) };
            assert!(!ptr.is_null());
            unsafe { core::ptr::write_bytes(ptr, 0xAB, 128) };
            ptr
        };
        heap.delete(); // moves live allocations to the main heap
        unsafe {
            assert!((0..128).all(|index| *ptr.add(index) == 0xAB));
            rustfs_mimalloc_sys::mi_free(ptr.cast());
        }
    }

    #[test]
    fn heap_alloc_free() {
        let heap = Heap::new().unwrap();
        unsafe {
            let ptr = heap.malloc(128);
            assert!(!ptr.is_null());
            core::ptr::write_bytes(ptr, 0xCD, 128);
            rustfs_mimalloc_sys::mi_free(ptr as *mut c_void);
        }
        heap.delete();
    }

    #[test]
    fn heap_aligned_alloc() {
        let heap = Heap::new().unwrap();
        unsafe {
            for pow in 0..=12 {
                let align = 1usize << pow;
                let ptr = heap.malloc_aligned(64, align);
                assert!(!ptr.is_null());
                assert_eq!(ptr as usize % align, 0, "align={align}");
                rustfs_mimalloc_sys::mi_free(ptr as *mut c_void);
            }
        }
        heap.delete();
    }

    #[test]
    fn heap_aligned_zero_and_realloc_preserve_data() {
        let heap = Heap::new().unwrap();
        unsafe {
            for alignment in [8, 64, 4096, 16 * 1024] {
                let ptr = heap.zalloc_aligned(64, alignment);
                assert!(!ptr.is_null());
                assert_eq!(ptr as usize % alignment, 0);
                assert!((0..64).all(|i| *ptr.add(i) == 0));
                core::ptr::write_bytes(ptr, 0xA5, 64);
                let ptr = heap.realloc_aligned(ptr, 4096, alignment);
                assert!(!ptr.is_null());
                assert_eq!(ptr as usize % alignment, 0);
                assert!((0..64).all(|i| *ptr.add(i) == 0xA5));
                rustfs_mimalloc_sys::mi_free(ptr.cast());
            }
        }
    }

    #[test]
    fn arena_min_values_are_sane() {
        let a = arena_min_alignment();
        assert!(a > 0 && a.is_power_of_two());
    }

    #[test]
    fn heap_stats_are_available() {
        let heap = Heap::new().unwrap();
        assert!(!heap.stats_json().is_empty());
        assert!(!heap.stats_print().is_empty());
        heap.delete();
    }

    #[test]
    fn borrowed_main_heap_delete_is_noop() {
        let heap = Heap::main();
        heap.delete();

        unsafe {
            let ptr = rustfs_mimalloc_sys::mi_malloc(64);
            assert!(!ptr.is_null());
            rustfs_mimalloc_sys::mi_free(ptr);
        }
    }
}
