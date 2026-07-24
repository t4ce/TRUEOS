#!/usr/bin/env python3
"""Press the test rig's ESP32-controlled physical reset button via UDP."""

from __future__ import annotations

import argparse
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import socket
import sys
import time
from typing import Any


PROBE = b"probe"
ACK = b"ack"


def positive_timeout(value: str) -> float:
    parsed = float(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("timeout must be greater than zero")
    return parsed


def udp_port(value: str) -> int:
    parsed = int(value)
    if not 1 <= parsed <= 65535:
        raise argparse.ArgumentTypeError("port must be in 1..65535")
    return parsed


def ipv4_address(value: str) -> str:
    try:
        parsed = ipaddress.ip_address(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(str(exc)) from exc
    if parsed.version != 4:
        raise argparse.ArgumentTypeError("address must be IPv4")
    return str(parsed)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_watch(value: str) -> tuple[Path, str]:
    try:
        path_text, expected = value.rsplit("=", 1)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("watch must be PATH=SHA256") from exc
    if not path_text:
        raise argparse.ArgumentTypeError("watch path cannot be empty")
    expected = expected.lower()
    if len(expected) != 64 or any(char not in "0123456789abcdef" for char in expected):
        raise argparse.ArgumentTypeError("watch SHA256 must be 64 lowercase hex digits")
    return Path(path_text).resolve(), expected


def parse_identity(value: str) -> tuple[str, str]:
    try:
        key, identity_value = value.split("=", 1)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("identity must be KEY=VALUE") from exc
    if not key or not key.replace("_", "").isalnum():
        raise argparse.ArgumentTypeError("identity key must be alphanumeric/underscore")
    return key, identity_value


def write_receipt(path: Path | None, record: dict[str, Any]) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def prepare_watch(path: Path, expected_sha256: str) -> dict[str, Any]:
    original = path.stat()
    if not path.is_file():
        raise RuntimeError(f"PXE watch path is not a regular file: {path}")
    preflight_sha256 = sha256_file(path)
    after_hash = path.stat()
    if (
        after_hash.st_size != original.st_size
        or after_hash.st_mtime_ns != original.st_mtime_ns
    ):
        raise RuntimeError(f"PXE artifact changed during preflight hashing: {path}")
    if preflight_sha256 != expected_sha256:
        raise RuntimeError(
            f"PXE artifact identity mismatch before reset for {path}: "
            f"expected {expected_sha256}, got {preflight_sha256}"
        )
    return {
        "path": str(path),
        "expected_sha256": expected_sha256,
        "preflight_sha256": preflight_sha256,
        "size": original.st_size,
        "mtime_ns": original.st_mtime_ns,
        "original_atime_ns": original.st_atime_ns,
        "armed_atime_ns": None,
        "observed_atime_ns": None,
        "verified_sha256": None,
    }


def arm_pxe_read_witness(watches: list[dict[str, Any]]) -> None:
    """Make relatime update atime on the next content read without touching mtime."""
    for watch in watches:
        path = Path(watch["path"])
        before = path.stat()
        if before.st_size != watch["size"] or before.st_mtime_ns != watch["mtime_ns"]:
            raise RuntimeError(f"PXE artifact changed before witness arming: {path}")

        # Linux relatime updates atime when the stored atime is <= mtime.
        # Preserve mtime exactly so PXE publication identity remains stable.
        sentinel_atime_ns = before.st_mtime_ns
        os.utime(
            path,
            ns=(sentinel_atime_ns, before.st_mtime_ns),
            follow_symlinks=True,
        )
        armed = path.stat()
        if armed.st_size != watch["size"] or armed.st_mtime_ns != watch["mtime_ns"]:
            raise RuntimeError(f"arming the PXE read witness changed artifact metadata: {path}")
        if armed.st_atime_ns > armed.st_mtime_ns:
            raise RuntimeError(
                f"filesystem did not arm a relatime witness for {path}: "
                f"atime_ns={armed.st_atime_ns} mtime_ns={armed.st_mtime_ns}"
            )
        watch["armed_atime_ns"] = armed.st_atime_ns


def wait_for_pxe_reads(watches: list[dict[str, Any]], *, timeout: float) -> None:
    if not watches:
        return

    deadline = time.monotonic() + timeout
    pending = {watch["path"] for watch in watches}
    while pending and time.monotonic() < deadline:
        for watch in watches:
            path_text = watch["path"]
            if path_text not in pending:
                continue
            stat = Path(path_text).stat()
            if stat.st_size != watch["size"] or stat.st_mtime_ns != watch["mtime_ns"]:
                raise RuntimeError(f"PXE artifact changed during reset transaction: {path_text}")
            if stat.st_atime_ns > watch["armed_atime_ns"]:
                watch["observed_atime_ns"] = stat.st_atime_ns
                pending.remove(path_text)
        if pending:
            time.sleep(0.1)

    if pending:
        missing = ", ".join(sorted(pending))
        raise RuntimeError(
            f"timed out after {timeout:g}s waiting for fresh PXE reads: {missing}; "
            "the external PXE service must serve this repository's bld directory "
            "on an atime-enabled filesystem"
        )

    for watch in watches:
        path = Path(watch["path"])
        actual = sha256_file(path)
        watch["verified_sha256"] = actual
        if actual != watch["expected_sha256"]:
            raise RuntimeError(
                f"PXE artifact identity mismatch for {path}: "
                f"expected {watch['expected_sha256']}, got {actual}"
            )


def press_physical_reset_button(
    *,
    bind_host: str,
    listen_port: int,
    response_port: int,
    probe_timeout: float,
) -> dict[str, Any]:
    """Acknowledge the ESP32 probe that latches the rig's physical reset button."""
    deadline = time.monotonic() + probe_timeout
    ignored_datagrams = 0

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind((bind_host, listen_port))
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise RuntimeError(
                    "timed out after "
                    f"{probe_timeout:g}s waiting for the test-rig ESP32 physical-reset "
                    f"probe on UDP {bind_host}:{listen_port}"
                )
            sock.settimeout(remaining)
            try:
                payload, source = sock.recvfrom(2048)
            except socket.timeout as exc:
                raise RuntimeError(
                    "timed out after "
                    f"{probe_timeout:g}s waiting for the test-rig ESP32 physical-reset "
                    f"probe on UDP {bind_host}:{listen_port}"
                ) from exc
            if payload != PROBE:
                ignored_datagrams += 1
                continue

            # This is the historical test-rig protocol: the ESP32 sends
            # `probe` to host UDP/7777, then latches the physical reset button
            # when `ack` arrives at the same ESP32 address on UDP/7777.
            target = (source[0], response_port)
            sent = sock.sendto(ACK, target)
            if sent != len(ACK):
                raise RuntimeError(
                    f"short UDP ack for test-rig physical reset: sent {sent}/{len(ACK)} bytes"
                )
            return {
                "mechanism": "esp32-udp-latched-physical-reset-button",
                "protocol": "probe-ack-v1",
                "bind_host": bind_host,
                "listen_port": listen_port,
                "probe_source_host": source[0],
                "probe_source_port": source[1],
                "probe_bytes": len(payload),
                "ack_target_host": target[0],
                "ack_target_port": target[1],
                "ack_bytes": sent,
                "ack_sent_at_ns": time.time_ns(),
                "ignored_datagrams": ignored_datagrams,
            }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind-host", type=ipv4_address, default="0.0.0.0")
    parser.add_argument("--listen-port", type=udp_port, default=7777)
    parser.add_argument("--response-port", type=udp_port, default=7777)
    parser.add_argument("--probe-timeout", type=positive_timeout, default=30.0)
    parser.add_argument("--tftp-timeout", type=positive_timeout, default=240.0)
    parser.add_argument(
        "--watch",
        action="append",
        type=parse_watch,
        default=[],
        metavar="PATH=SHA256",
    )
    parser.add_argument(
        "--identity",
        action="append",
        type=parse_identity,
        default=[],
        metavar="KEY=VALUE",
    )
    parser.add_argument("--receipt", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    receipt_path = args.receipt.resolve() if args.receipt else None
    record: dict[str, Any] = {
        "version": 3,
        "status": "initializing",
        "reset_mechanism": "testrig-physical-reset-button",
        "identities": dict(args.identity),
        "watch": [],
        "physical_reset": None,
    }

    try:
        watches = [prepare_watch(path, expected) for path, expected in args.watch]
        record["watch"] = watches

        # Arm this immediately before exposing the UDP reset latch: reads made
        # by preflight validation cannot satisfy the deployment witness.
        arm_pxe_read_witness(watches)

        record["status"] = "awaiting-physical-reset-probe"
        write_receipt(receipt_path, record)
        reset_result = press_physical_reset_button(
            bind_host=args.bind_host,
            listen_port=args.listen_port,
            response_port=args.response_port,
            probe_timeout=args.probe_timeout,
        )
        record["physical_reset"] = reset_result
        record["status"] = "physical-reset-button-pressed"
        write_receipt(receipt_path, record)
        print(
            "testrig-physical-reset: button_pressed=1 "
            "mechanism=esp32-udp-latched "
            f"probe_source={reset_result['probe_source_host']}:{reset_result['probe_source_port']} "
            f"ack_target={reset_result['ack_target_host']}:{reset_result['ack_target_port']}",
            flush=True,
        )

        wait_for_pxe_reads(watches, timeout=args.tftp_timeout)
        record["status"] = "verified"
        record["completed_at_ns"] = time.time_ns()
        write_receipt(receipt_path, record)
        if watches:
            print(
                "testrig-physical-reset: fresh_pxe_reads=1 artifact_hashes_verified=1 "
                + " ".join(f"path={watch['path']}" for watch in watches),
                flush=True,
            )
        return 0
    except (OSError, RuntimeError) as exc:
        record["status"] = "failed"
        record["error"] = str(exc)
        record["failed_at_ns"] = time.time_ns()
        write_receipt(receipt_path, record)
        print(f"testrig-physical-reset: error: {exc}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
