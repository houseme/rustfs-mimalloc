//! Benchmarks: mimalloc vs system allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

#[global_allocator]
static GLOBAL: rustfs_mimalloc::MiMalloc = rustfs_mimalloc::MiMalloc;

fn bench_alloc_dealloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_dealloc");
    for size in [8, 64, 256, 1024, 4096, 65536] {
        // Use System explicitly; the global allocator above is mimalloc.
        // With override enabled the C allocator is intercepted as well.
        if !cfg!(feature = "override") {
            group.bench_with_input(BenchmarkId::new("system", size), &size, |b, &size| {
                let layout = Layout::from_size_align(size, 8).unwrap();
                b.iter(|| unsafe {
                    let ptr = System.alloc(layout);
                    assert!(!ptr.is_null());
                    std::hint::black_box(ptr);
                    System.dealloc(ptr, layout);
                });
            });
        }
        group.bench_with_input(BenchmarkId::new("mimalloc", size), &size, |b, &size| {
            let layout = Layout::from_size_align(size, 8).unwrap();
            b.iter(|| unsafe {
                let ptr = std::alloc::alloc(layout);
                assert!(!ptr.is_null());
                std::hint::black_box(ptr);
                std::alloc::dealloc(ptr, layout);
            });
        });
    }
    group.finish();
}

fn bench_aligned_alloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("aligned_alloc");
    for align in [8, 64, 256, 1024, 4096] {
        group.bench_with_input(BenchmarkId::new("mimalloc", align), &align, |b, &align| {
            b.iter(|| {
                let layout = std::alloc::Layout::from_size_align(128, align).unwrap();
                unsafe {
                    let ptr = std::alloc::alloc(layout);
                    std::hint::black_box(ptr);
                    std::alloc::dealloc(ptr, layout);
                }
            });
        });
    }
    group.finish();
}

fn bench_small_free(c: &mut Criterion) {
    let mut group = c.benchmark_group("small_free_64");
    for local in [false, true] {
        group.bench_function(if local { "local" } else { "generic" }, |b| {
            b.iter(|| unsafe {
                let ptr = rustfs_mimalloc::MiMalloc::malloc_csize(64);
                assert!(!ptr.is_null());
                std::hint::black_box(ptr);
                // Immediate free on the allocating thread, no collection or heap deletion.
                if local {
                    rustfs_mimalloc::MiMalloc::free_small_local(ptr);
                } else {
                    rustfs_mimalloc_sys::mi_free(ptr.cast());
                }
            });
        });
    }
    group.finish();
}

// End-to-end producer/consumer batches: includes channel synchronization, but
// excludes worker startup and reuses the pointer buffer between iterations.
fn bench_cross_thread<A: GlobalAlloc + Copy + Send + 'static>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    allocator: A,
    size: usize,
) {
    const BATCH: usize = 256;
    group.throughput(Throughput::Elements(BATCH as u64));
    group.bench_function(BenchmarkId::new(name, size), |b| {
        let layout = Layout::from_size_align(size, 8).unwrap();
        let (send_batch, receive_batch) = std::sync::mpsc::sync_channel::<Vec<usize>>(0);
        let (send_empty, receive_empty) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            for mut pointers in receive_batch {
                for address in pointers.drain(..) {
                    // The producer transfers ownership through the channel and
                    // initialized the first byte before sending the allocation.
                    unsafe {
                        let ptr = address as *mut u8;
                        std::hint::black_box(ptr.read());
                        allocator.dealloc(ptr, layout);
                    }
                }
                send_empty.send(pointers).unwrap();
            }
        });
        let mut batch = Some(Vec::with_capacity(BATCH));
        b.iter(|| {
            let mut pointers = batch.take().unwrap();
            for _ in 0..BATCH {
                unsafe {
                    let ptr = allocator.alloc(layout);
                    assert!(!ptr.is_null());
                    ptr.write(0x5A);
                    pointers.push(ptr as usize);
                }
            }
            send_batch.send(pointers).unwrap();
            batch = Some(receive_empty.recv().unwrap());
        });
        drop(send_batch);
        worker.join().unwrap();
    });
}

