//! Low-level FFI bindings to [mimalloc](https://github.com/microsoft/mimalloc) V3.
//!
//! Pinned to upstream dev3 commit `5424c54c` (C version 30702).
//!
//! For a safe wrapper, use the `rustfs-mimalloc` crate.
//!
//! # Safety policy
//! Raw `extern "C"` declarations carry their safety requirements in prose on
//! the declarations whose contracts go beyond the obvious "live handle, null
//! checked" defaults (heap lifetime, callback lifetime, owner-thread
//! contexts, quiescence). Rules that apply uniformly across the whole
//! surface, and are not repeated per declaration:
//!
//! - Every opaque handle parameter must be a live object of the matching
//!   type (or null where a declaration says null is meaningful) for the
//!   whole call, and must not race concurrent destruction.
//! - Allocation results must be null-checked and freed through a compatible
//!   mimalloc free function; sizes/alignments passed to sized/aligned APIs
//!   must match the original allocation contract.
//! - Usable-size output pointers in the `mi_u*` family may be null. Otherwise
//!   they must point to writable size storage outside the allocation being
//!   freed or resized. Read allocation and reallocation outputs on success.
//!   Usable capacity follows [`mi_usable_size`], including padding and offsets.
//! - Thread-local context APIs (`mi_theap_*`) are owner-thread only unless a
//!   declaration documents otherwise.
//! - Visitor and callback functions must not unwind and must honor the
//!   re-entrancy constraints stated on the registering function.
//!
//! The `unsafe` wrapper functions in the `rustfs-mimalloc` crate document
//! the combined requirements for their wrapped entry points.
//!
//! The opt-in `no_profile` feature disables allocation/free sampling and
//! automatic `MIMALLOC_PROFILE` startup. Raw profiler control functions remain
//! linked, and explicitly requested snapshot callbacks still execute, but
//! allocations do not produce profiling samples. Statistics and security/debug
//! checks remain available. Check [`MI_PROFILE_ENABLED`] for the linked crate's mode.

#![no_std]
#![allow(non_camel_case_types)]

// ── Type aliases ────────────────────────────────────────────────────────────

pub use core::ffi::{c_char, c_int, c_long, c_void};

pub type size_t = usize;

// ── Constants ──────────────────────────────────────────────────────────────

/// Whether this build supports allocation profiling (independent of whether
/// a profiler has been enabled at runtime).
pub const MI_PROFILE_ENABLED: bool = !cfg!(feature = "no_profile");

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
// Kept in sync with the pinned `mi_option_e` in `mimalloc.h`.

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
    /// Legacy ID 31. The upstream runtime control is deprecated.
    /// Precise placement is selected with the native MI_UNSAFE_GUARDED_PRECISE build control.
    mi_option_guarded_precise = 31,
    mi_option_guarded_sample_rate = 32,
    mi_option_guarded_sample_seed = 33,
    mi_option_generic_collect = 34,
    /// Heap-creation reclaim-on-free policy: -1 disables, 0 restricts reclaim to
    /// the originating context (default), and 1 allows other contexts.
    mi_option_page_reclaim_on_free = 35,
    mi_option_page_full_retain = 36,
    mi_option_page_max_candidates = 37,
    mi_option_max_vabits = 38,
    mi_option_pagemap_commit = 39,
    mi_option_page_commit_on_demand = 40,
    mi_option_page_max_reclaim = 41,
    mi_option_page_cross_thread_max_reclaim = 42,
    /// Transparent huge pages: 0 disables, 1 allows, 2 also avoids splitting
    /// on purge. Linux defaults to 2, Android to 0. An explicit minimal purge
    /// size overrides the purge policy.
    mi_option_allow_thp = 43,
    mi_option_arena_purge_min_size = 44,
    mi_option_arena_max_object_size = 45,
    mi_option_arena_is_numa_local = 46,
    mi_option_stats_merge_on_collect = 47,
    mi_option_profile_alloc_interval = 48,
    mi_option_profile_inuse_interval = 49,
    mi_option_profile_time_interval = 50,
    mi_option_profile_sample_rate = 51,
    /// Bytes allocated/freed between thread-heap stats merges (default 4 MiB).
    /// Zero disables threshold-based merging; smaller values update parent
    /// statistics sooner at the cost of more frequent merges.
    mi_option_stats_merge_threshold = 52,
    mi_option_arena_purge_immediate_size = 53,
    /// Memory tagging mode (default 1). Effective only in a native MTE build
    /// on supported hardware/OS; setting this option does not enable MI_MTE.
    /// Linux modes: 2 = synchronous, 3 = asynchronous, 4 = preferred checks.
    mi_option_mte = 54,
}

