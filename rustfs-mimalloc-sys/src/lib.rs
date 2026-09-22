//! Low-level FFI bindings to [mimalloc](https://github.com/microsoft/mimalloc) V3 (v3.5.3).
//!
//! For a safe wrapper, use the `rustfs-mimalloc` crate.

#![no_std]
#![allow(non_camel_case_types)]

// ── Type aliases ────────────────────────────────────────────────────────────

pub use core::ffi::{c_char, c_int, c_long, c_void};

pub type size_t = usize;

// ── Constants ──────────────────────────────────────────────────────────────

/// Maximum word count for mimalloc's small allocation fast path.
pub const MI_SMALL_WSIZE_MAX: size_t = 128;

/// Maximum byte size for mimalloc's small allocation fast path.
pub const MI_SMALL_SIZE_MAX: size_t = MI_SMALL_WSIZE_MAX * core::mem::size_of::<size_t>();

/// Maximum user data bytes stored inline with a sampled profiling allocation.
pub const MI_PROFILE_SAMPLE_DATA_MAX_SIZE: size_t = 1024;

// ── Opaque types ────────────────────────────────────────────────────────────

#[repr(C)]
pub struct mi_heap_t {
    _opaque: [u8; 0],
}

#[repr(C)]
pub struct mi_theap_t {
    _opaque: [u8; 0],
}

/// Subprocess identifier. Opaque handle — do not access fields directly.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mi_subproc_id_t {
    _id: *mut c_void,
}

/// Arena identifier. Opaque handle.
pub type mi_arena_id_t = *mut c_void;

// ── Option enum ─────────────────────────────────────────────────────────────
//
// Kept in sync with mimalloc V3.5.3 `mi_option_e` in `mimalloc.h`.

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum mi_option_t {
    mi_option_show_errors = 0,
    mi_option_show_stats = 1,
    mi_option_verbose = 2,
    mi_option_arena_eager_commit = 4,
    mi_option_purge_decommits = 5,
    mi_option_allow_large_os_pages = 6,
    mi_option_reserve_huge_os_pages = 7,
    mi_option_reserve_huge_os_pages_at = 8,
    mi_option_reserve_os_memory = 9,
    mi_option_purge_delay = 15,
    mi_option_use_numa_nodes = 16,
    mi_option_disallow_os_alloc = 17,
    mi_option_os_tag = 18,
    mi_option_max_errors = 19,
    mi_option_max_warnings = 20,
    mi_option_destroy_on_exit = 22,
    mi_option_arena_reserve = 23,
    mi_option_arena_purge_mult = 24,
    mi_option_disallow_arena_alloc = 26,
    mi_option_retry_on_oom = 27,
    mi_option_guarded_min = 29,
    mi_option_guarded_max = 30,
    mi_option_guarded_precise = 31,
    mi_option_guarded_sample_rate = 32,
    mi_option_guarded_sample_seed = 33,
    mi_option_generic_collect = 34,
    mi_option_page_reclaim_on_free = 35,
    mi_option_page_full_retain = 36,
    mi_option_page_max_candidates = 37,
    mi_option_max_vabits = 38,
    mi_option_pagemap_commit = 39,
    mi_option_page_commit_on_demand = 40,
    mi_option_page_max_reclaim = 41,
    mi_option_page_cross_thread_max_reclaim = 42,
    mi_option_allow_thp = 43,
    mi_option_minimal_purge_size = 44,
    mi_option_arena_max_object_size = 45,
    mi_option_arena_is_numa_local = 46,
    mi_option_collect_merges_stats = 47,
}

// ── Heap area (for visiting blocks) ─────────────────────────────────────────

#[repr(C)]
pub struct mi_heap_area_t {
    pub blocks: *mut c_void,
    pub reserved: size_t,
    pub committed: size_t,
    pub used: size_t,
    pub block_size: size_t,
    pub full_block_size: size_t,
    pub reserved1: *mut c_void,
}

// ── Callback types ──────────────────────────────────────────────────────────

