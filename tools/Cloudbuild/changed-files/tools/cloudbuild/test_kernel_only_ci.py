"""Kernel-only CI/worker tests use tiny source fixtures, never an OS build."""
from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_plan
import ci
import worker


class KernelOnlyCiTests(unittest.TestCase):
    COMMIT = "1" * 40

    def setUp(self):
        self.plan = build_plan.resolve_request({"preset": "kernel-only"}, self.COMMIT)
        self.pins = {"blueprints": "3" * 40, "toolchain": "4" * 40}
        self.environ = {
            "GITHUB_SHA": self.COMMIT,
            "CLOUDBUILD_MANIFEST": json.dumps(self.plan),
            "CLOUDBUILD_BLUEPRINTS_COMMIT": self.pins["blueprints"],
            "CLOUDBUILD_TOOLCHAIN_COMMIT": self.pins["toolchain"],
        }

    def test_kernel_only_accepts_missing_or_empty_buildins_pin(self):
        for overrides in ({}, {"CLOUDBUILD_BUILDINS_COMMIT": ""}):
            with self.subTest(overrides=overrides):
                plan, pins = ci.validate_inputs(dict(self.environ, **overrides), self.COMMIT)
                self.assertEqual(plan, self.plan)
                self.assertEqual(pins, self.pins)

    def test_valid_unused_buildins_pin_is_discarded(self):
        _, pins = ci.validate_inputs(
            dict(self.environ, CLOUDBUILD_BUILDINS_COMMIT="2" * 40), self.COMMIT
        )
        self.assertEqual(pins, self.pins)
        for invalid in ("main", "$(echo untrusted)", "2" * 40 + "\nprofile=dev"):
            with self.subTest(pin=invalid), self.assertRaisesRegex(ValueError, "or empty when buildins are disabled"):
                ci.validate_inputs(dict(self.environ, CLOUDBUILD_BUILDINS_COMMIT=invalid), self.COMMIT)

    def test_bundled_apps_still_require_a_pinned_buildins_source(self):
        normal = build_plan.resolve_request({"preset": "universal"}, self.COMMIT)
        self.assertIn("buildins", normal["features"])
        environ = dict(self.environ, CLOUDBUILD_MANIFEST=json.dumps(normal))
        with self.assertRaisesRegex(ValueError, "buildins_commit"):
            ci.validate_inputs(environ, self.COMMIT)
        _, pins = ci.validate_inputs(dict(environ, CLOUDBUILD_BUILDINS_COMMIT="2" * 40), self.COMMIT)
        self.assertEqual(pins, {"buildins": "2" * 40, **self.pins})

    def test_kernel_only_keeps_blueprints_and_toolchain_requirements(self):
        for field in ("CLOUDBUILD_BLUEPRINTS_COMMIT", "CLOUDBUILD_TOOLCHAIN_COMMIT"):
            environ = dict(self.environ)
            del environ[field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                ci.validate_inputs(environ, self.COMMIT)

    def test_workflow_outputs_skip_buildins_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "TRUEOS"
            output = Path(temporary) / "outputs"
            with mock.patch.object(ci, "ROOT", root), \
                    mock.patch.object(build_plan, "checkout_commit", return_value=self.COMMIT):
                ci.write_inputs(dict(self.environ, GITHUB_OUTPUT=str(output)))
            lines = output.read_text().splitlines()
            self.assertIn("buildins_enabled=false", lines)
            self.assertFalse(any(line.startswith("buildins_commit=") for line in lines))
            self.assertEqual(
                json.loads((root / "tgt/cloudbuild/dependency-pins.json").read_text()), self.pins
            )

    def fixture(self, root):
        staging = root / "tgt/cloudbuild"
        staging.mkdir(parents=True)
        (staging / "build-manifest.json").write_text(json.dumps(self.plan))
        (staging / "dependency-pins.json").write_text(json.dumps(self.pins))
        for name in self.pins:
            (root.parent / ci.DEPENDENCIES[name]).mkdir()
        return staging

    def command(self, argv, directory):
        self.assertNotEqual(directory.name, "TRUEOS-Buildins", "Kernel-only must never inspect the Buildins checkout")
        if argv == ["git", "rev-parse", "HEAD"]:
            key = next(key for key, repo in ci.DEPENDENCIES.items() if repo == directory.name)
            return self.pins[key]
        if argv == ["rustc", "-vV"]:
            return f"rustc fixture\ncommit-hash: {ci.EXPECTED_RUSTC_COMMIT}"
        if argv == ["cargo", "-V"]:
            return "cargo fixture"
        if argv == ["git", "submodule", "status", "--recursive"]:
            return ""
        self.fail(f"unexpected command: {argv}")

    def test_ci_verifies_kernel_only_without_a_buildins_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "TRUEOS"
            self.fixture(root)
            with mock.patch.object(ci, "ROOT", root), \
                    mock.patch.object(build_plan, "checkout_commit", return_value=self.COMMIT), \
                    mock.patch.object(ci, "_command", side_effect=self.command):
                result = ci.verify_sources()
            self.assertEqual(set(result["sources"]), {"trueos", "blueprints", "toolchain"})
            self.assertFalse((root.parent / "TRUEOS-Buildins").exists())

    def test_kernel_only_provenance_ignores_stale_buildins_and_its_lockfile(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "TRUEOS"
            self.fixture(root)
            stale = root.parent / "TRUEOS-Buildins"
            stale.mkdir()
            (stale / "Cargo.lock").write_text("irrelevant, stale, non-Git Buildins directory\n")
            (root / "Cargo.lock").write_text("version = 4\n")
            (root / "tgt/rigreal.tar.lz4").write_bytes(b"\x04\x22\x4d\x18test fixture, not a bootable image")
            prepared = root / "tgt/buildins/release"
            prepared.mkdir(parents=True)
            (prepared / "ready.json").write_text("stale invalid preparation from an earlier app build")
            real_sha256 = ci._sha256

            def sha256(path):
                self.assertNotIn(stale, path.parents, "Kernel-only must not fingerprint a stale Buildins file")
                return real_sha256(path)

            with mock.patch.object(ci, "ROOT", root), \
                    mock.patch.object(build_plan, "checkout_commit", return_value=self.COMMIT), \
                    mock.patch.object(ci, "_command", side_effect=self.command), \
                    mock.patch.object(ci, "_sha256", side_effect=sha256):
                artifact = ci.collect_artifact({})
            provenance = json.loads((artifact / "provenance.json").read_text())
            self.assertFalse(provenance["buildins_enabled"])
            self.assertEqual(provenance["buildins"], [])
            self.assertIsNone(provenance["buildins_preparation"])
            self.assertNotIn("buildins", provenance["sources"])
            self.assertNotIn("TRUEOS-Buildins/Cargo.lock", provenance["lockfiles_sha256"])
            self.assertEqual(provenance["config_hash"], self.plan["config_hash"])


class KernelOnlyWorkerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="trueos-kernel-only-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.repository = self.directory / "TRUEOS"
        self.commit = self.create_checkout(self.repository)
        self.blueprints = self.directory / "TRUEOS-Blueprints"
        self.blueprints_commit = self.create_checkout(self.blueprints)
        self.manifest = build_plan.resolve_request({"preset": "kernel-only"}, self.commit)

    def create_checkout(self, path):
        path.mkdir()
        subprocess.run(["git", "init", "-q", str(path)], check=True, capture_output=True)
        (path / "source.txt").write_text("trusted fixture source\n")
        subprocess.run(["git", "-C", str(path), "add", "."], check=True, capture_output=True)
        subprocess.run(
            ["git", "-C", str(path), "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
             "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"],
            check=True, capture_output=True,
        )
        return worker.git(path, "rev-parse", "HEAD")

    def preflight(self, manifest=None, compiler_commit=worker.TOOLCHAIN_COMMIT):
        real_run = subprocess.run

        def run(argv, **kwargs):
            if argv == ["rustc", "-vV"]:
                return subprocess.CompletedProcess(argv, 0, f"rustc fixture\ncommit-hash: {compiler_commit}")
            if len(argv) > 2 and argv[:2] == ["git", "-C"]:
                self.assertNotEqual(Path(argv[2]).name, "TRUEOS-Buildins")
            return real_run(argv, **kwargs)

        with mock.patch.object(worker.subprocess, "run", side_effect=run), \
                mock.patch.object(worker.shutil, "which", return_value="fixture-command"):
            return worker.preflight(self.repository, manifest or self.manifest)

    def test_worker_accepts_missing_or_stale_buildins_without_inspection(self):
        provenance = self.preflight()
        self.assertFalse(provenance["buildins_enabled"])
        self.assertEqual(provenance["buildins"], [])
        self.assertEqual(provenance["sibling_commits"], {"TRUEOS-Blueprints": self.blueprints_commit})
        stale = self.directory / "TRUEOS-Buildins"
        stale.mkdir()
        (stale / "not-a-git-repository").write_text("No Buildins should be inspected or compiled.\n")
        self.assertEqual(self.preflight()["sibling_commits"], provenance["sibling_commits"])

    def test_worker_with_bundled_apps_requires_buildins_checkout(self):
        normal = build_plan.resolve_request({}, self.commit)
        self.assertIn("buildins", normal["features"])
        with self.assertRaisesRegex(ValueError, "TRUEOS-Buildins"):
            self.preflight(normal)

    def test_kernel_only_still_requires_blueprints_and_correct_compiler(self):
        with self.assertRaisesRegex(ValueError, "archived TRUEOS Rust toolchain"):
            self.preflight(compiler_commit="0" * 40)
        self.blueprints.rename(self.directory / "unprovisioned-blueprints")
        with self.assertRaisesRegex(ValueError, "TRUEOS-Blueprints"):
            self.preflight()


if __name__ == "__main__":
    unittest.main()