impl mi_option_t {
    /// Upstream V3.6 spelling of [`Self::mi_option_guarded_precise`].
    /// The old enum variant remains available, including through glob imports.
    #[allow(non_upper_case_globals)]
    pub const mi_option_unsafe_guarded_precise: Self = Self::mi_option_guarded_precise;

    /// Current upstream spelling of the deprecated option at ID 31.
    #[allow(non_upper_case_globals)]
    pub const mi_option_deprecated_unsafe_guarded_precise: Self = Self::mi_option_guarded_precise;

    /// Deprecated upstream alias for [`Self::mi_option_arena_purge_min_size`].
    #[allow(non_upper_case_globals)]
    pub const mi_option_minimal_purge_size: Self = Self::mi_option_arena_purge_min_size;

    /// Upstream legacy spelling of [`Self::mi_option_allow_large_os_pages`].
    #[allow(non_upper_case_globals)]
    pub const mi_option_large_os_pages: Self = Self::mi_option_allow_large_os_pages;

    /// Upstream legacy spelling of [`Self::mi_option_arena_eager_commit`].
    #[allow(non_upper_case_globals)]
    pub const mi_option_eager_region_commit: Self = Self::mi_option_arena_eager_commit;

    /// Upstream legacy spelling of [`Self::mi_option_purge_decommits`].
    #[allow(non_upper_case_globals)]
    pub const mi_option_reset_decommits: Self = Self::mi_option_purge_decommits;

    /// Upstream legacy spelling of [`Self::mi_option_purge_delay`].
    #[allow(non_upper_case_globals)]
    pub const mi_option_reset_delay: Self = Self::mi_option_purge_delay;

    /// Upstream legacy spelling of [`Self::mi_option_disallow_os_alloc`].
    #[allow(non_upper_case_globals)]
    pub const mi_option_limit_os_alloc: Self = Self::mi_option_disallow_os_alloc;