pub type mi_output_fun = unsafe extern "C" fn(msg: *const c_char, arg: *mut c_void);
pub type mi_error_fun = unsafe extern "C" fn(err: c_int, arg: *mut c_void);
pub type mi_deferred_free_fun = unsafe extern "C" fn(force: bool, heartbeat: u64, arg: *mut c_void);
pub type mi_profiler_on_alloc_fun = unsafe extern "C" fn(
    profiler: *mut mi_profiler_t,
    profiler_data: *mut mi_profiler_sample_data_t,
    ptr: *mut c_void,
    requested_size: size_t,
    bytes_sample_rate: size_t,
    bytes_since_last_sample: u64,
    heap: *const mi_heap_t,
) -> size_t;
pub type mi_profiler_on_realloc_inplace_fun = unsafe extern "C" fn(
    profiler: *mut mi_profiler_t,
    profiler_data: *mut mi_profiler_sample_data_t,
    ptr: *mut c_void,
    old_size: size_t,
    heap: *const mi_heap_t,
) -> size_t;
pub type mi_profiler_on_free_fun = unsafe extern "C" fn(
    profiler: *mut mi_profiler_t,
    profiler_data: *mut mi_profiler_sample_data_t,
    ptr: *mut c_void,
    heap: *const mi_heap_t,
);
pub type mi_block_visit_fun = unsafe extern "C" fn(
    heap: *const mi_heap_t,
    area: *const mi_heap_area_t,
    block: *mut c_void,
    block_size: size_t,
    arg: *mut c_void,
) -> bool;
pub type mi_heap_visit_fun = unsafe extern "C" fn(heap: *mut mi_heap_t, arg: *mut c_void) -> bool;

// ── Profiling ───────────────────────────────────────────────────────────────

#[repr(C)]
pub struct mi_profiler_sample_data_t {
    pub user_data_size: size_t,
    pub user_data: [*mut c_void; 1],
}

/// Experimental mimalloc profiling hook table.
#[repr(C)]
pub struct mi_profiler_t {
    pub reserved: *mut c_void,
    pub sample_data_size: size_t,
    pub initial_sample_rate: size_t,
    pub on_alloc: Option<mi_profiler_on_alloc_fun>,
    pub on_free: Option<mi_profiler_on_free_fun>,
    pub on_realloc_inplace: Option<mi_profiler_on_realloc_inplace_fun>,
}

// ── Standard malloc interface ───────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_malloc(size: size_t) -> *mut c_void;
    pub fn mi_calloc(count: size_t, size: size_t) -> *mut c_void;
    pub fn mi_realloc(p: *mut c_void, newsize: size_t) -> *mut c_void;
    pub fn mi_free(p: *mut c_void);
    pub fn mi_strdup(s: *const c_char) -> *mut c_char;
}

// ── Extended allocation ─────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_malloc_small(size: size_t) -> *mut c_void;
    pub fn mi_zalloc_small(size: size_t) -> *mut c_void;
    pub fn mi_wmalloc_small(wsize: size_t) -> *mut c_void;
    pub fn mi_wzalloc_small(wsize: size_t) -> *mut c_void;
    pub fn mi_zalloc(size: size_t) -> *mut c_void;
    pub fn mi_mallocn(count: size_t, size: size_t) -> *mut c_void;
    pub fn mi_reallocn(p: *mut c_void, count: size_t, size: size_t) -> *mut c_void;
    pub fn mi_usable_size(p: *const c_void) -> size_t;
    pub fn mi_good_size(size: size_t) -> size_t;
    pub fn mi_free_size(p: *mut c_void, size: size_t);
    pub fn mi_free_small(p: *mut c_void);
    pub fn mi_free_small_nonnull(p: *mut c_void);
}

/// Convert a byte size to a mimalloc machine-word count.
#[inline]
pub const fn mi_wsize_from_size(size: size_t) -> size_t {
    size.div_ceil(core::mem::size_of::<size_t>())
}

