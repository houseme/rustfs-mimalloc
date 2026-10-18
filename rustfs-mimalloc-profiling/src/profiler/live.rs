//! Bounded sampled-pointer tracking. Each pointer belongs to one shard, so
//! cross-thread frees have the same ownership rules as same-thread frees.

use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use super::{
    SAMPLING_CONFIG_GENERATION, SampleWeight, StackKey, session_is_current, splitmix64,
    with_profiler_suppressed,
};

const SHARD_COUNT: usize = 64;
static ENABLED: AtomicBool = AtomicBool::new(false);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static METADATA_PAYLOAD_BYTES: AtomicUsize = AtomicUsize::new(0);
static SHARDS: OnceLock<[LiveShard; SHARD_COUNT]> = OnceLock::new();

// Keep frequently written occupancy counters on separate cache lines.
#[repr(align(64))]
struct LiveShard {
    entries: Mutex<LiveMap>,
    count: AtomicUsize,
    membership: AtomicU64,
}

struct LiveMap {
    samples: HashMap<usize, LiveEntry>,
    capacity: usize,
    // Exact bit reference counts let the atomic filter reject most untracked
    // frees without a mutex lookup, while never dropping a tracked removal.
    membership_counts: [usize; 64],
}

#[derive(Copy, Clone)]
struct LiveEntry {
    sample: LiveAllocationSample,
    // Retaining the entry reserves rollback storage and prevents an address
    // reused by another thread from replacing metadata before realloc completes.
    reallocating: bool,
}

#[derive(Debug, Copy, Clone)]
pub(super) struct LiveAllocationSample {
    pub(super) stack: StackKey,
    // Preserve fractional weights until per-stack aggregation. Rounding each
    // pointer's object count would systematically bias similarly sized objects.
    pub(super) weighted_objects: f64,
    pub(super) weighted_bytes: f64,
}

pub(super) fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

pub(super) fn prepare(enabled: bool, capacity: usize) {
    clear();
    DROPPED.store(0, Ordering::Relaxed);
    if !enabled {
        return;
    }
    let shards = SHARDS.get_or_init(|| {
        std::array::from_fn(|_| LiveShard {
            entries: Mutex::new(LiveMap {
                samples: HashMap::new(),
                capacity: 0,
                membership_counts: [0; 64],
            }),
            count: AtomicUsize::new(0),
            membership: AtomicU64::new(0),
        })
    });
    let mut payload_bytes = 0_usize;
    for (index, shard) in shards.iter().enumerate() {
        let mut entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
        let shard_capacity = capacity / SHARD_COUNT + usize::from(index < capacity % SHARD_COUNT);
        entries.samples = HashMap::with_capacity(shard_capacity);
        payload_bytes = payload_bytes.saturating_add(
            entries
                .samples
                .capacity()
                .saturating_mul(std::mem::size_of::<(usize, LiveEntry)>()),
        );
        entries.capacity = shard_capacity;
        entries.membership_counts.fill(0);
        shard.count.store(0, Ordering::Release);
        shard.membership.store(0, Ordering::Release);
    }
    METADATA_PAYLOAD_BYTES.store(payload_bytes, Ordering::Relaxed);
    ENABLED.store(true, Ordering::Release);
}

pub(super) fn clear() {
    ENABLED.store(false, Ordering::Release);
    if let Some(shards) = SHARDS.get() {
        for shard in shards {
            let mut entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
            entries.samples = HashMap::new();
            entries.capacity = 0;
            entries.membership_counts.fill(0);
            shard.count.store(0, Ordering::Release);
            shard.membership.store(0, Ordering::Release);
        }
    }
    METADATA_PAYLOAD_BYTES.store(0, Ordering::Relaxed);
}

fn shard_and_bit(pointer: usize) -> (usize, usize) {
    let hash = splitmix64(pointer as u64);
    (hash as usize % SHARD_COUNT, (hash >> 6) as usize % 64)
}

impl LiveMap {
    fn has_capacity(&self) -> bool {
        // Tombstones can reduce HashMap's insertion capacity after churn.
        // Never let an allocator hook grow or rebuild the backing table.
        self.samples.len() < self.capacity && self.samples.len() < self.samples.capacity()
    }
    fn add_membership(&mut self, shard: &LiveShard, bit: usize) {
        self.membership_counts[bit] += 1;
        shard.membership.fetch_or(1_u64 << bit, Ordering::Release);
        shard.count.fetch_add(1, Ordering::Relaxed);
    }

