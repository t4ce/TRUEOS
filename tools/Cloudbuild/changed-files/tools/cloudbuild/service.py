"""Local control plane for allowlisted TRUEOS builds and key-bound receipts.

The HTTP process holds the issuer key; worker.py never receives it. Deploy the
HTTP adapter behind a TLS reverse proxy with request/body/connection limits.
The SQLite queue and artifact directory are private operator-owned state.
"""
from __future__ import annotations

import hashlib
import hmac
import json
import re
import secrets
import sqlite3
import threading
import time
from collections import defaultdict, deque
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

from build_plan import catalog, resolve_request, validate_manifest
from identity import canonical_json, normalize_public_key, sign_receipt, verify_signature

NAMESPACE = "build-request@trueos.eu"
CHALLENGE_TTL = 300
MAX_BODY = 32_768
ID_RE = re.compile(r"[a-f0-9]{32}\Z")
TOKEN_RE = re.compile(r"[A-Za-z0-9_-]{43}\Z")
WEB = Path(__file__).parent / "web"


def strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Duplicate JSON fields are not accepted")
        result[key] = value
    return result


class APIError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(message)
        self.status = status


def digest_file(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


class Store:
    def __init__(self, directory: Path, clock=time.time):
        self.directory = directory.resolve()
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.directory.chmod(0o700)
        self.database = self.directory / "queue.sqlite3"
        self.clock = clock
        with self.connect() as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.executescript("""
                CREATE TABLE IF NOT EXISTS grants (
                    fingerprint TEXT PRIMARY KEY, public_key TEXT NOT NULL,
                    license_id TEXT NOT NULL, features TEXT NOT NULL,
                    expires_at INTEGER, revoked INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS challenges (
                    id TEXT PRIMARY KEY, payload TEXT NOT NULL,
                    public_key TEXT NOT NULL, fingerprint TEXT NOT NULL,
                    manifest TEXT NOT NULL, expires_at INTEGER NOT NULL,
                    used_at INTEGER
                );
                CREATE TABLE IF NOT EXISTS jobs (
                    id TEXT PRIMARY KEY, challenge_id TEXT UNIQUE,
                    access_hash TEXT NOT NULL, status TEXT NOT NULL,
                    manifest TEXT NOT NULL, fingerprint TEXT, license_id TEXT,
                    created_at INTEGER NOT NULL, started_at INTEGER,
                    finished_at INTEGER, error TEXT, artifact_sha256 TEXT,
                    artifact_size INTEGER, provenance TEXT, receipt TEXT
                );
            """)
        self.database.chmod(0o600)

    @contextmanager
    def connect(self, transaction=False):
        db = sqlite3.connect(self.database, timeout=15, isolation_level=None)
        db.row_factory = sqlite3.Row
        try:
            if transaction:
                db.execute("BEGIN IMMEDIATE")
            yield db
            if transaction:
                db.commit()
        except BaseException:
            if transaction:
                db.rollback()
            raise
        finally:
            db.close()

    def grant(self, public_key: str, license_id: str, features: list[str], expires_at=None):
        key = normalize_public_key(public_key)
        if not isinstance(license_id, str) or not 1 <= len(license_id) <= 128:
            raise ValueError("license_id must contain 1–128 characters")
        if not license_id.isprintable():
            raise ValueError("license_id must not contain control characters")
        known = {item["id"] for item in catalog()["features"]}
        if not isinstance(features, list) or any(not isinstance(x, str) for x in features):
            raise ValueError("grant features must be a list of feature IDs")
        if set(features) - known:
            raise ValueError("grant contains unknown features")
        if expires_at is not None and (type(expires_at) is not int or expires_at <= self.clock()):
            raise ValueError("grant expiry must be a future Unix timestamp")
        with self.connect(transaction=True) as db:
            db.execute("""INSERT INTO grants VALUES (?,?,?,?,?,0)
                ON CONFLICT(fingerprint) DO UPDATE SET public_key=excluded.public_key,
                license_id=excluded.license_id, features=excluded.features,
                expires_at=excluded.expires_at, revoked=0""",
                (key["fingerprint"], key["public_key"], license_id,
                 json.dumps(sorted(set(features))), expires_at))
        return key["fingerprint"]

    def revoke(self, fingerprint: str):
        with self.connect(transaction=True) as db:
            if db.execute("UPDATE grants SET revoked=1 WHERE fingerprint=?", (fingerprint,)).rowcount != 1:
                raise ValueError("No grant exists for that fingerprint")

    def authorize(self, db, fingerprint, manifest, now, expected_license=None):
        grant = db.execute("SELECT * FROM grants WHERE fingerprint=?", (fingerprint,)).fetchone()
        # Bundled apps are an image-packaging choice, not a new platform
        # capability. Existing feature grants retain their original meaning.
        packaging = {item["id"] for item in catalog()["features"] if item.get("kind") == "packaging"}
        requested_capabilities = set(manifest["features"]) - packaging
        if (grant is None or grant["revoked"] or
            (grant["expires_at"] is not None and now >= grant["expires_at"]) or
            not requested_capabilities.issubset(json.loads(grant["features"])) or
            (expected_license is not None and grant["license_id"] != expected_license)):
            raise APIError(403, "This key has no active grant for the selected features. Contact the operator.")
        return grant

    def new_challenge(self, manifest, key, audience):
        now = int(self.clock())
        identifier = secrets.token_hex(16)
        expires = now + CHALLENGE_TTL
        payload = canonical_json({
            "schema": "trueos-build-request/v1", "operation": "request-build",
            "audience": audience, "challenge_id": identifier,
            "nonce": secrets.token_urlsafe(32), "key_fingerprint": key["fingerprint"],
            "source_commit": manifest["source_commit"], "config_hash": manifest["config_hash"],
            "manifest": manifest, "issued_at": now, "expires_at": expires,
        }).decode("ascii")
        with self.connect(transaction=True) as db:
            # Expired challenges cannot authorize a request even after removal.
            db.execute("DELETE FROM challenges WHERE expires_at < ?", (now - 86400,))
            if db.execute("SELECT count(*) FROM challenges WHERE expires_at > ? AND used_at IS NULL", (now,)).fetchone()[0] >= 256:
                raise APIError(429, "Too many pending challenges; try again after some expire.")
            db.execute("INSERT INTO challenges VALUES (?,?,?,?,?,?,NULL)",
                       (identifier, payload, key["public_key"], key["fingerprint"],
                        json.dumps(manifest), expires))
        return {"challenge_id": identifier, "challenge": payload, "expires_at": expires, "ttl_seconds": CHALLENGE_TTL,
                "fingerprint": key["fingerprint"], "manifest": manifest, "namespace": NAMESPACE}

    def _insert_job(self, db, manifest, fingerprint=None, license_id=None, challenge_id=None):
        if db.execute("SELECT count(*) FROM jobs WHERE status IN ('queued','building','built')").fetchone()[0] >= 16:
            raise APIError(429, "Build queue is full; try again later.")
        if fingerprint and db.execute("SELECT count(*) FROM jobs WHERE fingerprint=? AND status IN ('queued','building','built')", (fingerprint,)).fetchone()[0] >= 2:
            raise APIError(429, "This key already has two pending builds.")
        identifier, token = secrets.token_hex(16), secrets.token_urlsafe(32)
        db.execute("""INSERT INTO jobs
            (id,challenge_id,access_hash,status,manifest,fingerprint,license_id,created_at)
            VALUES (?,?,?,'queued',?,?,?,?)""",
            (identifier, challenge_id, hashlib.sha256(token.encode()).hexdigest(),
             json.dumps(manifest), fingerprint, license_id, int(self.clock())))
        return {"job_id": identifier, "access_token": token, "status": "queued", "manifest": manifest}

    def queue_anonymous(self, manifest):
        with self.connect(transaction=True) as db:
            return self._insert_job(db, manifest)

    def verify_and_queue(self, identifier, signature, *, audience=None, source_commit=None):
        if not isinstance(identifier, str) or not ID_RE.fullmatch(identifier):
            raise APIError(400, "Invalid challenge ID")
        with self.connect() as db:
            challenge = db.execute("SELECT * FROM challenges WHERE id=?", (identifier,)).fetchone()
        self._check_challenge(challenge)
        payload = json.loads(challenge["payload"])
        if (audience is not None and payload["audience"] != audience) or (source_commit is not None and payload["source_commit"] != source_commit):
            raise APIError(409, "The service origin or source revision changed; request a new challenge.")
        if (payload.get("operation") != "request-build" or payload.get("challenge_id") != identifier or
            payload.get("key_fingerprint") != challenge["fingerprint"] or
            payload.get("manifest") != json.loads(challenge["manifest"])):
            raise APIError(409, "Challenge binding is invalid; request a new challenge.")
        if not verify_signature(challenge["public_key"], challenge["payload"].encode("ascii"), signature, NAMESPACE):
            raise APIError(403, "The signature does not match this key and challenge.")
        # Cryptography runs outside the write transaction. Recheck expiry and
        # consumption inside it, so two concurrent proofs create at most one job.
        with self.connect(transaction=True) as db:
            current = db.execute("SELECT * FROM challenges WHERE id=?", (identifier,)).fetchone()
            self._check_challenge(current)
            if dict(current) != dict(challenge):
                raise APIError(409, "Challenge changed during verification; request a new challenge.")
            manifest = validate_manifest(json.loads(current["manifest"]), source_commit)
            grant = self.authorize(db, current["fingerprint"], manifest, int(self.clock()))
            result = self._insert_job(db, manifest, current["fingerprint"], grant["license_id"], identifier)
            db.execute("UPDATE challenges SET used_at=? WHERE id=?", (int(self.clock()), identifier))
            return result

    def _check_challenge(self, challenge):
        if challenge is None:
            raise APIError(404, "Challenge not found; request a new one.")
        if challenge["used_at"] is not None:
            raise APIError(409, "This challenge has already been used.")
        if self.clock() >= challenge["expires_at"]:
            raise APIError(410, "Challenge expired; request a new one.")

    def owned_job(self, identifier, token):
        if not isinstance(identifier, str) or not ID_RE.fullmatch(identifier):
            raise APIError(404, "Build not found")
        if not isinstance(token, str) or not TOKEN_RE.fullmatch(token):
            raise APIError(401, "A build access token is required.")
        with self.connect() as db:
            row = db.execute("SELECT * FROM jobs WHERE id=?", (identifier,)).fetchone()
        if row is None or not hmac.compare_digest(row["access_hash"], hashlib.sha256(token.encode()).hexdigest()):
            raise APIError(404, "Build not found")
        return dict(row)

    def artifact_path(self, identifier):
        if not ID_RE.fullmatch(identifier):
            raise ValueError("Invalid job ID")
        return self.directory / "artifacts" / identifier / "TrueOS.tar.lz4"

    def claim(self):
        with self.connect(transaction=True) as db:
            row = db.execute("SELECT * FROM jobs WHERE status='queued' ORDER BY created_at,id LIMIT 1").fetchone()
            if row is None:
                return None
            db.execute("UPDATE jobs SET status='building',started_at=? WHERE id=?", (int(self.clock()), row["id"]))
            return dict(row)

    def finish_build(self, identifier, sha256, size, provenance):
        with self.connect(transaction=True) as db:
            db.execute("""UPDATE jobs SET status=CASE WHEN fingerprint IS NULL THEN 'succeeded' ELSE 'built' END,
                artifact_sha256=?,artifact_size=?,provenance=?,finished_at=?
                WHERE id=? AND status='building'""",
                (sha256, size, json.dumps(provenance), int(self.clock()), identifier))

    def fail(self, identifier, message):
        with self.connect(transaction=True) as db:
            db.execute("UPDATE jobs SET status='failed',error=?,finished_at=? WHERE id=? AND status != 'succeeded'",
                       (message[:500], int(self.clock()), identifier))


class Service:
    def __init__(self, store, source_commit, origin, *, issuer_key=None, allow_anonymous=False):
        parsed = urlsplit(origin)
        if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username or parsed.password or parsed.path not in {"", "/"} or parsed.query or parsed.fragment:
            raise ValueError("origin must be a single http(s) origin with no credentials or path")
        if parsed.scheme == "http" and parsed.hostname not in {"localhost", "127.0.0.1", "::1"}:
            raise ValueError("A public origin must use HTTPS through a TLS reverse proxy")
        self.origin = origin.rstrip("/")
        self.host = parsed.netloc.lower()
        self.store = store
        self.source_commit = source_commit
        resolve_request({}, source_commit)  # Validate immutable source revision.
        self.issuer_key = Path(issuer_key).resolve() if issuer_key else None
        if self.issuer_key and not self.issuer_key.is_file():
            raise ValueError("Issuer private key does not exist")
        self.allow_anonymous = allow_anonymous
        self.finalize_lock = threading.Lock()
        self.rate_lock = threading.Lock()
        self.requests = defaultdict(deque)

    def rate_limit(self, ip):
        now = time.monotonic()
        with self.rate_lock:
            # Bound tracking memory even for rotating client addresses.
            for key in list(self.requests):
                if not self.requests[key] or self.requests[key][-1] <= now - 60:
                    del self.requests[key]
            recent = self.requests[ip]
            while recent and recent[0] <= now - 60:
                recent.popleft()
            if len(recent) >= 20 or len(self.requests) > 4096:
                raise APIError(429, "Too many requests; wait a minute before retrying.")
            recent.append(now)

    def info(self):
        return {"catalog": catalog(), "source_commit": self.source_commit, "namespace": NAMESPACE,
                "capabilities": {"anonymous": self.allow_anonymous, "licensed": bool(self.issuer_key),
                                 "worker_mode": "separate-worker"}}

    @staticmethod
    def fields(body, required, optional=()):
        if not isinstance(body, dict) or set(body) - set(required) - set(optional) or set(required) - set(body):
            raise APIError(400, "Unexpected or missing request fields")

    def post(self, path, body):
        if path == "/api/plan":
            return resolve_request(body, self.source_commit)
        if path == "/api/challenges":
            self.fields(body, {"request", "public_key"})
            if not self.issuer_key:
                raise APIError(503, "The operator has not configured a license issuer.")
            manifest = resolve_request(body["request"], self.source_commit)
            key = normalize_public_key(body["public_key"])
            return self.store.new_challenge(manifest, key, self.origin)
        if path == "/api/builds":
            if isinstance(body, dict) and "challenge_id" in body:
                self.fields(body, {"challenge_id", "signature"})
                if not self.issuer_key:
                    raise APIError(503, "The operator has not configured a license issuer.")
                return self.store.verify_and_queue(body["challenge_id"], body["signature"],
                                                   audience=self.origin, source_commit=self.source_commit)
            self.fields(body, {"request"})
            if not self.allow_anonymous:
                raise APIError(403, "Anonymous builds are disabled. Use a key with an active grant.")
            return self.store.queue_anonymous(resolve_request(body["request"], self.source_commit))
        raise APIError(404, "Endpoint not found")

    def get_job(self, identifier, token):
        row = self.store.owned_job(identifier, token)
        if row["status"] == "built":
            with self.finalize_lock:
                row = self.store.owned_job(identifier, token)
                if row["status"] == "built":
                    self.finalize(row)
                    row = self.store.owned_job(identifier, token)
        result = {"job_id": row["id"], "status": "building" if row["status"] == "built" else row["status"],
                  "manifest": json.loads(row["manifest"]), "created_at": row["created_at"],
                  "key_fingerprint": row["fingerprint"]}
        if row["status"] == "succeeded":
            result["artifact"] = {"filename": "TrueOS.tar.lz4", "sha256": row["artifact_sha256"], "size": row["artifact_size"]}
            result["provenance"] = json.loads(row["provenance"])
            if row["receipt"]:
                result["receipt"] = json.loads(row["receipt"])
        if row["error"]:
            result["error"] = row["error"]
        return result

    def finalize(self, row):
        if not self.issuer_key:
            raise APIError(503, "License signing is unavailable; the built artifact is retained.")
        try:
            manifest = validate_manifest(json.loads(row["manifest"]))
        except ValueError:
            # A service upgrade must not reinterpret an old signed request or
            # stop the finalizer for other jobs. Resolve and sign a new plan.
            self.store.fail(row["id"], "Build manifest is no longer valid; resolve the configuration and request a fresh build.")
            return
        try:
            with self.store.connect() as db:
                grant = self.store.authorize(db, row["fingerprint"], manifest, int(self.store.clock()), row["license_id"])
            artifact = self.store.artifact_path(row["id"])
            if not artifact.is_file() or artifact.stat().st_size != row["artifact_size"] or digest_file(artifact) != row["artifact_sha256"]:
                raise ValueError("Artifact integrity check failed")
            receipt = {"schema": "trueos-build-license/v1", "license_id": row["license_id"],
                       "subject_key_fingerprint": row["fingerprint"], "job_id": row["id"],
                       "source_commit": manifest["source_commit"], "config_hash": manifest["config_hash"],
                       "features": manifest["features"], "buildins": manifest["buildins"],
                       "profile": manifest["profile"],
                       "artifact_sha256": row["artifact_sha256"], "artifact_size": row["artifact_size"],
                       "issued_at": int(self.store.clock()), "expires_at": grant["expires_at"],
                       "issuer_origin": self.origin, "scope": "official-personal-build",
                       "provenance": json.loads(row["provenance"])}
            envelope = sign_receipt(receipt, self.issuer_key)
            with self.store.connect(transaction=True) as db:
                # A grant may be revoked while hashing/signing the artifact.
                self.store.authorize(db, row["fingerprint"], manifest, int(self.store.clock()), row["license_id"])
                db.execute("UPDATE jobs SET status='succeeded',receipt=? WHERE id=? AND status='built'",
                           (json.dumps(envelope), row["id"]))
        except APIError:
            self.store.fail(row["id"], "The license grant expired, was revoked, or changed before issuance.")
        except (OSError, ValueError, RuntimeError):
            # Do not expose filesystem paths, OpenSSH stderr, or key details.
            raise APIError(503, "Receipt signing failed. The operator must check the issuer and artifact; the build is retained.")

    def finalize_pending(self):
        """Issue completed receipts even if the requesting browser is closed."""
        if not self.issuer_key:
            return
        with self.finalize_lock:
            with self.store.connect() as db:
                rows = [dict(row) for row in db.execute("SELECT * FROM jobs WHERE status='built' ORDER BY finished_at,id LIMIT 16")]
            for row in rows:
                try:
                    self.finalize(row)
                except APIError:
                    # An unavailable signer retains the artifact for another
                    # attempt, without blocking other jobs.
                    continue

    def finalize_loop(self, stopped):
        while not stopped.is_set():
            try:
                self.finalize_pending()
            except (OSError, sqlite3.Error):
                pass  # The next pass retries temporary storage failures.
            stopped.wait(2)


class HTTPServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address, service):
        self.service = service
        self.slots = threading.BoundedSemaphore(16)
        super().__init__(address, Handler)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            try:
                request.sendall(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            finally:
                self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            request.settimeout(15)
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()


class Handler(BaseHTTPRequestHandler):
    server_version = "TRUEOS-CloudBuild/1"

    def log_message(self, format, *args):
        pass  # Request paths, signatures and bearer tokens must not enter logs.

    def headers_ok(self):
        hosts = self.headers.get_all("Host", [])
        if len(hosts) != 1 or hosts[0].lower() != self.server.service.host:
            raise APIError(403, "Unexpected Host header")
        origins = self.headers.get_all("Origin", [])
        if len(origins) > 1:
            raise APIError(403, "Ambiguous Origin header")
        origin = origins[0] if origins else None
        if origin and origin != self.server.service.origin:
            raise APIError(403, "Cross-origin requests are not accepted")

    def common_headers(self, content_type, length):
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(length))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")

    def json_response(self, status, value):
        payload = canonical_json(value)
        self.send_response(status)
        self.common_headers("application/json; charset=utf-8", len(payload))
        self.end_headers()
        self.wfile.write(payload)

    def dispatch(self, method):
        try:
            self.headers_ok()
            path = urlsplit(self.path).path
            service = self.server.service
            if method == "POST":
                # Plan validation is cheap and supports responsive checkbox use.
                if path != "/api/plan":
                    service.rate_limit(self.client_address[0])
                if self.headers.get_content_type() != "application/json":
                    raise APIError(415, "Use Content-Type: application/json")
                if self.headers.get("Transfer-Encoding"):
                    raise APIError(400, "Transfer-Encoding is not supported")
                lengths = self.headers.get_all("Content-Length", [])
                if len(lengths) != 1 or not lengths[0].isdigit():
                    raise APIError(411, "One Content-Length header is required")
                length = int(lengths[0])
                if length > MAX_BODY:
                    raise APIError(413, "Request is too large")
                raw = self.rfile.read(length)
                if len(raw) != length:
                    raise APIError(400, "Incomplete request")
                body = json.loads(raw, object_pairs_hook=strict_object)
                self.json_response(201 if path in {"/api/challenges", "/api/builds"} else 200, service.post(path, body))
            elif path == "/api/catalog":
                self.json_response(200, service.info())
            elif path.startswith("/api/jobs/"):
                parts = path.split("/")
                if len(parts) not in {4, 5} or (len(parts) == 5 and parts[4] != "artifact"):
                    raise APIError(404, "Endpoint not found")
                header = self.headers.get("Authorization", "")
                token = header[7:] if header.startswith("Bearer ") else ""
                result = service.get_job(parts[3], token)
                if len(parts) == 4:
                    self.json_response(200, result)
                else:
                    if result["status"] != "succeeded":
                        raise APIError(409, "The artifact is not ready")
                    artifact = service.store.artifact_path(parts[3])
                    with artifact.open("rb") as stream:
                        self.send_response(200)
                        self.common_headers("application/octet-stream", artifact.stat().st_size)
                        self.send_header("Content-Disposition", 'attachment; filename="TrueOS.tar.lz4"')
                        self.end_headers()
                        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                            self.wfile.write(chunk)
            elif path in {"/", "/index.html", "/app.js", "/styles.css"}:
                name = "index.html" if path == "/" else path[1:]
                data = (WEB / name).read_bytes()
                types = {"html": "text/html", "js": "text/javascript", "css": "text/css"}
                self.send_response(200)
                self.common_headers(types[name.rsplit(".", 1)[1]] + "; charset=utf-8", len(data))
                self.end_headers()
                self.wfile.write(data)
            else:
                raise APIError(404, "Endpoint not found")
        except APIError as exc:
            self.json_response(exc.status, {"error": str(exc)})
        except (ValueError, TypeError, UnicodeError) as exc:
            self.json_response(400, {"error": str(exc)[:300]})
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            self.close_connection = True
        except Exception:
            self.json_response(500, {"error": "The operator must check the service configuration."})

    def do_GET(self):
        self.dispatch("GET")

    def do_POST(self):
        self.dispatch("POST")
