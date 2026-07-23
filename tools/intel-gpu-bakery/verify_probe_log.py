#!/usr/bin/env python3
"""Verify a canonical physical TestRig copy-rect probe transcript."""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path


CANONICAL_ZEBIN_SHA256 = (
    "b36d1c7742003591a5074663d81a4162412618ae425c47d30be6d068ee144a25"
)
MAX_LOG_BYTES = 16 * 1024 * 1024
MAX_RETIRE_MS = 250

SUMMARY_MARKER = "gpgpu probe copy-rect:"
CASE_MARKER = "gpgpu probe copy-rect case="
ALLOWED_ARTIFACT_SOURCES = ("embedded", "fs")


def _expected_summary(artifact_source: str) -> str:
    return (
        "gpgpu probe copy-rect: "
        "ok=1 reboot_required=0 "
        "frontend=cpp-for-opencl "
        "feature=intel_gpu_cpp_aot feature_enabled=1 "
        f"artifact=copy_rect_rgba8 artifact_source={artifact_source} "
        "target=adls verified=1 "
        "device=00:02.0-0x4680-r0C "
        f"hash={CANONICAL_ZEBIN_SHA256} "
        "cases=4/4 retired=4 passed=4 "
        "first_failure_case=none first_failure=none"
    )


EXPECTED_SUMMARY_SOURCES = {
    _expected_summary(source): source for source in ALLOWED_ARTIFACT_SOURCES
}


class ProbeLogError(RuntimeError):
    """The transcript does not prove the canonical physical TestRig result."""


@dataclass(frozen=True)
class CaseExpectation:
    label: str
    src_width: int
    src_height: int
    src_pitch: int
    src_x: int
    src_y: int
    dst_width: int
    dst_height: int
    dst_pitch: int
    dst_x: int
    dst_y: int
    copy_width: int
    copy_height: int

    @property
    def copied_pixels(self) -> int:
        return self.copy_width * self.copy_height

    @property
    def guard_pixels(self) -> int:
        return 2048 - self.copied_pixels

    def canonical_line(self, retire_token: str) -> str:
        return (
            f"{CASE_MARKER}{self.label}: "
            "attempted=1 submitted=1 retired=1 ok=1 "
            f"src={self.src_width}x{self.src_height} pitch={self.src_pitch} "
            f"origin=({self.src_x}, {self.src_y}) "
            f"dst={self.dst_width}x{self.dst_height} pitch={self.dst_pitch} "
            f"origin=({self.dst_x}, {self.dst_y}) "
            f"copy={self.copy_width}x{self.copy_height} "
            f"checked_copy={self.copied_pixels} checked_guards={self.guard_pixels} "
            "checked_source=2048 "
            "markers=[0xC0DEA701,0xC0DEA702] "
            f"retire_ms={retire_token} "
            "first_failure=none failure_has_offset=0 failure_offset=0x0 "
            "expected=0x00000000 observed=0x00000000"
        )


EXPECTED_CASES = (
    CaseExpectation("even-small", 27, 13, 128, 3, 2, 25, 13, 112, 5, 4, 8, 3),
    CaseExpectation("odd-small", 23, 14, 112, 4, 3, 29, 14, 128, 7, 5, 7, 4),
    CaseExpectation("even-multigroup", 48, 12, 208, 7, 2, 46, 12, 192, 5, 3, 34, 2),
    CaseExpectation("odd-multigroup", 44, 12, 192, 6, 3, 45, 12, 208, 7, 4, 33, 3),
)
EXPECTED_CASE_BY_LABEL = {case.label: case for case in EXPECTED_CASES}
EXPECTED_CASE_ORDER = tuple(case.label for case in EXPECTED_CASES)
CASE_LABEL_RE = re.compile(r"^gpgpu probe copy-rect case=([^:\s]+):")
RETIRE_TOKEN = "__RETIRE_MS__"


def _case_pattern(case: CaseExpectation) -> re.Pattern[str]:
    escaped = re.escape(case.canonical_line(RETIRE_TOKEN))
    escaped_token = re.escape(RETIRE_TOKEN)
    return re.compile(
        rf"\A{escaped.replace(escaped_token, r'(?P<retire_ms>[0-9]+)')}\Z"
    )


CASE_PATTERNS = {case.label: _case_pattern(case) for case in EXPECTED_CASES}


@dataclass(frozen=True)
class ProbeVerification:
    summary_line_number: int
    artifact_source: str
    case_line_numbers: tuple[int, ...]
    retire_ms: tuple[int, ...]


