"""Adversarial interoperability tests using real temporary OpenSSH keys."""

from __future__ import annotations

import base64
import copy
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

try:
    from . import identity
except ImportError:
    import identity


@unittest.skipUnless(shutil.which("ssh-keygen"), "OpenSSH ssh-keygen is required")
class IdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="trueos-identity-tests-")
        cls.directory = Path(cls.temporary.name)
        cls.user_key = cls.directory / "user"
        cls.other_key = cls.directory / "other"
        cls.issuer_key = cls.directory / "issuer"
        for path in (cls.user_key, cls.other_key, cls.issuer_key):
            subprocess.run(
                ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(path)],
                stdin=subprocess.DEVNULL,
                capture_output=True,
                check=True,
                timeout=10,
            )
        cls.user_public = cls.user_key.with_suffix(".pub").read_text()
        cls.other_public = cls.other_key.with_suffix(".pub").read_text()
        cls.issuer_public = cls.issuer_key.with_suffix(".pub").read_text()
        cls.message = b'{"challenge":"fresh-random-server-value","features":["usb"]}\n'
        cls.signature = cls.sign(cls.user_key, cls.message)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    @staticmethod
    def sign(key, message, namespace=identity.REQUEST_NAMESPACE):
        result = subprocess.run(
            ["ssh-keygen", "-Y", "sign", "-f", str(key), "-n", namespace],
            input=message,
            capture_output=True,
            check=True,
            timeout=10,
        )
        return result.stdout.decode("ascii")

    def receipt(self):
        return {
            "license_id": "test-license",
            "subject": identity.normalize_public_key(self.user_public)["fingerprint"],
            "source_commit": "a" * 40,
            "manifest_sha256": "b" * 64,
            "artifact_sha256": "c" * 64,
            "entitlements": ["build"],
        }

    def test_canonical_bytes_are_explicit_and_preserve_unicode(self):
        self.assertEqual(
            identity.canonical_json({"z": [True, 1], "a": "\u00fc"}),
            b'{"a":"\\u00fc","z":[true,1]}\n',
        )
        self.assertEqual(json.loads(identity.canonical_json(self.receipt())), self.receipt())

    def test_non_json_or_ambiguous_objects_are_rejected(self):
        cyclic = []
        cyclic.append(cyclic)
        for value in ({1: "non-string key"}, {"nan": float("nan")}, {"bytes": b"x"}, cyclic):
            with self.subTest(value_type=type(value).__name__):
                with self.assertRaises(ValueError):
                    identity.canonical_json(value)

    def test_public_key_comment_is_removed_and_fingerprint_matches_openssh(self):
        normalized = identity.normalize_public_key(self.user_public)
        self.assertEqual(len(normalized["public_key"].split()), 2)
        output = subprocess.run(
            ["ssh-keygen", "-l", "-E", "sha256", "-f", str(self.user_key.with_suffix(".pub"))],
            capture_output=True,
            check=True,
            timeout=10,
        ).stdout.decode("ascii")
        self.assertEqual(normalized["fingerprint"], output.split()[1])
        malicious_comment = normalized["public_key"] + " ;$(arbitrary-command) `another-command`"
        self.assertEqual(identity.normalize_public_key(malicious_comment), normalized)

    def test_malformed_and_authorized_keys_injection_are_rejected(self):
        key = identity.normalize_public_key(self.user_public)["public_key"]
        blob = base64.b64decode(key.split()[1])
        invalid = [
            "",
            None,
            "ssh-rsa " + key.split()[1],
            'command="malicious" ' + key,
            key + "\n" + key,
            key + "\nattacker ssh-ed25519 AAAA",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "ssh-ed25519 !!!",
            "ssh-ed25519 " + base64.b64encode(blob[:-1]).decode("ascii"),
            "ssh-ed25519 " + base64.b64encode(blob + b"\x00").decode("ascii"),
            "ssh-ed25519 " + base64.b64encode(b"x" * len(blob)).decode("ascii"),
            key + "\x00injected",
        ]
        for value in invalid:
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    identity.normalize_public_key(value)

    def test_valid_proof_and_comment_normalization(self):
        self.assertTrue(identity.verify_signature(self.user_public, self.message, self.signature))
        public = identity.normalize_public_key(self.user_public)["public_key"] + " ignored-comment"
        self.assertTrue(identity.verify_signature(public, self.message, self.signature))

    def test_wrong_key_and_modified_build_request_fail(self):
        self.assertFalse(identity.verify_signature(self.other_public, self.message, self.signature))
        self.assertFalse(
            identity.verify_signature(self.user_public, self.message.replace(b"usb", b"igpu"), self.signature)
        )
        self.assertFalse(identity.verify_signature(self.user_public, self.message.rstrip(b"\n"), self.signature))

    def test_cross_namespace_signature_is_rejected(self):
        self.assertFalse(
            identity.verify_signature(
                self.user_public, self.message, self.signature, identity.RECEIPT_NAMESPACE
            )
        )
        signature = self.sign(self.user_key, self.message, "git")
        self.assertFalse(identity.verify_signature(self.user_public, self.message, signature))

    def test_malformed_signature_and_namespace_are_rejected(self):
        malformed = [
            "",
            "not a signature",
            self.signature.replace("BEGIN SSH SIGNATURE", "BEGIN PGP SIGNATURE"),
            "-----BEGIN SSH SIGNATURE-----\n!!!!\n-----END SSH SIGNATURE-----\n",
            "-----BEGIN SSH SIGNATURE-----\naW52YWxpZA==\n-----END SSH SIGNATURE-----\n",
            self.signature + "untrusted trailing data",
            self.signature.replace("\n", "\n\u00fc", 1),
        ]
        for signature in malformed:
            with self.subTest(signature=signature[:60]):
                with self.assertRaises(ValueError):
                    identity.verify_signature(self.user_public, self.message, signature)
        for namespace in ("", "bad\nnamespace", 'a" b', None, "a" * 129):
            with self.subTest(namespace=namespace):
                with self.assertRaises(ValueError):
                    identity.verify_signature(self.user_public, self.message, self.signature, namespace)

    def test_input_sizes_are_bounded(self):
        with self.assertRaises(ValueError):
            identity.normalize_public_key("a" * (identity.MAX_PUBLIC_KEY_BYTES + 1))
        with self.assertRaises(ValueError):
            identity.verify_signature(
                self.user_public, b"a" * (identity.MAX_MESSAGE_BYTES + 1), self.signature
            )
        with self.assertRaises(ValueError):
            identity.verify_signature(
                self.user_public, self.message, "a" * (identity.MAX_SIGNATURE_BYTES + 1)
            )
        with self.assertRaises(ValueError):
            identity.verify_signature(self.user_public, self.message.decode(), self.signature)

    def test_openssh_timeout_fails_closed(self):
        with patch.object(identity.subprocess, "run", side_effect=subprocess.TimeoutExpired("ssh-keygen", 10)):
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                identity.verify_signature(self.user_public, self.message, self.signature)

    def test_receipt_roundtrip_and_input_mutation_do_not_change_envelope(self):
        receipt = self.receipt()
        envelope = identity.sign_receipt(receipt, self.issuer_key)
        receipt["entitlements"].append("unissued-extra-right")
        self.assertTrue(identity.verify_receipt(envelope, self.issuer_public))
        self.assertEqual(envelope["receipt"]["entitlements"], ["build"])
        self.assertEqual(envelope["namespace"], identity.RECEIPT_NAMESPACE)

    def test_tampered_receipt_and_replaced_issuer_fail(self):
        envelope = identity.sign_receipt(self.receipt(), self.issuer_key)
        tampered = copy.deepcopy(envelope)
        tampered["receipt"]["artifact_sha256"] = "d" * 64
        self.assertFalse(identity.verify_receipt(tampered, self.issuer_public))
        replaced = copy.deepcopy(envelope)
        replaced["issuer_public_key"] = self.other_public
        self.assertFalse(identity.verify_receipt(replaced, self.issuer_public))
        self.assertFalse(identity.verify_receipt(envelope, self.other_public))

    def test_self_issued_and_issuer_impersonation_receipts_fail(self):
        forged = identity.sign_receipt(self.receipt(), self.user_key)
        self.assertFalse(identity.verify_receipt(forged, self.issuer_public))
        forged["issuer_public_key"] = identity.normalize_public_key(self.issuer_public)["public_key"]
        self.assertFalse(identity.verify_receipt(forged, self.issuer_public))

    def test_request_signature_cannot_issue_license(self):
        receipt = self.receipt()
        envelope = {
            "receipt": receipt,
            "signature": self.sign(self.issuer_key, identity.canonical_json(receipt)),
            "issuer_public_key": self.issuer_public,
            "namespace": identity.RECEIPT_NAMESPACE,
        }
        self.assertFalse(identity.verify_receipt(envelope, self.issuer_public))

    def test_malformed_receipt_envelopes_fail_closed(self):
        envelope = identity.sign_receipt(self.receipt(), self.issuer_key)
        cases = [None, {}, {**envelope, "namespace": identity.REQUEST_NAMESPACE}, {**envelope, "extra": True}]
        for field in ("receipt", "signature", "issuer_public_key"):
            cases.append({**envelope, field: None})
        for candidate in cases:
            with self.subTest(candidate_type=type(candidate).__name__):
                self.assertFalse(identity.verify_receipt(candidate, self.issuer_public))

    def test_issuer_key_must_be_a_real_private_key(self):
        with self.assertRaises(ValueError):
            identity.sign_receipt(self.receipt(), self.directory / "missing")
        with self.assertRaises(ValueError):
            identity.sign_receipt(self.receipt(), self.directory)
        with self.assertRaises(RuntimeError):
            identity.sign_receipt(self.receipt(), self.issuer_key.with_suffix(".pub"))


if __name__ == "__main__":
    unittest.main()