    /// Compatibility alias for the former Rust binding name.
    #[allow(non_upper_case_globals)]
    pub const mi_option_collect_merges_stats: Self = Self::mi_option_stats_merge_on_collect;
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
    heap: *mut mi_heap_t,
) -> size_t;
pub type mi_profiler_on_realloc_inplace_fun = unsafe extern "C" fn(
    profiler: *mut mi_profiler_t,
    profiler_data: *mut mi_profiler_sample_data_t,
    ptr: *mut c_void,
    old_size: size_t,
    heap: *mut mi_heap_t,
) -> size_t;
pub type mi_profiler_on_free_fun = unsafe extern "C" fn(
    profiler: *mut mi_profiler_t,
    profiler_data: *mut mi_profiler_sample_data_t,
    ptr: *mut c_void,
    heap: *mut mi_heap_t,
);
pub type mi_profiler_on_snapshot_fun = unsafe extern "C" fn(profiler: *mut mi_profiler_t);
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
    pub on_snapshot: Option<mi_profiler_on_snapshot_fun>,
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
    /// Resize in place, returning null on failure without freeing or moving `p`.
    pub fn mi_expand(p: *mut c_void, newsize: size_t) -> *mut c_void;
    /// Only valid for null or blocks originally allocated with zero initialization.
    pub fn mi_rezalloc(p: *mut c_void, newsize: size_t) -> *mut c_void;
    /// As `mi_rezalloc`, with checked multiplication of `count` and `size`.
    pub fn mi_recalloc(p: *mut c_void, count: size_t, size: size_t) -> *mut c_void;
    /// Zero-initialized reallocation retaining the original power-of-two alignment.
    pub fn mi_rezalloc_aligned(p: *mut c_void, newsize: size_t, alignment: size_t) -> *mut c_void;
    pub fn mi_usable_size(p: *const c_void) -> size_t;
    /// Total allocator block size, including any padding or alignment overhead.
    /// Pass null or a live allocation. Use `mi_usable_size` for accessible bytes.
    pub fn mi_allocated_size(p: *const c_void) -> size_t;
    /// Snapshot of the allocation page's current owner, or zero for null,
    /// detached or abandoned pages. Not a Rust `ThreadId` or an ownership lock.
    /// A non-null pointer must be a live mimalloc allocation; its owner can change.
    /// QSBR-enabled host adapters may retain the previous owner on abandonment.
    pub fn mi_thread_id_of(p: *const c_void) -> size_t;
    pub fn mi_good_size(size: size_t) -> size_t;
    /// Free with the original requested size.
    ///
    /// # Safety
    /// `p` must be null or a live mimalloc allocation, and `size` must be the
    /// size used for the allocation. Debug builds assert the size fits; a
    /// mismatched size on a real pointer is an allocator contract violation.
    pub fn mi_free_size(p: *mut c_void, size: size_t);
    /// C23 sized free. Non-null `p` must have been allocated with exactly `size`
    /// and ordinary alignment. For aligned allocations use `mi_free_aligned_sized`.
    ///
    /// # Safety
    /// [`mi_free_size`]'s requirements apply; a non-null pointer with an
    /// incorrect size or non-ordinary alignment is undefined behavior in
    /// release builds.
    pub fn mi_free_sized(p: *mut c_void, size: size_t);
    /// C23 aligned sized free; note alignment precedes size, unlike
    /// `mi_free_size_aligned`. Both must match the original allocation.
    ///
    /// # Safety
    /// [`mi_free_sized`]'s requirements apply, with `alignment` also required
    /// to match the allocation's alignment.
    pub fn mi_free_aligned_sized(p: *mut c_void, alignment: size_t, size: size_t);
    /// Free a small-page allocation, skipping the page-size dispatch.
    ///
    /// # Safety
    /// `p` must be null or a live allocation on a small page; freeing a
    /// non-small or non-mimalloc pointer is undefined behavior.
    pub fn mi_free_small(p: *mut c_void);
    /// As `mi_free_small`, additionally requiring a non-null pointer.
    ///
    /// # Safety
    /// [`mi_free_small`]'s requirements apply; `p` must additionally be
    /// non-null (upstream asserts this even in debug builds).
    pub fn mi_free_small_nonnull(p: *mut c_void);
    /// The non-null allocation must be on a small page still owned by the calling thread.
    /// Allocation on this thread alone does not prove current page ownership.
    ///
    /// # Safety
    /// [`mi_free_small`]'s requirements apply, and the owning page must
    /// belong to the calling thread's context at the time of the call.
    pub fn mi_free_small_local(p: *mut c_void);
    /// As `mi_free_small_local`, additionally requiring a non-null pointer.
    ///
    /// # Safety
    /// [`mi_free_small_local`]'s requirements apply; `p` must additionally
    /// be non-null.
    pub fn mi_free_small_local_nonnull(p: *mut c_void);
    /// Free with the original requested size and alignment.
    ///
    /// # Safety
    /// `p` must be null or a live mimalloc allocation whose size and
    /// alignment match the call exactly; small allocations with a larger
    /// alignment route through the generic free (matching upstream), so only
    /// the arguments' correctness is the caller's responsibility.
    pub fn mi_free_size_aligned(p: *mut c_void, size: size_t, alignment: size_t);
    /// Free an aligned allocation by its alignment alone.
    ///
    /// # Safety
    /// `p` must be null or a live mimalloc allocation allocated with exactly
    /// `alignment`; upstream ignores the value in release builds.
    pub fn mi_free_aligned(p: *mut c_void, alignment: size_t);
}

