use rustfs_mimalloc_sys::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(C)]
struct Profiler {
    hooks: mi_profiler_t,
    allocations: AtomicUsize,
    frees: AtomicUsize,
    snapshots: AtomicUsize,
}

unsafe extern "C" fn allocated(
    profiler: *mut mi_profiler_t,
    _: *mut mi_profiler_sample_data_t,
    _: *mut c_void,
    _: usize,
    _: usize,
    _: u64,
    _: *mut mi_heap_t,
) -> usize {
    unsafe { &*profiler.cast::<Profiler>() }
        .allocations
        .fetch_add(1, Ordering::Relaxed);
    1
}
unsafe extern "C" fn freed(
    profiler: *mut mi_profiler_t,
    _: *mut mi_profiler_sample_data_t,
    _: *mut c_void,
    _: *mut mi_heap_t,
) {
    unsafe { &*profiler.cast::<Profiler>() }
        .frees
        .fetch_add(1, Ordering::Relaxed);
}
unsafe extern "C" fn snapshot(profiler: *mut mi_profiler_t) {
    unsafe { &*profiler.cast::<Profiler>() }
        .snapshots
        .fetch_add(1, Ordering::Relaxed);
}

fn profiler_callbacks_match_upstream_abi() {
    let mut profiler = Profiler {
        hooks: mi_profiler_t {
            reserved: std::ptr::null_mut(),
            sample_data_size: 8,
            initial_sample_rate: 1,
            on_alloc: Some(allocated),
            on_free: Some(freed),
            on_realloc_inplace: None,
            on_snapshot: Some(snapshot),
        },
        allocations: AtomicUsize::new(0),
        frees: AtomicUsize::new(0),
        snapshots: AtomicUsize::new(0),
    };
    unsafe {
        let heap = mi_heap_main();
        assert!(!heap.is_null());
        assert!(mi_heap_profile(heap, &mut profiler.hooks));
        let mut allocations = Vec::with_capacity(64);
        // Initialize the main theap before starting sampling.
        mi_free(mi_heap_malloc(heap, 64));
        // start/stop return the previous running state, not a success flag.
        assert!(!mi_profiler_start(&mut profiler.hooks));
        for _ in 0..64 {
            let ptr = mi_heap_malloc(heap, 64 * 1024);
            assert!(!ptr.is_null());
            allocations.push(ptr);
        }
        for ptr in allocations.drain(..) {
            mi_free(ptr);
        }
        mi_profiler_snapshot(&mut profiler.hooks);
        assert!(mi_profiler_stop(&mut profiler.hooks));
        assert!(mi_heap_profile(heap, std::ptr::null_mut()));
    }
    assert!(profiler.allocations.load(Ordering::Relaxed) > 0);
    assert_eq!(
        profiler.frees.load(Ordering::Relaxed),
        profiler.allocations.load(Ordering::Relaxed)
    );
    assert_eq!(profiler.snapshots.load(Ordering::Relaxed), 1);
}

fn pprof_writes_a_sampled_heap_snapshot() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "rustfs-mimalloc-pprof-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let base = std::ffi::CString::new(dir.join("sample.heap").to_str().unwrap()).unwrap();
    unsafe {
        let profiler = mi_pprof_profiler_new(1, base.as_ptr(), 0, 0, 0);
        assert!(!profiler.is_null());
        let heap = mi_heap_main();
        assert!(!heap.is_null());
        assert!(mi_heap_profile(heap, profiler));
        let mut allocations = Vec::with_capacity(64);
        mi_free(mi_heap_malloc(heap, 64));
        mi_profiler_start(profiler);
        for _ in 0..64 {
            let ptr = mi_heap_malloc(heap, 64 * 1024);
            assert!(!ptr.is_null());
            allocations.push(ptr);
        }
        mi_profiler_stop(profiler);
        // No allocation or callback runs concurrently with this snapshot.
        mi_profiler_snapshot(profiler);
        for ptr in allocations.drain(..) {
            mi_free(ptr);
        }
        assert!(mi_heap_profile(heap, std::ptr::null_mut()));
        mi_pprof_profiler_delete(profiler);
    }
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let profile = std::fs::read_to_string(&files[0]).unwrap();
    assert!(profile.starts_with("heap profile:"), "{profile}");
    assert!(profile.contains("MAPPED_LIBRARIES:"));
    assert!(
        profile.lines().skip(1).any(|line| line.contains(" @")),
        "snapshot has no samples"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

fn header_and_runtime_versions_agree() {
    assert_eq!(
        unsafe { mi_version() }.to_string(),
        env!("MIMALLOC_VERSION")
    );
}

#[cfg(feature = "no_thp")]
fn no_thp_sets_the_runtime_default() {
    if std::env::var_os("MIMALLOC_ALLOW_THP").is_none() {
        assert_eq!(
            unsafe { mi_option_get(mi_option_t::mi_option_allow_thp) },
            0
        );
    }
}

#[cfg(all(feature = "override", not(target_os = "windows")))]
fn override_routes_system_allocations_to_mimalloc() {
    use std::alloc::{GlobalAlloc, Layout, System};
    let layout = Layout::from_size_align(64, 8).unwrap();
    unsafe {
        let ptr = System.alloc(layout);
        assert!(!ptr.is_null());
        let owned = mi_any_heap_contains(ptr.cast());
        System.dealloc(ptr, layout);
        assert!(owned, "override did not intercept the system allocator");
    }
}

// Process-global profiler registration requires a single-threaded executable,
// with no Rust test-harness threads allocating concurrently (especially override).
fn main() {
    header_and_runtime_versions_agree();
    #[cfg(feature = "no_thp")]
    no_thp_sets_the_runtime_default();
    #[cfg(all(feature = "override", not(target_os = "windows")))]
    override_routes_system_allocations_to_mimalloc();
    profiler_callbacks_match_upstream_abi();
    pprof_writes_a_sampled_heap_snapshot();
    println!("upstream API integration checks passed");
}
