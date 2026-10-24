# Rust maintenance tools

Run from the repository root with stable Rust and Cargo:

    cargo xtask help
    cargo test --locked -p xtask
    cargo xtask check-api --json target/upstream-api-audit.json
    cargo xtask check-abi
    cargo xtask test-abi
    cargo xtask test-providers --local-dependencies
    cargo xtask test-providers --local-dependencies --release
    cargo xtask verify-package --local-dependencies --allow-dirty
    cargo xtask release-check 0.7.2
    cargo xtask release-notes 0.7.2 --output target/release-notes.md

The crate is unpublished and excluded from default workspace members. Its
dependencies do not become allocator/profiler runtime dependencies. All version
requirements stay in the root workspace manifest. The CI includes Windows,
macOS, Linux and Rust 1.95 compatibility checks.

## Gates

The API inventory checks public names and numeric option IDs. The ABI checker
uses the Rust syntax tree to project the supported extern-C declarations,
callbacks and repr(C) layouts into a C compiler probe. It rejects unsupported
types/conventions rather than silently omitting them. Seven matching/mutation
controls exercise compiler diagnostics, layout order, size aliases and legacy
option aliases. GCC or Clang is required for these C probes.

Provider tests generate separate Cargo workspaces, seed their reviewed lockfile
and reject graphs containing both native allocator providers. Package validation
extracts the actual Cargo archive into a temporary directory, rejects links and
escaping paths, and tests the default core, all features, release profiles and
both providers. Local dependency patches are explicit; omit that flag after
publication to require official registry allocator versions.

The release commands validate versions and internal requirements, preserve the
changelog section and prior-tag comparison, and produce GitHub outputs without
evaluating input as shell code. Registry verification uses HTTPS, the official
sparse index and a downloaded archive SHA-256. A mismatching local archive fails
immediately; visibility/network retries remain bounded at 40 attempts.

    cargo xtask verify-registry rustfs-mimalloc-sys 0.7.1
    cargo xtask verify-registry rustfs-mimalloc-sys 0.7.2 --archive target/package/rustfs-mimalloc-sys-0.7.2.crate

Registry verification is read-only. These commands never upload a package,
create a tag or dispatch a workflow.

Local archive, downloaded archive and frozen executable hashes are streamed
through a 32 KiB payload buffer. HTTP/TLS transport buffers are additional.
Network archives retain their 64 MiB limit and oversize failures are terminal;
file hashing does not impose that download limit on benchmark executables.

## Benchmarks

    cargo xtask bench-heap-abba --offline --output target/heap-abba-run1
    cargo xtask bench-profile-abba --offline --output target/profile-abba-run1
    cargo xtask bench-profiling-abba --baseline /path/to/baseline --candidate /path/to/candidate --output /path/to/fresh-output

The heap runner compares ordinary heap lookup to a cached ThreadHeap. The
profile runner freezes default and no_profile executables. Both retain four
separate A/B/B/A process logs, source/compiler/build provenance, unit-normalized
Criterion estimates and the 5% baseline/candidate drift gate. Rejected workloads
keep their evidence and have no performance-change estimate.

The frozen-profiler runner requires Linux with its affinity already restricted
to two CPUs. The operator must reserve those resources and run a same-binary
control first. It rejects workload/allocator overrides and checks record loss,
pending records, live drops and periodic report counts before allowing a
throughput conclusion. CLI affinity checks do not establish resource isolation.