// ── Allocation with usable-size outputs ─────────────────────────────────────
unsafe extern "C" {
    pub fn mi_umalloc(size: size_t, usable_size: *mut size_t) -> *mut c_void;
    pub fn mi_uzalloc(size: size_t, usable_size: *mut size_t) -> *mut c_void;
    pub fn mi_ucalloc(count: size_t, size: size_t, usable_size: *mut size_t) -> *mut c_void;
    pub fn mi_umalloc_small(size: size_t, usable_size: *mut size_t) -> *mut c_void;
    pub fn mi_uzalloc_small(size: size_t, usable_size: *mut size_t) -> *mut c_void;
    pub fn mi_umalloc_aligned(
        size: size_t,
        alignment: size_t,
        usable_size: *mut size_t,
    ) -> *mut c_void;
    pub fn mi_uzalloc_aligned(
        size: size_t,
        alignment: size_t,
        usable_size: *mut size_t,
    ) -> *mut c_void;
    /// Resize a live allocation or allocate for null. Failure preserves the old block.
    pub fn mi_urealloc(
        p: *mut c_void,
        newsize: size_t,
        usable_size_pre: *mut size_t,
        usable_size_post: *mut size_t,
    ) -> *mut c_void;
    /// As `mi_urealloc`, with a nonzero power-of-two output alignment.
    pub fn mi_urealloc_aligned(
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
        usable_size_pre: *mut size_t,
        usable_size_post: *mut size_t,
    ) -> *mut c_void;
    /// Report usable size before freeing a live allocation. Null reports zero.
    pub fn mi_ufree(p: *mut c_void, usable_size: *mut size_t);
}

/// Convert a byte size to a mimalloc machine-word count.
///
/// Unlike the upstream C macro `(size + word - 1) / word`, which wraps for
/// sizes above `SIZE_MAX - word + 1`, `div_ceil` cannot overflow. Such sizes
/// are rejected by every caller (the helpers gate on
/// [`MI_SMALL_SIZE_MAX`]), so results differ only at nonsensical inputs
/// where the Rust value is the mathematically correct one.
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
/// allocation size used for the corresponding allocation. Small allocations
/// must not be over-aligned; use [`mi_free_csize_aligned`] for aligned allocations.
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
/// allocation size used for the corresponding allocation. Small allocations
/// must not be over-aligned; use [`mi_free_csize_aligned_nonnull`] for aligned allocations.
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

// ── Offset-aligned allocation (payload after an in-block header) ───────────

