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
    RCS_PROBE_BUCKET_SCHEMA,
    SIGNATURE_LOGICAL_BYTES,
    expected_done_counts,
    expected_prefill_counts,
    parse_log_text,
    parse_rcs_probe_buckets,
    parse_rcs_probe_phase_us,
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
    context_before: int = 0,
) -> str:
    if gpu_samples is None:
        gpu_samples = submissions
    return (
        "[global] [info] lfm25: turn "
        f"stage={stage} scope=turn turn={turn} elapsed_ms={elapsed} "
        f"prompt_tokens={prompt} reply_tokens={reply} stop={stop} "
        f"context_before={context_before} callbacks={callbacks} "
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


def rcs_probe_record(*, turn: int = 1, phase_delta: int = 0) -> str:
    sample_counts = {
        "shortconv-in": (4, 3),
        "hidden": (3, 2),
        "attention-qkv": (2, 2),
        "ffn-gate-up": (2, 1),
        "ffn-down": (1, 1),
        "vocabulary": (1, 1),
        "unknown": (0, 0),
    }
    aggregate_samples = 0
    aggregate_valid = 0
    aggregate_phases = [0] * 6
    buckets = []
    for label, (samples, valid) in sample_counts.items():
        phases = [
            2 * valid,
            valid,
            4 * valid,
            valid,
            2 * valid,
            10 * valid,
        ]
        if label == "vocabulary":
            phases[-1] -= phase_delta
        aggregate_samples += samples
        aggregate_valid += valid
        aggregate_phases = [
            total + value
            for total, value in zip(aggregate_phases, phases)
        ]
        buckets.append(
            ":".join(
                [
                    label,
                    str(samples),
                    str(valid),
                    *(str(value) for value in phases),
                ]
            )
        )
    invalid = aggregate_samples - aggregate_valid
    return (
        "[global] [info] lfm25: turn-rcs-probe "
        f"stage=done scope=turn turn={turn} schema=1 "
        f"samples={aggregate_samples} valid={aggregate_valid} "
        f"invalid={invalid} "
        "phase_us="
        f"queue_to_batch:{aggregate_phases[0]},"
        f"preamble:{aggregate_phases[1]},"
        f"walkers:{aggregate_phases[2]},"
        f"epilogue:{aggregate_phases[3]},"
        f"release_to_observe:{aggregate_phases[4]},"
        f"queue_to_observe:{aggregate_phases[5]} "
        f"gpu_hz={GPU_HZ} policy=first+power-of-two-per-signature "
        f"clock=rcs-36bit bucket_schema={RCS_PROBE_BUCKET_SCHEMA} "
        f"buckets={'|'.join(buckets)}"
    )


def signature_buckets(prompt: int, reply: int) -> dict[str, tuple[int, int]]:
    layer_tokens = prompt + reply
    full_tokens = 1 + reply
    return {
        "shortconv-in": (10 * layer_tokens, 10 * layer_tokens),
        "hidden": (16 * layer_tokens, 16 * layer_tokens),
        "attention-qkv": (6 * layer_tokens, 18 * layer_tokens),
        "ffn-gate-up": (16 * layer_tokens, 32 * layer_tokens),
        "ffn-down": (16 * layer_tokens, 16 * layer_tokens),
        "vocabulary": (full_tokens, full_tokens),
        "unknown": (0, 0),
    }


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
        buckets = signature_buckets(10, 9)
        lines.extend(
            signature_record(label, submissions, projections)
            for label, (submissions, projections) in buckets.items()
        )
        lines.append(cpu_record())
    return "\n".join(lines) + "\n"


def canonical_sky_log() -> str:
    prefill = expected_prefill_counts(21)
    done = expected_done_counts(21, 22, "eot")
    lines = [
        (
            "[global] [info] lfm25: resident stage=ready scope=session "
            "open_ms=20 tokenizer_open_ms=5 model_open_ms=15 "
            "executor_slot=2 core_kind=Performance "
            "backend=cpu+intel-igc-q8 completion=guc-rcs"
        ),
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
            first_token=1_098,
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
            first_token=1_098,
        ),
    ]
    lines.extend(
        signature_record(label, submissions, projections)
        for label, (submissions, projections) in signature_buckets(21, 22).items()
    )
    lines.append(cpu_record(projection_calls=done.submissions))
    return "\n".join(lines) + "\n"