/// Allocate when the size is statically known by the caller.
///
/// This mirrors mimalloc's inline `mi_malloc_csize` helper.
///
/// # Safety
/// The returned pointer must be checked for null and freed with a compatible
/// mimalloc free API. The caller is responsible for honoring raw allocation
/// pointer aliasing and lifetime rules.
#[inline]
pub unsafe fn mi_malloc_csize(size: size_t) -> *mut c_void {
    if size <= MI_SMALL_SIZE_MAX {
        unsafe { mi_wmalloc_small(mi_wsize_from_size(size)) }
    } else {
        unsafe { mi_malloc(size) }
    }
}

/// Allocate zeroed memory when the size is statically known by the caller.
///
/// This mirrors mimalloc's inline `mi_zalloc_csize` helper.
///
/// # Safety
/// The returned pointer must be checked for null and freed with a compatible
/// mimalloc free API. The caller is responsible for honoring raw allocation
/// pointer aliasing and lifetime rules.
#[inline]
pub unsafe fn mi_zalloc_csize(size: size_t) -> *mut c_void {
    if size <= MI_SMALL_SIZE_MAX {
        unsafe { mi_wzalloc_small(mi_wsize_from_size(size)) }
    } else {
        unsafe { mi_zalloc(size) }
    }
}

/// Free an allocation when the size is statically known by the caller.
///
/// This mirrors mimalloc's inline `mi_free_csize` helper.
///
/// # Safety
///
/// `p` must be null or a valid mimalloc allocation, and `size` must be the
/// allocation size used for the corresponding allocation.
#[inline]
pub unsafe fn mi_free_csize(p: *mut c_void, size: size_t) {
    if size <= MI_SMALL_SIZE_MAX {
        unsafe { mi_free_small(p) };
    } else {
        unsafe { mi_free(p) };
    }
}

/// Free a non-null allocation when the size is statically known by the caller.
///
/// This mirrors mimalloc's inline `mi_free_csize_nonnull` helper.
///
/// # Safety
///
/// `p` must be a non-null valid mimalloc allocation, and `size` must be the
/// allocation size used for the corresponding allocation.
#[inline]
pub unsafe fn mi_free_csize_nonnull(p: *mut c_void, size: size_t) {
    if size <= MI_SMALL_SIZE_MAX {
        unsafe { mi_free_small_nonnull(p) };
    } else {
        unsafe { mi_free(p) };
    }
}

/// Free an allocation when its size and alignment are statically known.
///
/// This mirrors mimalloc's inline `mi_free_csize_aligned` helper. Over-aligned
/// small allocations must use the general free path, which this helper selects
/// when `alignment > size`.
///
/// # Safety
/// `p` must be null or a valid mimalloc allocation. `size` and `alignment`
/// must match the corresponding allocation contract.
#[inline]
pub unsafe fn mi_free_csize_aligned(p: *mut c_void, size: size_t, alignment: size_t) {
    if alignment <= size && size <= MI_SMALL_SIZE_MAX {
        unsafe { mi_free_small(p) };
    } else {
        unsafe { mi_free(p) };
    }
}

/// Free a non-null allocation when its size and alignment are statically known.
///
/// This mirrors mimalloc's inline `mi_free_csize_aligned_nonnull` helper.
///
/// # Safety
/// `p` must be a non-null valid mimalloc allocation. `size` and `alignment`
/// must match the corresponding allocation contract.
#[inline]
pub unsafe fn mi_free_csize_aligned_nonnull(p: *mut c_void, size: size_t, alignment: size_t) {
    if alignment <= size && size <= MI_SMALL_SIZE_MAX {
        unsafe { mi_free_small_nonnull(p) };
    } else {
        unsafe { mi_free(p) };
    }
}

