//! One inclusion-probability estimator for allocation events and live objects.
//!
//! For size S and average byte interval R, inclusion probability is
//! 1 - exp(-S/R). Each included allocation represents 1/p objects and S/p bytes.
//! Keep fractions until stack aggregation; per-event integer division biases
//! object counts. Exponential memorylessness allows one fresh interval at the
//! end of a sampled allocation, independent of the number of crossed intervals.

use super::{RNG_INCREMENT, splitmix64};

#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct SampleWeight {
    pub(super) weighted_objects: f64,
    pub(super) weighted_bytes: f64,
}

impl SampleWeight {
    pub(super) fn for_allocation(size: u64, interval: u64) -> Self {
        let probability = -(-(size as f64) / interval.max(1) as f64).exp_m1();
        Self {
            weighted_objects: (1.0 / probability).max(1.0),
            weighted_bytes: (size as f64 / probability).max(size as f64),
        }
    }
}

pub(super) fn next_poisson_interval(interval: u64, rng: &mut u64) -> u64 {
    *rng = rng.wrapping_add(RNG_INCREMENT);
    let mantissa = (splitmix64(*rng) >> 11).max(1);
    let uniform = mantissa as f64 * (1.0 / ((1_u64 << 53) as f64));
    let next = -uniform.ln() * interval.max(1) as f64;
    // Float casts saturate, so even u64::MAX intervals stay bounded.
    (next.ceil() as u64).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inclusion_weights_cover_small_large_and_extreme_allocations() {
        let small = SampleWeight::for_allocation(1, 1024);
        assert!((small.weighted_objects - 1024.5).abs() < 0.001);
        assert_eq!(small.weighted_objects, small.weighted_bytes);
        let large = SampleWeight::for_allocation(1024 * 1024, 1024);
        assert_eq!(large.weighted_objects, 1.0);
        assert_eq!(large.weighted_bytes, (1024 * 1024) as f64);
        for (size, interval) in [(1, u64::MAX), (u64::MAX, 1), (u64::MAX, u64::MAX)] {
            let weight = SampleWeight::for_allocation(size, interval);
            assert!(weight.weighted_objects.is_finite());
            assert!(weight.weighted_bytes.is_finite());
            assert!(weight.weighted_objects >= 1.0);
            assert!(weight.weighted_bytes >= size as f64);
        }
    }

    #[test]
    fn allocation_estimates_do_not_truncate_individual_sample_weights() {
        for (size, interval) in [
            (1, 1),
            (3, 2),
            (64, 1024),
            (768, 1024),
            (1536, 1024),
            (8192, 1024),
        ] {
            let allocations = 500_000;
            let mut rng = 42;
            let mut remaining = next_poisson_interval(interval, &mut rng);
            let mut objects = 0.0;
            let mut bytes = 0.0;
            for _ in 0..allocations {
                if size < remaining {
                    remaining -= size;
                } else {
                    remaining = next_poisson_interval(interval, &mut rng);
                    let weight = SampleWeight::for_allocation(size, interval);
                    objects += weight.weighted_objects;
                    bytes += weight.weighted_bytes;
                }
            }
            let object_error = (objects / allocations as f64 - 1.0).abs();
            let byte_error = (bytes / (allocations as f64 * size as f64) - 1.0).abs();
            assert!(
                object_error < 0.02 && byte_error < 0.02,
                "size={size} interval={interval}: objects={objects}, bytes={bytes}, errors={object_error}/{byte_error}"
            );
        }
    }

    #[test]
    fn intervals_are_positive_and_advance_the_rng_once() {
        for interval in [1, 1024, u64::MAX] {
            let mut rng = 0;
            assert!(next_poisson_interval(interval, &mut rng) > 0);
            assert_eq!(rng, RNG_INCREMENT);
        }
    }
}
