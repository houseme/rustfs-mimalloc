//! Sampled allocation and optional live heap profiling for mimalloc.
//!
//! Install [`SamplingMiMalloc`] as the global allocator, then own a [`Profiler`]
//! for each recording session. Reports are uncompressed pprof protobuf bytes.
//! There is no uploader, background thread, or dependency on a profiling service.
//!
//! ```no_run
//! use rustfs_mimalloc_profiling::{MimallocConfig, Profiler, SamplingMiMalloc};
//!
//! #[global_allocator]
//! static ALLOC: SamplingMiMalloc = SamplingMiMalloc::new();
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

mod error;
mod memory_pprof;
mod profiler;

/// Standard pprof protobuf types for decoding or consuming reports with `prost`.
pub mod pprof;

pub use error::Error;
pub use profiler::{
    MimallocConfig, MimallocStackCapture, MimallocStats, Profiler, SamplingMiMalloc, mimalloc_stats,
};

/// Result of starting a profiling session or collecting a report.
pub type Result<T> = std::result::Result<T, Error>;
