//! Allocation profiler for applications that install
//! [`SamplingAllocator`] as their global allocator.
//!
//! The allocator hot path records sampled allocation events into fixed TLS
//! rings with best-effort, non-blocking handoff to sharded global buffers.
//! `report()` is the non-hot path: it may block briefly to drain registered TLS
//! rings, aggregate samples, resolve symbols, and encode memory pprof data.
//! Optional live heap tracking maintains sampled pointers in bounded shards.

mod live;
mod mappings;
mod sampling;

use sampling::{SampleWeight, next_poisson_interval};

use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

use crate::{
    Error, Result,
    memory_pprof::{self, AllocationSample, MemoryFrame, MemorySymbol},
};

const LOG_TAG: &str = "rustfs_mimalloc_profiling";
const DEFAULT_SAMPLE_INTERVAL_BYTES: u64 = 1024 * 1024;
const DEFAULT_MAX_DEPTH: usize = 64;
const DEFAULT_RING_CAPACITY: usize = 512;
const DEFAULT_REPORT_DRAIN_LIMIT: usize = 1_000_000;
const DEFAULT_MAX_LIVE_SAMPLES: usize = 16_384;
const MAX_CAPTURE_DEPTH: usize = 64;
const PROFILER_FRAME_ALLOWANCE: usize = 16;
const TLS_SAMPLE_RING_CAPACITY: usize = 64;
const RECORDED_SAMPLE_SHARD_COUNT: usize = 8;
const SYNTHETIC_FRAME: &str = "[mimalloc] unresolved allocation stack";
const RNG_INCREMENT: u64 = 0x9e37_79b9_7f4a_7c15;
const RNG_INITIAL_STATE: u64 = 0xa076_1d64_78bd_642f;

static BACKEND_CLAIMED: AtomicBool = AtomicBool::new(false);
static CAPTURE_DEPTH: AtomicUsize = AtomicUsize::new(MAX_CAPTURE_DEPTH);
static NATIVE_STACK_CAPTURE: AtomicBool = AtomicBool::new(false);
static SAMPLE_INTERVAL_BYTES: AtomicU64 = AtomicU64::new(DEFAULT_SAMPLE_INTERVAL_BYTES);

/// Per-allocation hot-path state. These are read on every allocation, so they
/// share one aligned cache line that sample-hit counter writes cannot bounce.
#[repr(align(64))]
struct HotAllocationState {
    recorder_active: AtomicBool,
    allocator_seen: AtomicBool,
    sampling_config_generation: AtomicU64,
}

static HOT_ALLOCATION_STATE: HotAllocationState = HotAllocationState {
    recorder_active: AtomicBool::new(false),
    allocator_seen: AtomicBool::new(false),
    sampling_config_generation: AtomicU64::new(0),
};
static SAMPLING_RNG_SEED: AtomicU64 = AtomicU64::new(RNG_INITIAL_STATE);
static MAX_RECORDED_SAMPLES: AtomicUsize = AtomicUsize::new(DEFAULT_RING_CAPACITY);
static GLOBAL_BUFFERED_SAMPLE_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_RECORDED_SAMPLE_SHARD: AtomicUsize = AtomicUsize::new(0);
static NEXT_DRAINED_SAMPLE_SHARD: AtomicUsize = AtomicUsize::new(0);
static NEXT_REPORT_SOURCE: AtomicUsize = AtomicUsize::new(0);
static NEXT_TLS_DRAIN_BUFFER: AtomicUsize = AtomicUsize::new(0);
static RECORDED_SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);
static REPORTED_SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);
static REPORT_COUNT: AtomicU64 = AtomicU64::new(0);
static FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);
static FLUSHED_SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);
static DROPPED_SAMPLES: AtomicU64 = AtomicU64::new(0);
static LAST_PPROF_ENCODE_ELAPSED_MICROS: AtomicU64 = AtomicU64::new(0);

static RECORDED_SAMPLE_SHARDS: LazyLock<Vec<Mutex<VecDeque<RecordedAllocationSample>>>> =
    LazyLock::new(|| {
        (0..RECORDED_SAMPLE_SHARD_COUNT)
            .map(|_| Mutex::new(VecDeque::new()))
            .collect()
    });
static TLS_SAMPLE_BUFFER_REGISTRY: LazyLock<Mutex<TlsSampleBufferRegistry>> =
    LazyLock::new(|| Mutex::new(TlsSampleBufferRegistry::new()));

#[derive(Debug, Copy, Clone)]
struct SamplerState {
    in_profiler: bool,
    profiler_suppressed: bool,
    remaining_bytes: u64,
    remaining_config_generation: u64,
    rng_state: u64,
}

impl SamplerState {
    const fn new() -> Self {
        Self {
            in_profiler: false,
            profiler_suppressed: false,
            remaining_bytes: DEFAULT_SAMPLE_INTERVAL_BYTES,
            remaining_config_generation: 0,
            rng_state: 0,
        }
    }
}

thread_local! {
    // Keep all allocation-hook state in one TLS cell so the hot path pays one
    // state lookup instead of one lookup per guard, sampler, RNG, and flush flag.
    static SAMPLER_STATE: Cell<SamplerState> = const { Cell::new(SamplerState::new()) };
    static TLS_SAMPLE_BUFFER: RegisteredTlsSampleBuffer = RegisteredTlsSampleBuffer::new();
}

#[derive(Debug, Copy, Clone)]
struct StackKey {
    frames: [usize; MAX_CAPTURE_DEPTH],
    depth: usize,
}

impl PartialEq for StackKey {
    fn eq(&self, other: &Self) -> bool {
        self.frames[..self.depth] == other.frames[..other.depth]
    }
}

impl Eq for StackKey {}

impl Hash for StackKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Unused slots do not identify a stack. Hash only captured addresses,
        // particularly for shallow stacks during report aggregation.
        self.frames[..self.depth].hash(state);
    }
}

impl StackKey {
    fn capture(max_depth: usize) -> Self {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        if NATIVE_STACK_CAPTURE.load(Ordering::Relaxed) {
            return Self::capture_native(max_depth);
        }
        Self::capture_portable(max_depth)
    }

    fn capture_portable(max_depth: usize) -> Self {
        let mut key = Self {
            frames: [0; MAX_CAPTURE_DEPTH],
            depth: 0,
        };
        let max_depth = max_depth.min(MAX_CAPTURE_DEPTH);

        backtrace::trace(|frame| {
            if key.depth >= max_depth {
                return false;
            }
            key.frames[key.depth] = frame.ip() as usize;
            key.depth += 1;
            true
        });

        key
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    fn capture_native(max_depth: usize) -> Self {
        let limit = max_depth.min(MAX_CAPTURE_DEPTH);
        let mut key = Self {
            frames: [0; MAX_CAPTURE_DEPTH],
            depth: 0,
        };
        if limit == 0 {
            return key;
        }
        let mut addresses = [std::ptr::null_mut::<std::ffi::c_void>(); MAX_CAPTURE_DEPTH];
        // SAFETY: glibc backtrace is MT-Safe, writes at most limit pointers to
        // this writable array, and never retains it. No callback or symbols
        // are invoked here. Initialization warms its lazy unwinder loading.
        let count = unsafe { libc::backtrace(addresses.as_mut_ptr(), limit as libc::c_int) };
        key.depth = usize::try_from(count).unwrap_or(0).min(limit);
        for (output, address) in key.frames[..key.depth].iter_mut().zip(&addresses) {
            *output = *address as usize;
        }
        key
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.frames[..self.depth].iter().copied()
    }
}

#[derive(Debug, Copy, Clone)]
struct RecordedAllocationSample {
    stack: StackKey,
    weighted_objects: f64,
    weighted_bytes: f64,
}

#[derive(Debug)]
struct TlsSampleBuffer {
    samples: [Option<RecordedAllocationSample>; TLS_SAMPLE_RING_CAPACITY],
    head: usize,
    len: usize,
    generation: u64,
}

impl TlsSampleBuffer {
    fn new() -> Self {
        Self {
            samples: [None; TLS_SAMPLE_RING_CAPACITY],
            head: 0,
            len: 0,
            generation: HOT_ALLOCATION_STATE
                .sampling_config_generation
                .load(Ordering::Acquire),
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn is_full(&self) -> bool {
        self.len == TLS_SAMPLE_RING_CAPACITY
    }

    fn push(&mut self, sample: RecordedAllocationSample) -> bool {
        if self.is_full() {
            return false;
        }

        let tail = (self.head + self.len) % TLS_SAMPLE_RING_CAPACITY;
        self.samples[tail] = Some(sample);
        self.len += 1;
        true
    }

    fn clear(&mut self) {
        for offset in 0..self.len {
            self.samples[(self.head + offset) % TLS_SAMPLE_RING_CAPACITY] = None;
        }
        self.head = 0;
        self.len = 0;
    }

    fn drain_into(
        &mut self,
        out: &mut impl Extend<RecordedAllocationSample>,
        limit: usize,
    ) -> usize {
        let drained = self.len.min(limit);
        for offset in 0..drained {
            let slot = (self.head + offset) % TLS_SAMPLE_RING_CAPACITY;
            if let Some(sample) = self.samples[slot].take() {
                out.extend(std::iter::once(sample));
            }
        }
        self.head = (self.head + drained) % TLS_SAMPLE_RING_CAPACITY;
        self.len -= drained;
        drained
    }
}

#[derive(Debug)]
struct RegisteredTlsSampleBuffer {
    id: Cell<Option<usize>>,
    buffer: Arc<Mutex<TlsSampleBuffer>>,
}

impl RegisteredTlsSampleBuffer {
    fn new() -> Self {
        let buffer = Arc::new(Mutex::new(TlsSampleBuffer::new()));
        let id = register_tls_sample_buffer(buffer.clone());
        Self {
            id: Cell::new(id),
            buffer,
        }
    }

    fn ensure_registered(&self) -> Option<usize> {
        if let Some(id) = self.id.get() {
            return Some(id);
        }
        let id = register_tls_sample_buffer(self.buffer.clone())?;
        self.id.set(Some(id));
        Some(id)
    }

    fn try_lock(&self) -> Option<std::sync::MutexGuard<'_, TlsSampleBuffer>> {
        // Every accepted record must be visible to the reporter. A registry
        // contention miss is counted as a dropped event, never hidden in an
        // unregistered ring waiting for this thread to allocate again.
        self.ensure_registered()?;
        self.buffer.try_lock().ok()
    }
}

impl Drop for RegisteredTlsSampleBuffer {
    fn drop(&mut self) {
        with_profiler_suppressed(|| {
            if HOT_ALLOCATION_STATE.recorder_active.load(Ordering::Acquire)
                && let Ok(mut buffer) = self.buffer.lock()
                && !flush_tls_samples_on_exit(&mut buffer)
            {
                drop_tls_samples(&mut buffer);
            }
            // Deregister only after the final handoff attempt so a concurrent
            // report can still see this thread's ring until thread teardown.
            if let Some(id) = self.id.get() {
                deregister_tls_sample_buffer(id);
            }
        });
    }
}

#[derive(Debug, Copy, Clone, Default)]
struct AggregatedAllocationSample {
    alloc_objects: f64,
    alloc_space: f64,
    inuse_objects: f64,
    inuse_space: f64,
}

/// Configuration for a mimalloc allocation profiling session.
///
/// The profiler records sampled allocation call stacks and reports memory pprof
/// data with `alloc_objects/count` and `alloc_space/bytes` sample types. Enabling
/// `live_heap_tracking` adds `inuse_objects/count` and `inuse_space/bytes`, with
/// `inuse_space` selected by default, matching jemalloc's live heap view.
/// Unresolved physical addresses remain available for downstream symbolization;
/// a synthetic fallback is used only when no usable frame remains.
///
/// # Examples
///
/// ```rust
/// use rustfs_mimalloc_profiling::MimallocConfig;
///
/// let config = MimallocConfig {
///     sample_interval_bytes: 512 * 1024,
///     max_depth: 48,
///     ..MimallocConfig::default()
/// };
///
/// assert_eq!(config.sample_interval_bytes, 512 * 1024);
/// assert_eq!(config.max_depth, 48);
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MimallocConfig {
    /// Average number of allocated bytes between sampled allocation events.
    ///
    /// The sampler uses a byte-based Poisson process, so this value is the mean
    /// interval rather than a fixed every-N-bytes trigger. Lower values increase
    /// profile detail and hot-path overhead.
    pub sample_interval_bytes: u64,
    /// Maximum number of stack frames captured for each sampled allocation.
    pub max_depth: usize,
    /// Stack capture engine. Portable is the default; Native is an opt-in
    /// Linux glibc raw-address collector without backtrace-rs's shared lock.
    pub stack_capture: MimallocStackCapture,
    /// Maximum number of samples retained in the global recorder between reports.
    ///
    /// If the recorder is full or contended, new samples are dropped rather than
    /// blocking the allocator hot path.
    pub ring_capacity: usize,
    /// Maximum number of allocation samples drained by one `report()` call.
    ///
    /// A bounded drain keeps large bursts from making a single report interval do
    /// unbounded aggregation and pprof encoding work.
    pub report_drain_limit: usize,
    /// Track sampled live pointers and emit a live heap snapshot on every report.
    ///
    /// Disabled by default. This adds a sharded lookup on deallocation and
    /// successful reallocation. Removals may briefly wait for a shard lock so
    /// cross-thread frees never leave stale live entries. Only Rust allocations
    /// made through `SamplingAllocator` after initialization are tracked.
    /// Same-address realloc preserves the original allocation stack and object
    /// weight while updating size. Moving realloc samples the replacement
    /// independently, without depending on the allocation-event byte remainder.
    pub live_heap_tracking: bool,
    /// Maximum number of sampled live pointers retained across all shards.
    ///
    /// Capacity is partitioned across shards and preallocated at initialization.
    /// A full or contended shard drops new live samples, counted separately in
    /// `MimallocStats::dropped_live_samples`. Independent of `ring_capacity` and
    /// `report_drain_limit`; live snapshots always include every retained entry.
    pub max_live_samples: usize,
}

/// Raw stack capture engine used only on sampling hits.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum MimallocStackCapture {
    /// Synchronized backtrace-rs collector, available on supported platforms.
    #[default]
    Portable,
    /// MT-Safe glibc backtrace collector into a fixed buffer. Linux GNU only;
    /// initialization rejects this mode elsewhere rather than silently falling back.
    Native,
}

/// Runtime counters for the active mimalloc profiling session.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MimallocStats {
    /// Number of samples accepted into the recorder since session initialization.
    pub recorded_samples: u64,
    /// Allocation records encoded into completed reports, before stack aggregation.
    /// Does not count live entries repeatedly included in heap snapshots or uploads.
    pub reported_samples: u64,
    /// Number of successfully encoded reports since initialization, including empty reports.
    pub reports: u64,
    /// Number of successful TLS-to-global or TLS-to-report flushes since initialization.
    pub flushes: u64,
    /// Number of samples moved from TLS rings into global buffers or reports.
    pub flushed_samples: u64,
    /// Sample records rejected by queue pressure, registration contention,
    /// or unavailable TLS during thread teardown.
    pub dropped_samples: u64,
    /// Number of samples currently buffered for the next report, if the buffer lock is available.
    pub buffered_samples: Option<usize>,
    /// Duration of the most recent image-metadata collection and pprof encoding
    /// step in microseconds, excluding stack resolution and sample aggregation.
    pub last_pprof_encode_elapsed_micros: u64,
    /// Number of sampled live pointers currently retained (zero when disabled).
    pub live_samples: usize,
    /// Live samples omitted because their metadata shard was full or contended.
    pub dropped_live_samples: u64,
    /// Preallocated live-table entry storage in bytes, excluding hash control
    /// bytes, shard headers, and allocator bookkeeping. Zero when disabled.
    pub live_metadata_payload_bytes: usize,
}

