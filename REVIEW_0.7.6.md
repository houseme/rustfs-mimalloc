# 0.7.6 allocator and performance review — 2026-11-27

The remote project base `d927f57` carries 0.7.5 and one Unreleased section.
The workspace, inherited requirements, lockfile and current installation
examples now use 0.7.6. The pending notes are in a single 0.7.6 section.
The follow-up moved the native pin from `5424c54c` to `a34616d3`.
The source and PR comparison is recorded in [UPSTREAM_REVIEW.md](UPSTREAM_REVIEW.md).

## Official Rust API synchronization

The official allocator exposes `Allocator` for `MiMalloc` and borrowed
heaps, with `MiMallocExcess` reporting native usable capacity. A Rust
1.100 stable compile probe and standard `Vec::new_in` tests confirm
availability. This workspace now exposes those three implementations.
Returning requested capacity avoids an extra size query for the ordinary
allocator. Returning usable capacity lets containers use size-class
headroom. The zeroed path explicitly initializes any returned tail beyond
the requested size. Fitting layouts can describe more bytes than the
original request, so trait deallocation uses general native free.

The official lower secure levels and five linked statistics exports are
also available. The strongest unified Cargo secure feature wins.
Source-level differences and the omitted linkless declaration are listed
in the official Rust audit.

The follow-up passed macOS arm64 workspace tests with default features,
`secure_basic`, `secure_checked` and all features. Combined lower-level
features selected level 2. API, ABI, Clippy, rustdoc and local package checks
passed for the unified 0.7.6 version. Cross-built Docker tests on Linux arm64
passed 70 allocator tests in each of the default and combined checked/no_profile
configurations, plus the raw API and 44 option cases in each configuration.

## Owned statistics JSON

Process and heap statistics previously always copied the native buffer into
a Rust `String`. The official-style `MiString` owner lets callers borrow
the result directly. A standalone allocator-count test records zero Rust
allocator calls for owned JSON and a Rust allocation for each `String`
API, including optimized builds. The native JSON buffer is still allocated
by mimalloc. Lifetime tests cover invalid UTF-8, dropping the heap before
reading its snapshot and freeing the buffer on another thread.

Default and all-feature macOS workspace tests passed, along with Clippy,
rustdoc, API/ABI and local archive verification. The optimized allocator-count
test passed on macOS and in Linux arm64 Docker. Linux arm64 builds passed
72 allocator tests apiece in the default and secure/debug/no_profile modes.

## Reusable statistics buffers

For repeated collection, `stats_json_into()` uses the existing native
caller-buffer interface. A nonempty buffer selects native fixed-buffer
mode without the allocation and resizing used for owned JSON. The wrapper
returns `None` for empty or insufficient storage. Text statistics use
`stats_print_into()` to overwrite a vector up to its current capacity,
with a truncation result. The callback never grows that vector, preserving
its allocation-free behavior during native output. The original text
`String` helpers still reserve their own fixed buffer.

Allocator-count tests check both process and heap paths for zero Rust
allocations, verify the JSON result borrows the supplied bytes and exercise
short buffers.

The follow-up passed default and all-feature macOS workspace tests, API and
ABI checks, Clippy, rustdoc, provider tests and local archive verification.
Linux arm64 Docker passed 72 allocator tests per configuration and the
allocation-count harness in both default and secure/debug/no_profile builds.

### JSON encoding and caller parsing — 2026-11-28

The release-mode benchmark used stable Rust 1.100.0, mimalloc 3.7.2 and
`serde_json` 1.0.154 with `MiMalloc` as the global allocator. One thread
retained 224 allocations across seven sizes. The process and heap fixtures
each contained about 15 KB of JSON and 52 top-level keys.

The C benchmark timed `mi_stats_as_json` with a pre-captured allocator
snapshot and a reused 64 KiB output buffer. The call still clears the
output buffer and samples process information. The Rust API measurement
timed `stats_json_into()` through return of its borrowed C string. The
parser cases reused fixed JSON bytes and included the cost of dropping
their results. The typed parser extracted the mimalloc version, current
RSS, current commit and page fault count and checked those fields against
the full `serde_json::Value` before timing.

