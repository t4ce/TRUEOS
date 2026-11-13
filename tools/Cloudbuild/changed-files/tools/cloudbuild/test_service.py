"""Security boundary tests for the build queue, licensing, and HTTP adapter."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
import copy
import hashlib
import http.client
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

# The tool is runnable directly as well as through unittest discovery. Its
# production entry point imports these neighboring modules in the same way.
MODULE_DIRECTORY = Path(__file__).resolve().parent
if str(MODULE_DIRECTORY) not in sys.path:
    sys.path.insert(0, str(MODULE_DIRECTORY))

import identity
import service
from build_plan import catalog


SOURCE = "1" * 40
REQUEST = {"preset": "qemu-virgl"}


@unittest.skipUnless(shutil.which("ssh-keygen"), "OpenSSH ssh-keygen is required")
class ServiceSecurityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.key_directory = tempfile.TemporaryDirectory(prefix="trueos-service-keys-")
        root = Path(cls.key_directory.name)
        cls.user_key, cls.other_key, cls.issuer_key = (root / name for name in ("user", "other", "issuer"))
        for key in (cls.user_key, cls.other_key, cls.issuer_key):
            subprocess.run(
                ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
                stdin=subprocess.DEVNULL,
                capture_output=True,
                check=True,
                timeout=10,
            )
        cls.user_public = cls.user_key.with_suffix(".pub").read_text()
        cls.other_public = cls.other_key.with_suffix(".pub").read_text()
        cls.issuer_public = cls.issuer_key.with_suffix(".pub").read_text()
        cls.feature_ids = [item["id"] for item in catalog()["features"]]

    @classmethod
    def tearDownClass(cls):
        cls.key_directory.cleanup()

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="trueos-service-tests-")
        self.addCleanup(temporary.cleanup)
        self.now = 1_700_000_000
        self.store = service.Store(Path(temporary.name) / "state", clock=lambda: self.now)
        self.service = service.Service(
            self.store, SOURCE, "http://127.0.0.1", issuer_key=self.issuer_key
        )

    def grant(self, public_key=None, features=None, *, expires_at=None, license_id="test-license"):
        return self.store.grant(
            public_key or self.user_public,
            license_id,
            self.feature_ids if features is None else features,
            expires_at,
        )

    def challenge(self, *, request=None, public_key=None):
        return self.service.post(
            "/api/challenges",
            {"request": REQUEST if request is None else request, "public_key": public_key or self.user_public},
        )

    def sign(self, challenge, *, key=None):
        result = subprocess.run(
            ["ssh-keygen", "-Y", "sign", "-f", str(key or self.user_key), "-n", service.NAMESPACE],
            input=challenge["challenge"].encode("ascii"),
            capture_output=True,
            check=True,
            timeout=10,
        )
        return result.stdout.decode("ascii")

    def submit(self, challenge, signature=None):
        return self.service.post(
            "/api/builds",
            {
                "challenge_id": challenge["challenge_id"],
                "signature": signature if signature is not None else self.sign(challenge),
            },
        )

    def job_count(self):
        with self.store.connect() as db:
            return db.execute("SELECT count(*) FROM jobs").fetchone()[0]

    def assert_api_error(self, expected, call, *args, **kwargs):
        expected = (expected,) if isinstance(expected, int) else expected
        with self.assertRaises(service.APIError) as caught:
            call(*args, **kwargs)
        self.assertIn(caught.exception.status, expected)

    def finish_fixture_build(self, job):
        claimed = self.store.claim()
        self.assertEqual(claimed["id"], job["job_id"])
        payload = b"TRUEOS test artifact: no operating-system compilation is performed here.\n"
        artifact = self.store.artifact_path(job["job_id"])
        artifact.parent.mkdir(parents=True)
        artifact.write_bytes(payload)
        self.store.finish_build(
            job["job_id"], hashlib.sha256(payload).hexdigest(), len(payload),
            {"source_commit": SOURCE, "test_fixture": True},
        )
        return artifact, payload

    def test_default_denies_anonymous_builds_but_allows_planning(self):
        plan = self.service.post("/api/plan", REQUEST)
        self.assertEqual(plan["source_commit"], SOURCE)
        self.assert_api_error(403, self.service.post, "/api/builds", {"request": REQUEST})
        self.assertEqual(self.job_count(), 0)

    def test_opt_in_anonymous_build_has_no_license(self):
        anonymous = service.Service(self.store, SOURCE, "http://127.0.0.1", allow_anonymous=True)
        job = anonymous.post("/api/builds", {"request": REQUEST})
        self.finish_fixture_build(job)
        result = anonymous.get_job(job["job_id"], job["access_token"])
        self.assertEqual(result["status"], "succeeded")
        self.assertNotIn("receipt", result)
        row = self.store.owned_job(job["job_id"], job["access_token"])
        self.assertIsNone(row["fingerprint"])
        self.assertIsNone(row["license_id"])

    def test_background_finalization_issues_receipt_without_browser_polling(self):
        self.grant()
        job = self.submit(self.challenge())
        self.finish_fixture_build(job)
        self.assertEqual(self.store.owned_job(job["job_id"], job["access_token"])["status"], "built")
        self.service.finalize_pending()
        row = self.store.owned_job(job["job_id"], job["access_token"])
        self.assertEqual(row["status"], "succeeded")
        self.assertTrue(identity.verify_receipt(json.loads(row["receipt"]), self.issuer_public))

    def test_key_possession_without_grant_does_not_authorize_build(self):
        challenge = self.challenge()
        signature = self.sign(challenge)
        self.assert_api_error(403, self.submit, challenge, signature)
        self.assertEqual(self.job_count(), 0)
        self.grant()
        job = self.submit(challenge, signature)
        self.assertEqual(job["status"], "queued")

    def test_app_payload_choice_preserves_existing_capability_grants(self):
        self.grant(features=["usb", "virgl", "task_profile"])
        for buildins in (True, False):
            challenge = self.challenge(request={"preset": "qemu-virgl", "features": {"buildins": buildins}})
            job = self.submit(challenge)
            self.assertEqual("buildins" in job["manifest"]["features"], buildins)
        # Packaging cannot authorize a previously ungranted kernel feature.
        challenge = self.challenge(request={"preset": "qemu-virgl", "features": {"buildins": False, "speech": True}})
        self.assert_api_error(403, self.submit, challenge)

    def test_exact_app_set_is_bound_to_challenge_job_and_signed_receipt(self):
        self.grant(features=["usb", "virgl", "task_profile"])
        first = self.challenge(request={"preset": "qemu-virgl", "buildins": ["termdir", "edit"]})
        second = self.challenge(request={"preset": "qemu-virgl", "buildins": ["apps"]})
        self.assertEqual(first["manifest"]["features"], second["manifest"]["features"])
        self.assertNotEqual(first["manifest"]["config_hash"], second["manifest"]["config_hash"])
        self.assertEqual(json.loads(first["challenge"])["manifest"]["buildins"], ["edit", "termdir"])
        self.assert_api_error(403, self.submit, second, self.sign(first))
        job = self.submit(first)
        self.assertEqual(job["manifest"]["buildins"], ["edit", "termdir"])
        self.finish_fixture_build(job)
        result = self.service.get_job(job["job_id"], job["access_token"])
        self.assertEqual(result["receipt"]["receipt"]["buildins"], ["edit", "termdir"])
        self.assertTrue(identity.verify_receipt(result["receipt"], self.issuer_public))
        altered = copy.deepcopy(result["receipt"])
        altered["receipt"]["buildins"] = ["apps"]
        self.assertFalse(identity.verify_receipt(altered, self.issuer_public))

    def test_obsolete_manifest_fails_without_stopping_receipt_finalization(self):
        self.grant()
        obsolete = self.submit(self.challenge())
        self.finish_fixture_build(obsolete)
        manifest = dict(obsolete["manifest"], schema=1)
        del manifest["buildins"]
        with self.store.connect(transaction=True) as db:
            db.execute("UPDATE jobs SET manifest=? WHERE id=?", (json.dumps(manifest), obsolete["job_id"]))
        current = self.submit(self.challenge(request={"preset": "qemu-virgl", "buildins": ["edit"]}))
        self.finish_fixture_build(current)
        self.service.finalize_pending()
        rejected = self.store.owned_job(obsolete["job_id"], obsolete["access_token"])
        accepted = self.store.owned_job(current["job_id"], current["access_token"])
        self.assertEqual(rejected["status"], "failed")
        self.assertIsNone(rejected["receipt"])
        self.assertIn("fresh build", rejected["error"])
        self.assertEqual(accepted["status"], "succeeded")

    def test_revoked_expired_and_feature_restricted_grants_are_rejected(self):
        fingerprint = self.grant()
        challenge = self.challenge()
        signature = self.sign(challenge)
        self.store.revoke(fingerprint)
        self.assert_api_error(403, self.submit, challenge, signature)
        self.grant(expires_at=self.now + 10)
        self.now += 10
        self.assert_api_error(403, self.submit, challenge, signature)
        self.grant(features=["usb"])
        self.assert_api_error(403, self.submit, challenge, signature)
        self.assertEqual(self.job_count(), 0)
        self.grant(features=challenge["manifest"]["features"])
        self.assertEqual(self.submit(challenge, signature)["status"], "queued")

    def test_challenge_binds_source_configuration_origin_key_and_nonce(self):
        self.grant()
        first = self.challenge()
        second = self.challenge(request={**REQUEST, "profile": "dev"})
        payload = json.loads(first["challenge"])
        self.assertEqual(payload["source_commit"], SOURCE)
        self.assertEqual(payload["audience"], self.service.origin)
        self.assertEqual(payload["manifest"], first["manifest"])
        self.assertEqual(payload["config_hash"], first["manifest"]["config_hash"])
        self.assertEqual(payload["key_fingerprint"], identity.normalize_public_key(self.user_public)["fingerprint"])
        self.assertNotEqual(payload["nonce"], json.loads(second["challenge"])["nonce"])
        self.assert_api_error(403, self.submit, second, self.sign(first))
        self.assert_api_error(403, self.submit, first, self.sign(first, key=self.other_key))
        self.assertEqual(self.job_count(), 0)
        self.assertEqual(self.submit(second)["manifest"], second["manifest"])

    def test_changed_server_origin_or_revision_cannot_accept_old_challenge(self):
        self.grant()
        challenge = self.challenge()
        proof = {"challenge_id": challenge["challenge_id"], "signature": self.sign(challenge)}
        for revision, origin in ((SOURCE, "https://other.example"), ("2" * 40, self.service.origin)):
            changed = service.Service(self.store, revision, origin, issuer_key=self.issuer_key)
            with self.subTest(origin=origin, revision=revision):
                self.assert_api_error((400, 403, 409, 410), changed.post, "/api/builds", proof)
        self.assertEqual(self.job_count(), 0)
        self.assertEqual(self.service.post("/api/builds", proof)["status"], "queued")

    def test_signature_cannot_be_replayed_sequentially(self):
        self.grant()
        challenge = self.challenge()
        signature = self.sign(challenge)
        self.submit(challenge, signature)
        self.assert_api_error(409, self.submit, challenge, signature)
        self.assertEqual(self.job_count(), 1)

    def test_concurrent_valid_proofs_create_exactly_one_job(self):
        self.grant()
        challenge = self.challenge()
        signature = self.sign(challenge)
        barrier = threading.Barrier(2)
        real_verify = service.verify_signature

        def both_verify_before_writing(*args, **kwargs):
            result = real_verify(*args, **kwargs)
            barrier.wait(timeout=5)
            return result

        def submit_once():
            try:
                return self.submit(challenge, signature)
            except service.APIError as error:
                return error.status

        with patch.object(service, "verify_signature", side_effect=both_verify_before_writing):
            with ThreadPoolExecutor(max_workers=2) as pool:
                results = list(pool.map(lambda _: submit_once(), range(2)))
        self.assertEqual(sum(isinstance(result, dict) for result in results), 1)
        self.assertEqual(sum(result == 409 for result in results), 1)
        self.assertEqual(self.job_count(), 1)

    def test_expired_challenges_cannot_queue_and_expiry_is_rechecked_after_crypto(self):
        self.grant()
        challenge = self.challenge()
        signature = self.sign(challenge)
        self.now = challenge["expires_at"]
        self.assert_api_error(410, self.submit, challenge, signature)
        self.assertEqual(self.job_count(), 0)

        challenge = self.challenge()
        signature = self.sign(challenge)
        real_verify = service.verify_signature

        def expire_during_verification(*args, **kwargs):
            result = real_verify(*args, **kwargs)
            self.now = challenge["expires_at"]
            return result

        with patch.object(service, "verify_signature", side_effect=expire_during_verification):
            self.assert_api_error(410, self.submit, challenge, signature)
        self.assertEqual(self.job_count(), 0)

    def test_submit_cannot_replace_stored_request_or_manifest(self):
        self.grant()
        challenge = self.challenge()
        signature = self.sign(challenge)
        proof = {"challenge_id": challenge["challenge_id"], "signature": signature}
        replacement = copy.deepcopy(challenge["manifest"])
        replacement["source_commit"] = "f" * 40
        for field, value in (
            ("manifest", replacement), ("request", {"preset": "universal"}),
            ("public_key", self.other_public), ("source_commit", "f" * 40),
        ):
            with self.subTest(field=field):
                self.assert_api_error(400, self.service.post, "/api/builds", {**proof, field: value})
        job = self.service.post("/api/builds", proof)
        self.assertEqual(job["manifest"], challenge["manifest"])

    def test_build_tokens_are_isolated_between_owners(self):
        self.grant()
        self.grant(self.other_public, license_id="other-license")
        first = self.submit(self.challenge())
        second_challenge = self.challenge(public_key=self.other_public)
        second = self.submit(second_challenge, self.sign(second_challenge, key=self.other_key))
        self.assert_api_error(404, self.service.get_job, first["job_id"], second["access_token"])
        self.assert_api_error(404, self.service.get_job, second["job_id"], first["access_token"])
        self.assert_api_error(401, self.service.get_job, first["job_id"], "")
        self.assertEqual(self.service.get_job(first["job_id"], first["access_token"])["status"], "queued")
        self.assertEqual(first["manifest"]["config_hash"], second["manifest"]["config_hash"])

    def test_license_receipt_binds_verified_artifact_and_configuration(self):
        fingerprint = self.grant(expires_at=self.now + 3600)
        job = self.submit(self.challenge())
        artifact, payload = self.finish_fixture_build(job)
        result = self.service.get_job(job["job_id"], job["access_token"])
        self.assertEqual(result["status"], "succeeded")
        self.assertTrue(identity.verify_receipt(result["receipt"], self.issuer_public))
        receipt = result["receipt"]["receipt"]
        self.assertEqual(receipt["subject_key_fingerprint"], fingerprint)
        self.assertEqual(receipt["config_hash"], job["manifest"]["config_hash"])
        self.assertEqual(receipt["features"], job["manifest"]["features"])
        self.assertEqual(receipt["buildins"], job["manifest"]["buildins"])
        self.assertEqual(receipt["source_commit"], SOURCE)
        self.assertEqual(receipt["artifact_sha256"], hashlib.sha256(artifact.read_bytes()).hexdigest())
        self.assertEqual(receipt["artifact_size"], len(payload))
        self.assertEqual(receipt["expires_at"], self.now + 3600)
        self.assertEqual(result["artifact"]["sha256"], receipt["artifact_sha256"])

    def test_changed_artifact_is_not_signed_or_released(self):
        self.grant()
        job = self.submit(self.challenge())
        artifact, payload = self.finish_fixture_build(job)
        artifact.write_bytes(b"X" + payload[1:])  # Same length, different digest.
        self.assert_api_error(503, self.service.get_job, job["job_id"], job["access_token"])
        row = self.store.owned_job(job["job_id"], job["access_token"])
        self.assertEqual(row["status"], "built")
        self.assertIsNone(row["receipt"])

    def test_revocation_or_license_change_before_finalization_blocks_issuance(self):
        fingerprint = self.grant()
        first = self.submit(self.challenge())
        self.finish_fixture_build(first)
        self.store.revoke(fingerprint)
        result = self.service.get_job(first["job_id"], first["access_token"])
        self.assertEqual(result["status"], "failed")
        self.assertNotIn("receipt", result)
        self.assertNotIn("artifact", result)

        self.grant()
        second = self.submit(self.challenge())
        self.finish_fixture_build(second)
        self.grant(license_id="different-license")
        result = self.service.get_job(second["job_id"], second["access_token"])
        self.assertEqual(result["status"], "failed")
        self.assertNotIn("receipt", result)

    def test_revocation_during_signing_is_rechecked_transactionally(self):
        fingerprint = self.grant()
        job = self.submit(self.challenge())
        self.finish_fixture_build(job)
        real_sign = service.sign_receipt

        def revoke_after_signature(*args, **kwargs):
            envelope = real_sign(*args, **kwargs)
            self.store.revoke(fingerprint)
            return envelope

        with patch.object(service, "sign_receipt", side_effect=revoke_after_signature):
            result = self.service.get_job(job["job_id"], job["access_token"])
        self.assertEqual(result["status"], "failed")
        self.assertNotIn("receipt", result)
        self.assertIsNone(self.store.owned_job(job["job_id"], job["access_token"])["receipt"])

    def start_http(self):
        server = service.HTTPServer(("127.0.0.1", 0), None)
        origin = f"http://127.0.0.1:{server.server_port}"
        self.service = service.Service(self.store, SOURCE, origin, issuer_key=self.issuer_key)
        server.service = self.service
        thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
        thread.start()

        def stop():
            server.shutdown()
            thread.join(timeout=2)
            server.server_close()

        self.addCleanup(stop)
        self.http_server = server

    def http_request(self, method, path, *, body=b"", headers=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.http_server.server_port, timeout=5)
        try:
            connection.putrequest(method, path, skip_host=True)
            values = headers if headers is not None else [
                ("Host", self.service.host), ("Content-Type", "application/json"),
                ("Content-Length", str(len(body))),
            ]
            for name, value in values:
                connection.putheader(name, value)
            connection.endheaders(body)
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()

    def test_http_rejects_wrong_duplicate_or_missing_host_and_cross_origin(self):
        self.start_http()
        base = [("Content-Type", "application/json"), ("Content-Length", "2")]
        cases = [
            [],
            [("Host", "attacker.example")],
            [("Host", self.service.host), ("Host", "attacker.example")],
            [("Host", self.service.host), ("Origin", "https://attacker.example")],
            [("Host", self.service.host), ("Origin", self.service.origin), ("Origin", "https://attacker.example")],
        ]
        for headers in cases:
            with self.subTest(headers=headers):
                status, _, _ = self.http_request("POST", "/api/plan", body=b"{}", headers=headers + base)
                self.assertIn(status, (400, 403))
        status, _, _ = self.http_request("POST", "/api/plan", body=b"{}")
        self.assertEqual(status, 200)

    def test_http_rejects_ambiguous_body_framing_and_oversized_requests(self):
        self.start_http()
        host = [("Host", self.service.host)]
        cases = [
            (415, host + [("Content-Type", "text/plain"), ("Content-Length", "2")]),
            (411, host + [("Content-Type", "application/json")]),
            (411, host + [("Content-Type", "application/json"), ("Content-Length", "2"), ("Content-Length", "2")]),
            (400, host + [("Content-Type", "application/json"), ("Content-Length", "2"), ("Transfer-Encoding", "chunked")]),
            (413, host + [("Content-Type", "application/json"), ("Content-Length", str(service.MAX_BODY + 1))]),
        ]
        for expected, headers in cases:
            with self.subTest(expected=expected, headers=headers):
                status, _, _ = self.http_request("POST", "/api/plan", body=b"{}", headers=headers)
                self.assertEqual(status, expected)

    def test_http_rejects_duplicate_json_fields(self):
        self.start_http()
        for body in (
            b'{"preset":"qemu-virgl","preset":"universal"}',
            b'{"features":{"usb":true,"usb":false}}',
        ):
            with self.subTest(body=body):
                status, _, _ = self.http_request("POST", "/api/plan", body=body)
                self.assertEqual(status, 400)

    def test_http_licensed_flow_and_private_artifact_download(self):
        self.start_http()
        self.grant()
        status, headers, data = self.http_request(
            "POST", "/api/challenges",
            body=identity.canonical_json({"request": REQUEST, "public_key": self.user_public}),
        )
        self.assertEqual(status, 201)
        self.assertEqual(headers["Cache-Control"], "no-store")
        challenge = json.loads(data)
        status, _, data = self.http_request(
            "POST", "/api/builds",
            body=identity.canonical_json({"challenge_id": challenge["challenge_id"], "signature": self.sign(challenge)}),
        )
        self.assertEqual(status, 201)
        job = json.loads(data)
        _, payload = self.finish_fixture_build(job)
        path = f'/api/jobs/{job["job_id"]}'
        authorization = [("Host", self.service.host), ("Authorization", "Bearer " + job["access_token"])]
        status, _, data = self.http_request("GET", path, headers=authorization)
        self.assertEqual(status, 200)
        self.assertTrue(identity.verify_receipt(json.loads(data)["receipt"], self.issuer_public))
        status, headers, data = self.http_request("GET", path + "/artifact", headers=authorization)
        self.assertEqual(status, 200)
        self.assertEqual(data, payload)
        self.assertEqual(headers["Content-Type"], "application/octet-stream")
        status, _, _ = self.http_request("GET", path + "/artifact", headers=[("Host", self.service.host)])
        self.assertEqual(status, 401)


if __name__ == "__main__":
    unittest.main()
