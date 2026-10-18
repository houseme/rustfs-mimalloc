#!/usr/bin/env python3
"""Negative controls for the ABI gate; mutations never touch workspace files."""

import unittest

from check_upstream_abi import BINDINGS, INCLUDE, compile_probe, generate
from check_upstream_api import uncomment, validate_options


class AbiControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.bindings = BINDINGS.read_text(encoding="utf-8")

    def check_rejected(self, bindings, diagnostic):
        self.assertNotEqual(bindings, self.bindings)
        probe, _, _ = generate(bindings)
        result = compile_probe(probe, ["cc"])
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(diagnostic, result.stderr)

    def test_matching_bindings_compile(self):
        probe, _, _ = generate(self.bindings)
        result = compile_probe(probe, ["cc"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_wrong_return_type_is_rejected(self):
        self.check_rejected(self.bindings.replace(
            "pub fn mi_malloc(size: size_t) -> *mut c_void;",
            "pub fn mi_malloc(size: size_t) -> c_int;", 1,
        ), "check_mi_malloc")

    def test_callback_constness_drift_is_rejected(self):
        self.check_rejected(self.bindings.replace(
            'fn(msg: *const c_char, arg: *mut c_void);',
            'fn(msg: *mut c_char, arg: *mut c_void);', 1,
        ), "check_mi_output_fun")

    def test_callback_table_field_reordering_is_rejected(self):
        self.check_rejected(self.bindings.replace(
            "pub on_alloc: Option<mi_profiler_on_alloc_fun>,\n    pub on_free: Option<mi_profiler_on_free_fun>,",
            "pub on_free: Option<mi_profiler_on_free_fun>,\n    pub on_alloc: Option<mi_profiler_on_alloc_fun>,", 1,
        ), "mi_profiler_t.on_")

    def test_changed_size_alias_is_not_silently_projected_as_size_t(self):
        with self.assertRaisesRegex(ValueError, "unsupported ABI alias: size_t"):
            generate(self.bindings.replace("pub type size_t = usize;", "pub type size_t = u32;", 1))

    def test_renamed_option_preserves_the_legacy_variant(self):
        options = validate_options(uncomment((INCLUDE / "mimalloc.h").read_text()), uncomment(self.bindings))
        self.assertEqual(options["mi_option_guarded_precise"], 31)

    def test_renamed_option_alias_cannot_hide_numeric_drift(self):
        wrong = self.bindings.replace("mi_option_guarded_precise = 31", "mi_option_guarded_precise = 32", 1)
        with self.assertRaisesRegex(SystemExit, "option alias ID drift"):
            validate_options(uncomment((INCLUDE / "mimalloc.h").read_text()), uncomment(wrong))


if __name__ == "__main__":
    unittest.main()