// ── Aligned allocation ──────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_malloc_aligned(size: size_t, alignment: size_t) -> *mut c_void;
    pub fn mi_zalloc_aligned(size: size_t, alignment: size_t) -> *mut c_void;
    pub fn mi_calloc_aligned(count: size_t, size: size_t, alignment: size_t) -> *mut c_void;
    pub fn mi_realloc_aligned(p: *mut c_void, newsize: size_t, alignment: size_t) -> *mut c_void;
}

// ── Process & thread lifecycle ──────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_collect(force: bool);
    pub fn mi_thread_set_in_threadpool();
    pub fn mi_version() -> c_int;
    pub fn mi_process_info_print_out(out: Option<mi_output_fun>, arg: *mut c_void);
    pub fn mi_process_info(
        elapsed_msecs: *mut size_t,
        user_msecs: *mut size_t,
        system_msecs: *mut size_t,
        current_rss: *mut size_t,
        peak_rss: *mut size_t,
        current_commit: *mut size_t,
        peak_commit: *mut size_t,
        page_faults: *mut size_t,
    );
}

// ── Heaps ───────────────────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_heap_new() -> *mut mi_heap_t;
    pub fn mi_heap_delete(heap: *mut mi_heap_t);
    pub fn mi_heap_destroy(heap: *mut mi_heap_t);
    pub fn mi_heap_collect(heap: *mut mi_heap_t, force: bool);
    pub fn mi_heap_main() -> *mut mi_heap_t;
    pub fn mi_heap_of(p: *const c_void) -> *mut mi_heap_t;
    pub fn mi_heap_contains(heap: *const mi_heap_t, p: *const c_void) -> bool;
    pub fn mi_heap_theap(heap: *mut mi_heap_t) -> *mut mi_theap_t;

    pub fn mi_heap_malloc(heap: *mut mi_heap_t, size: size_t) -> *mut c_void;
    pub fn mi_heap_zalloc(heap: *mut mi_heap_t, size: size_t) -> *mut c_void;
    pub fn mi_heap_calloc(heap: *mut mi_heap_t, count: size_t, size: size_t) -> *mut c_void;
    pub fn mi_heap_realloc(heap: *mut mi_heap_t, p: *mut c_void, newsize: size_t) -> *mut c_void;
    pub fn mi_heap_malloc_aligned(
        heap: *mut mi_heap_t,
        size: size_t,
        alignment: size_t,
    ) -> *mut c_void;
}

// ── Thread-local heaps ──────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_theap_malloc(theap: *mut mi_theap_t, size: size_t) -> *mut c_void;
    pub fn mi_theap_zalloc(theap: *mut mi_theap_t, size: size_t) -> *mut c_void;
    pub fn mi_theap_malloc_small(theap: *mut mi_theap_t, size: size_t) -> *mut c_void;
    pub fn mi_theap_zalloc_small(theap: *mut mi_theap_t, size: size_t) -> *mut c_void;
    pub fn mi_theap_wmalloc_small(theap: *mut mi_theap_t, wsize: size_t) -> *mut c_void;
    pub fn mi_theap_wzalloc_small(theap: *mut mi_theap_t, wsize: size_t) -> *mut c_void;

    /// C++-semantics allocation APIs. An installed C++ new-handler may throw;
    /// callers must not allow foreign exceptions to cross into Rust.
    pub fn mi_theap_alloc_new(theap: *mut mi_theap_t, size: size_t) -> *mut c_void;
    pub fn mi_theap_alloc_new_n(theap: *mut mi_theap_t, count: size_t, size: size_t)
    -> *mut c_void;
    pub fn mi_theap_alloc_new_nothrow(theap: *mut mi_theap_t, size: size_t) -> *mut c_void;
}

/// Allocate from a thread-local heap when the size is statically known.
///
/// This mirrors mimalloc's inline `mi_theap_malloc_csize` helper.
///
/// # Safety
/// `theap` must be non-null and valid for the calling thread. The returned
/// pointer must be checked for null and freed with a compatible mimalloc free
/// API.
#[inline]
pub unsafe fn mi_theap_malloc_csize(theap: *mut mi_theap_t, size: size_t) -> *mut c_void {
    if size <= MI_SMALL_SIZE_MAX {
        unsafe { mi_theap_wmalloc_small(theap, mi_wsize_from_size(size)) }
    } else {
        unsafe { mi_theap_malloc(theap, size) }
    }
}

