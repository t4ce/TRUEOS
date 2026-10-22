#!/usr/bin/env python3
"""Exercise the Hull raster-font service on an already booted physical rig.

The default action launches three Hull instances. It neither builds nor resets
the rig. Use --analyze LOG to inspect a previously captured font-service log.
"""
from __future__ import annotations

import argparse
import fcntl
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import pty
import re
import select
import statistics
import struct
import subprocess
import termios
import time
from urllib.parse import urljoin, urlsplit
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[2]
EVENT = re.compile(r"hull-font: (prepare|paint|overlay) ([^\r\n]*)")
FIELD = re.compile(r"([a-z_]+)=([^\s]+)")


def font_events(text: str) -> list[dict]:
    result = []
    for match in EVENT.finditer(text):
        row = {"event": match[1]}
        for name, value in FIELD.findall(match[2]):
            try:
                row[name] = float(value) if "." in value else int(value)
            except ValueError:
                row[name] = value
        result.append(row)
    return result


def summarize(events: list[dict]) -> dict:
    prepares = [e for e in events if e["event"] == "prepare" and e.get("glyphs", 0)]
    paints = [e for e in events if e["event"] == "paint" and e.get("glyphs", 0)]
    overlays = [e for e in events if e["event"] == "overlay"]
    errors = []
    for row in prepares:
        for required in ("rasterized", "hits", "cache_bytes", "budget_bytes", "prepare_us", "raster_us"):
            if required not in row:
                errors.append(f"prepare record lacks {required}")
        if row.get("cache_bytes", 0) > row.get("budget_bytes", 0):
            errors.append("resident glyph masks exceed the cache budget")
        if row.get("format") != "r8" or row.get("quantum_mm") != 2.5:
            errors.append("font record does not report R8 masks and 2.5 mm tiers")
    if not prepares:
        errors.append("no nonempty Hull font preparations captured")
    if not paints:
        errors.append("no nonempty Hull font paints captured")
    if any(row.get("status") != 0 for row in overlays):
        errors.append("Hull text overlay reported an unsuccessful status")
    durations = {}
    for kind, rows, fields in (("prepare", prepares, ("prepare_us", "raster_us")),
                               ("paint", paints, ("compose_us",)),
                               ("overlay", overlays, ("overlay_us", "cache_sync_us", "paint_us"))):
        for name in fields:
            values = [row[name] for row in rows if isinstance(row.get(name), (int, float))]
            if values:
                durations[name] = {"samples": len(values), "median": statistics.median(values), "max": max(values)}
    return {
        "prepare_count": len(prepares), "paint_count": len(paints), "overlay_count": len(overlays),
        "rasterized": sum(row.get("rasterized", 0) for row in prepares),
        "raster_pixels": sum(row.get("raster_pixels", 0) for row in prepares),
        "hits": sum(row.get("hits", 0) for row in prepares),
        "concurrent_duplicates": sum(row.get("concurrent_duplicates", 0) for row in prepares),
        "evictions": sum(row.get("evictions", 0) for row in prepares),
        "missing_glyphs": sum(row.get("missing", 0) for row in paints),
        "composed_pixels": sum(row.get("pixels", 0) for row in paints),
        "peak_cache_bytes": max((row.get("cache_bytes", 0) for row in prepares), default=0),
        "cache_budget_bytes": max((row.get("budget_bytes", 0) for row in prepares), default=0),
        "overlay_frame_bytes": max((row.get("frame_bytes", 0) for row in overlays), default=0),
        "cache_sync_bytes": max((row.get("cache_sync_bytes", 0) for row in overlays), default=0),
        "cache_sync_rows": max((row.get("cache_sync_rows", 0) for row in overlays), default=0),
        "timings_us": durations, "errors": sorted(set(errors)),
    }


class FileIndex(HTMLParser):
    def __init__(self, html: str):
        super().__init__()
        self.roots: set[str] = set()
        self.links: set[str] = set()
        self.feed(html)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "data-root" in attrs:
            self.roots.add(attrs["data-root"])
        href = attrs.get("href", "")
        if tag == "a" and href.startswith("/dl/"):
            self.links.add(href)

    def screenshots(self) -> set[str]:
        return {href for href in self.links if "/screenshots/wd-postblend-" in href and href.endswith(".png")}


def get_bytes(base: str, path: str) -> bytes:
    # Filesystem index links are data. Accept only the selected rig's origin.
    url = urljoin(base + "/", path)
    if urlsplit(url).netloc != urlsplit(base).netloc:
        raise RuntimeError("filesystem link points outside the selected rig")
    with urlopen(url, timeout=10) as response:
        return response.read()


def file_index(base: str) -> FileIndex:
    return FileIndex(get_bytes(base, "/").decode("utf-8"))


