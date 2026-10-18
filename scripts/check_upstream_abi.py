#!/usr/bin/env python3
"""Compile C projections of the Rust FFI signatures against the pinned headers.

This checks the supported repr(C)/extern-C subset, not arbitrary Rust layouts,
inline Rust helpers, symbol availability or runtime safety. Runtime tests still
exercise linking and callback invocation. The C compiler checks its host ABI.
"""

import argparse
from pathlib import Path
import re
import shlex
import subprocess
import tempfile

from check_upstream_api import uncomment


ROOT = Path(__file__).resolve().parents[1]
INCLUDE = ROOT / "rustfs-mimalloc-sys/c_src/mimalloc/include"
BINDINGS = ROOT / "rustfs-mimalloc-sys/src/lib.rs"
LAYOUTS = ("mi_heap_area_t", "mi_profiler_sample_data_t", "mi_profiler_t")
PRIMITIVES = {
    "c_void": "void", "c_char": "char", "c_int": "int", "c_long": "long",
    "usize": "size_t", "size_t": "size_t", "u64": "uint64_t", "bool": "bool",
}


def c_type(rust):
    rust = rust.strip()
    if rust.startswith("*const "):
        pointee = c_type(rust[7:])
        return pointee + " const*" if pointee.endswith("*") else "const " + pointee + "*"
    if rust.startswith("*mut "):
        return c_type(rust[5:]) + "*"
    optional = re.fullmatch(r"Option<(mi_\w+_fun)>", rust)
    if optional:
        return optional[1] + "*"
    if rust in PRIMITIVES:
        return PRIMITIVES[rust]
    if re.fullmatch(r"mi_\w+_t", rust):
        return rust
    raise ValueError(f"unsupported Rust FFI type: {rust}")


def parameters(rust, *, callback=None):
    result = []
    for parameter in rust.split(","):
        if not parameter.strip():
            continue
        name, kind = (part.strip() for part in parameter.split(":", 1))
        # C spells this fixed-width argument unsigned long long, which can be
        # a different C type from uint64_t. Size/alignment are asserted below;
        # both project to Rust u64. Preserve this spelling for strict C checks.
        if callback == "mi_deferred_free_fun" and name == "heartbeat" and kind == "u64":
            result.append("unsigned long long")
        else:
            result.append(c_type(kind))
    return ", ".join(result) or "void"


def generate(bindings):
    source = uncomment(bindings)
    if any(abi != "C" for abi in re.findall(r'unsafe extern "([^"]+)"', source)):
        raise ValueError("unsupported calling convention")
    for name, expected in {"size_t": "usize", "mi_arena_id_t": "*mut c_void"}.items():
        alias = re.search(rf"pub type {name}\s*=\s*([^;]+);", source)
        if not alias or alias[1].strip() != expected:
            raise ValueError(f"unsupported ABI alias: {name}; expected {expected}")
    lines = [
        '#include "mimalloc.h"', '#include "mimalloc-profile.h"',
        '#include "mimalloc-stats.h"', '#include "mimalloc/qsbr.h"', "#include <stddef.h>",
        '_Static_assert(sizeof(unsigned long long) == sizeof(uint64_t), "heartbeat width");',
        '_Static_assert(_Alignof(unsigned long long) == _Alignof(uint64_t), "heartbeat alignment");',
    ]
    callbacks = re.findall(
        r'pub type (mi_\w+_fun)\s*=\s*unsafe extern "C" fn\((.*?)\)\s*(?:->\s*([^;]+))?;',
        source, re.DOTALL,
    )
    if len(callbacks) != len(re.findall(r'pub type mi_\w+_fun\s*=', source)):
        raise ValueError("unparsed callback declaration")
    for name, args, result in callbacks:
        lines.extend([
            f"typedef {c_type(result) if result else 'void'} (*rust_{name})({parameters(args, callback=name)});",
            f"static void check_{name}({name}* actual) {{ rust_{name} expected = actual; (void)expected; }}",
        ])
    functions = []
    for block in re.findall(r'unsafe extern "C"\s*\{(.*?)\}', source, re.DOTALL):
        parsed = re.findall(
            r"pub fn (mi_\w+)\((.*?)\)\s*(?:->\s*([^;]+))?;", block, re.DOTALL,
        )
        if len(parsed) != len(re.findall(r"\bpub fn\b", block)):
            raise ValueError("unparsed FFI declaration")
        functions.extend(parsed)
    lines.append("static void check_functions(void) {")
    for name, args, result in functions:
        lines.append(f"  {c_type(result) if result else 'void'} (*check_{name})({parameters(args)}) = &{name};")
        lines.append(f"  (void)check_{name};")
    lines.append("}")
    for name in LAYOUTS:
        match = re.search(rf"#\[repr\(C\)\]\s*pub struct {name}\s*\{{(.*?)\}}", source, re.DOTALL)
        if not match:
            raise ValueError(f"missing repr(C) layout: {name}")
        fields = re.findall(r"(?:pub )?(\w+):\s*([^,]+),", match[1])
        if not fields:
            raise ValueError(f"no layout fields parsed: {name}")
        lines.append(f"struct rust_{name} {{")
        for field, kind in fields:
            array = re.fullmatch(r"\[(.*);\s*(\d+)\]", kind.strip())
            if array:
                lines.append(f"  {c_type(array[1])} {field}[{array[2]}];")
            else:
                lines.append(f"  {c_type(kind)} {field};")
        lines.append("};")
        for measure in ("sizeof", "_Alignof"):
            lines.append(f'_Static_assert({measure}(struct rust_{name}) == {measure}({name}), "{name} {measure}");')
        for field, _ in fields:
            lines.append(f'_Static_assert(offsetof(struct rust_{name}, {field}) == offsetof({name}, {field}), "{name}.{field}");')
    if not functions or not callbacks:
        raise ValueError("no FFI declarations parsed")
    return "\n".join(lines) + "\n", len(functions), len(callbacks)


def compile_probe(probe, compiler):
    with tempfile.TemporaryDirectory(prefix="mimalloc-abi-") as temporary:
        path = Path(temporary) / "check.c"
        path.write_text(probe, encoding="utf-8")
        return subprocess.run(
            [*compiler, "-std=c11", "-Werror", "-fsyntax-only", "-I", str(INCLUDE), str(path)],
            text=True, capture_output=True,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cc", default="cc", help="host C compiler command (GCC/Clang)")
    args = parser.parse_args()
    probe, functions, callbacks = generate(BINDINGS.read_text(encoding="utf-8"))
    result = compile_probe(probe, shlex.split(args.cc))
    if result.returncode:
        raise SystemExit(result.stdout + result.stderr)
    print(f"C ABI projection passed: {functions} FFI functions, {callbacks} callbacks, {len(LAYOUTS)} repr(C) layouts")


if __name__ == "__main__":
    main()
