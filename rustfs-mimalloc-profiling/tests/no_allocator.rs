use rustfs_mimalloc_profiling::{Error, MimallocConfig, Profiler};

#[cfg(not(feature = "no_hooks"))]
#[test]
fn initialization_rejects_a_missing_sampling_allocator() {
    assert!(matches!(
        Profiler::start(MimallocConfig::default()),
        Err(Error::AllocatorNotObserved)
    ));
}

#[cfg(feature = "no_hooks")]
#[test]
fn no_hooks_rejects_sessions_even_without_a_sampling_allocator() {
    assert!(matches!(
        Profiler::start(MimallocConfig::default()),
        Err(Error::InvalidConfig(_))
    ));
}
