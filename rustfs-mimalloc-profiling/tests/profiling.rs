mod tests {
    use std::alloc::{Layout, alloc_zeroed, dealloc, realloc};
    use std::sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use prost::Message;
    use rustfs_mimalloc_profiling::pprof::Profile;
    use rustfs_mimalloc_profiling::{MimallocConfig, Profiler, SamplingMiMalloc, mimalloc_stats};

    #[global_allocator]
    static ALLOC: SamplingMiMalloc = SamplingMiMalloc::new();

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn public_session_lifecycle_rejects_stopped_reports_and_preserves_new_owner() {
        use rustfs_mimalloc_profiling::Error;
        let _guard = TEST_LOCK.lock().unwrap();
        let mut first = Profiler::start(MimallocConfig::default()).unwrap();
        assert!(matches!(
            Profiler::start(MimallocConfig::default()),
            Err(Error::AlreadyActive)
        ));
        first.stop();
        first.stop();
        assert_eq!(first.report(), Err(Error::Inactive));
        let mut second = Profiler::start(MimallocConfig::default()).unwrap();
        drop(first);
        assert!(second.report().is_ok());
        drop(second);
        assert!(Profiler::start(MimallocConfig::default()).is_ok());
    }

    #[test]
    fn failed_configuration_does_not_claim_the_recorder() {
        use rustfs_mimalloc_profiling::Error;
        let _guard = TEST_LOCK.lock().unwrap();
        assert!(matches!(
            Profiler::start(MimallocConfig {
                sample_interval_bytes: 0,
                ..MimallocConfig::default()
            }),
            Err(Error::InvalidConfig(_))
        ));
        let mut profiler = Profiler::start(MimallocConfig::default()).unwrap();
        assert!(profiler.report().is_ok());
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn native_capture_preserves_user_frames_source_metadata_and_live_lifecycle() {
        use rustfs_mimalloc_profiling::MimallocStackCapture;
        let _guard = TEST_LOCK.lock().unwrap();
        let mut backend = Profiler::start(MimallocConfig {
            stack_capture: MimallocStackCapture::Native,
            sample_interval_bytes: 1024,
            live_heap_tracking: true,
            ..MimallocConfig::default()
        })
        .unwrap();
        let retained = allocate_retained_live_test(2 * 1024 * 1024);
        let profile = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&profile, "allocate_retained_live_test", "inuse_space"),
            retained.len() as i64
        );
        let function = profile
            .function
            .iter()
            .find(|function| {
                profile.string_table[function.name as usize].contains("allocate_retained_live_test")
            })
            .unwrap();
        assert!(profile.string_table[function.filename as usize].ends_with("profiling.rs"));
        assert!(profile.location.iter().any(|location| {
            location.address != 0
                && location.mapping_id != 0
                && location
                    .line
                    .iter()
                    .any(|line| line.function_id == function.id && line.line > 0)
        }));
        std::thread::spawn(move || drop(retained)).join().unwrap();
        let freed = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&freed, "allocate_retained_live_test", "inuse_space"),
            0
        );
        backend.stop();
    }

    #[test]
    fn mimalloc_backend_reports_raw_memory_pprof() {
        let _guard = TEST_LOCK.lock().expect("lock mimalloc backend test");
        let mut backend = Profiler::start(MimallocConfig {
            sample_interval_bytes: 1024,
            ..MimallocConfig::default()
        })
        .expect("initialize mimalloc backend");
        assert_eq!(mimalloc_stats().reports, 0);
        assert_eq!(mimalloc_stats().reported_samples, 0);

        let allocations: Vec<Vec<u8>> = (0..4096).map(|_| vec![0_u8; 1024]).collect();
        std::hint::black_box(&allocations);

        let profile = report_profile(&mut backend);
        assert!(profile.string_table.iter().any(|s| s == "alloc_space"));
        backend.stop();
    }

    #[test]
    fn mimalloc_backend_reports_multithreaded_allocation_churn() {
        let _guard = TEST_LOCK.lock().expect("lock mimalloc backend test");
        let mut backend = Profiler::start(MimallocConfig {
            sample_interval_bytes: 4096,
            ring_capacity: 16_384,
            report_drain_limit: 16_384,
            ..MimallocConfig::default()
        })
        .expect("initialize mimalloc backend");

        let workers: Vec<_> = (0..4)
            .map(|worker| {
                std::thread::spawn(move || {
                    let allocations: Vec<Vec<u8>> = (0..512)
                        .map(|iteration| vec![worker as u8; 512 + (iteration % 4) * 128])
                        .collect();
                    std::hint::black_box(&allocations);
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("join allocation worker");
        }

        let stats = mimalloc_stats();
        assert!(stats.recorded_samples > 0);
        assert!(stats.flushes > 0);
        assert!(stats.flushed_samples > 0);

        let profile = report_profile(&mut backend);
        assert!(!profile.sample.is_empty());
        assert!(
            profile
                .sample
                .iter()
                .any(|sample| matches!(sample.value.get(1), Some(value) if *value > 0))
        );
        backend.stop();
    }

    #[test]
    fn mimalloc_backend_reports_while_worker_threads_are_allocating() {
        let _guard = TEST_LOCK.lock().expect("lock mimalloc backend test");
        let mut backend = Profiler::start(MimallocConfig {
            sample_interval_bytes: 1024,
            ring_capacity: 65_536,
            report_drain_limit: 65_536,
            ..MimallocConfig::default()
        })
        .expect("initialize mimalloc backend");

        let worker_count = 4;
        let start = Arc::new(Barrier::new(worker_count + 1));
        let stop = Arc::new(AtomicBool::new(false));
        let workers: Vec<_> = (0..worker_count)
            .map(|worker| {
                let start = Arc::clone(&start);
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    start.wait();
                    let mut rounds = 0;
                    while !stop.load(Ordering::Acquire) || rounds < 64 {
                        let allocations: Vec<Vec<u8>> = (0..128)
                            .map(|iteration| {
                                vec![worker as u8; 256 + ((rounds + iteration) % 8) * 64]
                            })
                            .collect();
                        std::hint::black_box(&allocations);
                        rounds += 1;
                        if rounds >= 512 {
                            break;
                        }
                    }
                })
            })
            .collect();

        start.wait();
        let live_profiles: Vec<_> = (0..3).map(|_| report_profile(&mut backend)).collect();
        stop.store(true, Ordering::Release);
        for worker in workers {
            worker.join().expect("join allocation worker");
        }

        let final_profile = report_profile(&mut backend);
        let stats = mimalloc_stats();
        assert!(stats.recorded_samples > 0);
        assert!(stats.flushes > 0);
        assert!(
            live_profiles.iter().any(profile_has_alloc_space_sample)
                || profile_has_alloc_space_sample(&final_profile)
        );
        backend.stop();
    }

    #[test]
    fn live_heap_profiles_retained_and_freed_allocations_across_reports() {
        let _guard = TEST_LOCK.lock().expect("lock live heap test");
        let mut backend = live_backend();
        let retained = allocate_retained_live_test(2 * 1024 * 1024);
        std::hint::black_box(&retained);

        let first = report_profile(&mut backend);
        assert_eq!(
            first.string_table[first.default_sample_type as usize],
            "inuse_space"
        );
        assert_eq!(
            sample_value_for_frame(&first, "allocate_retained_live_test", "inuse_space"),
            retained.len() as i64
        );
        assert!(sample_value_for_frame(&first, "allocate_retained_live_test", "alloc_space") > 0);
        let second = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&second, "allocate_retained_live_test", "inuse_space"),
            retained.len() as i64
        );
        assert_eq!(
            sample_value_for_frame(&second, "allocate_retained_live_test", "alloc_space"),
            0
        );

        drop(retained);
        let freed = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&freed, "allocate_retained_live_test", "inuse_space"),
            0
        );
        backend.stop();
        assert_eq!(mimalloc_stats().live_samples, 0);
    }

    #[test]
    fn live_heap_tracks_zeroed_allocation_after_thread_exit_and_cross_thread_free() {
        let _guard = TEST_LOCK.lock().expect("lock cross-thread live test");
        let mut backend = live_backend();
        let size = 2 * 1024 * 1024;
        let pointer = std::thread::spawn(move || allocate_zeroed_live_test(size))
            .join()
            .expect("join allocating thread");
        let held = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&held, "allocate_zeroed_live_test", "inuse_space"),
            size as i64
        );

        std::thread::spawn(move || {
            // SAFETY: The allocating thread transferred ownership of this
            // pointer, and the layout matches its alloc_zeroed request.
            unsafe {
                dealloc(
                    pointer as *mut u8,
                    Layout::from_size_align(size, 8).unwrap(),
                )
            };
        })
        .join()
        .expect("join freeing thread");
        let freed = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&freed, "allocate_zeroed_live_test", "inuse_space"),
            0
        );
        backend.stop();
    }

    #[test]
    fn live_heap_maintains_physical_allocation_samples_through_reallocation_and_failure() {
        let _guard = TEST_LOCK.lock().expect("lock realloc live test");
        let mut backend = live_backend();
        let old_size = 2 * 1024 * 1024;
        let pointer = allocate_zeroed_live_test(old_size);
        let old_layout = Layout::from_size_align(old_size, 8).unwrap();
        let new_size = 4 * 1024 * 1024;
        // SAFETY: pointer is live and its layout matches the original request.
        let new_pointer = unsafe { resize_live_test(pointer as *mut u8, old_layout, new_size) };
        assert!(!new_pointer.is_null());
        let origin = if new_pointer == pointer as *mut u8 {
            "allocate_zeroed_live_test"
        } else {
            "resize_live_test"
        };
        let grown = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&grown, origin, "inuse_space"),
            new_size as i64
        );

        let new_layout = Layout::from_size_align(new_size, 8).unwrap();
        let samples_before_same_size = mimalloc_stats().recorded_samples;
        // SAFETY: The replacement is live with the layout supplied above.
        let same_pointer = unsafe { resize_live_test(new_pointer, new_layout, new_size) };
        assert_eq!(
            same_pointer, new_pointer,
            "same-size mimalloc realloc should retain its address"
        );
        assert_eq!(mimalloc_stats().recorded_samples, samples_before_same_size);
        let in_place = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&in_place, origin, "inuse_space"),
            new_size as i64
        );
        // SAFETY: This valid but unfulfillable request exercises the allocator's
        // null-return path without invoking Rust's handle_alloc_error.
        let failed = unsafe { realloc(new_pointer, new_layout, isize::MAX as usize / 2) };
        assert!(
            failed.is_null(),
            "expected address-space-sized realloc to fail"
        );
        let after_failure = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&after_failure, origin, "inuse_space"),
            new_size as i64
        );
        let shrunk_size = 1024 * 1024;
        // SAFETY: Failed realloc preserves new_pointer and its original layout.
        let shrunk_pointer = unsafe { resize_live_test(new_pointer, new_layout, shrunk_size) };
        assert!(!shrunk_pointer.is_null());
        let shrunk_origin = if shrunk_pointer == new_pointer {
            origin
        } else {
            "resize_live_test"
        };
        let shrunk = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&shrunk, shrunk_origin, "inuse_space"),
            shrunk_size as i64
        );
        // SAFETY: Successful realloc transfers ownership to shrunk_pointer.
        unsafe {
            dealloc(
                shrunk_pointer,
                Layout::from_size_align(shrunk_size, 8).unwrap(),
            )
        };
        let freed = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&freed, shrunk_origin, "inuse_space"),
            0
        );
        backend.stop();
    }

    #[test]
    fn live_heap_restart_excludes_objects_from_the_previous_session() {
        let _guard = TEST_LOCK.lock().expect("lock restart live heap test");
        let mut first = live_backend();
        let retained = allocate_retained_live_test(2 * 1024 * 1024);
        assert!(
            sample_value_for_frame(
                &report_profile(&mut first),
                "allocate_retained_live_test",
                "inuse_space"
            ) > 0
        );
        first.stop();
        let mut second = live_backend();
        assert_eq!(
            sample_value_for_frame(
                &report_profile(&mut second),
                "allocate_retained_live_test",
                "inuse_space"
            ),
            0
        );
        drop(retained);
        second.stop();
    }

    #[test]
    fn live_heap_stays_correct_during_concurrent_reports_and_cross_thread_frees() {
        let _guard = TEST_LOCK.lock().expect("lock concurrent live heap test");
        let mut backend = live_backend();
        let size = 2 * 1024 * 1024;
        let pointers: Vec<_> = (0..16).map(|_| allocate_zeroed_live_test(size)).collect();
        let start = Arc::new(Barrier::new(2));
        let worker_start = Arc::clone(&start);
        let worker = std::thread::spawn(move || {
            worker_start.wait();
            for pointer in pointers {
                // SAFETY: Each pointer is owned by this worker and was
                // allocated with the matching size/alignment.
                unsafe {
                    dealloc(
                        pointer as *mut u8,
                        Layout::from_size_align(size, 8).unwrap(),
                    )
                };
            }
        });
        start.wait();
        for _ in 0..4 {
            let _ = report_profile(&mut backend);
        }
        worker.join().expect("join concurrent freeing thread");
        let final_profile = report_profile(&mut backend);
        assert_eq!(
            sample_value_for_frame(&final_profile, "allocate_zeroed_live_test", "inuse_space"),
            0
        );
        backend.stop();
    }

    fn live_backend() -> Profiler {
        Profiler::start(MimallocConfig {
            sample_interval_bytes: 4096,
            live_heap_tracking: true,
            max_live_samples: 4096,
            ring_capacity: 4096,
            ..MimallocConfig::default()
        })
        .expect("initialize live heap backend")
    }

    #[inline(never)]
    fn allocate_retained_live_test(size: usize) -> Vec<u8> {
        std::hint::black_box(vec![0; size])
    }

    #[inline(never)]
    fn allocate_zeroed_live_test(size: usize) -> usize {
        // SAFETY: size is nonzero and the layout has valid alignment.
        let pointer = unsafe { alloc_zeroed(Layout::from_size_align(size, 8).unwrap()) };
        assert!(!pointer.is_null());
        std::hint::black_box(pointer as usize)
    }

    #[inline(never)]
    unsafe fn resize_live_test(pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: The caller supplies a live allocation and its matching layout.
        let pointer = unsafe { realloc(pointer, layout, size) };
        std::hint::black_box(pointer)
    }

    fn sample_value_for_frame(profile: &Profile, frame: &str, sample_type: &str) -> i64 {
        let value_index = profile
            .sample_type
            .iter()
            .position(|ty| profile.string_table[ty.r#type as usize] == sample_type)
            .expect("profile includes requested sample type");
        profile
            .sample
            .iter()
            .filter(|sample| {
                sample.location_id.iter().any(|id| {
                    profile
                        .location
                        .iter()
                        .filter(|location| location.id == *id)
                        .any(|location| {
                            location.line.iter().any(|line| {
                                profile
                                    .function
                                    .iter()
                                    .filter(|function| function.id == line.function_id)
                                    .any(|function| {
                                        profile.string_table[function.name as usize].contains(frame)
                                    })
                            })
                        })
                })
            })
            .map(|sample| sample.value[value_index])
            .sum()
    }

    #[test]
    #[ignore = "stress test for release validation; run with --ignored when validating mimalloc pressure"]
    fn mimalloc_backend_stress_tests_thread_matrix_and_drop_pressure() {
        let _guard = TEST_LOCK.lock().expect("lock mimalloc backend stress test");

        for worker_count in [1, 2, 4, 8, 16, 32] {
            let stats = run_thread_matrix_case(worker_count);
            assert!(
                stats.recorded_samples > 0,
                "expected samples for {worker_count} worker threads, got {stats:?}"
            );
            assert!(
                stats.flushes > 0,
                "expected flushes for {worker_count} worker threads, got {stats:?}"
            );
        }

        let drop_pressure_stats = run_drop_pressure_case();
        assert!(
            drop_pressure_stats.dropped_samples > 0,
            "expected dropped samples under constrained recorder capacity, got {drop_pressure_stats:?}"
        );
    }

    #[test]
    #[ignore = "live heap release validation across allocation/free thread pressure"]
    fn live_heap_stress_thread_matrix_leaves_no_stale_worker_allocations() {
        let _guard = TEST_LOCK.lock().expect("lock live heap stress test");
        for worker_count in [1, 2, 4, 8, 16, 32] {
            let mut backend = live_backend();
            run_allocation_workers(worker_count, 64, 64);
            let profile = report_profile(&mut backend);
            assert_eq!(
                sample_value_for_frame(&profile, "run_allocation_workers", "inuse_space"),
                0
            );
            assert!(mimalloc_stats().live_samples <= 4096);
            backend.stop();
            assert_eq!(mimalloc_stats().live_samples, 0);
        }
    }

    fn report_profile(backend: &mut Profiler) -> Profile {
        let bytes = backend.report().expect("report memory profile");
        Profile::decode(bytes.as_slice()).expect("decode memory pprof")
    }

    fn profile_has_alloc_space_sample(profile: &Profile) -> bool {
        profile
            .sample
            .iter()
            .any(|sample| matches!(sample.value.get(1), Some(value) if *value > 0))
    }

    fn run_thread_matrix_case(worker_count: usize) -> rustfs_mimalloc_profiling::MimallocStats {
        let mut backend = Profiler::start(MimallocConfig {
            sample_interval_bytes: 2048,
            ring_capacity: 65_536,
            report_drain_limit: 65_536,
            ..MimallocConfig::default()
        })
        .expect("initialize mimalloc backend");

        run_allocation_workers(worker_count, 128, 64);

        let profile = report_profile(&mut backend);
        assert!(
            profile_has_alloc_space_sample(&profile),
            "expected alloc_space sample for {worker_count} worker threads"
        );
        let stats = mimalloc_stats();
        backend.stop();
        stats
    }

    fn run_drop_pressure_case() -> rustfs_mimalloc_profiling::MimallocStats {
        let mut backend = Profiler::start(MimallocConfig {
            sample_interval_bytes: 1,
            ring_capacity: 8,
            report_drain_limit: 8,
            ..MimallocConfig::default()
        })
        .expect("initialize mimalloc backend");

        run_allocation_workers(4, 16, 128);

        let profile = report_profile(&mut backend);
        assert!(
            profile_has_alloc_space_sample(&profile),
            "expected alloc_space sample under drop pressure"
        );
        let stats = mimalloc_stats();
        backend.stop();
        stats
    }

    fn run_allocation_workers(worker_count: usize, rounds: usize, allocations_per_round: usize) {
        let workers: Vec<_> = (0..worker_count)
            .map(|worker| {
                std::thread::spawn(move || {
                    for round in 0..rounds {
                        let allocations: Vec<Vec<u8>> = (0..allocations_per_round)
                            .map(|iteration| {
                                vec![worker as u8; 256 + ((round + iteration) % 8) * 64]
                            })
                            .collect();
                        std::hint::black_box(&allocations);
                    }
                })
            })
            .collect();

        for worker in workers {
            worker.join().expect("join allocation worker");
        }
    }
}
