# Release Skill

Publish a new version of `rustfs-mimalloc` and `rustfs-mimalloc-sys` to crates.io, then create the GitHub Release.

## Trigger

Use this skill when the user asks to release, publish, bump a version, create a release tag, run a release dry run, or inspect release readiness.

## Synchronized Files

When release behavior, supported Rust versions, feature flags, or publish steps change, check these files together:

| File | Update When |
|------|-------------|
| `Cargo.toml` | Release version changes, MSRV changes, or workspace dependency versions change |
| `rustfs-mimalloc/Cargo.toml` | Package metadata, inherited fields, features, or dev-dependencies change |
| `rustfs-mimalloc-sys/Cargo.toml` | Package metadata, inherited fields, build features, or build dependencies change |
| `Cargo.lock` | Cargo updates package versions or dependency resolution |
| `.gitmodules` | mimalloc submodule URL or path changes |
| `CHANGELOG.md` | User-visible release notes or release date changes |
| `README.md` | Install snippet, feature table, MSRV, platform support, or comparison table changes |
| `rustfs-mimalloc/README.md` | Dependency snippet version, feature table, or MSRV changes |
| `rustfs-mimalloc-sys/README.md` | Dependency snippet version, feature table, or MSRV changes |
| `.github/workflows/ci.yml` | MSRV, feature matrix, musl setup, lint, docs, or bench gates change |
| `.github/workflows/release.yml` | Release gates, publish order, dry-run behavior, or release-note generation changes |
| `.agents/skills/release.md` | Any release process rule changes |
| `CLAUDE.md` | Project guidance, MSRV, feature list, or release flow changes |
| `docs/release-checklist.md` | Local release checklist mirror, if present; it is git ignored in this repo |

Do not use `git add -A` for releases. Stage explicit files so ignored/local notes and unrelated work do not leak into the release commit.

## Pre-check

Start with a clean, current tree:

```bash
git status --short --branch
git fetch origin --tags
git submodule update --init --recursive
git submodule status --recursive
git log --oneline --decorate -8
git tag --sort=-v:refname | head -8
```

If the worktree is dirty, inspect the diff and separate unrelated changes before continuing.

## Version Model

The two crates inherit their package version from the workspace:

```toml
# Cargo.toml
[workspace.package]
version = "<VERSION>"
```

The workspace and member manifests intentionally omit `rust-version`. Do not
reintroduce it during a release. CI keeps a Rust 1.96.0 compatibility job; this
is not a declared Cargo MSRV.

The wrapper's registry dependency requirement for the sys crate is also in the workspace root:

```toml
# Cargo.toml
[workspace.dependencies]
rustfs-mimalloc-sys = { path = "rustfs-mimalloc-sys", version = "<VERSION>" }
```

For a release version bump, update:

- `Cargo.toml`: `[workspace.package].version`
- `Cargo.toml`: `workspace.dependencies.rustfs-mimalloc.version`
- `Cargo.toml`: `workspace.dependencies.rustfs-mimalloc-sys.version`
- `CHANGELOG.md`: move relevant `Unreleased` entries under `## [<VERSION>] - <YYYY-MM-DD>`
- `README.md`: dependency snippet if the recommended version changes
- `rustfs-mimalloc/README.md`: dependency snippet version (e.g., `rustfs-mimalloc = "<VERSION>"`)
- `rustfs-mimalloc-sys/README.md`: dependency snippet version (e.g., `rustfs-mimalloc-sys = "<VERSION>"`)
- `Cargo.lock`: only if Cargo changes it after validation

Do not add direct `version = "..."` fields to the member crate manifests unless the workspace inheritance model changes.

## Version Consistency Check

Use `cargo metadata`; grep-based checks are wrong for workspace-inherited versions.