/// Return current mimalloc recorder counters.
///
/// This is mainly intended for tests, diagnostics, and benchmark reports.
pub fn mimalloc_stats() -> MimallocStats {
    let tls_buffered_samples = registered_tls_buffered_samples();

    MimallocStats {
        recorded_samples: RECORDED_SAMPLE_COUNT.load(Ordering::Relaxed),
        reported_samples: REPORTED_SAMPLE_COUNT.load(Ordering::Relaxed),
        reports: REPORT_COUNT.load(Ordering::Relaxed),
        flushes: FLUSH_COUNT.load(Ordering::Relaxed),
        flushed_samples: FLUSHED_SAMPLE_COUNT.load(Ordering::Relaxed),
        dropped_samples: DROPPED_SAMPLES.load(Ordering::Relaxed),
        buffered_samples: tls_buffered_samples.map(|tls| {
            GLOBAL_BUFFERED_SAMPLE_COUNT
                .load(Ordering::Relaxed)
                .saturating_add(tls)
        }),
        last_pprof_encode_elapsed_micros: LAST_PPROF_ENCODE_ELAPSED_MICROS.load(Ordering::Relaxed),
        live_samples: live::sample_count(),
        dropped_live_samples: live::dropped_sample_count(),
        live_metadata_payload_bytes: live::metadata_payload_bytes(),
    }
}

impl Default for MimallocConfig {
    fn default() -> Self {
        Self {
            sample_interval_bytes: DEFAULT_SAMPLE_INTERVAL_BYTES,
            max_depth: DEFAULT_MAX_DEPTH,
            stack_capture: MimallocStackCapture::Portable,
            ring_capacity: DEFAULT_RING_CAPACITY,
            report_drain_limit: DEFAULT_REPORT_DRAIN_LIMIT,
            live_heap_tracking: false,
            max_live_samples: DEFAULT_MAX_LIVE_SAMPLES,
        }
    }
}

impl MimallocConfig {
    fn validate(&self) -> Result<()> {
        if self.stack_capture == MimallocStackCapture::Native
            && !cfg!(all(target_os = "linux", target_env = "gnu"))
        {
            return Err(Error::InvalidConfig(
                "mimalloc: native stack capture requires Linux glibc",
            ));
        }
        if self.sample_interval_bytes == 0 {
            return Err(Error::InvalidConfig(
                "mimalloc: sample_interval_bytes must be greater than zero",
            ));
        }
        if self.max_depth == 0 || self.max_depth > MAX_CAPTURE_DEPTH {
            return Err(Error::InvalidConfig(
                "mimalloc: max_depth must be between 1 and 64",
            ));
        }
        if self.ring_capacity == 0 {
            return Err(Error::InvalidConfig(
                "mimalloc: ring_capacity must be greater than zero",
            ));
        }
        if self.report_drain_limit == 0 {
            return Err(Error::InvalidConfig(
                "mimalloc: report_drain_limit must be greater than zero",
            ));
        }
        if self.live_heap_tracking && self.max_live_samples == 0 {
            return Err(Error::InvalidConfig(
                "mimalloc: max_live_samples must be greater than zero when live heap tracking is enabled",
            ));
        }
        Ok(())
    }
}

/// A sampling wrapper around an application's chosen global allocator.
///
/// Install this wrapper as `#[global_allocator]` to observe Rust allocations.
/// Sampling is active only while a [`Profiler`] session is running.
///
/// ```
/// use rustfs_mimalloc_profiling::SamplingAllocator;
///
/// #[global_allocator]
/// static ALLOC: SamplingAllocator<std::alloc::System> =
///     SamplingAllocator::with_allocator(std::alloc::System);
/// ```
pub struct SamplingAllocator<A> {
    inner: A,
}

impl<A> SamplingAllocator<A> {
    /// Wrap an allocator without allocating or starting a recording session.
    pub const fn with_allocator(inner: A) -> Self {
        Self { inner }
    }
}

impl<A: Default> Default for SamplingAllocator<A> {
    fn default() -> Self {
        Self::with_allocator(A::default())
    }
}

/// Sampling wrapper for the RustFS mimalloc provider.
///
/// Enable `rustfs-allocator` to retain the pre-0.7 convenience API.
#[cfg(feature = "rustfs-allocator")]
pub type SamplingMiMalloc = SamplingAllocator<rustfs_mimalloc::MiMalloc>;

#[cfg(feature = "rustfs-allocator")]
impl SamplingAllocator<rustfs_mimalloc::MiMalloc> {
    /// Construct the optional RustFS provider wrapper.
    pub const fn new() -> Self {
        Self::with_allocator(rustfs_mimalloc::MiMalloc)
    }
}

// SAFETY: Every allocation operation is forwarded to the same A with the exact
// pointer/layout contract. Recorder bookkeeping never substitutes an allocator.
unsafe impl<A: GlobalAlloc> GlobalAlloc for SamplingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        mark_allocator_seen();
        // SAFETY: `SamplingAllocator` preserves the caller's `GlobalAlloc`
        // contract and forwards the exact layout to the wrapped
        // allocator.
        let ptr = unsafe { self.inner.alloc(layout) };
        if !ptr.is_null() {
            record_allocation(ptr, layout.size() as u64);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        mark_allocator_seen();
        // SAFETY: The caller provided a valid `GlobalAlloc` layout; this wrapper
        // only forwards it to the provider and records after successful allocation.
        let ptr = unsafe { self.inner.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_allocation(ptr, layout.size() as u64);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Remove before freeing: another thread may immediately reuse this
        // address once the provider receives it.
        if hooks_enabled() && live::enabled() {
            remove_live_allocation(ptr as usize);
        }
        // SAFETY: Deallocation is forwarded unchanged; callers must pass a
        // pointer and layout that satisfy the `GlobalAlloc::dealloc` contract.
        unsafe { self.inner.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        mark_allocator_seen();
        // Keep the metadata slot reserved before the provider can recycle the old
        // address. Lifecycle maintenance must also run while sampling is suppressed.
        let pending = if hooks_enabled() && live::enabled() {
            with_profiler_suppressed(|| live::begin_reallocation(ptr as usize))
        } else {
            None
        };
        // SAFETY: Reallocation is forwarded unchanged to the provider with the
        // caller-provided pointer, old layout, and requested new size.
        let new_ptr = unsafe { self.inner.realloc(ptr, layout, new_size) };
        let previous = pending.and_then(|pending| {
            with_profiler_suppressed(|| pending.finish(new_ptr as usize, new_size as u64))
        });
        if !new_ptr.is_null() {
            if new_ptr != ptr && hooks_enabled() && live::enabled() {
                record_live_reallocation(new_ptr as usize, new_size as u64, previous);
            }
            let recorded_size = realloc_recorded_size(ptr, new_ptr, layout.size(), new_size);
            // Live lifecycle updates and replacement sampling are independent
            // of the event sampler, which charges only newly allocated bytes.
            record_allocation(std::ptr::null_mut(), recorded_size as u64);
        }
        new_ptr
    }
}

/// Exclusive owner of a process-wide profiling session.
///
/// Dropping the owner stops recording and releases session metadata. Call
/// [`Self::report`] before stopping to collect pending data. Allocations continue
/// to work after shutdown; a new session can then be started.
#[derive(Debug)]
pub struct Profiler {
    config: MimallocConfig,
    last_report: Option<Instant>,
    initialized: bool,
}

impl Profiler {
    /// Start a session after installing [`SamplingAllocator`] as global allocator.
    ///
    /// Fails if the configuration is invalid, the sampling allocator has not
    /// observed allocations, or another session is active. With the `no_hooks`
    /// feature it fails with [`Error::InvalidConfig`] because no observation
    /// machinery is compiled in.
    pub fn start(config: MimallocConfig) -> Result<Self> {
        if !hooks_enabled() {
            // Sessions have nothing to observe when hooks are compiled out.
            return Err(Error::InvalidConfig(
                "allocation hooks are compiled out by the no_hooks feature",
            ));
        }
        let mut profiler = Self::new(config);
        profiler.initialize()?;
        Ok(profiler)
    }

    fn new(config: MimallocConfig) -> Self {
        Self {
            config,
            last_report: None,
            initialized: false,
        }
    }

    /// Stop recording, release metadata, and relinquish recorder ownership.
    ///
    /// This is idempotent. Pending samples are discarded; call [`Self::report`]
    /// first when they are needed. The allocator remains usable after stopping.
    pub fn stop(&mut self) {
        if !self.initialized {
            return;
        }
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(false, Ordering::Release);
        with_profiler_suppressed(|| {
            clear_registered_tls_samples();
            live::clear();
        });
        self.initialized = false;
        BACKEND_CLAIMED.store(false, Ordering::Release);
    }
}

impl Drop for Profiler {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Profiler {
    fn initialize(&mut self) -> Result<()> {
        self.config.validate()?;
        if !HOT_ALLOCATION_STATE.allocator_seen.load(Ordering::Relaxed) {
            return Err(Error::AllocatorNotObserved);
        }
        BACKEND_CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::AlreadyActive)?;
        self.initialized = true;
        with_profiler_suppressed(|| {
            SAMPLE_INTERVAL_BYTES.store(self.config.sample_interval_bytes, Ordering::Relaxed);
            HOT_ALLOCATION_STATE
                .sampling_config_generation
                .fetch_add(1, Ordering::AcqRel);
            CAPTURE_DEPTH.store(
                self.config
                    .max_depth
                    .saturating_add(PROFILER_FRAME_ALLOWANCE)
                    .min(MAX_CAPTURE_DEPTH),
                Ordering::Relaxed,
            );
            NATIVE_STACK_CAPTURE.store(
                self.config.stack_capture == MimallocStackCapture::Native,
                Ordering::Relaxed,
            );
            NEXT_RECORDED_SAMPLE_SHARD.store(0, Ordering::Relaxed);
            NEXT_DRAINED_SAMPLE_SHARD.store(0, Ordering::Relaxed);
            NEXT_REPORT_SOURCE.store(0, Ordering::Relaxed);
            NEXT_TLS_DRAIN_BUFFER.store(0, Ordering::Relaxed);
            MAX_RECORDED_SAMPLES.store(self.config.ring_capacity, Ordering::Relaxed);
            RECORDED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
            REPORTED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
            REPORT_COUNT.store(0, Ordering::Relaxed);
            FLUSH_COUNT.store(0, Ordering::Relaxed);
            FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
            DROPPED_SAMPLES.store(0, Ordering::Relaxed);
            LAST_PPROF_ENCODE_ELAPSED_MICROS.store(0, Ordering::Relaxed);
            prepare_sample_buffer(self.config.ring_capacity);
            // A session can be stopped and started again while worker threads keep
            // their TLS rings alive. Clear registered rings at the session boundary
            // so old samples cannot be flushed into the next profile interval.
            clear_registered_tls_samples();
            warm_backtrace();
            live::prepare(self.config.live_heap_tracking, self.config.max_live_samples);
            HOT_ALLOCATION_STATE
                .recorder_active
                .store(true, Ordering::Release);
            self.last_report = Some(Instant::now());
        });

        log::info!(target: LOG_TAG, "Mimalloc profiling session started");
        Ok(())
    }

    /// Collect interval allocation counters and an optional live heap snapshot.
    ///
    /// Returns uncompressed pprof bytes, including timestamps, mappings, and
    /// source metadata when available. `alloc_*` values cover drained events;
    /// `inuse_*` values describe retained sampled allocations. The `__delta__`
    /// sample label is `false` so compatible consumers do not difference
    /// interval counters again. Reporting may allocate and wait for recorder
    /// locks. Returns [`Error::Inactive`] after this session has stopped.
    pub fn report(&mut self) -> Result<Vec<u8>> {
        if !self.initialized {
            return Err(Error::Inactive);
        }
        // Reporting allocates for registry snapshots, aggregation, symbol
        // resolution, and pprof encoding. Suppressing this thread avoids
        // profiler self-sampling without disabling worker-thread sampling.
        with_profiler_suppressed(|| {
            let now = Instant::now();
            let duration_nanos = self
                .last_report
                .replace(now)
                .map(|last_report| duration_to_i64_nanos(now.duration_since(last_report)))
                .unwrap_or_default();

            let recorded = drain_samples_for_report(self.config.report_drain_limit);
            let recorded_count = recorded.len();
            let dropped_count = DROPPED_SAMPLES.load(Ordering::Relaxed);
            if dropped_count > 0 {
                log::debug!(
                    target: LOG_TAG,
                    "Profiler report drained {recorded_count} samples; {dropped_count} samples have been dropped since initialization"
                );
            }

            let samples = build_memory_samples(recorded, live::snapshot(), self.config.max_depth);

            let encode_start = Instant::now();
            let mappings = mappings::collect(&samples);
            let pprof_data = memory_pprof::encode_memory_profile_with_mappings(
                &samples,
                self.config.sample_interval_bytes,
                duration_nanos,
                self.config.live_heap_tracking,
                &mappings,
            );
            LAST_PPROF_ENCODE_ELAPSED_MICROS.store(
                duration_to_u64_micros(encode_start.elapsed()),
                Ordering::Relaxed,
            );
            REPORTED_SAMPLE_COUNT.fetch_add(recorded_count as u64, Ordering::Relaxed);
            REPORT_COUNT.fetch_add(1, Ordering::Relaxed);

            Ok(pprof_data)
        })
    }
}

/// Compile-time hook switch. With `no_hooks`, observation paths fold away and
/// every allocation forwards straight to the wrapped allocator.
#[inline(always)]
const fn hooks_enabled() -> bool {
    cfg!(not(feature = "no_hooks"))
}

fn mark_allocator_seen() {
    if !hooks_enabled() {
        return;
    }
    if !HOT_ALLOCATION_STATE.allocator_seen.load(Ordering::Relaxed) {
        HOT_ALLOCATION_STATE
            .allocator_seen
            .store(true, Ordering::Relaxed);
    }
}

// Keep TLS suppression and table lookups out of the default allocator's
// deallocation body, including their stack/register requirements.
#[inline(never)]
fn remove_live_allocation(pointer: usize) {
    if !hooks_enabled() {
        return;
    }
    with_profiler_suppressed(|| live::remove(pointer));
}

fn with_profiler_suppressed<R>(f: impl FnOnce() -> R) -> R {
    struct SuppressionGuard<'a> {
        sampler: &'a Cell<SamplerState>,
        previous: bool,
    }

    impl Drop for SuppressionGuard<'_> {
        fn drop(&mut self) {
            let mut state = self.sampler.get();
            state.profiler_suppressed = self.previous;
            self.sampler.set(state);
        }
    }

    let mut f = Some(f);
    match SAMPLER_STATE.try_with(|sampler| {
        let mut state = sampler.get();
        let previous = state.profiler_suppressed;
        state.profiler_suppressed = true;
        sampler.set(state);

        let _guard = SuppressionGuard { sampler, previous };
        f.take().expect("suppression closure was already taken")()
    }) {
        Ok(result) => result,
        Err(_) => f
            .take()
            .expect("suppression fallback closure was already taken")(),
    }
}

