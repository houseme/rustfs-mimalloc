use prost::Message;
use rustfs_mimalloc_profiling::{
    MimallocConfig, MimallocStackCapture, Profiler,
    pprof::{Profile, Sample},
};
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn portable_profile_preserves_physical_stacks_and_live_lifecycle() {
    check_profile(MimallocStackCapture::Portable);
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn native_profile_preserves_physical_stacks_and_live_lifecycle() {
    check_profile(MimallocStackCapture::Native);
}

fn check_profile(stack_capture: MimallocStackCapture) {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut backend = Profiler::start(MimallocConfig {
        sample_interval_bytes: 1024,
        stack_capture,
        live_heap_tracking: true,
        ..MimallocConfig::default()
    })
    .unwrap();
    // Large enough for inclusion probability to round to one. No symbols or
    // source lines are needed to identify this allocation in a stripped binary.
    let retained = allocate_retained_marker(2 * 1024 * 1024);
    let profile = report_profile(&mut backend);
    let sample_types: Vec<_> = profile
        .sample_type
        .iter()
        .map(|kind| profile.string_table[kind.r#type as usize].as_str())
        .collect();
    assert_eq!(
        sample_types,
        [
            "alloc_objects",
            "alloc_space",
            "inuse_objects",
            "inuse_space"
        ]
    );
    assert_eq!(
        profile.string_table[profile.default_sample_type as usize],
        "inuse_space"
    );
    let retained_sample = profile
        .sample
        .iter()
        .find(|sample| sample.value[3] >= retained.len() as i64)
        .expect("retained allocation in live heap snapshot");
    assert_eq!(retained_sample.value[2], 1);
    let addresses = stack_addresses(&profile, retained_sample);
    assert!(addresses.len() >= 2, "physical call chain is missing");
    assert!(addresses.iter().all(|address| *address != 0));
    if std::env::var("MIMALLOC_TEST_EXPECT_SOURCE").as_deref() == Ok("1") {
        let function = profile
            .function
            .iter()
            .find(|function| {
                profile.string_table[function.name as usize].contains("allocate_retained_marker")
            })
            .expect("allocation helper symbol in source-enabled release");
        assert!(profile.string_table[function.filename as usize].ends_with("mimalloc_release.rs"));
        assert!(retained_sample.location_id.iter().any(|id| {
            profile.location.iter().any(|location| {
                location.id == *id
                    && location
                        .line
                        .iter()
                        .any(|line| line.function_id == function.id && line.line > 0)
            })
        }));
    }
    // Mach-O linker headers are not functions. Publishing these names would
    // hide unknown frames from an offline symbolizer in stripped builds.
    assert!(!profile.string_table.iter().any(|name| {
        matches!(
            name.as_str(),
            "__mh_execute_header"
                | "__mh_dylib_header"
                | "__mh_bundle_header"
                | "__mh_object_header"
                | "__mh_preload_header"
                | "__mh_dylinker_header"
        )
    }));
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
    {
        let main = profile.mapping.first().expect("main executable mapping");
        assert!(!profile.string_table[main.build_id as usize].is_empty());
        assert!(retained_sample.location_id.iter().any(|id| {
            let location = profile
                .location
                .iter()
                .find(|location| location.id == *id)
                .unwrap();
            location.mapping_id == main.id
                && location.address >= main.memory_start
                && location.address < main.memory_limit
        }));
        for mapping in &profile.mapping {
            if profile
                .location
                .iter()
                .any(|location| location.mapping_id == mapping.id && location.line.is_empty())
            {
                assert!(!mapping.has_functions);
                assert!(!mapping.has_filenames);
                assert!(!mapping.has_line_numbers);
            }
        }
    }
    std::thread::spawn(move || drop(retained)).join().unwrap();
    let freed = report_profile(&mut backend);
    // IDs are report-local, so compare physical addresses across snapshots.
    assert!(
        !freed
            .sample
            .iter()
            .any(|sample| { sample.value[3] > 0 && stack_addresses(&freed, sample) == addresses })
    );
    backend.stop();
}

#[inline(never)]
fn allocate_retained_marker(size: usize) -> Vec<u8> {
    let retained = vec![0; size];
    std::hint::black_box(&retained);
    retained
}

fn report_profile(backend: &mut Profiler) -> Profile {
    let bytes = backend.report().unwrap();
    Profile::decode(bytes.as_slice()).unwrap()
}

fn stack_addresses(profile: &Profile, sample: &Sample) -> Vec<u64> {
    sample
        .location_id
        .iter()
        .map(|id| {
            profile
                .location
                .iter()
                .find(|location| location.id == *id)
                .unwrap()
                .address
        })
        .collect()
}
