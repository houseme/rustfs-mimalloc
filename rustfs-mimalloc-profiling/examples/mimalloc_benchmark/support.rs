use std::{
    env,
    time::{Duration, Instant},
};

#[derive(Debug, Copy, Clone)]
pub struct WorkloadConfig {
    pub duration: Duration,
    pub batch_size: usize,
    pub min_size: usize,
    pub max_size: usize,
    pub size_step: usize,
    pub latency_sample_interval: u64,
    pub latency_sample_limit: usize,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct LatencyPercentiles {
    pub p50_nanos: u128,
    pub p95_nanos: u128,
    pub p99_nanos: u128,
}

#[derive(Debug, Copy, Clone)]
pub struct WorkloadResult {
    pub elapsed: Duration,
    pub allocations: u64,
    pub bytes: u64,
    pub latency_percentiles: Option<LatencyPercentiles>,
    pub latency_samples: usize,
    pub latency_sample_min_size: usize,
    pub latency_sample_max_size: usize,
}

struct LatencySampler {
    interval: u64,
    remaining: usize,
    window: u64,
    rng: u64,
    next_index: Option<u64>,
}

impl LatencySampler {
    fn new(interval: u64, limit: usize) -> Self {
        let mut sampler = Self {
            interval,
            remaining: limit,
            window: 0,
            rng: 0xa076_1d64_78bd_642f,
            next_index: None,
        };
        if interval > 0 && limit > 0 {
            sampler.schedule();
        }
        sampler
    }

