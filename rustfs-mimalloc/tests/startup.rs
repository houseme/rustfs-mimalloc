//! Isolated process: verifies startup before libtest creates any worker threads.
use rustfs_mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    #[cfg(windows)]
    unsafe {
        unsafe extern "C" {
            static __ImageBase: [u8; 2];
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
        }
        let image_base = core::ptr::addr_of!(__ImageBase).cast::<core::ffi::c_void>();
        assert_eq!(image_base, GetModuleHandleW(core::ptr::null()).cast_const());
        assert_eq!(
            __ImageBase, *b"MZ",
            "image base must refer to the PE header"
        );
    }

    let bytes = vec![0xA5u8; 64 * 1024];
    assert!(bytes.iter().all(|byte| *byte == 0xA5));
    drop(bytes);
    assert!(!MiMalloc::stats_json().is_empty());
    println!("mimalloc startup reached main and allocation/statistics checks passed");
}