def _payload_from_line(line: str, marker: str) -> str | None:
    marker_offset = line.find(marker)
    if marker_offset < 0:
        return None
    # Prefixes from serial consoles, timestamps, or shell multiplexers are
    # outside the signed-off payload. Trailing transport whitespace is benign.
    return line[marker_offset:].rstrip()


def verify_probe_log(text: str) -> ProbeVerification:
    summaries: list[tuple[int, str]] = []
    case_payloads: list[tuple[int, str]] = []

    for line_number, line in enumerate(text.splitlines(), start=1):
        summary = _payload_from_line(line, SUMMARY_MARKER)
        if summary is not None:
            summaries.append((line_number, summary))

        case = _payload_from_line(line, CASE_MARKER)
        if case is not None:
            case_payloads.append((line_number, case))

    if len(summaries) != 1:
        raise ProbeLogError(
            "expected exactly one copy-rect summary line; "
            f"observed {len(summaries)}"
        )
    summary_line_number, summary = summaries[0]
    artifact_source = EXPECTED_SUMMARY_SOURCES.get(summary)
    if artifact_source is None:
        raise ProbeLogError(
            f"line {summary_line_number}: summary contradicts the canonical "
            "C++ TestRig identity or success result"
        )

    if len(case_payloads) != len(EXPECTED_CASES):
        raise ProbeLogError(
            f"expected exactly {len(EXPECTED_CASES)} case lines; "
            f"observed {len(case_payloads)}"
        )

    observed_labels: list[str] = []
    observed_lines: list[int] = []
    retire_times: list[int] = []
    seen_labels: set[str] = set()

    for line_number, payload in case_payloads:
        label_match = CASE_LABEL_RE.match(payload)
        if label_match is None:
            raise ProbeLogError(f"line {line_number}: malformed copy-rect case line")
        label = label_match.group(1)
        if label not in EXPECTED_CASE_BY_LABEL:
            raise ProbeLogError(f"line {line_number}: unexpected case {label!r}")
        if label in seen_labels:
            raise ProbeLogError(f"line {line_number}: duplicate case {label!r}")
        seen_labels.add(label)

        match = CASE_PATTERNS[label].fullmatch(payload)
        if match is None:
            raise ProbeLogError(
                f"line {line_number}: case {label!r} contradicts its canonical "
                "geometry, counters, markers, or success fields"
            )
        retire_ms = int(match.group("retire_ms"), 10)
        if retire_ms > MAX_RETIRE_MS:
            raise ProbeLogError(
                f"line {line_number}: case {label!r} retire_ms={retire_ms} "
                f"exceeds the {MAX_RETIRE_MS} ms probe timeout"
            )

        observed_labels.append(label)
        observed_lines.append(line_number)
        retire_times.append(retire_ms)

    if tuple(observed_labels) != EXPECTED_CASE_ORDER:
        raise ProbeLogError(
            "case order contradicts the canonical probe order: "
            f"observed {tuple(observed_labels)!r}"
        )
    if observed_lines[0] <= summary_line_number:
        raise ProbeLogError("case lines must follow the single summary line")

    return ProbeVerification(
        summary_line_number=summary_line_number,
        artifact_source=artifact_source,
        case_line_numbers=tuple(observed_lines),
        retire_ms=tuple(retire_times),
    )


def _read_log(path_text: str) -> str:
    if path_text == "-":
        data = sys.stdin.buffer.read(MAX_LOG_BYTES + 1)
        source = "stdin"
    else:
        path = Path(path_text).expanduser()
        if not path.is_file():
            raise ProbeLogError(f"log path is not a file: {path}")
        with path.open("rb") as log_file:
            data = log_file.read(MAX_LOG_BYTES + 1)
        source = str(path)

    if len(data) > MAX_LOG_BYTES:
        raise ProbeLogError(
            f"{source}: log exceeds the {MAX_LOG_BYTES}-byte verification limit"
        )
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ProbeLogError(f"{source}: log is not valid UTF-8") from error


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Verify the exact successful C++ copy-rect transcript from the "
            "physical ADL-S TestRig. Omit LOG or pass '-' to read stdin."
        )
    )
    parser.add_argument("log", nargs="?", default="-", metavar="LOG")
    args = parser.parse_args(argv)

    verification = verify_probe_log(_read_log(args.log))
    retire_csv = ",".join(str(value) for value in verification.retire_ms)
    print(
        "copy-rect TestRig transcript verified: "
        "device=00:02.0-0x4680-r0C "
        "frontend=cpp-for-opencl feature=intel_gpu_cpp_aot "
        f"artifact_source={verification.artifact_source} "
        f"zebin_sha256={CANONICAL_ZEBIN_SHA256} "
        f"cases=4/4 retire_ms=[{retire_csv}]"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ProbeLogError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