    fn schedule(&mut self) {
        // Select one deterministic pseudorandom position in each window. A
        // fixed cadence aliases the default 1024-allocation size cycle.
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut random = self.rng;
        random = (random ^ (random >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        random = (random ^ (random >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        random ^= random >> 31;
        self.next_index = self
            .window
            .checked_mul(self.interval)
            .and_then(|start| start.checked_add(random % self.interval));
    }

    fn should_sample(&mut self, allocation_index: u64) -> bool {
        if self.next_index != Some(allocation_index) {
            return false;
        }
        self.remaining -= 1;
        if self.remaining == 0 {
            self.next_index = None;
        } else if let Some(window) = self.window.checked_add(1) {
            self.window = window;
            self.schedule();
        } else {
            self.next_index = None;
        }
        true
    }
}

impl WorkloadConfig {
    pub fn from_env() -> Self {
        Self {
            duration: Duration::from_millis(read_env_u64("MIMALLOC_BENCH_DURATION_MS", 3_000)),
            batch_size: read_env_usize("MIMALLOC_BENCH_BATCH_SIZE", 1024).max(1),
            min_size: read_env_usize("MIMALLOC_BENCH_MIN_SIZE", 64).max(1),
            max_size: read_env_usize("MIMALLOC_BENCH_MAX_SIZE", 64 * 1024).max(1),
            size_step: read_env_usize("MIMALLOC_BENCH_SIZE_STEP", 64).max(1),
            latency_sample_interval: read_env_u64("MIMALLOC_BENCH_LATENCY_SAMPLE_INTERVAL", 1024),
            latency_sample_limit: read_env_usize("MIMALLOC_BENCH_LATENCY_SAMPLE_LIMIT", 4096),
        }
    }

    fn next_size(&self, allocation_index: u64) -> usize {
        let span = self.max_size.saturating_sub(self.min_size);
        if span == 0 {
            return self.min_size;
        }

        let slots = span / self.size_step + 1;
        self.min_size + (allocation_index as usize % slots) * self.size_step
    }
}

pub fn run_workload(config: WorkloadConfig) -> WorkloadResult {
    let mut sampler =
        LatencySampler::new(config.latency_sample_interval, config.latency_sample_limit);
    let mut allocations = 0_u64;
    let mut bytes = 0_u64;
    let mut latency_samples = Vec::with_capacity(if config.latency_sample_interval > 0 {
        config.latency_sample_limit
    } else {
        0
    });
    let mut latency_sample_min_size = usize::MAX;
    let mut latency_sample_max_size = 0;
    let start = Instant::now();

    while start.elapsed() < config.duration {
        for _ in 0..config.batch_size {
            let size = config.next_size(allocations);
            if sampler.should_sample(allocations) {
                let allocation_start = Instant::now();
                let allocation = vec![0_u8; size];
                std::hint::black_box(&allocation);
                latency_samples.push(allocation_start.elapsed().as_nanos());
                latency_sample_min_size = latency_sample_min_size.min(size);
                latency_sample_max_size = latency_sample_max_size.max(size);
            } else {
                let allocation = vec![0_u8; size];
                std::hint::black_box(&allocation);
            }
            allocations = allocations.saturating_add(1);
            bytes = bytes.saturating_add(size as u64);
        }
    }

    let sample_count = latency_samples.len();
    WorkloadResult {
        elapsed: start.elapsed(),
        allocations,
        bytes,
        latency_percentiles: calculate_latency_percentiles(latency_samples),
        latency_samples: sample_count,
        latency_sample_min_size: if sample_count == 0 {
            0
        } else {
            latency_sample_min_size
        },
        latency_sample_max_size,
    }
}

pub fn print_workload(label: &str, config: WorkloadConfig, result: WorkloadResult) {
    let elapsed_secs = result.elapsed.as_secs_f64();
    let allocations_per_sec = result.allocations as f64 / elapsed_secs;
    let mib_per_sec = result.bytes as f64 / elapsed_secs / 1024.0 / 1024.0;

    println!("label={label}");
    println!("duration_ms={}", config.duration.as_millis());
    println!("batch_size={}", config.batch_size);
    println!("min_size={}", config.min_size);
    println!("max_size={}", config.max_size);
    println!("size_step={}", config.size_step);
    println!("latency_sample_interval={}", config.latency_sample_interval);
    println!("latency_sample_limit={}", config.latency_sample_limit);
    println!("latency_sampling_policy=stratified_v1");
    println!("elapsed_ms={}", result.elapsed.as_millis());
    println!("allocations={}", result.allocations);
    println!("bytes={}", result.bytes);
    println!("allocations_per_sec={allocations_per_sec:.2}");
    println!("mib_per_sec={mib_per_sec:.2}");
    println!("allocation_latency_samples={}", result.latency_samples);
    println!(
        "allocation_latency_min_size={}",
        result.latency_sample_min_size
    );
    println!(
        "allocation_latency_max_size={}",
        result.latency_sample_max_size
    );
    if let Some(percentiles) = result.latency_percentiles {
        println!("allocation_latency_p50_ns={}", percentiles.p50_nanos);
        println!("allocation_latency_p95_ns={}", percentiles.p95_nanos);
        println!("allocation_latency_p99_ns={}", percentiles.p99_nanos);
    }
}

fn calculate_latency_percentiles(mut samples: Vec<u128>) -> Option<LatencyPercentiles> {
    if samples.is_empty() {
        return None;
    }

    samples.sort_unstable();
    Some(LatencyPercentiles {
        p50_nanos: percentile(&samples, 50),
        p95_nanos: percentile(&samples, 95),
        p99_nanos: percentile(&samples, 99),
    })
}

fn percentile(sorted_samples: &[u128], percentile: usize) -> u128 {
    let index = sorted_samples
        .len()
        .saturating_mul(percentile)
        .saturating_add(99)
        .checked_div(100)
        .unwrap_or_default()
        .saturating_sub(1)
        .min(sorted_samples.len() - 1);
    sorted_samples[index]
}

fn read_env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn read_env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn stratified_latency_samples_cover_the_default_size_cycle() {
        let mut sampler = LatencySampler::new(1024, 1024);
        let config = WorkloadConfig {
            duration: Duration::ZERO,
            batch_size: 1024,
            min_size: 64,
            max_size: 65536,
            size_step: 64,
            latency_sample_interval: 1024,
            latency_sample_limit: 1024,
        };
        let mut sizes = HashSet::new();
        let mut windows = HashSet::new();
        for index in 0..1024 * 1024 {
            if sampler.should_sample(index) {
                sizes.insert(config.next_size(index));
                assert!(windows.insert(index / 1024), "one sample per window");
            }
        }
        assert_eq!(windows.len(), 1024);
        assert!(
            sizes.len() > 600,
            "samples must cover more than the smallest size class"
        );
        assert_eq!(sampler.next_index, None);
    }

    #[test]
    fn latency_sampling_respects_disabled_limit_and_overflow_cases() {
        for (interval, limit) in [(0, 4), (4, 0)] {
            let mut sampler = LatencySampler::new(interval, limit);
            assert!(!(0..10).any(|index| sampler.should_sample(index)));
        }
        let mut sampler = LatencySampler::new(1, 2);
        assert!(sampler.should_sample(0));
        assert!(sampler.should_sample(1));
        assert!(!sampler.should_sample(2));
        let mut sampler = LatencySampler::new(u64::MAX, 2);
        assert!(sampler.should_sample(sampler.next_index.unwrap()));
        assert_eq!(sampler.next_index, None);
    }
}
