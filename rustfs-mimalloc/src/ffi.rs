use core::ffi::{CStr, c_void};

const OUTPUT_BUFFER_CAPACITY: usize = 64 * 1024;

pub(crate) unsafe fn owned_mimalloc_string(ptr: *mut rustfs_mimalloc_sys::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }

    let result = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe { rustfs_mimalloc_sys::mi_free(ptr as *mut c_void) };
    result
}

pub(crate) fn collect_mimalloc_output(
    write: impl FnOnce(Option<rustfs_mimalloc_sys::mi_output_fun>, *mut c_void),
) -> String {
    let mut output = OutputBuffer {
        bytes: Vec::with_capacity(OUTPUT_BUFFER_CAPACITY),
        truncated: false,
    };

    write(
        Some(collect_mimalloc_output_callback),
        &mut output as *mut OutputBuffer as *mut c_void,
    );

    // Reuse the callback buffer for the usual UTF-8 output instead of copying it.
    let mut result = String::from_utf8(output.bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    if output.truncated {
        result.push_str("\n[truncated: mimalloc profile output exceeded internal buffer]\n");
    }
    result
}

struct OutputBuffer {
    bytes: Vec<u8>,
    truncated: bool,
}

unsafe extern "C" fn collect_mimalloc_output_callback(
    msg: *const rustfs_mimalloc_sys::c_char,
    arg: *mut c_void,
) {
    if msg.is_null() || arg.is_null() {
        return;
    }

    let output = unsafe { &mut *(arg as *mut OutputBuffer) };
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
