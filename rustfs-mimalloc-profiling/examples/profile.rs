use rustfs_mimalloc_profiling::{MimallocConfig, Profiler, SamplingAllocator};

#[global_allocator]
static ALLOC: SamplingAllocator<std::alloc::System> =
    SamplingAllocator::with_allocator(std::alloc::System);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut profiler = Profiler::start(MimallocConfig {
        live_heap_tracking: true,
        ..MimallocConfig::default()
    })?;
    let retained = vec![0_u8; 2 * 1024 * 1024];
    std::hint::black_box(&retained);
    let report = profiler.report()?;
    profiler.stop();
    std::fs::write("heap.pprof", report)?;
    Ok(())
}
