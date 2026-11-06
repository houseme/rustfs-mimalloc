# 0.7.4 runtime review and improvements — 2026-11-07

This pass starts from `c905f59` and examines production allocation, heap and
arena contracts, profiling session ownership, sampled pointer lifecycles,
buffer admission, report snapshots, symbolization and pprof generation.

## Functional assessment

| Area | Assessment and action |
| --- | --- |
| Global allocator and raw allocation helpers | Alignment and allocation ownership are forwarded through the native entry points. Specialized small/sized frees require their documented caller contracts. Existing alignment, zeroing, resize and remote-free regressions cover the exercised paths. |
| Heap and arena ownership | Cached contexts borrow their parent and stay on their creating thread. Borrowed raw heap handles require caller-enforced lifetimes. Arena reservations have process lifetime. These constraints remain part of the advanced API contracts. |
| Session startup | Extreme queue/live capacities passed configuration validation and panicked in infallible reservations. Use fallible preallocation and expose `MetadataAllocationFailed`, with partial storage and recorder ownership released before retry. |
| Session stop | Stop cleared TLS rings and live maps but left global queue records and backing storage. Discard global queues after synchronizing registered TLS handoffs and free their storage outside the locks. |
| Live snapshots | Normal snapshots acquired each shard twice. Retain the first guard through the common copy path, reducing stable-snapshot acquisitions from 128 to 64. Growth and table rebuilding reserve outside locks using the shard admission bound. |
| Reallocation and cross-thread frees | Pending entries reserve rollback storage, and tracked frees remove metadata before provider reuse. Snapshot rebuilding preserves pending markers. Existing rollback, moved/in-place realloc, address reuse and session-generation tests exercise these contracts. |
| Reporting and pprof | Reports aggregate stacks, resolve each instruction once per report and encode interval/live values separately. Add complete report timing through `last_report_elapsed_micros()`, covering work before encoding and temporary cleanup. |
| Build and API maintenance | The native pin and reviewed export inventory remain current. Existing C/C++ header, ABI and option checks cover the native boundary. Profiling additions expose a diagnostic function and a variant on the existing non-exhaustive error enum. |

## Reproduced defects and implementation validation

The startup regression calls the public `Profiler::start` with `usize::MAX`
queue capacity and live capacity. The previous implementation panicked with
`capacity overflow`. Both cases now return the typed error, clear partial
storage and allow a subsequent ordinary session to start and report.

The stop regression inserts an accepted global record, stops the session and
checks diagnostics plus queue capacity. Previously `buffered_samples` stayed
at one. It now reaches zero and every global queue releases its backing storage.
Each worker retains its fixed TLS ring storage until thread exit.

The snapshot regression exercises occupied output, a capacity-sufficient fast
path, table rebuilding and an admission during the unlocked reservation gap.
It verifies that reservation occurs outside the shard lock, copied records fit
reserved storage, and both old and newly admitted pending realloc markers
survive the rebuild. Table deallocation runs after unlocking.

Local checks pass with stable Rust 1.99.1 on macOS arm64. They include workspace
tests, 79 profiling unit tests, both ignored release pressure tests, no_hooks,
isolated provider contracts in debug/release, all-target/all-feature Clippy,
rustdoc with warnings denied, normalized package validation and version checks.
API coverage remains 137 bound native functions and 46 options.

## Runtime diagnostics and tuning

Two one-second, two-worker live-profiling runs exercise the public example,
with a 1 MiB sample interval and periodic reports. These runs use different
buffer/report configurations on the development host. They establish the
diagnostic behavior and sample-quality tradeoff. Software speedup attribution
requires an isolated same-configuration ABBA comparison.

| Configuration | Recorded / reported | Allocation drops | Live drops / pending | Last full report / encoding phase |
| --- | --- | --- | --- | --- |
| 512 global slots, 100 ms reports | 6,459 / 6,331 | 52,392 | 0 / 0 | 158 / 80 microseconds |
| 8,192 global slots, 25 ms reports | 60,381 / 60,381 | 6 | 0 / 0 | 175 / 49 microseconds |

