use rustfs_mimalloc::{MiMalloc, heap::Heap};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
};

struct CountingSystem;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL: CountingSystem = CountingSystem;

unsafe impl GlobalAlloc for CountingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

fn rust_allocations<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    let before = ALLOCATIONS.load(Relaxed);
    let result = operation();
    (result, ALLOCATIONS.load(Relaxed) - before)
}

fn main() {
    let heap = Heap::new().unwrap();
    let (process, count) = rust_allocations(MiMalloc::stats_json_owned);
    assert_eq!(count, 0);
    assert!(process.unwrap().contains("\"mimalloc_version\""));
    let (process, count) = rust_allocations(MiMalloc::stats_json);
    assert!(count > 0);
    assert!(process.contains("\"mimalloc_version\""));

    let (snapshot, count) = rust_allocations(|| heap.stats_json_owned());
    assert_eq!(count, 0);
    assert!(snapshot.unwrap().contains("\"mimalloc_version\""));
    let (snapshot, count) = rust_allocations(|| heap.stats_json());
    assert!(count > 0);
    assert!(snapshot.contains("\"mimalloc_version\""));

    let mut json_buffer = vec![0; 64 * 1024];
    let address = json_buffer.as_ptr().cast();
    let buffer = json_buffer.as_mut_slice();
    let (process, count) = rust_allocations(move || MiMalloc::stats_json_into(buffer));
    assert_eq!(count, 0);
    let process = process.unwrap();
    assert_eq!(process.as_ptr(), address);
    assert!(process.to_str().unwrap().contains("\"mimalloc_version\""));
    let buffer = json_buffer.as_mut_slice();
    let heap_ref = &heap;
    let (snapshot, count) = rust_allocations(move || heap_ref.stats_json_into(buffer));
    assert_eq!(count, 0);
    let snapshot = snapshot.unwrap();
    assert_eq!(snapshot.as_ptr(), address);
    assert!(snapshot.to_str().unwrap().contains("\"mimalloc_version\""));

    assert!(MiMalloc::stats_json_into(&mut []).is_none());
    assert!(heap.stats_json_into(&mut [0; 1]).is_none());

    let mut text_buffer = Vec::with_capacity(64 * 1024);
    let address = text_buffer.as_ptr();
    text_buffer.extend_from_slice(b"previous contents");
    let (complete, count) = rust_allocations(|| MiMalloc::stats_print_into(&mut text_buffer));
    assert!(complete);
    assert_eq!(count, 0);
    assert_eq!(text_buffer.as_ptr(), address);
    assert!(!text_buffer.is_empty());
    assert!(!text_buffer.starts_with(b"previous contents"));

    let (complete, count) = rust_allocations(|| heap.stats_print_into(&mut text_buffer));
    assert!(complete);
    assert_eq!(count, 0);
    assert_eq!(text_buffer.as_ptr(), address);

    let mut tiny = Vec::with_capacity(1);
    let (complete, count) = rust_allocations(|| heap.stats_print_into(&mut tiny));
    assert!(!complete);
    assert_eq!(count, 0);
    assert_eq!(tiny.len(), tiny.capacity());
}