fn bench_cross_thread_free(c: &mut Criterion) {
    let mut group = c.benchmark_group("cross_thread_free_batch");
    for size in [64, 4096] {
        if !cfg!(feature = "override") {
            bench_cross_thread(&mut group, "system", System, size);
        }
        bench_cross_thread(&mut group, "mimalloc", rustfs_mimalloc::MiMalloc, size);
    }
    group.finish();
}

fn bench_heap_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("heap_lookup");
    for size in [64, 4096] {
        let heap = rustfs_mimalloc::heap::Heap::new().unwrap();
        group.bench_function(BenchmarkId::new("heap_aligned", size), |b| {
            b.iter(|| unsafe {
                let ptr = heap.malloc_aligned(size, 8);
                assert!(!ptr.is_null());
                std::hint::black_box(ptr);
                rustfs_mimalloc_sys::mi_free(ptr.cast());
            });
        });
        let local = heap.thread_local().unwrap();
        group.bench_function(BenchmarkId::new("cached_aligned", size), |b| {
            b.iter(|| unsafe {
                let ptr = local.malloc_aligned(size, 8);
                assert!(!ptr.is_null());
                std::hint::black_box(ptr);
                rustfs_mimalloc_sys::mi_free(ptr.cast());
            });
        });
    }
    group.finish();
}

fn bench_heap_switching(c: &mut Criterion) {
    let mut group = c.benchmark_group("heap_switching");
    for size in [64, 4096] {
        let first = rustfs_mimalloc::heap::Heap::new().unwrap();
        let second = rustfs_mimalloc::heap::Heap::new().unwrap();
        group.bench_function(BenchmarkId::new("lookup", size), |b| {
            b.iter(|| unsafe {
                for heap in [&first, &second] {
                    let ptr = heap.malloc_aligned(size, 8);
                    assert!(!ptr.is_null());
                    std::hint::black_box(ptr);
                    rustfs_mimalloc_sys::mi_free(ptr.cast());
                }
            });
        });
        let first_cached = first.thread_local().unwrap();
        let second_cached = second.thread_local().unwrap();
        group.bench_function(BenchmarkId::new("cached", size), |b| {
            b.iter(|| unsafe {
                for local in [&first_cached, &second_cached] {
                    let ptr = local.malloc_aligned(size, 8);
                    assert!(!ptr.is_null());
                    std::hint::black_box(ptr);
                    rustfs_mimalloc_sys::mi_free(ptr.cast());
                }
            });
        });
    }
    group.finish();
}

fn bench_vec(c: &mut Criterion) {
    let mut group = c.benchmark_group("vec");
    group.bench_function("push_1000", |b| {
        b.iter(|| {
            let mut v = Vec::new();
            for i in 0i32..1000 {
                v.push(i);
            }
            std::hint::black_box(&v);
        });
    });
    group.bench_function("extend_10000", |b| {
        b.iter(|| {
            let mut v = Vec::with_capacity(10000);
            v.extend(0i32..10000);
            std::hint::black_box(&v);
        });
    });
    group.finish();
}

fn bench_alloc_zeroed(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_zeroed");
    for size in [64, 4096, 65536] {
        if !cfg!(feature = "override") {
            group.bench_with_input(BenchmarkId::new("system", size), &size, |b, &size| {
                let layout = Layout::from_size_align(size, 8).unwrap();
                b.iter(|| unsafe {
                    let ptr = System.alloc_zeroed(layout);
                    assert!(!ptr.is_null());
                    std::hint::black_box(ptr);
                    System.dealloc(ptr, layout);
                });
            });
        }
        group.bench_with_input(BenchmarkId::new("mimalloc", size), &size, |b, &size| {
            let layout = Layout::from_size_align(size, 8).unwrap();
            b.iter(|| unsafe {
                let ptr = std::alloc::alloc_zeroed(layout);
                assert!(!ptr.is_null());
                std::hint::black_box(ptr);
                std::alloc::dealloc(ptr, layout);
            });
        });
    }
    group.finish();
}