def valid_warm_turn(
    *,
    turn: int,
    context_before: int,
    prompt: int,
    reply: int,
    first_token: int,
    digest: str,
) -> str:
    prefill = expected_prefill_counts(prompt)
    done = expected_done_counts(prompt, reply, "eot")
    lines = [
        turn_record(
            "start",
            prompt=prompt,
            turn=turn,
            context_before=context_before,
        ),
        turn_record(
            "prefill",
            prompt=prompt,
            elapsed=8_000,
            callbacks=prefill.callbacks,
            projections=prefill.projections,
            submissions=prefill.submissions,
            submit_ms=prefill.submissions * 6,
            completion_us=prefill.submissions * 5_000,
            gpu_us=prefill.submissions * 4_000,
            first_token=first_token,
            turn=turn,
            context_before=context_before,
        ),
        turn_record(
            "done",
            prompt=prompt,
            reply=reply,
            elapsed=16_000,
            callbacks=done.callbacks,
            projections=done.projections,
            submissions=done.submissions,
            submit_ms=done.submissions * 6,
            completion_us=done.submissions * 5_000,
            gpu_us=done.submissions * 4_000,
            stop="eot",
            digest=digest,
            first_token=first_token,
            turn=turn,
            context_before=context_before,
        ),
    ]
    lines.extend(
        signature_record(
            label,
            submissions,
            projections,
            turn=turn,
        )
        for label, (submissions, projections) in signature_buckets(
            prompt, reply
        ).items()
    )
    lines.append(
        cpu_record(
            projection_calls=done.submissions,
            turn=turn,
        )
    )
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
                    first_token=1_098,
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

    def test_old_m1_log_and_unknown_lfm25_lines_remain_compatible(self) -> None:
        text = canonical_hi_log()
        text = text.replace(
            "Spirit and network noise",
            "\n".join(
                (
                    "[global] [info] lfm25: future-record schema=99 value=1",
                    "[global] [info] lfm25: turn-rcs-probe-v2 "
                    "stage=done turn=1 opaque=true",
                )
            ),
        )
        capture = parse_log_text(text).turns[0]
        self.assertIsNone(capture.rcs_probe)
        result = validate_turn(capture)
        self.assertTrue(result.passed, result.issues)

    def test_parses_binds_and_reports_schema_one_rcs_probe(self) -> None:
        text = canonical_hi_log() + rcs_probe_record() + "\n"
        capture = parse_log_text(text, "probe.log").turns[0]
        self.assertIsNotNone(capture.rcs_probe)
        self.assertEqual(capture.rcs_probe.fields["turn"], "1")
        phases = parse_rcs_probe_phase_us(capture.rcs_probe)
        self.assertEqual(phases["queue_to_observe"], 100)
        buckets = parse_rcs_probe_buckets(capture.rcs_probe)
        self.assertEqual(buckets["shortconv-in"].samples, 4)
        self.assertEqual(buckets["shortconv-in"].valid, 3)
        self.assertEqual(
            buckets["attention-qkv"].phase_us["walkers"],
            8,
        )
        result = validate_turn(capture)
        self.assertTrue(result.passed, result.issues)

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "probe.log"
            path.write_text(text, encoding="utf-8")
            report = subprocess.run(
                [sys.executable, str(SCRIPT), str(path)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(report.returncode, 0, report.stdout + report.stderr)
            self.assertIn(
                "rcs_probe line=",
                report.stdout,
            )
            self.assertIn("queue_to_observe_us=100", report.stdout)
            self.assertIn(
                "shortconv-in  samples=4 valid=3",
                report.stdout,
            )

            json_report = subprocess.run(
                [sys.executable, str(SCRIPT), "--json", str(path)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(
                json_report.returncode,
                0,
                json_report.stdout + json_report.stderr,
            )
            document = json.loads(json_report.stdout)
            probe = document["runs"][0]["rcs_probe"]
            self.assertEqual(probe["schema"], 1)
            self.assertEqual(probe["phase_us"]["walkers"], 40)
            self.assertEqual(
                probe["buckets"][5]["phase_us"]["queue_to_observe"],
                10,
            )

    def test_rcs_probe_allows_only_per_sample_rounding_error(self) -> None:
        within = parse_log_text(
            canonical_hi_log() + rcs_probe_record(phase_delta=2) + "\n"
        ).turns[0]
        within_result = validate_turn(within)
        self.assertTrue(within_result.passed, within_result.issues)

        outside = parse_log_text(
            canonical_hi_log() + rcs_probe_record(phase_delta=3) + "\n"
        ).turns[0]
        outside_result = validate_turn(outside)
        self.assertFalse(outside_result.passed)
        self.assertTrue(
            any(
                "bucket vocabulary component sum=10, "
                "queue_to_observe=7, rounding tolerance=2" in issue
                for issue in outside_result.issues
            ),
            outside_result.issues,
        )

    def test_rcs_probe_rejects_inconsistent_aggregate_and_buckets(self) -> None:
        probe = rcs_probe_record()
        probe = probe.replace(
            "samples=13 valid=10 invalid=3",
            "samples=13 valid=10 invalid=4",
        )
        probe = probe.replace("schema=1", "schema=2")
        probe = probe.replace(
            "policy=first+power-of-two-per-signature",
            "policy=all",
        )
        probe = probe.replace("clock=rcs-36bit", "clock=host")
        probe = probe.replace(
            "phase_us=queue_to_batch:20",
            "phase_us=queue_to_batch:21",
        )
        probe = probe.replace(
            "buckets=shortconv-in:4:3:",
            "buckets=shortconv-in:4:5:",
        )
        capture = parse_log_text(canonical_hi_log() + probe + "\n").turns[0]
        result = validate_turn(capture)
        joined = "\n".join(result.issues)
        self.assertIn("schema=2, expected 1", joined)
        self.assertIn("policy='all'", joined)
        self.assertIn("clock='host'", joined)
        self.assertIn("invalid=4, expected samples-valid=3", joined)
        self.assertIn("bucket shortconv-in valid=5, exceeds samples=4", joined)
        self.assertIn("bucket valid sum=12, aggregate=10", joined)
        self.assertIn("bucket queue_to_batch sum=20, aggregate=21", joined)

        bad_global_counts = rcs_probe_record().replace(
            "samples=13 valid=10 invalid=3",
            "samples=9 valid=10 invalid=0",
        )
        count_result = validate_turn(
            parse_log_text(
                canonical_hi_log() + bad_global_counts + "\n"
            ).turns[0]
        )
        self.assertTrue(
            any(
                "valid=10, exceeds samples=9" in issue
                for issue in count_result.issues
            ),
            count_result.issues,
        )

    def test_rcs_probe_turn_binding_and_duplicates_are_rejected(self) -> None:
        wrong = parse_log_text(
            canonical_hi_log() + rcs_probe_record(turn=2) + "\n"
        ).turns[0]
        self.assertIsNone(wrong.rcs_probe)
        self.assertTrue(
            any("RCS probe turn does not match" in issue for issue in wrong.parse_issues)
        )

        duplicate = parse_log_text(
            canonical_hi_log()
            + rcs_probe_record()
            + "\n"
            + rcs_probe_record()
            + "\n"
        ).turns[0]
        self.assertIsNotNone(duplicate.rcs_probe)
        self.assertTrue(
            any("duplicate RCS probe record" in issue for issue in duplicate.parse_issues)
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

    def test_cli_can_require_rcs_probe_without_changing_default(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            old = root / "m1.log"
            old.write_text(canonical_hi_log(), encoding="utf-8")
            new = root / "m2.log"
            new.write_text(
                canonical_hi_log() + rcs_probe_record() + "\n",
                encoding="utf-8",
            )

            default = subprocess.run(
                [sys.executable, str(SCRIPT), str(old)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(default.returncode, 0, default.stdout + default.stderr)

            required_old = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--require-rcs-probe",
                    "--expect-runs",
                    "1",
                    str(old),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(required_old.returncode, 1)
            self.assertIn(
                "missing turn-rcs-probe record",
                required_old.stdout,
            )

            required_new = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--require-rcs-probe",
                    "--expect-runs",
                    "1",
                    str(new),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(
                required_new.returncode,
                0,
                required_new.stdout + required_new.stderr,
            )

            help_report = subprocess.run(
                [sys.executable, str(SCRIPT), "--help"],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(help_report.returncode, 0, help_report.stderr)
            self.assertIn("--require-rcs-probe", help_report.stdout)

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

    def test_expect_runs_accepts_fresh_hi_hi_sky_across_logs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = [
                root / "run-1.log",
                root / "run-2.log",
                root / "run-3.log",
            ]
            paths[0].write_text(canonical_hi_log(), encoding="utf-8")
            paths[1].write_text(canonical_hi_log(), encoding="utf-8")
            paths[2].write_text(canonical_sky_log(), encoding="utf-8")
            report = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--expect-runs",
                    "3",
                    *(str(path) for path in paths),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(report.returncode, 0, report.stdout + report.stderr)
            self.assertIn("completed=3 passed=3 failed=0", report.stdout)

    def test_expect_runs_repeats_identity_sequence_for_nine_runs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payloads = (
                canonical_hi_log,
                canonical_hi_log,
                canonical_sky_log,
            ) * 3
            paths = []
            for index, make_payload in enumerate(payloads, start=1):
                path = root / f"run-{index}.log"
                path.write_text(make_payload(), encoding="utf-8")
                paths.append(path)
            report = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--expect-runs",
                    "9",
                    *(str(path) for path in paths),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(report.returncode, 0, report.stdout + report.stderr)
            self.assertIn("completed=9 passed=9 failed=0", report.stdout)

    def test_expect_runs_excludes_warm_conversation_but_generic_accepts(self) -> None:
        text = canonical_hi_log()
        text += valid_warm_turn(
            turn=2,
            context_before=19,
            prompt=12,
            reply=18,
            first_token=4_083,
            digest="c6bb8fa50c9822fd7afb0cd115c4ed03b2fc06aec6804c9296950949da2fe5f6",
        )
        text += valid_warm_turn(
            turn=3,
            context_before=49,
            prompt=22,
            reply=16,
            first_token=1_098,
            digest="dbf137310c7269ed9c41e5353c4088e3ffbbf719c280de953c1df62baa27b6d9",
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "warm.log"
            path.write_text(text, encoding="utf-8")
            generic = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--require-turns",
                    str(path),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(
                generic.returncode,
                0,
                generic.stdout + generic.stderr,
            )

            campaign = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--expect-runs",
                    "3",
                    str(path),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(campaign.returncode, 1)
            self.assertIn(
                "run=2 status=EXCLUDED",
                campaign.stdout,
            )
            self.assertIn(
                "campaign: excluded: start turn=2, expected fresh turn=1",
                campaign.stdout,
            )
            self.assertIn(
                "campaign: excluded: start context_before=19, expected 0",
                campaign.stdout,
            )
            self.assertIn(
                "resident boundary already belongs to an earlier turn",
                campaign.stdout,
            )
            self.assertIn(
                "campaign_selected=1 excluded=2",
                campaign.stdout,
            )
            self.assertIn("campaign runs=1, expected 3", campaign.stderr)

    def test_campaign_selects_archived_and_rolling_fresh_candidates(self) -> None:
        validation1 = canonical_hi_log()
        validation1 += valid_warm_turn(
            turn=2,
            context_before=19,
            prompt=12,
            reply=18,
            first_token=4_083,
            digest="c6bb8fa50c9822fd7afb0cd115c4ed03b2fc06aec6804c9296950949da2fe5f6",
        )
        validation1 += valid_warm_turn(
            turn=3,
            context_before=49,
            prompt=22,
            reply=16,
            first_token=1_098,
            digest="dbf137310c7269ed9c41e5353c4088e3ffbbf719c280de953c1df62baa27b6d9",
        )
        validation3 = canonical_hi_log()
        validation3 += valid_warm_turn(
            turn=2,
            context_before=19,
            prompt=22,
            reply=16,
            first_token=1_098,
            digest="dbf137310c7269ed9c41e5353c4088e3ffbbf719c280de953c1df62baa27b6d9",
        )
        validation3 += canonical_sky_log()

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            validation1_path = root / "validation1.log"
            validation2_path = root / "validation2.log"
            validation3_path = root / "validation3.log"
            validation1_path.write_text(validation1, encoding="utf-8")
            validation2_path.write_text(canonical_hi_log(), encoding="utf-8")
            validation3_path.write_text(validation3, encoding="utf-8")

            first_two = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--expect-runs",
                    "2",
                    str(validation1_path),
                    str(validation2_path),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(
                first_two.returncode,
                0,
                first_two.stdout + first_two.stderr,
            )
            self.assertIn(
                "campaign_selected=2 excluded=2",
                first_two.stdout,
            )

            full_set = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--expect-runs",
                    "3",
                    str(validation1_path),
                    str(validation3_path),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(
                full_set.returncode,
                0,
                full_set.stdout + full_set.stderr,
            )
            self.assertIn(
                "completed=6 passed=3 failed=0 incomplete=0 "
                "campaign_selected=3 excluded=3",
                full_set.stdout,
            )


if __name__ == "__main__":
    unittest.main()