The dense workload exceeds the smaller queue's reporting budget. More slots
and more frequent reports improve retained sample quality, with additional
metadata and reporting work. The new full-report timer makes the earlier
drain/snapshot/symbolization costs visible alongside the encoding phase.
Choose these capacities and intervals using the application's allocation rate,
drop counts, retained live metadata, report latency and RSS budget.

Further performance work should first measure stack capture under active
sampling and report aggregation/symbolization at realistic stack cardinality.
Cached aligned `ThreadHeap` growth and native reclaim/purge tuning remain
workload-specific opportunities. The constant 64-shard fast-path lock reduction
is established by the implementation. End-to-end throughput impact awaits an
isolated comparison.

Evidence is saved in `target/review-runtime-*.log`. The `start-before` and
`stop-before` logs contain the failing regressions. The `live-example` and
`live-quality` logs preserve both diagnostic runs. The following sections
retain the earlier 0.7.4 audit history.

---

# 0.7.4 follow-up review — 2026-11-07

Project base `475aac0` is current after `git pull --ff-only`. A fresh upstream
fetch confirms dev3 at `ea136f74`, and the live release/PR metadata confirms
the v3.6.1 release and the previously reviewed merges. All 48 CI jobs for
`475aac0` passed, covering Windows static CRT, clang-cl, Linux/musl, macOS,
Rust 1.95 compatibility, native tests and profiling providers.
[CI evidence](https://github.com/houseme/rustfs-mimalloc/actions/runs/42242690498)

## Confirmed findings and fixes

| Finding | Trigger and impact | Fix and evidence |
| --- | --- | --- |
| Generated header tree drift, minor | `public_headers::copy_tree` copied source entries but retained obsolete output entries. A rename such as bits.h to arch.h left old exported files available to downstream compilers. File/directory transitions failed with an OS path-type error. | Reconcile each generated directory before copying. Remove obsolete entries, replace changed types and unlink generated symlinks. Preserve unchanged file timestamps. The new layout regression failed with `IsADirectory` before the fix and passes after it. |
| Newly exported APIs could escape review, minor | `api::inventory` classified any new C export as unbound and allowed `check-api` to succeed. A synthetic `mi_future_api` reproduced the omission. | Compare the unbound set with `xtask/unbound-exports.txt`. Reject new exports and stale exclusions, requiring an explicit binding decision. The new-export negative test failed before the fix and passes afterward. |

The header test also covers renamed and removed paths, directory/file
transitions and unchanged QSBR timestamps. A Unix symlink regression checks
that generated links can be replaced while external sentinel data survives.
The API test also requires removal of an exclusion after adding its binding.
Public API coverage is 137 bound functions and 46 options, with 96 explicit
unbound exports. Host ABI checks cover 128 signatures, nine callbacks and
three layouts.

## Performance and remaining work

Header reconciliation retains content-aware writes, keeping unchanged header
timestamps available to downstream compiler caches. Runtime optimization
priorities from the November 6 review continue to apply. Aligned growth through
cached `ThreadHeap` contexts is the first workload to measure, followed by
active stack capture/live tracking and report allocation/symbolization costs.
Reclaim and purge tuning requires throughput, tail-latency and RSS measurements
together. The existing ABBA tools and profiler quality counters provide the
acceptance criteria. Throughput impact has yet to be measured.

## Follow-up validation

Local checks pass on macOS arm64 with stable Rust 1.99.1. They include workspace
tests, five public-header tests with C/C++ compile/link coverage, 17 xtask tests,
API inventory, ABI projection and all seven ABI/option controls. All-target and
all-feature Clippy, rustdoc with warnings denied, formatting, sys archive
verification, normalized profiling archive/provider verification and the 0.7.4
release consistency check also pass. The before/after API JSON files are
byte-identical.

The failing-before regression logs are saved under
`target/review-0.7.4-{header,api}-before.log`. Current validation output uses
`target/review-0.7.4-followup-*.log` and
`target/upstream-api-2026-11-07-final.json`.

This follow-up consolidates its entries into the existing 0.7.4 changelog and
uses the synchronized 0.7.4 workspace version for source commit/push.

---

The following November 6 report records the original upstream synchronization
and its local validation.

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
