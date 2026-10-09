//! Stats, options, version, and process information APIs.

use crate::MiMalloc;
use core::ffi::c_void;
use core::ptr::NonNull;

/// Mark the current thread as part of a thread pool for mimalloc.
///
/// This is a safe wrapper around mimalloc V3's `mi_thread_set_in_threadpool`.
/// The upstream API takes no pointers, only updates the current thread's
/// mimalloc thread-local state, and is intended to be called by custom
/// thread-pool worker threads. Repeated calls keep the same threadpool marker.
#[inline]
pub fn set_current_thread_in_threadpool() {
    unsafe { rustfs_mimalloc_sys::mi_thread_set_in_threadpool() }
}

/// Process memory information returned by [`MiMalloc::process_info`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessInfo {
    pub elapsed_msecs: usize,
    pub user_msecs: usize,
    pub system_msecs: usize,
    pub current_rss: usize,
    pub peak_rss: usize,
    pub current_commit: usize,
    pub peak_commit: usize,
    pub page_faults: usize,
}

impl MiMalloc {
    /// Whether the linked allocator supports allocation profiling.
    ///
    /// Returns false with the sys crate's `no_profile` feature, including when
    /// another dependency enables it through Cargo feature unification.
    /// Statistics, error/output callbacks, and security checks are independent.
    #[inline]
    pub const fn profiling_enabled() -> bool {
        rustfs_mimalloc_sys::MI_PROFILE_ENABLED
    }

    /// mimalloc version as `major * 10000 + minor * 100 + patch`.
    #[inline]
    pub fn version() -> i32 {
        unsafe { rustfs_mimalloc_sys::mi_version() }
    }

    /// Force garbage collection.
    #[inline]
    pub fn collect(force: bool) {
        unsafe { rustfs_mimalloc_sys::mi_collect(force) }
    }

    /// Immediately purge pending arena ranges in the main subprocess.
    ///
    /// This can issue operating-system memory calls and may add latency. It is
    /// useful for idle services that need to release delayed arena purges.
    #[inline]
    pub fn arenas_purge() {
        unsafe { rustfs_mimalloc_sys::mi_arenas_purge() }
    }

    /// Usable size of an allocated block (may be larger than requested).
    ///
    /// # Safety
    /// `ptr` must have been allocated by mimalloc.
    #[inline]
    pub unsafe fn usable_size(ptr: *const u8) -> usize {
        unsafe { rustfs_mimalloc_sys::mi_usable_size(ptr as *const c_void) }
    }

    /// Allocation size class for a requested byte count, useful for capacity planning.
    /// This is a hint; it does not allocate memory or increase an existing allocation.
    #[inline]
    pub fn good_size(size: usize) -> usize {
        unsafe { rustfs_mimalloc_sys::mi_good_size(size) }
    }

    /// Convert a byte size to a mimalloc machine-word count.
    ///
    /// This mirrors mimalloc's `mi_wsize_from_size` helper for the word-size
    /// small allocation fast paths.
    #[inline]
    pub const fn wsize_from_size(size: usize) -> usize {
        rustfs_mimalloc_sys::mi_wsize_from_size(size)
    }

    /// Allocate a mimalloc block when the allocation size is known.
    ///
    /// For small sizes this uses mimalloc's word-size small-allocation fast path.
    ///
    /// # Safety
    /// The returned raw pointer must be checked for null and eventually freed
    /// with a compatible mimalloc free API.
    #[inline]
    pub unsafe fn malloc_csize(size: usize) -> *mut u8 {
        unsafe { rustfs_mimalloc_sys::mi_malloc_csize(size) as *mut u8 }
    }

    /// Allocate a zeroed mimalloc block when the allocation size is known.
    ///
    /// For small sizes this uses mimalloc's word-size small-allocation fast path.
    ///
    /// # Safety
    /// The returned raw pointer must be checked for null and eventually freed
    /// with a compatible mimalloc free API.
    #[inline]
    pub unsafe fn zalloc_csize(size: usize) -> *mut u8 {
        unsafe { rustfs_mimalloc_sys::mi_zalloc_csize(size) as *mut u8 }
    }

