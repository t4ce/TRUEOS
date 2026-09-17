#!/usr/bin/env python3
"""Offline tests of the validation gate, not of the Rust crate or hardware."""
import copy
import json
import tomllib
import unittest

from check_x86_64_next import FEATURES, GIT, REV, check_resolution, probe_manifest, read_contract, ROOT


class ValidationGateTests(unittest.TestCase):
    def metadata(self):
        return {
            "packages": [{"name": "x86_64", "version": "0.15.5", "id": "test-id",
                          "source": f"git+{GIT}?rev={REV}#{REV}"}],
            "resolve": {"nodes": [{"id": "test-id", "features": sorted(FEATURES)}]},
        }

    def test_reviewed_source_accepted(self):
        check_resolution(self.metadata())

    def test_registry_fallback_rejected(self):
        data = self.metadata()
        data["packages"][0]["source"] = "registry+https://github.com/rust-lang/crates.io-index"
        with self.assertRaises(RuntimeError):
            check_resolution(data)

    def test_wrong_revision_rejected(self):
        data = self.metadata()
        data["packages"][0]["source"] = f"git+{GIT}?rev={REV}#{'0' * 40}"
        with self.assertRaises(RuntimeError):
            check_resolution(data)

    def test_duplicates_rejected(self):
        data = self.metadata()
        data["packages"].append(copy.deepcopy(data["packages"][0]))
        with self.assertRaises(RuntimeError):
            check_resolution(data)

    def test_missing_feature_rejected(self):
        data = self.metadata()
        data["resolve"]["nodes"][0]["features"].remove("memory_encryption")
        with self.assertRaises(RuntimeError):
            check_resolution(data)

    def test_missing_package_rejected(self):
        data = self.metadata()
        data["packages"] = []
        with self.assertRaises(RuntimeError):
            check_resolution(data)

    def test_generated_probe_uses_root_contract(self):
        dep, patch, _ = read_contract(ROOT)
        generated = tomllib.loads(probe_manifest(dep, patch))
        self.assertEqual(generated["dependencies"]["x86_64"], dep)
        self.assertEqual(generated["patch"]["crates-io"]["x86_64"], patch)
        self.assertFalse(generated["package"]["publish"])
        self.assertIn("workspace", generated)
        # Metadata evidence must be serializable without any live connections.
        json.dumps(self.metadata())


if __name__ == "__main__":
    unittest.main()