unsafe extern "C" {
    /// Allocate with the byte at `offset` guaranteed `alignment`-aligned.
    /// Useful when a block stores an inline header before the aligned payload.
    pub fn mi_malloc_aligned_at(size: size_t, alignment: size_t, offset: size_t) -> *mut c_void;
    pub fn mi_zalloc_aligned_at(size: size_t, alignment: size_t, offset: size_t) -> *mut c_void;
    pub fn mi_calloc_aligned_at(
        count: size_t,
        size: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
    /// As `mi_realloc_aligned`, keeping the byte at `offset` aligned.
    pub fn mi_realloc_aligned_at(
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
    /// Only valid for null or originally zero-initialized blocks.
    pub fn mi_rezalloc_aligned_at(
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
    /// As `mi_rezalloc_aligned_at`, with checked multiplication of `newcount`
    /// and `size`.
    pub fn mi_recalloc_aligned_at(
        p: *mut c_void,
        newcount: size_t,
        size: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
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
    /// Create a first-class heap. Never returns null; check nothing but free
    /// the result eventually through [`mi_heap_delete`] or [`mi_heap_destroy`].
    ///
    /// # Safety
    /// The returned handle may be shared across threads (first-class heaps
    /// accept allocations from any thread), but deleting/destroying it must
    /// exclude every concurrent use, including cached thread-local views.
    pub fn mi_heap_new() -> *mut mi_heap_t;
    /// Experimental QSBR constructor from `mimalloc/qsbr.h`.
    ///
    /// Ordinary builds have no QSBR runtime adapter: this reports ENOSYS and
    /// returns null. Enabling QSBR requires host-defined C metadata and hooks
    /// embedded in the single-source build. It delays page release, not block
    /// reuse, and does not make freed Rust objects readable. Protected heaps
    /// reject `mi_heap_delete`; destroy them only after every reader and mutator
    /// has stopped. The last free on a delayed abandoned page can initialize
    /// the receiving thread's context; do not free these allocations from late
    /// TLS destructors after mimalloc thread shutdown. There is deliberately
    /// no owning `Heap` wrapper for them.
    pub fn mi_heap_new_qsbr() -> *mut mi_heap_t;
    /// Delete the heap, moving its live blocks to the main heap.
    ///
    /// # Safety
    /// `heap` must be a live, non-null handle with no concurrent access and
    /// no cached thread-local context views anywhere. The main heap is
    /// unaffected by this call (deleting it is a no-op there). All pointers
    /// allocated from the heap remain valid and frees on any thread keep
    /// working, but the handle itself is invalid afterwards.
    pub fn mi_heap_delete(heap: *mut mi_heap_t);
    /// Release the calling thread's context and permanently disable its heap
    /// allocations. Existing blocks remain valid and may be freed on any thread.
    /// Discard all cached contexts for this heap on the caller before calling.
    /// Main heaps ignore this operation. Keep the heap alive during the call.
    pub fn mi_thread_heap_release(heap: *mut mi_heap_t);
    /// Destroy the heap, freeing its live blocks.
    ///
    /// # Safety
    /// [`mi_heap_delete`]'s requirements apply, and every pointer allocated
    /// from the heap becomes dangling the moment the call returns: nothing
    /// may read, write or free them afterwards. Main heaps are unaffected.
    pub fn mi_heap_destroy(heap: *mut mi_heap_t);
    /// Collects this heap's context on the caller, not all of its worker contexts.
    pub fn mi_heap_collect(heap: *mut mi_heap_t, force: bool);
    pub fn mi_heap_main() -> *mut mi_heap_t;
    pub fn mi_heap_of(p: *const c_void) -> *mut mi_heap_t;
    pub fn mi_heap_contains(heap: *const mi_heap_t, p: *const c_void) -> bool;
    pub fn mi_any_heap_contains(p: *const c_void) -> bool;
    /// Call only while no thread accesses the heap (including its allocations).
    pub fn mi_heap_set_numa_affinity(heap: *mut mi_heap_t, numa_node: c_int);
    /// Atomically preserve up to 1 MiB of each block from debug-pattern filling.
    /// This does not preserve freed objects: free-list metadata can overwrite them.
    /// `heap` must be a live non-null handle. Obtain the main handle with `mi_heap_main`.
    pub fn mi_heap_set_debug_fill_preserve(heap: *mut mi_heap_t, preserve: size_t);
    /// Never returns null. On initialization failure or after thread teardown,
    /// returns an empty context whose allocations fail. Owner-thread use only.
    pub fn mi_heap_theap(heap: *mut mi_heap_t) -> *mut mi_theap_t;

    pub fn mi_heap_malloc(heap: *mut mi_heap_t, size: size_t) -> *mut c_void;
    pub fn mi_heap_zalloc(heap: *mut mi_heap_t, size: size_t) -> *mut c_void;
    pub fn mi_heap_calloc(heap: *mut mi_heap_t, count: size_t, size: size_t) -> *mut c_void;
    pub fn mi_heap_mallocn(heap: *mut mi_heap_t, count: size_t, size: size_t) -> *mut c_void;
    /// `size` must not exceed [`MI_SMALL_SIZE_MAX`]; no extra alignment is implied.
    pub fn mi_heap_malloc_small(heap: *mut mi_heap_t, size: size_t) -> *mut c_void;
    /// As `mi_heap_malloc_small`, zero-initializing the returned block.
    pub fn mi_heap_zalloc_small(heap: *mut mi_heap_t, size: size_t) -> *mut c_void;
    pub fn mi_heap_realloc(heap: *mut mi_heap_t, p: *mut c_void, newsize: size_t) -> *mut c_void;
    pub fn mi_heap_reallocn(
        heap: *mut mi_heap_t,
        p: *mut c_void,
        count: size_t,
        size: size_t,
    ) -> *mut c_void;
    /// Only valid for null or blocks originally allocated with zero initialization.
    pub fn mi_heap_rezalloc(heap: *mut mi_heap_t, p: *mut c_void, newsize: size_t) -> *mut c_void;
    /// As `mi_heap_rezalloc`, with checked multiplication of `count` and `size`.
    pub fn mi_heap_recalloc(
        heap: *mut mi_heap_t,
        p: *mut c_void,
        count: size_t,
        size: size_t,
    ) -> *mut c_void;
    pub fn mi_heap_malloc_aligned(
        heap: *mut mi_heap_t,
        size: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    pub fn mi_heap_zalloc_aligned(
        heap: *mut mi_heap_t,
        size: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    pub fn mi_heap_realloc_aligned(
        heap: *mut mi_heap_t,
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    /// Checked zeroed array allocation; `alignment` must be a nonzero power of two.
    pub fn mi_heap_calloc_aligned(
        heap: *mut mi_heap_t,
        count: size_t,
        size: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    /// Only valid for null or zero-initialized blocks with the given alignment.
    pub fn mi_heap_rezalloc_aligned(
        heap: *mut mi_heap_t,
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    /// As `mi_heap_rezalloc_aligned`, with checked multiplication of `count` and `size`.
    pub fn mi_heap_recalloc_aligned(
        heap: *mut mi_heap_t,
        p: *mut c_void,
        count: size_t,
        size: size_t,
        alignment: size_t,
    ) -> *mut c_void;

    /// Allocate from this heap with the byte at `offset` `alignment`-aligned.
    pub fn mi_heap_malloc_aligned_at(
        heap: *mut mi_heap_t,
        size: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
    pub fn mi_heap_zalloc_aligned_at(
        heap: *mut mi_heap_t,
        size: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
    pub fn mi_heap_calloc_aligned_at(
        heap: *mut mi_heap_t,
        count: size_t,
        size: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
    /// As `mi_heap_realloc_aligned`, keeping the byte at `offset` aligned.
    pub fn mi_heap_realloc_aligned_at(
        heap: *mut mi_heap_t,
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
    /// Only valid for null or originally zero-initialized blocks.
    pub fn mi_heap_rezalloc_aligned_at(
        heap: *mut mi_heap_t,
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
    /// As `mi_heap_rezalloc_aligned_at`, with checked multiplication of
    /// `newcount` and `size`.
    pub fn mi_heap_recalloc_aligned_at(
        heap: *mut mi_heap_t,
        p: *mut c_void,
        newcount: size_t,
        size: size_t,
        alignment: size_t,
        offset: size_t,
    ) -> *mut c_void;
}

// ── Thread-local heaps ──────────────────────────────────────────────────────

unsafe extern "C" {
    /// The returned handle belongs to the calling thread; do not send it to another thread.
    pub fn mi_theap_get_default() -> *mut mi_theap_t;
    pub fn mi_theap_set_default(theap: *mut mi_theap_t) -> *mut mi_theap_t;
    pub fn mi_theap_collect(theap: *mut mi_theap_t, force: bool);
    /// Owner-thread only. Abandons pages while preserving live allocations and
    /// the parent heap. Invalidates this handle and every cached `ThreadHeap`
    /// alias; reacquire a context through `mi_heap_theap` before reuse. Upstream
    /// retains the current thread's main context as a special case.
    pub fn mi_theap_delete(theap: *mut mi_theap_t);
    /// Abandon pages, preserving live allocations and this context for reuse.
    /// The owner must be the caller or quiescent outside mimalloc. Exclude
    /// concurrent heap/context deletion and keep the parent heap alive.
    pub fn mi_theap_quiescent_abandon(theap: *mut mi_theap_t);
    pub fn mi_theap_calloc(theap: *mut mi_theap_t, count: size_t, size: size_t) -> *mut c_void;
    pub fn mi_theap_malloc_aligned(
        theap: *mut mi_theap_t,
        size: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    pub fn mi_theap_zalloc_aligned(
        theap: *mut mi_theap_t,
        size: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    /// Owner-thread aligned reallocation. On failure, non-null `p` stays valid.
    /// `alignment` must be a nonzero power of two; keep the context initialized.
    pub fn mi_theap_realloc_aligned(
        theap: *mut mi_theap_t,
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    /// As `mi_theap_realloc_aligned`, for null or originally zero-initialized
    /// blocks only. Preserves existing contents and zeroes growth.
    pub fn mi_theap_rezalloc_aligned(
        theap: *mut mi_theap_t,
        p: *mut c_void,
        newsize: size_t,
        alignment: size_t,
    ) -> *mut c_void;
    pub fn mi_theap_realloc(theap: *mut mi_theap_t, p: *mut c_void, newsize: size_t)
    -> *mut c_void;
    /// Only valid for blocks originally allocated with zero initialization.
    pub fn mi_theap_rezalloc(
        theap: *mut mi_theap_t,
        p: *mut c_void,
        newsize: size_t,
    ) -> *mut c_void;
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
    /// Query the arena's reserved region, including allocator metadata.
    /// `arena_id` must be valid; `size` may be null or writable for one size_t.
    pub fn mi_arena_area(arena_id: mi_arena_id_t, size: *mut size_t) -> *mut c_void;
    /// Test address membership without reading the pointed-to memory.
    /// `arena_id` must be valid; `p` need not be an allocated block.
    pub fn mi_arena_contains(arena_id: mi_arena_id_t, p: *const c_void) -> bool;
    pub fn mi_arenas_purge();
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
    pub fn mi_option_get_clamp(option: mi_option_t, min: c_long, max: c_long) -> c_long;
    pub fn mi_option_set_default(option: mi_option_t, value: c_long);
    pub fn mi_option_set_enabled(option: mi_option_t, enable: bool);
    pub fn mi_option_set_enabled_default(option: mi_option_t, enable: bool);
    pub fn mi_options_print_out(out: Option<mi_output_fun>, arg: *mut c_void);
}

// ── Process-wide callbacks ─────────────────────────────────────────────────

unsafe extern "C" {
    /// Install an error callback; `None` restores the default handler.
    ///
    /// Register only while no other thread can report allocator errors. The
    /// callback and `arg` must outlive registration and all in-flight calls.
    /// Callbacks may run concurrently and must not unwind into C.
    pub fn mi_register_error(callback: Option<mi_error_fun>, arg: *mut c_void);
    /// Install an output callback; registration can immediately flush buffered
    /// output. `None` restores stderr. Register with no concurrent output or
    /// registration, keep callback state alive, and never unwind into C.
    pub fn mi_register_output(callback: Option<mi_output_fun>, arg: *mut c_void);
    /// Register a deferred-free callback, or `None` to unregister it. Exclude
    /// concurrent allocator activity during registration. The callback must
    /// support concurrent calls, retain valid state, and must not unwind.
    pub fn mi_register_deferred_free(callback: Option<mi_deferred_free_fun>, arg: *mut c_void);
}

// ── Experimental profiling ─────────────────────────────────────────────────

unsafe extern "C" {
    /// Attach and stop the profiler. Free callbacks cover samples from this
    /// attachment generation. Detach and quiesce callbacks before deletion.
    pub fn mi_heap_profile(heap: *mut mi_heap_t, profiler: *mut mi_profiler_t) -> bool;
    pub fn mi_heap_profile_disable(heap: *mut mi_heap_t);
    pub fn mi_subproc_profile(subproc_id: mi_subproc_id_t, profiler: *mut mi_profiler_t) -> bool;
    pub fn mi_profile(profiler: *mut mi_profiler_t) -> bool;
    /// Start sampling. Returns whether the profiler was already running —
    /// this is **not** a success flag; starting an already-running profiler
    /// returns early with `true` and changes nothing.
    pub fn mi_profiler_start(profiler: *mut mi_profiler_t) -> bool;
    /// Stop allocation sampling, returning the previous running state.
    /// Free callbacks continue for sampled blocks until the profiler is detached.
    pub fn mi_profiler_stop(profiler: *mut mi_profiler_t) -> bool;
    /// Invokes the snapshot callback; callback state must remain alive and must not unwind.
    pub fn mi_profiler_snapshot(profiler: *mut mi_profiler_t);
    /// Creates an experimental pprof profiler. The file name is a NUL-terminated
    /// string. Stop and detach the profiler, then free all sampled allocations
    /// and quiesce its callbacks before deleting it.
    pub fn mi_pprof_profiler_new(
        initial_threshold: size_t,
        base_file_name: *const c_char,
        alloc_interval_size: size_t,
        inuse_interval_size: size_t,
        time_interval_secs: size_t,
    ) -> *mut mi_profiler_t;
    pub fn mi_pprof_profiler_delete(profiler: *mut mi_profiler_t);
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
    /// Native block size for a statistics bin, or zero when out of range.
    pub fn mi_stats_get_bin_size(bin: size_t) -> size_t;
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
    /// Merge accumulated heap statistics into its subprocess and clear the
    /// heap counters. The heap handle must remain live for the call.
    pub fn mi_heap_stats_merge_to_subproc(heap: *mut mi_heap_t);
    /// Aggregate subprocess statistics as JSON. Free the returned string
    /// with `mi_free` when the caller passes a null buffer. A non-null
    /// buffer must be writable for `buf_size` bytes.
    pub fn mi_subproc_stats_get_json(
        subproc_id: mi_subproc_id_t,
        buf_size: size_t,
        buf: *mut c_char,
    ) -> *mut c_char;
    /// Print aggregated subprocess statistics through the supplied callback.
    pub fn mi_subproc_stats_print_out(
        subproc_id: mi_subproc_id_t,
        out: Option<mi_output_fun>,
        arg: *mut c_void,
    );
    /// Print statistics for each heap and the subprocess separately. The
    /// callback must not reenter operations that acquire the subprocess's
    /// heap-list lock, including heap creation, deletion or visitation.
    pub fn mi_subproc_heap_stats_print_out(
        subproc_id: mi_subproc_id_t,
        out: Option<mi_output_fun>,
        arg: *mut c_void,
    );
    pub fn mi_theap_stats_merge_to_heap(theap: *mut mi_theap_t);
}

// ── Subprocesses ────────────────────────────────────────────────────────────

unsafe extern "C" {
    /// The initial subprocess containing the main thread's heaps.
    pub fn mi_subproc_main() -> mi_subproc_id_t;
    /// The subprocess that owns the calling thread.
    pub fn mi_subproc_current() -> mi_subproc_id_t;
    /// Create a new subprocess. Call `mi_subproc_add_current_thread` on a
    /// freshly created thread before it allocates.
    pub fn mi_subproc_new() -> mi_subproc_id_t;
    /// Destroy a subprocess created with `mi_subproc_new`. Its heaps must no
    /// longer be used; live blocks move to the main subprocess.
    pub fn mi_subproc_destroy(subproc: mi_subproc_id_t);
    /// Attach the calling thread to `subproc`. Call right after thread creation,
    /// before the thread allocates.
    pub fn mi_subproc_add_current_thread(subproc: mi_subproc_id_t);
    /// Visit all heaps of `subproc`. Return `false` from `visitor` to stop
    /// early. Only valid while subprocess threads are quiescent.
    ///
    /// `visitor` must be non-null: upstream dereferences it unconditionally
    /// and every subprocess owns at least its main heap, so [`None`]
    /// always crashes the process. Unlike [`mi_heap_visit_blocks`], a null
    /// visitor is not a supported no-op here.
    pub fn mi_subproc_visit_heaps(
        subproc: mi_subproc_id_t,
        visitor: Option<mi_heap_visit_fun>,
        arg: *mut c_void,
    ) -> bool;
}

// ── Heap block visitors ─────────────────────────────────────────────────────

unsafe extern "C" {
    /// Visit the live blocks of `heap`. With `visit_blocks`, the visitor also
    /// receives each block; otherwise it is called once per heap area. Only
    /// valid while no thread allocates in or frees from `heap`.
    pub fn mi_heap_visit_blocks(
        heap: *mut mi_heap_t,
        visit_blocks: bool,
        visitor: Option<mi_block_visit_fun>,
        arg: *mut c_void,
    ) -> bool;
    /// As `mi_heap_visit_blocks`, additionally visiting abandoned heaps.
    /// Blocks in abandoned heaps are not owned by any live thread context.
    pub fn mi_heap_visit_abandoned_blocks(
        heap: *mut mi_heap_t,
        visit_blocks: bool,
        visitor: Option<mi_block_visit_fun>,
        arg: *mut c_void,
    ) -> bool;
    /// Visit the live blocks of a cached thread-local context. Owner-thread
    /// or quiescent-context use only, like the other `mi_theap_*` APIs.
    ///
    /// `visitor` must be non-null: upstream only null-checks its internal
    /// area wrapper and invokes the visitor unconditionally for every page,
    /// so [`None`] crashes whenever the context owns at least one page.
    pub fn mi_theap_visit_blocks(
        theap: *const mi_theap_t,
        visit_blocks: bool,
        visitor: Option<mi_block_visit_fun>,
        arg: *mut c_void,
    ) -> bool;
}
