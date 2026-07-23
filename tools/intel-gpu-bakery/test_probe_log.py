#!/usr/bin/env python3
"""Tests for the strict physical copy-rect transcript verifier."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from verify_probe_log import ProbeLogError, verify_probe_log


SCRIPT = Path(__file__).with_name("verify_probe_log.py")
HASH = "b36d1c7742003591a5074663d81a4162412618ae425c47d30be6d068ee144a25"
SUMMARY = (
    "gpgpu probe copy-rect: ok=1 reboot_required=0 "
    "frontend=cpp-for-opencl feature=intel_gpu_cpp_aot feature_enabled=1 "
    "artifact=copy_rect_rgba8 artifact_source=embedded target=adls verified=1 "
    "device=00:02.0-0x4680-r0C "
    f"hash={HASH} cases=4/4 retired=4 passed=4 "
    "first_failure_case=none first_failure=none"
)
CASES = (
    "gpgpu probe copy-rect case=even-small: "
    "attempted=1 submitted=1 retired=1 ok=1 "
    "src=27x13 pitch=128 origin=(3, 2) "
    "dst=25x13 pitch=112 origin=(5, 4) copy=8x3 "
    "checked_copy=24 checked_guards=2024 checked_source=2048 "
    "markers=[0xC0DEA701,0xC0DEA702] retire_ms=2 "
    "first_failure=none failure_has_offset=0 failure_offset=0x0 "
    "expected=0x00000000 observed=0x00000000",
    "gpgpu probe copy-rect case=odd-small: "
    "attempted=1 submitted=1 retired=1 ok=1 "
    "src=23x14 pitch=112 origin=(4, 3) "
    "dst=29x14 pitch=128 origin=(7, 5) copy=7x4 "
    "checked_copy=28 checked_guards=2020 checked_source=2048 "
    "markers=[0xC0DEA701,0xC0DEA702] retire_ms=0 "
    "first_failure=none failure_has_offset=0 failure_offset=0x0 "
    "expected=0x00000000 observed=0x00000000",
    "gpgpu probe copy-rect case=even-multigroup: "
    "attempted=1 submitted=1 retired=1 ok=1 "
    "src=48x12 pitch=208 origin=(7, 2) "
    "dst=46x12 pitch=192 origin=(5, 3) copy=34x2 "
    "checked_copy=68 checked_guards=1980 checked_source=2048 "
    "markers=[0xC0DEA701,0xC0DEA702] retire_ms=17 "
    "first_failure=none failure_has_offset=0 failure_offset=0x0 "
    "expected=0x00000000 observed=0x00000000",
    "gpgpu probe copy-rect case=odd-multigroup: "
    "attempted=1 submitted=1 retired=1 ok=1 "
    "src=44x12 pitch=192 origin=(6, 3) "
    "dst=45x12 pitch=208 origin=(7, 4) copy=33x3 "
    "checked_copy=99 checked_guards=1949 checked_source=2048 "
    "markers=[0xC0DEA701,0xC0DEA702] retire_ms=250 "
    "first_failure=none failure_has_offset=0 failure_offset=0x0 "
    "expected=0x00000000 observed=0x00000000",
)


def canonical_log(*, prefix: str = "") -> str:
    lines = [SUMMARY, *CASES]
    return "\n".join(f"{prefix}{line}" for line in lines) + "\n"


def replace_line(text: str, index: int, old: str, new: str) -> str:
    lines = text.splitlines()
    lines[index] = lines[index].replace(old, new)
    return "\n".join(lines) + "\n"


class ProbeLogVerifierTests(unittest.TestCase):
    def test_accepts_exact_transcript(self) -> None:
        result = verify_probe_log(canonical_log())
        self.assertEqual(result.summary_line_number, 1)
        self.assertEqual(result.artifact_source, "embedded")
        self.assertEqual(result.case_line_numbers, (2, 3, 4, 5))
        self.assertEqual(result.retire_ms, (2, 0, 17, 250))

    def test_accepts_exact_allowlisted_filesystem_artifact(self) -> None:
        text = canonical_log().replace(
            "artifact_source=embedded", "artifact_source=fs", 1
        )
        result = verify_probe_log(text)
        self.assertEqual(result.artifact_source, "fs")

    def test_tolerates_shell_prefixes_and_unrelated_lines(self) -> None:
        prefixed = canonical_log(prefix="[serial0 17:42:01] shell> ")
        text = "boot noise\ngpgpu probe copy-rect\n" + prefixed + "prompt> \n"
        result = verify_probe_log(text)
        self.assertEqual(result.summary_line_number, 3)
        self.assertEqual(result.case_line_numbers, (4, 5, 6, 7))

    def test_rejects_missing_summary_or_case(self) -> None:
        with self.assertRaisesRegex(ProbeLogError, "summary"):
            verify_probe_log("\n".join(CASES) + "\n")
        with self.assertRaisesRegex(ProbeLogError, "exactly 4 case"):
            verify_probe_log("\n".join((SUMMARY, *CASES[:-1])) + "\n")

    def test_rejects_duplicate_summary_even_when_identical(self) -> None:
        text = "\n".join((SUMMARY, SUMMARY, *CASES)) + "\n"
        with self.assertRaisesRegex(ProbeLogError, "exactly one"):
            verify_probe_log(text)

    def test_rejects_duplicate_or_unknown_case(self) -> None:
        duplicate = "\n".join((SUMMARY, CASES[0], CASES[0], CASES[2], CASES[3])) + "\n"
        with self.assertRaisesRegex(ProbeLogError, "duplicate case"):
            verify_probe_log(duplicate)

        unknown = CASES[3].replace("odd-multigroup", "invented")
        text = "\n".join((SUMMARY, *CASES[:3], unknown)) + "\n"
        with self.assertRaisesRegex(ProbeLogError, "unexpected case"):
            verify_probe_log(text)

    def test_rejects_summary_identity_and_status_contradictions(self) -> None:
        contradictions = (
            ("ok=1", "ok=0"),
            ("reboot_required=0", "reboot_required=1"),
            ("frontend=cpp-for-opencl", "frontend=opencl-c"),
            ("feature=intel_gpu_cpp_aot", "feature=other"),
            ("feature_enabled=1", "feature_enabled=0"),
            ("artifact_source=embedded", "artifact_source=other"),
            ("target=adls", "target=other"),
            ("verified=1", "verified=0"),
            ("00:02.0", "00:03.0"),
            ("0x4680", "0x46D1"),
            ("r0C", "r0B"),
            (HASH, "0" * 64),
            ("cases=4/4", "cases=3/4"),
            ("retired=4", "retired=3"),
            ("passed=4", "passed=3"),
            ("first_failure=none", "first_failure=walker"),
        )
        for old, new in contradictions:
            with self.subTest(old=old, new=new):
                text = canonical_log().replace(old, new, 1)
                with self.assertRaisesRegex(ProbeLogError, "summary contradicts"):
                    verify_probe_log(text)

    def test_rejects_case_geometry_counter_marker_and_failure_changes(self) -> None:
        contradictions = (
            (1, "src=27x13", "src=28x13"),
            (1, "origin=(3, 2)", "origin=(2, 3)"),
            (2, "copy=7x4", "copy=8x4"),
            (3, "checked_copy=68", "checked_copy=67"),
            (3, "checked_guards=1980", "checked_guards=1981"),
            (4, "checked_source=2048", "checked_source=2047"),
            (4, "0xC0DEA701", "0xC0DEA700"),
            (1, "submitted=1", "submitted=0"),
            (2, "first_failure=none", "first_failure=mismatch"),
            (3, "failure_has_offset=0", "failure_has_offset=1"),
            (4, "failure_offset=0x0", "failure_offset=0x4"),
            (1, "observed=0x00000000", "observed=0x00000001"),
        )
        for line_index, old, new in contradictions:
            with self.subTest(line_index=line_index, old=old, new=new):
                text = replace_line(canonical_log(), line_index, old, new)
                with self.assertRaisesRegex(ProbeLogError, "contradicts"):
                    verify_probe_log(text)

    def test_rejects_out_of_order_cases(self) -> None:
        text = "\n".join((SUMMARY, CASES[1], CASES[0], CASES[2], CASES[3])) + "\n"
        with self.assertRaisesRegex(ProbeLogError, "case order"):
            verify_probe_log(text)

    def test_rejects_case_before_summary(self) -> None:
        text = "\n".join((CASES[0], SUMMARY, *CASES[1:])) + "\n"
        with self.assertRaisesRegex(ProbeLogError, "must follow"):
            verify_probe_log(text)

    def test_rejects_retirement_beyond_timeout(self) -> None:
        text = replace_line(canonical_log(), 4, "retire_ms=250", "retire_ms=251")
        with self.assertRaisesRegex(ProbeLogError, "exceeds"):
            verify_probe_log(text)

    def test_cli_accepts_stdin_and_log_path(self) -> None:
        stdin_run = subprocess.run(
            [sys.executable, "-B", str(SCRIPT)],
            input=canonical_log(),
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(stdin_run.returncode, 0, stdin_run.stderr)
        self.assertIn("transcript verified", stdin_run.stdout)
        self.assertIn(HASH, stdin_run.stdout)

        with tempfile.TemporaryDirectory() as temp_dir:
            log_path = Path(temp_dir) / "testrig.log"
            log_path.write_text(canonical_log(prefix="serial: "), encoding="utf-8")
            path_run = subprocess.run(
                [sys.executable, "-B", str(SCRIPT), str(log_path)],
                text=True,
                capture_output=True,
                check=False,
            )
        self.assertEqual(path_run.returncode, 0, path_run.stderr)
        self.assertIn("cases=4/4", path_run.stdout)

    def test_cli_rejects_contradictory_stdin(self) -> None:
        bad_log = canonical_log().replace("feature_enabled=1", "feature_enabled=0", 1)
        run = subprocess.run(
            [sys.executable, "-B", str(SCRIPT), "-"],
            input=bad_log,
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(run.returncode, 2)
        self.assertIn("error:", run.stderr)


if __name__ == "__main__":
    unittest.main()
