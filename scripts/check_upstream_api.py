#!/usr/bin/env python3
"""Audit binding names and option IDs against the pinned public C headers.

This is a source parity check, not a C ABI or linker check. Integration tests
exercise allocation symbols and profiler callbacks separately.
"""

import argparse
import json
from pathlib import Path
import re


def uncomment(text):
    return re.sub(r"//[^\n]*|/\*.*?\*/", "", text, flags=re.DOTALL)


def option_ids(header):
    body = re.search(r"typedef enum mi_option_e\s*\{(.*?)\}", header, re.DOTALL)[1]
    values = {}
    next_value = 0
    for entry in body.split(","):
        name, *expression = entry.strip().split("=", 1)
        if not name:
            continue
        if expression:
            value = expression[0].strip()
            next_value = values[value] if value in values else int(value, 0)
        values[name.strip()] = next_value
        next_value += 1
    return values


def validate_options(headers, bindings):
    c_options = option_ids(headers)
    rust_enum = re.search(r"pub enum mi_option_t\s*\{(.*?)\}", bindings, re.DOTALL)[1]
    rust_options = {name: int(value) for name, value in re.findall(r"(mi_option_\w+)\s*=\s*(\d+)", rust_enum)}
    aliases = dict(re.findall(r"pub const (mi_option_\w+): Self = Self::(mi_option_\w+);", bindings))
    for alias, target in aliases.items():
        if target not in rust_options:
            raise SystemExit(f"unknown option alias target: {alias} -> {target}")
        if alias in c_options and c_options[alias] != rust_options[target]:
            raise SystemExit(f"option alias ID drift: {alias}")
    for name, value in rust_options.items():
        if name in c_options and c_options[name] != value:
            raise SystemExit(f"option ID drift: {name}: Rust={value}, C={c_options[name]}")
        candidates = [name] + [alias for alias, target in aliases.items() if target == name]
        if not any(c_options.get(candidate) == value for candidate in candidates):
            raise SystemExit(f"option ID drift: {name}: Rust={value}, C={c_options.get(name)}")
    active = {name for name in c_options if name.startswith("mi_option_") and "deprecated" not in name}
    unbound = sorted(name for name in active - rust_options.keys() - aliases.keys()
                     if c_options[name] not in rust_options.values())
    if unbound:
        raise SystemExit("unbound active options: " + ", ".join(unbound))
    return rust_options


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", type=Path, help="save the API inventory")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    include = root / "rustfs-mimalloc-sys/c_src/mimalloc/include"
    headers = uncomment("\n".join((include / name).read_text(encoding="utf-8") for name in [
        "mimalloc.h", "mimalloc-profile.h", "mimalloc-stats.h", "mimalloc/qsbr.h",
    ]))
    bindings = uncomment((root / "rustfs-mimalloc-sys/src/lib.rs").read_text(encoding="utf-8"))
    bound = set(re.findall(r"pub (?:unsafe )?(?:const )?fn (mi_\w+)\(", bindings))
    header_names = set(re.findall(r"\b(mi_\w+)\s*\(", headers))
    missing = sorted(bound - header_names)
    if missing:
        raise SystemExit("bindings absent from public headers: " + ", ".join(missing))

    rust_options = validate_options(headers, bindings)

    exported = set()
    for declaration in re.findall(r"\bmi_decl_export\b([^;{}]+);", headers):
        names = re.findall(r"\b(mi_\w+)\s*\(", declaration)
        exported.update(name for name in names if not name.startswith(("mi_attr_", "mi_decl_")))
    report = {
        "source_version": int(re.search(r"#define\s+MI_MALLOC_VERSION\s+(\d+)", headers)[1]),
        "bound_functions": sorted(bound),
        "unbound_exports": sorted(exported - bound),
        "option_ids": rust_options,
        "scope": "names and numeric option IDs; ABI and link behavior require runtime tests",
    }
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Public header parity passed: {len(bound)} Rust functions, {len(rust_options)} option IDs; "
          f"{len(report['unbound_exports'])} C exports remain outside the binding scope")


if __name__ == "__main__":
    main()
