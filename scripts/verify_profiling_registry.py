#!/usr/bin/env python3
"""Verify official index visibility and the uploaded profiling crate checksum."""
import argparse
import hashlib
import json
from pathlib import Path
import time
import urllib.error
import urllib.request

NAME = "rustfs-mimalloc-profiling"
INDEX = f"https://index.crates.io/ru/st/{NAME}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    expected = hashlib.sha256(args.archive.read_bytes()).hexdigest() if args.archive else None
    for attempt in range(40):
        try:
            request = urllib.request.Request(INDEX, headers={"User-Agent": "rustfs-mimalloc-release-verification", "Cache-Control": "no-cache"})
            with urllib.request.urlopen(request, timeout=15) as response:
                records = [json.loads(line) for line in response.read().decode().splitlines()]
            release = next((r for r in records if r["vers"] == args.version), None)
            if release and not release["yanked"]:
                url = f"https://static.crates.io/crates/{NAME}/{NAME}-{args.version}.crate"
                with urllib.request.urlopen(url, timeout=30) as response:
                    checksum = hashlib.sha256(response.read()).hexdigest()
                if checksum != release["cksum"] or (expected and checksum != expected):
                    raise RuntimeError("published archive checksum does not match")
                print(f"Verified {NAME} {args.version}: official index and archive checksum match")
                return
        except (urllib.error.URLError, TimeoutError) as error:
            if attempt == 39:
                raise RuntimeError("official registry verification failed") from error
        time.sleep(5)
    raise RuntimeError("requested version is not visible in the official registry")


if __name__ == "__main__":
    main()
