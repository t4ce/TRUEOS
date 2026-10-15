#!/usr/bin/env python3
"""Measure the real SQLite service with synthetic images and bounded requests.

Outputs contain operation labels, sizes and timings only. Reads, prepares and
failure cases must preserve the persisted image. Mutating scripts must retain
their existing publish-before-ack and rollback contracts.
"""
import argparse
import hashlib
import json
import itertools
import os
from pathlib import Path
import select
import socket
import sqlite3
import statistics
import struct
import subprocess
import tempfile
import time
import re

REQUEST_IDS = itertools.count(1)
LAST_REQUEST_ID = 0


def receive(sock, count):
    data = bytearray()
    while len(data) < count:
        chunk = sock.recv(count - len(data))
        if not chunk:
            raise AssertionError("synthetic session closed early")
        data.extend(chunk)
    return data


def connect(port, database, read_only=False):
    sock = socket.create_connection(("127.0.0.1", port), timeout=20)
    sock.sendall(b"GET /v1/connect HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: trueos-sqlite-v1\r\n\r\n")
    header = bytearray()
    while not header.endswith(b"\r\n\r\n"):
        header.extend(receive(sock, 1))
    assert header.startswith(b"HTTP/1.1 101")
    assert request(sock, op="open", database=database, create=True, read_only=read_only)[0]["error"] is None
    return sock


def request(sock, **command):
    global LAST_REQUEST_ID
    LAST_REQUEST_ID = next(REQUEST_IDS)
    command = {"connection_id": 1, "request_id": LAST_REQUEST_ID, **command}
    encoded = json.dumps(command).encode()
    started = time.perf_counter_ns()
    sock.sendall(struct.pack("!I", len(encoded)) + encoded)
    reply = json.loads(receive(sock, struct.unpack("!I", receive(sock, 4))[0]))
    return reply, (time.perf_counter_ns() - started) / 1_000_000


