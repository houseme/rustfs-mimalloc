# 0.7.3 release readiness — 2026-11-02

## Final main-based review and tag preparation

The follow-up review starts from refreshed origin/main at
66ef33dce245c0603c56f2a4d7da9772a5cc193c. The tree was clean and no v0.7.3
tag existed locally or remotely. The allocator/profiler runtime, native pin,
public APIs, heap ownership and recording/lock contracts remain unchanged.
No additional confirmed runtime defect was established in this review.

The concrete changes concern generated headers and ABI preparation. Previously
prepare copied every header and then overwrote QSBR again. A deterministic
regression sets output timestamps to a known old value: it fails on the base
revision and passes after the refactor. Unchanged output now preserves its
bytes/timestamps, avoiding needless downstream dependency invalidation; changed
inputs still refresh. First-build reads/copies remain necessary. No numeric
build-speed or runtime-throughput gain is claimed.

Validate the source correction before creating or mutating output. Canonicalize
existing source/destination ancestors and reject equal, ancestor/descendant or
existing symlink-alias overlap. Missing destination suffixes may not contain
unresolved parent traversal. This prevents recursive self-copy and accidental
source mutation. Tests cover source sentinels, untouched output on source drift,
unchanged timestamps, refresh behavior and Unix aliases. This is a path-usage
contract for owned build outputs, not a general filesystem race-hardening API.

The ABI-control driver now owns one temporary directory and prepared include
tree per invocation, with the compiler command parsed once. Each C probe still
compiles and validates independently; all matching/negative controls remain.
The C/C++ standalone-header and actual native-link contracts are retained.

These are build/tooling improvements. Active-profile capture and report costs
still need isolated A/B/B/A measurements before changing runtime defaults. The
tag is created locally only after final-source validation; pushing it would
trigger actual publication and is outside this tag-creation request.

The earlier release-preparation review and results follow. Final-source local,
CI and dry-run evidence is reported with delivery rather than attributed to
the earlier source commit.

---

Base: a1f256a, clean and current after git pull --ff-only. All three released
crates remain version 0.7.3, edition 2024, without a declared Cargo rust-version.
Local verification uses stable Rust 1.99.1. The source pin is the official
mimalloc v3.6.1 release, 56dc5eb80145f9a3ba7ca23d47c6a248b9d8586c, rather than
post-release dev3. The upstream/PR audit is in [UPSTREAM_REVIEW.md](UPSTREAM_REVIEW.md).

## Delivered APIs and compatibility fixes

Two public C23 functions are added to the sys bindings and unsafe MiMalloc
wrapper: free_sized(pointer, size) and free_aligned_sized(pointer, alignment,
size). The aligned argument order differs from the older free_size_aligned.
Both values must match the original allocation. Tests cover null, irregular
sizes, large blocks and over-aligned blocks under the native feature matrix.
These are aliases of existing native free implementations; there is no claim
of a faster algorithm. GlobalAlloc retains its direct aligned allocation and
general cross-thread free path.

The official release still fails standalone QSBR header compilation because
it uses a private macro and lacks its public base include. Export a generated
include tree with a guarded constructor declaration correction, preserving
the original vendor checkout. C++ consumers get C linkage without importing
internal headers. DEP_MIMALLOC_INCLUDE points to that actual exported tree,
and the ABI checker invokes the same Rust transformation. Tests compile the
four public headers separately in C and C++, compare the exported correction,
reject changed anchors and link/run a C++ consumer against the native C archive.
This is an explicit downstream correction, not a claim of an upstream fix.

The removed native weak-initializer function also invalidates our previous
Windows static-CRT source patch. Replace the generated theap copy with one
generated random.c correction: while the CRT is unavailable, every initializer
entry, including the new split retry, bypasses system entropy and keeps a weak
context. Once the CRT is ready, native strong initialization/reseeding resumes.
Intentional bootstrap fallback does not consume the warning for an actual OS
entropy failure. Keep the late .CRT$XCT startup fix and thread cleanup. Dynamic
CRT source is unchanged. A C probe extracted from the corrected initializer
checks early bypass, later strong entropy and once-only real failure warnings.
Windows startup/runtime CI is needed in addition to these local controls.

QSBR still requires a custom host adapter. Its default constructor reports
ENOSYS through the mimalloc error callback and returns null; errno is not an
API guarantee. The retained-owner behavior in QSBR-enabled mi_thread_id_of is
documented without changing the ordinary build's ownership-query behavior.

## Functional and performance review

Reviewed the new native delta, public ABI, build/startup path, C++ consumers,
allocator forwarding, heap/arena lifetime boundaries, profiling feature/lifecycle
contracts and package boundaries. The profiler's hot-state isolation, no_hooks
opt-out, report-time capacity/lock rules and allocator-neutral default graph
remain intact. No additional confirmed Rust runtime defect was established in
these paths. Native post-release MTE, sealed-area and setuid-option changes are
outside this formal-release pin and are not advertised as delivered features.

Performance priorities remain active-profile stack capture and symbol/mapping
work, stats merging/reclaim freshness versus throughput/RSS, and arena purging
versus tail latency. The current review delivers standard API access and
release compatibility; it does not establish a causal throughput gain. Use
isolated resources, frozen binaries, same-binary controls and A/B/B/A drift/loss
gates before attributing changes. Shared CI diagnostics are not such evidence.

## Local verification and publication boundary

- Public inventory: 135 bound functions, 45 option IDs; ABI projection: 126
  extern signatures, nine callbacks, three layouts, seven controls. No option,
  callback or layout changes in the selected release range.
- Default, secure/debug, no_profile and combined feature tests; no_hooks tests;
  all-target check; Clippy/rustdoc with warnings denied; format/diff checks.
  The allocator has 46 unit cases; profiling retains 77 default / 19 no_hooks
  cases; xtask has 15 cases. Native options retain 34 isolated process cases.
- Both isolated providers pass debug/release contracts. Two explicitly invoked
  release pressure cases pass. Normalized profiling archives pass default,
  all-feature and provider validation. Verified archives contain 106 sys,
  12 allocator and 27 profiling files.
- Native C debug suite passes 8/8, secure allocation/arena/QSBR passes 3/3, and
  macOS zone-override fork/pprof passes 2/2 on the local host. New C/C++ header
  and entropy-probe cases also pass.

The official crates.io sparse index was read: none of the three 0.7.3 versions
is present at preparation time. Final-commit CI and release dry-run results
are associated with delivery; older runs do not validate this new native pin.
The user requested readiness and source commit/push. No release tag is created
or pushed, no package is uploaded, and no GitHub Release is created.
