//! Sampled allocation and optional live heap profiling for mimalloc.
//!
//! Install [`SamplingAllocator`] as the global allocator, then own a [`Profiler`]
//! for each recording session. Reports are uncompressed pprof protobuf bytes.
//! There is no uploader, background thread, or dependency on a profiling service.
//! The application-level `no_hooks` feature disables observation for the entire
//! feature-unified dependency graph; [`Profiler::start`] then rejects sessions.
//!
//! ```no_run
//! use rustfs_mimalloc_profiling::{MimallocConfig, Profiler, SamplingAllocator};
//!
//! #[global_allocator]
//! static ALLOC: SamplingAllocator<std::alloc::System> =
//!     SamplingAllocator::with_allocator(std::alloc::System);
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut profiler = Profiler::start(MimallocConfig {
//!     live_heap_tracking: true,
//!     ..MimallocConfig::default()
//! })?;
//! let retained = vec![0_u8; 2 * 1024 * 1024];
//! std::hint::black_box(&retained);
//! let bytes = profiler.report()?;
//! profiler.stop();
//! std::fs::write("heap.pprof", bytes)?;
//! # Ok(())
//! # }
//! ```
//!
//! Only Rust allocations through the wrapper after session initialization are
//! observed. Requested byte sizes are sampling-weighted, not process RSS or
//! allocator usable sizes. Live tracking is opt-in and bounded; inspect
//! [`mimalloc_stats`] for dropped records. See the crate README for platform,
//! concurrency, and performance limitations.
//!
//! # Relation to the native pprof profiler
//! This crate stays allocator-neutral and captures stacks on the Rust side
//! per sampled allocation. When the linked allocator is
//! [`rustfs-mimalloc`](https://crates.io/crates/rustfs-mimalloc), the native
//! mimalloc pprof profiler is a lower-overhead alternative: it samples with
//! C-side stack capture and writes protobuf snapshots directly — see
//! `rustfs_mimalloc::pprof::PprofProfiler` and
//! `rustfs_mimalloc::callback`. Choose this crate when you need
//! allocator-neutral instrumentation, live-heap tracking across arbitrary
//! allocators, or programmatic report bytes; choose the native profiler for
//! minimal sampling cost against mimalloc itself.

mod error;
mod memory_pprof;
mod profiler;

/// Standard pprof protobuf types for decoding or consuming reports with `prost`.
pub mod pprof;

pub use error::Error;
pub use profiler::{
    MimallocConfig, MimallocStackCapture, MimallocStats, Profiler, SamplingAllocator,
    last_report_elapsed_micros, mimalloc_stats,
};

#[cfg(feature = "rustfs-allocator")]
pub use profiler::SamplingMiMalloc;

/// Result of starting a profiling session or collecting a report.
pub type Result<T> = std::result::Result<T, Error>;
