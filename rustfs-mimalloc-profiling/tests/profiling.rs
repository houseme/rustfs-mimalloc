use rustfs_mimalloc_profiling::SamplingAllocator;

#[global_allocator]
static ALLOC: SamplingAllocator<std::alloc::System> =
    SamplingAllocator::with_allocator(std::alloc::System);

#[path = "support/profiling.rs"]
mod tests;
