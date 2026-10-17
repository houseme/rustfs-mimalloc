#!/usr/bin/env python3
"""Run shared profiling contracts with separately resolved allocator providers.

Requires Python 3.11+. Dependency requirements come from workspace.dependencies.
Each generated consumer is its own Cargo workspace; features cannot leak from
another provider or the optional RustFS compatibility API into its dependency graph.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parents[1]
PROVIDERS = {"rustfs": "rustfs-mimalloc", "upstream": "mimalloc"}


def quoted(value):
    return json.dumps(value, ensure_ascii=False)


def requirement(dependencies, name):
    value = dependencies[name]
    return value if isinstance(value, str) else value["version"]


def command(config, *args, capture=False, env=None):
    argv = ["cargo", *config, *args]
    if capture:
        return json.loads(subprocess.check_output(argv, text=True, env=env))
    subprocess.run(argv, check=True, env=env)


def reachable_packages(metadata):
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    seen, pending = set(), [metadata["resolve"]["root"]]
    while pending:
        identity = pending.pop()
        if identity in seen:
            continue
        seen.add(identity)
        pending.extend(dep["pkg"] for dep in nodes[identity]["deps"])
    return {p["name"] for p in metadata["packages"] if p["id"] in seen}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", choices=[*PROVIDERS, "all"], default="all")
    parser.add_argument("--profiling-dir", type=Path,
                        default=ROOT / "rustfs-mimalloc-profiling")
    parser.add_argument("--local-dependencies", action="store_true")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--output", type=Path, default=ROOT / "target/provider-tests")
    args = parser.parse_args()
    dependencies = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["dependencies"]
    profiling = args.profiling_dir.resolve()
    config = []
    if args.local_dependencies:
        for name in ("rustfs-mimalloc", "rustfs-mimalloc-sys"):
            config.extend(["--config", f"patch.crates-io.{name}.path={quoted((ROOT / name).as_posix())}"])
    environment = os.environ.copy()
    environment.setdefault("CARGO_TARGET_DIR", str(ROOT / "target/provider-build"))
    evidence = {}
    for provider in PROVIDERS if args.provider == "all" else [args.provider]:
        name = PROVIDERS[provider]
        consumer = args.output.resolve() / provider
        consumer.mkdir(parents=True, exist_ok=True)
        provider_dependency = f"version = {quoted(requirement(dependencies, name))}"
        if provider == "rustfs" and args.local_dependencies:
            provider_dependency += f", path = {quoted((ROOT / name).as_posix())}"
        manifest = consumer / "Cargo.toml"
        manifest.write_text(f'''[package]
name = "profiling-provider-{provider}"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
rustfs-mimalloc-profiling = {{ path = {quoted(profiling.as_posix())}, default-features = false }}
{name} = {{ {provider_dependency} }}
prost = {quoted(requirement(dependencies, "prost"))}
libflate = {quoted(requirement(dependencies, "libflate"))}

[[test]]
name = "profiling"
path = "profiling.rs"

[[test]]
name = "mimalloc_release"
path = "mimalloc_release.rs"
''', encoding="utf-8")
        crate = name.replace("-", "_")
        for suite in ("profiling", "mimalloc_release"):
            shared = profiling / f"tests/support/{suite}.rs"
            (consumer / f"{suite}.rs").write_text(f'''use rustfs_mimalloc_profiling::SamplingAllocator;

#[global_allocator]
static ALLOC: SamplingAllocator<{crate}::MiMalloc> =
    SamplingAllocator::with_allocator({crate}::MiMalloc);

#[path = {quoted(shared.as_posix())}]
mod tests;
''', encoding="utf-8")
        # Seed each isolated consumer from the reviewed workspace resolutions.
        lock = consumer / "Cargo.lock"
        if not lock.exists():
            lock.write_bytes((ROOT / "Cargo.lock").read_bytes())
        metadata = command(config, "metadata", "--format-version", "1",
                           "--manifest-path", str(manifest), capture=True, env=environment)
        selected = reachable_packages(metadata)
        expected = {"rustfs-mimalloc", "rustfs-mimalloc-sys"} if provider == "rustfs" else {"mimalloc", "libmimalloc-sys"}
        forbidden = {"mimalloc", "libmimalloc-sys", "rustfs-mimalloc", "rustfs-mimalloc-sys"} - expected
        if not expected <= selected or forbidden & selected:
            raise RuntimeError(f"{provider}: unexpected allocator graph: {sorted(selected & (expected | forbidden))}")
        evidence[provider] = sorted(selected)
        args_test = ["test", "--locked", "--manifest-path", str(manifest)]
        if args.release:
            args_test.extend(["--release", "--test", "mimalloc_release"])
        # A process-wide sampler also observes allocations in libtest workers.
        # Serialize test cases; the contracts exercise explicit worker concurrency.
        args_test.extend(["--", "--test-threads=1"])
        command(config, *args_test, env=environment)
        print(f"{provider}: shared contracts PASS; only {sorted(expected)} selected", flush=True)
        (args.output.resolve() / "dependency-graphs.json").write_text(json.dumps(evidence, indent=2) + "\n")


if __name__ == "__main__":
    main()
