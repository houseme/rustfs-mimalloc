//! Safe process-wide callback registration.
//!
//! mimalloc's raw `mi_register_error`, `mi_register_output` and
//! `mi_register_deferred_free` functions take a bare function pointer plus a
//! caller-managed `arg`, requiring manual proof that both outlive registration
//! and every in-flight call — including calls from other threads that may
//! briefly continue after re-registration. The wrappers here hold a
//! `&'static` callback behind a lock, route every C invocation through a
//! trampoline that keeps the guard alive for the call's duration, and contain
//! panics so nothing unwinds into C.
//!
//! Callbacks are one-per-kind process-wide. Registering again replaces the
//! previous callback; clearing restores the native default. A callback that
//! replaced another one is guaranteed not to race with calls still running in
//! the previous callback: readers hold the registration lock for the whole
//! call.
//!
//! # Re-entrancy
//! A registered callback must never call [`register_error`],
//! [`register_output`], [`register_deferred_free`] or their `clear_*`
//! counterparts — directly or transitively. Trampolines hold the registration
//! lock for the whole call while registration takes the write side, so a
//! re-entrant call from the callback's own thread would deadlock. The
//! registration entry points detect this situation and panic with a clear
//! message instead of hanging.

use core::cell::Cell;
use core::ffi::{c_int, c_void};
use core::panic::AssertUnwindSafe;
use std::sync::{RwLock, RwLockReadGuard};

type ErrorCallback = dyn Fn(c_int) + Send + Sync;
type OutputCallback = dyn Fn(&str) + Send + Sync;
type DeferredFreeCallback = dyn Fn(bool, u64) + Send + Sync;

static ERROR_CALLBACK: RwLock<Option<&'static ErrorCallback>> = RwLock::new(None);
static OUTPUT_CALLBACK: RwLock<Option<&'static OutputCallback>> = RwLock::new(None);
static DEFERRED_FREE_CALLBACK: RwLock<Option<&'static DeferredFreeCallback>> = RwLock::new(None);