def deployment_witness(args, log: Path, output: Path) -> dict:
    receipt = json.loads(args.receipt.read_text())
    if receipt.get("status") != "verified":
        raise RuntimeError("physical reset receipt does not verify fresh PXE reads")
    identities = receipt.get("identities", {})
    artifacts = {}
    with log.open("r", errors="replace") as handle:
        header = "".join(handle.readline() for _ in range(3))
    for name, path in (("runtime_elf_sha256", ROOT / "bld/TRUEOS.elf"),
                       ("iso_sha256", ROOT / "bld/trueos.iso")):
        with path.open("rb") as handle:
            digest = hashlib.file_digest(handle, "sha256").hexdigest()
        if identities.get(name) != digest:
            raise RuntimeError(f"current {path.name} does not match the deployed image receipt")
        if f"expected_{name}={digest}" not in header:
            raise RuntimeError("selected log belongs to a different deployed image")
        artifacts[name] = digest
    (output / "physical-reset-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    (output / "capture-header.txt").write_text(header)
    return {"profile": args.profile, "receipt": str(args.receipt.resolve()), **artifacts}


class Shell3:
    def __init__(self, args, number: int, output: Path):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        self.transcript = (output / f"shell3-{number}.ansi").open("wb")
        self.raw = bytearray()
        command = ["ssh", "-F", "/dev/null", "-tt", "-p", "22", "-i", str(args.identity.expanduser()),
                   "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ConnectionAttempts=10",
                   "-o", "StrictHostKeyChecking=yes",
                   f"{args.user}@{args.host}"]
        self.process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave, close_fds=True)
        os.close(slave)

    def pump(self, wait: float = 0) -> None:
        if not select.select([self.master], [], [], wait)[0]:
            return
        try:
            data = os.read(self.master, 65536)
        except OSError:
            data = b""
        if data:
            self.raw.extend(data)
            self.transcript.write(data)
            self.transcript.flush()

    def send(self, data: bytes) -> None:
        os.write(self.master, data)

    def contains(self, text: str, start: int = 0) -> bool:
        return text.encode() in self.raw[start:]

    def close(self) -> None:
        # Disconnect only this test's client. Retain launched Hulls for visual inspection.
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.pump()
        os.close(self.master)
        self.transcript.close()


def await_condition(sessions: list[Shell3], predicate, timeout: float, description: str) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for session in sessions:
            session.pump()
            if session.process.poll() is not None:
                raise RuntimeError("SSH disconnected; inspect the saved Shell3 transcript")
        if predicate():
            return
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for {description}")


