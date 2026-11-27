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
}
