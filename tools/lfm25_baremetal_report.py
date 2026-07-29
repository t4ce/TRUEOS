#!/usr/bin/env python3
"""Report and validate fixed LFM2.5-350M bare-metal inference turns.

The parser intentionally searches for the stable ``lfm25:`` payload inside
each line, so timestamps, log targets, TCP-drain prefixes, and unrelated
records do not affect it.  Turn records are joined in stream order rather than
by ``turn=`` alone because every fresh Lumen session starts again at turn 1.

Exit status is zero when every completed turn is valid.  A legacy log without
turn telemetry is informational unless ``--require-turns`` or
``--expect-runs`` is supplied.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Mapping, Sequence


OPS_PER_PREFILL_TOKEN = 97
OPS_PER_TOKEN = 99
PROJECTIONS_PER_PREFILL_TOKEN = 92
PROJECTIONS_PER_TOKEN = 93
SUBMISSIONS_PER_PREFILL_TOKEN = 64
SUBMISSIONS_PER_TOKEN = 65

PACKED_MODEL_BYTES = 376_701_952
PACKED_MODEL_SHA256 = (
    "90876f02e0cc224fe23e01c8739dcbb94d7bcc8fbfa3d36204c6267a440f5fd8"
)

SIGNATURE_LOGICAL_BYTES = {
    "shortconv-in": 3_342_336,
    "hidden": 1_114_112,
    "attention-qkv": 2_228_224,
    "ffn-gate-up": 10_027_008,
    "ffn-down": 5_013_504,
    "vocabulary": 71_303_168,
    "unknown": 0,
}

FIELD_RE = re.compile(
    r"(?<!\S)([A-Za-z_][A-Za-z0-9_]*)=(\"(?:[^\"\\]|\\.)*\"|\S+)"
)


@dataclass(frozen=True)
class CanonicalReply:
    name: str
    prompt_tokens: int
    reply_tokens: int
    stop: str
    sha256: str
    first_token: int | None = None


CANONICAL_REPLIES = (
    CanonicalReply(
        name="hi",
        prompt_tokens=10,
        reply_tokens=9,
        stop="eot",
        sha256=(
            "fda564ba3f7a0f028106d468420f674898ed99ac5bf2765ac9586206e39d73c5"
        ),
        first_token=36_309,
    ),
    CanonicalReply(
        name="sky",
        prompt_tokens=21,
        reply_tokens=22,
        stop="eot",
        sha256=(
            "79953eee1910284066aebc0a0147a1359c9b6ca6778ac98fc43f1eec05e5b3ce"
        ),
    ),
)


@dataclass(frozen=True)
class Record:
    source: str
    line_number: int
    fields: Mapping[str, str]


@dataclass(frozen=True)
class PackContext:
    source: str
    line_number: int
    fields: Mapping[str, str]


@dataclass(frozen=True)
class ResidentContext:
    source: str
    line_number: int
    fields: Mapping[str, str]
    pack: PackContext | None


@dataclass
class TurnCapture:
    source: str
    source_ordinal: int
    start: Record | None = None
    prefill: Record | None = None
    done: Record | None = None
    cpu: Record | None = None
    resident: ResidentContext | None = None
    pack: PackContext | None = None
    signatures: dict[str, Record] = field(default_factory=dict)
    parse_issues: list[str] = field(default_factory=list)

    @property
    def has_done(self) -> bool:
        return self.done is not None

    @property
    def declared_turn(self) -> str:
        for record in (self.start, self.prefill, self.done):
            if record is not None and "turn" in record.fields:
                return record.fields["turn"]
        return "?"

    @property
    def line_span(self) -> str:
        lines = [
            record.line_number
            for record in (self.start, self.prefill, self.done)
            if record is not None
        ]
        if not lines:
            return "-"
        if len(lines) == 1:
            return str(lines[0])
        return f"{min(lines)}-{max(lines)}"


@dataclass(frozen=True)
class ParsedLog:
    source: str
    turns: tuple[TurnCapture, ...]
    packs: tuple[PackContext, ...]
    residents: tuple[ResidentContext, ...]


@dataclass(frozen=True)
class ExpectedCounts:
    callbacks: int
    projections: int
    submissions: int


@dataclass(frozen=True)
class TurnMetrics:
    elapsed_ms: int | None
    prefill_ms: int | None
    reply_ms: int | None
    prefill_tokens_per_second: float | None
    reply_tokens_per_second: float | None
    gpu_us: int | None
    completion_us: int | None
    completion_gap_us: int | None
    gpu_us_per_submission: float | None
    completion_us_per_submission: float | None
    completion_gap_us_per_submission: float | None
    projections_per_submission: float | None
    attention_calls: int | None
    attention_positions: int | None
    attention_us: int | None
    projection_calls: int | None
    projection_prepare_us: int | None
    projection_quantize_us: int | None
    projection_batch_us: int | None
    projection_batch_residual_us: int | None


@dataclass(frozen=True)
class TurnResult:
    capture: TurnCapture
    issues: tuple[str, ...]
    canonical: CanonicalReply | None
    expected: ExpectedCounts | None
    metrics: TurnMetrics

    @property
    def passed(self) -> bool:
        return self.capture.has_done and not self.issues


def parse_fields(payload: str) -> dict[str, str]:
    """Extract whitespace-separated key/value fields from a log payload."""

    result: dict[str, str] = {}
    for match in FIELD_RE.finditer(payload):
        value = match.group(2)
        if len(value) >= 2 and value[0] == value[-1] == '"':
            try:
                value = json.loads(value)
            except json.JSONDecodeError:
                pass
        result[match.group(1)] = value
    return result


def parse_int(record: Record | None, key: str) -> int | None:
    if record is None:
        return None
    value = record.fields.get(key)
    if value is None:
        return None
    try:
        return int(value, 0)
    except ValueError:
        return None


def parse_phase_us(record: Record | None) -> dict[str, int] | None:
    if record is None:
        return None
    encoded = record.fields.get("phase_us")
    if encoded is None:
        return None
    phases: dict[str, int] = {}
    for item in encoded.split(","):
        name, separator, value = item.partition(":")
        if not separator:
            return None
        try:
            phases[name] = int(value, 0)
        except ValueError:
            return None
    required = {"encode", "admission", "completion", "gpu"}
    return phases if required <= phases.keys() else None


def parse_log_text(text: str, source: str = "<memory>") -> ParsedLog:
    """Parse all Lumen turn records in *text* in their stream order."""

    turns: list[TurnCapture] = []
    packs: list[PackContext] = []
    residents: list[ResidentContext] = []
    current_pack: PackContext | None = None
    current_resident: ResidentContext | None = None
    active: TurnCapture | None = None
    last_done: TurnCapture | None = None

    def new_turn() -> TurnCapture:
        capture = TurnCapture(
            source=source,
            source_ordinal=len(turns) + 1,
            resident=current_resident,
            pack=current_pack,
        )
        turns.append(capture)
        return capture

    for line_number, line in enumerate(text.splitlines(), start=1):
        if "lfm25:" not in line:
            continue
        payload = line[line.index("lfm25:") :]

        if payload.startswith("lfm25: packed model ready "):
            current_pack = PackContext(
                source=source,
                line_number=line_number,
                fields=parse_fields(payload),
            )
            packs.append(current_pack)
            continue

        if payload.startswith("lfm25: resident "):
            fields = parse_fields(payload)
            if fields.get("stage") == "ready":
                current_resident = ResidentContext(
                    source=source,
                    line_number=line_number,
                    fields=fields,
                    pack=current_pack,
                )
                residents.append(current_resident)
                last_done = None
            continue

        if payload.startswith("lfm25: turn-cpu "):
            fields = parse_fields(payload)
            if last_done is None or fields.get("stage") != "done":
                continue
            declared = fields.get("turn")
            done_turn = (
                last_done.done.fields.get("turn")
                if last_done.done is not None
                else None
            )
            if declared != done_turn:
                last_done.parse_issues.append(
                    "CPU turn does not match preceding done record"
                )
                continue
            if last_done.cpu is not None:
                last_done.parse_issues.append(
                    f"duplicate CPU record at line {line_number}"
                )
                continue
            last_done.cpu = Record(
                source=source,
                line_number=line_number,
                fields=fields,
            )
            continue

        if payload.startswith("lfm25: turn-signature "):
            fields = parse_fields(payload)
            signature = fields.get("signature")
            if (
                last_done is None
                or signature is None
                or fields.get("stage") != "done"
            ):
                continue
            declared = fields.get("turn")
            done_turn = (
                last_done.done.fields.get("turn")
                if last_done.done is not None
                else None
            )
            if declared != done_turn:
                last_done.parse_issues.append(
                    "signature turn does not match preceding done record"
                )
                continue
            if signature in last_done.signatures:
                last_done.parse_issues.append(
                    f"duplicate signature bucket {signature!r}"
                )
                continue
            last_done.signatures[signature] = Record(
                source=source,
                line_number=line_number,
                fields=fields,
            )
            continue

        if not payload.startswith("lfm25: turn "):
            continue
        fields = parse_fields(payload)
        stage = fields.get("stage")
        if stage not in {"start", "prefill", "done"}:
            continue
        record = Record(source=source, line_number=line_number, fields=fields)

        if stage == "start":
            if active is not None:
                active.parse_issues.append(
                    f"superseded by a new start at line {line_number}"
                )
            active = new_turn()
            active.start = record
            last_done = None
            continue

        if active is None:
            active = new_turn()
            active.parse_issues.append(f"{stage} record has no preceding start")
            last_done = None

        if stage == "prefill":
            if active.prefill is not None:
                active.parse_issues.append(
                    f"duplicate prefill record at line {line_number}"
                )
            else:
                active.prefill = record
            continue

        if active.done is not None:
            active.parse_issues.append(f"duplicate done record at line {line_number}")
        else:
            active.done = record
        last_done = active
        active = None

    return ParsedLog(
        source=source,
        turns=tuple(turns),
        packs=tuple(packs),
        residents=tuple(residents),
    )


def parse_log_path(path: Path) -> ParsedLog:
    return parse_log_text(
        path.read_text(encoding="utf-8", errors="replace"),
        source=str(path),
    )


def expected_prefill_counts(prompt_tokens: int) -> ExpectedCounts:
    state_only = max(prompt_tokens - 1, 0)
    full = int(prompt_tokens > 0)
    return ExpectedCounts(
        callbacks=state_only * OPS_PER_PREFILL_TOKEN + full * OPS_PER_TOKEN,
        projections=(
            state_only * PROJECTIONS_PER_PREFILL_TOKEN
            + full * PROJECTIONS_PER_TOKEN
        ),
        submissions=(
            state_only * SUBMISSIONS_PER_PREFILL_TOKEN
            + full * SUBMISSIONS_PER_TOKEN
        ),
    )


def expected_done_counts(
    prompt_tokens: int,
    reply_tokens: int,
    stop: str,
) -> ExpectedCounts:
    """Return fixed-graph counts, including the decode that discovers EOT."""

    state_only = max(prompt_tokens - 1, 0)
    prompt_full = int(prompt_tokens > 0)
    if stop == "eot":
        reply_full = reply_tokens
    elif stop == "limit":
        # The token at the hard limit is emitted but is not fed back.
        reply_full = max(reply_tokens - 1, 0)
    else:
        reply_full = 0
    full = prompt_full + reply_full
    return ExpectedCounts(
        callbacks=state_only * OPS_PER_PREFILL_TOKEN + full * OPS_PER_TOKEN,
        projections=(
            state_only * PROJECTIONS_PER_PREFILL_TOKEN
            + full * PROJECTIONS_PER_TOKEN
        ),
        submissions=(
            state_only * SUBMISSIONS_PER_PREFILL_TOKEN
            + full * SUBMISSIONS_PER_TOKEN
        ),
    )


def canonical_reply(done: Record | None) -> CanonicalReply | None:
    if done is None or parse_int(done, "context_before") not in (None, 0):
        return None
    prompt_tokens = parse_int(done, "prompt_tokens")
    reply_tokens = parse_int(done, "reply_tokens")
    stop = done.fields.get("stop")
    for candidate in CANONICAL_REPLIES:
        if (
            prompt_tokens == candidate.prompt_tokens
            and reply_tokens == candidate.reply_tokens
            and stop == candidate.stop
        ):
            return candidate
    return None


def require_int(
    record: Record,
    key: str,
    stage: str,
    issues: list[str],
) -> int | None:
    value = parse_int(record, key)
    if value is None:
        issues.append(f"{stage} has missing or invalid {key}")
    return value


def validate_stage_counts(
    record: Record,
    stage: str,
    expected: ExpectedCounts,
    issues: list[str],
) -> None:
    for field_name, expected_value in (
        ("callbacks", expected.callbacks),
        ("igpu_projections", expected.projections),
        ("igpu_submissions", expected.submissions),
    ):
        observed = require_int(record, field_name, stage, issues)
        if observed is not None and observed != expected_value:
            issues.append(
                f"{stage} {field_name}={observed}, expected {expected_value}"
            )

    failures = require_int(record, "igpu_failures", stage, issues)
    if failures is not None and failures != 0:
        issues.append(f"{stage} igpu_failures={failures}, expected 0")

    submissions = parse_int(record, "igpu_submissions")
    gpu_samples = require_int(record, "gpu_samples", stage, issues)
    if (
        submissions is not None
        and gpu_samples is not None
        and gpu_samples != submissions
    ):
        issues.append(
            f"{stage} gpu_samples={gpu_samples}, expected {submissions} submissions"
        )

    phases = parse_phase_us(record)
    if phases is None:
        issues.append(f"{stage} has missing or invalid phase_us")
    gpu_hz = require_int(record, "gpu_hz", stage, issues)
    if submissions and gpu_hz is not None and gpu_hz <= 0:
        issues.append(f"{stage} gpu_hz={gpu_hz}, expected a positive clock")


def validate_signatures(capture: TurnCapture, issues: list[str]) -> None:
    if not capture.signatures or capture.done is None:
        return

    labels = set(capture.signatures)
    expected_labels = set(SIGNATURE_LOGICAL_BYTES)
    missing = sorted(expected_labels - labels)
    extra = sorted(labels - expected_labels)
    if missing:
        issues.append("signature buckets missing: " + ",".join(missing))
    if extra:
        issues.append("unexpected signature buckets: " + ",".join(extra))

    totals = {
        "submissions": 0,
        "projections": 0,
        "submit_ms": 0,
        "completion_us": 0,
        "gpu_us": 0,
        "gpu_samples": 0,
    }
    for label, record in capture.signatures.items():
        for key in totals:
            value = require_int(record, key, f"signature {label}", issues)
            if value is not None:
                totals[key] += value
        submissions = parse_int(record, "submissions")
        gpu_samples = parse_int(record, "gpu_samples")
        if (
            submissions is not None
            and gpu_samples is not None
            and gpu_samples != submissions
        ):
            issues.append(
                f"signature {label} gpu_samples={gpu_samples}, "
                f"expected {submissions}"
            )

    unknown = capture.signatures.get("unknown")
    if unknown is not None:
        unknown_submissions = parse_int(unknown, "submissions")
        unknown_projections = parse_int(unknown, "projections")
        if (unknown_submissions or 0) > 0 or (unknown_projections or 0) > 0:
            issues.append(
                "unknown signature bucket is nonzero "
                f"(submissions={unknown_submissions}, "
                f"projections={unknown_projections})"
            )

    done_phases = parse_phase_us(capture.done)
    done_expected: dict[str, int | None] = {
        "submissions": parse_int(capture.done, "igpu_submissions"),
        "projections": parse_int(capture.done, "igpu_projections"),
        "submit_ms": parse_int(capture.done, "igpu_submit_ms"),
        "completion_us": (
            done_phases.get("completion") if done_phases is not None else None
        ),
        "gpu_us": done_phases.get("gpu") if done_phases is not None else None,
        "gpu_samples": parse_int(capture.done, "gpu_samples"),
    }
    for key, expected in done_expected.items():
        if expected is not None and totals[key] != expected:
            issues.append(
                f"signature {key} sum={totals[key]}, done total={expected}"
            )


def calculate_metrics(capture: TurnCapture) -> TurnMetrics:
    done = capture.done
    prefill = capture.prefill
    elapsed_ms = parse_int(done, "elapsed_ms")
    prefill_ms = parse_int(prefill, "elapsed_ms")
    reply_ms = (
        max(elapsed_ms - prefill_ms, 0)
        if elapsed_ms is not None and prefill_ms is not None
        else None
    )
    prompt_tokens = parse_int(done, "prompt_tokens")
    reply_tokens = parse_int(done, "reply_tokens")
    submissions = parse_int(done, "igpu_submissions")
    projections = parse_int(done, "igpu_projections")
    submit_ms = parse_int(done, "igpu_submit_ms")
    phases = parse_phase_us(done)
    completion_us = phases.get("completion") if phases is not None else None
    gpu_us = phases.get("gpu") if phases is not None else None
    gap_us = (
        completion_us - gpu_us
        if completion_us is not None and gpu_us is not None
        else None
    )
    projection_batch_us = parse_int(capture.cpu, "projection_batch_us")
    projection_batch_residual_us = (
        projection_batch_us - submit_ms * 1_000
        if projection_batch_us is not None and submit_ms is not None
        else None
    )

    def rate(count: int | None, duration_ms: int | None) -> float | None:
        if count is None or duration_ms is None or duration_ms <= 0:
            return None
        return count * 1_000.0 / duration_ms

    def per_submission(value: int | None) -> float | None:
        if value is None or submissions is None or submissions <= 0:
            return None
        return value / submissions

    return TurnMetrics(
        elapsed_ms=elapsed_ms,
        prefill_ms=prefill_ms,
        reply_ms=reply_ms,
        prefill_tokens_per_second=rate(prompt_tokens, prefill_ms),
        reply_tokens_per_second=rate(reply_tokens, reply_ms),
        gpu_us=gpu_us,
        completion_us=completion_us,
        completion_gap_us=gap_us,
        gpu_us_per_submission=per_submission(gpu_us),
        completion_us_per_submission=per_submission(completion_us),
        completion_gap_us_per_submission=per_submission(gap_us),
        projections_per_submission=(
            projections / submissions
            if projections is not None and submissions is not None and submissions > 0
            else None
        ),
        attention_calls=parse_int(capture.cpu, "attention_calls"),
        attention_positions=parse_int(capture.cpu, "attention_positions"),
        attention_us=parse_int(capture.cpu, "attention_us"),
        projection_calls=parse_int(capture.cpu, "projection_calls"),
        projection_prepare_us=parse_int(capture.cpu, "projection_prepare_us"),
        projection_quantize_us=parse_int(capture.cpu, "projection_quantize_us"),
        projection_batch_us=projection_batch_us,
        projection_batch_residual_us=projection_batch_residual_us,
    )


def validate_turn(
    capture: TurnCapture,
    *,
    require_detail: bool = False,
) -> TurnResult:
    issues = list(capture.parse_issues)
    if capture.done is None:
        return TurnResult(
            capture=capture,
            issues=tuple(issues),
            canonical=None,
            expected=None,
            metrics=calculate_metrics(capture),
        )

    if capture.start is None:
        issues.append("missing start record")
    if capture.prefill is None:
        issues.append("missing prefill record")
    if require_detail and not capture.signatures:
        issues.append("missing turn-signature detail records")
    if require_detail and capture.cpu is None:
        issues.append("missing turn-cpu detail record")

    records = [
        (stage, record)
        for stage, record in (
            ("start", capture.start),
            ("prefill", capture.prefill),
            ("done", capture.done),
        )
        if record is not None
    ]
    declared_turns = {record.fields.get("turn") for _, record in records}
    if None in declared_turns:
        issues.append("one or more stages have no turn field")
    if len(declared_turns) > 1:
        issues.append(
            "stage turn identifiers differ: "
            + ",".join(sorted(str(value) for value in declared_turns))
        )

    done_prompt = require_int(capture.done, "prompt_tokens", "done", issues)
    done_reply = require_int(capture.done, "reply_tokens", "done", issues)
    stop = capture.done.fields.get("stop")
    if stop not in {"eot", "limit"}:
        issues.append(f"done stop={stop!r}, expected 'eot' or 'limit'")

    for stage, record in records:
        stage_prompt = require_int(record, "prompt_tokens", stage, issues)
        if (
            done_prompt is not None
            and stage_prompt is not None
            and stage_prompt != done_prompt
        ):
            issues.append(
                f"{stage} prompt_tokens={stage_prompt}, done has {done_prompt}"
            )

    if capture.start is not None:
        start_expected = ExpectedCounts(0, 0, 0)
        validate_stage_counts(capture.start, "start", start_expected, issues)
        start_elapsed = require_int(capture.start, "elapsed_ms", "start", issues)
        if start_elapsed is not None and start_elapsed < 0:
            issues.append("start elapsed_ms is negative")

    prefill_expected: ExpectedCounts | None = None
    if capture.prefill is not None and done_prompt is not None:
        prefill_expected = expected_prefill_counts(done_prompt)
        validate_stage_counts(capture.prefill, "prefill", prefill_expected, issues)
        if capture.prefill.fields.get("stop") != "pending":
            issues.append(
                f"prefill stop={capture.prefill.fields.get('stop')!r}, "
                "expected 'pending'"
            )

    expected: ExpectedCounts | None = None
    if done_prompt is not None and done_reply is not None and stop in {"eot", "limit"}:
        expected = expected_done_counts(done_prompt, done_reply, stop)
        validate_stage_counts(capture.done, "done", expected, issues)

    elapsed_values = [
        (stage, parse_int(record, "elapsed_ms")) for stage, record in records
    ]
    previous: int | None = None
    for stage, elapsed in elapsed_values:
        if elapsed is None:
            issues.append(f"{stage} has missing or invalid elapsed_ms")
            continue
        if previous is not None and elapsed < previous:
            issues.append(f"{stage} elapsed_ms={elapsed} is not monotonic")
        previous = elapsed

    candidate = canonical_reply(capture.done)
    if candidate is not None:
        digest = capture.done.fields.get("raw_reply_sha256")
        if digest is not None:
            if digest == "-":
                issues.append(f"canonical {candidate.name} reply hash is missing")
            elif digest.lower() != candidate.sha256:
                issues.append(
                    f"canonical {candidate.name} raw_reply_sha256={digest}, "
                    f"expected {candidate.sha256}"
                )
        if candidate.first_token is not None:
            first_token = require_int(capture.done, "first_token", "done", issues)
            if first_token is not None and first_token != candidate.first_token:
                issues.append(
                    f"canonical {candidate.name} first_token={first_token}, "
                    f"expected {candidate.first_token}"
                )

    validate_signatures(capture, issues)

    if capture.cpu is not None:
        cpu_values = {
            key: require_int(capture.cpu, key, "cpu", issues)
            for key in (
                "attention_calls",
                "attention_positions",
                "attention_us",
                "projection_calls",
                "projection_prepare_us",
                "projection_quantize_us",
                "projection_batch_us",
            )
        }
        projection_calls = cpu_values["projection_calls"]
        submissions = parse_int(capture.done, "igpu_submissions")
        if (
            projection_calls is not None
            and submissions is not None
            and projection_calls != submissions
        ):
            issues.append(
                f"cpu projection_calls={projection_calls}, "
                f"expected {submissions} igpu_submissions"
            )

    if capture.pack is not None:
        pack_bytes = capture.pack.fields.get("bytes")
        pack_hash = capture.pack.fields.get("sha256")
        if pack_bytes is not None:
            try:
                observed_bytes = int(pack_bytes, 0)
            except ValueError:
                issues.append("packed-model bytes is invalid")
            else:
                if observed_bytes != PACKED_MODEL_BYTES:
                    issues.append(
                        f"packed-model bytes={observed_bytes}, "
                        f"expected {PACKED_MODEL_BYTES}"
                    )
        if pack_hash is not None and pack_hash.lower() != PACKED_MODEL_SHA256:
            issues.append(
                f"packed-model sha256={pack_hash}, expected {PACKED_MODEL_SHA256}"
            )

    return TurnResult(
        capture=capture,
        issues=tuple(dict.fromkeys(issues)),
        canonical=candidate,
        expected=expected,
        metrics=calculate_metrics(capture),
    )


def fmt_int(value: int | None) -> str:
    return "-" if value is None else str(value)


def fmt_float(value: float | None, digits: int = 2) -> str:
    return "-" if value is None else f"{value:.{digits}f}"


def signature_gbps(record: Record) -> float | None:
    label = record.fields.get("signature")
    submissions = parse_int(record, "submissions")
    gpu_us = parse_int(record, "gpu_us")
    logical_bytes = SIGNATURE_LOGICAL_BYTES.get(label or "")
    if (
        logical_bytes is None
        or logical_bytes == 0
        or submissions is None
        or submissions <= 0
        or gpu_us is None
        or gpu_us <= 0
    ):
        return None
    return logical_bytes * submissions / (gpu_us * 1_000.0)


def context_dict(capture: TurnCapture) -> dict[str, object] | None:
    if capture.resident is None and capture.pack is None:
        return None
    result: dict[str, object] = {}
    if capture.pack is not None:
        result["pack"] = {
            "line": capture.pack.line_number,
            **dict(capture.pack.fields),
        }
    if capture.resident is not None:
        result["resident"] = {
            "line": capture.resident.line_number,
            **dict(capture.resident.fields),
        }
    return result


def result_dict(result: TurnResult, run_number: int) -> dict[str, object]:
    capture = result.capture
    done = capture.done
    expected = result.expected
    metrics = result.metrics
    signatures = []
    for label in SIGNATURE_LOGICAL_BYTES:
        record = capture.signatures.get(label)
        if record is None:
            continue
        signatures.append(
            {
                "signature": label,
                "line": record.line_number,
                **dict(record.fields),
                "logical_bytes_per_submission": SIGNATURE_LOGICAL_BYTES[label],
                "effective_logical_gbps": signature_gbps(record),
            }
        )
    return {
        "run": run_number,
        "source": capture.source,
        "source_ordinal": capture.source_ordinal,
        "turn": capture.declared_turn,
        "line_span": capture.line_span,
        "status": (
            "pass"
            if result.passed
            else "fail"
            if capture.has_done
            else "incomplete"
        ),
        "canonical": result.canonical.name if result.canonical else None,
        "issues": list(result.issues),
        "context": context_dict(capture),
        "observed": dict(done.fields) if done is not None else None,
        "cpu": (
            {"line": capture.cpu.line_number, **dict(capture.cpu.fields)}
            if capture.cpu is not None
            else None
        ),
        "expected": (
            {
                "callbacks": expected.callbacks,
                "igpu_projections": expected.projections,
                "igpu_submissions": expected.submissions,
            }
            if expected is not None
            else None
        ),
        "metrics": {
            field_name: getattr(metrics, field_name)
            for field_name in TurnMetrics.__dataclass_fields__
        },
        "signatures": signatures,
    }


def print_text_report(
    parsed_logs: Sequence[ParsedLog],
    results: Sequence[TurnResult],
) -> None:
    print("LFM2.5 bare-metal inference report")
    for parsed in parsed_logs:
        print(
            f"source={parsed.source} turns={len(parsed.turns)} "
            f"residents={len(parsed.residents)} packs={len(parsed.packs)}"
        )
        if not parsed.turns:
            print("  no turn telemetry (legacy/pre-schema log is informational)")

    for run_number, result in enumerate(results, start=1):
        capture = result.capture
        status = (
            "PASS"
            if result.passed
            else "FAIL"
            if capture.has_done
            else "INCOMPLETE"
        )
        canonical = result.canonical.name if result.canonical else "-"
        print(
            f"\nrun={run_number} status={status} source={capture.source} "
            f"source_run={capture.source_ordinal} turn={capture.declared_turn} "
            f"lines={capture.line_span} canonical={canonical}"
        )

        pack = capture.pack
        resident = capture.resident
        print(
            "  context "
            f"pack_seal_ms={pack.fields.get('pack_seal_ms', '-') if pack else '-'} "
            f"resident_open_ms={resident.fields.get('open_ms', '-') if resident else '-'} "
            f"tokenizer_open_ms="
            f"{resident.fields.get('tokenizer_open_ms', '-') if resident else '-'} "
            f"model_open_ms="
            f"{resident.fields.get('model_open_ms', '-') if resident else '-'}"
        )

        done = capture.done
        if done is not None:
            expected = result.expected
            print(
                "  tokens "
                f"prompt={done.fields.get('prompt_tokens', '-')} "
                f"reply={done.fields.get('reply_tokens', '-')} "
                f"stop={done.fields.get('stop', '-')} "
                f"first={done.fields.get('first_token', '-')} "
                f"hash={done.fields.get('raw_reply_sha256', '-')}"
            )
            print(
                "  work "
                f"callbacks={done.fields.get('callbacks', '-')}/"
                f"{fmt_int(expected.callbacks if expected else None)} "
                f"projections={done.fields.get('igpu_projections', '-')}/"
                f"{fmt_int(expected.projections if expected else None)} "
                f"submissions={done.fields.get('igpu_submissions', '-')}/"
                f"{fmt_int(expected.submissions if expected else None)} "
                f"failures={done.fields.get('igpu_failures', '-')} "
                f"gpu_samples={done.fields.get('gpu_samples', '-')}"
            )

        metrics = result.metrics
        print(
            "  latency "
            f"elapsed_ms={fmt_int(metrics.elapsed_ms)} "
            f"prefill_ms={fmt_int(metrics.prefill_ms)} "
            f"reply_ms={fmt_int(metrics.reply_ms)} "
            f"prefill_tok_s={fmt_float(metrics.prefill_tokens_per_second)} "
            f"reply_tok_s={fmt_float(metrics.reply_tokens_per_second)}"
        )
        print(
            "  gpu "
            f"gpu_us={fmt_int(metrics.gpu_us)} "
            f"completion_us={fmt_int(metrics.completion_us)} "
            f"gap_us={fmt_int(metrics.completion_gap_us)} "
            f"gpu_us_per_submit={fmt_float(metrics.gpu_us_per_submission)} "
            f"completion_us_per_submit="
            f"{fmt_float(metrics.completion_us_per_submission)} "
            f"gap_us_per_submit="
            f"{fmt_float(metrics.completion_gap_us_per_submission)} "
            f"projections_per_submit="
            f"{fmt_float(metrics.projections_per_submission, 3)}"
        )
        if capture.cpu is not None:
            print(
                "  cpu "
                f"attention_calls={fmt_int(metrics.attention_calls)} "
                f"attention_positions={fmt_int(metrics.attention_positions)} "
                f"attention_us={fmt_int(metrics.attention_us)} "
                f"projection_calls={fmt_int(metrics.projection_calls)} "
                f"prepare_us={fmt_int(metrics.projection_prepare_us)} "
                f"quantize_us={fmt_int(metrics.projection_quantize_us)} "
                f"batch_us={fmt_int(metrics.projection_batch_us)} "
                f"batch_minus_submit_us="
                f"{fmt_int(metrics.projection_batch_residual_us)} "
                "(igpu_submit_ms has 1 ms per-call rounding)"
            )
        else:
            print("  cpu not captured")

        if capture.signatures:
            print("  signatures")
            for label in SIGNATURE_LOGICAL_BYTES:
                record = capture.signatures.get(label)
                if record is None:
                    continue
                print(
                    f"    {label:<13} "
                    f"submissions={record.fields.get('submissions', '-')} "
                    f"projections={record.fields.get('projections', '-')} "
                    f"gpu_us={record.fields.get('gpu_us', '-')} "
                    f"completion_us={record.fields.get('completion_us', '-')} "
                    f"logical_gbps={fmt_float(signature_gbps(record), 3)}"
                )
        else:
            print("  signatures not captured")

        for issue in result.issues:
            print(f"  issue: {issue}")

    completed = sum(result.capture.has_done for result in results)
    passed = sum(result.passed for result in results)
    failed = sum(
        result.capture.has_done and not result.passed for result in results
    )
    incomplete = len(results) - completed
    print(
        f"\nsummary runs={len(results)} completed={completed} passed={passed} "
        f"failed={failed} incomplete={incomplete}"
    )


def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Report and validate sequential LFM2.5 bare-metal turn telemetry."
        )
    )
    parser.add_argument("logs", nargs="+", type=Path, help="one or more log paths")
    parser.add_argument(
        "--require-turns",
        action="store_true",
        help="fail unless at least one completed turn is present",
    )
    parser.add_argument(
        "--expect-runs",
        type=int,
        metavar="N",
        help="fail unless exactly N completed turns are present (use 9 for the campaign)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="write a machine-readable report instead of the text report",
    )
    return parser


def run(argv: Sequence[str] | None = None) -> int:
    args = build_argument_parser().parse_args(argv)
    if args.expect_runs is not None and args.expect_runs < 0:
        print("--expect-runs must be non-negative", file=sys.stderr)
        return 2

    parsed_logs: list[ParsedLog] = []
    try:
        for path in args.logs:
            parsed_logs.append(parse_log_path(path))
    except OSError as error:
        print(f"lfm25-baremetal-report: {error}", file=sys.stderr)
        return 2

    captures = [
        capture for parsed in parsed_logs for capture in parsed.turns
    ]
    results = [
        validate_turn(capture, require_detail=args.expect_runs is not None)
        for capture in captures
    ]
    completed = sum(capture.has_done for capture in captures)
    failed_completed = any(
        result.capture.has_done and not result.passed for result in results
    )
    requirement_issues: list[str] = []
    if args.require_turns and completed == 0:
        requirement_issues.append("no completed turn telemetry found")
    if args.expect_runs is not None and completed != args.expect_runs:
        requirement_issues.append(
            f"completed runs={completed}, expected {args.expect_runs}"
        )

    if args.json:
        document = {
            "schema": "trueos-lfm25-baremetal-report-v1",
            "notes": {
                "projection_batch_residual_us": (
                    "projection_batch_us - igpu_submit_ms * 1000; "
                    "igpu_submit_ms is rounded independently to 1 ms per call"
                ),
                "effective_logical_gbps": (
                    "fixed logical weight bytes per signature divided by "
                    "that signature's GPU timestamp total"
                ),
            },
            "sources": [
                {
                    "path": parsed.source,
                    "turns": len(parsed.turns),
                    "residents": len(parsed.residents),
                    "packs": len(parsed.packs),
                }
                for parsed in parsed_logs
            ],
            "runs": [
                result_dict(result, run_number)
                for run_number, result in enumerate(results, start=1)
            ],
            "summary": {
                "runs": len(results),
                "completed": completed,
                "passed": sum(result.passed for result in results),
                "failed": sum(
                    result.capture.has_done and not result.passed
                    for result in results
                ),
                "incomplete": len(results) - completed,
                "requirement_issues": requirement_issues,
            },
        }
        print(json.dumps(document, indent=2, sort_keys=True))
    else:
        print_text_report(parsed_logs, results)
        for issue in requirement_issues:
            print(f"requirement failure: {issue}", file=sys.stderr)

    return 1 if failed_completed or requirement_issues else 0


def main() -> int:
    return run()


if __name__ == "__main__":
    raise SystemExit(main())
