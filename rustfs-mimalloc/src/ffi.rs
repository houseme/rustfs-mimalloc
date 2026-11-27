use core::{
    ffi::{CStr, c_char, c_void},
    fmt,
    ptr::NonNull,
};

const OUTPUT_BUFFER_CAPACITY: usize = 64 * 1024;

/// An owned, immutable C string allocated and freed by mimalloc.
pub struct MiString(NonNull<c_char>);

// SAFETY: The buffer is immutable while borrowed. Mimalloc supports freeing
// live allocations on threads other than the allocating thread.
unsafe impl Send for MiString {}
// SAFETY: Shared access only exposes immutable string bytes.
unsafe impl Sync for MiString {}

impl MiString {
    /// Take ownership of a native string. Returns `None` for null.
    ///
    /// # Safety
    /// A non-null pointer must be the start of a uniquely owned mimalloc
    /// allocation containing a valid, initialized NUL-terminated string.
    /// Keep the allocation live and its bytes immutable until this value
    /// frees it. The caller transfers responsibility for freeing the pointer.
    pub unsafe fn from_raw(ptr: *mut c_char) -> Option<Self> {
        NonNull::new(ptr).map(Self)
    }

    /// Borrow the native bytes without allocating.
    pub fn as_c_str(&self) -> &CStr {
        unsafe { CStr::from_ptr(self.0.as_ptr()) }
    }

    /// Borrow the UTF-8 string. Invalid UTF-8 yields an empty string.
    /// Use [Self::as_c_str] to distinguish invalid UTF-8 from empty input.
    pub fn as_str(&self) -> &str {
        self.as_c_str().to_str().unwrap_or_default()
    }
}

impl Drop for MiString {
    fn drop(&mut self) {
        unsafe { rustfs_mimalloc_sys::mi_free(self.0.as_ptr().cast()) };
    }
}

impl core::ops::Deref for MiString {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for MiString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for MiString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_c_str(), f)
    }
}

pub(crate) fn collect_mimalloc_output(
    write: impl FnOnce(Option<rustfs_mimalloc_sys::mi_output_fun>, *mut c_void),
) -> String {
    let mut bytes = Vec::with_capacity(OUTPUT_BUFFER_CAPACITY);
    let complete = write_mimalloc_output(&mut bytes, write);

    // Reuse the callback buffer for the usual UTF-8 output instead of copying it.
    let mut result = String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    if !complete {
        result.push_str("\n[truncated: mimalloc profile output exceeded internal buffer]\n");
    }
    result
}

pub(crate) fn write_mimalloc_output(
    bytes: &mut Vec<u8>,
    write: impl FnOnce(Option<rustfs_mimalloc_sys::mi_output_fun>, *mut c_void),
) -> bool {
    bytes.clear();
    let mut output = OutputBuffer {
        bytes,
        truncated: false,
    };

    write(
        Some(collect_mimalloc_output_callback),
        core::ptr::from_mut(&mut output).cast(),
    );
    !output.truncated
}

pub(crate) fn write_mimalloc_json(
    buffer: &mut [u8],
    write: impl FnOnce(usize, *mut c_char) -> *mut c_char,
) -> Option<&CStr> {
    // Mimalloc allocates its own output if the caller's buffer is empty.
    if buffer.is_empty() || write(buffer.len(), buffer.as_mut_ptr().cast()).is_null() {
        return None;
    }
    CStr::from_bytes_until_nul(buffer).ok()
}

struct OutputBuffer<'a> {
    bytes: &'a mut Vec<u8>,
    truncated: bool,
}

unsafe extern "C" fn collect_mimalloc_output_callback(
    msg: *const rustfs_mimalloc_sys::c_char,
    arg: *mut c_void,
) {
    if msg.is_null() || arg.is_null() {
        return;
    }

    let output = unsafe { &mut *arg.cast::<OutputBuffer<'_>>() };
    if output.truncated {
        return;
    }

    let msg = unsafe { CStr::from_ptr(msg) }.to_bytes();
    let available = output.bytes.capacity().saturating_sub(output.bytes.len());
    if msg.len() <= available {
        output.bytes.extend_from_slice(msg);
    } else {
        output.bytes.extend_from_slice(&msg[..available]);
        output.truncated = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mi_string_handles_null_and_invalid_utf8() {
        assert!(unsafe { MiString::from_raw(core::ptr::null_mut()) }.is_none());
        let ptr = unsafe { rustfs_mimalloc_sys::mi_malloc(2) }.cast::<u8>();
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(0xff);
            ptr.add(1).write(0);
        }
        let owned = unsafe { MiString::from_raw(ptr.cast()) }.unwrap();
        assert_eq!(owned.as_c_str().to_bytes(), &[0xff]);
        assert!(owned.as_c_str().to_str().is_err());
        assert_eq!(owned.as_str(), "");
    }
}