    /// Allocate a small block by machine-word count.
    ///
    /// # Safety
    /// `wsize` must not exceed [`crate::MI_SMALL_WSIZE_MAX`] and is measured
    /// in `usize` machine words, not bytes. The returned raw
    /// pointer must be checked for null and eventually freed with a compatible
    /// mimalloc free API.
    #[inline]
    pub unsafe fn wmalloc_small(wsize: usize) -> *mut u8 {
        unsafe { rustfs_mimalloc_sys::mi_wmalloc_small(wsize) as *mut u8 }
    }

    /// Allocate a zeroed small block by machine-word count.
    ///
    /// # Safety
    /// `wsize` must not exceed [`crate::MI_SMALL_WSIZE_MAX`] and is measured
    /// in `usize` machine words, not bytes. The returned raw
    /// pointer must be checked for null and eventually freed with a compatible
    /// mimalloc free API.
    #[inline]
    pub unsafe fn wzalloc_small(wsize: usize) -> *mut u8 {
        unsafe { rustfs_mimalloc_sys::mi_wzalloc_small(wsize) as *mut u8 }
    }

    /// Free a mimalloc block when the allocation size is known.
    ///
    /// For small sizes this uses mimalloc's small-free fast path.
    ///
    /// # Safety
    /// `ptr` must be null or a valid mimalloc allocation, and `size` must be
    /// the allocation size used for the corresponding allocation. For aligned
    /// allocations use [`Self::free_csize_aligned`] instead.
    #[inline]
    pub unsafe fn free_csize(ptr: *mut u8, size: usize) {
        unsafe { rustfs_mimalloc_sys::mi_free_csize(ptr as *mut c_void, size) }
    }

    /// Free a non-null mimalloc block when the allocation size is known.
    ///
    /// For small sizes this uses mimalloc's non-null small-free fast path.
    ///
    /// # Safety
    /// `ptr` must be a valid mimalloc allocation, and `size` must be the
    /// allocation size used for the corresponding allocation. For aligned
    /// allocations use [`Self::free_csize_aligned_nonnull`] instead.
    #[inline]
    pub unsafe fn free_csize_nonnull(ptr: NonNull<u8>, size: usize) {
        unsafe { rustfs_mimalloc_sys::mi_free_csize_nonnull(ptr.as_ptr() as *mut c_void, size) }
    }

    /// Free a mimalloc block when its allocation size and alignment are known.
    ///
    /// For non-over-aligned small allocations this uses mimalloc's small-free
    /// fast path. Over-aligned allocations use the general free path.
    ///
    /// # Safety
    /// `ptr` must be null or a valid mimalloc allocation. `size` and
    /// `alignment` must match the corresponding allocation contract.
    #[inline]
    pub unsafe fn free_csize_aligned(ptr: *mut u8, size: usize, alignment: usize) {
        unsafe { rustfs_mimalloc_sys::mi_free_csize_aligned(ptr as *mut c_void, size, alignment) }
    }

    /// Free a non-null mimalloc block when its allocation size and alignment are known.
    ///
    /// # Safety
    /// `ptr` must be a valid mimalloc allocation. `size` and `alignment` must
    /// match the corresponding allocation contract.
    #[inline]
    pub unsafe fn free_csize_aligned_nonnull(ptr: NonNull<u8>, size: usize, alignment: usize) {
        unsafe {
            rustfs_mimalloc_sys::mi_free_csize_aligned_nonnull(
                ptr.as_ptr() as *mut c_void,
                size,
                alignment,
            )
        }
    }

    /// Free a small mimalloc block.
    ///
    /// # Safety
    /// `ptr` must be null or a valid mimalloc allocation whose allocation size
    /// is less than or equal to [`crate::MI_SMALL_SIZE_MAX`], obtained through
    /// a small-allocation API. Over-aligned allocations require an aligned free.
    #[inline]
    pub unsafe fn free_small(ptr: *mut u8) {
        unsafe { rustfs_mimalloc_sys::mi_free_small(ptr as *mut c_void) }
    }