fn bench_realloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("realloc");
    for size in [64, 1024, 65536] {
        for new_size in [size, size * 2] {
            let variant = if new_size == size { "same" } else { "grow" };
            if !cfg!(feature = "override") {
                group.bench_function(BenchmarkId::new(format!("system_{variant}"), size), |b| {
                    let layout = Layout::from_size_align(size, 8).unwrap();
                    let new_layout = Layout::from_size_align(new_size, 8).unwrap();
                    b.iter(|| unsafe {
                        let ptr = System.alloc(layout);
                        assert!(!ptr.is_null());
                        let grown = System.realloc(ptr, layout, new_size);
                        assert!(!grown.is_null());
                        std::hint::black_box(grown);
                        System.dealloc(grown, new_layout);
                    });
                });
            }
            group.bench_function(BenchmarkId::new(format!("mimalloc_{variant}"), size), |b| {
                let layout = Layout::from_size_align(size, 8).unwrap();
                let new_layout = Layout::from_size_align(new_size, 8).unwrap();
                b.iter(|| unsafe {
                    let ptr = std::alloc::alloc(layout);
                    assert!(!ptr.is_null());
                    let grown = std::alloc::realloc(ptr, layout, new_size);
                    assert!(!grown.is_null());
                    std::hint::black_box(grown);
                    std::alloc::dealloc(grown, new_layout);
                });
            });
        }
    }
    group.finish();
}

// Contended same-size alloc/free hammering: worker threads start once and
// rendezvous with the harness per iteration, so spawn cost stays out of the
// measurement and thread counts expose cross-thread free-list contention.
fn bench_threaded_alloc_free(c: &mut Criterion) {
    const OPS_PER_WORKER: usize = 512;
    static DONE: AtomicBool = AtomicBool::new(false);
    let mut group = c.benchmark_group("threaded_alloc_free");
    for threads in [1, 2, 4, 8] {
        group.throughput(Throughput::Elements((threads * OPS_PER_WORKER) as u64));
        group.bench_function(BenchmarkId::new("mimalloc", threads), |b| {
            DONE.store(false, Ordering::Relaxed);
            let layout = Layout::from_size_align(128, 8).unwrap();
            // Workers plus the harness participate in both barriers.
            let start = Arc::new(Barrier::new(threads + 1));
            let stop = Arc::new(Barrier::new(threads + 1));
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    let start = Arc::clone(&start);
                    let stop = Arc::clone(&stop);
                    std::thread::spawn(move || {
                        loop {
                            start.wait();
                            if DONE.load(Ordering::Relaxed) {
                                break;
                            }
                            for _ in 0..OPS_PER_WORKER {
                                unsafe {
                                    let ptr = std::alloc::alloc(layout);
                                    assert!(!ptr.is_null());
                                    ptr.write(0x5A);
                                    std::hint::black_box(ptr);
                                    std::alloc::dealloc(ptr, layout);
                                }
                            }
                            stop.wait();
                        }
                    })
                })
                .collect();
            b.iter(|| {
                start.wait();
                stop.wait();
            });
            DONE.store(true, Ordering::Relaxed);
            // Release workers waiting for the next round so they can exit.
            start.wait();
            for worker in workers {
                worker.join().unwrap();
            }
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_alloc_dealloc,
    bench_aligned_alloc,
    bench_small_free,
    bench_cross_thread_free,
    bench_threaded_alloc_free,
    bench_heap_lookup,
    bench_heap_switching,
    bench_vec,
    bench_alloc_zeroed,
    bench_realloc
);
criterion_main!(benches);
