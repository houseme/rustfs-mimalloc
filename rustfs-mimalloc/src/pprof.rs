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
//! `mi_profiler_stop` and detaches from the attached subprocess in that order.
//! Attaching is unsafe because the caller must quiesce all native callbacks
//! before the value drops, including callbacks from earlier attachments.
//!
//! # Subprocess attachments
//! Each native subprocess has one profiler slot. Attaching to an occupied
//! slot fails. This wrapper tracks its subprocess, detaches it before a new
//! attachment and stops sampling when detached. Distinct subprocesses can
//! have independent profilers.
//!
//! # Examples
//! ```no_run
//! use rustfs_mimalloc::pprof::{PprofConfig, PprofProfiler};
//!
//! # fn main() {
//! let mut profiler = PprofProfiler::new("myprofile", PprofConfig::default())
//!     .expect("failed to create the native profiler");
//! // SAFETY: This example finishes all sampled work before profiler teardown.
//! unsafe { profiler.attach() };
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
/// it from its subprocess's heaps, and deletes the native profiler.
pub struct PprofProfiler {
    ptr: NonNull<mi_profiler_t>,
    base_file_name: CString,
    attached_to: Option<rustfs_mimalloc_sys::mi_subproc_id_t>,
}

impl core::fmt::Debug for PprofProfiler {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PprofProfiler")
            .field("ptr", &self.ptr)
            .field("base_file_name", &self.base_file_name)
            .field("attached", &self.attached_to.is_some())
            .finish()
    }
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
    ///
    /// The native copy strips any dot-extension from the base name (also
    /// choosing the format) and truncates names beyond 1024 bytes;
    /// [`Self::base_file_name`] returns the caller's original string.
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
            attached_to: None,
        })
    }

    /// The base file name as passed to [`Self::new`]. Note that the native
    /// profiler strips any dot-extension (also choosing the output format)
    /// and truncates names beyond 1024 bytes, so snapshot file prefixes may
    /// differ from this string for long or extension-carrying names.
    #[inline]
    pub fn base_file_name(&self) -> &str {
        self.base_file_name
            .to_str()
            .expect("base file name was built from valid UTF-8")
    }

    /// Attach to all current and future heaps in the main subprocess.
    /// Returns `true` when the profiler is attached. Attaching stops a
    /// running profiler first; call [`Self::start`] to begin sampling.
    /// Any previous subprocess attachment is detached before this attempt.
    ///
    /// # Safety
    /// Before dropping this value, quiesce every native callback that could
    /// use it, including callbacks from earlier attachments. Stopping or
    /// detaching prevents future sampling but does not wait for callbacks
    /// already in progress. Exclude external changes to this registration
    /// until it is detached.
    ///
    /// ```compile_fail
    /// use rustfs_mimalloc::pprof::{PprofConfig, PprofProfiler};
    /// let mut profiler = PprofProfiler::new("profile", PprofConfig::default()).unwrap();
    /// profiler.attach(); // Requires a callback-lifetime guarantee.
    /// ```
    pub unsafe fn attach(&mut self) -> bool {
        // SAFETY: The main subprocess lives for the process lifetime. The
        // caller upholds the callback lifetime and registration requirements.
        unsafe { self.attach_subproc(rustfs_mimalloc_sys::mi_subproc_main()) }
    }

    /// Attach to all current and future heaps of a custom subprocess
    /// created through [`rustfs_mimalloc_sys::mi_subproc_new`], over
    /// [`rustfs_mimalloc_sys::mi_subproc_profile`]. See [`Self::attach`]
    /// for attach semantics. Each subprocess has an independent slot.
    ///
    /// # Safety
    /// [`Self::attach`]'s callback-quiescence and registration requirements
    /// apply. `subproc` must remain live until this attachment is detached,
    /// including detachment during Drop or a subsequent attach attempt.
    pub unsafe fn attach_subproc(&mut self, subproc: rustfs_mimalloc_sys::mi_subproc_id_t) -> bool {
        self.detach();
        // SAFETY: `self.ptr` is a live profiler handle and `subproc` is the
        // caller-validated handle; the native attach stops the profiler.
        let attached =
            unsafe { rustfs_mimalloc_sys::mi_subproc_profile(subproc, self.ptr.as_ptr()) };
        if attached {
            self.attached_to = Some(subproc);
        }
        attached
    }

    /// Stop sampling and detach from this profiler's subprocess and its heaps.
    /// Callbacks already in progress must still finish before Drop.
    pub fn detach(&mut self) {
        self.stop();
        if let Some(subproc) = self.attached_to.take() {
            // SAFETY: A successful unsafe attachment guarantees the subprocess
            // is live and its registration belongs to this handle until detach.
            unsafe { rustfs_mimalloc_sys::mi_subproc_profile(subproc, core::ptr::null_mut()) };
        }
    }

    /// Start sampling. Returns whether the profiler was already running.
    pub fn start(&mut self) -> bool {
        // SAFETY: `self.ptr` is a live profiler handle.
        unsafe { rustfs_mimalloc_sys::mi_profiler_start(self.ptr.as_ptr()) }
    }

    /// Stop sampling. Returns whether the profiler was running. Sampling can
    /// resume with [`Self::start`] alone; re-attaching is only needed after
    /// [`Self::detach`].
    pub fn stop(&mut self) -> bool {
        // SAFETY: `self.ptr` is a live profiler handle.
        unsafe { rustfs_mimalloc_sys::mi_profiler_stop(self.ptr.as_ptr()) }
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
        self.detach();
        // SAFETY: The profiler is stopped and detached, and the owner upholds
        // the callback-quiescence requirement documented on this type.
        unsafe { rustfs_mimalloc_sys::mi_pprof_profiler_delete(self.ptr.as_ptr()) };
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

// In-process state transitions (attach/start/stop/drop) deliberately live in
// `tests/pprof_native.rs`: this lib test binary runs other unit tests whose
// allocations would race the profiler drop quiescence contract.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_interior_nul_and_reports_base_name() {
        assert!(PprofProfiler::new("bad\0name", PprofConfig::default()).is_none());
    }

    #[test]
    fn creation_only_paths_stay_out_of_process_wide_state() {
        // Creation and accessors never touch process-wide profiler state, so
        // they are safe next to sibling unit tests.
        let profiler = PprofProfiler::new("rustfs-mimalloc-pprof-create", PprofConfig::default())
            .expect("create native profiler");
        assert_eq!(profiler.base_file_name(), "rustfs-mimalloc-pprof-create");
        // Dropping a never-attached, never-started profiler exercises the
        // stop+detach+delete ordering with no registered callbacks to race.
        drop(profiler);
    }
}
