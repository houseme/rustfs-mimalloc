# rustfs-mimalloc-sys

[![Crates.io](https://img.shields.io/crates/v/rustfs-mimalloc-sys.svg)](https://crates.io/crates/rustfs-mimalloc-sys)
[![Documentation](https://docs.rs/rustfs-mimalloc-sys/badge.svg)](https://docs.rs/rustfs-mimalloc-sys)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](../LICENSE)

Low-level FFI bindings to [mimalloc](https://github.com/microsoft/mimalloc) V3 (v3.5.3).

For a safe, ergonomic wrapper, use [`rustfs-mimalloc`](https://crates.io/crates/rustfs-mimalloc).

## Usage

```toml
[dependencies]
rustfs-mimalloc-sys = "0.5.5"
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
| `secure` | Heap allocation encryption (MI_SECURE=4) |
| `debug` | mimalloc debug checks |
| `debug_in_debug` | Auto-enable `debug` in Cargo debug builds |
| `override` | Override system `malloc`/`free` |
| `local_dynamic_tls` | Use local-dynamic TLS model |
| `no_thp` | Disable Transparent Huge Pages |

## Windows Static CRT

On `*-pc-windows-msvc`, configure the final Rust binary with
`-C target-feature=+crt-static`. The build script lets `cc` select `/MT` for
mimalloc from Cargo's `crt-static` target feature, so the C allocator and Rust
share one CRT mode. Without that target feature, the default remains `/MD`.

## What's Included

This crate vendors the mimalloc V3 C source and compiles it via the `cc` crate. No system mimalloc installation required.

- Static linking — no runtime dependencies
- Platform-specific flags handled automatically (Windows libs, musl compat, TLS model)
- `links = "mimalloc"` — exports `DEP_MIMALLOC_INCLUDE` for downstream C/C++ crates
- V3.5.3 small allocation/free APIs, including `mi_wmalloc_small`,
  `mi_wzalloc_small`, the thread-local heap word-size variants,
  `mi_free_small_nonnull`, and inline Rust mirrors for `mi_malloc_csize`,
  `mi_zalloc_csize`, `mi_theap_malloc_csize`, `mi_theap_zalloc_csize`,
  `mi_free_csize`, `mi_free_csize_nonnull`, `mi_free_csize_aligned`, and
  `mi_free_csize_aligned_nonnull`
- Raw `mi_theap_alloc_new*` C++-semantics FFI; callers must not allow a foreign
  exception from an installed C++ new-handler to unwind into Rust
- Raw experimental profiling FFI from `mimalloc-profile.h`

## MSRV

Rust 1.96.0. Rolling support window for the latest three stable Rust releases.

## License

Apache-2.0. The vendored mimalloc C library is MIT-licensed.
