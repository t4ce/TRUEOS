#!/usr/bin/env python3
"""Actively reboot a TRUEOS rig through Shell2, then verify fresh PXE reads."""

from __future__ import annotations

import argparse
import base64
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time
from typing import Any


def positive_timeout(value: str) -> float:
    parsed = float(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("timeout must be greater than zero")
    return parsed


def tcp_port(value: str) -> int:
    parsed = int(value)
    if not 1 <= parsed <= 65535:
        raise argparse.ArgumentTypeError("port must be in 1..65535")
    return parsed


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
            f"PXE artifact identity mismatch before reboot for {path}: "
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
                raise RuntimeError(f"PXE artifact changed during reboot transaction: {path_text}")
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


def process_argv(pid: int) -> list[str] | None:
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes()
    except (FileNotFoundError, PermissionError, ProcessLookupError):
        return None
    return [
        value.decode("utf-8", errors="surrogateescape")
        for value in raw.split(b"\0")
        if value
    ]


def process_link(pid: int, name: str) -> Path | None:
    try:
        return Path(os.readlink(f"/proc/{pid}/{name}")).resolve()
    except (FileNotFoundError, PermissionError, ProcessLookupError, OSError):
        return None


def process_alive(pid: int) -> bool:
    try:
        fields = Path(f"/proc/{pid}/stat").read_text(encoding="ascii").split()
    except (FileNotFoundError, PermissionError, ProcessLookupError):
        return False
    return len(fields) >= 3 and fields[2] != "Z"


def process_exists(pid: int) -> bool:
    return Path(f"/proc/{pid}").exists()


def scoped_stale_net_shell_clients(
    *,
    host: str,
    port: int,
    repo_root: Path,
) -> list[dict[str, Any]]:
    owned: list[dict[str, Any]] = []
    expected_repo_log = (repo_root / "bld/net-shell-console.log").resolve()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        try:
            if entry.stat().st_uid != os.getuid():
                continue
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
        argv = process_argv(pid)
        if (
            argv is None
            or len(argv) != 3
            or Path(argv[0]).name not in {"nc", "netcat"}
            or argv[1] != host
            or argv[2] != str(port)
        ):
            continue
        cwd = process_link(pid, "cwd")
        stdout = process_link(pid, "fd/1")
        if cwd != repo_root or stdout is None:
            continue
        tmp_capture = (
            stdout.parent == Path("/tmp")
            and stdout.name.startswith("trueos-net-shell-")
            and stdout.suffix in {".out", ".log"}
        )
        if stdout != expected_repo_log and not tmp_capture:
            continue
        owned.append(
            {
                "pid": pid,
                "argv": argv,
                "cwd": str(cwd),
                "stdout": str(stdout),
            }
        )
    return owned


def stop_scoped_clients(clients: list[dict[str, Any]]) -> list[dict[str, Any]]:
    def signal_exact_pid(pid: int, sig: signal.Signals) -> None:
        try:
            os.kill(pid, sig)
            return
        except ProcessLookupError:
            return
        except PermissionError:
            # This host's AppArmor profile can deny a direct signal to
            # nc.openbsd even for the same uid. The user manager is the narrow
            # fallback: one already validated positive PID, never a name,
            # pattern, process group, or port-wide kill.
            result = subprocess.run(
                [
                    "systemd-run",
                    "--user",
                    "--wait",
                    "--pipe",
                    "--quiet",
                    "/bin/kill",
                    f"-{sig.name.removeprefix('SIG')}",
                    str(pid),
                ],
                check=False,
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode != 0:
                detail = (result.stderr or result.stdout).strip()
                raise RuntimeError(
                    f"could not signal validated stale Shell2 nc pid={pid}: "
                    f"systemd-run exit={result.returncode} detail={detail!r}"
                )

    for client in clients:
        signal_exact_pid(client["pid"], signal.SIGTERM)
    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline and any(
        process_alive(client["pid"]) for client in clients
    ):
        time.sleep(0.05)
    for client in clients:
        if process_alive(client["pid"]):
            signal_exact_pid(client["pid"], signal.SIGKILL)
            client["forced_kill"] = True
        else:
            client["forced_kill"] = False
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and any(
        process_exists(client["pid"]) for client in clients
    ):
        time.sleep(0.05)
    survivors = [client["pid"] for client in clients if process_exists(client["pid"])]
    if survivors:
        raise RuntimeError(
            "validated stale Shell2 nc retained /proc state after exact-PID signalling: "
            + ", ".join(str(pid) for pid in survivors)
        )
    for client in clients:
        client["stopped"] = True
    if clients:
        time.sleep(0.2)
    return clients


def receive_bounded(sock: socket.socket, *, timeout: float, cap: int = 64 * 1024) -> bytes:
    deadline = time.monotonic() + timeout
    chunks: list[bytes] = []
    total = 0
    while total < cap and time.monotonic() < deadline:
        sock.settimeout(max(0.01, min(0.2, deadline - time.monotonic())))
        try:
            chunk = sock.recv(min(4096, cap - total))
        except socket.timeout:
            continue
        except (ConnectionResetError, ConnectionAbortedError):
            break
        if not chunk:
            break
        chunks.append(chunk)
        total += len(chunk)
    return b"".join(chunks)


def send_shell_reboot(
    *,
    host: str,
    port: int,
    command: str,
    connect_timeout: float,
    response_timeout: float,
) -> dict[str, Any]:
    if not command or "\r" in command or "\n" in command:
        raise RuntimeError("Shell2 reboot command must be one non-empty line")

    try:
        sock = socket.create_connection((host, port), timeout=connect_timeout)
    except OSError as exc:
        raise RuntimeError(
            f"could not connect to Shell2 at {host}:{port} within {connect_timeout:g}s: {exc}"
        ) from exc

    with sock:
        prelude = receive_bounded(sock, timeout=0.3, cap=16 * 1024)
        # Shell2 asks new clients for terminal size. Supplying that response
        # before the command avoids its short initial repaint buffer delaying
        # or swallowing a small non-interactive command.
        terminal_size_reply = b"\x1b[8;24;120t"
        command_wire = command.encode("utf-8") + b"\r\n"
        wire = terminal_size_reply + command_wire
        try:
            sock.sendall(wire)
        except OSError as exc:
            raise RuntimeError(
                f"connected to Shell2 but could not send the reboot command: {exc}"
            ) from exc
        sent_at_ns = time.time_ns()
        response = receive_bounded(sock, timeout=response_timeout)

    captured = prelude + response
    return {
        "host": host,
        "port": port,
        "command": command,
        "command_crlf": True,
        "terminal_size_reply": "ESC[8;24;120t",
        "wire_bytes_sent": len(wire),
        "command_sent_at_ns": sent_at_ns,
        "prelude_bytes": len(prelude),
        "response_bytes": len(response),
        "captured_base64": base64.b64encode(captured).decode("ascii"),
        "captured_text": captured.decode("utf-8", errors="replace"),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shell-host", required=True)
    parser.add_argument("--shell-port", type=tcp_port, default=4245)
    parser.add_argument("--command", default="acpi reboot")
    parser.add_argument("--connect-timeout", type=positive_timeout, default=5.0)
    parser.add_argument("--response-timeout", type=positive_timeout, default=2.0)
    parser.add_argument("--tftp-timeout", type=positive_timeout, default=240.0)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--no-cleanup-stale-nc",
        action="store_false",
        dest="cleanup_stale_nc",
        help="do not stop narrowly identified repo-owned stale nc clients",
    )
    parser.set_defaults(cleanup_stale_nc=True)
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
    args = parser.parse_args()
    try:
        target = ipaddress.ip_address(args.shell_host)
    except ValueError as exc:
        parser.error(f"invalid --shell-host: {exc}")
    if target.version != 4:
        parser.error("--shell-host must be IPv4")
    return args


def main() -> int:
    args = parse_args()
    receipt_path = args.receipt.resolve() if args.receipt else None
    repo_root = args.repo_root.resolve()
    record: dict[str, Any] = {
        "version": 2,
        "status": "initializing",
        "identities": dict(args.identity),
        "watch": [],
        "shell": None,
        "stale_shell_clients_stopped": [],
    }

    try:
        watches = [prepare_watch(path, expected) for path, expected in args.watch]
        record["watch"] = watches
        if args.cleanup_stale_nc:
            clients = scoped_stale_net_shell_clients(
                host=args.shell_host,
                port=args.shell_port,
                repo_root=repo_root,
            )
            record["stale_shell_clients_stopped"] = stop_scoped_clients(clients)

        # Arm this immediately before the command: reads made by preflight
        # validation cannot satisfy the deployment witness.
        arm_pxe_read_witness(watches)

        record["status"] = "sending-shell-reboot"
        write_receipt(receipt_path, record)
        shell_result = send_shell_reboot(
            host=args.shell_host,
            port=args.shell_port,
            command=args.command,
            connect_timeout=args.connect_timeout,
            response_timeout=args.response_timeout,
        )
        record["shell"] = shell_result
        record["status"] = "shell-command-sent"
        write_receipt(receipt_path, record)
        print(
            "baremetal-reboot: shell_command_sent=1 "
            f"target={args.shell_host}:{args.shell_port} "
            f"command={args.command!r} crlf=1 wire_bytes={shell_result['wire_bytes_sent']} "
            f"response_bytes={shell_result['response_bytes']} "
            f"scoped_stale_nc_stopped={len(record['stale_shell_clients_stopped'])}",
            flush=True,
        )

        wait_for_pxe_reads(watches, timeout=args.tftp_timeout)
        record["status"] = "verified"
        record["completed_at_ns"] = time.time_ns()
        write_receipt(receipt_path, record)
        if watches:
            print(
                "baremetal-reboot: fresh_pxe_reads=1 artifact_hashes_verified=1 "
                + " ".join(f"path={watch['path']}" for watch in watches),
                flush=True,
            )
        return 0
    except (OSError, RuntimeError) as exc:
        record["status"] = "failed"
        record["error"] = str(exc)
        record["failed_at_ns"] = time.time_ns()
        write_receipt(receipt_path, record)
        print(f"baremetal-reboot: error: {exc}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