Each run used 12 batches per case with alternating order, after warmup.
The results use median thread CPU time per operation. The drift check
compared the two halves of each run and the two run medians against a 5%
threshold. Linux ran in a Debian arm64 Docker container pinned to one
virtual CPU.

| Environment and scope | Native JSON encoding | Rust `stats_json_into()` | Full `Value` parse | Typed parse |
| --- | ---: | ---: | ---: | ---: |
| macOS arm64, process | 37.26 | 38.67 | 32.00 | 12.99 |
| macOS arm64, heap | 37.06 | 37.51 | 32.42 | 12.96 |
| Linux arm64 Docker, process | 40.61 | 42.70 | 33.27 | 13.13 |
| Linux arm64 Docker, heap (observed range) | 40.46–43.60 | 41.03–44.26 | 33.30–37.77 | 12.93–14.34 |

Values are in µs per operation. The point estimates pool the two initial
runs. The Linux heap row gives the range of per-run medians across all six
runs. The full heap parse exceeded the 5% drift threshold in each Linux
pair, and the other heap stages exceeded it in at least one pair. The
initial macOS process snapshot timing also exceeded the threshold. A
standalone heap snapshot took 0.23 µs on macOS and 0.20 µs in the two
Linux repeat pairs. The standalone Linux process snapshot took 1.01–1.02 µs.

Where the encoding and both parse timings passed the drift check across
the available pairs, typed parsing reduced parse CPU time by 59–61%.
Native encoding was 2.9–3.1 times as expensive as typed parsing. Reducing
the buffer from 64 KiB to 16 KiB changed the stable encoding point estimates
by at most 1.6% in this fixture.

The local benchmark source, binary hashes, per-batch CPU and wall times,
and drift summaries are under `target/stats-json-cost/`.

### Native JSON append cost — 2026-11-28

The follow-up used the same fixture and 5% drift check. Each optimized
binary timed the installed native entry point and five builds of the same
serializer in alternating order. The variants used the original code,
omitted the buffer clear, replayed process information, replayed the
preformatted malloc and page bin rows, or replaced byte-wise appends of
fitting fragments with a bulk copy. The replay variants attribute cost
for a fixed snapshot. The bulk-copy variant keeps the existing path for
fragments that trigger truncation or buffer growth.

The standalone controls put the 64 KiB clear below 0.4 µs and
`MiMalloc::process_info()` at about 0.8 µs on macOS and 0.2 µs on Linux.
On macOS, replaying the preformatted bins removed about 10 µs of the
standalone serialization cost, including the avoided bin metadata lookups.
The standalone Linux reference was materially faster than the crate-linked
serializer, so the candidate was also tested through two complete crate
builds in original/candidate/candidate/original order.

| Linux arm64 Docker | Original CPU µs/op | Bulk copy CPU µs/op | Reduction |
| --- | ---: | ---: | ---: |
| Process native encoding | 42.20 | 18.09 | 57.1% |
| Heap native encoding | 40.82 | 17.85 | 56.3% |
| Process `stats_json_into()` | 42.94 | 19.52 | 54.5% |
| Heap `stats_json_into()` | 41.30 | 18.89 | 54.3% |

These four Linux cases passed the 5% drift check for both thread CPU and
wall time. The point estimates pool the two runs of each build. On macOS,
the initial same-binary attribution passed and the standalone append
variant went from about 37 µs to 19 µs. Both macOS complete-build attempts
exceeded the drift threshold. They retain the same direction of change
and are excluded from the complete-build reduction figures.

The correctness check froze both the allocator snapshot and process
metrics before comparing the original and bulk-copy paths. It compared
bytes and the return value for exact-fit, one-byte-short, and smaller
buffers, checked the guard bytes and NUL terminators, and verified the
owned result after growing its initial buffer. The patched checkout also
passed the release allocator and sys crate suites on macOS under default
features and with `secure_checked,debug,no_profile`.

