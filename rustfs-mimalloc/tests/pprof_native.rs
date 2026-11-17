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
    assert!(profiler.attach(), "attach to the main subprocess heaps");
    assert!(!profiler.start(), "a fresh profiler is not yet running");

    // 64 MiB against a 512 KiB Poisson threshold guarantees sampled
    // allocations reach the native on_alloc callback.
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

    drop(profiler);

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
