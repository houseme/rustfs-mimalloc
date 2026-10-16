# CLAUDE.md — Project Instructions

## Project

rustfs-mimalloc: high-performance mimalloc V3 bindings for Rust, pinned to
post-v3.5.4 interim commit `6f45cc52`.
Cargo manifests intentionally omit `rust-version`. CI checks stable and Rust
1.96.0 compatibility; this is not a declared minimum supported Rust version.

## Structure

```
Cargo.toml                      workspace root
rustfs-mimalloc-sys/            FFI sys crate (C compilation + bindings)
rustfs-mimalloc/                safe Rust wrapper (GlobalAlloc + stats/heap APIs)
rustfs-mimalloc-profiling/      Rust heap profiler, versioned with the allocator
.agents/skills/release.md       release skill (/release)
.github/workflows/ci.yml        CI
.github/workflows/release.yml   three-crate release workflow
docs/release-checklist.md       release checklist (git ignored)
```

## Build & Test

```bash
cargo build
cargo test
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Features

`secure` / `debug` / `debug_in_debug` / `override` / `local_dynamic_tls` / `no_thp` / `no_profile`

## Dependency and changelog policy

All dependency version requirements belong in root `[workspace.dependencies]`.
Members inherit them with `workspace = true` and keep feature selections and
target conditions locally. The root `CHANGELOG.md` is the only changelog for
all three crates; member READMEs link to it.

## Release

Use the `/release` skill to publish a new version. Flow: version validation → local checks → commit → tag → push triggers CI auto-publish.

See `docs/release-checklist.md` for detailed documentation (git ignored).

All three crates inherit the workspace version. The release workflow publishes
sys, allocator, then profiling, verifies official registry visibility/checksums,
and tests the normalized profiling archive against registry dependencies.
See `.agents/skills/release.md` for package checks and the required dry run.
