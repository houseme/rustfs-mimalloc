#!/usr/bin/env python3
"""Test the normalized profiling archive with published allocator dependencies."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile


def cargo(*args, cwd=None):
    return subprocess.run(["cargo", *args], cwd=cwd, check=True)


def metadata(manifest=None):
    command = ["cargo", "metadata", "--format-version", "1"]
    if manifest:
        command += ["--manifest-path", str(manifest)]
    else:
        command.append("--no-deps")
    return json.loads(subprocess.check_output(command, text=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-dirty", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    os.chdir(root)
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
        resolved = metadata(manifest)
        allocators = [p for p in resolved["packages"] if p["name"] in {"rustfs-mimalloc", "rustfs-mimalloc-sys"}]
        if len(allocators) != 2 or any(not (p.get("source") or "").startswith("registry+") for p in allocators):
            raise RuntimeError("package verification must use registry allocator dependencies")
        print("Registry allocator versions:", {p["name"]: p["version"] for p in allocators}, flush=True)
        cargo("test", "--locked", "--all-features", "--manifest-path", str(manifest), cwd=extracted)
        cargo("test", "--locked", "--release", "--test", "mimalloc_release", "--manifest-path", str(manifest), cwd=extracted)


if __name__ == "__main__":
    main()