def run_rig(args) -> dict:
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    log = args.log.resolve(strict=True)
    log_identity = log.stat().st_ino
    log_start = log.stat().st_size
    witness = deployment_witness(args, log, output)
    print("Waiting for Shell3 SSH readiness", flush=True)
    base = f"http://{args.host}"
    baseline = file_index(base)
    sessions = []

    def captured_log() -> str:
        if log.stat().st_ino != log_identity or log.stat().st_size < log_start:
            raise RuntimeError("baremetal capture rotated during the test; rerun against the current capture")
        with log.open("rb") as handle:
            handle.seek(log_start)
            return handle.read().decode("utf-8", errors="replace")

    def records() -> list[dict]:
        return font_events(captured_log())

    try:
        for number in range(args.instances):
            sessions.append(Shell3(args, number + 1, output))
            # The kernel replaces its SSH listener after each acceptance.
            # Establish clients separately, then issue Hull requests together.
            await_condition(sessions, lambda: sessions[-1].contains("AppDB["),
                            30, f"Shell3 client {number + 1} CMD strip")
        await_condition(sessions, lambda: all(s.contains("AppDB[") for s in sessions),
                        20, "initial CMD-mode AppDB strips")
        launch_times = []
        print(f"Launching {args.instances} Hulls with {args.gap:g} s between requests", flush=True)
        for session in sessions:
            session.send(b"hull")  # Current Shell3 recognizes the exact app name immediately.
            launch_times.append(time.monotonic())
            for other in sessions:
                other.pump()
            time.sleep(args.gap)
        await_condition(sessions,
                        lambda: all(s.contains("reason=layout") for s in sessions)
                        and len([e for e in records() if e["event"] == "paint" and e.get("glyphs", 0)]) >= args.instances
                        and (not args.require_overlay or len([e for e in records() if e["event"] == "overlay"]) >= args.instances),
                        args.timeout, "all Hull text frames")
        first_events = records()
        first_summary = summarize(first_events)
        if args.require_cold and first_summary["rasterized"] == 0:
            first_summary["errors"].append("cold run did not rasterize a glyph; boot the new image immediately before testing")
        if args.require_overlay and first_summary["overlay_count"] < args.instances:
            first_summary["errors"].append("missing per-instance overlay timing records")

        # Repeat the identical page while the other Hulls retain the shared cache.
        warm_offset = len(captured_log())
        warm_terminal_offset = len(sessions[0].raw)
        print("Reloading the identical page to measure the warm cache", flush=True)
        sessions[0].send(b"https://example.com/\r")

        def warm_records():
            return font_events(captured_log()[warm_offset:])

        await_condition(sessions,
                        lambda: sessions[0].contains("reason=layout", warm_terminal_offset)
                        and any(e["event"] == "paint" and e.get("glyphs", 0) for e in warm_records())
                        and (not args.require_overlay or any(e["event"] == "overlay" for e in warm_records())),
                        args.timeout, "warm Hull text frame")
        warm_events = warm_records()
        warm_summary = summarize(warm_events)
        warm_prepares = [e for e in warm_events if e["event"] == "prepare" and e.get("glyphs", 0)]
        if not any(e.get("rasterized") == 0 and e.get("hits", 0) > 0 for e in warm_prepares):
            warm_summary["errors"].append("identical warm page did not reuse glyphs without rasterization")
        if args.require_overlay and not warm_summary["overlay_count"]:
            warm_summary["errors"].append("missing warm overlay timing record")

        # First Tab leaves the app alive in CMD; second Tab selects ADM.
        sessions[0].send(b"\t")
        time.sleep(0.3)
        marker = len(sessions[0].raw)
        sessions[0].send(b"\t")
        await_condition(sessions, lambda: sessions[0].contains("System[", marker), 10, "ADM system strip")
        marker = len(sessions[0].raw)
        sessions[0].send(b"pic")
        await_condition(sessions, lambda: sessions[0].contains("Take one screenshot", marker), 10, "PIC menu")
        before_picture = file_index(base).screenshots() | baseline.screenshots()
        sessions[0].send(b"\r")
        fresh = set()

        def new_picture():
            nonlocal fresh
            fresh = file_index(base).screenshots() - before_picture
            return bool(fresh)

        await_condition(sessions, new_picture, 30, "a newly saved postblend PNG")
        captures = []
        for number, href in enumerate(sorted(fresh), 1):
            png = get_bytes(base, href)
            if png[:8] != b"\x89PNG\r\n\x1a\n":
                raise RuntimeError("capture route did not return PNG data")
            width, height = struct.unpack(">II", png[16:24])
            target = output / f"postblend-{number}.png"
            target.write_bytes(png)
            captures.append({"path": str(target), "rig_path": href, "width": width, "height": height})
        current = file_index(base)
        layouts = sorted(href for href in current.links if href.endswith("/apps/hull/example.com/layout.svg"))
        if len(layouts) != 1:
            raise RuntimeError(f"expected one example.com layout on discovered roots, found {len(layouts)}")
        svg = get_bytes(base, layouts[0])
        (output / "layout.svg").write_bytes(svg)
        sessions[0].send(b"q")
        result = {
            "status": "passed" if not first_summary["errors"] and not warm_summary["errors"] else "failed",
            "host": args.host, "instances": args.instances,
            "deployment": witness,
            "launch_request_span_ms": (launch_times[-1] - launch_times[0]) * 1000,
            "log": str(log), "log_start_bytes": log_start,
            "initial": first_summary, "warm_reload": warm_summary,
            "captures": captures, "layout": str(output / "layout.svg"),
            "note": "Hull VMs remain running for visual inspection. Timings do not measure AP-pool scaling.",
        }
        return result
    finally:
        (output / "font.log").write_text(captured_log())
        for session in sessions:
            session.close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--analyze", type=Path, help="analyze a captured log without accessing the rig")
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--user", default="t4ce")
    parser.add_argument("--identity", type=Path, default=Path("~/.ssh/id_ed25519_trueos"))
    parser.add_argument("--log", type=Path, default=ROOT / "bld/baremetal-logs/LatestOfThree.logs")
    parser.add_argument("--receipt", type=Path, default=ROOT / "bld/testrig-physical-reset-receipt.json")
    parser.add_argument("--profile", choices=("debug", "release"), default="debug", help="profile used to build the deployed image")
    parser.add_argument("--instances", type=int, choices=range(2, 5), default=3)
    parser.add_argument("--gap", type=float, default=0.1)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--require-cold", action="store_true")
    parser.add_argument("--require-overlay", action="store_true", help="require complete overlay/cache-sync timing records")
    parser.add_argument("--output", type=Path, default=ROOT / "bld" / f"hull-microfont-{time.time_ns()}")
    args = parser.parse_args()
    if args.gap < 0 or args.timeout <= 0:
        parser.error("gap must be nonnegative and timeout must be positive")
    if args.analyze:
        result = summarize(font_events(args.analyze.read_text(errors="replace")))
        print(json.dumps(result, indent=2))
        return int(bool(result["errors"]))
    try:
        result = run_rig(args)
    except (OSError, RuntimeError) as error:
        result = {"status": "failed", "error": str(error), "output": str(args.output.resolve())}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "summary.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return int(result["status"] != "passed")


if __name__ == "__main__":
    raise SystemExit(main())
