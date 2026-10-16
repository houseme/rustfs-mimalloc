use rustfs_mimalloc_profiling::{MimallocConfig, Profiler, SamplingMiMalloc};

#[global_allocator]
static ALLOC: SamplingMiMalloc = SamplingMiMalloc::new();

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
