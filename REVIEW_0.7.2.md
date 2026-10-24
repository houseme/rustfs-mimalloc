# Global main review and final 0.7.2 refactor — 2026-10-25

Base: refreshed origin/main at
[467e663](https://github.com/houseme/rustfs-mimalloc/commit/467e663e7f726713fa5ca261b72bf056e325d898).
The workspace was clean before review. The native source remains the official
mimalloc v3.6.0 pin b18ab4ee. This review does not change C ABI, allocation
sampling policy, GlobalAlloc provider forwarding or the three-crate version.

The scope includes the allocator/wrapper boundary, Heap/ThreadHeap/ArenaId
lifetime contracts, profiler reporting and lifecycle, provider isolation,
maintenance tooling and the release/tag boundary. The concrete changes below
address report-time critical sections and payload hashing. There is no claim
that one implementation is optimal for every workload.

## Findings and implemented remedies

### Report output growth ran inside sample-source critical sections

Severity: Minor — performance/contention mechanism, not a demonstrated data race.

Baseline: [global drain at profiler.rs:1278](https://github.com/houseme/rustfs-mimalloc/blob/467e663e7f726713fa5ca261b72bf056e325d898/rustfs-mimalloc-profiling/src/profiler.rs#L1278)
and [TLS drain at profiler.rs:1100](https://github.com/houseme/rustfs-mimalloc/blob/467e663e7f726713fa5ca261b72bf056e325d898/rustfs-mimalloc-profiling/src/profiler.rs#L1100).

In the base revision, drain_recorded_samples extended a growing output Vec
while holding a global sample shard lock. TLS report draining could likewise
grow output while holding a TLS ring lock. Allocation hooks use non-blocking
handoff, so allocator work in these critical sections lengthens their contention
window. The second global pass also constructed a separate temporary sample Vec
and then appended it into the report output.

The refactor has one owned sample-output Vec for the whole report drain:

- Global queues and TLS rings use the same capacity/locking helper. It returns
  a source guard only when the bounded batch fits output's spare capacity.
- Insufficient capacity releases the source guard before reserving storage,
  then reacquires and rechecks occupancy. Stable sparse sources reserve for
  observed occupancy; arrivals that outgrow that reservation cause a bounded
  retry. No retained record is discarded to fit a stale estimate.
- Both global passes append directly to that Vec. Slot release counts only the
  global records appended in that call, excluding an existing TLS/global prefix.
- A zero TLS budget returns before cloning the registry snapshot or taking its
  mutex. Registry snapshots and live tracking retain their separate lock rules.

Report drain limits, source rotation, per-source FIFO order, generation checks,
weighted aggregation and recorded/flushed/dropped accounting are preserved.
The design deliberately retains std Mutex and the existing bounded ownership
model; it introduces no lock-free reclamation protocol or public API.

Deterministic contracts check that the reservation callback can acquire the
source lock, simulate admissions during the unlocked window, and verify copying
does not change Vec capacity. Other cases cover empty/sparse output, a prefix
from another source, pending global slot accounting and zero-budget progress
while the registry lock is held. Existing FIFO, fairness, restart, realloc,
cross-thread and pressure tests remain relevant to the final implementation.

### Archive and executable hashing buffered whole payloads

Severity: Minor — bounded-memory efficiency and oversize failure handling.

Baseline: [archive download at release.rs:265](https://github.com/houseme/rustfs-mimalloc/blob/467e663e7f726713fa5ca261b72bf056e325d898/xtask/src/release.rs#L265).

The base verifier read the whole local archive with fs::read and the whole
download into a Vec before hashing. Frozen-profiler provenance also read both
executables into memory. Hashing therefore needed payload-sized temporary
storage despite only requiring a digest.

One shared streaming SHA-256 module now hashes files, downloaded archives and
frozen executables through a fixed 32 KiB payload buffer. The hash state and
64-character result are bounded; HTTP/TLS transport buffers are additional.
The small, separately bounded registry-index response retains its existing
metadata parsing path.

Official sparse-index/download/local-archive digest agreement is still required.
Downloaded archives retain the 64 MiB bound; a typed oversize failure terminates
verification instead of retrying the same oversized body 40 times. Network
failures retain bounded retries. File hashing has no download-size limit, so
large frozen executables can still be audited with bounded scratch storage.

Tests cover known SHA-256 vectors, exact/over-limit boundaries, final partial
chunks, interrupted reads and the maximum requested read-buffer size. A
generated multi-megabyte reader proves the input need not be materialized in a
payload-sized Vec. The verifier is also checked against an official published
0.7.1 archive and the retained identical local sys archive.

## Performance interpretation and remaining priorities

The verified improvements are structural: allocator work moves outside sample
locks, a temporary sample Vec/copy disappears, zero-budget registry work is
skipped, and hash scratch storage is independent of payload size. No isolated
before/after throughput benchmark was performed. Additional lock acquisitions
on output growth are a deliberate tradeoff and should be evaluated with
representative concurrent reports before making latency or throughput claims.

The next measurements should reserve CPU resources, use frozen binaries and
same-binary controls, and reject conclusions when A/B/B/A drift or sample-loss
gates fail. Priorities remain report contention/stack capture, opt-in arena
purging versus tail latency/RSS, and real Linux large-page behavior. Shared CI
diagnostics and historical figures do not establish causal performance gains.

## Verification and tag delivery

Local validation passes: 75 profiler unit cases, 15 tool unit cases, all-target
checks, default/secure-debug/no-profile feature combinations, Clippy/rustdoc with
warnings denied, API/ABI controls, both provider graphs, normalized archives and
two explicit release-mode pressure contracts. Streaming verification against
the official 0.7.1 sys archive and its retained local archive also passes.

Before creating the local v0.7.2 tag, run the repository's full local gates,
including feature combinations, API/ABI checks, both provider graphs, normalized
archives and explicit release pressure tests. Run final-commit cross-platform CI
and release dry-run as well; the latter must skip upload and GitHub Release.
The final source commit and exact run results are reported with delivery.

The tag is local until explicitly pushed. Pushing v0.7.2 triggers real package
publication, so creation and publication remain distinct operations.
