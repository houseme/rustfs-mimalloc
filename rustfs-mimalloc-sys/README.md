# rustfs-mimalloc-sys

[![Crates.io](https://img.shields.io/crates/v/rustfs-mimalloc-sys.svg)](https://crates.io/crates/rustfs-mimalloc-sys)
[![Documentation](https://docs.rs/rustfs-mimalloc-sys/badge.svg)](https://docs.rs/rustfs-mimalloc-sys)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](../LICENSE)

Low-level FFI bindings to [mimalloc](https://github.com/microsoft/mimalloc) V3, pinned to V3.6.2 development commit `07db29aa` (not a release tag).

For a safe, ergonomic wrapper, use [`rustfs-mimalloc`](https://crates.io/crates/rustfs-mimalloc).

## Usage

```toml
[dependencies]
rustfs-mimalloc-sys = "0.7.5"
```

```rust
unsafe {
    let ptr = rustfs_mimalloc_sys::mi_malloc_aligned(64, 8);
    // ... use ptr ...
    rustfs_mimalloc_sys::mi_free(ptr);
}
```

## Features

| Feature | Description |
|---------|-------------|
| `secure` | Metadata guard pages, checked frees and encoded/randomized free lists (MI_SECURE=4) |
| `debug` | mimalloc debug checks |
| `debug_in_debug` | Auto-enable `debug` in Cargo debug builds |
| `override` | Override system `malloc`/`free` on non-Windows targets |
| `local_dynamic_tls` | Use local-dynamic TLS model |
| `no_thp` | Disable Transparent Huge Pages |
| `no_profile` | Compile out allocation profiling and automatic pprof startup |

## Allocation Profiling Capability

`no_profile` sets `MI_PROFILE=0`. Check `MI_PROFILE_ENABLED` for the sys crate's
compiled capability (including Cargo feature unification). Allocation and free
callbacks produce no profiling samples, and `MIMALLOC_PROFILE` automatic startup
is disabled. Statistics, guarded debug allocations and secure checks are retained.
Raw profiler control/snapshot symbols remain linked: explicit snapshot callbacks
can still execute, but there are no sampled allocations to report.

The build generates a guarded sampler correction for `no_profile`, forcing its
profile rate to zero and disabling enabled-profiler lookup. This addresses an
upstream MI_PROFILE=0 regression with explicit profiler registration. Explicit
raw snapshots and the built-in pprof implementation remain available. Both
ordinary and Windows static-CRT builds use the same correction.

## Windows Static CRT

All Windows C builds preserve the linker's `__ImageBase` symbol with an explicit
`extern` declaration. The pinned upstream linkage macro is empty in C and would
otherwise create a variable with that reserved name, breaking static UCRT's
image-relative API lookups before `main`.

On `*-pc-windows-msvc`, configure the final Rust binary with
`-C target-feature=+crt-static`. The build script selects `/MT` for mimalloc
from Cargo's target features and Rust codegen flags, so the C allocator and Rust
share one CRT mode. Without that target feature, the default remains `/MD`.

For the pinned upstream snapshot, static CRT builds defer full process startup
and strong random reseeding to a late CRT initializer (`.CRT$XCT`), after UCRT
initialization. TLS process-attach runs while UCRT is still initializing; early
allocations retain a weak bootstrap seed until the late initializer reseeds it.
TLS thread-exit cleanup remains enabled.

## What's Included

This crate vendors the mimalloc V3 C source and compiles it via the `cc` crate. No system mimalloc installation required.

- Static mimalloc linking; platform system libraries are linked as needed
- Platform-specific flags handled automatically (Windows libs, musl compat, TLS model)
- `links = "mimalloc"` — exports `DEP_MIMALLOC_INCLUDE` for downstream C/C++ crates
- V3 small allocation/free APIs, including `mi_wmalloc_small`,
  `mi_wzalloc_small`, the thread-local heap word-size variants,
  `mi_free_small_nonnull`, and inline Rust mirrors for `mi_malloc_csize`,
  `mi_zalloc_csize`, `mi_theap_malloc_csize`, `mi_theap_zalloc_csize`,
  `mi_free_csize`, `mi_free_csize_nonnull`, `mi_free_csize_aligned`, and
  `mi_free_csize_aligned_nonnull`
- Raw `mi_theap_alloc_new*` C++-semantics FFI; callers must not allow a foreign
  exception from an installed C++ new-handler to unwind into Rust
- Raw experimental profiling FFI from `mimalloc-profile.h`, including
  `mi_profiler_snapshot` and `mi_pprof_profiler_{new,delete}`. V3.5.4 adds
  `on_snapshot` to `mi_profiler_t` and changes callback heap pointers to mutable
- `mi_free_small_local{,_nonnull}` for small pages still owned by the calling thread
- `mi_arena_area` and `mi_arena_contains` query reserved regions and membership
- Raw error, output and deferred-free callback registration APIs
- `mi_arenas_purge`, `mi_option_stats_merge_threshold`, and
  `mi_option_arena_purge_immediate_size`, plus all `mi_option_profile_*`
  controls, default/clamped option APIs, aligned
  heap reallocation, and thread-local aligned allocation/reallocation APIs

The V3.6.2 development pin adds 110 commits after the 0.7.4 native pin,
reviewed on 2026-11-12. Raw `mi_thread_heap_release` releases a calling-thread
context while preserving live blocks and permanently disabling that thread's
future allocations in the heap. Cached aliases must be discarded first.
The current deprecated precise option spelling is
`mi_option_deprecated_unsafe_guarded_precise` at ID 31, with older Rust names
retained. Public callbacks and exposed layouts retain their signatures.

Native fixes cover concurrent heap deletion and remote frees, profiler attachment
generations and stopped-profiler free callbacks, pprof writes without stdio,
Linux pprof mapping offsets, arena/purge accounting and QSBR retention. Native
aligned-allocation fast paths, bitmap updates, huge searches and realloc shrinking
also change. Measure service throughput, RSS and tail latency for these policies.

The pinned upstream source still has a standalone QSBR header defect. Build scripts
export a corrected include tree through `DEP_MIMALLOC_INCLUDE`, using a guarded
copy that adds the base public header and standard C++ C-linkage guards. Use
that include path for downstream C/C++; the vendored originals remain pristine.
The ABI tool uses the same transformation, and tests compile all four public
headers individually in C/C++ and link a C++ consumer to the native C library.
QSBR's ENOSYS diagnostic is delivered through mimalloc's error callback, not a
promise to set libc errno. See [the upstream review](../UPSTREAM_REVIEW.md).
Available APIs include `mi_arenas_purge` and two options
for thread-heap stats merging and large-range purging.
The previously added `mi_thread_id_of`,
`mi_heap_set_debug_fill_preserve`, `mi_theap_delete` and
`mi_theap_quiescent_abandon` all retain raw bindings. Context deletion
invalidates cached aliases. Foreign-context abandonment requires a quiescent
owner outside mimalloc and no concurrent heap/context deletion. Reclaim on free
defaults to originating-context-only (`0`) again; configure before heap creation
to opt into cross-context reclaim (`1`) or disallow it (`-1`). Existing option
IDs and profiler ABI fields are retained. Earlier updates also exposed `mi_expand`,
zero-initialized
reallocation, checked heap array allocation, and heap small-allocation APIs.
Run `cargo xtask check-api` from the repository root to audit
binding names and option IDs; integration tests cover linkage and behavior.
The complementary `cargo xtask check-abi` uses the host GCC/Clang C
compiler to check extern signatures, callback compatibility, and size/alignment/
offsets of the exposed heap-area and profiler layouts. This C projection check
does not replace runtime linking or target-specific validation.

V3.6.0 adds experimental `mi_heap_new_qsbr` in `mimalloc/qsbr.h`. The default
build has no QSBR adapter and returns null/ENOSYS. Host callbacks and embedded C
metadata are application-defined, not allocator exports or Rust-managed reader
state. Protected heaps cannot use normal `mi_heap_delete`; destroy only after
all readers and mutators stop. This remains raw FFI without a Cargo QSBR feature.
Protected full pages follow the configured retention policy. The last free of
a delayed abandoned page can create a same-heap context and initialize its
receiving thread, independently of nonempty reclaim limits. Exclude late TLS
destructor frees after mimalloc shutdown. Allocation retries when an empty
reclaimed page was released; the changed helper is internal, not new raw FFI.
The deprecated precise option retains ID 31 and the old Rust names.
The MI_UNSAFE_GUARDED_PRECISE build control must preserve the caller's alignment
contract. Reclaim-on-free policy is captured at heap creation in this pin.

The latest native changes add a bounded internal integer parser and correct
oversized-page THP detection/advice; no public API or ABI changes. Environment integers
accept hexadecimal and reject zero-padded decimal input. K/KiB suffixes now scale
correctly; native profiling defaults are 512 KiB samples and 1 GiB snapshots.
PMD size is read from sysfs with a 2 MiB fallback. Mappings at least as large as
detected PMD pages above 8 MiB receive `MADV_NOHUGEPAGE`. Upstream PR #1445
fixes Linux/Android `no_thp`, so no local source copy is needed. Native probes validate bounded reads and advice
decisions; deployment RSS still requires measurement on the target kernel.

Upstream custom tracking requires `MI_TRACK_CUSTOM` at C compile time and two
application-defined C symbols. See the wrapper's `custom_tracking` example;
hooks must never allocate or unwind, and must tolerate concurrent/startup calls.
These are application symbols, not mimalloc exports or registration functions.

Dedicated heaps and worker contexts can sample after lazy activation by native
generic-allocation maintenance. The caller/worker callback tests exercise that
activation and nested `on_alloc` suppression. Profiling callbacks must not unwind;
stop and detach a profiler, free sampled allocations, and quiesce callbacks
before deleting it. `mi_profiler_start`/`stop` return the previous running state.

Upstream compile-time opt-ins `MI_NO_CXX_NEW_HANDLER` (omit the weak C++ new-handler
symbol) and `MI_NO_ALIGN_FALLBACK_WARNING` (omit the benign fallback warning) can
be passed through target-specific `CFLAGS`. Neither is enabled by default or
exposed as a redundant Cargo feature. Disabling the handler changes the raw
`mi_*new` out-of-memory path; these controls are for embedding requirements.
`MI_ARENA_PURGE_ON_FREE=1` is a separate upstream CFLAGS opt-in to process
expired purges on arena free; it remains disabled by default.

Upstream's native pprof fallback now uses `unwind.h` and `_Unwind_Backtrace`
instead of `unw_backtrace`. The Cargo build does not automatically enable this
fallback on musl; native musl backtraces still need a target-configured compiler
unwinder. The allocator-neutral Rust profiler uses its own capture path. GNU Linux
and macOS use `execinfo`, Windows uses `CaptureStackBackTrace`; FreeBSD/DragonFly
also link `execinfo` and `util`.

## Rust Toolchain Compatibility

Cargo manifests intentionally omit `rust-version`. CI checks stable Rust and
Rust 1.95.0 compatibility; no minimum Rust version is declared by these crates.

## License

Apache-2.0. The vendored mimalloc C library is MIT-licensed.

Release notes are maintained in the shared
[workspace changelog](https://github.com/houseme/rustfs-mimalloc/blob/main/CHANGELOG.md).

## Native MTE and PAC controls

Default Cargo builds leave upstream's experimental memory tagging (MTE) and
pointer authentication (PAC) disabled. `mi_option_mte` and `MIMALLOC_MTE` do not
change that build choice. The runtime MTE option accepts off/on or 0/1 and, on
Linux, sync/async/preferred (2/3/4). Its configured default is 1.

Advanced native builds can pass numeric definitions through target-specific
`CFLAGS`: `-DMI_MTE=1` prefers MTE where available, `-DMI_MTE=2` requires it;
`-DMI_PAC=1` selects software pointer signing, `-DMI_PAC=2` requires arm64
hardware PAC with GCC/Clang. These are C definitions, not CMake string values.
MTE is arm64-only and needs OS support (including appropriate Apple process
entitlements). Required hardware modes fail when support is unavailable.
These modes can change default native security checks and allocation costs.
Hardware-enabled execution is outside this update's validation; no Cargo
feature or automatic CPU tuning is added. Target CFLAGS configure native C;
RUSTFLAGS alone do not configure its compiler.

Deprecated upstream options have no effect in v3. `mi_heap_theap` never returns
null; a failed or torn-down thread context is represented by an empty context
whose allocations fail. A non-null context is not proof of allocation readiness.

## Secure environment policy

With the `secure` feature, native `MI_SECURE_ENV` defaults to 1. Only
`MIMALLOC_SHOW_STATS`, `MIMALLOC_VERBOSE` and `MIMALLOC_ALLOW_THP` environment
options are applied. Automatic `MIMALLOC_PROFILE` startup is restricted.
Programmatic option APIs remain available with their initialization/synchronization
requirements. Target CFLAGS may select `-DMI_SECURE_ENV=0`; native secure mode
still restricts large-page, MTE, destroy-on-exit and precise-placement controls.
`MI_UNSAFE_GUARDED_PRECISE=1` is a separate compile-time opt-in that can violate
Rust alignment contracts. The runtime precise option is deprecated.

Native profiler attachment establishes a fresh generation and stops sampling.
`on_free` continues after `mi_profiler_stop` for sampled blocks until detachment.
Detach and quiesce every callback before destroying profiler state.
