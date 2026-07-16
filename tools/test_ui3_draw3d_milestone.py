#!/usr/bin/env python3
"""Exercise the production Draw3D -> UI3 single-output milestone.

The test deliberately separates three kinds of evidence:

* boot log evidence proves UI3 presents its proof frame without TCP activity;
* Draw3D PNG capture proves scene rendering, but does not claim to capture UI3;
* kernel lifecycle summaries prove static/animated presentation cadence.

Keep ``tools/baremetal-log-drain.sh`` running so the selected log grows while
the scenarios execute.
"""

import argparse
import math
import re
import time
from pathlib import Path

from draw3d_house_demo import Draw3dClient


MESH_ID = 39_001
INSTANCE_ID = 49_001
TRIANGLE_VERTICES = (
    (0.0, 2.7, 0.0),
    (-2.5, -1.7, 0.0),
    (2.5, -1.7, 0.0),
)
KV_RE = re.compile(r"([A-Za-z0-9_]+)=([^\s]+)")


class TestFailure(RuntimeError):
    pass


class LogWatcher:
    def __init__(self, path: Path):
        self.path = path

    def text(self):
        try:
            return self.path.read_text(errors="replace")
        except FileNotFoundError as error:
            raise TestFailure(f"log does not exist: {self.path}") from error

    def mark(self):
        return len(self.text())

    def since(self, mark):
        text = self.text()
        # A rotating/truncated log invalidates the old byte mark. In that case
        # inspect the complete new file rather than silently returning empty.
        return text[mark:] if mark <= len(text) else text

    def wait_line(self, mark, contains, timeout=8.0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for line in self.since(mark).splitlines():
                if contains in line:
                    return line
            time.sleep(0.1)
        raise TestFailure(
            f"timed out after {timeout:.1f}s waiting for {contains!r} in {self.path}"
        )


def fields(line):
    return dict(KV_RE.findall(line))


def require(condition, message):
    if not condition:
        raise TestFailure(message)


def configure_triangle(client, speed):
    client.clear()
    # Phase zero is rotated from +X to +Z, matching the familiar front view.
    client.camera(
        (0.0, 0.0, 10.0),
        (0.0, 0.0, 0.0),
        50.0,
        orbit_scale=(10.0, 10.0),
        orbit_rotation=(0.0, -math.pi / 2.0, 0.0),
        orbit_speed=speed,
    )
    client.mesh(
        MESH_ID,
        (48, 220, 96, 210),
        TRIANGLE_VERTICES,
        ((0, 1, 2),),
    )
    client.instance(
        INSTANCE_ID,
        MESH_ID,
        (0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0),
    )


def validate_capture(client, output, expect_width, expect_height):
    output, image_format, width, height, image = client.render(output)
    stats = client.stats()
    require(image_format == 2, f"scene capture is not PNG: format={image_format}")
    require(
        (width, height) == (expect_width, expect_height),
        f"unexpected capture size {width}x{height}; "
        f"expected {expect_width}x{expect_height}",
    )
    require(stats[:3] == (1, 1, 3), f"unexpected scene counts: {stats}")
    require(stats[4] == 1, f"unexpected face count: {stats}")
    require(len(image) > 256, f"capture is implausibly small: {len(image)} bytes")
    print(
        f"scene_capture=PASS path={output} size={width}x{height} bytes={len(image)} "
        "scope=draw3d-source-not-ui3-scanout"
    )


def verify_boot(watcher):
    log = watcher.text()
    proof_lines = [
        line for line in log.splitlines() if "ui3-compositor: proof source" in line
    ]
    boot_lines = [
        line for line in log.splitlines() if "ui3-compositor: bootstrap" in line
    ]
    output_proof_lines = [
        line for line in log.splitlines() if "ui3-compositor: proof output" in line
    ]
    owner_lines = [
        line for line in log.splitlines() if "ui3-overlay-owner claimed=1" in line
    ]
    require(proof_lines, "missing UI3 proof-source diagnostic")
    require(output_proof_lines, "missing UI3 composed-output diagnostic")
    require(boot_lines, "missing independent UI3 bootstrap diagnostic")
    require(owner_lines, "missing exclusive UI3 overlay-owner claim")
    proof = fields(proof_lines[-1])
    output_proof = fields(output_proof_lines[-1])
    boot = fields(boot_lines[-1])
    require(proof.get("exact") == "1", f"proof source readback failed: {proof_lines[-1]}")
    require(proof.get("mismatches") == "0", f"proof source is not dense-exact: {proof_lines[-1]}")
    require(proof.get("verification") == "dense", "proof source still uses sparse verification")
    require(
        output_proof.get("exact") == "1" and output_proof.get("mismatches") == "0",
        f"composed output readback failed: {output_proof_lines[-1]}",
    )
    require(
        output_proof.get("stage") == "post-compose-pre-commit",
        "composed output was not verified at the presentation boundary",
    )
    require(boot.get("complete") == "1", f"UI3 bootstrap failed: {boot_lines[-1]}")
    require(boot.get("layers") == "1", f"boot is not proof-only: {boot_lines[-1]}")
    require(boot.get("tcp_dependency") == "none", "bootstrap still depends on TCP")
    print(
        "boot_proof=PASS source_dense_exact=1 output_dense_exact=1 "
        "layers=1 tcp_dependency=none owner=ui3-compositor"
    )


def verify_ownership(watcher):
    log = watcher.text()
    claim = log.rfind("ui3-overlay-owner claimed=1")
    require(claim >= 0, "missing UI3 overlay-owner claim")
    owned_log = log[claim:]
    rejection_lines = [
        line
        for line in owned_log.splitlines()
        if "legacy-overlay-present rejected" in line
    ]
    require(
        any("reason=font-tessel-render-target" in line for line in rejection_lines),
        "font render proof did not exercise the legacy-present rejection guard",
    )
    forbidden = [
        line
        for line in owned_log.splitlines()
        if "overlay-arm reason=font-tessel-render-target" in line
        or "overlay-present seq=" in line
        or "rgba-tile-overlay-present" in line
        or "live-overlay-present" in line
    ]
    require(forbidden == [], f"legacy presenter bypassed UI3 ownership: {forbidden[-1]}")
    print(
        f"ownership=PASS owner=ui3-compositor legacy_rejections={len(rejection_lines)} "
        "accepted_legacy_presents=0 lifetime=service"
    )


def parse_run_summary(line, expected_mode):
    data = fields(line)
    require(data.get("mode") == expected_mode, f"unexpected run mode: {line}")
    attempted = int(data["attempted"])
    presented = int(data["presented"])
    require(attempted > 0, f"no frames attempted: {line}")
    require(presented == attempted, f"incomplete presentation run: {line}")
    return data


def run_scene(args, watcher, mode):
    speed = 0.0 if mode == "static" else args.orbit_speed
    duration = args.static_seconds if mode == "static" else args.animated_seconds
    client = Draw3dClient(args.host)
    try:
        # Establish a known empty compositor/scene boundary before marking the
        # log. Permanent reset remains separate from the measured run.
        client.stop(permanent=True)
        time.sleep(args.reset_settle)
        configure_triangle(client, speed)
        mark = watcher.mark()
        client.start()  # transparent Draw3D background; desktop/proof remain meaningful
        time.sleep(duration)
        client.stop()
        summary_line = watcher.wait_line(mark, "draw3d-run-summary", timeout=args.log_timeout)
        validate_capture(
            client,
            args.output_dir / f"ui3-milestone-{mode}-scene.png",
            args.expect_width,
            args.expect_height,
        )
    finally:
        client.close()

    data = parse_run_summary(summary_line, mode)
    attempted = int(data["attempted"])
    hz = float(data["present_hz"])
    if mode == "static":
        require(attempted == 1, f"static scene resubmitted {attempted} frames: {summary_line}")
        print(
            f"static_persistence=PASS attempted=1 presented=1 duration_s={duration:.2f} "
            f"avg_frame_us={data['avg_frame_us']} avg_ui3_us={data['avg_ui3_us']}"
        )
    else:
        require(attempted >= 2, f"animated scene did not advance: {summary_line}")
        performance_pass = hz >= args.target_hz
        print(
            f"animated_functional=PASS attempted={attempted} presented={data['presented']} "
            f"present_hz={hz:.2f} avg_frame_us={data['avg_frame_us']} "
            f"avg_ui3_us={data['avg_ui3_us']} performance_target_hz={args.target_hz:.2f} "
            f"performance_gate={'PASS' if performance_pass else 'FAIL'}"
        )
        if args.require_target_hz and not performance_pass:
            raise TestFailure(
                f"animated cadence {hz:.2f} Hz is below required {args.target_hz:.2f} Hz"
            )


def run_lifecycle(args, watcher):
    client = Draw3dClient(args.host)
    try:
        client.stop(permanent=True)
        time.sleep(args.reset_settle)
        configure_triangle(client, 0.0)
        client.start()
        time.sleep(args.static_seconds)
        client.stop()
        time.sleep(args.reset_settle)
        mark = watcher.mark()
        client.stop(permanent=True)
        reset = watcher.wait_line(
            mark,
            "permanent scene-frame reset",
            timeout=args.log_timeout,
        )
        proof_only = watcher.wait_line(mark, "layers=1", timeout=args.log_timeout)
        _, image_format, width, height, _ = client.render(
            args.output_dir / "ui3-milestone-after-permanent-reset.jpg"
        )
    finally:
        client.close()
    require("complete=1" in reset, f"permanent reset failed: {reset}")
    require("ui3-compositor: output frame" in proof_only, f"not a UI3 output frame: {proof_only}")
    require(
        (image_format, width, height) == (1, 3840, 2160),
        "permanent reset retained screenshot eligibility: "
        f"format={image_format} size={width}x{height}",
    )
    print(
        "lifecycle=PASS permanent_reset=complete scene_layer=removed "
        "remaining_layers=1 screenshot=placeholder"
    )


def self_test():
    sample = (
        "draw3d-run-summary revision=4 mode=animated elapsed_ms=5000 attempted=31 "
        "presented=31 present_hz=6.20 avg_frame_us=158000 max_frame_us=180000 "
        "avg_ui3_us=140000 max_ui3_us=160000 over_budget=31 frame_budget_us=16667"
    )
    parsed = fields(sample)
    require(parsed["mode"] == "animated", "field parser lost mode")
    require(float(parsed["present_hz"]) == 6.20, "field parser lost decimal cadence")
    require(int(parsed["attempted"]) == 31, "field parser lost integer count")
    print("self_test=PASS parser=key-value cadence=decimal")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "scenario",
        choices=(
            "self-test",
            "boot",
            "ownership",
            "static",
            "animated",
            "lifecycle",
            "all",
        ),
        nargs="?",
        default="all",
    )
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument(
        "--log",
        type=Path,
        default=Path("bld/baremetal-logs/LatestOfThree.logs"),
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("bld/draw3d-captures/ui3-milestone"),
    )
    parser.add_argument("--static-seconds", type=float, default=2.0)
    parser.add_argument("--animated-seconds", type=float, default=10.0)
    parser.add_argument("--orbit-speed", type=float, default=0.8)
    parser.add_argument("--reset-settle", type=float, default=0.75)
    parser.add_argument("--log-timeout", type=float, default=10.0)
    parser.add_argument("--target-hz", type=float, default=55.0)
    parser.add_argument("--require-target-hz", action="store_true")
    parser.add_argument(
        "--expect-width",
        type=int,
        default=2560,
        help="expected Draw3D source/capture width (not a UI3 scanout readback)",
    )
    parser.add_argument(
        "--expect-height",
        type=int,
        default=1440,
        help="expected Draw3D source/capture height (not a UI3 scanout readback)",
    )
    args = parser.parse_args()

    if args.scenario == "self-test":
        self_test()
        return

    watcher = LogWatcher(args.log)
    if args.scenario in ("boot", "all"):
        verify_boot(watcher)
    if args.scenario in ("ownership", "all"):
        verify_ownership(watcher)
    if args.scenario in ("static", "all"):
        run_scene(args, watcher, "static")
    if args.scenario in ("animated", "all"):
        run_scene(args, watcher, "animated")
    if args.scenario in ("lifecycle", "all"):
        run_lifecycle(args, watcher)
    print(f"milestone_test=PASS scenario={args.scenario}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, TestFailure, RuntimeError, ValueError) as error:
        raise SystemExit(f"milestone_test=FAIL reason={error}") from error
