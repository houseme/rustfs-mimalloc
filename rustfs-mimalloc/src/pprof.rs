//! RAII wrapper for mimalloc's native pprof profiler.
//!
//! The native profiler samples allocations with C-side stack capture and
//! writes pprof-compatible protobuf (or text) snapshots, at a fraction of the
//! cost of Rust-side unwinding. Snapshots appear as `<base>.NNNN.pb` files
//! next to the requested base name and can be analyzed with pprof tools
//! against the running executable.
//!
//! # Ordering
//! Upstream requires a profiler to be stopped and detached, with its
//! callbacks quiesced, before deletion. [`PprofProfiler`]'s [`Drop`] runs
//! `mi_profiler_stop` and detaches from all attached heaps in that order, but
//! the caller must still guarantee no thread is inside a callback (that is,
//! no sampled allocation is being freed concurrently) when the value drops.
//!
//! # Examples
//! ```no_run
//! use rustfs_mimalloc::pprof::{PprofConfig, PprofProfiler};
//!
//! # fn main() {
//! let mut profiler = PprofProfiler::new("myprofile", PprofConfig::default())
//!     .expect("failed to create the native profiler");
//! profiler.attach();
//! profiler.start();
//! // ... the workload ...
//! profiler.snapshot(); // writes `myprofile.0001.pb`
//! profiler.stop();
//! profiler.detach();
//! # }
//! ```

use core::ptr::NonNull;
use std::ffi::CString;

use rustfs_mimalloc_sys::mi_profiler_t;

/// Native profiler creation parameters. Interval values of zero disable the
/// corresponding periodic snapshot, matching the native semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PprofConfig {
    /// Poisson sampling rate in bytes; roughly one sample per this many
    /// allocated bytes (upstream example: `512 * 1024`).
    pub initial_threshold: usize,
    /// Bytes allocated between automatic snapshot writes; zero disables them.
    pub alloc_interval_size: usize,
    /// Bytes of live (in-use) memory growth between automatic snapshot
    /// writes; zero disables them.
    pub inuse_interval_size: usize,
    /// Seconds between automatic snapshot writes; zero disables them.
    pub time_interval_secs: usize,
}

impl Default for PprofConfig {
    fn default() -> Self {
        Self {
            initial_threshold: 512 * 1024,
            alloc_interval_size: 0,
            inuse_interval_size: 0,
            time_interval_secs: 0,
        }
    }
}

/// Owned handle for a native pprof profiler. Creating it allocates the
/// profiler from a private native heap; dropping it stops sampling, detaches
/// it from all heaps, and deletes the native profiler.
#[derive(Debug)]
pub struct PprofProfiler {
    ptr: NonNull<mi_profiler_t>,
    base_file_name: CString,
    attached: bool,
    started: bool,
}

// SAFETY: Every native entry point used here is callable from any thread and
// the wrapper serializes them behind `&mut self`. Moving the handle to
// another thread is safe while the owner keeps the documented quiescence
// contract for `Drop`.
unsafe impl Send for PprofProfiler {}

impl PprofProfiler {
    /// Create a native pprof profiler that writes snapshots prefixed with
    /// `base_file_name`. A `.heap` or `.text` extension selects the text
    /// format; other extensions (or none) select protobuf. Returns `None`
    /// when the name contains an interior NUL or the native profiler cannot
    /// be created.
    pub fn new(base_file_name: &str, config: PprofConfig) -> Option<Self> {
        let base_file_name = CString::new(base_file_name).ok()?;
        // SAFETY: `base_file_name` is a valid NUL-terminated string kept alive
        // in `self`; the native call copies it into the profiler's own heap.
        let ptr = unsafe {
            rustfs_mimalloc_sys::mi_pprof_profiler_new(
                config.initial_threshold,
                base_file_name.as_ptr(),
                config.alloc_interval_size,
                config.inuse_interval_size,
                config.time_interval_secs,
            )
        };
        NonNull::new(ptr).map(|ptr| Self {
            ptr,
            base_file_name,
            attached: false,
            started: false,
        })
    }