The candidate patch was initially tested against an isolated copy of the
pinned mimalloc commit. The sys crate now generates the same statistics
source under Cargo's output directory alongside the existing platform
and feature patches. The sys crate archive includes the source generator
and the pinned input.

The generated source matched the prototype byte for byte. Repeating the
complete-build comparison in Linux arm64 Docker confirmed the following
thread CPU medians in µs per operation.

| Integrated path | Original | Bulk copy | Reduction |
| --- | ---: | ---: | ---: |
| Process native encoding | 41.44 | 18.15 | 56.2% |
| Process `stats_json_into()` | 42.74 | 18.80 | 56.0% |
| Heap `stats_json_into()` | 42.06 | 17.99 | 57.2% |

The integrated native heap encoding result exceeded the 5% drift threshold.
The integrated build passed default and all-feature release workspace
tests on macOS. The allocator, raw API, and buffer tests passed in Docker
with default and `secure_checked,debug,no_profile` features. Clippy, public
API and ABI checks, and offline sys archive verification passed.

The prototype and integrated measurements are under
`target/stats-json-attribution/` and `target/stats-json-integrated/`.

## Usable-capacity reallocation

`MiMalloc::realloc_aligned_with_usable_size` previously always used
`mi_urealloc_aligned`, which looks up the result's size after reallocation.
For alignment at most one machine word, the native implementation already
redirects aligned reallocations to ordinary reallocation if the original
address meets the requested alignment. The Rust wrapper now applies the
same guard and uses `mi_urealloc` to return size from that operation.
Offset pointers that fail the guard and over-aligned requests still use the
aligned entry point. Tests exercise null input, retained data, output
alignment, capacity reporting and preserved allocations after failed growth.
No workload-level speedup was measured.

## Further measurements

### dev3 pin comparison — 2026-11-28

Two complete release builds compared `5424c54c` and `a34616d3` in
Linux arm64 Docker on one virtual CPU. The benchmark allocated and freed
64-byte or 4096-byte blocks using `GlobalAlloc`, repeated heap lookup, or
cached `ThreadHeap` handles. The heap cases alternated between two heaps.
The original/current/current/original sequence used 12 batches per case
with alternating case order. The first run series exceeded the 5%
drift threshold.

Five cases passed the CPU and wall-time drift check in the longer repeat.
Their thread CPU medians changed by -2.7% to +1.4% between pins. The
64-byte repeated-lookup baseline still drifted by 7.4% and is excluded
from the comparison. In the current 4096-byte case, cached handles used
about 16 ns per allocation/free pair, compared with about 25 ns when
looking up the heap on every allocation. The benchmark keeps objects on
one thread. Raw results are under `target/upstream-refresh-076/`.

### Workloads to measure next

1. Compare usable-capacity reallocations for in-place shrink and moved
   growth, using ordinary, secure and over-aligned layouts. Track usable
   capacity and bytes copied alongside latency, with same-binary controls.
2. Compare cached ThreadHeap handles against repeated lookup under worker
   churn, including remote frees and idle-thread arena collection. Track
   heap retirement time, RSS and p99 allocation and purge latency.
   Profile time acquiring the reentrant heap locks separately from time
   spent in visitor callbacks, particularly on directly OS-backed pages.
3. Attribute the remaining profiling report costs to symbol resolution,
   serialization and live-map contention. Use the existing profiling
   diagnostics and verify sample counts and drops before drawing throughput
   conclusions.

The release workflow checks the official index before uploading and waits
60 seconds before fetching and verifying the published versions. That
workflow is included in the source preparation notes. This review covers
the source and local validation for 0.7.6.

## Validation

Stable Rust 1.100.0 on macOS arm64 passed default, secure/debug and
all-feature workspace tests, both isolated providers, the normalized
profiling archive, Clippy, rustdoc, release consistency and release-note
generation. The public API inventory, C ABI projection and eight controls
passed. Linux arm64 allocator test binaries were cross-built and run in
Docker, with 67 tests passing in each of the release default and
secure/debug/no_profile configurations.
