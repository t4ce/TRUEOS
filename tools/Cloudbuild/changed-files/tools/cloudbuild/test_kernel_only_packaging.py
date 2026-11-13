"""Exercise real preparation control flow with sentinel host tools, not an OS build."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest

import build_plan


class KernelOnlyPreparationTests(unittest.TestCase):
    COMMIT = "1" * 40

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="trueos-kernel-only-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.root = self.directory / "TRUEOS"
        for relative in ("tools/trueos-boot/prepare.sh", "tools/cloudbuild/build_plan.py", "tools/cloudbuild/catalog.json"):
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(build_plan.ROOT / relative, destination)
        (self.root / "Cargo.lock").write_text("version = 4\n")
        self.calls = self.directory / "calls.jsonl"
        bindir = self.directory / "bin"
        bindir.mkdir()
        (bindir / "git").write_text(f"#!/bin/sh\nprintf '%s\\n' '{self.COMMIT}'\n")
        (bindir / "cargo").write_text(
            "#!/usr/bin/env python3\nimport json, os, sys\n"
            "with open(os.environ['TRUEOS_TEST_CALLS'], 'a') as log:\n"
            "    log.write(json.dumps(['cargo', *sys.argv[1:]]) + '\\n')\n"
            "sys.exit(81)  # Stop before any real build, loader or publication.\n"
        )
        for tool in bindir.iterdir():
            tool.chmod(0o755)
        self.environment = dict(os.environ, PATH=str(bindir) + os.pathsep + os.environ["PATH"],
                                TRUEOS_TEST_CALLS=str(self.calls))
        for name in ("TRUEOS_CLOUDBUILD_SPEC", "TRUEOS_BUILDINS_SOURCE", "TRUEOS_BUILDINS_DIR",
                     "TRUEOS_BUILDINS_SELECTION", "TRUEOS_BUILDINS_NATIVE_ONLY", "CARGO_TARGET_DIR"):
            self.environment.pop(name, None)
        self.api = self.root / "tools/build_api_prm.py"
        self.api.write_text(self.record_step("api-reference"))

    @staticmethod
    def record_step(name):
        return ("import json, os, sys\n"
                "with open(os.environ['TRUEOS_TEST_CALLS'], 'a') as log:\n"
                f"    log.write(json.dumps([{name!r}, *sys.argv[1:]]) + '\\n')\n")

    def provide_buildins(self):
        builder = self.directory / "TRUEOS-Buildins/tools/build.py"
        builder.parent.mkdir(parents=True, exist_ok=True)
        # The separate adapter tests execute the real selected-set builder.
        # This fixture stops at its argument boundary, then at sentinel Cargo.
        builder.write_text("# sibling source fixture\n")
        adapter = self.root / "tools/cloudbuild/buildins.py"
        adapter.write_text(self.record_step("buildins"))

    def run_prepare(self, *arguments, manifest=None):
        environment = self.environment.copy()
        if manifest is not None:
            spec = self.directory / "manifest.json"
            spec.write_text(json.dumps(manifest))
            environment["TRUEOS_CLOUDBUILD_SPEC"] = str(spec)
        if self.calls.exists():
            self.calls.unlink()
        result = subprocess.run(
            ["bash", str(self.root / "tools/trueos-boot/prepare.sh"), *arguments],
            env=environment, capture_output=True, text=True, timeout=15,
        )
        calls = [json.loads(line) for line in self.calls.read_text().splitlines()] if self.calls.exists() else []
        return result, calls

    def test_direct_option_skips_app_steps_and_preserves_other_defaults_in_both_profiles(self):
        defaults = set(tomllib.loads((build_plan.ROOT / "Cargo.toml").read_text())["features"]["default"])
        expected = sorted(defaults - {"buildin-apps"})
        for arguments, profile_arguments in (
            (("--no-buildins",), ["--release"]),
            (("--release", "--no-buildins"), ["--release"]),
            (("--no-buildins", "--dev"), []),
            (("--buildins", "none"), ["--release"]),
        ):
            with self.subTest(arguments=arguments):
                result, calls = self.run_prepare(*arguments)
                self.assertEqual(result.returncode, 81, result.stderr)
                self.assertEqual(calls, [["cargo", "build", *profile_arguments,
                                          "--no-default-features", "--features", ",".join(expected)]])
                self.assertIn("skipping buildin compilation", result.stdout)
        # No sibling Buildins tree exists in this fixture.
        self.assertFalse((self.directory / "TRUEOS-Buildins").exists())

    def test_cloud_option_stacks_with_subsystem_deselection_without_app_validation(self):
        manifest = build_plan.resolve_request({"preset": "qemu-virgl", "features": {"buildins": False, "usb": False}}, self.COMMIT)
        result, calls = self.run_prepare("--release", manifest=manifest)
        self.assertEqual(result.returncode, 81, result.stderr)
        self.assertEqual(calls, [["cargo", "build", "--release", *build_plan.cargo_args(manifest)]])
        self.assertNotIn("buildin-apps", ",".join(calls[0]))

    def test_no_buildins_flag_cannot_override_a_manifest_that_includes_apps(self):
        manifest = build_plan.resolve_request({}, self.COMMIT)
        result, calls = self.run_prepare("--no-buildins", manifest=manifest)
        self.assertEqual(result.returncode, 2)
        self.assertIn("conflicts with the supplied build manifest", result.stderr)
        self.assertEqual(calls, [])

    def test_default_and_reenabled_builds_use_the_adapter(self):
        self.provide_buildins()
        result, _ = self.run_prepare("--no-buildins")
        self.assertEqual(result.returncode, 81, result.stderr)
        result, calls = self.run_prepare()
        self.assertEqual(result.returncode, 81, result.stderr)
        self.assertEqual([call[0] for call in calls], ["buildins", "cargo"])
        self.assertEqual(calls[-1], ["cargo", "build", "--release"])

    def test_all_features_disabled_has_no_empty_cargo_feature_argument(self):
        selections = dict.fromkeys((item["id"] for item in build_plan.catalog()["features"]), False)
        manifest = build_plan.resolve_request({"features": selections}, self.COMMIT)
        result, calls = self.run_prepare("--no-buildins", manifest=manifest)
        self.assertEqual(result.returncode, 81, result.stderr)
        self.assertEqual(calls, [["cargo", "build", "--release", "--no-default-features"]])

    def test_stale_buildin_cache_is_not_read_or_checked(self):
        stale = self.root / "tgt/buildins/release"
        stale.mkdir(parents=True)
        (stale / "buildins.json").write_text("deliberately invalid old manifest")
        (stale / "app.db.lz4").write_bytes(b"old app payload")
        self.environment["TRUEOS_BUILDINS_SOURCE"] = str(self.directory / "missing-buildins-source")
        result, calls = self.run_prepare("--no-buildins")
        self.assertEqual(result.returncode, 81, result.stderr)
        self.assertEqual([call[0] for call in calls], ["cargo"])

    def test_individual_apps_are_forwarded_without_unselected_editor_reference(self):
        self.provide_buildins()
        for selected, steps in (("commander", ["buildins", "cargo"]),
                                ("edit,commander", ["buildins", "cargo"])):
            with self.subTest(selected=selected):
                result, calls = self.run_prepare("--buildins", selected)
                self.assertEqual(result.returncode, 81, result.stderr)
                self.assertEqual([call[0] for call in calls], steps)
                invocation = next(call for call in calls if call[0] == "buildins")
                self.assertEqual(invocation[invocation.index("--selection") + 1], ",".join(sorted(selected.split(","))))
                self.assertIn("buildin-apps", calls[-1][-1].split(","))

    def test_cloud_selection_is_exact_and_cannot_be_overridden(self):
        self.provide_buildins()
        manifest = build_plan.resolve_request({"preset": "qemu-virgl", "buildins": ["apps", "edit"]}, self.COMMIT)
        result, calls = self.run_prepare("--buildins", "edit,apps", manifest=manifest)
        self.assertEqual(result.returncode, 81, result.stderr)
        self.assertEqual(calls[-1], ["cargo", "build", "--release", *build_plan.cargo_args(manifest)])
        invocation = next(call for call in calls if call[0] == "buildins")
        self.assertEqual(invocation[invocation.index("--selection") + 1], "apps,edit")
        for selection in ("all", "none", "edit"):
            result, calls = self.run_prepare("--buildins", selection, manifest=manifest)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn("conflicts with the supplied build manifest", result.stderr)
            self.assertEqual(calls, [])

    def test_invalid_or_ambiguous_cli_selection_stops_before_compilation(self):
        for arguments in (("--buildins", "not-an-app"), ("--buildins", "edit,edit"),
                          ("--buildins",), ("--no-buildins", "--buildins", "edit"),
                          ("--native-only", "--native-only")):
            with self.subTest(arguments=arguments):
                result, calls = self.run_prepare(*arguments)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(calls, [])

    def test_native_mode_keeps_exact_manifest_features_and_app_selection(self):
        self.provide_buildins()
        manifest = build_plan.resolve_request({"preset": "qemu-virgl", "buildins": ["ssh"]}, self.COMMIT)
        result, calls = self.run_prepare("--native-only", manifest=manifest)
        self.assertEqual(result.returncode, 81, result.stderr)
        invocation = calls[0]
        self.assertEqual(invocation[0], "buildins")
        self.assertEqual(invocation[invocation.index("--selection") + 1], "ssh")
        self.assertEqual(invocation[-1], "--native-only")
        self.assertEqual(calls[-1][:-3], ["cargo", "rustc", "--bin", "TRUEOS", "--release", *build_plan.cargo_args(manifest)])
        self.assertEqual(calls[-1][-3:-1], ["--", "-C"])
        self.assertRegex(calls[-1][-1], r"link-arg=-Map=.*/tgt/native-only\.[^/]+/kernel\.map$")

    def test_native_no_app_mode_never_opens_sibling_sources(self):
        result, calls = self.run_prepare("--native-only", "--no-buildins")
        self.assertEqual(result.returncode, 81, result.stderr)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][:5], ["cargo", "rustc", "--bin", "TRUEOS", "--release"])
        self.assertNotIn("buildin-apps", ",".join(calls[0]))
        self.assertFalse((self.directory / "TRUEOS-Buildins").exists())


@unittest.skipUnless(shutil.which("rustc"), "host rustc is required to execute the build-script seed generator")
class BuildScriptSeedTests(unittest.TestCase):
    """Compile the exact std-only seed generator from build.rs without Cargo."""

    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="trueos-seed-generator-")
        cls.directory = Path(cls.temporary.name)
        # These two production functions are std-only; the rest of build.rs
        # needs the OS checkout and unrelated build dependencies.
        production = (build_plan.ROOT / "build.rs").read_text()
        functions = "fn manifest_names(" + production.split("fn manifest_names(", 1)[1]
        harness = cls.directory / "seed.rs"
        harness.write_text("use std::{env, fs, path::PathBuf};\n" + functions +
                           "\nfn main() { generate_app_buildins(); }\n")
        cls.binary = cls.directory / "seed"
        subprocess.run([shutil.which("rustc"), "--edition=2021", str(harness), "-o", str(cls.binary)],
                       cwd=cls.directory, check=True, capture_output=True, timeout=30)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(dir=self.directory)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.out = self.root / "out"
        self.out.mkdir()
        self.environment = dict(os.environ, OUT_DIR=str(self.out))
        self.environment.pop("CARGO_FEATURE_BUILDIN_APPS", None)
        self.environment.pop("CARGO_MANIFEST_DIR", None)
        self.environment.pop("TRUEOS_BUILDINS_SELECTION", None)

    def test_no_app_mode_erases_stale_payload_before_any_source_check(self):
        (self.out / "app.db.lz4").write_bytes(b"old bundled application bytes")
        (self.out / "appdb-seed.rs").write_text("const APP_DB_EXPECTED_APPS: usize = 50;\n")
        self.environment["TRUEOS_BUILDINS_SOURCE"] = str(self.root / "missing")
        # CARGO_MANIFEST_DIR is absent: reaching the old validation path would
        # panic. No Python builder is needed for an unseeded kernel.
        result = subprocess.run([str(self.binary)], env=self.environment, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.out / "app.db.lz4").read_bytes(), b"")
        self.assertEqual((self.out / "appdb-seed.rs").read_text(), "const APP_DB_EXPECTED_APPS: usize = 0;\n")

    def test_enabled_mode_still_checks_and_embeds_prepared_apps(self):
        adapter = self.root / "tools/cloudbuild/buildins.py"
        adapter.parent.mkdir(parents=True)
        marker = self.root / "checked"
        adapter.write_text("import pathlib, sys\nassert '--check' in sys.argv\n"
                           "assert sys.argv[sys.argv.index('--selection') + 1] == 'commander'\n" +
                           f"pathlib.Path({str(marker)!r}).write_text('checked')\n")
        prepared = self.root / "prepared"
        prepared.mkdir()
        (prepared / "buildins.json").write_text('{"buildins":["commander"]}')
        (prepared / "app.db").write_bytes(b"fixture raw bytes")
        payload = b"fixture compressed bytes; generator does not decode"
        (prepared / "app.db.lz4").write_bytes(payload)
        self.environment.update(CARGO_MANIFEST_DIR=str(self.root), CARGO_FEATURE_BUILDIN_APPS="1",
                                TRUEOS_BUILDINS_SOURCE=str(self.root / "source"), TRUEOS_BUILDINS_DIR=str(prepared),
                                TRUEOS_BUILDINS_SELECTION="commander")
        result = subprocess.run([str(self.binary)], env=self.environment, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(marker.exists())
        self.assertEqual((self.out / "app.db.lz4").read_bytes(), payload)
        self.assertIn("APP_DB_EXPECTED_APPS: usize = 1;", (self.out / "appdb-seed.rs").read_text())


if __name__ == "__main__":
    unittest.main()