    fn remove_membership(&mut self, shard: &LiveShard, bit: usize) {
        self.membership_counts[bit] -= 1;
        if self.membership_counts[bit] == 0 {
            shard
                .membership
                .fetch_and(!(1_u64 << bit), Ordering::Release);
        }
        shard.count.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(super) fn record(pointer: usize, stack: StackKey, weight: SampleWeight, generation: u64) {
    let Some(shards) = SHARDS.get() else {
        return;
    };
    let (index, bit) = shard_and_bit(pointer);
    let shard = &shards[index];
    let Ok(mut entries) = shard.entries.try_lock() else {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if !enabled() || !session_is_current(generation) {
        return;
    }
    if !entries.has_capacity() || entries.samples.contains_key(&pointer) {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    entries.samples.insert(
        pointer,
        LiveEntry {
            sample: LiveAllocationSample {
                stack,
                weighted_objects: weight.weighted_objects,
                weighted_bytes: weight.weighted_bytes,
            },
            reallocating: false,
        },
    );
    entries.add_membership(shard, bit);
}

pub(super) fn remove(pointer: usize) {
    let Some(shards) = SHARDS.get() else {
        return;
    };
    let (index, bit) = shard_and_bit(pointer);
    let shard = &shards[index];
    if shard.membership.load(Ordering::Acquire) & (1_u64 << bit) == 0 {
        return;
    }
    // Dropping a removal on contention would fabricate a live allocation.
    // This short critical section never allocates or resolves symbols.
    let mut entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
    // A different allocation may already reuse the address freed by mimalloc
    // during an in-flight realloc. That allocation was not admitted to this
    // reserved slot, and must not remove the original realloc token's entry.
    if entries
        .samples
        .get(&pointer)
        .is_some_and(|entry| !entry.reallocating)
    {
        entries.samples.remove(&pointer);
        entries.remove_membership(shard, bit);
    }
}

pub(super) struct PendingReallocation {
    pointer: usize,
    generation: u64,
    finished: bool,
}

pub(super) fn begin_reallocation(pointer: usize) -> Option<PendingReallocation> {
    let shards = SHARDS.get()?;
    let (index, bit) = shard_and_bit(pointer);
    let shard = &shards[index];
    if shard.membership.load(Ordering::Acquire) & (1_u64 << bit) == 0 {
        return None;
    }
    let mut entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
    let entry = entries.samples.get_mut(&pointer)?;
    if entry.reallocating {
        return None;
    }
    entry.reallocating = true;
    Some(PendingReallocation {
        pointer,
        generation: SAMPLING_CONFIG_GENERATION.load(Ordering::Acquire),
        finished: false,
    })
}

impl PendingReallocation {
    pub(super) fn finish(
        mut self,
        new_pointer: usize,
        new_size: u64,
    ) -> Option<(LiveAllocationSample, u64)> {
        self.finished = true;
        let shards = SHARDS.get()?;
        let (index, bit) = shard_and_bit(self.pointer);
        let shard = &shards[index];
        let mut entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
        if !enabled() || !session_is_current(self.generation) {
            return None;
        }
        let entry = entries.samples.get_mut(&self.pointer)?;
        if new_pointer == 0 || new_pointer == self.pointer {
            entry.reallocating = false;
            if new_pointer != 0 {
                entry.sample.weighted_bytes = entry.sample.weighted_objects * new_size as f64;
            }
            return None;
        }
        let entry = entries.samples.remove(&self.pointer)?;
        let mut sample = entry.sample;
        entries.remove_membership(shard, bit);
        drop(entries);
        sample.weighted_bytes = sample.weighted_objects * new_size as f64;
        Some((sample, self.generation))
    }
}

impl Drop for PendingReallocation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        with_profiler_suppressed(|| {
            let Some(shards) = SHARDS.get() else {
                return;
            };
            let shard = &shards[shard_and_bit(self.pointer).0];
            let mut entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
            if enabled()
                && session_is_current(self.generation)
                && let Some(entry) = entries.samples.get_mut(&self.pointer)
            {
                entry.reallocating = false;
            }
        });
    }
}

pub(super) fn transfer_sample(pointer: usize, sample: LiveAllocationSample, generation: u64) {
    let Some(shards) = SHARDS.get() else {
        return;
    };
    let (index, bit) = shard_and_bit(pointer);
    let shard = &shards[index];
    let mut entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
    if !enabled() || !session_is_current(generation) {
        return;
    }
    if !entries.has_capacity() || entries.samples.contains_key(&pointer) {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    entries.samples.insert(
        pointer,
        LiveEntry {
            sample,
            reallocating: false,
        },
    );
    entries.add_membership(shard, bit);
}

pub(super) fn snapshot() -> Vec<LiveAllocationSample> {
    if !enabled() {
        return Vec::new();
    }
    let Some(shards) = SHARDS.get() else {
        return Vec::new();
    };
    // Occupancy is only a sizing hint: workers can add/remove records while
    // reporting. Stable empty/sparse heaps need no whole-shard reservation.
    let mut samples = Vec::with_capacity(sample_count());
    for shard in shards {
        // Allocate the output storage before taking a shard lock. Samples are
        // copied under the lock, with aggregation/symbolization done afterward.
        let (capacity, rebuild) = {
            let entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
            (
                entries.capacity,
                entries.samples.capacity() < entries.capacity,
            )
        };
        let mut replacement = rebuild.then(|| HashMap::with_capacity(capacity));
        let mut entries = lock_snapshot_entries(shard, &mut samples);
        let old = replacement.take().map(|mut replacement| {
            replacement.extend(
                entries
                    .samples
                    .iter()
                    .map(|(pointer, entry)| (*pointer, *entry)),
            );
            std::mem::replace(&mut entries.samples, replacement)
        });
        samples.extend(entries.samples.values().map(|entry| entry.sample));
        drop(entries);
        // Rebuilding happens only during reporting. Free the old table outside
        // its shard lock so allocator metadata cleanup cannot recurse into it.
        drop(old);
    }
    samples
}

fn lock_snapshot_entries<'a>(
    shard: &'a LiveShard,
    samples: &mut Vec<LiveAllocationSample>,
) -> std::sync::MutexGuard<'a, LiveMap> {
    let entries = shard.entries.lock().unwrap_or_else(|err| err.into_inner());
    if samples.capacity() - samples.len() >= entries.samples.len() {
        return entries;
    }
    // Admissions outgrew the initial occupancy estimate. Reserve this shard's
    // hard limit outside its lock, then reacquire it. All later admissions fit;
    // copying under the lock cannot allocate or discard a retained sample.
    let capacity = entries.capacity;
    drop(entries);
    samples.reserve(capacity);
    shard.entries.lock().unwrap_or_else(|err| err.into_inner())
}

pub(super) fn sample_count() -> usize {
    SHARDS
        .get()
        .map(|shards| {
            shards
                .iter()
                .map(|shard| shard.count.load(Ordering::Relaxed))
                .sum()
        })
        .unwrap_or(0)
}

pub(super) fn dropped_sample_count() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

pub(super) fn metadata_payload_bytes() -> usize {
    METADATA_PAYLOAD_BYTES.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reserves_for_admissions_after_its_initial_estimate() {
        let sample = LiveAllocationSample {
            stack: StackKey {
                frames: [42; super::super::MAX_CAPTURE_DEPTH],
                depth: 1,
            },
            weighted_objects: 1.0,
            weighted_bytes: 1024.0,
        };
        let shard = LiveShard {
            entries: Mutex::new(LiveMap {
                samples: (0..8)
                    .map(|pointer| {
                        (
                            pointer,
                            LiveEntry {
                                sample,
                                reallocating: pointer == 0,
                            },
                        )
                    })
                    .collect(),
                capacity: 8,
                membership_counts: [0; 64],
            }),
            count: AtomicUsize::new(8),
            membership: AtomicU64::new(0),
        };
        let mut samples = Vec::with_capacity(1);
        samples.push(sample);
        let entries = lock_snapshot_entries(&shard, &mut samples);
        assert!(samples.capacity() - samples.len() >= entries.samples.len());
        let capacity = samples.capacity();
        samples.extend(entries.samples.values().map(|entry| entry.sample));
        assert_eq!(
            samples.len(),
            9,
            "retain the prefix and every new admission"
        );
        assert_eq!(
            samples.capacity(),
            capacity,
            "copying must not grow storage"
        );
        assert!(
            entries.samples[&0].reallocating,
            "keep pending realloc state"
        );
    }
}
