"""Selected-app CI/worker contracts; all artifacts and compilers are fixtures."""
from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import build_plan
import ci
import worker
from buildins import READY_SCHEMA
from service import Store


def write_prepared_fixture(repository, manifest):
    """Supply only the metadata consumed after a fixture compiler returns."""
    generated = {"trueos-ide": {"tree_sha256": "a" * 64, "api_manifest_sha256": "b" * 64}} if "edit" in manifest["buildins"] else {}
    ready = {
        "schema": READY_SCHEMA, "profile": manifest["profile"], "buildins": manifest["buildins"],
        "sources": "c" * 64, "packages": dict.fromkeys(manifest["buildins"], "d" * 64),
        "database": {"app.db": "e" * 64, "app.db.lz4": "f" * 64}, "generated": generated,
    }
    path = repository / "tgt/buildins" / manifest["profile"] / "ready.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(ready) + "\n")
    return path, generated


class SelectedAppsCiTests(unittest.TestCase):
    COMMIT = "1" * 40

    def setUp(self):
        self.ids = sorted(app["id"] for app in build_plan.catalog()["buildins"])
        self.selected = ["apps", "edit"]
        self.assertGreater(len(self.ids), len(self.selected), "Fixture must select a strict subset")
        self.plan = build_plan.resolve_request({"buildins": self.selected}, self.COMMIT)
        self.pins = {"buildins": "2" * 40, "blueprints": "3" * 40, "toolchain": "4" * 40}
        self.environ = {
            "GITHUB_SHA": self.COMMIT,
            "CLOUDBUILD_MANIFEST": json.dumps(self.plan),
            **{f"CLOUDBUILD_{name.upper()}_COMMIT": value for name, value in self.pins.items()},
        }

    def test_every_single_app_requires_a_pinned_buildins_source(self):
        for app in self.ids:
            with self.subTest(app=app):
                manifest = build_plan.resolve_request({"buildins": [app]}, self.COMMIT)
                environ = dict(self.environ, CLOUDBUILD_MANIFEST=json.dumps(manifest))
                accepted, pins = ci.validate_inputs(environ, self.COMMIT)
                self.assertEqual(accepted["buildins"], [app])
                self.assertEqual(pins, self.pins)
                del environ["CLOUDBUILD_BUILDINS_COMMIT"]
                with self.assertRaisesRegex(ValueError, "buildins_commit"):
                    ci.validate_inputs(environ, self.COMMIT)

    def test_workflow_preserves_exact_selection_before_dependency_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "TRUEOS"
            output = Path(temporary) / "outputs"
            with mock.patch.object(ci, "ROOT", root), \
                    mock.patch.object(build_plan, "checkout_commit", return_value=self.COMMIT):
                ci.write_inputs(dict(self.environ, GITHUB_OUTPUT=str(output)))
            self.assertIn("buildins_enabled=true", output.read_text().splitlines())
            self.assertIn(f"buildins_commit={self.pins['buildins']}", output.read_text().splitlines())
            saved = json.loads((root / "tgt/cloudbuild/build-manifest.json").read_text())
            self.assertEqual(saved, self.plan)
            self.assertEqual(saved["buildins"], self.selected)

    def test_changing_one_selected_app_invalidates_the_admitted_manifest(self):
        altered = copy.deepcopy(self.plan)
        altered["buildins"] = sorted([self.selected[0], self.ids[-1]])
        # Both choices still enable the same Cargo payload feature. The exact
        # app list must nevertheless be bound by the configuration hash.
        with self.assertRaises(ValueError):
            ci.validate_inputs(dict(self.environ, CLOUDBUILD_MANIFEST=json.dumps(altered)), self.COMMIT)

    def test_artifact_provenance_records_exact_apps_and_their_pinned_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "TRUEOS"
            bundle = self.fixture(root)
            ready, generated = write_prepared_fixture(root, self.plan)
            artifact = self.collect(root)
            provenance = json.loads((artifact / "provenance.json").read_text())
            self.assertTrue(provenance["buildins_enabled"])
            self.assertEqual(provenance["buildins"], self.selected)
            self.assertEqual(provenance["sources"]["buildins"]["commit"], self.pins["buildins"])
            self.assertIn("TRUEOS-Buildins/Cargo.lock", provenance["lockfiles_sha256"])
            self.assertEqual(provenance["config_hash"], self.plan["config_hash"])
            self.assertEqual(provenance["artifact"]["sha256"], hashlib.sha256(bundle).hexdigest())
            self.assertEqual(provenance["buildins_preparation"], {
                "ready_sha256": hashlib.sha256(ready.read_bytes()).hexdigest(), "generated": generated,
            })
            self.assertEqual(json.loads((artifact / "build-manifest.json").read_text()), self.plan)

    def fixture(self, root):
        staging = root / "tgt/cloudbuild"
        staging.mkdir(parents=True)
        (staging / "build-manifest.json").write_text(json.dumps(self.plan))
        (staging / "dependency-pins.json").write_text(json.dumps(self.pins))
        (root / "Cargo.lock").write_text("version = 4\n")
        bundle = b"\x04\x22\x4d\x18selected-app fixture, not a boot image"
        (root / "tgt/rigreal.tar.lz4").write_bytes(bundle)
        for name, repository in ci.DEPENDENCIES.items():
            directory = root.parent / repository
            directory.mkdir()
            if name == "buildins":
                (directory / "Cargo.lock").write_text("version = 4\n# app dependency fixture\n")
        return bundle

    def command(self, argv, directory):
        if argv == ["git", "rev-parse", "HEAD"]:
            name = next(name for name, repository in ci.DEPENDENCIES.items() if directory.name == repository)
            return self.pins[name]
        if argv == ["rustc", "-vV"]:
            return f"rustc fixture\ncommit-hash: {ci.EXPECTED_RUSTC_COMMIT}"
        if argv == ["cargo", "-V"]:
            return "cargo fixture"
        if argv == ["git", "submodule", "status", "--recursive"]:
            return ""
        self.fail(f"unexpected command: {argv}")

    def collect(self, root):
        with mock.patch.object(ci, "ROOT", root), \
                mock.patch.object(build_plan, "checkout_commit", return_value=self.COMMIT), \
                mock.patch.object(ci, "_command", side_effect=self.command):
            return ci.collect_artifact({})

    def test_missing_or_wrong_preparation_cannot_publish_an_artifact(self):
        for state in ("missing", "wrong selection", "incomplete"):
            with self.subTest(state=state), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary) / "TRUEOS"
                self.fixture(root)
                if state != "missing":
                    other = build_plan.resolve_request({"buildins": ["apps"]}, self.COMMIT) if state == "wrong selection" else self.plan
                    ready, _ = write_prepared_fixture(root, other)
                    if state == "incomplete":
                        (ready.parent / ".building").touch()
                with self.assertRaises((OSError, ValueError)):
                    self.collect(root)
                self.assertFalse((root / "tgt/cloudbuild/artifact").exists())


class SelectedAppsWorkerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="trueos-selected-apps-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.repository = self.directory / "TRUEOS"
        self.commit = self.create_checkout(self.repository)
        self.blueprints_commit = self.create_checkout(self.directory / "TRUEOS-Blueprints")
        self.selected = ["apps", "edit"]
        self.plan = build_plan.resolve_request({"preset": "qemu-virgl", "buildins": self.selected}, self.commit)
        self.store = Store(self.directory / "state")
        self.job = self.store.queue_anonymous(self.plan)

    @staticmethod
    def create_checkout(path):
        path.mkdir()
        subprocess.run(["git", "init", "-q", str(path)], check=True, capture_output=True)
        (path / "source.txt").write_text("trusted selected-app fixture\n")
        (path / ".gitignore").write_text("tgt/\n")
        subprocess.run(["git", "-C", str(path), "add", "."], check=True, capture_output=True)
        subprocess.run(
            ["git", "-C", str(path), "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
             "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"],
            check=True, capture_output=True,
        )
        return worker.git(path, "rev-parse", "HEAD")

    def preflight(self, repository, manifest):
        real_run = subprocess.run

        def run(argv, **kwargs):
            if argv == ["rustc", "-vV"]:
                return subprocess.CompletedProcess(argv, 0, f"rustc fixture\ncommit-hash: {worker.TOOLCHAIN_COMMIT}")
            return real_run(argv, **kwargs)

        with mock.patch.object(worker.subprocess, "run", side_effect=run), \
                mock.patch.object(worker.shutil, "which", return_value="fixture-command"):
            return worker.preflight(repository, manifest)

    def test_subset_requires_buildins_before_builder_starts(self):
        with mock.patch.object(worker, "run_build") as build:
            self.assertTrue(worker.process_one(
                self.store, self.repository, preflight_fn=self.preflight, build_fn=build
            ))
            build.assert_not_called()
        row = self.store.owned_job(self.job["job_id"], self.job["access_token"])
        self.assertEqual(row["status"], "failed")
        self.assertIn("TRUEOS-Buildins", row["error"])

    def test_exact_app_selection_survives_queue_build_and_publication(self):
        buildins_commit = self.create_checkout(self.directory / "TRUEOS-Buildins")

        def build(repository, manifest, spec, log, timeout):
            self.assertEqual(manifest, self.plan)
            self.assertEqual(json.loads(spec.read_text()), self.plan)
            (repository / "tgt/rigreal.tar.lz4").write_bytes(b"selected-app worker fixture, not a boot image")
            write_prepared_fixture(repository, manifest)

        self.assertTrue(worker.process_one(
            self.store, self.repository, preflight_fn=self.preflight, build_fn=build
        ))
        row = self.store.owned_job(self.job["job_id"], self.job["access_token"])
        self.assertEqual(row["status"], "succeeded", row["error"])
        self.assertEqual(json.loads(row["manifest"]), self.plan)
        provenance = json.loads(row["provenance"])
        self.assertTrue(provenance["buildins_enabled"])
        self.assertEqual(provenance["buildins"], self.selected)
        self.assertEqual(provenance["sibling_commits"], {
            "TRUEOS-Buildins": buildins_commit,
            "TRUEOS-Blueprints": self.blueprints_commit,
        })
        ready = self.repository / "tgt/buildins/release/ready.json"
        self.assertEqual(provenance["buildins_preparation"], {
            "ready_sha256": hashlib.sha256(ready.read_bytes()).hexdigest(),
            "generated": json.loads(ready.read_text())["generated"],
        })
        artifact = self.store.artifact_path(self.job["job_id"]).parent
        self.assertEqual(json.loads((artifact / "manifest.json").read_text()), self.plan)
        self.assertEqual(json.loads((artifact / "provenance.json").read_text()), provenance)

    def test_missing_preparation_cannot_publish_a_selected_app_job(self):
        self.create_checkout(self.directory / "TRUEOS-Buildins")

        def build(repository, *args):
            (repository / "tgt/rigreal.tar.lz4").write_bytes(b"fixture with no matching app preparation")

        worker.process_one(self.store, self.repository, preflight_fn=self.preflight, build_fn=build)
        row = self.store.owned_job(self.job["job_id"], self.job["access_token"])
        self.assertEqual(row["status"], "failed")
        self.assertFalse(self.store.artifact_path(self.job["job_id"]).exists())

    def test_wrong_preparation_cannot_publish_a_selected_app_job(self):
        self.create_checkout(self.directory / "TRUEOS-Buildins")

        def build(repository, *args):
            (repository / "tgt/rigreal.tar.lz4").write_bytes(b"fixture with a different app preparation")
            other = build_plan.resolve_request({"buildins": ["apps"]}, self.commit)
            write_prepared_fixture(repository, other)

        worker.process_one(self.store, self.repository, preflight_fn=self.preflight, build_fn=build)
        row = self.store.owned_job(self.job["job_id"], self.job["access_token"])
        self.assertEqual(row["status"], "failed")
        self.assertFalse(self.store.artifact_path(self.job["job_id"]).exists())


if __name__ == "__main__":
    unittest.main()
