"""Host-only contract tests; never compile the OS or start a real build."""

from __future__ import annotations

import copy
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from unittest import mock

import build_plan
import ci


class BuildPlanTests(unittest.TestCase):
    COMMIT = "1" * 40

    def setUp(self):
        self.catalog = build_plan.catalog()
        self.ids = [entry["id"] for entry in self.catalog["features"]]

    def test_universal_preserves_existing_defaults(self):
        cargo = tomllib.loads((build_plan.ROOT / "Cargo.toml").read_text())
        plan = build_plan.resolve_request({}, self.COMMIT)
        self.assertEqual(plan["cargo_features"], sorted(cargo["features"]["default"]))
        for feature in self.catalog["features"]:
            self.assertIn(feature["cargo_feature"], cargo["features"])
        self.assertEqual(
            build_plan.cargo_args(plan),
            ["--no-default-features", "--features", ",".join(plan["cargo_features"])],
        )

    def test_presets_have_expected_effective_subsystems(self):
        expected = {
            "universal": {"buildins", "usb", "igpu", "virgl", "h264", "native_gpgpu", "task_profile"},
            "intel-desktop": {"buildins", "usb", "igpu", "h264", "native_gpgpu", "task_profile"},
            "qemu-virgl": {"buildins", "usb", "virgl", "task_profile"},
            "kernel-only": {"usb", "igpu", "virgl", "h264", "native_gpgpu", "task_profile"},
        }
        for name, selected in expected.items():
            with self.subTest(preset=name):
                plan = build_plan.resolve_request({"preset": name}, self.COMMIT)
                self.assertEqual(set(plan["features"]), selected)
                self.assertEqual(build_plan.validate_manifest(plan, self.COMMIT), plan)

    def test_complete_feature_matrix_and_dependencies(self):
        accepted = rejected = 0
        for values in itertools.product((False, True), repeat=len(self.ids)):
            overrides = dict(zip(self.ids, values))
            needs_missing_intel = (overrides["h264"] or overrides["native_gpgpu"]) and not overrides["igpu"]
            with self.subTest(features=overrides):
                if needs_missing_intel:
                    with self.assertRaisesRegex(ValueError, "requires igpu"):
                        build_plan.resolve_request({"features": overrides}, self.COMMIT)
                    rejected += 1
                else:
                    plan = build_plan.resolve_request({"features": overrides}, self.COMMIT)
                    self.assertEqual(plan["features"], sorted(key for key, value in overrides.items() if value))
                    self.assertEqual(build_plan.validate_manifest(plan, self.COMMIT), plan)
                    accepted += 1
        self.assertEqual((accepted, rejected), (160, 96))

    def test_kernel_only_changes_app_payload_and_hash_without_removing_capabilities(self):
        normal = build_plan.resolve_request({}, self.COMMIT)
        bare = build_plan.resolve_request({"preset": "kernel-only"}, self.COMMIT)
        self.assertEqual(set(bare["features"]), set(normal["features"]) - {"buildins"})
        self.assertEqual(set(bare["cargo_features"]), set(normal["cargo_features"]) - {"buildin-apps"})
        self.assertNotEqual(bare["config_hash"], normal["config_hash"])
        self.assertEqual(bare, build_plan.resolve_request({"features": {"buildins": False}}, self.COMMIT))
        for preset in ("universal", "intel-desktop", "qemu-virgl"):
            request = {"preset": preset, "features": {"buildins": False, "usb": False}}
            plan = build_plan.resolve_request(request, self.COMMIT)
            self.assertNotIn("buildins", plan["features"])
            self.assertNotIn("usb", plan["features"])
            self.assertEqual(build_plan.validate_manifest(plan, self.COMMIT), plan)
        self.assertTrue(any("no bundled file explorer, app store, or text editor" in note for note in bare["warnings"]))

    def test_kernel_only_command_line_reports_no_buildins_in_both_profiles(self):
        for profile in ("release", "dev"):
            result = subprocess.run(
                ["python3", str(build_plan.ROOT / "tools/cloudbuild/build_plan.py"),
                 "--kernel-only", "--profile", profile],
                capture_output=True, text=True, check=True, timeout=15,
            )
            plan = json.loads(result.stdout)
            self.assertEqual(plan["profile"], profile)
            self.assertNotIn("buildins", plan["features"])
            mode = subprocess.run(
                ["python3", str(build_plan.ROOT / "tools/cloudbuild/build_plan.py"),
                 "--kernel-only", "--profile", profile, "--buildins-mode"],
                capture_output=True, text=True, check=True, timeout=15,
            )
            self.assertEqual(mode.stdout.strip(), "none")

    def test_hash_tracks_effective_configuration_source_and_profile(self):
        universal = build_plan.resolve_request({}, self.COMMIT)
        equivalent = build_plan.resolve_request({"preset": "intel-desktop", "features": {"virgl": True}}, self.COMMIT)
        self.assertEqual(universal["config_hash"], equivalent["config_hash"])
        for request, commit in (({}, "2" * 40), ({"profile": "dev"}, self.COMMIT), ({"features": {"usb": False}}, self.COMMIT)):
            with self.subTest(request=request, commit=commit):
                self.assertNotEqual(universal["config_hash"], build_plan.resolve_request(request, commit)["config_hash"])
        first = {"usb": False, "virgl": False}
        second = dict(reversed(list(first.items())))
        self.assertEqual(
            build_plan.resolve_request({"features": first}, self.COMMIT),
            build_plan.resolve_request({"features": second}, self.COMMIT),
        )

    def test_request_rejects_unknown_fields_and_nonboolean_selections(self):
        invalid = [
            [], None, True, {"preset": []}, {"preset": "unknown"}, {"profile": "debug"},
            {"profile": []}, {"features": []}, {"features": {"unknown": True}},
            {"source_commit": self.COMMIT}, {"command": ["sh"]}, {"public_key": "key"},
            {"repository": "example/other"}, {"env": {"RUSTFLAGS": "extra"}},
        ]
        invalid.extend({"features": {"usb": value}} for value in (None, 0, 1, "false", [], {}))
        for request in invalid:
            with self.subTest(request=request), self.assertRaises(ValueError):
                build_plan.resolve_request(request, self.COMMIT)

    def test_manifest_source_binding_and_derived_field_tampering(self):
        plan = build_plan.resolve_request({"preset": "qemu-virgl"}, self.COMMIT)
        with self.assertRaisesRegex(ValueError, "worker checkout"):
            build_plan.validate_manifest(plan, "2" * 40)
        changes = {
            "schema": True,
            "config_hash": "0" * 64,
            "command": ["sh", "-c", "printf untrusted"],
            "cargo_features": ["subsystem-igpu"],
            "features": ["usb", "usb", "virgl"],
            "profile": "dev",
            "warnings": [],
        }
        for field, value in changes.items():
            with self.subTest(field=field), self.assertRaises(ValueError):
                build_plan.validate_manifest(dict(plan, **{field: value}), self.COMMIT)
        altered = dict(plan)
        del altered["command"]
        with self.assertRaises(ValueError):
            build_plan.validate_manifest(altered)
        with self.assertRaises(ValueError):
            build_plan.validate_manifest(dict(plan, unexpected="field"))

    def test_all_disabled_has_no_empty_features_argument(self):
        plan = build_plan.resolve_request({"features": dict.fromkeys(self.ids, False)}, self.COMMIT)
        self.assertEqual(build_plan.cargo_args(plan), ["--no-default-features"])
        self.assertTrue(any("Both graphics backends" in text for text in plan["warnings"]))

    def test_prepare_rejects_invalid_source_before_cargo(self):
        # Use an isolated skeletal checkout and a sentinel Cargo executable:
        # even a regression cannot accidentally start a real kernel build.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "TRUEOS"
            for relative in ("tools/trueos-boot/prepare.sh", "tools/cloudbuild/build_plan.py", "tools/cloudbuild/catalog.json"):
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(build_plan.ROOT / relative, target)
            bindir = Path(temporary) / "bin"
            bindir.mkdir()
            (bindir / "git").write_text(f"#!/bin/sh\nprintf '%s\\n' '{self.COMMIT}'\n")
            (bindir / "cargo").write_text('#!/bin/sh\nprintf called > "$CLOUDBUILD_TEST_CARGO_MARKER"\nexit 81\n')
            for tool in bindir.iterdir():
                tool.chmod(0o755)
            manifest = build_plan.resolve_request({}, "2" * 40)
            spec = Path(temporary) / "manifest.json"
            spec.write_text(json.dumps(manifest))
            marker = Path(temporary) / "cargo-called"
            env = dict(os.environ, PATH=str(bindir) + os.pathsep + os.environ["PATH"],
                       TRUEOS_CLOUDBUILD_SPEC=str(spec), CLOUDBUILD_TEST_CARGO_MARKER=str(marker))
            result = subprocess.run(["bash", str(root / "tools/trueos-boot/prepare.sh"), "--release"],
                                    env=env, capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("source_commit does not match", result.stderr)
            self.assertFalse(marker.exists(), "Cargo must not run before manifest validation")


class ManualCiInputTests(unittest.TestCase):
    COMMIT = "1" * 40

    def setUp(self):
        self.plan = build_plan.resolve_request({"preset": "qemu-virgl"}, self.COMMIT)
        self.environ = {
            "GITHUB_SHA": self.COMMIT,
            "CLOUDBUILD_MANIFEST": json.dumps(self.plan),
            "CLOUDBUILD_BUILDINS_COMMIT": "2" * 40,
            "CLOUDBUILD_BLUEPRINTS_COMMIT": "3" * 40,
            "CLOUDBUILD_TOOLCHAIN_COMMIT": "4" * 40,
        }

    def test_accepts_only_the_dispatched_source_and_immutable_dependencies(self):
        plan, pins = ci.validate_inputs(self.environ, self.COMMIT)
        self.assertEqual(plan, self.plan)
        self.assertEqual(pins, {"buildins": "2" * 40, "blueprints": "3" * 40, "toolchain": "4" * 40})
        with self.assertRaisesRegex(ValueError, "GITHUB_SHA"):
            ci.validate_inputs(self.environ, "5" * 40)
        for key in ("CLOUDBUILD_BUILDINS_COMMIT", "CLOUDBUILD_BLUEPRINTS_COMMIT", "CLOUDBUILD_TOOLCHAIN_COMMIT"):
            for value in ("main", "abcd", "A" * 40, "2" * 40 + "\ninjected=value", "$(echo untrusted)"):
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    ci.validate_inputs(dict(self.environ, **{key: value}), self.COMMIT)

    def test_rejects_untrusted_json_and_oversize_input(self):
        for text in ("", "{}", "[]", "x" * (build_plan.MAX_SPEC_BYTES + 1),
                     '{"schema":1,"schema":1}', '"$(echo untrusted)"'):
            with self.subTest(text=text[:60]), self.assertRaises(ValueError):
                ci.validate_inputs(dict(self.environ, CLOUDBUILD_MANIFEST=text), self.COMMIT)
        altered = copy.deepcopy(self.plan)
        altered["source_commit"] = "5" * 40
        with self.assertRaisesRegex(ValueError, "worker checkout"):
            ci.validate_inputs(dict(self.environ, CLOUDBUILD_MANIFEST=json.dumps(altered)), self.COMMIT)

    def test_artifact_records_actual_revisions_and_checksummed_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "TRUEOS"
            staging = root / "tgt/cloudbuild"
            staging.mkdir(parents=True)
            (staging / "build-manifest.json").write_text(json.dumps(self.plan))
            pins = {"buildins": "2" * 40, "blueprints": "3" * 40, "toolchain": "4" * 40}
            (staging / "dependency-pins.json").write_text(json.dumps(pins))
            (root / "Cargo.lock").write_text("version = 4\n")
            bundle = b"\x04\x22\x4d\x18fixture-only-not-a-boot-image"
            (root / "tgt/rigreal.tar.lz4").write_bytes(bundle)
            prepared = root / "tgt/buildins/release"
            prepared.mkdir(parents=True)
            generated = {"trueos-ide": {"tree_sha256": "6" * 64, "api_manifest_sha256": "7" * 64}}
            ready = {
                "schema": "trueos-selected-buildins/v2", "profile": self.plan["profile"],
                "buildins": self.plan["buildins"], "sources": "0" * 64,
                "packages": dict.fromkeys(self.plan["buildins"], "0" * 64),
                "database": {"app.db": "0" * 64, "app.db.lz4": "0" * 64},
                "generated": generated,
            }
            ready_bytes = (json.dumps(ready) + "\n").encode()
            (prepared / "ready.json").write_bytes(ready_bytes)
            for repository in ci.DEPENDENCIES.values():
                (root.parent / repository).mkdir()

            def command(argv, directory):
                if argv == ["git", "rev-parse", "HEAD"]:
                    name = next(key for key, value in ci.DEPENDENCIES.items() if directory.name == value)
                    return pins[name]
                if argv == ["rustc", "-vV"]:
                    return f"rustc archived\ncommit-hash: {ci.EXPECTED_RUSTC_COMMIT}"
                if argv == ["cargo", "-V"]:
                    return "cargo archived"
                if argv == ["git", "submodule", "status", "--recursive"]:
                    return " 5555555555555555555555555555555555555555 vendor/fixture"
                self.fail(f"unexpected command: {argv}")

            with mock.patch.object(ci, "ROOT", root), \
                    mock.patch.object(build_plan, "checkout_commit", return_value=self.COMMIT), \
                    mock.patch.object(ci, "_command", side_effect=command):
                artifact = ci.collect_artifact({"GITHUB_RUN_ID": "123"})
            self.assertEqual({path.name for path in artifact.iterdir()},
                             {"TrueOS.tar.lz4", "Cargo.lock", "build-manifest.json", "provenance.json", "SHA256SUMS"})
            provenance = json.loads((artifact / "provenance.json").read_text())
            self.assertEqual(provenance["sources"]["trueos"]["commit"], self.COMMIT)
            for name, commit in pins.items():
                self.assertEqual(provenance["sources"][name]["commit"], commit)
            self.assertEqual(provenance["artifact"]["sha256"], hashlib.sha256(bundle).hexdigest())
            self.assertEqual(provenance["buildins_preparation"], {
                "ready_sha256": hashlib.sha256(ready_bytes).hexdigest(), "generated": generated,
            })
            self.assertEqual(provenance["lockfiles_sha256"]["TRUEOS/Cargo.lock"],
                             hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest())
            for line in (artifact / "SHA256SUMS").read_text().splitlines():
                expected, name = line.split("  ", 1)
                self.assertEqual(hashlib.sha256((artifact / name).read_bytes()).hexdigest(), expected)

    def test_wrong_dependency_is_rejected_before_artifact_collection(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "TRUEOS"
            staging = root / "tgt/cloudbuild"
            staging.mkdir(parents=True)
            (staging / "build-manifest.json").write_text(json.dumps(self.plan))
            (staging / "dependency-pins.json").write_text(json.dumps(dict.fromkeys(ci.DEPENDENCIES, "2" * 40)))
            with mock.patch.object(ci, "ROOT", root), \
                    mock.patch.object(build_plan, "checkout_commit", return_value=self.COMMIT), \
                    mock.patch.object(ci, "_command", return_value="3" * 40), \
                    self.assertRaisesRegex(ValueError, "does not match"):
                ci.collect_artifact({})
            self.assertFalse((staging / "artifact").exists())


if __name__ == "__main__":
    unittest.main()