thread_local! {
    /// Set while a registration trampoline runs a callback on this thread.
    /// Registration entry points refuse to run re-entrantly instead of
    /// deadlocking on the write side of the lock the caller already holds.
    static IN_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

fn ensure_not_in_callback(operation: &str) {
    let reentrant = IN_CALLBACK.with(|flag| flag.get());
    assert!(
        !reentrant,
        "rustfs-mimalloc: {operation} called from inside an allocator callback \
         would deadlock on the registration lock; defer the call instead"
    );
}

fn invoke<T: ?Sized>(guard: &RwLockReadGuard<'_, Option<&'static T>>, run: impl FnOnce(&T)) {
    if let Some(callback) = guard.as_ref() {
        struct CallbackScope {
            previous: bool,
        }

        impl Drop for CallbackScope {
            fn drop(&mut self) {
                IN_CALLBACK.with(|flag| flag.set(self.previous));
            }
        }

        // Mark this thread as inside a callback so registration entry points
        // can refuse re-entrant calls; nesting restores the previous state.
        let previous = IN_CALLBACK.with(|flag| flag.replace(true));
        let _scope = CallbackScope { previous };
        // A panic inside a C callback must not unwind across FFI: contain it
        // and abort loudly instead.
        if std::panic::catch_unwind(AssertUnwindSafe(|| run(callback))).is_err() {
            std::eprintln!("rustfs-mimalloc: callback panicked; aborting");
            std::process::abort();
        }
    }
}

/// Install a process-wide allocator error callback, replacing the default
/// stderr reporting. Pass a `&'static` closure to keep its captured state
/// alive for the process lifetime; [`clear_error`] restores the default.
///
/// The callback receives the mimalloc error code (for example `ENOMEM` or
/// `EINVAL`). Allocation failures still return null to the caller and set
/// `errno` independently of the callback.
///
/// # Safety contract
/// Register only while no other thread can report allocator errors, and keep
/// concurrent registrations exclusive. The callback may be invoked
/// concurrently from multiple threads, must be minimal (it can run inside
/// allocation failure paths), and must not allocate through mimalloc.
pub fn register_error(callback: &'static (dyn Fn(c_int) + Send + Sync)) {
    ensure_not_in_callback("register_error");
    *ERROR_CALLBACK
        .write()
        .unwrap_or_else(|err| err.into_inner()) = Some(callback);
    // SAFETY: The trampoline reads the static registration, whose reference is
    // valid for the process lifetime, and never unwinds into C.
    unsafe { rustfs_mimalloc_sys::mi_register_error(Some(trampoline_error), core::ptr::null_mut()) }
}

/// Restore the default allocator error reporting and drop the registered
/// callback.
///
/// # Safety contract
/// The same quiescence requirements as [`register_error`] apply.
pub fn clear_error() {
    ensure_not_in_callback("clear_error");
    // Restore the C handler first: once the native call returns, no new
    // trampoline invocation can occur, so clearing the Rust slot afterwards
    // never drops a message into a handlerless window.
    //
    // SAFETY: Restoring the default handler takes no callback state.
    unsafe { rustfs_mimalloc_sys::mi_register_error(None, core::ptr::null_mut()) };
    *ERROR_CALLBACK
        .write()
        .unwrap_or_else(|err| err.into_inner()) = None;
}

unsafe extern "C" fn trampoline_error(err: c_int, _arg: *mut c_void) {
    let registration = ERROR_CALLBACK.read().unwrap_or_else(|err| err.into_inner());
    invoke(&registration, |callback| callback(err));
}

/// Install a process-wide allocator output callback, replacing the default
/// stderr sink for verbose messages, warnings and statistics output.
/// Registration can immediately flush buffered output through the new
/// callback, so install it before enabling verbose output. [`clear_output`]
/// restores stderr.
///
/// Messages arrive as UTF-8 slices without a terminating newline; non-UTF-8
/// output is skipped rather than reallocated.
///
/// # Safety contract
/// Register with no concurrent output or registration from other threads. The
/// callback must not allocate through mimalloc or it can recurse into its own
/// output.
pub fn register_output(callback: &'static (dyn Fn(&str) + Send + Sync)) {
    ensure_not_in_callback("register_output");
    *OUTPUT_CALLBACK
        .write()
        .unwrap_or_else(|err| err.into_inner()) = Some(callback);
    // SAFETY: The trampoline reads the static registration, whose reference is
    // valid for the process lifetime, and never unwinds into C.
    unsafe {
        rustfs_mimalloc_sys::mi_register_output(Some(trampoline_output), core::ptr::null_mut())
    }
}

/// Restore the default stderr output sink and drop the registered callback.
///
/// # Safety contract
/// The same quiescence requirements as [`register_output`] apply.
pub fn clear_output() {
    ensure_not_in_callback("clear_output");
    // SAFETY: Restoring the default sink takes no callback state.
    unsafe { rustfs_mimalloc_sys::mi_register_output(None, core::ptr::null_mut()) };
    *OUTPUT_CALLBACK
        .write()
        .unwrap_or_else(|err| err.into_inner()) = None;
}

unsafe extern "C" fn trampoline_output(msg: *const core::ffi::c_char, _arg: *mut c_void) {
    let registration = OUTPUT_CALLBACK
        .read()
        .unwrap_or_else(|err| err.into_inner());
    if registration.is_some() && !msg.is_null() {
        // SAFETY: mimalloc passes a NUL-terminated message buffer that is
        // valid for the call's duration.
        let bytes = unsafe { core::ffi::CStr::from_ptr(msg) }.to_bytes();
        if let Ok(message) = core::str::from_utf8(bytes) {
            invoke(&registration, |callback| callback(message));
        }
    }
}

/// Register a deferred-free callback, replacing any previous one.
/// [`clear_deferred_free`] unregisters it.
///
/// mimalloc invokes the callback during deferred maintenance with the
/// `force` flag and a monotonically increasing heartbeat. Work should be
/// bounded: the call happens while allocation maintenance is pending.
///
/// # Safety contract
/// Exclude concurrent allocator activity while registering or clearing. The
/// callback must support concurrent invocations and must not allocate through
/// mimalloc.
pub fn register_deferred_free(callback: &'static (dyn Fn(bool, u64) + Send + Sync)) {
    ensure_not_in_callback("register_deferred_free");
    *DEFERRED_FREE_CALLBACK
        .write()
        .unwrap_or_else(|err| err.into_inner()) = Some(callback);
    // SAFETY: The trampoline reads the static registration, whose reference is
    // valid for the process lifetime, and never unwinds into C.
    unsafe {
        rustfs_mimalloc_sys::mi_register_deferred_free(
            Some(trampoline_deferred_free),
            core::ptr::null_mut(),
        )
    }
}

/// Unregister the deferred-free callback.
///
/// # Safety contract
/// The same quiescence requirements as [`register_deferred_free`] apply.
pub fn clear_deferred_free() {
    ensure_not_in_callback("clear_deferred_free");
    // SAFETY: Unregistering takes no callback state.
    unsafe { rustfs_mimalloc_sys::mi_register_deferred_free(None, core::ptr::null_mut()) };
    *DEFERRED_FREE_CALLBACK
        .write()
        .unwrap_or_else(|err| err.into_inner()) = None;
}

unsafe extern "C" fn trampoline_deferred_free(force: bool, heartbeat: u64, _arg: *mut c_void) {
    let registration = DEFERRED_FREE_CALLBACK
        .read()
        .unwrap_or_else(|err| err.into_inner());
    invoke(&registration, |callback| callback(force, heartbeat));
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MiMalloc;
    use std::sync::Mutex;

    /// Callback registrations are process-wide, so these tests must not run
    /// concurrently with each other or with other output-emitting tests.
    fn test_lock() -> &'static Mutex<()> {
        static TEST_LOCK: Mutex<()> = Mutex::new(());
        &TEST_LOCK
    }

    fn captured() -> &'static Mutex<Vec<String>> {
        static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());
        &CAPTURED
    }

    #[test]
    fn output_callback_receives_statistics_output() {
        let _guard = test_lock().lock().unwrap_or_else(|err| err.into_inner());
        captured()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        register_output(&|message| {
            captured()
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(message.to_owned());
        });
        // Statistics output goes through the registered sink instead of stderr.
        unsafe {
            rustfs_mimalloc_sys::mi_stats_print_out(None, core::ptr::null_mut());
        }
        assert!(
            !captured()
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_empty(),
            "statistics output must reach the registered callback"
        );
        clear_output();
        captured()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        unsafe {
            rustfs_mimalloc_sys::mi_stats_print_out(None, core::ptr::null_mut());
        }
        assert!(
            captured()
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_empty(),
            "clearing must restore the default sink"
        );
    }

    #[test]
    fn error_callback_receives_overflow_error() {
        let _guard = test_lock().lock().unwrap_or_else(|err| err.into_inner());
        static ERRORS: Mutex<Vec<c_int>> = Mutex::new(Vec::new());
        ERRORS.lock().unwrap_or_else(|err| err.into_inner()).clear();
        register_error(&|err| {
            ERRORS
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(err);
        });

        // A checked-multiplication overflow is reported through the error
        // callback in every build mode, before touching memory.
        let result = unsafe { rustfs_mimalloc_sys::mi_calloc(usize::MAX, 2) };
        assert!(result.is_null(), "an overflowing request must fail");
        let errors = ERRORS.lock().unwrap_or_else(|err| err.into_inner());
        assert!(
            !errors.is_empty(),
            "the overflow error must reach the registered error callback"
        );
        drop(errors);
        clear_error();
        ERRORS.lock().unwrap_or_else(|err| err.into_inner()).clear();
        let result = unsafe { rustfs_mimalloc_sys::mi_calloc(usize::MAX, 2) };
        assert!(result.is_null());
        assert!(
            ERRORS
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_empty(),
            "clearing must restore the default handler"
        );
    }

    #[test]
    fn deferred_free_registration_round_trip_is_safe() {
        let _guard = test_lock().lock().unwrap_or_else(|err| err.into_inner());
        static CALLS: Mutex<u32> = Mutex::new(0);
        register_deferred_free(&|_force, _heartbeat| {
            *CALLS.lock().unwrap_or_else(|err| err.into_inner()) += 1;
        });
        // Maintenance may or may not invoke the callback here; the required
        // guarantee is that registration, collection and clearing stay safe.
        MiMalloc::collect(true);
        clear_deferred_free();
        MiMalloc::collect(true);
    }

    #[test]
    fn replacement_tolerates_concurrent_invocations() {
        let _guard = test_lock().lock().unwrap_or_else(|err| err.into_inner());
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::thread;

        static OUTPUTS: AtomicUsize = AtomicUsize::new(0);
        register_output(&|_message| {
            OUTPUTS.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_micros(50));
        });
        let first = thread::spawn(|| {
            for _ in 0..20 {
                unsafe {
                    rustfs_mimalloc_sys::mi_stats_print_out(None, core::ptr::null_mut());
                }
            }
        });
        // Re-register from another thread while the first callback is being
        // invoked: readers keep the previous callback for the call's duration.
        let second = thread::spawn(|| {
            register_output(&|_message| {
                OUTPUTS.fetch_add(1, Ordering::Relaxed);
            });
        });
        first.join().unwrap();
        second.join().unwrap();
        assert!(OUTPUTS.load(Ordering::Relaxed) >= 20);
        clear_output();
    }

    /// Child-process helper: performs the forbidden re-entrant registration
    /// from inside a callback. Panics inside callbacks abort the process, so
    /// the parent asserts a non-zero exit instead of a deadlock hang.
    #[test]
    fn reentrant_registration_child() {
        if std::env::var_os("RUSTFS_CALLBACK_REENTRANCY_CHILD").is_none() {
            return;
        }
        register_output(&|_message| {
            // Would deadlock on the registration write lock without the
            // re-entrancy guard.
            clear_output();
        });
        unsafe {
            rustfs_mimalloc_sys::mi_stats_print_out(None, core::ptr::null_mut());
        }
    }

    #[test]
    fn reentrant_registration_fails_fast_instead_of_deadlocking() {
        let _guard = test_lock().lock().unwrap_or_else(|err| err.into_inner());
        let executable = std::env::current_exe().expect("locate the test binary");
        let status = std::process::Command::new(executable)
            .args([
                "callback::tests::reentrant_registration_child",
                "--exact",
                "--nocapture",
            ])
            .env("RUSTFS_CALLBACK_REENTRANCY_CHILD", "1")
            .status()
            .expect("spawn the reentrancy child");
        assert!(
            !status.success(),
            "re-entrant registration must abort loudly, not hang; got {status:?}"
        );
        // The parent process registrations stay untouched by the child.
    }
}
