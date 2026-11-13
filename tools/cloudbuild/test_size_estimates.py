"""Keep release measurements honest and outside signed build identity."""

from __future__ import annotations

import copy
import json
from pathlib import Path
import unittest
from unittest import mock

import build_plan


class SizeEstimateTests(unittest.TestCase):
    def setUp(self):
        self.catalog = build_plan.catalog()
        self.estimates = self.catalog["size_estimates"]
        self.evidence = json.loads(
            (Path(__file__).parent / "measurements/0.0.235.json").read_text()
        )

    def test_catalog_uses_exact_measured_packages_for_every_existing_app(self):
        expected = {app["id"]: app for app in self.evidence["buildins"]}
        self.assertEqual(set(expected), {app["id"] for app in self.catalog["buildins"]})
        self.assertEqual(set(self.estimates["buildins"]), set(expected))
        for name, measurements in self.estimates["buildins"].items():
            with self.subTest(buildin=name):
                for field in ("packed_bp_bytes", "elf_bytes"):
                    self.assertIs(type(measurements[field]), int)
                    self.assertGreater(measurements[field], 0)
                    self.assertEqual(measurements[field], expected[name][field])
        self.assertEqual(
            sum(app["packed_bp_bytes"] for app in self.estimates["buildins"].values()),
            self.evidence["sizes"]["all_packed_bp_bytes"],
        )

    def test_kernel_baseline_subtracts_actual_embedded_database(self):
        sizes = self.evidence["sizes"]
        reference = self.estimates["reference"]
        kernel = self.estimates["kernel"]
        self.assertEqual(reference["kernel_elf_bytes"], sizes["kernel_elf_bytes"])
        self.assertEqual(kernel["embedded_appdb_packed_bytes"], sizes["embedded_appdb_packed_bytes"])
        self.assertEqual(
            kernel["base_bytes"],
            reference["kernel_elf_bytes"] - kernel["embedded_appdb_packed_bytes"],
        )
        self.assertEqual(kernel["base_bytes"], sizes["non_appdb_kernel_file_bytes"])
        self.assertEqual(kernel["basis"], "derived")
        self.assertIsNone(sizes["measured_kernel_only_build_bytes"])
        # The package sum is a different encoding from the embedded AppDB.
        # Subtracting it would create a misleading, fitted "kernel" baseline.
        self.assertNotEqual(
            kernel["base_bytes"],
            reference["kernel_elf_bytes"] - sizes["all_packed_bp_bytes"],
        )

    def test_reference_preserves_artifact_identity_without_inventing_provenance(self):
        reference = self.estimates["reference"]
        measured = self.evidence["reference"]
        self.assertEqual(reference["archive_bytes"], self.evidence["sizes"]["release_archive_bytes"])
        for field in ("archive_sha256", "kernel_elf_sha256", "tag_commit"):
            self.assertEqual(reference[field], measured[field])
        self.assertIsNone(reference["source_commit"])
        self.assertIsNone(measured["source_commit_verified_from_archive"])
        self.assertEqual(reference["profile"], "release")
        self.assertIn("user-described", reference["profile_basis"])
        self.assertEqual(self.estimates["subsystems"]["status"], "unmeasured")
        self.assertFalse(any(
            isinstance(value, (int, float))
            for value in self.estimates["subsystems"].values()
        ))

    def test_size_metadata_changes_do_not_change_manifest_or_configuration_hash(self):
        requests = [
            {}, {"preset": "kernel-only"},
            {"buildins": ["edit", "termdir"], "profile": "dev"},
            {"preset": "qemu-virgl", "features": {"usb": False}, "buildins": ["ssh"]},
        ]
        changed = copy.deepcopy(self.catalog)
        changed["size_estimates"]["reference"]["version"] = "future"
        changed["size_estimates"]["kernel"]["base_bytes"] += 100000
        changed["size_estimates"]["buildins"]["ssh"]["packed_bp_bytes"] += 10000
        absent = copy.deepcopy(self.catalog)
        del absent["size_estimates"]
        for request in requests:
            original = build_plan.resolve_request(request, "1" * 40)
            for definition in (changed, absent):
                with self.subTest(request=request, metadata_present="size_estimates" in definition):
                    with mock.patch.object(build_plan, "catalog", return_value=definition):
                        self.assertEqual(build_plan.resolve_request(request, "1" * 40), original)
                        self.assertEqual(build_plan.validate_manifest(original), original)

    def test_size_metadata_is_not_a_request_or_manifest_control(self):
        with self.assertRaisesRegex(ValueError, "unknown request field"):
            build_plan.resolve_request({"size_estimates": self.estimates}, "1" * 40)
        manifest = build_plan.resolve_request({}, "1" * 40)
        manifest["size_estimates"] = self.estimates
        with self.assertRaisesRegex(ValueError, "unknown manifest field"):
            build_plan.validate_manifest(manifest)


if __name__ == "__main__":
    unittest.main()
