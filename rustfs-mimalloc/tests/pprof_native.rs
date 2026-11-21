//! Native pprof profiler lifecycle. Runs as its own test binary so no other
//! test allocates through the sampled global allocator while the profiler is
//! deleted (the native drop contract requires callback quiescence).

use rustfs_mimalloc::MiMalloc;
use rustfs_mimalloc::pprof::{PprofConfig, PprofProfiler};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[test]
fn native_pprof_profiler_writes_snapshots() {
    let base_path =
        std::env::temp_dir().join(format!("rustfs-mimalloc-pprof-test-{}", std::process::id()));
    let base = base_path.to_string_lossy().into_owned();
    // Snapshot files are named `<base>.NNNN.pb`: match against the file name
    // part only, not the directory-qualified path.
    let prefix = base_path
        .file_name()
        .expect("base must be a file path")
        .to_string_lossy()
        .into_owned();
    cleanup_snapshots(&prefix);

    let mut profiler =
        PprofProfiler::new(&base, PprofConfig::default()).expect("create native pprof profiler");
    // SAFETY: This binary completes sampled work before teardown and owns
    // every registration below. Custom subprocesses outlive their attachments.
    assert!(
        unsafe { profiler.attach() },
        "attach to the main subprocess heaps"
    );
    assert!(!profiler.start(), "a fresh profiler is not yet running");

    // Exercise C-side sampling with 64 MiB and a 512 KiB Poisson threshold.
    let mut sink: Vec<Vec<u8>> = Vec::new();
    for _ in 0..64 {
        sink.push(vec![0xAA_u8; 1024 * 1024]);
    }
    profiler.snapshot();
    // Freeing while attached exercises the native on_free callback.
    drop(sink);
    profiler.snapshot();

    assert!(profiler.stop(), "stop after sampling was running");
    profiler.detach();

    // Round-trip native running state and attachment generations in this
    // quiescent binary, including restart after detach and repeated attach.
    assert!(!profiler.start(), "restart after detach");
    assert!(profiler.stop(), "stop the restarted profiler");
    assert!(!profiler.stop(), "second stop reports not-running");
    assert!(unsafe { profiler.attach() }, "re-attach after stop");
    assert!(!profiler.start(), "attach stops before sampling starts");
    assert!(unsafe { profiler.attach() }, "repeat an active attachment");
    assert!(!profiler.start(), "reattachment stops the running profiler");
    assert!(profiler.stop(), "stop the sampled start");

    let mut contender = PprofProfiler::new(&base, PprofConfig::default()).unwrap();
    assert!(
        !unsafe { contender.attach() },
        "an occupied slot rejects another profiler"
    );

    let subproc = unsafe { rustfs_mimalloc_sys::mi_subproc_new() };
    let mut custom = PprofProfiler::new(&base, PprofConfig::default()).unwrap();
    assert!(unsafe { custom.attach_subproc(subproc) });
    custom.start();
    custom.detach();
    assert!(!custom.stop(), "detach also stops native sampling");
    assert!(
        !unsafe { contender.attach() },
        "custom detach preserves the main attachment"
    );
    assert!(
        unsafe { custom.attach_subproc(subproc) },
        "custom slot is available after detach"
    );
    assert!(
        !unsafe { custom.attach() },
        "retarget to an occupied main slot fails"
    );
    assert!(
        unsafe { custom.attach_subproc(subproc) },
        "failed retarget released the prior slot"
    );
    drop(custom);
    // A failed retarget must not have recorded the main slot: if Drop had
    // detached it, the profiler's registration would be gone and this
    // contender could take the slot.
    assert!(
        !unsafe { contender.attach() },
        "failed retarget preserves the profiler's main attachment"
    );
    assert!(
        unsafe { contender.attach_subproc(subproc) },
        "Drop detaches the custom subprocess"
    );
    contender.detach();
    unsafe { rustfs_mimalloc_sys::mi_subproc_destroy(subproc) };

    drop(profiler);
    assert!(
        unsafe { contender.attach() },
        "Drop releases the main registration"
    );
    drop(contender);

    let snapshots = collect_snapshots(&prefix);
    assert!(
        snapshots >= 2,
        "two explicit snapshots must be written, found {snapshots}"
    );
    cleanup_snapshots(&prefix);
}

fn collect_snapshots(prefix: &str) -> usize {
    std::fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .count()
}

fn cleanup_snapshots(prefix: &str) {
    for entry in std::fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            drop(std::fs::remove_file(entry.path()));
        }
    }
}
