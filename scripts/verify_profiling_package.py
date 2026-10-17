#!/usr/bin/env python3
"""Test the normalized profiling archive with matching allocator dependencies."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import sys

from test_profiling_providers import reachable_packages


CONFIG = []


def cargo(*args, cwd=None):
    return subprocess.run(["cargo", *CONFIG, *args], cwd=cwd, check=True)


def metadata(manifest=None, all_features=False):
    command = ["cargo", *CONFIG, "metadata", "--format-version", "1"]
    if manifest:
        command += ["--manifest-path", str(manifest)]
        command += ["--all-features"] if all_features else ["--no-default-features"]
    else:
        command.append("--no-deps")
    return json.loads(subprocess.check_output(command, text=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--local-dependencies", action="store_true",
                        help="Use matching workspace dependencies before publication")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    os.chdir(root)
    if args.local_dependencies:
        for name in ("rustfs-mimalloc", "rustfs-mimalloc-sys"):
            CONFIG.extend(["--config", f'patch.crates-io.{name}.path="{(root / name).as_posix()}"'])
    workspace = metadata()
    package = next(p for p in workspace["packages"] if p["name"] == "rustfs-mimalloc-profiling")
    command = ["package", "--locked", "-p", package["name"]]
    if args.allow_dirty:
        command.append("--allow-dirty")
    cargo(*command)
    archive = Path(workspace["target_directory"]) / "package" / f'{package["name"]}-{package["version"]}.crate'
    with tempfile.TemporaryDirectory(prefix="profiling-registry-test-") as temporary:
        # Cargo created this archive from our own checked-out package.
        with tarfile.open(archive) as bundle:
            for member in bundle.getmembers():
                path = Path(member.name)
                if (not (member.isfile() or member.isdir()) or not path.parts
                        or ".." in path.parts or path.is_absolute()):
                    raise RuntimeError("unexpected package archive entry")
            bundle.extractall(temporary)
        extracted = Path(temporary) / f'{package["name"]}-{package["version"]}'
        manifest = extracted / "Cargo.toml"
        neutral = metadata(manifest)
        native = {"rustfs-mimalloc", "rustfs-mimalloc-sys", "mimalloc", "libmimalloc-sys"}
        if native & reachable_packages(neutral):
            raise RuntimeError("default profiling archive must not select an allocator")
        if "ruzstd" in reachable_packages(neutral):
            raise RuntimeError("default profiling archive must not enable unused ELF decompression")
        resolved = metadata(manifest, all_features=True)
        allocators = [p for p in resolved["packages"] if p["name"] in {"rustfs-mimalloc", "rustfs-mimalloc-sys"}]
        if len(allocators) != 2 or any(p["version"] != package["version"] for p in allocators):
            raise RuntimeError("package must use matching allocator/sys versions")
        if not args.local_dependencies and any(not (p.get("source") or "").startswith("registry+") for p in allocators):
            raise RuntimeError("package verification must use registry allocator dependencies")
        print("Allocator versions:", {p["name"]: p["version"] for p in allocators},
              "(local patches)" if args.local_dependencies else "(registry)", flush=True)
        cargo("test", "--locked", "--no-default-features", "--manifest-path", str(manifest), cwd=extracted)
        cargo("test", "--locked", "--all-features", "--manifest-path", str(manifest), cwd=extracted)
        cargo("test", "--locked", "--release", "--test", "mimalloc_release", "--manifest-path", str(manifest), cwd=extracted)
        provider_command = [sys.executable, str(root / "scripts/test_profiling_providers.py"),
                            "--profiling-dir", str(extracted),
                            "--output", str(Path(workspace["target_directory"]) / "package-provider-tests")]
        if args.local_dependencies:
            provider_command.append("--local-dependencies")
        subprocess.run(provider_command, check=True)


if __name__ == "__main__":
    main()