```bash
cargo metadata --no-deps --format-version 1 > /tmp/rustfs-mimalloc-metadata.json
python3 - /tmp/rustfs-mimalloc-metadata.json <VERSION> <<'PY'
import json
import sys

metadata_path, expected = sys.argv[1], sys.argv[2]
with open(metadata_path, encoding="utf-8") as f:
    metadata = json.load(f)

versions = {
    package["name"]: package["version"]
    for package in metadata["packages"]
    if package["name"] in {"rustfs-mimalloc", "rustfs-mimalloc-sys"}
}

missing = {"rustfs-mimalloc", "rustfs-mimalloc-sys"} - set(versions)
if missing:
    raise SystemExit(f"missing package metadata for: {', '.join(sorted(missing))}")

mismatched = {name: version for name, version in versions.items() if version != expected}
if mismatched:
    details = ", ".join(f"{name}={version}" for name, version in sorted(mismatched.items()))
    raise SystemExit(f"version mismatch; expected {expected}, got {details}")

print(f"Version OK: {expected}")
PY
```

## Local Validation

Run these checks before tagging. Stop on the first failure and report the exact failing command.

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo test --workspace --features secure,debug
cargo test --workspace --features no_profile
cargo test --workspace --features secure,debug,no_profile
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
cargo package -p rustfs-mimalloc-sys --allow-dirty
cargo package -p rustfs-mimalloc --allow-dirty --config 'patch.crates-io.rustfs-mimalloc-sys.path="rustfs-mimalloc-sys"'
```

The wrapper crate depends on the exact sys crate release version. Before
`rustfs-mimalloc-sys` is published, a plain wrapper `cargo package --verify`
cannot resolve that version from the registry. Use the temporary local
`patch.crates-io` config above for pre-tag verification; the release workflow
still publishes `rustfs-mimalloc-sys` first and then publishes the wrapper
against the real registry version.

For musl validation, run this only when the host has a musl C compiler installed:

```bash
cargo build --target x86_64-unknown-linux-musl --features secure
```

On GitHub Actions, `ci.yml` installs `musl-tools` before the musl cross-build.

## Commit

Stage only the files that belong to the release:

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md README.md
git add .gitmodules
git add rustfs-mimalloc/Cargo.toml rustfs-mimalloc-sys/Cargo.toml
git add .github/workflows/ci.yml .github/workflows/release.yml
git add .agents/skills/release.md CLAUDE.md
```

Only include a path if it actually changed.

Commit with the required trailer:

```bash
git commit -m "chore(release): v<VERSION>" \
  -m "Co-Authored-By: heihutu <heihutu@gmail.com>" \
  -m "Co-Authored-By: zhi22915 <qiuzgang@gmail.com>"
```

## Tag And Publish

Create the tag only after local validation passes and the release commit is ready:

```bash
git tag v<VERSION>
git push origin main
git push origin v<VERSION>
```

Pushing the tag triggers `.github/workflows/release.yml`.

The release workflow publishes in this order:

1. `rustfs-mimalloc-sys`
2. wait for crates.io index propagation
3. `rustfs-mimalloc`
4. GitHub Release

## Dry Run

Use a dry run before the real publish when changing release infrastructure:

```bash
gh workflow run release.yml -f version=<VERSION> -f dry_run=true
```

Watch it with:

```bash
gh run list --workflow release.yml --limit 5
gh run watch <RUN_ID>
```

## Confirmation

After pushing a real release, report:

- tag: `v<VERSION>`
- release workflow run URL
- crates.io sys crate URL: `https://crates.io/crates/rustfs-mimalloc-sys/<VERSION>`
- crates.io wrapper crate URL: `https://crates.io/crates/rustfs-mimalloc/<VERSION>`
- GitHub Release URL: `https://github.com/houseme/rustfs-mimalloc/releases/tag/v<VERSION>`
- whether CI was awaited or intentionally not awaited

## Rollback

If the release is published but broken:

```bash
cargo yank --vers <VERSION> -p rustfs-mimalloc
cargo yank --vers <VERSION> -p rustfs-mimalloc-sys
```

Yanking prevents new dependency resolution to that version, but it does not delete already downloaded crates.

Delete or edit the GitHub Release manually if the release notes are wrong. Do not delete a pushed tag unless the user explicitly asks for that history change.

## Notes

- Versions follow SemVer: `MAJOR.MINOR.PATCH[-PRERELEASE]`.
- crates.io does not allow publishing the same package version twice.
- The repository must have the `CARGO_REGISTRY_TOKEN` GitHub Actions secret.
- Pre-release versions containing `-` are marked as prereleases on GitHub.
- This crate is v3-only and performance-first. Do not reintroduce v2 or `nightly_allocator_api` release paths.
