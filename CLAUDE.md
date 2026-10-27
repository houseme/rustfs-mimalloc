# CLAUDE.md — Project Instructions

## Project

rustfs-mimalloc: high-performance mimalloc V3 bindings for Rust, pinned to
V3.6.1 development snapshot commit `810774fb`.
Cargo manifests intentionally omit `rust-version`. CI checks stable and Rust
1.95.0 compatibility; this is not a declared minimum supported Rust version.
Edition 2024 alone does not prove compiler or dependency compatibility. Lint
and documentation use stable; release jobs share one resolved source commit.

## Structure

```
Cargo.toml                      workspace root
rustfs-mimalloc-sys/            FFI sys crate (C compilation + bindings)
rustfs-mimalloc/                safe Rust wrapper (GlobalAlloc + stats/heap APIs)
rustfs-mimalloc-profiling/      Allocator-neutral Rust heap profiler, versioned with the allocator
xtask/                         Unpublished Rust maintenance, validation and release tools
.agents/skills/release.md       release skill (/release)
.github/workflows/ci.yml        CI
.github/workflows/release.yml   three-crate release workflow
docs/release-checklist.md       release checklist (git ignored)
```

## Build & Test

Maintenance and release tools use Rust: run cargo xtask help. The unpublished
xtask crate is excluded from default members and allocator dependency graphs.

```bash
cargo build
cargo test
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Features

`secure` / `debug` / `debug_in_debug` / `override` / `local_dynamic_tls` / `no_thp` / `no_profile`

The profiling crate separately offers `rustfs-allocator` and application-level
`no_hooks`. The latter disables observation for the unified dependency graph
and makes Profiler::start return InvalidConfig; it differs from native no_profile.

## Dependency and changelog policy

All dependency version requirements belong in root `[workspace.dependencies]`.
Members inherit them with `workspace = true` and keep feature selections and
target conditions locally. The root `CHANGELOG.md` is the only changelog for
all three crates; member READMEs link to it.

## Profiling providers

The profiling core has no allocator dependency enabled by default. Keep provider
requirements in root `workspace.dependencies`. The provider-test driver generates
isolated consumer workspaces so feature unification cannot link both providers.
`rustfs-allocator` is an additive compatibility API feature on the profiling crate.

## Release

Use the `/release` skill to publish a new version. Flow: version validation → local checks → commit → tag → push triggers CI auto-publish.

See `docs/release-checklist.md` for detailed documentation (git ignored).

All three crates inherit the workspace version. The release workflow publishes
sys, allocator, then profiling, verifies official registry visibility/checksums,
and tests the normalized profiling archive against registry dependencies.
See `.agents/skills/release.md` for package checks and the required dry run.
