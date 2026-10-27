# 0.7.3 maintenance and latest-five-commit audit — 2026-10-28

Reviewed project head: `13fd20fed06131c0c8f5a99a59145abead17fc61`, clean and
current after git pull --ff-only. The five-commit range starts after `ac47f7a`.
The workspace uses edition 2024 with no declared Cargo rust-version. Local
validation uses stable Rust 1.99.1. Version 0.7.3 is retained throughout.

## Findings and corrections

### Hot-state padding did not cover 128-byte cache lines

Severity: Minor — a concrete layout/performance-contract gap; no demonstrated
memory-safety defect or measured throughput regression.

At reviewed head, profiler.rs:54 uses repr(align(64)) for HotAllocationState.
The comment promises isolation from sample-hit counter writes. A neighbouring
counter can occupy the other half of the same 128-byte cache line. The local
Apple Silicon host reports 128 bytes through hw.cachelinesize, so 64-byte
padding alone does not establish that isolation on this supported platform.

Change the private region to 128-byte alignment and padding. A regression
checks the type's alignment, placement of an adjacent counter, and the actual
static address. It fails before the fix and passes afterwards, including with
no_hooks. The three atomic fields, their memory orderings and the session
generation protocol are unchanged. This proves layout isolation for 64/128-byte
lines, not a universal cache-line size or a causal speedup.

### Threaded benchmark described a different workload than it executes

Severity: Minor — benchmark interpretation and comparison gap, not an allocator
runtime defect.

At reviewed head, alloc.rs:275-277 describes cross-thread free-list contention.
Each worker actually allocates and frees its own block before the next
allocation. The timed round also contains two barriers, so scheduling and
synchronization can dominate a small local-allocation batch. Only mimalloc was
represented in that threaded group.

Keep the worker lifecycle outside timing, describe the workload as same-thread
allocation/free scaling, and run both explicit System and mimalloc providers
through the same harness. Under override, omit System to avoid interception.
Add a separate barrier-only group, reported in rounds rather than fictitious
allocation operations. It is a diagnostic control, not a value to subtract.
Batch size and throughput accounting share one constant. The existing
cross_thread_free_batch continues to transfer allocation ownership through a
channel. Realloc rows measure alloc/realloc/free round trips; zeroed rows
measure the allocator API without a full payload read.

### Latest upstream QSBR declaration breaks standalone C inclusion

Severity: Major — verified build/validation failure in an upstream candidate;
the final pin excludes it.

Upstream `8374d1b22404e2a93f4d5f4f3d6da007534f73fc` adds mi_decl_externc to
mimalloc/qsbr.h:35 without defining the private macro in its public include
chain. Trying dev3 head c9c93615 makes the existing C ABI projection fail with
unknown type name mi_decl_externc. No new Rust declaration can repair that
public header contract.

Select its parent `810774fbc87ef545ccee2995552f47371b7e7b78`. This retains 29
new commits since the previous pin, while excluding the broken declaration
and its child padding-warning change. Keep the vendored tree pristine; neither
inject a private macro into the ABI test nor hide the diagnostic. The selected
pin passes all header/ABI controls. C++ QSBR linkage remains an upstream follow-up,
not a delivered fix.

## Per-commit result

| Commit | Reviewed behavior and result |
| --- | --- |
| `0253caf32bb273a4ce6e426688d24539862348f3` | syn 3 PointerMutability and FnPtr migration. The complete parser and its callers preserve const/mutable pointer projection, callback calling convention and rejection behavior. Host C signatures/layouts and all seven matching/negative controls pass. No additional confirmed defect. |
| `7530fe404c0ef06a509687c5d32795f1896d428a` | Hot-state consolidation, including live tracking's generation reads. Atomic orderings and generation/session callers remain consistent; the 64-byte-only padding gap is corrected above. |
| `71ecfac9c30e4e6ca75d7fce079a49cdd9b9ce6e` | Realloc/zeroed/threaded benchmarks. Pointer/layout use and ownership are retained; the threaded workload description and missing comparisons are corrected above. |
| `407ebc83f7d1d6b436660921e99d84a1daef6650` | no_hooks compile-time opt-out. Public start rejects sessions before claiming ownership; provider operations preserve failure/zeroing/alignment/realloc/cross-thread-free contracts. Add explicit feature CI and document dependency-graph-wide feature unification. No additional confirmed runtime defect. |
| `13fd20fed06131c0c8f5a99a59145abead17fc61` | Formatting changes inspected with their surrounding functions. No additional behavioral finding. Final formatting is checked with cargo fmt. |