    /// Free a non-null small mimalloc block.
    ///
    /// # Safety
    /// `ptr` must be a valid mimalloc allocation whose allocation size is less
    /// than or equal to [`crate::MI_SMALL_SIZE_MAX`], obtained through a
    /// small-allocation API. Over-aligned allocations require an aligned free.
    #[inline]
    pub unsafe fn free_small_nonnull(ptr: NonNull<u8>) {
        unsafe { rustfs_mimalloc_sys::mi_free_small_nonnull(ptr.as_ptr() as *mut c_void) }
    }

    /// Free a small allocation from a page owned by the calling thread.
    ///
    /// # Safety
    /// `ptr` must come from a mimalloc small-allocation API and its page must
    /// still be owned by the calling thread. Allocation on this thread alone
    /// does not establish this: collection or heap deletion can abandon pages.
    /// Do not use this for arbitrary `GlobalAlloc` or cross-thread frees.
    #[inline]
    pub unsafe fn free_small_local_nonnull(ptr: NonNull<u8>) {
        unsafe { rustfs_mimalloc_sys::mi_free_small_local_nonnull(ptr.as_ptr().cast()) }
    }

    /// Free a local small allocation, accepting null as a no-op.
    ///
    /// # Safety
    /// A non-null `ptr` must meet [`Self::free_small_local_nonnull`]'s contract.
    #[inline]
    pub unsafe fn free_small_local(ptr: *mut u8) {
        // Upstream's debug build asserts non-null even for its nullable variant.
        if let Some(ptr) = NonNull::new(ptr) {
            unsafe { Self::free_small_local_nonnull(ptr) }
        }
    }

    /// Process memory information.
    pub fn process_info() -> ProcessInfo {
        let mut info = ProcessInfo::default();
        unsafe {
            rustfs_mimalloc_sys::mi_process_info(
                &mut info.elapsed_msecs,
                &mut info.user_msecs,
                &mut info.system_msecs,
                &mut info.current_rss,
                &mut info.peak_rss,
                &mut info.current_commit,
                &mut info.peak_commit,
                &mut info.page_faults,
            );
        }
        info
    }

    // ── Stats ───────────────────────────────────────────────────────────────

    /// Allocation statistics as JSON. Returns empty string on failure.
    pub fn stats_json() -> String {
        unsafe {
            crate::ffi::owned_mimalloc_string(rustfs_mimalloc_sys::mi_stats_get_json(
                0,
                core::ptr::null_mut(),
            ))
        }
    }

    /// Allocation statistics in mimalloc's human-readable text format.
    pub fn stats_print() -> String {
        crate::ffi::collect_mimalloc_output(|out, arg| unsafe {
            rustfs_mimalloc_sys::mi_stats_print_out(out, arg);
        })
    }

    /// Reset accumulated mimalloc allocation statistics.
    #[inline]
    pub fn stats_reset() {
        unsafe { rustfs_mimalloc_sys::mi_stats_reset() }
    }

    /// Process memory information in mimalloc's human-readable text format.
    pub fn process_info_print() -> String {
        crate::ffi::collect_mimalloc_output(|out, arg| unsafe {
            rustfs_mimalloc_sys::mi_process_info_print_out(out, arg);
        })
    }

    // ── Options ─────────────────────────────────────────────────────────────

    /// Check if an option is enabled.
    #[inline]
    pub fn option_is_enabled(option: rustfs_mimalloc_sys::mi_option_t) -> bool {
        unsafe { rustfs_mimalloc_sys::mi_option_is_enabled(option) }
    }

    /// Get an option value.
    #[inline]
    pub fn option_get(option: rustfs_mimalloc_sys::mi_option_t) -> rustfs_mimalloc_sys::c_long {
        unsafe { rustfs_mimalloc_sys::mi_option_get(option) }
    }

    /// Get an option value as size (bytes).
    #[inline]
    pub fn option_get_size(option: rustfs_mimalloc_sys::mi_option_t) -> usize {
        unsafe { rustfs_mimalloc_sys::mi_option_get_size(option) }
    }

