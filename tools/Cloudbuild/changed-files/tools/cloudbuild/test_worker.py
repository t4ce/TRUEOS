"""Worker lifecycle tests use a tiny Git fixture, never the TRUEOS toolchain."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).parent))
from build_plan import resolve_request
from buildins import READY_SCHEMA
from service import Store, digest_file
from worker import process_one, run_build


class WorkerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.repository = self.directory / "TRUEOS"
        self.repository.mkdir()
        self.git("init", "-q")
        (self.repository / "source.txt").write_text("trusted source\n")
        (self.repository / ".gitignore").write_text("tgt/\n")
        self.git("add", ".")
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture")
        self.commit = self.git("rev-parse", "HEAD")
        self.store = Store(self.directory / "state")
        self.manifest = resolve_request({"preset": "qemu-virgl"}, self.commit)
        self.job = self.store.queue_anonymous(self.manifest)

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repository), *args], check=True,
                              capture_output=True, text=True).stdout.strip()

    def preflight(self, repository, manifest):
        self.assertEqual(manifest, self.manifest)
        return {"source_commit": self.commit, "sibling_commits": {}, "fixture": True}

    def build(self, repository, manifest, spec, log, timeout):
        self.assertEqual(json.loads(spec.read_text()), self.manifest)
        (repository / "tgt/rigreal.tar.lz4").write_bytes(b"test fixture, not a bootable image")
        if manifest["buildins"]:
            prepared = repository / "tgt/buildins" / manifest["profile"]
            prepared.mkdir(parents=True, exist_ok=True)
            generated = {"trueos-ide": {"tree_sha256": "a" * 64, "api_manifest_sha256": "b" * 64}} if "edit" in manifest["buildins"] else {}
            (prepared / "ready.json").write_text(json.dumps({
                "schema": READY_SCHEMA, "profile": manifest["profile"], "buildins": manifest["buildins"],
                "sources": "c" * 64, "packages": dict.fromkeys(manifest["buildins"], "d" * 64),
                "database": {"app.db": "e" * 64, "app.db.lz4": "f" * 64}, "generated": generated,
            }))

    def row(self):
        return self.store.owned_job(self.job["job_id"], self.job["access_token"])

    def test_success_copies_artifact_and_records_exact_digest(self):
        self.assertTrue(process_one(self.store, self.repository, preflight_fn=self.preflight, build_fn=self.build))
        row = self.row()
        self.assertEqual(row["status"], "succeeded")
        artifact = self.store.artifact_path(row["id"])
        self.assertEqual(row["artifact_sha256"], digest_file(artifact))
        self.assertEqual(row["artifact_size"], artifact.stat().st_size)
        self.assertFalse(process_one(self.store, self.repository, preflight_fn=self.preflight, build_fn=self.build))

    def test_build_failure_does_not_publish_old_artifact(self):
        (self.repository / "tgt").mkdir()
        (self.repository / "tgt/rigreal.tar.lz4").write_bytes(b"stale")
        def fail(*args):
            raise ValueError("Compiler failed")
        process_one(self.store, self.repository, preflight_fn=self.preflight, build_fn=fail)
        self.assertEqual(self.row()["status"], "failed")
        self.assertFalse(self.store.artifact_path(self.job["job_id"]).exists())
        self.assertEqual((self.repository / "tgt/rigreal.tar.lz4").read_bytes(), b"stale")

    def test_success_without_new_output_cannot_license_a_stale_image(self):
        (self.repository / "tgt").mkdir()
        old = self.repository / "tgt/rigreal.tar.lz4"
        old.write_bytes(b"previous configuration")
        process_one(self.store, self.repository, preflight_fn=self.preflight, build_fn=lambda *args: None)
        self.assertEqual(self.row()["status"], "failed")
        self.assertFalse(self.store.artifact_path(self.job["job_id"]).exists())
        self.assertEqual(old.read_bytes(), b"previous configuration")

    def test_source_change_during_build_fails_before_publication(self):
        def mutate(*args):
            self.build(*args)
            (self.repository / "source.txt").write_text("changed during build")
        process_one(self.store, self.repository, preflight_fn=self.preflight, build_fn=mutate)
        self.assertEqual(self.row()["status"], "failed")
        self.assertFalse(self.store.artifact_path(self.job["job_id"]).exists())

    def test_external_symlink_is_not_published(self):
        external = self.directory / "secret"
        external.write_text("not a build output")
        def malicious(*args):
            (self.repository / "tgt/rigreal.tar.lz4").symlink_to(external)
        process_one(self.store, self.repository, preflight_fn=self.preflight, build_fn=malicious)
        self.assertEqual(self.row()["status"], "failed")
        self.assertFalse(self.store.artifact_path(self.job["job_id"]).exists())

    def test_preflight_failure_never_starts_builder(self):
        def unavailable(*args):
            raise ValueError("Missing custom toolchain")
        with mock.patch("worker.run_build") as build:
            process_one(self.store, self.repository, preflight_fn=unavailable, build_fn=build)
            build.assert_not_called()
        self.assertEqual(self.row()["status"], "failed")

    def test_worker_does_not_forward_server_secrets(self):
        script = self.directory / "environment.py"
        output = self.directory / "environment.json"
        script.write_text("import json, os, pathlib, sys\npathlib.Path(sys.argv[1]).write_text(json.dumps(dict(os.environ)))\n")
        plan = {"command": [sys.executable, str(script), str(output)]}
        with mock.patch.dict(os.environ, {"TRUEOS_ISSUER_KEY": "secret-key-path", "GH_TOKEN": "secret-token"}):
            run_build(self.repository, plan, self.directory / "manifest.json", self.directory / "build.log", 10)
        environment = json.loads(output.read_text())
        self.assertNotIn("TRUEOS_ISSUER_KEY", environment)
        self.assertNotIn("GH_TOKEN", environment)
        self.assertEqual(environment["TRUEOS_CLOUDBUILD_SPEC"], str(self.directory / "manifest.json"))


if __name__ == "__main__":
    unittest.main()
