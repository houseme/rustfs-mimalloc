//! Baseline mimalloc allocation workload for local overhead comparisons.
//!
//! ```sh
//! cargo run --release --example mimalloc_baseline -p rustfs-mimalloc-profiling
//! ```

#[path = "mimalloc_benchmark/support.rs"]
mod support;

use support::{WorkloadConfig, print_workload, run_workload};

#[global_allocator]
static ALLOC: rustfs_mimalloc::MiMalloc = rustfs_mimalloc::MiMalloc;

fn main() {
    let config = WorkloadConfig::from_env();
    let result = run_workload(config);
    print_workload("mimalloc_baseline", config, result);
}
