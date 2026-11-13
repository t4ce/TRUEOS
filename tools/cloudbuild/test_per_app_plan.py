"""Exact app-set configuration contracts, independent of Rust/toolchain access."""

from __future__ import annotations

import copy
import itertools
import json
import subprocess
import unittest

import build_plan


class PerAppPlanTests(unittest.TestCase):
    COMMIT = "1" * 40

    def setUp(self):
        self.ids = sorted(app["id"] for app in build_plan.catalog()["buildins"])

    def test_catalog_contains_only_the_twelve_existing_buildins(self):
        self.assertEqual(self.ids, sorted([
            "apps", "commander", "hull", "trauma", "ssh", "img", "os", "backup",
            "termdir", "edit", "share", "paper",
        ]))
        for preset in build_plan.catalog()["presets"]:
            plan = build_plan.resolve_request({"preset": preset["id"]}, self.COMMIT)
            self.assertEqual(plan["buildins"], [] if preset["id"] == "kernel-only" else self.ids)

    def test_every_app_subset_is_valid_distinct_and_roundtrips(self):
        # There are 2**12 independent choices. No desktop/appstore dependency
        # may silently put an unchecked application back into a plan.
        hashes = set()
        base = build_plan.resolve_request({}, self.COMMIT)
        for bits in itertools.product((False, True), repeat=len(self.ids)):
            apps = [name for name, enabled in zip(self.ids, bits) if enabled]
            with self.subTest(buildins=apps):
                plan = build_plan.resolve_request({"buildins": apps}, self.COMMIT)
                self.assertEqual(plan["buildins"], apps)
                self.assertEqual(set(plan["features"]) - {"buildins"}, set(base["features"]) - {"buildins"})
                self.assertEqual("buildin-apps" in plan["cargo_features"], bool(apps))
                self.assertEqual(build_plan.validate_manifest(plan, self.COMMIT), plan)
                self.assertNotIn(plan["config_hash"], hashes)
                hashes.add(plan["config_hash"])
        self.assertEqual(len(hashes), 4096)

    def test_app_order_does_not_change_configuration_identity(self):
        first = build_plan.resolve_request({"buildins": ["edit", "termdir"]}, self.COMMIT)
        second = build_plan.resolve_request({"buildins": ["termdir", "edit"]}, self.COMMIT)
        self.assertEqual(first, second)
        same_kernel = build_plan.resolve_request({"buildins": ["apps", "hull"]}, self.COMMIT)
        self.assertEqual(first["cargo_features"], same_kernel["cargo_features"])
        self.assertNotEqual(first["config_hash"], same_kernel["config_hash"])

    def test_exact_lists_override_preset_payload_without_changing_subsystems(self):
        for preset in ("universal", "intel-desktop", "qemu-virgl", "kernel-only"):
            original = build_plan.resolve_request({"preset": preset}, self.COMMIT)
            for apps in ([], ["edit"], ["apps", "hull"], self.ids):
                with self.subTest(preset=preset, apps=apps):
                    plan = build_plan.resolve_request({"preset": preset, "buildins": apps,
                                                       "features": {"usb": False}}, self.COMMIT)
                    self.assertEqual(plan["buildins"], apps)
                    self.assertEqual(set(plan["features"]) - {"buildins"},
                                     set(original["features"]) - {"buildins", "usb"})
                    self.assertEqual(build_plan.validate_manifest(plan), plan)

    def test_existing_all_none_shorthand_has_the_same_identity(self):
        for enabled, apps in ((False, []), (True, self.ids)):
            exact = build_plan.resolve_request({"buildins": apps}, self.COMMIT)
            shorthand = build_plan.resolve_request({"features": {"buildins": enabled}}, self.COMMIT)
            self.assertEqual(exact, shorthand)
        bare = build_plan.resolve_request({"preset": "kernel-only"}, self.COMMIT)
        self.assertEqual(bare, build_plan.resolve_request({"buildins": []}, self.COMMIT))
        build_plan.resolve_request({"features": {"buildins": True}, "buildins": ["edit"]}, self.COMMIT)
        for request in ({"features": {"buildins": False}, "buildins": ["edit"]},
                        {"features": {"buildins": True}, "buildins": []}):
            with self.assertRaisesRegex(ValueError, "conflicts"):
                build_plan.resolve_request(request, self.COMMIT)

    def test_invalid_app_selections_are_rejected_before_any_build(self):
        invalid = [None, True, False, "all", "edit", {}, 12, ["edit", "edit"],
                   ["all"], ["none"], ["unknown"], ["../hull"], ["edit,apps"],
                   ["$(touch bad)"], ["--help"], [1], [[]], [None], [True]]
        for apps in invalid:
            with self.subTest(apps=apps), self.assertRaises(ValueError):
                build_plan.resolve_request({"buildins": apps}, self.COMMIT)

    def test_manifest_cannot_swap_apps_hide_them_or_use_old_schema(self):
        original = build_plan.resolve_request({"buildins": ["apps", "edit"]}, self.COMMIT)
        for value in (["apps", "hull"], [], ["edit", "apps"], ["apps", "apps"], ["unknown"]):
            changed = copy.deepcopy(original)
            changed["buildins"] = value
            with self.subTest(buildins=value), self.assertRaises(ValueError):
                build_plan.validate_manifest(changed)
        changed = dict(original, schema=1)
        with self.assertRaisesRegex(ValueError, "schema"):
            build_plan.validate_manifest(changed)
        del changed["buildins"]
        with self.assertRaisesRegex(ValueError, "incomplete"):
            build_plan.validate_manifest(changed)

    def test_local_selection_shorthand_has_no_extra_flag_or_path_escape(self):
        self.assertEqual(build_plan.buildins_from_cli("all"), self.ids)
        self.assertEqual(build_plan.buildins_from_cli("none"), [])
        self.assertEqual(build_plan.buildins_from_cli("termdir,edit"), ["edit", "termdir"])
        for text in ("", "edit,", ",edit", "edit,edit", "../edit", "edit --flag", "all,edit"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                build_plan.buildins_from_cli(text)

    def test_cli_emits_exact_list_for_prepare_and_normalized_manifest(self):
        command = ["python3", str(build_plan.ROOT / "tools/cloudbuild/build_plan.py")]
        for choice, expected in (("all", self.ids), ("none", []), ("termdir,edit", ["edit", "termdir"])):
            result = subprocess.run(command + ["--buildins", choice, "--profile", "dev"],
                                    check=True, capture_output=True, text=True, timeout=15)
            plan = json.loads(result.stdout)
            self.assertEqual(plan["buildins"], expected)
            self.assertEqual(plan["profile"], "dev")
            result = subprocess.run(command + ["--buildins", choice, "--buildins-list"],
                                    check=True, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.stdout, ",".join(expected) + "\n")


if __name__ == "__main__":
    unittest.main()
