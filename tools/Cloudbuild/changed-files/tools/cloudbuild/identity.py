"""OpenSSH-backed identity proofs and operator-signed build receipts.

Only ordinary SSH Ed25519 keys are supported. User private keys never enter
this module: ``verify_signature`` accepts a public key and detached SSH
signature. The separate receipt issuer is configured by the operator and must
not be made available to build workers.

``canonical_json`` is this service's byte encoding, not a claim to implement
RFC 8785. Callers must sign the exact bytes returned by it.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from typing import Any


REQUEST_NAMESPACE = "build-request@trueos.eu"
RECEIPT_NAMESPACE = "build-license@trueos.eu"
MAX_PUBLIC_KEY_BYTES = 8192
MAX_SIGNATURE_BYTES = 16384
MAX_MESSAGE_BYTES = 1024 * 1024
OPENSSH_TIMEOUT_SECONDS = 10

_ED25519_PREFIX = b"\x00\x00\x00\x0bssh-ed25519\x00\x00\x00\x20"
_SIGNATURE_BEGIN = b"-----BEGIN SSH SIGNATURE-----"
_SIGNATURE_END = b"-----END SSH SIGNATURE-----"
_SIGNER_IDENTITY = "trueos-key-holder"


def _validate_json_keys(value: Any) -> None:
    if isinstance(value, dict):
        if not all(isinstance(key, str) for key in value):
            raise ValueError("JSON object keys must be strings")
        for item in value.values():
            _validate_json_keys(item)
    elif isinstance(value, (list, tuple)):
        for item in value:
            _validate_json_keys(item)


def canonical_json(value: Any) -> bytes:
    """Encode sorted, compact ASCII JSON followed by exactly one newline."""
    try:
        _validate_json_keys(value)
        return (
            json.dumps(
                value,
                sort_keys=True,
                separators=(",", ":"),
                ensure_ascii=True,
                allow_nan=False,
            ).encode("ascii")
            + b"\n"
        )
    except (TypeError, ValueError, RecursionError) as exc:
        raise ValueError(
            "Value must contain JSON data with finite numbers and string object keys"
        ) from exc


def normalize_public_key(text: str) -> dict[str, str]:
    """Validate one SSH Ed25519 public key, remove its comment, and fingerprint it.

    The fingerprint hashes the SSH public-key wire blob, matching OpenSSH's
    SHA256 fingerprint format. This parses serialization only; OpenSSH performs
    every cryptographic signature operation.
    """
    if not isinstance(text, str):
        raise ValueError("Public key must be text")
    if len(text.encode("utf-8", errors="replace")) > MAX_PUBLIC_KEY_BYTES:
        raise ValueError("Public key is too large")
    line = text.strip()
    if not line or len(line.splitlines()) != 1:
        raise ValueError("Provide exactly one SSH Ed25519 public key")
    if any(ord(character) < 32 and character != "\t" for character in line):
        raise ValueError("Public key contains control characters")
    parts = line.split(maxsplit=2)
    if len(parts) < 2 or parts[0] != "ssh-ed25519":
        raise ValueError("Only ordinary ssh-ed25519 public keys are supported")
    try:
        blob = base64.b64decode(parts[1], validate=True)
    except (binascii.Error, ValueError) as exc:
        raise ValueError("Public key has invalid base64 encoding") from exc
    if len(blob) != len(_ED25519_PREFIX) + 32 or not blob.startswith(_ED25519_PREFIX):
        raise ValueError("Public key has an invalid SSH Ed25519 key structure")
    normalized = "ssh-ed25519 " + base64.b64encode(blob).decode("ascii")
    fingerprint = "SHA256:" + base64.b64encode(hashlib.sha256(blob).digest()).decode(
        "ascii"
    ).rstrip("=")
    return {"public_key": normalized, "fingerprint": fingerprint}


def _validate_message(message: bytes) -> None:
    if not isinstance(message, bytes):
        raise ValueError("Signed message must be exact bytes")
    if len(message) > MAX_MESSAGE_BYTES:
        raise ValueError("Signed message is too large")


def _validate_namespace(namespace: str) -> None:
    if not isinstance(namespace, str) or not re.fullmatch(
        r"[A-Za-z0-9][A-Za-z0-9@._:/+\-]{0,127}", namespace
    ):
        raise ValueError("Signature namespace must be a short printable identifier")


def _normalize_signature(signature: str) -> bytes:
    if not isinstance(signature, str):
        raise ValueError("Signature must be ASCII-armored SSH signature text")
    try:
        raw = signature.encode("ascii")
    except UnicodeEncodeError as exc:
        raise ValueError("SSH signature must contain ASCII text only") from exc
    if len(raw) > MAX_SIGNATURE_BYTES:
        raise ValueError("SSH signature is too large")
    lines = raw.strip().splitlines()
    if (
        len(lines) < 3
        or lines[0] != _SIGNATURE_BEGIN
        or lines[-1] != _SIGNATURE_END
        or any(not line for line in lines[1:-1])
    ):
        raise ValueError("Expected a complete armored SSH signature")
    body = b"".join(lines[1:-1])
    try:
        decoded = base64.b64decode(body, validate=True)
    except (binascii.Error, ValueError) as exc:
        raise ValueError("SSH signature has invalid base64 encoding") from exc
    if not decoded.startswith(b"SSHSIG\x00\x00\x00\x01"):
        raise ValueError("Expected an SSH signature with format version 1")
    # OpenSSH validates the remaining SSHSIG structure and cryptography. Fold
    # the armor consistently without interpreting or rebuilding signed fields.
    folded = b"\n".join(body[index : index + 70] for index in range(0, len(body), 70))
    return _SIGNATURE_BEGIN + b"\n" + folded + b"\n" + _SIGNATURE_END + b"\n"


def _ssh_keygen(arguments: list[str], message: bytes | None = None) -> subprocess.CompletedProcess:
    executable = shutil.which("ssh-keygen")
    if executable is None:
        raise RuntimeError("OpenSSH ssh-keygen is required for key proof verification")
    environment = os.environ.copy()
    environment["LC_ALL"] = "C"
    environment["SSH_ASKPASS_REQUIRE"] = "never"
    # Receipt signing must use the configured private key, without borrowing
    # an interactive operator's agent or prompting for credentials.
    for name in ("SSH_AUTH_SOCK", "SSH_ASKPASS", "DISPLAY"):
        environment.pop(name, None)
    try:
        return subprocess.run(
            [executable, *arguments],
            input=message if message is not None else b"",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=OPENSSH_TIMEOUT_SECONDS,
            check=False,
            env=environment,
            start_new_session=True,
        )
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError("OpenSSH signature operation timed out") from exc
    except OSError as exc:
        raise RuntimeError("OpenSSH ssh-keygen could not be executed") from exc


def verify_signature(
    public_key: str,
    message: bytes,
    signature: str,
    namespace: str = REQUEST_NAMESPACE,
) -> bool:
    """Verify exact bytes against one key and namespace, failing closed.

    Malformed or oversized inputs raise ``ValueError``. A failed OpenSSH
    verification (including a wrong key, altered bytes, or wrong namespace)
    returns ``False``. Missing OpenSSH or a timeout raises ``RuntimeError``.
    """
    key = normalize_public_key(public_key)["public_key"]
    _validate_message(message)
    _validate_namespace(namespace)
    armored = _normalize_signature(signature)
    with tempfile.TemporaryDirectory(prefix="trueos-identity-") as temporary:
        directory = Path(temporary)
        signers_path = directory / "allowed_signers"
        signature_path = directory / "proof.sig"
        signers_path.write_text(f"{_SIGNER_IDENTITY} {key}\n", encoding="ascii")
        signature_path.write_bytes(armored)
        result = _ssh_keygen(
            [
                "-Y",
                "verify",
                "-f",
                str(signers_path),
                "-I",
                _SIGNER_IDENTITY,
                "-n",
                namespace,
                "-s",
                str(signature_path),
            ],
            message,
        )
    return result.returncode == 0


def sign_receipt(receipt: dict, issuer_key: Path) -> dict:
    """Sign a receipt with an operator's unencrypted Ed25519 private key.

    The returned payload is a detached copy, so mutating the input later cannot
    silently change the signed envelope. The issuer key must remain outside
    build workers; this function does not grant an entitlement by itself.
    """
    if not isinstance(receipt, dict):
        raise ValueError("Receipt must be a JSON object")
    message = canonical_json(receipt)
    _validate_message(message)
    try:
        path = Path(issuer_key).resolve(strict=True)
    except (TypeError, ValueError, OSError) as exc:
        raise ValueError("Issuer private-key path must identify a readable file") from exc
    if not path.is_file():
        raise ValueError("Issuer private-key path must identify a file")
    public_result = _ssh_keygen(["-y", "-P", "", "-f", str(path)])
    if public_result.returncode != 0:
        raise RuntimeError(
            "Cannot read issuer key; configure a readable, unencrypted Ed25519 private key"
        )
    try:
        public_key = normalize_public_key(public_result.stdout.decode("ascii"))["public_key"]
    except (UnicodeDecodeError, ValueError) as exc:
        raise ValueError("Issuer key must be an ordinary SSH Ed25519 key") from exc
    result = _ssh_keygen(
        ["-Y", "sign", "-f", str(path), "-n", RECEIPT_NAMESPACE], message
    )
    if result.returncode != 0:
        raise RuntimeError("OpenSSH could not sign the build receipt")
    try:
        signature = _normalize_signature(result.stdout.decode("ascii")).decode("ascii")
    except (UnicodeDecodeError, ValueError) as exc:
        raise RuntimeError("OpenSSH did not return a valid SSH signature") from exc
    if not verify_signature(public_key, message, signature, RECEIPT_NAMESPACE):
        raise RuntimeError("Issued receipt did not verify against the configured issuer key")
    return {
        "receipt": json.loads(message),
        "signature": signature,
        "issuer_public_key": public_key,
        "namespace": RECEIPT_NAMESPACE,
    }


def verify_receipt(envelope: dict, trusted_public_key: str) -> bool:
    """Check an envelope against an out-of-band trusted operator public key.

    A key included in a received envelope is never its own trust anchor.
    Malformed envelopes return ``False``; unavailable OpenSSH still raises
    ``RuntimeError``. This verifies authenticity, not entitlement expiry or
    artifact hashes: the caller must validate those application-level fields.
    """
    if not isinstance(envelope, dict) or set(envelope) != {
        "receipt",
        "signature",
        "issuer_public_key",
        "namespace",
    }:
        return False
    if envelope["namespace"] != RECEIPT_NAMESPACE or not isinstance(envelope["receipt"], dict):
        return False
    try:
        trusted = normalize_public_key(trusted_public_key)["public_key"]
        claimed = normalize_public_key(envelope["issuer_public_key"])["public_key"]
        if claimed != trusted:
            return False
        return verify_signature(
            trusted,
            canonical_json(envelope["receipt"]),
            envelope["signature"],
            RECEIPT_NAMESPACE,
        )
    except (TypeError, ValueError):
        return False