def execute(sock, sql, timeout_ms=5000):
    return request(sock, op="execute", sql=sql, params=[], timeout_ms=timeout_ms)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(binary, sizes, samples):
    results = {"service_sha256": digest(binary), "samples_per_operation": samples, "images": []}
    with tempfile.TemporaryDirectory(prefix="sqlite-foundation-") as folder:
        folder = Path(folder)
        instance = "foundation"
        directory = folder / instance
        directory.mkdir()
        for size in sizes:
            with sqlite3.connect(directory / f"image-{size}.sqlite") as db:
                db.executescript("CREATE TABLE payload(value BLOB); CREATE TABLE counter(value INTEGER); INSERT INTO counter VALUES(0);")
                db.execute("INSERT INTO payload VALUES(zeroblob(?))", (size * 1024 * 1024,))
        reclaim_path = directory / "reclaim.sqlite"
        with sqlite3.connect(reclaim_path) as db:
            db.executescript("PRAGMA auto_vacuum=INCREMENTAL; VACUUM; CREATE TABLE payload(value BLOB); CREATE TABLE counter(value INTEGER);")
            db.execute("INSERT INTO payload VALUES(zeroblob(?))", (72 * 1024 * 1024,))
            db.execute("DELETE FROM payload")
            reclaim_free_before = db.execute("PRAGMA freelist_count").fetchone()[0]
            assert reclaim_free_before * db.execute("PRAGMA page_size").fetchone()[0] >= 64 * 1024 * 1024
        with (folder / "stderr.log").open("w") as stderr:
            service = subprocess.Popen([str(binary), "--instance", instance, "--data-dir", str(folder)],
                                       stdout=subprocess.PIPE, stderr=stderr, text=True,
                                       env=os.environ | {"TRUEOS_SQLITE_TIMING_ALL": "1"})
            try:
                if not select.select([service.stdout], [], [], 20)[0]:
                    raise AssertionError("synthetic service readiness timed out")
                ready = service.stdout.readline().strip()
                assert ready.startswith("sqlite-service: READY ")
                advertisement = json.loads(ready.removeprefix("sqlite-service: READY "))
                port = int(advertisement["ipv4_endpoint"].rsplit(":", 1)[1])
                for size in sizes:
                    path = directory / f"image-{size}.sqlite"
                    before = digest(path)
                    modified = path.stat().st_mtime_ns
                    with connect(port, f"image-{size}") as writable, connect(port, f"image-{size}", True) as readonly:
                        image = {"payload_mib": size, "image_bytes": path.stat().st_size, "operations": {}}
                        for label, sock, command in [
                            ("prepare", writable, {"op": "prepare", "sql": "SELECT value FROM counter"}),
                            ("read_execute", writable, {"op": "execute", "sql": "SELECT value FROM counter", "params": [], "timeout_ms": 5000}),
                            ("readonly_execute", readonly, {"op": "execute", "sql": "SELECT value FROM counter", "params": [], "timeout_ms": 5000}),
                            ("ping", writable, {"op": "ping"}),
                        ]:
                            for _ in range(3):
                                assert request(sock, **command)[0]["error"] is None
                            durations = []
                            request_ids = []
                            for _ in range(samples):
                                reply, elapsed = request(sock, **command)
                                assert reply["error"] is None
                                durations.append(elapsed)
                                request_ids.append(LAST_REQUEST_ID)
                            durations.sort()
                            image["operations"][label] = {"median_ms": statistics.median(durations), "p95_ms": durations[min(len(durations) - 1, int(len(durations) * .95))], "max_ms": max(durations), "request_ids": request_ids}
                        assert digest(path) == before
                        assert path.stat().st_mtime_ns == modified
                        # No-write failures leave both the image and its state intact.
                        assert execute(writable, "SELECT * FROM absent_table")[0]["error"]
                        assert execute(readonly, "UPDATE counter SET value=1")[0]["error"]
                        assert digest(path) == before
                        assert execute(writable, "SELECT value FROM counter")[0]["results"][0]["rows"][0][0]["value"] == 0
                        request(writable, op="close")
                        request(readonly, op="close")
                        results["images"].append(image)
                with connect(port, "contracts") as sock:
                    path = directory / "contracts.sqlite"
                    # Later statements may depend on an earlier DDL statement.
                    assert execute(sock, "CREATE TABLE c(value INTEGER); INSERT INTO c VALUES(1)")[0]["error"] is None
                    assert path.exists(), "successful mutation was acknowledged before publication"
                    with sqlite3.connect(f"file:{path}?mode=ro", uri=True) as db:
                        assert db.execute("SELECT value FROM c").fetchone() == (1,)
                    before = digest(path)
                    assert execute(sock, "SELECT value FROM c; UPDATE c SET value=2; SELECT * FROM absent_table")[0]["error"]
                    assert digest(path) == before
                    assert execute(sock, "SELECT value FROM c")[0]["results"][0]["rows"][0][0]["value"] == 1
                    assert request(sock, op="begin", sql=None)[0]["depth"] == 1
                    assert execute(sock, "UPDATE c SET value=3")[0]["error"] is None
                    assert request(sock, op="rollback")[0]["depth"] == 0
                    assert digest(path) == before
                    assert request(sock, op="begin", sql=None)[0]["depth"] == 1
                    assert execute(sock, "UPDATE c SET value=4")[0]["error"] is None
                    pending = path.with_suffix(".pending")
                    pending.mkdir()
                    assert request(sock, op="commit")[0]["error"], "failed publication must be reported"
                    pending.rmdir()
                    assert digest(path) == before
                    assert execute(sock, "SELECT value FROM c")[0]["results"][0]["rows"][0][0]["value"] == 1
                    assert execute(sock, "WITH RECURSIVE x(v) AS (SELECT 1 UNION ALL SELECT v+1 FROM x WHERE v<100000000) SELECT sum(v) FROM x", timeout_ms=1)[0]["error"]
                    assert digest(path) == before
                    # SQLite may apply PRAGMA effects during preparation. A
                    # subsequent failed statement must still restore the image.
                    for pragma in ["user_version=23", "schema_version=31", "application_id=17", "page_size=8192", "encoding='UTF-16le'", "auto_vacuum=INCREMENTAL", "foreign_keys=OFF", "writable_schema=ON"]:
                        assert execute(sock, "PRAGMA " + pragma + "; SELECT * FROM absent_table")[0]["error"]
                        assert digest(path) == before
                        assert request(sock, op="prepare", sql="PRAGMA " + pragma + "; SELECT * FROM absent_table")[0]["error"]
                        assert digest(path) == before
                    request(sock, op="close")
                with connect(port, "reclaim") as sock:
                    assert execute(sock, "INSERT INTO counter VALUES(1)")[0]["error"] is None
                    request(sock, op="close")
                with sqlite3.connect(f"file:{reclaim_path}?mode=ro", uri=True) as db:
                    reclaimed = reclaim_free_before - db.execute("PRAGMA freelist_count").fetchone()[0]
                    assert 0 < reclaimed <= 64, "bounded owner reclamation did not run after mutation"
                    assert db.execute("SELECT value FROM counter").fetchone() == (1,)
                    assert db.execute("PRAGMA integrity_check").fetchone() == ("ok",)
                results["reclaimed_pages"] = reclaimed
                for oversized in [False, True]:
                    with connect(port, "contracts") as abandoned:
                        assert request(abandoned, op="begin", sql=None)[0]["depth"] == 1
                        assert execute(abandoned, "UPDATE c SET value=99")[0]["error"] is None
                        if oversized:
                            abandoned.sendall(struct.pack("!I", 8 * 1024 * 1024 + 1))
                        else:
                            abandoned.sendall(struct.pack("!I", 100) + b"{")
                        abandoned.shutdown(socket.SHUT_WR)
                        assert abandoned.recv(1) == b"", "malformed frame must close the session"
                    with connect(port, "contracts") as check:
                        assert execute(check, "SELECT value FROM c")[0]["results"][0]["rows"][0][0]["value"] == 1
                        request(check, op="close")
                    assert digest(path) == before
                results["contracts"] = "PASS readonly_no_publish failed_read_no_change ddl_then_write mixed_failure_rollback explicit_rollback commit_publication_failure_restore read_interruption prepare_pragma_failure_restore execute_pragma_failure_restore bounded_reclaim oversized_frame_rollback partial_frame_eof_rollback"
            finally:
                service.terminate()
                service.wait(timeout=10)
        timing = {}
        for line in (folder / "stderr.log").read_text().splitlines():
            if not line.startswith("sqlite-service-timing "):
                continue
            fields = {key: int(value) for key, value in re.findall(r"\b([a-z_]+)=([0-9]+)\b", line)}
            timing[fields["request_id"]] = fields
        for image in results["images"]:
            for operation in image["operations"].values():
                measurements = [timing[key] for key in operation.pop("request_ids") if key in timing]
                operation["server_samples"] = len(measurements)
                for field in ["elapsed_us", "lock_us", "queue_us", "before_us", "before_bytes", "execute_us", "reclaim_us", "after_us", "after_bytes", "publish_us", "publish_bytes", "reply_serialize_us", "reply_write_us"]:
                    values = [row[field] for row in measurements]
                    if values:
                        operation["server_" + field + "_median"] = statistics.median(values)
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--service", required=True, type=Path)
    parser.add_argument("--sizes", type=int, nargs="+", default=[1, 16, 64])
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    result = run(args.service.resolve(), args.sizes, args.samples)
    encoded = json.dumps(result, indent=2) + "\n"
    if args.report:
        args.report.write_text(encoded)
    print(encoded, end="")


if __name__ == "__main__":
    main()
