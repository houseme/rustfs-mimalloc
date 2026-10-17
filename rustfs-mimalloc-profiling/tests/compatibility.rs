#![cfg(feature = "rustfs-allocator")]

use rustfs_mimalloc_profiling::{MimallocConfig, Profiler, SamplingMiMalloc};

#[global_allocator]
static ALLOC: SamplingMiMalloc = SamplingMiMalloc::new();

#[test]
fn rustfs_convenience_constructor_and_default_share_the_generic_recorder() {
    let _allocator = SamplingMiMalloc::default();
    let mut profiler = Profiler::start(MimallocConfig::default()).unwrap();
    let retained = std::hint::black_box(vec![0_u8; 2 * 1024 * 1024]);
    assert!(!profiler.report().unwrap().is_empty());
    std::hint::black_box(&retained);
    profiler.stop();
}