/// Allocate zeroed memory from a thread-local heap when the size is statically known.
///
/// This mirrors mimalloc's inline `mi_theap_zalloc_csize` helper.
///
/// # Safety
/// `theap` must be non-null and valid for the calling thread. The returned
/// pointer must be checked for null and freed with a compatible mimalloc free
/// API.
#[inline]
pub unsafe fn mi_theap_zalloc_csize(theap: *mut mi_theap_t, size: size_t) -> *mut c_void {
    if size <= MI_SMALL_SIZE_MAX {
        unsafe { mi_theap_wzalloc_small(theap, mi_wsize_from_size(size)) }
    } else {
        unsafe { mi_theap_zalloc(theap, size) }
    }
}

// ── Arena management ────────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_reserve_os_memory_ex(
        size: size_t,
        commit: bool,
        allow_large: bool,
        exclusive: bool,
        arena_id: *mut mi_arena_id_t,
    ) -> c_int;
    pub fn mi_manage_os_memory_ex(
        start: *mut c_void,
        size: size_t,
        is_committed: bool,
        is_pinned: bool,
        is_zero: bool,
        numa_node: c_int,
        exclusive: bool,
        arena_id: *mut mi_arena_id_t,
    ) -> bool;
    pub fn mi_arena_min_alignment() -> size_t;
    pub fn mi_arena_min_size() -> size_t;
    pub fn mi_arena_max_object_size() -> size_t;
    pub fn mi_heap_new_in_arena(arena_id: mi_arena_id_t) -> *mut mi_heap_t;
}

// ── Options ─────────────────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_option_is_enabled(option: mi_option_t) -> bool;
    pub fn mi_option_enable(option: mi_option_t);
    pub fn mi_option_disable(option: mi_option_t);
    pub fn mi_option_get(option: mi_option_t) -> c_long;
    pub fn mi_option_get_size(option: mi_option_t) -> size_t;
    pub fn mi_option_set(option: mi_option_t, value: c_long);
}

// ── Experimental profiling ─────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_heap_profile(heap: *mut mi_heap_t, profiler: *mut mi_profiler_t) -> bool;
    pub fn mi_heap_profile_disable(heap: *mut mi_heap_t);
    pub fn mi_subproc_profile(subproc_id: mi_subproc_id_t, profiler: *mut mi_profiler_t) -> bool;
    pub fn mi_profile(profiler: *mut mi_profiler_t) -> bool;
    pub fn mi_profiler_start(profiler: *mut mi_profiler_t) -> bool;
    pub fn mi_profiler_stop(profiler: *mut mi_profiler_t) -> bool;
}

// ── POSIX-compatible ────────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_posix_memalign(p: *mut *mut c_void, alignment: size_t, size: size_t) -> c_int;
    pub fn mi_memalign(alignment: size_t, size: size_t) -> *mut c_void;
    pub fn mi_malloc_size(p: *const c_void) -> size_t;
    pub fn mi_malloc_usable_size(p: *const c_void) -> size_t;
}

// ── Statistics ──────────────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn mi_stats_get_json(buf_size: size_t, buf: *mut c_char) -> *mut c_char;
    pub fn mi_stats_print_out(out: Option<mi_output_fun>, arg: *mut c_void);
    pub fn mi_stats_reset();
    pub fn mi_heap_stats_get_json(
        heap: *mut mi_heap_t,
        buf_size: size_t,
        buf: *mut c_char,
    ) -> *mut c_char;
    pub fn mi_heap_stats_print_out(
        heap: *mut mi_heap_t,
        out: Option<mi_output_fun>,
        arg: *mut c_void,
    );
    pub fn mi_theap_stats_merge_to_heap(theap: *mut mi_theap_t);
}