fn realloc_recorded_size(
    old_ptr: *mut u8,
    new_ptr: *mut u8,
    old_size: usize,
    new_size: usize,
) -> usize {
    if old_ptr == new_ptr {
        new_size.saturating_sub(old_size)
    } else {
        new_size
    }
}

struct ProfilerReentryGuard<'a> {
    sampler: &'a Cell<SamplerState>,
}

impl Drop for ProfilerReentryGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.sampler.get();
        state.in_profiler = false;
        self.sampler.set(state);
    }
}

fn with_recording_guard<R>(f: impl FnOnce(&Cell<SamplerState>) -> R) -> Option<R> {
    SAMPLER_STATE
        .try_with(|sampler| {
            let mut state = sampler.get();
            if state.profiler_suppressed || state.in_profiler {
                return None;
            }
            state.in_profiler = true;
            sampler.set(state);
            let _guard = ProfilerReentryGuard { sampler };
            Some(f(sampler))
        })
        .ok()
        .flatten()
}

fn record_allocation(ptr: *mut u8, size: u64) {
    if size == 0
        || !hooks_enabled()
        || !HOT_ALLOCATION_STATE.recorder_active.load(Ordering::Acquire)
    {
        return;
    }
    let generation = HOT_ALLOCATION_STATE
        .sampling_config_generation
        .load(Ordering::Acquire);
    let _ = SAMPLER_STATE.try_with(|sampler| {
        let mut state = sampler.get();
        if state.profiler_suppressed || state.in_profiler {
            return;
        }

        // The common unsampled allocation touches only its byte countdown.
        // Enter recursion suppression only when resetting or
        // capturing a sample: those operations can allocate or call the unwinder.
        if size < state.remaining_bytes && state.remaining_config_generation == generation {
            state.remaining_bytes -= size;
            sampler.set(state);
            return;
        }

        state.in_profiler = true;
        sampler.set(state);
        let _guard = ProfilerReentryGuard { sampler };
        let interval = SAMPLE_INTERVAL_BYTES.load(Ordering::Relaxed).max(1);
        let mut current = state.remaining_bytes;
        if state.remaining_config_generation != generation || current == 0 {
            state.rng_state = next_thread_rng_seed();
            current = next_poisson_interval(interval, &mut state.rng_state);
            state.remaining_config_generation = generation;
        }

        if size < current {
            state.remaining_bytes = current - size;
            sampler.set(state);
        } else {
            // Exponential intervals are memoryless. Start at the end of the
            // allocation; counting all crossed intervals is unnecessary.
            state.remaining_bytes = next_poisson_interval(interval, &mut state.rng_state);
            sampler.set(state);
            record_sample(
                ptr,
                SampleWeight::for_allocation(size, interval),
                generation,
            );
        }
    });
}

fn next_thread_rng_seed() -> u64 {
    let seed = SAMPLING_RNG_SEED.fetch_add(RNG_INCREMENT, Ordering::Relaxed);
    splitmix64(seed.wrapping_add(RNG_INCREMENT))
}

fn splitmix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn session_is_current(generation: u64) -> bool {
    HOT_ALLOCATION_STATE.recorder_active.load(Ordering::Acquire)
        && HOT_ALLOCATION_STATE
            .sampling_config_generation
            .load(Ordering::Acquire)
            == generation
}

fn record_live_reallocation(
    pointer: usize,
    size: u64,
    previous: Option<(live::LiveAllocationSample, u64)>,
) {
    if !HOT_ALLOCATION_STATE.recorder_active.load(Ordering::Acquire) {
        return;
    }
    let generation = HOT_ALLOCATION_STATE
        .sampling_config_generation
        .load(Ordering::Acquire);
    let sampled = with_recording_guard(|sampler| {
        let mut state = sampler.get();
        if state.rng_state == 0 {
            state.rng_state = next_thread_rng_seed();
        }
        let interval = SAMPLE_INTERVAL_BYTES.load(Ordering::Relaxed).max(1);
        let hit = next_poisson_interval(interval, &mut state.rng_state) <= size;
        sampler.set(state);
        if hit {
            let stack = StackKey::capture(CAPTURE_DEPTH.load(Ordering::Relaxed));
            live::record(
                pointer,
                stack,
                SampleWeight::for_allocation(size, interval),
                generation,
            );
        }
    });
    if sampled.is_none() {
        // Suppression prevents capturing new profiler allocations, but never
        // abandons the lifecycle of a pointer that was already tracked.
        if let Some((sample, generation)) = previous {
            with_profiler_suppressed(|| live::transfer_sample(pointer, sample, generation));
        }
    }
}

fn record_sample(ptr: *mut u8, weight: SampleWeight, generation: u64) {
    let tracking_live = live::enabled() && !ptr.is_null();
    let mut stack = None;
    let dropped = TLS_SAMPLE_BUFFER
        .try_with(|buffer| {
            let Some(mut buffer) = buffer.try_lock() else {
                return true;
            };
            // Check while holding the ring lock so shutdown/restart cannot admit
            // a stack captured for the previous profiling session.
            if !session_is_current(generation) {
                return false;
            }
            if buffer.generation != generation {
                buffer.clear();
                buffer.generation = generation;
            }

            if buffer.is_full() {
                flush_tls_samples(&mut buffer);
            }
            if buffer.is_full() && !tracking_live {
                return true;
            }

            let captured = StackKey::capture(CAPTURE_DEPTH.load(Ordering::Relaxed));
            stack = Some(captured);
            let sample = RecordedAllocationSample {
                stack: captured,
                weighted_objects: weight.weighted_objects,
                weighted_bytes: weight.weighted_bytes,
            };

            if buffer.push(sample) {
                RECORDED_SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed);
                false
            } else {
                true
            }
        })
        .unwrap_or(true);
    // Registry contention, queue pressure, and unavailable TLS all reject
    // the event explicitly. Drops from an already-stale session are ignored.
    if dropped && session_is_current(generation) {
        DROPPED_SAMPLES.fetch_add(1, Ordering::Relaxed);
    }
    // The allocation event queue and live table have independent budgets.
    // Dropping an interval event must not suppress an otherwise valid live hit.
    if tracking_live && session_is_current(generation) {
        let stack =
            stack.unwrap_or_else(|| StackKey::capture(CAPTURE_DEPTH.load(Ordering::Relaxed)));
        live::record(ptr as usize, stack, weight, generation);
    }
}

#[derive(Debug)]
struct TlsSampleBufferRegistry {
    buffers: Vec<Option<Arc<Mutex<TlsSampleBuffer>>>>,
    free_ids: Vec<usize>,
}

impl TlsSampleBufferRegistry {
    const fn new() -> Self {
        Self {
            buffers: Vec::new(),
            free_ids: Vec::new(),
        }
    }

    fn register(&mut self, buffer: Arc<Mutex<TlsSampleBuffer>>) -> usize {
        if let Some(id) = self.free_ids.pop() {
            self.buffers[id] = Some(buffer);
            id
        } else {
            let id = self.buffers.len();
            self.buffers.push(Some(buffer));
            id
        }
    }

    fn deregister(&mut self, id: usize) {
        if id < self.buffers.len() && self.buffers[id].take().is_some() {
            self.free_ids.push(id);
        }
    }

    fn buffers(&self) -> Vec<Arc<Mutex<TlsSampleBuffer>>> {
        self.buffers.iter().filter_map(Clone::clone).collect()
    }
}

fn register_tls_sample_buffer(buffer: Arc<Mutex<TlsSampleBuffer>>) -> Option<usize> {
    // TLS registration can happen from the allocator hook on first use. Do not
    // wait for the global registry lock there. Admission requires registration;
    // a later sampling hit retries after an explicitly counted contention drop.
    let Ok(mut registry) = TLS_SAMPLE_BUFFER_REGISTRY.try_lock() else {
        return None;
    };

    Some(registry.register(buffer))
}

fn deregister_tls_sample_buffer(id: usize) {
    if let Ok(mut registry) = TLS_SAMPLE_BUFFER_REGISTRY.lock() {
        registry.deregister(id);
    }
}

fn registered_tls_sample_buffers() -> Vec<Arc<Mutex<TlsSampleBuffer>>> {
    let Ok(registry) = TLS_SAMPLE_BUFFER_REGISTRY.lock() else {
        DROPPED_SAMPLES.fetch_add(1, Ordering::Relaxed);
        return Vec::new();
    };

    registry.buffers()
}

fn registered_tls_buffered_samples() -> Option<usize> {
    with_profiler_suppressed(|| {
        let buffers = registered_tls_sample_buffers();
        let mut buffered_samples = 0_usize;
        for buffer in buffers {
            let Ok(buffer) = buffer.try_lock() else {
                return None;
            };
            buffered_samples = buffered_samples.saturating_add(buffer.len());
        }
        Some(buffered_samples)
    })
}

fn count_recorded_samples() -> usize {
    RECORDED_SAMPLE_SHARDS
        .iter()
        .filter_map(|shard| match shard.lock() {
            Ok(samples) => Some(samples.len()),
            Err(_) => {
                DROPPED_SAMPLES.fetch_add(1, Ordering::Relaxed);
                None
            }
        })
        .sum()
}

fn drain_registered_tls_samples_for_report(
    out: &mut Vec<RecordedAllocationSample>,
    limit: usize,
) -> usize {
    if limit == 0 {
        return 0;
    }
    let mut drained = 0;
    let buffers = registered_tls_sample_buffers();
    if buffers.is_empty() {
        return 0;
    }
    let start = NEXT_TLS_DRAIN_BUFFER.fetch_add(1, Ordering::Relaxed) % buffers.len();
    for offset in 0..buffers.len() {
        if drained == limit {
            break;
        }
        let buffer = &buffers[(start + offset) % buffers.len()];
        if let Ok(mut buffer) = lock_for_report_drain(
            buffer,
            out,
            limit - drained,
            |_| TLS_SAMPLE_RING_CAPACITY,
            TlsSampleBuffer::len,
            Vec::reserve,
        ) {
            let moved = buffer.drain_into(out, limit - drained);
            if moved > 0 {
                FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
                FLUSHED_SAMPLE_COUNT.fetch_add(moved as u64, Ordering::Relaxed);
                drained += moved;
            }
        }
    }
    drained
}

fn clear_registered_tls_samples() {
    // Used only at session lifecycle boundaries; clear rather than flush so
    // stale samples from a previous session cannot leak into a new report.
    for buffer in registered_tls_sample_buffers() {
        if let Ok(mut buffer) = buffer.lock() {
            buffer.clear();
            buffer.generation = HOT_ALLOCATION_STATE
                .sampling_config_generation
                .load(Ordering::Acquire);
        }
    }
}

fn drop_tls_samples(buffer: &mut TlsSampleBuffer) {
    let dropped = buffer.len();
    if dropped > 0 {
        DROPPED_SAMPLES.fetch_add(dropped as u64, Ordering::Relaxed);
        buffer.clear();
    }
}

fn flush_tls_samples(buffer: &mut TlsSampleBuffer) -> bool {
    flush_tls_samples_to_global(buffer, GlobalSampleShardLock::Try)
}

fn flush_tls_samples_on_exit(buffer: &mut TlsSampleBuffer) -> bool {
    flush_tls_samples_to_global(buffer, GlobalSampleShardLock::Blocking)
}

#[derive(Debug, Copy, Clone)]
enum GlobalSampleShardLock {
    Try,
    Blocking,
}

