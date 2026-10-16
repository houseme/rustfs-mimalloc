use rustfs_mimalloc_profiling::{Error, MimallocConfig, Profiler};

#[test]
fn initialization_rejects_a_missing_sampling_allocator() {
    assert!(matches!(
        Profiler::start(MimallocConfig::default()),
        Err(Error::AllocatorNotObserved)
    ));
}