    /// Set an option value.
    ///
    /// ```rust
    /// use rustfs_mimalloc::MiMalloc;
    /// use rustfs_mimalloc_sys::mi_option_t;
    ///
    /// // Return memory to OS immediately
    /// // At startup, before other threads can access mimalloc.
    /// unsafe { MiMalloc::option_set(mi_option_t::mi_option_purge_delay, 0); }
    /// ```
    ///
    /// # Safety
    /// Upstream option storage is not atomic. Configure options before starting
    /// other threads, or otherwise exclude every concurrent mimalloc access.
    #[inline]
    pub unsafe fn option_set(
        option: rustfs_mimalloc_sys::mi_option_t,
        value: rustfs_mimalloc_sys::c_long,
    ) {
        unsafe { rustfs_mimalloc_sys::mi_option_set(option, value) }
    }

    /// Enable an option.
    ///
    /// # Safety
    /// Upstream option storage is not atomic. Configure options before starting
    /// other threads, or otherwise exclude every concurrent mimalloc access.
    #[inline]
    pub unsafe fn option_enable(option: rustfs_mimalloc_sys::mi_option_t) {
        unsafe { rustfs_mimalloc_sys::mi_option_enable(option) }
    }

    /// Disable an option.
    ///
    /// # Safety
    /// Upstream option storage is not atomic. Configure options before starting
    /// other threads, or otherwise exclude every concurrent mimalloc access.
    #[inline]
    pub unsafe fn option_disable(option: rustfs_mimalloc_sys::mi_option_t) {
        unsafe { rustfs_mimalloc_sys::mi_option_disable(option) }
    }

    /// Set a fallback value without overriding an environment or explicit setting.
    ///
    /// # Safety
    /// Upstream option storage is not atomic. Configure options before starting
    /// other threads, or otherwise exclude every concurrent mimalloc access.
    #[inline]
    pub unsafe fn option_set_default(
        option: rustfs_mimalloc_sys::mi_option_t,
        value: rustfs_mimalloc_sys::c_long,
    ) {
        unsafe { rustfs_mimalloc_sys::mi_option_set_default(option, value) }
    }

    /// Set whether an option is enabled.
    ///
    /// # Safety
    /// Upstream option storage is not atomic. Configure options before starting
    /// other threads, or otherwise exclude every concurrent mimalloc access.
    #[inline]
    pub unsafe fn option_set_enabled(option: rustfs_mimalloc_sys::mi_option_t, enabled: bool) {
        unsafe { rustfs_mimalloc_sys::mi_option_set_enabled(option, enabled) }
    }

    /// Set a fallback toggle without overriding an environment or explicit setting.
    ///
    /// # Safety
    /// Upstream option storage is not atomic. Configure options before starting
    /// other threads, or otherwise exclude every concurrent mimalloc access.
    #[inline]
    pub unsafe fn option_set_enabled_default(
        option: rustfs_mimalloc_sys::mi_option_t,
        enabled: bool,
    ) {
        unsafe { rustfs_mimalloc_sys::mi_option_set_enabled_default(option, enabled) }
    }

    /// Get an option clamped to the inclusive range. Requires `min <= max`.
    #[inline]
    pub fn option_get_clamp(
        option: rustfs_mimalloc_sys::mi_option_t,
        min: rustfs_mimalloc_sys::c_long,
        max: rustfs_mimalloc_sys::c_long,
    ) -> rustfs_mimalloc_sys::c_long {
        assert!(min <= max, "option clamp range is reversed");
        unsafe { rustfs_mimalloc_sys::mi_option_get_clamp(option, min, max) }
    }