fn flush_tls_samples_to_global(
    buffer: &mut TlsSampleBuffer,
    lock_mode: GlobalSampleShardLock,
) -> bool {
    if buffer.is_empty() {
        return true;
    }

    let shard_index =
        NEXT_RECORDED_SAMPLE_SHARD.fetch_add(1, Ordering::Relaxed) % RECORDED_SAMPLE_SHARD_COUNT;
    let mut samples = match lock_mode {
        GlobalSampleShardLock::Try => {
            let Ok(samples) = RECORDED_SAMPLE_SHARDS[shard_index].try_lock() else {
                return false;
            };
            samples
        }
        GlobalSampleShardLock::Blocking => {
            let Ok(samples) = RECORDED_SAMPLE_SHARDS[shard_index].lock() else {
                return false;
            };
            samples
        }
    };

    // Generation validation and reservations belong inside the shard lock.
    // Initialization clears each shard under the same lock before resetting
    // the global count, so an old flush cannot reserve slots in a new session.
    if buffer.generation
        != HOT_ALLOCATION_STATE
            .sampling_config_generation
            .load(Ordering::Acquire)
    {
        buffer.clear();
        return true;
    }
    let reserved_slots = reserve_global_sample_slots(buffer.len());
    if reserved_slots == 0 {
        return false;
    }

    // Keep handoffs inside the storage allocated during initialization, even
    // when contention makes occupancy uneven across shards.
    let writable_slots = reserved_slots.min(samples.capacity().saturating_sub(samples.len()));
    let flushed = buffer.drain_into(&mut *samples, writable_slots);
    if flushed < reserved_slots {
        release_global_sample_slots(reserved_slots - flushed);
    }
    if flushed > 0 {
        FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.fetch_add(flushed as u64, Ordering::Relaxed);
    }
    if !buffer.is_empty() {
        return false;
    }

    flushed > 0
}

fn reserve_global_sample_slots(wanted: usize) -> usize {
    let max_samples = MAX_RECORDED_SAMPLES.load(Ordering::Relaxed);
    let mut current = GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed);

    loop {
        let available = max_samples.saturating_sub(current);
        if available == 0 {
            return 0;
        }

        let reserved = wanted.min(available);
        match GLOBAL_BUFFERED_SAMPLE_COUNT.compare_exchange_weak(
            current,
            current + reserved,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return reserved,
            Err(observed) => current = observed,
        }
    }
}

