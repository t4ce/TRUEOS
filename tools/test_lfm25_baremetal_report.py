#!/usr/bin/env python3
"""Regression tests for the LFM2.5 bare-metal campaign report."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))

from lfm25_baremetal_report import (  # noqa: E402
    CANONICAL_REPLIES,
    PACKED_MODEL_SHA256,
    SIGNATURE_LOGICAL_BYTES,
    expected_done_counts,
    expected_prefill_counts,
    parse_log_text,
    signature_gbps,
    validate_turn,
)


SCRIPT = TOOLS / "lfm25_baremetal_report.py"
GPU_HZ = 12_000_000


def turn_record(
    stage: str,
    *,
    prompt: int = 10,
    reply: int = 0,
    elapsed: int = 0,
    callbacks: int = 0,
    projections: int = 0,
    submissions: int = 0,
    submit_ms: int = 0,
    completion_us: int = 0,
    gpu_us: int = 0,
    gpu_samples: int | None = None,
    failures: int = 0,
    stop: str = "pending",
    digest: str = "-",
    turn: int = 1,
    first_token: int = 0,
) -> str:
    if gpu_samples is None:
        gpu_samples = submissions
    return (
        "[global] [info] lfm25: turn "
        f"stage={stage} scope=turn turn={turn} elapsed_ms={elapsed} "
        f"prompt_tokens={prompt} reply_tokens={reply} stop={stop} "
        f"context_before=0 callbacks={callbacks} "
        f"igpu_projections={projections} igpu_submissions={submissions} "
        f"igpu_failures={failures} igpu_submit_ms={submit_ms} "
        f"phase_us=encode:0,admission:0,completion:{completion_us},gpu:{gpu_us} "
        f"gpu_samples={gpu_samples} gpu_hz={GPU_HZ} last_rows=1024 "
        f"first_token={first_token} raw_reply_sha256={digest}"
    )


def signature_record(
    label: str,
    submissions: int,
    projections: int,
    *,
    turn: int = 1,
    gpu_us_per_submission: int = 4_000,
    completion_us_per_submission: int = 5_000,
    submit_ms_per_submission: int = 6,
) -> str:
    gpu_us = submissions * gpu_us_per_submission
    completion_us = submissions * completion_us_per_submission
    submit_ms = submissions * submit_ms_per_submission
    return (
        "[global] [info] lfm25: turn-signature "
        f"stage=done scope=turn turn={turn} signature={label} "
        f"submissions={submissions} projections={projections} "
        f"submit_ms={submit_ms} completion_us={completion_us} gpu_us={gpu_us} "
        f"gpu_samples={submissions} "
        f"avg_us=completion:{completion_us_per_submission},"
        f"gpu:{gpu_us_per_submission} "
        "range_submit_ms=6:6 range_completion_us=5000:5000 "
        "range_gpu_us=4000:4000 extrema_valid=submission:1,gpu:1"
    )


def cpu_record(*, projection_calls: int = 1_226, turn: int = 1) -> str:
    return (
        "[global] [info] lfm25: turn-cpu "
        f"stage=done scope=turn turn={turn} "
        "attention_calls=114 attention_positions=2000 attention_us=250000 "
        f"projection_calls={projection_calls} "
        "projection_prepare_us=150000 projection_quantize_us=300000 "
        "projection_batch_us=8000000"
    )


def canonical_hi_log(*, include_signatures: bool = True) -> str:
    prefill = expected_prefill_counts(10)
    done = expected_done_counts(10, 9, "eot")
    lines = [
        "unrelated boot record",
        (
            "[global] [info] lfm25: packed model ready "
            "weight_layout=pair1088-x16-dp4a bytes=376701952 tensors=93 "
            "block_tiles=692224 quantized_values=354418688 "
            "subnormal_scales=25994 pack_seal_ms=2753 "
            f"sha256={PACKED_MODEL_SHA256}"
        ),
        (
            "[global] [info] lfm25: resident stage=ready scope=session "
            "open_ms=4300 tokenizer_open_ms=12 model_open_ms=4288 "
            "executor_slot=2 core_kind=Performance "
            "backend=cpu+intel-igc-q8 completion=guc-rcs"
        ),
        turn_record("start"),
        "Spirit and network noise",
        turn_record(
            "prefill",
            elapsed=5_000,
            callbacks=prefill.callbacks,
            projections=prefill.projections,
            submissions=prefill.submissions,
            submit_ms=prefill.submissions * 6,
            completion_us=prefill.submissions * 5_000,
            gpu_us=prefill.submissions * 4_000,
            first_token=36_309,
        ),
        turn_record(
            "done",
            reply=9,
            elapsed=10_000,
            callbacks=done.callbacks,
            projections=done.projections,
            submissions=done.submissions,
            submit_ms=done.submissions * 6,
            completion_us=done.submissions * 5_000,
            gpu_us=done.submissions * 4_000,
            stop="eot",
            digest=CANONICAL_REPLIES[0].sha256,
            first_token=36_309,
        ),
    ]
    if include_signatures:
        # Per fixed token: 10/16/6/16/16/0 submissions, with vocabulary on
        # each full token.  Nine state-only plus ten full tokens gives:
        buckets = {
            "shortconv-in": (190, 190),
            "hidden": (304, 304),
            "attention-qkv": (114, 342),
            "ffn-gate-up": (304, 608),
            "ffn-down": (304, 304),
            "vocabulary": (10, 10),
            "unknown": (0, 0),
        }
        lines.extend(
            signature_record(label, submissions, projections)
            for label, (submissions, projections) in buckets.items()
        )
        lines.append(cpu_record())
    return "\n".join(lines) + "\n"


class Lfm25BaremetalReportTests(unittest.TestCase):
    def test_eot_and_limit_fixed_graph_formulas(self) -> None:
        self.assertEqual(
            expected_prefill_counts(10).__dict__,
            {"callbacks": 972, "projections": 921, "submissions": 641},
        )
        self.assertEqual(
            expected_done_counts(10, 9, "eot").__dict__,
            {"callbacks": 1863, "projections": 1758, "submissions": 1226},
        )
        self.assertEqual(
            expected_done_counts(10, 48, "limit").__dict__,
            {"callbacks": 5625, "projections": 5292, "submissions": 3696},
        )

    def test_parses_noise_context_and_valid_canonical_turn(self) -> None:
        parsed = parse_log_text(canonical_hi_log(), "capture.log")
        self.assertEqual(len(parsed.turns), 1)
        result = validate_turn(parsed.turns[0])
        self.assertTrue(result.passed, result.issues)
        self.assertEqual(result.canonical.name, "hi")
        self.assertEqual(result.metrics.prefill_ms, 5_000)
        self.assertEqual(result.metrics.reply_ms, 5_000)
        self.assertAlmostEqual(result.metrics.reply_tokens_per_second, 1.8)
        self.assertEqual(result.metrics.completion_gap_us, 1_226_000)
        self.assertEqual(len(parsed.turns[0].signatures), 7)
        self.assertEqual(parsed.turns[0].cpu.fields["projection_calls"], "1226")
        self.assertEqual(result.metrics.projection_batch_residual_us, 644_000)
        vocabulary = parsed.turns[0].signatures["vocabulary"]
        expected_gbps = (
            SIGNATURE_LOGICAL_BYTES["vocabulary"] * 10 / (40_000 * 1_000)
        )
        self.assertAlmostEqual(signature_gbps(vocabulary), expected_gbps)

    def test_recognizes_canonical_sky_hash(self) -> None:
        prefill = expected_prefill_counts(21)
        done = expected_done_counts(21, 22, "eot")
        text = "\n".join(
            (
                turn_record("start", prompt=21),
                turn_record(
                    "prefill",
                    prompt=21,
                    elapsed=12_000,
                    callbacks=prefill.callbacks,
                    projections=prefill.projections,
                    submissions=prefill.submissions,
                    submit_ms=prefill.submissions * 6,
                    completion_us=prefill.submissions * 5_000,
                    gpu_us=prefill.submissions * 4_000,
                    first_token=123,
                ),
                turn_record(
                    "done",
                    prompt=21,
                    reply=22,
                    elapsed=25_000,
                    callbacks=done.callbacks,
                    projections=done.projections,
                    submissions=done.submissions,
                    submit_ms=done.submissions * 6,
                    completion_us=done.submissions * 5_000,
                    gpu_us=done.submissions * 4_000,
                    stop="eot",
                    digest=CANONICAL_REPLIES[1].sha256,
                    first_token=123,
                ),
            )
        )
        result = validate_turn(parse_log_text(text).turns[0])
        self.assertTrue(result.passed, result.issues)
        self.assertEqual(result.canonical.name, "sky")

    def test_repeated_turn_one_is_associated_sequentially(self) -> None:
        one = canonical_hi_log(include_signatures=False)
        two = canonical_hi_log(include_signatures=False)
        parsed = parse_log_text(one + two, "two-sessions.log")
        self.assertEqual(len(parsed.turns), 2)
        self.assertEqual([turn.declared_turn for turn in parsed.turns], ["1", "1"])
        self.assertTrue(all(validate_turn(turn).passed for turn in parsed.turns))
        self.assertNotEqual(
            parsed.turns[0].resident.line_number,
            parsed.turns[1].resident.line_number,
        )

    def test_rejects_bad_hash_counts_samples_and_unknown_bucket(self) -> None:
        text = canonical_hi_log()
        text = text.replace(
            f"raw_reply_sha256={CANONICAL_REPLIES[0].sha256}",
            "raw_reply_sha256=" + "0" * 64,
        )
        text = text.replace(
            "callbacks=1863 igpu_projections=1758",
            "callbacks=1862 igpu_projections=1758",
        )
        text = text.replace(
            "gpu_samples=1226 gpu_hz",
            "gpu_samples=1225 gpu_hz",
        )
        text = text.replace(
            "signature=unknown submissions=0 projections=0",
            "signature=unknown submissions=1 projections=1",
        )
        text = text.replace("projection_calls=1226", "projection_calls=1225")
        result = validate_turn(parse_log_text(text).turns[0])
        joined = "\n".join(result.issues)
        self.assertIn("raw_reply_sha256", joined)
        self.assertIn("callbacks=1862", joined)
        self.assertIn("gpu_samples=1225", joined)
        self.assertIn("unknown signature bucket is nonzero", joined)
        self.assertIn("signature submissions sum=1227", joined)
        self.assertIn("cpu projection_calls=1225", joined)

    def test_partial_bucket_schema_is_flagged_but_absent_schema_is_allowed(self) -> None:
        without = validate_turn(
            parse_log_text(canonical_hi_log(include_signatures=False)).turns[0]
        )
        self.assertTrue(without.passed, without.issues)

        partial_text = canonical_hi_log(include_signatures=False)
        partial_text += signature_record("vocabulary", 10, 10) + "\n"
        partial = validate_turn(parse_log_text(partial_text).turns[0])
        self.assertFalse(partial.passed)
        self.assertTrue(
            any("signature buckets missing" in issue for issue in partial.issues)
        )
        self.assertTrue(
            any("signature submissions sum" in issue for issue in partial.issues)
        )

    def test_cli_legacy_policy_json_and_multiple_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            legacy = root / "legacy.log"
            legacy.write_text("old unrelated log\n", encoding="utf-8")
            good = root / "good.log"
            good.write_text(canonical_hi_log(), encoding="utf-8")

            default = subprocess.run(
                [sys.executable, str(SCRIPT), str(legacy)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(default.returncode, 0, default.stderr)
            self.assertIn("no turn telemetry", default.stdout)

            required = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--require-turns",
                    str(legacy),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(required.returncode, 1)

            report = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--json",
                    "--expect-runs",
                    "1",
                    str(legacy),
                    str(good),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(report.returncode, 0, report.stderr)
            document = json.loads(report.stdout)
            self.assertEqual(document["summary"]["completed"], 1)
            self.assertEqual(document["summary"]["passed"], 1)
            self.assertEqual(document["runs"][0]["turn"], "1")

    def test_cli_failed_completed_turn_is_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "bad.log"
            path.write_text(
                canonical_hi_log().replace(
                    "igpu_failures=0 igpu_submit_ms=7356",
                    "igpu_failures=1 igpu_submit_ms=7356",
                ),
                encoding="utf-8",
            )
            completed = subprocess.run(
                [sys.executable, str(SCRIPT), str(path)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(completed.returncode, 1)
            self.assertIn("status=FAIL", completed.stdout)
            self.assertIn("igpu_failures=1", completed.stdout)

    def test_expect_runs_requires_signature_and_cpu_detail(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "summary-only.log"
            path.write_text(
                canonical_hi_log(include_signatures=False),
                encoding="utf-8",
            )
            report = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--expect-runs",
                    "1",
                    str(path),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(report.returncode, 0)
            self.assertIn("missing turn-signature", report.stdout)
            self.assertIn("missing turn-cpu", report.stdout)


if __name__ == "__main__":
    unittest.main()