    /// Runtime options in mimalloc's human-readable format.
    pub fn options_print() -> String {
        crate::ffi::collect_mimalloc_output(|out, arg| unsafe {
            rustfs_mimalloc_sys::mi_options_print_out(out, arg);
        })
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rustfs_mimalloc_sys::mi_option_t;

    #[test]
    fn version_is_v3() {
        assert_eq!(MiMalloc::version(), 30504, "expected V3.5.4");
    }

    #[test]
    fn local_small_free_and_null_roundtrip() {
        unsafe {
            MiMalloc::free_small_local(core::ptr::null_mut());
            // Free immediately without collection or any heap ownership change.
            for size in [1, 8, 64, crate::MI_SMALL_SIZE_MAX] {
                let ptr =
                    NonNull::new(rustfs_mimalloc_sys::mi_malloc_small(size).cast::<u8>()).unwrap();
                ptr.as_ptr().write(0xAB);
                MiMalloc::free_small_local_nonnull(ptr);
                let ptr = rustfs_mimalloc_sys::mi_malloc_small(size).cast::<u8>();
                assert!(!ptr.is_null());
                MiMalloc::free_small_local(ptr);
            }
        }
    }

    #[test]
    fn options_include_new_profiling_controls() {
        let text = MiMalloc::options_print();
        assert_eq!(
            text.contains("profiling   :"),
            MiMalloc::profiling_enabled()
        );
        if cfg!(feature = "secure") {
            assert!(text.contains("secure level: 4"));
        }
        if cfg!(feature = "debug") {
            assert!(text.contains("debug level : 2"));
        }
        for name in [
            "profile_alloc_interval",
            "profile_inuse_interval",
            "profile_time_interval",
            "profile_sample_rate",
        ] {
            assert!(text.contains(name), "missing {name}");
        }
        let rate = MiMalloc::option_get(mi_option_t::mi_option_profile_sample_rate);
        assert_eq!(
            MiMalloc::option_get_clamp(mi_option_t::mi_option_profile_sample_rate, 3, 8),
            rate.clamp(3, 8)
        );
        for size in [1, 8, 127, 1024, 4096] {
            assert!(MiMalloc::good_size(size) >= size);
        }
    }

    #[test]
    fn stats_json_not_empty() {
        let json = MiMalloc::stats_json();
        assert!(!json.is_empty());
    }

    #[test]
    fn stats_print_not_empty() {
        let stats = MiMalloc::stats_print();
        assert!(!stats.is_empty());
    }

    #[test]
    fn stats_reset_smoke() {
        MiMalloc::stats_reset();
    }

    #[test]
    fn process_info_print_not_empty() {
        let info = MiMalloc::process_info_print();
        assert!(!info.is_empty());
    }

    #[test]
    fn option_roundtrip() {
        // Just verify no panic
        let _ = MiMalloc::option_get(mi_option_t::mi_option_purge_delay);
        let _ = MiMalloc::option_is_enabled(mi_option_t::mi_option_show_errors);
    }

    #[test]
    fn process_info_smoke() {
        let info = MiMalloc::process_info();
        let _ = info;
    }

    #[test]
    fn set_current_thread_in_threadpool_smoke() {
        set_current_thread_in_threadpool();
        set_current_thread_in_threadpool();
    }

    #[test]
    fn usable_size_at_least_requested() {
        unsafe {
            let ptr = rustfs_mimalloc_sys::mi_malloc(64);
            assert!(MiMalloc::usable_size(ptr as *const u8) >= 64);
            rustfs_mimalloc_sys::mi_free(ptr);
        }
    }

    #[test]
    fn free_small_nonnull_smoke() {
        unsafe {
            let ptr = rustfs_mimalloc_sys::mi_malloc_small(64);
            let ptr = NonNull::new(ptr as *mut u8).expect("mi_malloc_small returned null");
            MiMalloc::free_small_nonnull(ptr);
        }
    }

    #[test]
    fn word_size_small_alloc_smoke() {
        unsafe {
            let wsize = MiMalloc::wsize_from_size(64);

            let ptr =
                NonNull::new(MiMalloc::wmalloc_small(wsize)).expect("wmalloc_small returned null");
            MiMalloc::free_small_nonnull(ptr);

            let zeroed =
                NonNull::new(MiMalloc::wzalloc_small(wsize)).expect("wzalloc_small returned null");
            assert!((0..64).all(|i| *zeroed.as_ptr().add(i) == 0));
            MiMalloc::free_small_nonnull(zeroed);
        }
    }

    #[test]
    fn csize_alloc_smoke() {
        unsafe {
            let ptr = NonNull::new(MiMalloc::malloc_csize(64)).expect("malloc_csize returned null");
            MiMalloc::free_csize_nonnull(ptr, 64);

            let zeroed =
                NonNull::new(MiMalloc::zalloc_csize(64)).expect("zalloc_csize returned null");
            assert!((0..64).all(|i| *zeroed.as_ptr().add(i) == 0));
            MiMalloc::free_csize_nonnull(zeroed, 64);
        }
    }

    #[test]
    fn sys_theap_csize_alloc_smoke() {
        unsafe {
            let heap = rustfs_mimalloc_sys::mi_heap_new();
            assert!(!heap.is_null(), "mi_heap_new returned null");

            let theap = rustfs_mimalloc_sys::mi_heap_theap(heap);
            assert!(!theap.is_null(), "mi_heap_theap returned null");

            let ptr =
                NonNull::new(rustfs_mimalloc_sys::mi_theap_malloc_csize(theap, 64) as *mut u8)
                    .expect("mi_theap_malloc_csize returned null");
            MiMalloc::free_csize_nonnull(ptr, 64);

            let zeroed =
                NonNull::new(rustfs_mimalloc_sys::mi_theap_zalloc_csize(theap, 64) as *mut u8)
                    .expect("mi_theap_zalloc_csize returned null");
            assert!((0..64).all(|i| *zeroed.as_ptr().add(i) == 0));
            MiMalloc::free_csize_nonnull(zeroed, 64);

            rustfs_mimalloc_sys::mi_heap_delete(heap);
        }
    }

    #[test]
    fn free_csize_routes_small_and_large() {
        unsafe {
            let small = rustfs_mimalloc_sys::mi_malloc_small(64);
            MiMalloc::free_csize(small as *mut u8, 64);

            let large_size = rustfs_mimalloc_sys::MI_SMALL_SIZE_MAX + 64;
            let large = rustfs_mimalloc_sys::mi_malloc(large_size);
            let large = NonNull::new(large as *mut u8).expect("mi_malloc returned null");
            MiMalloc::free_csize_nonnull(large, large_size);
        }
    }

    #[test]
    fn free_csize_aligned_routes_overaligned_small_allocations() {
        unsafe {
            let ptr = rustfs_mimalloc_sys::mi_malloc_aligned(8, 16 * 1024);
            let ptr = NonNull::new(ptr as *mut u8).expect("mi_malloc_aligned returned null");
            assert_eq!(ptr.as_ptr() as usize % (16 * 1024), 0);
            MiMalloc::free_csize_aligned_nonnull(ptr, 8, 16 * 1024);

            let ptr = rustfs_mimalloc_sys::mi_malloc_aligned(64, 8);
            assert!(!ptr.is_null(), "mi_malloc_aligned returned null");
            MiMalloc::free_csize_aligned(ptr as *mut u8, 64, 8);
        }
    }

    #[test]
    fn theap_alloc_new_ffi_symbols_are_available() {
        unsafe {
            let heap = rustfs_mimalloc_sys::mi_heap_new();
            assert!(!heap.is_null(), "mi_heap_new returned null");
            let theap = rustfs_mimalloc_sys::mi_heap_theap(heap);
            assert!(!theap.is_null(), "mi_heap_theap returned null");

            for ptr in [
                rustfs_mimalloc_sys::mi_theap_alloc_new(theap, 64),
                rustfs_mimalloc_sys::mi_theap_alloc_new_n(theap, 2, 32),
                rustfs_mimalloc_sys::mi_theap_alloc_new_nothrow(theap, 64),
            ] {
                assert!(!ptr.is_null(), "mi_theap_alloc_new* returned null");
                rustfs_mimalloc_sys::mi_free(ptr);
            }
            rustfs_mimalloc_sys::mi_heap_delete(heap);
        }
    }
}
