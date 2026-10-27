# 0.7.3 review index

The current source selection and latest-five-commit audit are in
[the 2026-10-28 follow-up](REVIEW_0.7.3_2026-10-28.md).
The earlier review below retains its original pin and validation evidence.

---

# Global 0.7.3 review — 2026-10-26

Base: refreshed origin/main at
[26ae27f](https://github.com/houseme/rustfs-mimalloc/commit/26ae27f745fab3ca11fd7394f1f7a32f40a1db83).
The worktree was clean and `git pull --ff-only` reported up to date. Review
covered native source selection, public C ABI, target build choices, Rust heap
ownership, GlobalAlloc forwarding, profiler lifecycle/reporting, provider
isolation, and synchronized package preparation. The crates use edition 2024;
no Cargo rust-version is declared. Local verification uses stable Rust 1.99.1.

## Findings and remedies

### Native fork cleanup could traverse another thread's inconsistent state

Severity: Major — upstream-confirmed crash mechanism in the previous native
source, not a newly discovered Rust ownership defect.

In the old `src/heap/subproc.c` fork-child handler, dead-thread heaps/pages were
traversed immediately. If another thread was allocating at fork, this could
dereference an inconsistent list even when the child only intended to exec or
_exit. Upstream [#1456](https://github.com/microsoft/mimalloc/issues/1456)
reports that failure; the complete old/new handler and its allocation-path
callers were reviewed.

Pin dev3 `3828cfee`, including `12db0977`: the child marks a fork epoch and
pending work; normal allocation/thread initialization later attempts cleanup.
The native fork suite now exercises both coordinated child allocation and
uncoordinated forks whose children only _exit. Continued child allocation still
requires quiescent allocator state at fork. This review does not claim a local
reproduction of the old intermittent crash or unrestricted Rust fork safety.

### Statistics merging default increased allocator maintenance frequency

Severity: Minor — workload-dependent upstream performance regression.

The previous native `src/util/options.c` default was 512 KiB. Upstream
[#1458](https://github.com/microsoft/mimalloc/issues/1458) attributes a small
allocation-batch regression to frequent thread-heap stats merging. Commit
`f0032d4c` raises it to 4 MiB; this update inherits that default and documents
the tradeoff against parent-heap statistics freshness.

The existing option ID 52 and byte units are preserved. An independent child
process checks both native getters at the clean 4 MiB default; startup tests
cover 0, 512 KiB and 4 MiB. Environment removal is case-insensitive, and options
are not mutated after worker threads start. No new runtime wrapper or Cargo
feature is needed.

### Compiler-unwinder fallback lacked a dedicated downstream CI contract

Severity: Informational — validation gap, not a proven Cargo runtime defect.

The snapshot fixes LLVM libunwind compatibility with `_Unwind_Backtrace`.
The standard Linux CMake build selects execinfo, while our Cargo musl build
does not enable the native fallback. Add a Linux CMake configuration that
forces execinfo off, verifies the unwind probe succeeded, then runs native
profile/pprof tests. This prevents silent no-backtrace success in that CI
configuration. It does not claim automatic native musl backtraces.

## Rust and API conclusions

The four public headers contain no new public API in this range. Name/option
inventories differ only in the source version. Existing Rust declarations
remain aligned: 133 functions, 45 option IDs; host C ABI checks cover 124 extern
signatures, nine callback types and three exposed layouts. Seven mutation and
matching controls pass. No additional confirmed defect was established in
the reviewed Rust runtime paths.

GlobalAlloc retains direct aligned native calls, and general frees retain
cross-thread support. ThreadHeap remains tied to its parent's borrow and
creating thread, with explicit teardown restrictions. ArenaId remains a
copyable process-lifetime reservation handle; area/membership do not grant
memory access. QSBR remains raw experimental FFI without a host adapter.

The profiler retains its session generation/ownership checks, weighted
aggregation, single report output vector, growth outside sample locks and
allocator-neutral default dependency graph. Existing concurrency, rollback,
fairness and pressure contracts provide the relevant regression coverage.
Maintenance/release tooling stays in unpublished Rust xtask; no Python code
or interpreter setup is introduced.

## Further optimization priorities

1. Measure the 512 KiB/4 MiB/disabled stats policies with representative block
   sizes, concurrent remote frees and required statistics freshness. Freeze
   binaries, isolate CPU resources, use same-binary controls and A/B/B/A drift
   gates. Upstream percentages are not measurements of this release.
2. Profile stack capture and report-time symbol/mapping work before changing
   caches. Cross-report caching must account for dynamic image unload/reload;
   the present per-report cache avoids stale symbols.
3. Evaluate arena purge-on-free and large-range immediate purge against both
   RSS and tail latency on the deployment kernel. Keep them opt-in until the
   workload evidence supports different defaults.
4. Treat native musl unwinding and WebAssembly/CHERI Rust support as separate
   target-validation tasks. Do not infer support from inherited C changes.

No isolated before/after throughput benchmark is claimed. The delivered
performance change is the upstream stats policy correction, with an explicit
freshness tradeoff, rather than a claim of a universal optimal allocator.

## Verification and delivery

Local checks pass: all-target workspace check; default, secure/debug,
no_profile and combined feature tests; 45 allocator and 76 profiler unit cases;
15 xtask cases; Clippy and rustdoc with warnings denied; formatting and diff
checks. The native options harness passes 31 independent-process cases. API/ABI
checks and all seven mutation/matching controls pass.

Both isolated providers pass shared debug and release contracts, with distinct
native dependency graphs. The RustFS provider's two explicitly ignored release
pressure cases pass with two executed tests. The normalized profiling archive
passes default/all-feature/release and isolated-provider validation; verified
archives contain 102 sys, 12 wrapper and 27 profiling files. Actionlint passes
for the changed CI workflow. Native debug C tests pass 8/8, including fork,
QSBR, profiling and pprof; purge-on-free allocation/arena tests pass 2/2 on the
local macOS host.

Linux compiler-unwinder and Windows/static-CRT runtime coverage belong to
cross-platform CI. CI and release dry-run results must be associated with the
final pushed source commit; earlier 0.7.2 runs are not evidence for this pin.
This preparation keeps all released crates at 0.7.3, consolidates the root
changelog and pushes source commits only. It creates no tag and publishes no
package.
