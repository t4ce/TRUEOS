#!/usr/bin/env python3
"""TRUEOS subsystem selector, build queue, key grants, and receipt verification."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
import threading
import time

from build_plan import ROOT, catalog, checkout_commit, resolve_request
from identity import canonical_json, normalize_public_key, verify_receipt
from service import HTTPServer, Service, Store, digest_file, strict_object


def bounded_json(path: Path):
    with path.open("rb") as stream:
        raw = stream.read(1024 * 1024 + 1)
    if len(raw) > 1024 * 1024:
        raise ValueError("JSON input exceeds 1 MiB")
    return json.loads(raw, object_pairs_hook=strict_object)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, default=ROOT / "tgt/cloudbuild",
                        help="private queue and artifact directory; use the same path for serve/worker/grant")
    commands = parser.add_subparsers(dest="operation", required=True)
    serve = commands.add_parser("serve", help="run the selector and control-plane HTTP service")
    serve.add_argument("--host", default="127.0.0.1")
    serve.add_argument("--port", type=int, default=8787)
    serve.add_argument("--origin", help="browser-facing origin; public origins must use HTTPS")
    serve.add_argument("--issuer-key", type=Path, help="operator Ed25519 private key, outside the builder")
    serve.add_argument("--allow-anonymous", action="store_true", help="explicitly permit unsigned, unlicensed builds")
    serve.add_argument("--source-commit", help="immutable revision provisioned on the worker; defaults to this checkout")
    worker = commands.add_parser("worker", help="build queued manifests on a dedicated cloud worker")
    worker.add_argument("--repository", type=Path, default=ROOT)
    worker.add_argument("--once", action="store_true")
    worker.add_argument("--timeout", type=int, default=7200)
    actions = commands.add_parser("actions-worker", help="dispatch queued builds to GitHub Actions and collect verified artifacts")
    actions.add_argument("--config", type=Path, required=True, help="operator repository, ref, source and dependency pins")
    actions.add_argument("--app-client-id", required=True)
    actions.add_argument("--installation-id", required=True)
    actions.add_argument("--app-key", type=Path, required=True, help="GitHub App RSA private key; never mounted into runners")
    actions.add_argument("--once", action="store_true", help="perform one dispatch/poll cycle")
    grant = commands.add_parser("grant", help="operator-only local entitlement administration")
    grant.add_argument("--public-key", type=Path, required=True)
    grant.add_argument("--license-id", required=True)
    grant.add_argument("--features", default="all", help="all, or comma-separated feature IDs")
    grant.add_argument("--expires-at", type=int, help="optional future Unix timestamp")
    revoke = commands.add_parser("revoke", help="revoke a key's grant for new builds and unissued receipts")
    revoke.add_argument("--fingerprint", required=True)
    plan = commands.add_parser("plan", help="resolve a selector request without running a build")
    plan.add_argument("--request", type=Path, required=True)
    plan.add_argument("--output", type=Path)
    verify = commands.add_parser("verify-receipt", help="verify issuer, entitlement scope, subject, and optional artifact")
    verify.add_argument("--receipt", type=Path, required=True)
    verify.add_argument("--issuer-public-key", type=Path, required=True,
                        help="trusted key obtained independently of the receipt")
    verify.add_argument("--subject-public-key", type=Path, help="check that this is your receipt")
    verify.add_argument("--artifact", type=Path, help="check the downloaded artifact's SHA-256 and size")
    args = parser.parse_args()
    try:
        if args.operation == "plan":
            result = resolve_request(bounded_json(args.request), checkout_commit())
            data = canonical_json(result)
            if args.output:
                args.output.write_bytes(data)
            else:
                sys.stdout.buffer.write(data)
            return
        if args.operation == "verify-receipt":
            envelope = bounded_json(args.receipt)
            if not verify_receipt(envelope, args.issuer_public_key.read_text()):
                raise ValueError("Receipt signature or trusted issuer does not match")
            receipt = envelope["receipt"]
            if receipt.get("schema") != "trueos-build-license/v1" or receipt.get("scope") != "official-personal-build":
                raise ValueError("Unsupported receipt schema or scope")
            expiry = receipt.get("expires_at")
            if expiry is not None and (type(expiry) is not int or time.time() >= expiry):
                raise ValueError("Receipt has expired")
            if args.subject_public_key and normalize_public_key(args.subject_public_key.read_text())["fingerprint"] != receipt.get("subject_key_fingerprint"):
                raise ValueError("Receipt belongs to a different public key")
            if args.artifact and (digest_file(args.artifact) != receipt.get("artifact_sha256") or args.artifact.stat().st_size != receipt.get("artifact_size")):
                raise ValueError("Artifact digest or size does not match the receipt")
            print("Receipt verified against the trusted issuer." + (" Artifact digest and size match." if args.artifact else ""))
            return
        store = Store(args.state_dir)
        if args.operation == "grant":
            features = [item["id"] for item in catalog()["features"]] if args.features == "all" else args.features.split(",") if args.features else []
            print(store.grant(args.public_key.read_text(), args.license_id, features, args.expires_at))
        elif args.operation == "revoke":
            store.revoke(args.fingerprint)
            print("Grant revoked. Previously issued offline receipts retain their signed validity period.")
        elif args.operation == "worker":
            if args.timeout < 1 or args.timeout > 86400:
                raise ValueError("worker timeout must be 1–86400 seconds")
            from worker import work_forever
            work_forever(store, args.repository, once=args.once, timeout=args.timeout)
        elif args.operation == "actions-worker":
            from github_actions import AppToken, GitHub, read_config, work_forever
            config = read_config(args.config)
            token = AppToken(args.app_client_id, args.installation_id, args.app_key)
            work_forever(store, config, GitHub(token), once=args.once)
        elif args.operation == "serve":
            if not 1 <= args.port <= 65535:
                raise ValueError("port must be between 1 and 65535")
            if args.host not in {"127.0.0.1", "localhost"} and not args.origin:
                raise ValueError("Set an explicit HTTPS --origin when binding a non-loopback interface")
            origin = args.origin or f"http://{args.host}:{args.port}"
            service = Service(store, args.source_commit or checkout_commit(), origin,
                              issuer_key=args.issuer_key, allow_anonymous=args.allow_anonymous)
            server = HTTPServer((args.host, args.port), service)
            stopped = threading.Event()
            finalizer = threading.Thread(target=service.finalize_loop, args=(stopped,), daemon=True)
            finalizer.start()
            print(f"TRUEOS Cloud Build: {service.origin}\nRun the worker separately to process queued builds.", flush=True)
            try:
                server.serve_forever()
            finally:
                stopped.set()
                server.server_close()
    except KeyboardInterrupt:
        pass
    except (OSError, ValueError, RuntimeError) as exc:
        parser.exit(2, f"cloudbuild: {exc}\n")


if __name__ == "__main__":
    main()
