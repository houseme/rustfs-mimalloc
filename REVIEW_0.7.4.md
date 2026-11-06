# 0.7.4 functional and performance review — 2026-11-06

The source preparation updates mimalloc and its Rust API coverage, keeping the
requested package version at **0.7.4**. It does not publish crates or create a
release tag. The clean project base after `git pull --ff-only` was `ddde603`.
That base was actually 0.7.3 and had neither a 0.7.4 nor an Unreleased changelog
section; this update creates one consolidated 0.7.4 section and preserves 0.7.3
as release history.

## Upstream selection and public API

The immutable native pin advances from v3.6.1 `56dc5eb8` to
[`ea136f74`](https://github.com/microsoft/mimalloc/commit/ea136f74031c0d954df7b609a6ede97c23cc81c2),
161 commits later, C version **30602**. This is a v3.6.2 development snapshot;
the latest official upstream release checked on this date remains v3.6.1.
Read all four public header diffs and the live merged-PR metadata, including
the destination branches. See [UPSTREAM_REVIEW.md](UPSTREAM_REVIEW.md) for the
PR-by-PR audit and direct native fixes.

| Surface | Result |
| --- | --- |
| `mi_theap_realloc_aligned` / `mi_theap_rezalloc_aligned` | Newly exported by merged PR #1477; add raw FFI and unsafe `ThreadHeap` methods with the same argument order. |
| `mi_option_mte` | New active ID 54, exposed through the existing option APIs. Native build support and hardware/OS support are separate from the runtime option value. |
| Existing option IDs, callback signatures and layouts | Retained; 137 bound Rust functions, 46 option IDs, 128 extern signatures, nine callbacks and three layouts pass the host controls. |
| `mi_heap_theap` | PR #1483 documents non-null empty contexts on initialization failure/teardown. Fix the wrapper's misleading null/OOM explanation; retain its existing Option signature. |
| Deprecated options | PRs #1480/#1481 clarify that these have no effect in v3; do not expose inactive controls as new functionality. |
| Experimental native MTE/PAC | Included in source, default-off. Document numeric target-CFLAGS controls; do not add hardware-requiring Cargo features to the portable all-feature build. |

## Functional review

One source review pass covered the allocator forwarding layer, heap/context
lifetimes, arena/query contracts, C build/header transformations, native API
changes, and the Rust profiler's provider forwarding, session publication,
reallocation metadata, bounded buffering and report path. This is a targeted
maintenance review, not a proof of all native allocator interleavings.

| Area | Evidence and decision |
| --- | --- |
| Global allocation | `rustfs-mimalloc/src/lib.rs` forwards alloc/zeroed/realloc with layout alignment and uses general `mi_free`. Preserve this path; size-specialized frees require their exact contracts. |
| Cached heaps | `heap.rs` borrows the parent and prevents Send/Sync on `ThreadHeap`. New methods directly call native thread-heap APIs; retain initialized-context and caller-thread requirements. |
| Reallocation correctness | Test null input, 1/8/64/4096/16384-byte alignment, data retention, growth, shrink, zeroed growth and oversized failure preserving the old block. A real offset-allocation regression covers upstream #1479's misaligned shortcut. |
| Arena and remote frees | Existing exclusive-arena queries, cross-thread ownership/content/reuse and context-deletion contracts remain enabled. Address membership does not establish allocation ownership. |
| Rust profiling | `SamplingAllocator<A>` retains exact provider/layout forwarding. Live metadata is removed before free and reserved before realloc; failure/in-place/moved realloc contracts and session-generation tests remain enabled. Native profiling is separate. |
| Sample/report cost | Allocation observation uses TLS/sharded bounded storage, with explicit loss counters. Reports aggregate stacks and cache frame resolution per report. Preserve suppression, session boundaries and loss accounting. |
| Build and headers | Pristine submodule; retain guarded QSBR export correction and static-CRT source generation. Source-anchor/entropy controls and C/C++ standalone header/link tests pass. Remove obsolete libutil linking following upstream's direct FreeBSD sysctl implementation. |

The corrected `Heap::thread_local` documentation is a verified minor issue:
the old wording implied that context initialization failure would produce
`None`, whereas upstream returns an empty context and subsequent allocation
fails. No additional confirmed Rust correctness defect remains from this pass.
Missing target execution is recorded below rather than described as a defect.

## Further optimization priorities

These are code-supported opportunities, not measured speedup estimates. Retain
the default behavior until isolated workload measurements justify a change.

| Priority | Opportunity and source evidence | Acceptance measurement |
| --- | --- | --- |
| 1 | Reuse a `ThreadHeap` per heap/worker for aligned buffers. New aligned realloc methods avoid the repeated `_mi_heap_theap` lookup used by native heap APIs, especially when alternating heaps. | Add aligned growth/zeroed-growth cases to the existing heap lookup/switching comparison; record same-size and moving realloc separately. |
| 1 | Profile active stack capture and live tracking separately. `profiler.rs` records sampled stacks; `live.rs` uses bounded sharded maps and can lose insertions under contention. | Compare sampling off, allocation samples, and live tracking with fixed rates/depths and periodic reports. Gate throughput on sample loss, pending samples, live drops and report counts. |
| 2 | Reduce report allocation/symbolization work only after measuring it. `build_memory_samples_with_resolver` already aggregates stacks and resolves each address once per report; `live::snapshot` allocates in proportion to retained entries. | Measure report duration, allocations, peak temporary memory and drain backlog. Any reusable cache must handle session changes and dynamic library unloads. |
| 2 | Tune native stats merging, reclaim and purge for the actual service. Existing controls expose the 4 MiB merge threshold, -1/0/1 reclaim policy, purge delays and optional purge-on-free. | Measure throughput together with p95/p99 latency, RSS, faults and statistics freshness. Configure heap-creation policies before heaps exist. |
| 3 | Quantify updated secure free-list/reciprocal/reuse-delay costs before changing feature defaults. MTE/PAC add separate hardware/security choices. | Separate default/secure builds; use eligible hardware and process entitlements for MTE/PAC. Verify allocation contracts and RSS alongside throughput. |

Use frozen executables, recorded compiler/native flags and reserved resources
with A1/B1/B2/A2 runs. The existing tools reject conclusions above 5% baseline
or candidate drift; profiler quality failures also reject throughput claims.
No performance benchmark was executed for this update, so the new API and
inherited upstream optimizations are not represented as measured gains.

## Validation

Host: macOS arm64, stable Rust 1.99.1. Tests use `RUST_TEST_THREADS=1`, since
profiler registration is process-global; concurrency tests create their own
workers. Runtime dependency requirements remain unchanged apart from internal
crate versions. The lockfile changes only the four workspace package versions.

Completed local gates:

- Default workspace tests, including 48 allocator unit tests, 77 profiler unit
  tests, native API/context regressions, compiler probes and documentation tests.
- Workspace tests with `secure,debug`, `no_profile`,
  `secure,debug,no_profile`, and `no_hooks,rustfs-allocator`.
- Sys/allocator tests with `secure,debug,override` on macOS, including generated
  C/C++ headers and the offset-pointer regression with native checks enabled.
- Final ordinary-process option/API tests: 42 isolated startup-option cases,
  including MTE numeric values and case-insensitive sync/async/preferred input.
- `cargo xtask check-api`, `check-abi`, `test-abi`: 137 functions, 46 options,
  128 extern signatures, nine callbacks, three layouts and all seven controls.
- `cargo fmt --all --check`, all-target/all-feature Clippy with warnings denied,
  workspace documentation with rustdoc warnings denied, and benchmark compilation.

- Isolated RustFS and upstream allocator providers pass in debug and release;
  their dependency graphs each contain exactly the selected native provider.
  The optional RustFS compatibility API and optimized profile lifecycle pass.
- Both ignored release pressure tests pass: live-heap thread matrix and sampled
  allocation/drop-pressure matrix. These validate correctness/loss accounting,
  not a performance improvement estimate.
- Eight focused native C tests pass: API, debug-fill API, arena, bitmap, fork,
  QSBR, native profile and pprof. Three additional tests pass with CMake
  `MI_SECURE=FULL` (native level 5): API, arena and QSBR. Cargo's `secure` remains
  level 4 and is covered separately above.
- Normalized profiling archive verification passes, including default-core
  graph, all features, release profiles and both isolated providers. Sys and
  allocator Cargo archives package and verify using explicit local dependency
  patches, since 0.7.4 has not been published.
- `cargo xtask release-check 0.7.4` passes version/edition/internal-requirement/
  changelog checks; release notes generate with previous tag v0.7.3.

Local evidence is in `target/review-0.7.4-*.log` and
`target/upstream-api-audit.json`; these ignored files are not committed.
The initial provider run encountered registry DNS restrictions; a separate
offline retry uses the reviewed lockfile and cached registry dependencies.

Windows startup/static CRT, Linux/musl, BSD, wasm32, 32-bit and Rust 1.95
compatibility were not executed on this host. Existing CI retains its platform
matrix; no passing target result is inferred from host compilation or upstream
CI. MTE/PAC hardware execution and production performance remain unverified.