The review covers changed Rust code, unsafe allocator forwarding, FFI/ABI,
atomics and session ownership, feature combinations and benchmark validity.
It is not a proof of correctness of every vendored allocator path or of all
possible C/OS callback schedules. No additional confirmed Rust memory-safety
or session-concurrency regression was established in the reviewed range.

## no_hooks performance evidence

An isolated allocator-neutral consumer instantiated four wrapped System methods
and four direct System equivalents with no_hooks, release optimization, one
codegen unit and LTO disabled. On the local target, LLVM IR merges wrapped
alloc, alloc_zeroed and dealloc into aliases of their direct counterparts.
Wrapped realloc retains the same System realloc/fallback paths; no observer,
atomic or TLS accesses remain. This is target/compiler-specific code-generation
evidence, not a throughput benchmark or a guarantee for unoptimized builds.

The existing stateful-provider contract also checks failure preservation,
in-place and moved realloc, alignment, zeroing and cross-thread free with
no_hooks. Default session tests still execute with hooks enabled; no_hooks
does not replace their coverage. Native no_profile and Rust no_hooks remain
separate controls. Reusable libraries should not enable the latter for their
consumers because Cargo unifies features.

## Upstream/API and further optimization

The selected native source remains a 3.6.1 development snapshot, version 30601,
70 commits after the latest formal v3.6.0 release. New merged PRs #1460 and
#1461, direct guard/page-map/process/fork changes, and the rejected head are
documented in [the upstream audit](UPSTREAM_REVIEW.md).

Four public-header diffs contain no new API at the selected revision. API
inventories are byte-identical: 133 Rust functions, 45 option IDs and 96
intentionally unbound exports. C projection covers 124 extern signatures,
nine callback types and three layouts; seven controls pass. New internal
metadata helpers and the init.c to process.c rename do not warrant Rust APIs.
The existing option ID 35 already exposes the restored reclaim policy; clean
process tests cover its default and explicit -1/0/1 settings.

Further performance work should isolate CPU resources and compare frozen
binaries with same-binary controls and A/B/B/A drift/loss gates. Priorities are
stats/reclaim freshness versus throughput/RSS, portable versus native stack
capture, report-time symbol/mapping work, and purge policy versus tail latency.
Secure metadata guards can add cost; they are not presented as throughput
optimizations. Shared CI diagnostics and benchmark smoke tests cannot establish
a causal speedup.

## Validation and delivery

The focused suite passes: default and secure/debug workspace tests, 77 profiler
unit cases, 45 allocator unit cases, 15 xtask cases, no_hooks contracts (18 unit
cases), 34 isolated native-option cases and 63 executable benchmark smoke rows.
The layout regression has a recorded failing-before/passing-after result.
Native debug tests pass 8/8; native secure allocation/arena/QSBR tests pass 3/3;
macOS zone-override fork/pprof tests pass 2/2 on the local host.

The remaining local gates pass: all-target checks, no_profile and combined
secure/debug/no_profile tests, Clippy/rustdoc with warnings denied, formatting,
Actionlint, both isolated providers in debug/release, two explicitly executed
release pressure tests, and normalized profiling-archive checks (including
all-features/no_hooks). Verified sys/wrapper/profiling archives contain
102/12/27 files. The version/changelog checks and generated 0.7.3 notes pass.

Final-commit CI results are recorded with delivery. Cross-platform runs must
use this update's source commit; earlier runs do not
validate the new native pin. All versions remain 0.7.3, pending changelog notes
are merged into one section, and source commit/push does not create a tag or
publish any package.
