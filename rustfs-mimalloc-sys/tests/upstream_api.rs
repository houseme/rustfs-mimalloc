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
    if MI_PROFILE_ENABLED {
        assert!(profiler.allocations.load(Ordering::Relaxed) > 0);
    } else {
        assert_eq!(profiler.allocations.load(Ordering::Relaxed), 0);
    }
    assert_eq!(
        profiler.frees.load(Ordering::Relaxed),
        profiler.allocations.load(Ordering::Relaxed)
    );
    assert_eq!(profiler.snapshots.load(Ordering::Relaxed), 1);
}

fn pprof_snapshot_matches_build_sampling_mode() {
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
    assert_eq!(
        profile.lines().skip(1).any(|line| line.contains(" @")),
        MI_PROFILE_ENABLED,
        "snapshot sample presence differs from build mode"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

fn header_and_runtime_versions_agree() {
    assert_eq!(
        unsafe { mi_version() }.to_string(),
        env!("MIMALLOC_VERSION")
    );
}

#[cfg(feature = "no_profile")]
fn no_profile_ignores_automatic_profile_startup() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "rustfs-mimalloc-profile-disabled-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    // Set the environment only on the child, before mimalloc initialization.
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--profile-environment-probe")
        .env("MIMALLOC_PROFILE", dir.join("disabled.heap"))
        .env("MIMALLOC_PROFILE_SAMPLE_RATE", "1")
        .env("MIMALLOC_PROFILE_ALLOC_INTERVAL", "1")
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir(&dir).unwrap();
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

unsafe extern "C" fn count_output(_: *const c_char, arg: *mut c_void) {
    unsafe { &*arg.cast::<AtomicUsize>() }.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn count_deferred_free(_: bool, _: u64, arg: *mut c_void) {
    unsafe { &*arg.cast::<AtomicUsize>() }.fetch_add(1, Ordering::Relaxed);
}

fn diagnostic_callbacks_receive_their_state() {
    let output_calls = AtomicUsize::new(0);
    let deferred_calls = AtomicUsize::new(0);
    unsafe {
        // This executable has no active worker at registration or unregistration.
        mi_register_output(
            Some(count_output),
            (&output_calls as *const AtomicUsize).cast_mut().cast(),
        );
        mi_register_output(None, std::ptr::null_mut());
        // Registering output flushes the upstream delayed-output buffer immediately.
        assert!(output_calls.load(Ordering::Relaxed) > 0);
        mi_register_deferred_free(
            Some(count_deferred_free),
            (&deferred_calls as *const AtomicUsize).cast_mut().cast(),
        );
        mi_collect(true);
        mi_register_deferred_free(None, std::ptr::null_mut());
    }
    assert!(deferred_calls.load(Ordering::Relaxed) > 0);
}

fn usable_size_symbols_work_without_override() {
    unsafe {
        for size in [1, 42, 1024, 64 * 1024] {
            let ptr = mi_malloc(size);
            assert!(!ptr.is_null());
            assert_eq!(mi_malloc_size(ptr), mi_usable_size(ptr));
            assert_eq!(mi_malloc_usable_size(ptr), mi_usable_size(ptr));
            assert!(mi_malloc_size(ptr) >= size);
            mi_free(ptr);
        }
        assert_eq!(mi_malloc_size(std::ptr::null()), 0);
        assert_eq!(mi_malloc_usable_size(std::ptr::null()), 0);
    }
}

static ALLOCATOR_ERRORS: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" fn count_allocator_error(_: c_int, _: *mut c_void) {
    ALLOCATOR_ERRORS.fetch_add(1, Ordering::Relaxed);
}

fn destroy_collects_pending_cross_thread_frees() {
    unsafe {
        let heap = mi_heap_new();
        assert!(!heap.is_null());
        let live = mi_heap_malloc(heap, 32);
        let remote = mi_heap_malloc(heap, 32);
        assert!(!live.is_null() && !remote.is_null());
        // The address is sent with exclusive ownership; the heap and allocating
        // thread remain alive until the worker has completed its free.
        let address = remote as usize;
        std::thread::spawn(move || mi_free(address as *mut c_void))
            .join()
            .unwrap();
        ALLOCATOR_ERRORS.store(0, Ordering::Relaxed);
        mi_register_error(Some(count_allocator_error), std::ptr::null_mut());
        mi_heap_destroy(heap); // also invalidates `live`; never access it again
        mi_register_error(None, std::ptr::null_mut());
    }
    assert_eq!(ALLOCATOR_ERRORS.load(Ordering::Relaxed), 0);
}

fn destroyed_heap_address_reuse_invalidates_cached_theap() {
    use std::sync::mpsc::sync_channel;
    let (send_heap, receive_heap) = sync_channel::<usize>(0);
    let (send_result, receive_result) = sync_channel(0);
    let worker = std::thread::spawn(move || {
        while let Ok(address) = receive_heap.recv() {
            let heap = address as *mut mi_heap_t;
            // The producer keeps this heap alive until it receives the result.
            // Two allocations ensure the worker's cache refers to this heap,
            // rather than the main heap used for thread metadata initialization.
            let valid = unsafe {
                let first = mi_heap_malloc(heap, 16);
                assert!(!first.is_null());
                mi_free(first);
                let live = mi_heap_malloc(heap, 16);
                assert!(!live.is_null());
                mi_heap_contains(heap, live)
                // `live` is left to heap_destroy; the worker never reuses it.
            };
            send_result.send(valid).unwrap();
        }
    });
    let mut reuse_hits = 0;
    unsafe {
        let mut heap = mi_heap_new();
        assert!(!heap.is_null());
        for _ in 0..32 {
            send_heap.send(heap as usize).unwrap();
            assert!(
                receive_result.recv().unwrap(),
                "allocation used a stale theap"
            );
            let old_address = heap as usize;
            mi_heap_destroy(heap); // the worker is now idle and holds no live reference
            let mut candidates = Vec::with_capacity(1024);
            loop {
                heap = mi_heap_new();
                assert!(!heap.is_null());
                if heap as usize == old_address {
                    reuse_hits += 1;
                    break;
                }
                if candidates.len() == 1024 {
                    break;
                }
                candidates.push(heap);
            }
            for candidate in candidates {
                mi_heap_destroy(candidate);
            }
        }
        // Exercise the last replacement before tearing down the worker.
        send_heap.send(heap as usize).unwrap();
        assert!(receive_result.recv().unwrap());
        drop(send_heap);
        worker.join().unwrap();
        mi_heap_destroy(heap);
    }
    // Allocator address reuse is not guaranteed, so report coverage explicitly.
    if reuse_hits == 0 {
        eprintln!("heap address reuse regression: INCONCLUSIVE (no address reused)");
    } else {
        println!("heap address reuse regression: {reuse_hits} observed replacements passed");
    }
}

// Process-global profiler registration requires a single-threaded executable,
// with no Rust test-harness threads allocating concurrently (especially override).
fn main() {
    #[cfg(feature = "no_profile")]
    {
        if std::env::args().any(|arg| arg == "--profile-environment-probe") {
            for _ in 0..128 {
                unsafe {
                    let ptr = mi_malloc(64 * 1024);
                    assert!(!ptr.is_null());
                    mi_free(ptr);
                }
            }
            return;
        }
        no_profile_ignores_automatic_profile_startup();
    }
    header_and_runtime_versions_agree();
    usable_size_symbols_work_without_override();
    diagnostic_callbacks_receive_their_state();
    destroy_collects_pending_cross_thread_frees();
    destroyed_heap_address_reuse_invalidates_cached_theap();
    #[cfg(feature = "no_thp")]
    no_thp_sets_the_runtime_default();
    #[cfg(all(feature = "override", not(target_os = "windows")))]
    override_routes_system_allocations_to_mimalloc();
    profiler_callbacks_match_upstream_abi();
    pprof_snapshot_matches_build_sampling_mode();
    println!("upstream API integration checks passed");
}
