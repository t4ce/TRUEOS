#!/usr/bin/env python3
"""Validate a document-sized Hull viewport on an already deployed rig.

Starts one owned Hull, loads web.de, and sends HID input only to its centered
window. Preserves screenshots, the SVG, complete logs, and navigation metrics.
The test never builds, resets the rig, or stops another app.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import secrets
import socket
import struct
import time
import xml.etree.ElementTree as ET

import test_hull_microfont_baremetal as shared

VIEW = re.compile(
    r"hull: viewport=(\d+)x(\d+) document=(\d+)x(\d+) "
    r"scroll=([\d.]+),([\d.]+) rects=(\d+)/(\d+) "
    r"text_runs=(\d+)/(\d+) glyphs=(\d+) scale=1"
)
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b[@-_]")


def viewport_records(data: bytes) -> list[dict]:
    result = []
    for match in VIEW.finditer(ANSI.sub("", data.decode("utf-8", errors="replace"))):
        values = [float(value) for value in match.groups()]
        result.append(dict(zip(
            ("width", "height", "document_width", "document_height", "x", "y",
             "visible_rects", "total_rects", "visible_text_runs", "total_text_runs", "glyphs"), values
        )))
    return result


class Hid:
    def __init__(self, host: str, display: tuple[int, int], viewport: tuple[int, int]):
        self.host = host
        self.device = secrets.randbelow(65535) + 1
        self.sequence = 0
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.x = round(((display[0] - viewport[0]) / 2 + 160) / (display[0] - 1) * 65535)
        self.y = round(((display[1] - viewport[1]) / 2 + 160) / (display[1] - 1) * 65535)

    def send(self, kind: int, payload: bytes):
        self.sequence += 1
        header = struct.pack("<4sBBHIHH", b"THID", 1, kind, 0, self.sequence, self.device, 0)
        self.socket.sendto(header + payload, (self.host, 100))

    def tablet(self, buttons: int = 0, wheel: int = 0):
        self.send(3, struct.pack("<IIIh", self.x, self.y, buttons, wheel))

    def focus(self):
        self.tablet()
        time.sleep(0.08)
        self.tablet(buttons=1)
        time.sleep(0.08)
        self.tablet()
        time.sleep(0.15)

    def keyboard(self, key: int = 0, modifiers: int = 0):
        self.send(2, struct.pack("8B", modifiers, 0, key, 0, 0, 0, 0, 0))

    def key(self, key: int):
        self.keyboard(key)
        time.sleep(0.08)
        self.keyboard()

    def wheel(self, amount: int, shift: bool = False):
        if shift:
            self.keyboard(modifiers=2)
            time.sleep(0.1)
        self.tablet(wheel=amount)
        time.sleep(0.08)
        self.tablet()
        if shift:
            self.keyboard()


def run(args) -> dict:
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    log = args.log.resolve(strict=True)
    witness = shared.deployment_witness(args, log, output)
    log_start = log.stat().st_size
    base = f"http://{args.host}"
    session = shared.Shell3(args, 1, output)
    mode = "CMD"
    captures = []
    navigation = []
    hid = None

    def logs():
        with log.open("rb") as handle:
            handle.seek(log_start)
            return handle.read().decode("utf-8", errors="replace")

    def wait_view(start: int, expected, description: str, timeout=15, retry_url=None):
        retries = 0
        def ready():
            nonlocal retries
            failures = bytes(session.raw[start:]).count(b"hull: download failed:")
            if retry_url and failures > retries and retries < 2:
                retries += 1
                print(f"Retrying transient page download ({retries}/2)", flush=True)
                session.send((retry_url + "\r").encode())
            rows = viewport_records(bytes(session.raw[start:]))
            return bool(rows and expected(rows[-1]))
        shared.await_condition([session], ready, timeout, description)
        row = viewport_records(bytes(session.raw[start:]))[-1]
        assert row["visible_text_runs"] <= row["total_text_runs"]
        assert row["visible_rects"] <= row["total_rects"]
        assert 0 <= row["x"] <= max(row["document_width"] - row["width"], 0)
        assert 0 <= row["y"] <= max(row["document_height"] - row["height"], 0)
        return row

    def capture(name: str):
        nonlocal mode
        before = shared.file_index(base).screenshots()
        marker = len(session.raw)
        session.send(b"\t")  # Leaves the active VM while retaining its mode.
        time.sleep(0.2)
        if mode == "CMD":
            session.send(b"\t")
            mode = "ADM"
        shared.await_condition([session], lambda: session.contains("System[", marker), 10, "ADM strip")
        marker = len(session.raw)
        session.send(b"pic")
        shared.await_condition([session], lambda: session.contains("Take one screenshot", marker), 10, "PIC helper")
        session.send(b"\r")
        fresh = set()

        def saved():
            nonlocal fresh
            fresh = shared.file_index(base).screenshots() - before
            return bool(fresh)

        shared.await_condition([session], saved, 30, f"fresh {name} screenshot")
        href = sorted(fresh)[-1]
        png = shared.get_bytes(base, href)
        if png[:8] != b"\x89PNG\r\n\x1a\n":
            raise RuntimeError("screenshot download was not PNG")
        target = output / f"{name}.png"
        target.write_bytes(png)
        dimensions = struct.unpack(">II", png[16:24])
        captures.append({"name": name, "path": str(target), "rig_path": href, "dimensions": dimensions})
        session.send(b"q")
        time.sleep(0.2)
        session.send(("§" + args.slot + "\r").encode())
        time.sleep(0.25)
        session.pump()
        return dimensions

    def command(name: str, action, x: float, y: float):
        marker = len(session.raw)
        action()
        row = wait_view(marker, lambda row: abs(row["x"] - x) < .1 and abs(row["y"] - y) < .1, name)
        navigation.append({"action": name, **row})
        print(f"{name}: scroll={row['x']:g},{row['y']:g} visible_text={row['visible_text_runs']:g}/{row['total_text_runs']:g}", flush=True)
        return row

    try:
        shared.await_condition([session], lambda: session.contains("AppDB["), 30, "initial CMD strip")
        if session.contains("§" + args.slot):
            raise RuntimeError("requested Hull slot already exists; use a fresh test boot or a distinct --slot")
        session.send(b"hull")
        shared.await_condition([session], lambda: session.contains("reason=layout"), 60, "initial example.com frame")
        marker = len(session.raw)
        session.send(b"https://web.de/\r")
        top = wait_view(marker,
                        lambda row: row["document_height"] > row["height"] and row["x"] == 0 and row["y"] == 0,
                        "successful web.de document viewport", args.timeout, retry_url="https://web.de/")
        shared.await_condition([session], lambda: "hull: saved apps/hull/web.de/layout.svg" in logs(),
                               10, "web.de SVG save log")
        # Navigation must reuse this layout rather than running it again.
        layout_count = logs().count("hull: layout start")
        navigation.append({"action": "top", **top})
        dimensions = capture("top")
        hid = Hid(args.host, dimensions, (int(top["width"]), int(top["height"])))
        hid.focus()
        max_x = max(top["document_width"] - top["width"], 0)
        max_y = top["document_height"] - top["height"]
        page = top["height"] * .9
        row = command("page_down", lambda: hid.key(0x4E), 0, min(page, max_y))
        while row["y"] < max_y / 2:
            row = command("page_down_to_middle", lambda: hid.key(0x4E), 0, min(row["y"] + page, max_y))
        capture("middle")
        command("end", lambda: hid.key(0x4D), 0, max_y)
        capture("bottom")
        command("home", lambda: hid.key(0x4A), 0, 0)
        if max_x:
            x = 0
            while x < max_x:
                x = min(x + 48, max_x)
                command("arrow_right", lambda: hid.key(0x4F), x, 0)
            capture("right_edge")
            while x > 0:
                x = max(x - 48, 0)
                command("arrow_left", lambda: hid.key(0x50), x, 0)
        command("wheel_down", lambda: hid.wheel(-1), 0, min(48, max_y))
        command("wheel_up", lambda: hid.wheel(1), 0, 0)
        if max_x:
            command("shift_wheel_right", lambda: hid.wheel(-1, shift=True), min(48, max_x), 0)
            command("shift_wheel_left", lambda: hid.wheel(1, shift=True), 0, 0)
        if logs().count("hull: layout start") != layout_count:
            raise RuntimeError("navigation unexpectedly recomputed the page layout")
        links = [href for href in shared.file_index(base).links if href.endswith("/apps/hull/web.de/layout.svg")]
        if len(links) != 1:
            raise RuntimeError("could not select the new web.de layout on mounted roots")
        svg = shared.get_bytes(base, links[0])
        (output / "layout.svg").write_bytes(svg)
        tree = ET.fromstring(svg)
        texts = [node for node in tree.iter() if node.tag.endswith("text")]
        if any(float(node.get("font-size", 0)) <= 0 for node in texts):
            raise RuntimeError("saved SVG still contains a nonpositive font size")
        if "hull: Invalid" in logs():
            raise RuntimeError("Hull reported Invalid while loading or navigating web.de")
        return {
            "status": "passed", "deployment": witness, "page": "https://web.de/",
            "slot": args.slot, "hid_device": hid.device, "top": top,
            "navigation": navigation, "layout_start_count_before_scroll": layout_count,
            "layout_start_count_after_scroll": logs().count("hull: layout start"),
            "svg": {"path": str(output / "layout.svg"), "extent": tree.attrib, "text_runs": len(texts),
                    "font_sizes": sorted(set(float(node.get("font-size")) for node in texts))},
            "font_metrics": shared.summarize(shared.font_events(logs())), "captures": captures,
            "note": "The owned Hull remains running at the top-left viewport. No other app was stopped.",
        }
    finally:
        (output / "runtime.log").write_text(logs())
        if hid is not None:
            hid.keyboard()
            hid.tablet()
            hid.socket.close()
        session.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--user", default="t4ce")
    parser.add_argument("--identity", type=Path, default=Path("~/.ssh/id_ed25519_trueos"))
    parser.add_argument("--log", type=Path, default=shared.ROOT / "bld/baremetal-logs/LatestOfThree.logs")
    parser.add_argument("--receipt", type=Path, default=shared.ROOT / "bld/testrig-physical-reset-receipt.json")
    parser.add_argument("--profile", choices=("debug", "release"), default="debug")
    parser.add_argument("--slot", default="hull")
    parser.add_argument("--timeout", type=float, default=180)
    parser.add_argument("--output", type=Path, default=shared.ROOT / "bld" / f"hull-webde-{time.time_ns()}")
    args = parser.parse_args()
    try:
        result = run(args)
    except (OSError, RuntimeError, AssertionError) as error:
        result = {"status": "failed", "error": str(error), "output": str(args.output.resolve())}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "summary.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return int(result["status"] != "passed")


if __name__ == "__main__":
    raise SystemExit(main())