fn release_global_sample_slots(slots: usize) {
    if slots == 0 {
        return;
    }

    let mut current = GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed);
    loop {
        let release = slots.min(current);
        if release == 0 {
            return;
        }

        match GLOBAL_BUFFERED_SAMPLE_COUNT.compare_exchange_weak(
            current,
            current - release,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

fn prepare_sample_buffer(capacity: usize) {
    for shard in RECORDED_SAMPLE_SHARDS.iter() {
        let Ok(mut samples) = shard.lock() else {
            DROPPED_SAMPLES.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        samples.clear();
        let current_capacity = samples.capacity();
        let shard_capacity = recorded_sample_shard_capacity(capacity);
        if current_capacity < shard_capacity {
            samples.reserve(shard_capacity);
        }
    }
    GLOBAL_BUFFERED_SAMPLE_COUNT.store(count_recorded_samples(), Ordering::Relaxed);
}

/// Return a source lock only when copying its bounded batch cannot grow output.
/// The reservation callback is monomorphized; tests use it to prove that source
/// locks are released before allocator work and simulate admissions in that gap.
fn lock_for_report_drain<'a, T>(
    source: &'a Mutex<T>,
    out: &mut Vec<RecordedAllocationSample>,
    limit: usize,
    capacity_bound: impl Fn(&T) -> usize,
    len: impl Fn(&T) -> usize,
    mut reserve: impl FnMut(&mut Vec<RecordedAllocationSample>, usize),
) -> std::sync::LockResult<std::sync::MutexGuard<'a, T>> {
    let mut retried = false;
    loop {
        let source = source.lock()?;
        let needed = len(&source).min(limit);
        if out.capacity() - out.len() >= needed {
            return Ok(source);
        }
        // Size stable sparse reports by actual occupancy. If producers outgrow
        // that reservation, use the source's bounded capacity on the retry.
        let additional = if retried {
            capacity_bound(&source).max(needed).min(limit)
        } else {
            needed
        };
        drop(source);
        reserve(out, additional);
        retried = true;
    }
}

fn drain_recorded_samples_into(out: &mut Vec<RecordedAllocationSample>, limit: usize) -> usize {
    let before = out.len();
    let mut remaining = limit;
    if remaining == 0 {
        return 0;
    }

    let start_shard =
        NEXT_DRAINED_SAMPLE_SHARD.fetch_add(1, Ordering::Relaxed) % RECORDED_SAMPLE_SHARD_COUNT;
    for offset in 0..RECORDED_SAMPLE_SHARD_COUNT {
        if remaining == 0 {
            break;
        }

        let shard_index = (start_shard + offset) % RECORDED_SAMPLE_SHARD_COUNT;
        let shard = &RECORDED_SAMPLE_SHARDS[shard_index];
        let Ok(mut samples) = lock_for_report_drain(
            shard,
            out,
            remaining,
            // A shard may retain storage from a larger previous session.
            // Only evaluate this bound after a raced reservation, not per lock.
            |samples| {
                samples
                    .capacity()
                    .min(MAX_RECORDED_SAMPLES.load(Ordering::Relaxed))
            },
            VecDeque::len,
            Vec::reserve,
        ) else {
            DROPPED_SAMPLES.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        let drain_len = samples.len().min(remaining);
        out.extend(samples.drain(..drain_len));
        remaining -= drain_len;
    }

    let drained = out.len() - before;
    release_global_sample_slots(drained);
    drained
}

#[cfg(all(test, not(feature = "no_hooks")))]
fn drain_recorded_samples(limit: usize) -> Vec<RecordedAllocationSample> {
    let mut out = Vec::new();
    drain_recorded_samples_into(&mut out, limit);
    out
}

fn drain_samples_for_report(limit: usize) -> Vec<RecordedAllocationSample> {
    // Give both sources progress under a small drain limit. For a limit of one,
    // alternate the preferred source instead of starving the global backlog.
    let global_budget = if limit == 1 {
        NEXT_REPORT_SOURCE.fetch_add(1, Ordering::Relaxed) % 2
    } else {
        limit / 2
    };
    let estimate = GLOBAL_BUFFERED_SAMPLE_COUNT
        .load(Ordering::Relaxed)
        .min(global_budget);
    let mut recorded = Vec::with_capacity(estimate);
    drain_recorded_samples_into(&mut recorded, global_budget);
    let remaining = limit - recorded.len();
    drain_registered_tls_samples_for_report(&mut recorded, remaining);
    let remaining = limit - recorded.len();
    drain_recorded_samples_into(&mut recorded, remaining);
    recorded
}

fn recorded_sample_shard_capacity(total_capacity: usize) -> usize {
    total_capacity.saturating_add(RECORDED_SAMPLE_SHARD_COUNT - 1) / RECORDED_SAMPLE_SHARD_COUNT
}

#[cfg(all(test, not(feature = "no_hooks")))]
fn build_allocation_samples(
    recorded: Vec<RecordedAllocationSample>,
    max_depth: usize,
) -> Vec<AllocationSample> {
    build_memory_samples(recorded, Vec::new(), max_depth)
}

fn build_memory_samples(
    recorded: Vec<RecordedAllocationSample>,
    live_samples: Vec<live::LiveAllocationSample>,
    max_depth: usize,
) -> Vec<AllocationSample> {
    build_memory_samples_with_resolver(recorded, live_samples, max_depth, resolve_frame)
}

fn build_memory_samples_with_resolver(
    recorded: Vec<RecordedAllocationSample>,
    live_samples: Vec<live::LiveAllocationSample>,
    max_depth: usize,
    mut resolve: impl FnMut(usize) -> MemoryFrame,
) -> Vec<AllocationSample> {
    let mut aggregated: HashMap<StackKey, AggregatedAllocationSample> = HashMap::new();
    for sample in recorded {
        let entry = aggregated.entry(sample.stack).or_default();
        entry.alloc_objects += sample.weighted_objects;
        entry.alloc_space += sample.weighted_bytes;
    }
    for sample in live_samples {
        let entry = aggregated.entry(sample.stack).or_default();
        entry.inuse_objects += sample.weighted_objects;
        entry.inuse_space += sample.weighted_bytes;
    }

    // Stacks commonly share most instruction pointers. Resolve each address
    // once per report, without retaining stale symbols across dynamic unloads.
    let mut frame_cache: HashMap<usize, Option<Arc<MemoryFrame>>> = HashMap::new();

    aggregated
        .into_iter()
        .map(|(stack, sample)| {
            let frames = resolve_stack_with(&stack, max_depth, |ip| {
                frame_cache
                    .entry(ip)
                    .or_insert_with(|| filter_profiler_frame(resolve(ip)).map(Arc::new))
                    .clone()
            });
            AllocationSample {
                frames,
                alloc_objects: sample.alloc_objects.round() as i64,
                alloc_space: sample.alloc_space.round() as i64,
                // Float-to-integer casts saturate at i64::MAX for very large
                // weighted totals. Round once, after summing the entire stack.
                inuse_objects: sample.inuse_objects.round() as i64,
                inuse_space: sample.inuse_space.round() as i64,
            }
        })
        .collect()
}

fn resolve_stack_with(
    stack: &StackKey,
    max_depth: usize,
    mut resolve: impl FnMut(usize) -> Option<Arc<MemoryFrame>>,
) -> Vec<Arc<MemoryFrame>> {
    let mut frames = Vec::new();
    let mut remaining = max_depth;
    for ip in stack.iter() {
        if remaining == 0 {
            break;
        }
        let Some(frame) = resolve(ip) else {
            continue;
        };
        let depth = frame.symbols.len().max(1);
        if depth <= remaining {
            remaining -= depth;
            frames.push(frame);
        } else {
            let symbols: Vec<_> = frame.symbols.iter().take(remaining).cloned().collect();
            if !symbols.is_empty() {
                remaining -= symbols.len();
                frames.push(Arc::new(MemoryFrame {
                    address: frame.address,
                    symbols,
                }));
            }
        }
    }

    if frames.is_empty() {
        vec![Arc::new(MemoryFrame::named(SYNTHETIC_FRAME.to_string()))]
    } else {
        frames
    }
}

fn filter_profiler_frame(mut frame: MemoryFrame) -> Option<MemoryFrame> {
    if frame.symbols.is_empty() {
        // Keep unresolved physical addresses for display and remote symbolization.
        return (frame.address != 0).then_some(frame);
    }
    // Filter once while owning the symbols, then share the result across stacks.
    // A fully filtered frame is distinct from an unresolved physical address.
    frame
        .symbols
        .retain(|symbol| !is_mimalloc_profiler_frame(&symbol.name));
    (!frame.symbols.is_empty()).then_some(frame)
}

fn resolve_frame(ip: usize) -> MemoryFrame {
    // backtrace resolves return addresses at IP - 1 internally. Export the
    // same instruction so offline symbolizers see the call site too.
    let address = ip.saturating_sub(1) as u64;
    let mut resolved = Vec::new();
    backtrace::resolve(ip as *mut std::ffi::c_void, |symbol| {
        let Some(name) = symbol.name().filter(|name| !name.as_bytes().is_empty()) else {
            return;
        };
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if is_macho_header_symbol(name.as_bytes()) {
            // A stripped Mach-O's nearest symbol can be its image header, not
            // a function. Keep the IP unresolved for downstream symbolization.
            return;
        }
        resolved.push(MemorySymbol {
            name: format!("{name:#}"),
            system_name: Some(String::from_utf8_lossy(name.as_bytes()).into_owned()),
            filename: symbol
                .filename()
                .map(|path| path.to_string_lossy().into_owned()),
            line: symbol.lineno().map(i64::from).unwrap_or(0),
        });
    });
    MemoryFrame {
        address,
        symbols: resolved,
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", test))]
fn is_macho_header_symbol(name: &[u8]) -> bool {
    // Symbol tables and dladdr use different leading-underscore conventions.
    let Some(name) = name.strip_prefix(b"__").or_else(|| name.strip_prefix(b"_")) else {
        return false;
    };
    matches!(
        name,
        b"mh_execute_header"
            | b"mh_dylib_header"
            | b"mh_bundle_header"
            | b"mh_object_header"
            | b"mh_preload_header"
            | b"mh_dylinker_header"
    )
}

fn is_mimalloc_profiler_frame(name: &str) -> bool {
    let owner = name.strip_prefix('<').unwrap_or(name);
    if owner.starts_with("rustfs_mimalloc_profiling::profiler::")
        || owner.starts_with("rustfs_mimalloc_profiling::memory_pprof::")
        || owner.starts_with("backtrace::")
    {
        return true;
    }
    // These private TLS types belong to recorder scaffolding. Do not exclude
    // arbitrary application functions just because a public profiler type
    // appears among their generic arguments.
    owner.starts_with("std::thread::local::LocalKey<")
        && (name.contains("rustfs_mimalloc_profiling::profiler::SamplerState>")
            || name.contains("rustfs_mimalloc_profiling::profiler::RegisteredTlsSampleBuffer>"))
}

fn warm_backtrace() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if NATIVE_STACK_CAPTURE.load(Ordering::Relaxed) {
        let _ = StackKey::capture_native(2);
        return;
    }
    let mut frames = 0;
    backtrace::trace(|_frame| {
        frames += 1;
        frames < 2
    });
}

fn duration_to_i64_nanos(duration: std::time::Duration) -> i64 {
    i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX)
}

fn duration_to_u64_micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(all(test, not(feature = "no_hooks")))]
mod tests {
    use super::*;
    use std::sync::Mutex as TestMutex;

    static TEST_LOCK: TestMutex<()> = TestMutex::new(());

    // Force each realloc outcome without depending on a provider's size classes.
    struct ReallocProvider {
        always_move: bool,
    }

    const TEST_ALLOCATION_CAPACITY: usize = 8 * 1024 * 1024;

    // SAFETY: Each logical allocation reserves a fixed System block. All frees
    // reconstruct that exact System layout, and moves copy only the valid prefix.
    unsafe impl GlobalAlloc for ReallocProvider {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if layout.size() > TEST_ALLOCATION_CAPACITY {
                return std::ptr::null_mut();
            }
            let reserved =
                Layout::from_size_align(TEST_ALLOCATION_CAPACITY, layout.align()).unwrap();
            // SAFETY: The valid reserved layout satisfies the logical request.
            unsafe { std::alloc::System.alloc(reserved) }
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            let reserved =
                Layout::from_size_align(TEST_ALLOCATION_CAPACITY, layout.align()).unwrap();
            // SAFETY: alloc reserved this exact System layout and ownership is live.
            unsafe { std::alloc::System.dealloc(pointer, reserved) }
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            if size > TEST_ALLOCATION_CAPACITY {
                return std::ptr::null_mut();
            }
            if !self.always_move {
                return pointer;
            }
            let next_layout = Layout::from_size_align(size, layout.align()).unwrap();
            // SAFETY: The new layout is valid and remains within reserved capacity.
            let next = unsafe { self.alloc(next_layout) };
            if !next.is_null() {
                // SAFETY: Distinct live reserved blocks do not overlap. Both logical
                // allocations contain the copied prefix; release the old one last.
                unsafe {
                    std::ptr::copy_nonoverlapping(pointer, next, layout.size().min(size));
                    self.dealloc(pointer, layout);
                }
            }
            next
        }
    }

    struct RecorderActiveGuard;

    impl Drop for RecorderActiveGuard {
        fn drop(&mut self) {
            HOT_ALLOCATION_STATE
                .recorder_active
                .store(false, Ordering::Release);
            live::prepare(false, 0);
        }
    }

    fn test_sample(stack: StackKey) -> RecordedAllocationSample {
        RecordedAllocationSample {
            stack,
            weighted_objects: 1.0,
            weighted_bytes: 1024.0,
        }
    }

    fn test_live_sample(stack: StackKey) -> live::LiveAllocationSample {
        live::LiveAllocationSample {
            stack,
            weighted_objects: 1.0,
            weighted_bytes: 1024.0,
        }
    }

    fn clear_test_buffers() {
        for shard in RECORDED_SAMPLE_SHARDS.iter() {
            shard.lock().expect("lock samples").clear();
        }
        GLOBAL_BUFFERED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        // Production initialize preallocates every shard before recording.
        // Tests must honor that contract instead of relying on Vec growth.
        prepare_sample_buffer(DEFAULT_RING_CAPACITY);
        NEXT_RECORDED_SAMPLE_SHARD.store(0, Ordering::Relaxed);
        NEXT_DRAINED_SAMPLE_SHARD.store(0, Ordering::Relaxed);
        NEXT_REPORT_SOURCE.store(0, Ordering::Relaxed);
        NEXT_TLS_DRAIN_BUFFER.store(0, Ordering::Relaxed);
        SAMPLER_STATE.with(|sampler| sampler.set(SamplerState::new()));
        for buffer in registered_tls_sample_buffers() {
            let mut buffer = buffer.lock().expect("lock tls samples");
            buffer.clear();
            buffer.generation = HOT_ALLOCATION_STATE
                .sampling_config_generation
                .load(Ordering::Acquire);
        }
    }

    fn push_global_test_samples(samples: impl IntoIterator<Item = RecordedAllocationSample>) {
        push_global_test_samples_to_shard(0, samples);
    }

    fn push_global_test_samples_to_shard(
        shard_index: usize,
        samples: impl IntoIterator<Item = RecordedAllocationSample>,
    ) {
        let mut shard = RECORDED_SAMPLE_SHARDS[shard_index]
            .lock()
            .expect("lock samples");
        let initial_len = shard.len();
        shard.extend(samples);
        GLOBAL_BUFFERED_SAMPLE_COUNT.fetch_add(shard.len() - initial_len, Ordering::Relaxed);
    }

    #[test]
    fn mimalloc_config_default_is_valid() {
        assert!(MimallocConfig::default().validate().is_ok());
    }

    #[test]
    fn native_stack_capture_configuration_matches_target_support() {
        let config = MimallocConfig {
            stack_capture: MimallocStackCapture::Native,
            ..MimallocConfig::default()
        };
        assert_eq!(
            config.validate().is_ok(),
            cfg!(all(target_os = "linux", target_env = "gnu"))
        );
        assert_eq!(
            MimallocConfig::default().stack_capture,
            MimallocStackCapture::Portable
        );
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn native_stack_capture_is_bounded_and_reentrant_across_threads() {
        for depth in [1, 16, 64, 128] {
            let captured = StackKey::capture_native(depth);
            assert!(captured.depth > 0 && captured.depth <= depth.min(MAX_CAPTURE_DEPTH));
            assert!(captured.iter().all(|address| address != 0));
        }
        assert_eq!(StackKey::capture_native(0).depth, 0);
        let workers: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..100 {
                        assert!(StackKey::capture_native(16).depth > 0);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn mimalloc_config_rejects_zero_sample_interval() {
        let config = MimallocConfig {
            sample_interval_bytes: 0,
            ..MimallocConfig::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn mimalloc_config_checks_capture_and_live_metadata_bounds() {
        assert!(
            MimallocConfig {
                max_depth: 65,
                ..MimallocConfig::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            MimallocConfig {
                live_heap_tracking: true,
                max_live_samples: 0,
                ..MimallocConfig::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            MimallocConfig {
                max_live_samples: 0,
                ..MimallocConfig::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn unsampled_thread_does_not_allocate_or_register_a_tls_ring() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let previous_interval = SAMPLE_INTERVAL_BYTES.swap(u64::MAX, Ordering::Relaxed);
        HOT_ALLOCATION_STATE
            .sampling_config_generation
            .fetch_add(1, Ordering::AcqRel);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let before = registered_tls_sample_buffers().len();
        std::thread::spawn(|| {
            record_allocation(std::ptr::null_mut(), 1);
        })
        .join()
        .expect("join unsampled thread");
        assert_eq!(registered_tls_sample_buffers().len(), before);
        SAMPLE_INTERVAL_BYTES.store(previous_interval, Ordering::Relaxed);
    }

    #[test]
    fn backend_rejects_concurrent_owners_and_drop_releases_recorder() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let previous_seen = HOT_ALLOCATION_STATE
            .allocator_seen
            .swap(true, Ordering::Relaxed);
        let mut first = Profiler::new(MimallocConfig::default());
        first.initialize().expect("initialize first backend");
        let mut second = Profiler::new(MimallocConfig::default());
        assert!(second.initialize().is_err());
        drop(second);
        assert!(HOT_ALLOCATION_STATE.recorder_active.load(Ordering::Acquire));
        drop(first);
        assert!(!HOT_ALLOCATION_STATE.recorder_active.load(Ordering::Acquire));
        let mut third = Profiler::new(MimallocConfig::default());
        third.initialize().expect("initialize after owner drop");
        drop(third);
        HOT_ALLOCATION_STATE
            .allocator_seen
            .store(previous_seen, Ordering::Relaxed);
        clear_test_buffers();
    }

    #[test]
    fn report_counters_count_raw_drains_not_repeated_live_snapshots_and_reset() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let previous_seen = HOT_ALLOCATION_STATE
            .allocator_seen
            .swap(true, Ordering::Relaxed);
        let mut backend = Profiler::new(MimallocConfig {
            ring_capacity: 16,
            report_drain_limit: 1,
            live_heap_tracking: true,
            max_live_samples: 128,
            ..MimallocConfig::default()
        });
        backend.initialize().unwrap();
        let stack = StackKey {
            frames: [0; MAX_CAPTURE_DEPTH],
            depth: 0,
        };
        push_global_test_samples([test_sample(stack), test_sample(stack)]);
        live::record(
            100,
            stack,
            SampleWeight::for_allocation(1024, 1),
            HOT_ALLOCATION_STATE
                .sampling_config_generation
                .load(Ordering::Acquire),
        );
        backend.report().unwrap();
        assert_eq!(
            (mimalloc_stats().reports, mimalloc_stats().reported_samples),
            (1, 1)
        );
        backend.report().unwrap();
        assert_eq!(
            (mimalloc_stats().reports, mimalloc_stats().reported_samples),
            (2, 2)
        );
        backend.report().unwrap();
        assert_eq!(
            (mimalloc_stats().reports, mimalloc_stats().reported_samples),
            (3, 2)
        );
        drop(backend);
        let mut restarted = Profiler::new(MimallocConfig::default());
        restarted.initialize().unwrap();
        assert_eq!(
            (mimalloc_stats().reports, mimalloc_stats().reported_samples),
            (0, 0)
        );
        drop(restarted);
        HOT_ALLOCATION_STATE
            .allocator_seen
            .store(previous_seen, Ordering::Relaxed);
        clear_test_buffers();
    }

    #[test]
    fn live_snapshot_storage_tracks_empty_and_sparse_occupancy() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        live::prepare(true, DEFAULT_MAX_LIVE_SAMPLES);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let empty = live::snapshot();
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        live::record(
            100,
            stack,
            SampleWeight::for_allocation(1024, 1),
            HOT_ALLOCATION_STATE
                .sampling_config_generation
                .load(Ordering::Acquire),
        );
        let sparse = live::snapshot();
        println!(
            "live snapshot storage: empty={} bytes, one={} bytes, entry={} bytes",
            empty.capacity() * size_of::<live::LiveAllocationSample>(),
            sparse.capacity() * size_of::<live::LiveAllocationSample>(),
            size_of::<live::LiveAllocationSample>()
        );
        assert!(empty.is_empty());
        assert_eq!(empty.capacity(), 0, "idle reports need no live-copy buffer");
        assert_eq!(sparse.len(), 1);
        assert_eq!(sparse[0].stack, stack);
        assert_eq!(sparse[0].weighted_bytes, 1024.0);
        assert!(
            sparse.capacity() < DEFAULT_MAX_LIVE_SAMPLES / 64,
            "sparse reports must not reserve a whole shard's configured capacity"
        );
    }

    #[test]
    fn live_snapshot_survives_reports_and_cross_thread_free() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        live::prepare(true, 4096);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let generation = HOT_ALLOCATION_STATE
            .sampling_config_generation
            .load(Ordering::Acquire);
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        live::record(
            100,
            stack,
            SampleWeight::for_allocation(1024 * 1024, 1024),
            generation,
        );
        assert_eq!(live::snapshot().len(), 1);
        assert_eq!(live::snapshot().len(), 1);
        std::thread::spawn(|| live::remove(100))
            .join()
            .expect("cross-thread removal");
        assert!(live::snapshot().is_empty());
        assert_eq!(live::sample_count(), 0);
    }

    #[test]
    fn live_membership_filter_keeps_colliding_pointers_until_both_are_freed() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        live::prepare(true, 4096);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let generation = HOT_ALLOCATION_STATE
            .sampling_config_generation
            .load(Ordering::Acquire);
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let pointer = 100;
        let mut collisions = (101..)
            .filter(|other| splitmix64(*other as u64) % 4096 == splitmix64(pointer as u64) % 4096);
        let second = collisions
            .next()
            .expect("second pointer with same shard and membership bit");
        let untracked = collisions.next().expect("untracked colliding pointer");
        live::record(
            pointer,
            stack,
            SampleWeight::for_allocation(1024, 1),
            generation,
        );
        live::record(
            second,
            stack,
            SampleWeight::for_allocation(2048, 1),
            generation,
        );
        live::remove(untracked);
        assert_eq!(live::sample_count(), 2);
        live::remove(pointer);
        assert_eq!(live::sample_count(), 1);
        live::remove(second);
        assert_eq!(live::sample_count(), 0);
    }

    #[test]
    fn failed_reallocation_preserves_metadata_under_capacity_pressure() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        live::prepare(true, 64);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let generation = HOT_ALLOCATION_STATE
            .sampling_config_generation
            .load(Ordering::Acquire);
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let pointer = 100;
        let collision = (101..)
            .find(|other| splitmix64(*other as u64) % 64 == splitmix64(pointer as u64) % 64)
            .expect("colliding pointer");
        live::record(
            pointer,
            stack,
            SampleWeight::for_allocation(1024, 1),
            generation,
        );
        let pending = live::begin_reallocation(pointer).expect("reserve sampled pointer");
        live::record(
            collision,
            stack,
            SampleWeight::for_allocation(2048, 1),
            generation,
        );
        assert_eq!(live::dropped_sample_count(), 1);
        pending.finish(0, 0);
        assert_eq!(live::sample_count(), 1);
        assert_eq!(live::snapshot()[0].weighted_bytes, 1024.0);
        let (sample, generation) = live::begin_reallocation(pointer)
            .expect("reserve restored pointer")
            .finish(collision, 2048)
            .expect("moved sample");
        live::transfer_sample(collision, sample, generation);
        assert_eq!(live::snapshot()[0].weighted_bytes, 2048.0);
    }

    #[test]
    fn old_live_samples_and_reallocation_tokens_cannot_enter_restarted_session() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        live::prepare(true, 4096);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let old_generation = HOT_ALLOCATION_STATE
            .sampling_config_generation
            .load(Ordering::Acquire);
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        live::record(
            100,
            stack,
            SampleWeight::for_allocation(1024, 1),
            old_generation,
        );
        let pending = live::begin_reallocation(100).expect("reserve old-session pointer");
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(false, Ordering::Release);
        live::clear();
        let generation = HOT_ALLOCATION_STATE
            .sampling_config_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        live::prepare(true, 4096);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        pending.finish(0, 0);
        live::record(
            100,
            stack,
            SampleWeight::for_allocation(1024, 1),
            old_generation,
        );
        assert_eq!(live::sample_count(), 0);
        live::record(
            100,
            stack,
            SampleWeight::for_allocation(2048, 1),
            generation,
        );
        assert_eq!(live::sample_count(), 1);
    }

    #[test]
    fn report_with_one_sample_budget_alternates_tls_and_global_sources() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        clear_test_buffers();
        let global_stack = StackKey {
            frames: [1; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let tls_stack = StackKey {
            frames: [2; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        push_global_test_samples([test_sample(global_stack)]);
        TLS_SAMPLE_BUFFER.with(|buffer| {
            let mut buffer = buffer.try_lock().expect("lock TLS");
            buffer.push(test_sample(tls_stack));
            buffer.push(test_sample(tls_stack));
        });
        assert_eq!(drain_samples_for_report(1)[0].stack, tls_stack);
        assert_eq!(drain_samples_for_report(1)[0].stack, global_stack);
        clear_test_buffers();
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
    }

    #[test]
    fn memory_aggregation_combines_interval_and_live_values_by_stack() {
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let samples = build_memory_samples(
            vec![test_sample(stack)],
            vec![test_live_sample(stack), test_live_sample(stack)],
            16,
        );
        assert_eq!(samples.len(), 1);
        assert_eq!(
            (samples[0].alloc_objects, samples[0].alloc_space),
            (1, 1024)
        );
        assert_eq!(
            (samples[0].inuse_objects, samples[0].inuse_space),
            (2, 2048)
        );
    }

    #[test]
    fn live_object_weights_are_rounded_after_stack_aggregation() {
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let sample = live::LiveAllocationSample {
            stack,
            weighted_objects: 1.582,
            weighted_bytes: 1620.5,
        };
        let samples = build_memory_samples(Vec::new(), vec![sample, sample], 8);
        assert_eq!(samples[0].inuse_objects, 3);
        assert_eq!(samples[0].inuse_space, 3241);
    }

    #[test]
    fn mimalloc_stats_reports_global_recorder_counters() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        clear_test_buffers();
        RECORDED_SAMPLE_COUNT.store(7, Ordering::Relaxed);
        REPORTED_SAMPLE_COUNT.store(5, Ordering::Relaxed);
        REPORT_COUNT.store(2, Ordering::Relaxed);
        DROPPED_SAMPLES.store(3, Ordering::Relaxed);
        LAST_PPROF_ENCODE_ELAPSED_MICROS.store(11, Ordering::Relaxed);

        let stats = mimalloc_stats();

        assert_eq!(
            stats,
            MimallocStats {
                recorded_samples: 7,
                reported_samples: 5,
                reports: 2,
                flushes: 0,
                flushed_samples: 0,
                dropped_samples: 3,
                buffered_samples: Some(0),
                last_pprof_encode_elapsed_micros: 11,
                live_samples: 0,
                dropped_live_samples: 0,
                live_metadata_payload_bytes: 0,
            }
        );

        RECORDED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        REPORTED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        REPORT_COUNT.store(0, Ordering::Relaxed);
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        LAST_PPROF_ENCODE_ELAPSED_MICROS.store(0, Ordering::Relaxed);
        clear_test_buffers();
    }

    #[test]
    fn stack_identity_ignores_unused_capture_slots() {
        let mut first = StackKey {
            frames: [0; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        first.frames[0] = 42;
        let second = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        assert_eq!(first, second);
        let samples =
            build_memory_samples(vec![test_sample(first), test_sample(second)], Vec::new(), 8);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].alloc_objects, 2);
    }

    #[test]
    fn previous_session_ring_cannot_flush_into_new_global_shards() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        clear_test_buffers();
        MAX_RECORDED_SAMPLES.store(10, Ordering::Relaxed);
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let mut buffer = TlsSampleBuffer::new();
        buffer.push(test_sample(stack));
        HOT_ALLOCATION_STATE
            .sampling_config_generation
            .fetch_add(1, Ordering::AcqRel);
        assert!(flush_tls_samples_on_exit(&mut buffer));
        assert!(buffer.is_empty());
        assert_eq!(count_recorded_samples(), 0);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);
        MAX_RECORDED_SAMPLES.store(DEFAULT_RING_CAPACITY, Ordering::Relaxed);
        clear_test_buffers();
    }

    #[test]
    fn abandoned_reallocation_token_restores_its_live_slot() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        live::prepare(true, 64);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let generation = HOT_ALLOCATION_STATE
            .sampling_config_generation
            .load(Ordering::Acquire);
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        live::record(
            100,
            stack,
            SampleWeight::for_allocation(1024, 1),
            generation,
        );
        drop(live::begin_reallocation(100).expect("reserve live slot"));
        assert_eq!(live::sample_count(), 1);
        live::remove(100);
        assert_eq!(live::sample_count(), 0);
    }

    #[test]
    fn suppressed_and_reentrant_frees_still_remove_live_metadata() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        live::prepare(true, 4096);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let allocator = SamplingAllocator::with_allocator(std::alloc::System);
        let layout = Layout::from_size_align(1024, 8).expect("layout");
        let generation = HOT_ALLOCATION_STATE
            .sampling_config_generation
            .load(Ordering::Acquire);
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        for reentrant in [false, true] {
            // SAFETY: Allocate valid storage directly through the wrapped
            // allocator so the test can install an exact live sample.
            let pointer = unsafe { allocator.inner.alloc(layout) };
            assert!(!pointer.is_null());
            live::record(
                pointer as usize,
                stack,
                SampleWeight::for_allocation(1024, 1),
                generation,
            );
            assert_eq!(live::sample_count(), 1);
            SAMPLER_STATE.with(|sampler| {
                let mut state = sampler.get();
                state.in_profiler = reentrant;
                sampler.set(state);
            });
            // SAFETY: The pointer is live and layout matches its allocation.
            with_profiler_suppressed(|| unsafe { allocator.dealloc(pointer, layout) });
            SAMPLER_STATE.with(|sampler| {
                let mut state = sampler.get();
                state.in_profiler = false;
                sampler.set(state);
            });
            assert_eq!(live::sample_count(), 0);
        }
    }

    #[test]
    fn tracked_reallocation_below_byte_remainder_keeps_live_sample_without_allocation_event() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        clear_test_buffers();
        live::prepare(true, 4096);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let allocator = SamplingAllocator::with_allocator(ReallocProvider { always_move: false });
        let layout = Layout::from_size_align(1024, 8).expect("layout");
        for suppressed in [false, true] {
            let new_size = if suppressed { 1536 } else { 768 };
            // SAFETY: Allocate valid storage directly through mimalloc.
            let pointer = unsafe { allocator.inner.alloc(layout) };
            assert!(!pointer.is_null());
            let generation = HOT_ALLOCATION_STATE
                .sampling_config_generation
                .load(Ordering::Acquire);
            let stack = StackKey {
                frames: [42; MAX_CAPTURE_DEPTH],
                depth: 1,
            };
            live::record(
                pointer as usize,
                stack,
                SampleWeight::for_allocation(1024, 1),
                generation,
            );
            SAMPLER_STATE.with(|sampler| {
                let mut state = sampler.get();
                state.remaining_bytes = 1024 * 1024;
                state.remaining_config_generation = generation;
                sampler.set(state);
            });
            RECORDED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
            // SAFETY: The pointer is live with the supplied layout.
            let resize = || unsafe { allocator.realloc(pointer, layout, new_size) };
            let resized = if suppressed {
                with_profiler_suppressed(resize)
            } else {
                resize()
            };
            assert!(!resized.is_null());
            if !suppressed {
                assert_eq!(
                    resized, pointer,
                    "controlled provider keeps the reserved block in place"
                );
            }
            assert_eq!(live::sample_count(), 1);
            assert_eq!(live::snapshot()[0].weighted_bytes, new_size as f64);
            assert_eq!(live::snapshot()[0].stack, stack);
            assert_eq!(RECORDED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);
            // SAFETY: Successful realloc transfers ownership to resized.
            unsafe { allocator.dealloc(resized, Layout::from_size_align(new_size, 8).unwrap()) };
        }
        clear_test_buffers();
    }

    #[test]
    fn failed_provider_realloc_restores_the_existing_live_record() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_test_buffers();
        live::prepare(true, 64);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let allocator = SamplingAllocator::with_allocator(ReallocProvider { always_move: false });
        let layout = Layout::from_size_align(1024, 64).unwrap();
        // SAFETY: Valid layout, with ownership retained until the final dealloc.
        let pointer = unsafe { allocator.inner.alloc(layout) };
        assert!(!pointer.is_null());
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        live::record(
            pointer as usize,
            stack,
            SampleWeight::for_allocation(1024, 1),
            HOT_ALLOCATION_STATE
                .sampling_config_generation
                .load(Ordering::Acquire),
        );
        // SAFETY: The provider deterministically rejects this valid request.
        let failed = unsafe { allocator.realloc(pointer, layout, TEST_ALLOCATION_CAPACITY + 1) };
        assert!(failed.is_null());
        let samples = live::snapshot();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].stack, stack);
        assert_eq!(samples[0].weighted_bytes, 1024.0);
        // SAFETY: Failed realloc leaves the original ownership/layout valid.
        unsafe { allocator.dealloc(pointer, layout) };
        assert_eq!(live::sample_count(), 0);
        clear_test_buffers();
    }

    #[test]
    fn moved_reallocation_samples_new_physical_allocations_independently() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        clear_test_buffers();
        let previous_interval = SAMPLE_INTERVAL_BYTES.swap(4096, Ordering::Relaxed);
        live::prepare(true, 4096);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let allocator = SamplingAllocator::with_allocator(ReallocProvider { always_move: true });
        let layout = Layout::from_size_align(128, 8).unwrap();
        // SAFETY: A valid layout is supplied to the wrapped allocator.
        let pointer = unsafe { allocator.inner.alloc(layout) };
        assert!(!pointer.is_null());
        assert_eq!(live::sample_count(), 0);
        // SAFETY: pointer is live and layout matches its original allocation.
        let replacement = unsafe { allocator.realloc(pointer, layout, 4 * 1024 * 1024) };
        assert!(!replacement.is_null());
        assert_ne!(replacement, pointer);
        assert_eq!(live::sample_count(), 1);
        assert_eq!(live::snapshot()[0].weighted_bytes, (4 * 1024 * 1024) as f64);
        // SAFETY: Successful realloc supplies the replacement's ownership/layout.
        unsafe {
            allocator.dealloc(
                replacement,
                Layout::from_size_align(4 * 1024 * 1024, 8).unwrap(),
            )
        };
        SAMPLE_INTERVAL_BYTES.store(previous_interval, Ordering::Relaxed);
        clear_test_buffers();
    }

    #[test]
    fn unregistered_thread_exit_discards_its_previous_session_ring() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        clear_test_buffers();
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let registered = RegisteredTlsSampleBuffer::new();
            if let Some(id) = registered.id.take() {
                deregister_tls_sample_buffer(id);
            }
            let stack = StackKey {
                frames: [42; MAX_CAPTURE_DEPTH],
                depth: 1,
            };
            registered
                .buffer
                .lock()
                .expect("lock detached ring")
                .push(test_sample(stack));
            ready_tx.send(()).expect("send old ring ready");
            release_rx.recv().expect("wait for new session");
        });
        ready_rx.recv().expect("wait for old ring");
        HOT_ALLOCATION_STATE
            .sampling_config_generation
            .fetch_add(1, Ordering::AcqRel);
        release_tx.send(()).expect("exit worker in new session");
        worker.join().expect("join old-session worker");
        assert_eq!(count_recorded_samples(), 0);
        clear_test_buffers();
    }

    #[test]
    fn mimalloc_stats_includes_current_thread_tls_samples() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        push_global_test_samples([test_sample(stack)]);
        TLS_SAMPLE_BUFFER.with(|buffer| {
            let mut buffer = buffer.try_lock().expect("lock current thread buffer");
            assert!(buffer.push(test_sample(stack)));
            assert!(buffer.push(test_sample(stack)));
        });

        assert_eq!(mimalloc_stats().buffered_samples, Some(3));

        clear_test_buffers();
    }

    #[test]
    fn profiler_suppression_skips_internal_allocations_without_drop_count() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        clear_test_buffers();
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active_guard = RecorderActiveGuard;
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        RECORDED_SAMPLE_COUNT.store(0, Ordering::Relaxed);

        with_profiler_suppressed(|| record_allocation(std::ptr::null_mut(), 1));

        assert_eq!(RECORDED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);
        assert_eq!(DROPPED_SAMPLES.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn reentrant_allocation_is_ignored_without_drop_count() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        clear_test_buffers();
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active_guard = RecorderActiveGuard;
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        RECORDED_SAMPLE_COUNT.store(0, Ordering::Relaxed);

        SAMPLER_STATE.with(|sampler| {
            let mut state = sampler.get();
            let previous = state.in_profiler;
            state.in_profiler = true;
            sampler.set(state);
            record_allocation(std::ptr::null_mut(), 1);
            let mut state = sampler.get();
            state.in_profiler = previous;
            sampler.set(state);
        });

        assert_eq!(RECORDED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);
        assert_eq!(DROPPED_SAMPLES.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn realloc_recorded_size_tracks_newly_allocated_bytes() {
        let mut old_byte = 0_u8;
        let mut new_byte = 0_u8;
        let old_ptr = &mut old_byte as *mut u8;
        let new_ptr = &mut new_byte as *mut u8;

        assert_eq!(realloc_recorded_size(old_ptr, old_ptr, 1024, 1536), 512);
        assert_eq!(realloc_recorded_size(old_ptr, old_ptr, 1536, 1024), 0);
        assert_eq!(realloc_recorded_size(old_ptr, new_ptr, 1024, 1536), 1536);
        assert_eq!(realloc_recorded_size(old_ptr, new_ptr, 1536, 1024), 1024);
    }

    #[test]
    fn report_storage_growth_releases_source_lock_and_keeps_new_admissions() {
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let source = Mutex::new(VecDeque::from([test_sample(stack)]));
        let mut out = Vec::new();
        let mut reservations = 0;
        let mut locked = lock_for_report_drain(
            &source,
            &mut out,
            8,
            |_| 8,
            VecDeque::len,
            |out, additional| {
                let mut source = source
                    .try_lock()
                    .expect("reservation must run outside the source lock");
                if reservations == 0 {
                    source.extend(std::iter::repeat_n(test_sample(stack), 7));
                }
                drop(source);
                out.reserve(additional);
                reservations += 1;
            },
        )
        .unwrap();
        let capacity = out.capacity();
        out.extend(locked.drain(..));
        assert_eq!(out.len(), 8);
        assert_eq!(
            out.capacity(),
            capacity,
            "copying a locked batch must not grow output"
        );
        assert!(reservations > 0);
    }

    #[test]
    fn report_retry_clips_retained_shard_storage_to_the_session_quota() {
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let source = Mutex::new(VecDeque::with_capacity(128));
        source.lock().unwrap().push_back(test_sample(stack));
        let mut out = Vec::new();
        let mut reservations = Vec::new();
        let bound_reads = Cell::new(0);
        let locked = lock_for_report_drain(
            &source,
            &mut out,
            1024,
            |source| {
                bound_reads.set(bound_reads.get() + 1);
                source.capacity().min(64)
            },
            VecDeque::len,
            |out, additional| {
                let mut source = source.try_lock().expect("reserve outside source lock");
                if reservations.is_empty() {
                    source.extend(std::iter::repeat_n(test_sample(stack), 7));
                }
                drop(source);
                out.reserve_exact(additional);
                reservations.push(additional);
            },
        )
        .unwrap();
        assert_eq!(locked.len(), 8);
        assert_eq!(reservations, [1, 64]);
        assert_eq!(bound_reads.get(), 1);
    }

    #[test]
    fn sparse_report_storage_uses_occupancy_and_empty_sources_reserve_nothing() {
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let source = Mutex::new(VecDeque::new());
        let mut out = Vec::new();
        drop(
            lock_for_report_drain(
                &source,
                &mut out,
                1_000_000,
                |_| panic!("empty source must not read the retry bound"),
                VecDeque::len,
                |_, _| panic!("empty source must not reserve"),
            )
            .unwrap(),
        );
        assert_eq!(out.capacity(), 0);
        source.lock().unwrap().push_back(test_sample(stack));
        let mut locked = lock_for_report_drain(
            &source,
            &mut out,
            1_000_000,
            |_| panic!("stable sparse source must not read the retry bound"),
            VecDeque::len,
            Vec::reserve,
        )
        .unwrap();
        let capacity = out.capacity();
        out.extend(locked.drain(..));
        assert_eq!(out.len(), 1);
        assert_eq!(out.capacity(), capacity);
        assert!(
            capacity < 1_000_000,
            "sparse source must not reserve the drain limit"
        );
    }

    #[test]
    fn zero_tls_drain_budget_does_not_wait_for_the_registry() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let registry = TLS_SAMPLE_BUFFER_REGISTRY.lock().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut out = Vec::new();
            send.send(drain_registered_tls_samples_for_report(&mut out, 0))
                .unwrap();
        });
        let result = receive.recv_timeout(std::time::Duration::from_secs(2));
        drop(registry);
        worker.join().unwrap();
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn appending_global_samples_releases_only_the_moved_global_reservations() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        push_global_test_samples([test_sample(stack); 3]);
        let mut out = Vec::with_capacity(4);
        out.push(test_sample(stack)); // a prefix from another report source
        let pointer = out.as_ptr();
        assert_eq!(drain_recorded_samples_into(&mut out, 2), 2);
        assert_eq!(out.len(), 3);
        assert_eq!(out.as_ptr(), pointer);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 1);
        assert_eq!(count_recorded_samples(), 1);
        clear_test_buffers();
    }

    #[test]
    fn drain_recorded_samples_respects_limit_and_keeps_remaining_samples() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        push_global_test_samples([test_sample(stack), test_sample(stack), test_sample(stack)]);

        let drained = drain_recorded_samples(2);

        assert_eq!(drained.len(), 2);
        assert_eq!(mimalloc_stats().buffered_samples, Some(1));

        clear_test_buffers();
    }

    #[test]
    fn drain_recorded_samples_rotates_starting_shard() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let low_stack = StackKey {
            frames: [1; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let high_stack = StackKey {
            frames: [2; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        push_global_test_samples_to_shard(0, [test_sample(low_stack)]);
        push_global_test_samples_to_shard(1, [test_sample(high_stack)]);
        NEXT_DRAINED_SAMPLE_SHARD.store(1, Ordering::Relaxed);

        let first = drain_recorded_samples(1);
        let second = drain_recorded_samples(1);

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].stack, high_stack);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].stack, low_stack);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);

        clear_test_buffers();
    }

    #[test]
    fn release_global_sample_slots_saturates_at_zero() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        GLOBAL_BUFFERED_SAMPLE_COUNT.store(0, Ordering::Relaxed);

        release_global_sample_slots(3);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);

        GLOBAL_BUFFERED_SAMPLE_COUNT.store(2, Ordering::Relaxed);
        release_global_sample_slots(5);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);

        GLOBAL_BUFFERED_SAMPLE_COUNT.store(5, Ordering::Relaxed);
        release_global_sample_slots(3);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 2);

        GLOBAL_BUFFERED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
    }

    #[test]
    fn prepare_sample_buffer_recounts_global_sample_slots_after_clear() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        push_global_test_samples([test_sample(stack), test_sample(stack)]);
        GLOBAL_BUFFERED_SAMPLE_COUNT.store(usize::MAX, Ordering::Relaxed);

        prepare_sample_buffer(10);

        assert_eq!(count_recorded_samples(), 0);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);

        clear_test_buffers();
    }

    #[test]
    fn tls_sample_buffer_drain_into_respects_limit() {
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let mut buffer = TlsSampleBuffer::new();
        assert!(buffer.push(test_sample(stack)));
        assert!(buffer.push(test_sample(stack)));
        assert!(buffer.push(test_sample(stack)));
        let mut out = Vec::new();

        let drained = buffer.drain_into(&mut out, 2);

        assert_eq!(drained, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn tls_sample_buffer_registry_reuses_deregistered_slots() {
        let _guard = TEST_LOCK.lock().expect("lock test");

        let first_id = register_tls_sample_buffer(Arc::new(Mutex::new(TlsSampleBuffer::new())))
            .expect("register first buffer");
        deregister_tls_sample_buffer(first_id);

        let second_id = register_tls_sample_buffer(Arc::new(Mutex::new(TlsSampleBuffer::new())))
            .expect("register second buffer");

        assert_eq!(second_id, first_id);
        deregister_tls_sample_buffer(second_id);
    }

    #[test]
    fn allocation_after_tls_ring_teardown_is_counted_as_dropped() {
        struct LateAllocation;
        impl Drop for LateAllocation {
            fn drop(&mut self) {
                record_allocation(std::ptr::null_mut(), 4096);
            }
        }
        thread_local! {
            static LATE_ALLOCATION: LateAllocation = const { LateAllocation };
        }
        let _guard = TEST_LOCK.lock().unwrap();
        clear_test_buffers();
        let previous_interval = SAMPLE_INTERVAL_BYTES.swap(1, Ordering::Relaxed);
        HOT_ALLOCATION_STATE
            .sampling_config_generation
            .fetch_add(1, Ordering::AcqRel);
        RECORDED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        std::thread::spawn(|| {
            // Reverse destruction order closes the recorder before this last
            // allocation, as can happen in an application's other TLS values.
            LATE_ALLOCATION.with(|_| {});
            TLS_SAMPLE_BUFFER.with(|_| {});
        })
        .join()
        .unwrap();
        SAMPLE_INTERVAL_BYTES.store(previous_interval, Ordering::Relaxed);
        assert_eq!(RECORDED_SAMPLE_COUNT.load(Ordering::Relaxed), 0);
        assert_eq!(DROPPED_SAMPLES.load(Ordering::Relaxed), 1);
        clear_test_buffers();
    }

    #[test]
    fn registry_contention_never_accepts_samples_hidden_from_the_reporter() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_test_buffers();
        live::prepare(false, 0);
        let previous_interval = SAMPLE_INTERVAL_BYTES.swap(1, Ordering::Relaxed);
        HOT_ALLOCATION_STATE
            .sampling_config_generation
            .fetch_add(1, Ordering::AcqRel);
        RECORDED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active = RecorderActiveGuard;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (continue_tx, continue_rx) = std::sync::mpsc::channel();
        let registry = TLS_SAMPLE_BUFFER_REGISTRY.lock().unwrap();
        let worker = std::thread::spawn(move || {
            record_allocation(std::ptr::null_mut(), 4096);
            ready_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
            record_allocation(std::ptr::null_mut(), 4096);
            ready_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
        });
        ready_rx.recv().unwrap();
        drop(registry);
        let accepted_while_contended = RECORDED_SAMPLE_COUNT.load(Ordering::Relaxed);
        let first = drain_samples_for_report(10);
        let dropped_while_contended = DROPPED_SAMPLES.load(Ordering::Relaxed);
        continue_tx.send(()).unwrap();
        ready_rx.recv().unwrap();
        // The worker is idle: reporting must not require another allocation.
        let retried = drain_samples_for_report(10);
        continue_tx.send(()).unwrap();
        worker.join().unwrap();
        SAMPLE_INTERVAL_BYTES.store(previous_interval, Ordering::Relaxed);
        clear_test_buffers();
        assert_eq!(
            accepted_while_contended,
            first.len() as u64,
            "accepted samples were invisible while their thread was idle"
        );
        assert_eq!(accepted_while_contended, 0);
        assert_eq!(dropped_while_contended, 1);
        assert_eq!(retried.len(), 1);
    }

    #[test]
    fn tls_sample_buffer_retries_registration_after_initial_contention() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let registry_guard = TLS_SAMPLE_BUFFER_REGISTRY.lock().expect("lock registry");

        let buffer = RegisteredTlsSampleBuffer::new();
        assert_eq!(buffer.id.get(), None);
        assert!(buffer.try_lock().is_none());
        assert_eq!(buffer.id.get(), None);

        drop(registry_guard);
        drop(buffer.try_lock().expect("lock local buffer after retry"));

        let id = buffer.id.get().expect("registration retried");
        deregister_tls_sample_buffer(id);
        buffer.id.set(None);
    }

    #[test]
    fn flush_tls_samples_moves_buffer_to_global_samples() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        MAX_RECORDED_SAMPLES.store(10, Ordering::Relaxed);
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        let mut buffer = TlsSampleBuffer::new();
        assert!(buffer.push(test_sample(stack)));
        assert!(buffer.push(test_sample(stack)));

        assert!(flush_tls_samples(&mut buffer));

        assert_eq!(buffer.len(), 0);
        assert_eq!(mimalloc_stats().flushes, 1);
        assert_eq!(mimalloc_stats().flushed_samples, 2);
        assert_eq!(mimalloc_stats().buffered_samples, Some(2));
        clear_test_buffers();
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        MAX_RECORDED_SAMPLES.store(DEFAULT_RING_CAPACITY, Ordering::Relaxed);
    }

    #[test]
    fn flush_tls_samples_keeps_tls_samples_when_global_capacity_is_full() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        MAX_RECORDED_SAMPLES.store(0, Ordering::Relaxed);
        let mut buffer = TlsSampleBuffer::new();
        assert!(buffer.push(test_sample(stack)));
        assert!(buffer.push(test_sample(stack)));

        assert!(!flush_tls_samples(&mut buffer));

        assert_eq!(buffer.len(), 2);
        assert_eq!(DROPPED_SAMPLES.load(Ordering::Relaxed), 0);
        MAX_RECORDED_SAMPLES.store(DEFAULT_RING_CAPACITY, Ordering::Relaxed);
        clear_test_buffers();
    }

    #[test]
    fn flush_tls_samples_keeps_unflushed_tls_samples_after_partial_flush() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        MAX_RECORDED_SAMPLES.store(1, Ordering::Relaxed);
        let mut buffer = TlsSampleBuffer::new();
        assert!(buffer.push(test_sample(stack)));
        assert!(buffer.push(test_sample(stack)));

        assert!(!flush_tls_samples(&mut buffer));

        assert_eq!(buffer.len(), 1);
        assert_eq!(count_recorded_samples(), 1);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 1);
        assert_eq!(DROPPED_SAMPLES.load(Ordering::Relaxed), 0);
        MAX_RECORDED_SAMPLES.store(DEFAULT_RING_CAPACITY, Ordering::Relaxed);
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        clear_test_buffers();
    }

    #[test]
    fn report_tls_drain_bypasses_full_global_buffer() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let global_stack = StackKey {
            frames: [1; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let tls_stack = StackKey {
            frames: [2; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        MAX_RECORDED_SAMPLES.store(1, Ordering::Relaxed);
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        push_global_test_samples([test_sample(global_stack)]);

        let tls_buffer = Arc::new(Mutex::new(TlsSampleBuffer::new()));
        {
            let mut buffer = tls_buffer.lock().expect("lock tls buffer");
            assert!(buffer.push(test_sample(tls_stack)));
            assert!(buffer.push(test_sample(tls_stack)));
        }
        let id = register_tls_sample_buffer(tls_buffer.clone()).expect("register tls buffer");
        let mut recorded = Vec::new();

        let drained = drain_registered_tls_samples_for_report(&mut recorded, 10);

        assert_eq!(drained, 2);
        assert_eq!(recorded.len(), 2);
        assert!(recorded.iter().all(|sample| sample.stack == tls_stack));
        assert_eq!(tls_buffer.lock().expect("lock tls buffer").len(), 0);
        assert_eq!(count_recorded_samples(), 1);
        assert_eq!(GLOBAL_BUFFERED_SAMPLE_COUNT.load(Ordering::Relaxed), 1);
        assert_eq!(mimalloc_stats().flushes, 1);
        assert_eq!(mimalloc_stats().flushed_samples, 2);

        deregister_tls_sample_buffer(id);
        clear_test_buffers();
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        MAX_RECORDED_SAMPLES.store(DEFAULT_RING_CAPACITY, Ordering::Relaxed);
    }

    #[test]
    fn tls_sample_buffer_flushes_on_thread_exit_when_recorder_is_active() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        MAX_RECORDED_SAMPLES.store(10, Ordering::Relaxed);
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active_guard = RecorderActiveGuard;

        std::thread::spawn(move || {
            TLS_SAMPLE_BUFFER.with(|buffer| {
                let mut buffer = buffer.try_lock().expect("lock worker buffer");
                assert!(buffer.push(test_sample(stack)));
                assert!(buffer.push(test_sample(stack)));
            });
        })
        .join()
        .expect("join allocation thread");

        let stats = mimalloc_stats();
        assert!(matches!(stats.buffered_samples, Some(samples) if samples >= 2));
        assert!(stats.flushes >= 1);
        assert!(stats.flushed_samples >= 2);
        assert_eq!(stats.dropped_samples, 0);
        assert_eq!(registered_tls_buffered_samples(), Some(0));

        clear_test_buffers();
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        MAX_RECORDED_SAMPLES.store(DEFAULT_RING_CAPACITY, Ordering::Relaxed);
    }

    #[test]
    fn tls_sample_buffer_counts_drop_when_thread_exit_flush_has_no_global_capacity() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        MAX_RECORDED_SAMPLES.store(0, Ordering::Relaxed);
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        HOT_ALLOCATION_STATE
            .recorder_active
            .store(true, Ordering::Release);
        let _active_guard = RecorderActiveGuard;

        std::thread::spawn(move || {
            TLS_SAMPLE_BUFFER.with(|buffer| {
                let mut buffer = buffer.try_lock().expect("lock worker buffer");
                assert!(buffer.push(test_sample(stack)));
                assert!(buffer.push(test_sample(stack)));
            });
        })
        .join()
        .expect("join allocation thread");

        let stats = mimalloc_stats();
        assert_eq!(stats.dropped_samples, 2);
        assert_eq!(stats.flushes, 0);
        assert_eq!(stats.flushed_samples, 0);
        assert_eq!(stats.buffered_samples, Some(0));
        assert_eq!(registered_tls_buffered_samples(), Some(0));

        clear_test_buffers();
        FLUSH_COUNT.store(0, Ordering::Relaxed);
        FLUSHED_SAMPLE_COUNT.store(0, Ordering::Relaxed);
        DROPPED_SAMPLES.store(0, Ordering::Relaxed);
        MAX_RECORDED_SAMPLES.store(DEFAULT_RING_CAPACITY, Ordering::Relaxed);
    }

    #[test]
    fn clear_registered_tls_samples_drains_all_live_thread_buffers() {
        let _guard = TEST_LOCK.lock().expect("lock test");
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        clear_test_buffers();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let worker = std::thread::spawn(move || {
            TLS_SAMPLE_BUFFER.with(|buffer| {
                let mut buffer = buffer.try_lock().expect("lock worker buffer");
                assert!(buffer.push(test_sample(stack)));
                assert!(buffer.push(test_sample(stack)));
            });
            ready_tx.send(()).expect("send ready");
            release_rx.recv().expect("wait for release");
        });

        ready_rx.recv().expect("wait for worker buffer");
        assert_eq!(registered_tls_buffered_samples(), Some(2));

        clear_registered_tls_samples();

        assert_eq!(registered_tls_buffered_samples(), Some(0));
        release_tx.send(()).expect("release worker");
        worker.join().expect("join worker");
        clear_test_buffers();
    }

    #[test]
    fn build_allocation_samples_groups_matching_stacks() {
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let samples = build_allocation_samples(
            vec![
                RecordedAllocationSample {
                    stack,
                    weighted_objects: 8.0,
                    weighted_bytes: 1024.0,
                },
                RecordedAllocationSample {
                    stack,
                    weighted_objects: 4.0,
                    weighted_bytes: 1024.0,
                },
            ],
            DEFAULT_MAX_DEPTH,
        );

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].alloc_objects, 12);
        assert_eq!(samples[0].alloc_space, 2048);
    }

    #[test]
    fn resolved_frame_address_matches_backtrace_call_site_adjustment() {
        let address = resolve_frame as *const () as usize;
        assert_eq!(resolve_frame(address + 1).address, address as u64);
        assert_eq!(resolve_frame(0).address, 0);
    }

    #[test]
    fn macho_image_headers_are_not_function_symbols() {
        for kind in [
            "execute", "dylib", "bundle", "object", "preload", "dylinker",
        ] {
            for prefix in ["_", "__"] {
                assert!(is_macho_header_symbol(
                    format!("{prefix}mh_{kind}_header").as_bytes()
                ));
            }
        }
        for name in [
            "mh_execute_header",
            "___mh_execute_header",
            "__mh_execute_header_suffix",
            "application::__mh_execute_header",
            "application::allocate",
        ] {
            assert!(!is_macho_header_symbol(name.as_bytes()));
        }
    }

    #[test]
    fn rust_symbol_normalization_removes_hashes_before_profiler_filtering() {
        let names: &[&[u8]] = &[
            b"_ZN25rustfs_mimalloc_profiling8profiler13record_sample17h0123456789abcdefE",
            b"_RNvNtCs4fqI2P2rA04_25rustfs_mimalloc_profiling8profiler13record_sample",
        ];
        for raw in names {
            let symbol = backtrace::SymbolName::new(raw);
            let display = format!("{symbol:#}");
            assert_eq!(
                display,
                "rustfs_mimalloc_profiling::profiler::record_sample"
            );
            assert!(is_mimalloc_profiler_frame(&display));
            assert_eq!(symbol.as_bytes(), *raw);
        }
    }

    #[test]
    fn profiler_filter_preserves_application_functions_with_public_profiler_type_arguments() {
        for name in [
            "application::store::<rustfs_mimalloc_profiling::profiler::MimallocConfig>",
            "<application::Store<rustfs_mimalloc_profiling::profiler::SamplingMiMalloc> as application::Trait>::reserve",
            "<alloc::vec::Vec<rustfs_mimalloc_profiling::profiler::MimallocConfig>>::reserve",
            "application::inspect::<backtrace::Backtrace>",
            "std::sys::backtrace::__rust_begin_short_backtrace::<application::run, ()>",
            "test::__rust_begin_short_backtrace::<core::result::Result<(), ()>, application::run>",
        ] {
            assert!(
                !is_mimalloc_profiler_frame(name),
                "application frame removed: {name}"
            );
        }
        assert!(is_mimalloc_profiler_frame(
            "<rustfs_mimalloc_profiling::profiler::SamplingMiMalloc as core::alloc::global::GlobalAlloc>::alloc"
        ));
        assert!(is_mimalloc_profiler_frame(
            "<std::thread::local::LocalKey<core::cell::Cell<rustfs_mimalloc_profiling::profiler::SamplerState>>>::try_with"
        ));
        assert!(is_mimalloc_profiler_frame(
            "<std::thread::local::LocalKey<rustfs_mimalloc_profiling::profiler::RegisteredTlsSampleBuffer>>::try_with"
        ));
    }

    #[test]
    fn filtered_inline_frames_are_shared_and_resolved_once_per_report() {
        let mut first = StackKey {
            frames: [0; MAX_CAPTURE_DEPTH],
            depth: 2,
        };
        first.frames[..2].copy_from_slice(&[1, 2]);
        let mut second = first;
        second.frames[1] = 3;
        let recorded = [first, second]
            .into_iter()
            .map(|stack| RecordedAllocationSample {
                stack,
                weighted_objects: 1.0,
                weighted_bytes: 1024.0,
            })
            .collect();
        let mut resolutions = HashMap::<usize, usize>::new();
        let samples = build_memory_samples_with_resolver(recorded, Vec::new(), 64, |ip| {
            *resolutions.entry(ip).or_default() += 1;
            let names = if ip == 1 {
                vec![
                    "rustfs_mimalloc_profiling::profiler::record_sample",
                    "example::allocate",
                    "example::caller",
                ]
            } else {
                vec!["example::root"]
            };
            MemoryFrame {
                address: ip as u64,
                symbols: names
                    .into_iter()
                    .map(|name| MemorySymbol {
                        name: name.into(),
                        system_name: None,
                        filename: None,
                        line: 0,
                    })
                    .collect(),
            }
        });
        assert_eq!(resolutions, HashMap::from([(1, 1), (2, 1), (3, 1)]));
        assert_eq!(samples.len(), 2);
        assert!(Arc::ptr_eq(&samples[0].frames[0], &samples[1].frames[0]));
        assert_eq!(samples[0].frames[0].symbols.len(), 2);
        assert_eq!(samples[0].frames[0].symbols[0].name, "example::allocate");
    }

    #[test]
    fn unresolved_addresses_survive_depth_limits_and_profiler_filtering() {
        let mut stack = StackKey {
            frames: [0; MAX_CAPTURE_DEPTH],
            depth: 2,
        };
        stack.frames[..2].copy_from_slice(&[7, 8]);
        let frames = resolve_stack_with(&stack, 1, |ip| {
            filter_profiler_frame(MemoryFrame {
                address: ip as u64,
                symbols: Vec::new(),
            })
            .map(Arc::new)
        });
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].address, 7);
        assert!(frames[0].symbols.is_empty());
        assert!(
            filter_profiler_frame(MemoryFrame {
                address: 0,
                symbols: Vec::new()
            })
            .is_none()
        );
    }

    #[test]
    fn resolve_stack_applies_depth_after_profiler_frame_filtering() {
        let mut frames = [0; MAX_CAPTURE_DEPTH];
        frames[..4].copy_from_slice(&[1, 2, 3, 4]);
        let stack = StackKey { frames, depth: 4 };

        let resolved = resolve_stack_with(&stack, 1, |ip| {
            let name = match ip {
                1 => "rustfs_mimalloc_profiling::profiler::record_sample",
                2 => "backtrace::trace_unsynchronized",
                3 => "example::allocate",
                4 => "example::caller",
                _ => "unknown",
            };
            filter_profiler_frame(MemoryFrame {
                address: ip as u64,
                symbols: vec![MemorySymbol {
                    name: name.into(),
                    system_name: None,
                    filename: None,
                    line: 0,
                }],
            })
            .map(Arc::new)
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].address, 3);
        assert_eq!(resolved[0].symbols[0].name, "example::allocate");
    }

    #[test]
    fn resolve_stack_expands_inline_symbols_before_filtering() {
        let mut frames = [0; MAX_CAPTURE_DEPTH];
        frames[0] = 1;
        let stack = StackKey { frames, depth: 1 };

        let resolved = resolve_stack_with(&stack, 2, |ip| {
            assert_eq!(ip, 1);
            filter_profiler_frame(MemoryFrame {
                address: ip as u64,
                symbols: [
                    "rustfs_mimalloc_profiling::profiler::record_sample",
                    "example::inline_allocate",
                    "example::caller",
                ]
                .into_iter()
                .map(|name| MemorySymbol {
                    name: name.into(),
                    system_name: None,
                    filename: Some("example.rs".into()),
                    line: 42,
                })
                .collect(),
            })
            .map(Arc::new)
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].address, 1);
        let names: Vec<_> = resolved[0]
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect();
        assert_eq!(names, ["example::inline_allocate", "example::caller"]);
        assert_eq!(
            resolved[0].symbols[0].filename.as_deref(),
            Some("example.rs")
        );
        assert_eq!(resolved[0].symbols[0].line, 42);
    }

    #[test]
    fn allocation_estimates_round_only_after_stack_aggregation() {
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        let samples = vec![
            RecordedAllocationSample {
                stack,
                weighted_objects: 1.4,
                weighted_bytes: 1075.2,
            };
            2
        ];
        let profile = build_allocation_samples(samples, 8);
        assert_eq!(profile.len(), 1);
        assert_eq!(profile[0].alloc_objects, 3);
        assert_eq!(profile[0].alloc_space, 2150);
    }

    #[test]
    fn tls_ring_wraps_and_partial_drains_preserve_fifo_without_compaction() {
        let mut ring = TlsSampleBuffer::new();
        let stack = StackKey {
            frames: [42; MAX_CAPTURE_DEPTH],
            depth: 1,
        };
        for value in 0..TLS_SAMPLE_RING_CAPACITY {
            let mut sample = test_sample(stack);
            sample.weighted_bytes = value as f64;
            assert!(ring.push(sample));
        }
        let mut drained = Vec::new();
        assert_eq!(ring.drain_into(&mut drained, 1), 1);
        assert_eq!(ring.head, 1);
        assert_eq!(ring.samples[1].unwrap().weighted_bytes, 1.0);
        let mut sample = test_sample(stack);
        sample.weighted_bytes = TLS_SAMPLE_RING_CAPACITY as f64;
        assert!(ring.push(sample));
        assert_eq!(
            ring.drain_into(&mut drained, usize::MAX),
            TLS_SAMPLE_RING_CAPACITY
        );
        assert_eq!(
            drained
                .iter()
                .map(|s| s.weighted_bytes as usize)
                .collect::<Vec<_>>(),
            (0..=TLS_SAMPLE_RING_CAPACITY).collect::<Vec<_>>()
        );
        ring.clear();
        assert!(ring.is_empty());
        assert!(ring.push(test_sample(stack)));
        assert_eq!(ring.drain_into(&mut Vec::new(), 0), 0);
        assert_eq!(ring.len(), 1);
    }
}

#[cfg(all(test, feature = "no_hooks"))]
mod no_hooks_tests {
    use super::*;

    #[test]
    fn hooks_fold_to_plain_forwarding_and_sessions_are_rejected() {
        let allocator = SamplingAllocator::with_allocator(std::alloc::System);
        let layout = Layout::from_size_align(64, 8).unwrap();
        unsafe {
            let ptr = allocator.alloc(layout);
            assert!(!ptr.is_null());
            let grown = allocator.realloc(ptr, layout, 128);
            assert!(!grown.is_null());
            allocator.dealloc(grown, Layout::from_size_align(128, 8).unwrap());
        }
        assert!(!HOT_ALLOCATION_STATE.allocator_seen.load(Ordering::Relaxed));
        assert!(!HOT_ALLOCATION_STATE.recorder_active.load(Ordering::Acquire));
        assert!(!live::enabled());
        assert!(matches!(
            Profiler::start(MimallocConfig::default()),
            Err(Error::InvalidConfig(_))
        ));
    }
}
