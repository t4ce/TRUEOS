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
    output_frame_lines = [
        line for line in log.splitlines() if "ui3-compositor: output frame" in line
    ]
    owner_lines = [
        line for line in log.splitlines() if "ui3-overlay-owner claimed=1" in line
    ]
    require(proof_lines, "missing UI3 proof-source diagnostic")
    require(output_proof_lines, "missing UI3 composed-output diagnostic")
    require(output_frame_lines, "missing UI3 output-frame diagnostic")
    require(boot_lines, "missing independent UI3 bootstrap diagnostic")
    require(owner_lines, "missing exclusive UI3 overlay-owner claim")
    proof = fields(proof_lines[-1])
    output_proof = fields(output_proof_lines[-1])
    output_frame = fields(output_frame_lines[-1])
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
    require(
        output_frame.get("clear") == "gpu-2d-walker",
        f"UI3 clear is not the native 2D dispatch: {output_frame_lines[-1]}",
    )
    require(boot.get("complete") == "1", f"UI3 bootstrap failed: {boot_lines[-1]}")
    require(boot.get("layers") == "1", f"boot is not proof-only: {boot_lines[-1]}")
    require(boot.get("tcp_dependency") == "none", "bootstrap still depends on TCP")
    print(
        "boot_proof=PASS source_dense_exact=1 output_dense_exact=1 "
        "clear=gpu-2d-walker layers=1 tcp_dependency=none owner=ui3-compositor"
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
    obsolete_probe_lines = [
        line
        for line in log.splitlines()
        if "font-boot-tessel" in line or "font-tessel-boot-probe" in line
    ]
    if obsolete_probe_lines:
        raise TestFailure(f"obsolete font boot probe still ran: {obsolete_probe_lines[-1]}")
    forbidden = [
        line
        for line in owned_log.splitlines()
        if "overlay-arm reason=font-tessel-render-target" in line
        or "overlay-present seq=" in line
        or "rgba-tile-overlay-present" in line
        or "live-overlay-present" in line
    ]
    if forbidden:
        raise TestFailure(f"legacy presenter bypassed UI3 ownership: {forbidden[-1]}")
    print(
        f"ownership=PASS owner=ui3-compositor legacy_rejections={len(rejection_lines)} "
        "accepted_legacy_presents=0 obsolete_boot_probes=0 lifetime=service"
    )


def parse_run_summary(line, expected_mode):
    data = fields(line)
    require(data.get("mode") == expected_mode, f"unexpected run mode: {line}")
    attempted = int(data["attempted"])
    presented = int(data["presented"])
    require(attempted > 0, f"no frames attempted: {line}")
    require(presented == attempted, f"incomplete presentation run: {line}")
    return data


def percentile(values, numerator, denominator):
    require(values, "cannot calculate percentile of an empty sample")
    ordered = sorted(values)
    rank = max(1, math.ceil(len(ordered) * numerator / denominator))
    return ordered[min(rank - 1, len(ordered) - 1)]


def validate_ui3_timing_samples(log, mode):
    samples = []
    for line in log.splitlines():
        if "ui3-compositor: output frame" not in line:
            continue
        sample = fields(line)
        if sample.get("layers") != "2":
            continue
        require(sample.get("present") == "1", f"UI3 sampled a failed present: {line}")
        require(
            sample.get("clear") == "gpu-2d-walker",
            f"UI3 scene clear is not the native 2D dispatch: {line}",
        )
        try:
            samples.append(
                {
                    key: int(sample[key])
                    for key in (
                        "acquire_us",
                        "clear_us",
                        "layers_us",
                        "draw3d_us",
                        "proof_us",
                        "blend_us",
                        "commit_us",
                        "total_us",
                    )
                }
            )
        except (KeyError, ValueError) as error:
            raise TestFailure(f"malformed UI3 timing sample: {line}") from error
    require(samples, f"no two-layer UI3 timing samples were logged for {mode}")
    clear = [sample["clear_us"] for sample in samples]
    blend = [sample["blend_us"] for sample in samples]
    commit = [sample["commit_us"] for sample in samples]
    total = [sample["total_us"] for sample in samples]
    result = {
        "count": len(samples),
        "clear_median_us": percentile(clear, 1, 2),
        "clear_p95_us": percentile(clear, 95, 100),
        "blend_median_us": percentile(blend, 1, 2),
        "commit_median_us": percentile(commit, 1, 2),
        "total_median_us": percentile(total, 1, 2),
        "total_p95_us": percentile(total, 95, 100),
    }
    print(
        f"ui3_components=PASS mode={mode} samples={result['count']} "
        "clear=gpu-2d-walker "
        f"clear_median_us={result['clear_median_us']} "
        f"clear_p95_us={result['clear_p95_us']} "
        f"blend_median_us={result['blend_median_us']} "
        f"commit_median_us={result['commit_median_us']} "
        f"total_median_us={result['total_median_us']} "
        f"total_p95_us={result['total_p95_us']}"
    )
    return result


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
        stopped_line = watcher.wait_line(mark, "draw3d: scene stopped", timeout=args.log_timeout)
        stopped = fields(stopped_line)
        require(
            stopped.get("overlay_cleared") == "1",
            f"scene stop did not reach the retained-frame boundary: {stopped_line}",
        )
        validate_capture(
            client,
            args.output_dir / f"ui3-milestone-{mode}-scene.png",
            args.expect_width,
            args.expect_height,
        )
        run_log = watcher.since(mark)
    finally:
        client.close()

    data = parse_run_summary(summary_line, mode)
    timing = validate_ui3_timing_samples(run_log, mode)
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
        avg_ui3_us = int(data["avg_ui3_us"])
        ui3_delta_us = avg_ui3_us - args.baseline_ui3_us
        clear_delta_us = timing["clear_median_us"] - args.baseline_clear_us
        print(
            f"animated_functional=PASS attempted={attempted} presented={data['presented']} "
            f"present_hz={hz:.2f} avg_frame_us={data['avg_frame_us']} "
            f"avg_ui3_us={avg_ui3_us} baseline_ui3_us={args.baseline_ui3_us} "
            f"ui3_delta_us={ui3_delta_us:+d} baseline_clear_us={args.baseline_clear_us} "
            f"clear_median_delta_us={clear_delta_us:+d} "
            f"performance_target_hz={args.target_hz:.2f} "
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
    class StaticWatcher:
        def __init__(self, log):
            self.log = log

        def text(self):
            return self.log

    sample = (
        "draw3d-run-summary revision=4 mode=animated elapsed_ms=5000 attempted=31 "
        "presented=31 present_hz=6.20 avg_frame_us=158000 max_frame_us=180000 "
        "avg_ui3_us=140000 max_ui3_us=160000 over_budget=31 frame_budget_us=16667"
    )
    parsed = fields(sample)
    require(parsed["mode"] == "animated", "field parser lost mode")
    require(float(parsed["present_hz"]) == 6.20, "field parser lost decimal cadence")
    require(int(parsed["attempted"]) == 31, "field parser lost integer count")
    verify_ownership(
        StaticWatcher(
            "intel/display: ui3-overlay-owner claimed=1 owner=ui3-compositor\n"
            "custom-triangle end seq=2 submit=font-tessel-3d-once "
            "target=scratch completed=1\n"
        )
    )
    try:
        verify_ownership(
            StaticWatcher(
                "intel/display: ui3-overlay-owner claimed=1 owner=ui3-compositor\n"
                "font-boot-tessel: begin delay_s=10\n"
            )
        )
    except TestFailure:
        pass
    else:
        raise TestFailure("ownership self-test accepted the obsolete font boot probe")
    timing = validate_ui3_timing_samples(
        "ui3-compositor: output frame seq=2 output=D01 layers=2 "
        "clear=gpu-2d-walker present=1 acquire_us=1000 clear_us=2000 "
        "layers_us=3000 draw3d_us=2200 proof_us=800 blend_us=3000 "
        "commit_us=4000 total_us=10000\n",
        "self-test",
    )
    require(timing["clear_median_us"] == 2000, "timing parser lost clear cost")
    require(timing["total_p95_us"] == 10000, "timing parser lost total cost")
    print(
        "self_test=PASS parser=key-value cadence=decimal ownership=clean-boot "
        "on_demand_font=allowed ui3_timing=component-level"
    )


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
    parser.add_argument(
        "--baseline-ui3-us",
        type=int,
        default=160_054,
        help="previous measured animated average UI3 frame cost",
    )
    parser.add_argument(
        "--baseline-clear-us",
        type=int,
        default=32_000,
        help="previous measured full-output worklist clear cost",
    )
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