    /// The base file name snapshots are written to (extension stripped by the
    /// native profiler if present).
    #[inline]
    pub fn base_file_name(&self) -> &str {
        self.base_file_name
            .to_str()
            .expect("base file name was built from valid UTF-8")
    }

    /// Attach to all current and future heaps in the main subprocess.
    /// Returns `true` when the profiler is attached. Attaching stops the
    /// profiler; call [`Self::start`] to begin sampling.
    pub fn attach(&mut self) -> bool {
        // SAFETY: `self.ptr` is a live profiler handle; attaching routes
        // sampled allocations to the native callbacks until detach.
        let attached = unsafe { rustfs_mimalloc_sys::mi_profile(self.ptr.as_ptr()) };
        self.attached = attached;
        if !attached {
            self.started = false;
        }
        attached
    }

    /// Detach from all heaps. Sampled blocks freed afterwards no longer
    /// invoke the profiler's free callback.
    pub fn detach(&mut self) {
        if self.attached {
            // SAFETY: A null profiler detaches the main subprocess heaps.
            unsafe { rustfs_mimalloc_sys::mi_profile(core::ptr::null_mut()) };
            self.attached = false;
        }
        self.started = false;
    }

    /// Start sampling. Returns whether the profiler was already running.
    pub fn start(&mut self) -> bool {
        // SAFETY: `self.ptr` is a live profiler handle.
        let was_running = unsafe { rustfs_mimalloc_sys::mi_profiler_start(self.ptr.as_ptr()) };
        self.started = true;
        was_running
    }

    /// Stop sampling. Returns whether the profiler was running. The profiler
    /// must be re-attached before sampling can resume.
    pub fn stop(&mut self) -> bool {
        // SAFETY: `self.ptr` is a live profiler handle.
        let was_running = unsafe { rustfs_mimalloc_sys::mi_profiler_stop(self.ptr.as_ptr()) };
        self.started = false;
        was_running
    }

    /// Write a snapshot now (numbered file next to the base name). Safe to
    /// call while sampling.
    pub fn snapshot(&mut self) {
        // SAFETY: `self.ptr` is a live profiler handle; the native snapshot
        // is best effort and never affects the profiled program.
        unsafe { rustfs_mimalloc_sys::mi_profiler_snapshot(self.ptr.as_ptr()) };
    }
}

impl Drop for PprofProfiler {
    fn drop(&mut self) {
        // Upstream ordering: stop sampling, detach so no further `on_free`
        // callbacks are issued, then delete. The caller guarantees no thread
        // is inside a callback at this point.
        if self.started {
            // SAFETY: `self.ptr` is a live profiler handle.
            unsafe { rustfs_mimalloc_sys::mi_profiler_stop(self.ptr.as_ptr()) };
        }
        self.detach();
        // SAFETY: The profiler is stopped and detached, and the owner upholds
        // the callback-quiescence requirement documented on this type.
        unsafe { rustfs_mimalloc_sys::mi_pprof_profiler_delete(self.ptr.as_ptr()) };
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_interior_nul_and_reports_base_name() {
        assert!(PprofProfiler::new("bad\0name", PprofConfig::default()).is_none());
    }

    #[test]
    fn attach_start_stop_round_trip_reports_states() {
        let mut profiler =
            PprofProfiler::new("rustfs-mimalloc-pprof-states", PprofConfig::default())
                .expect("create native profiler");
        assert!(!profiler.start());
        assert!(profiler.stop());
        assert!(!profiler.stop());
        assert!(profiler.attach());
        assert!(!profiler.start());
        profiler.detach();
        // Attaching stopped the profiler again; a restart requires re-attach.
        assert!(profiler.attach());
        assert!(!profiler.start());
        profiler.detach();
    }
}
