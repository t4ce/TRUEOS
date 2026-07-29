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

RCS_PROBE_PHASES = (
    "queue_to_batch",
    "preamble",
    "walkers",
    "epilogue",
    "release_to_observe",
    "queue_to_observe",
)
RCS_PROBE_COMPONENT_PHASES = RCS_PROBE_PHASES[:-1]
RCS_PROBE_BUCKET_SCHEMA = (
    "signature:samples:valid:queue_to_batch_us:preamble_us:walkers_us:"
    "epilogue_us:release_to_observe_us:queue_to_observe_us"
)
GT_STATE_BUCKET_SCHEMA = (
    "signature:samples:start_active:end_active:start_ratio_sum:end_ratio_sum"
)
GT_STATE_SAMPLING = "first+power-of-two-per-signature"
GT_STATE_OBSERVATION = "cpu-mmio-pre-submit+post-observe"
GT_STATE_REGISTER = "gen12-rpstat1"
GEN12_CAGF_SHIFT = 11
GEN12_CAGF_MASK = 0x1FF
GEN9_SW_REQ_UNSLICE_RATIO_SHIFT = 23
GEN12_GT0_PERF_LIMIT_REASONS_MASK = 0x0DE3
M3_ADMISSION_BDF = "00:02.0"
M3_ADMISSION_VENDOR = 0x8086
M3_ADMISSION_DEVICE = 0x4680
M3_ADMISSION_REVISION = 0x0C
M3_ADMISSION_GLOBAL = 0x00000005
M3_ADMISSION_L3CC_PAIR = 0x00100030
M3_ADMISSION_CHECKPOINTS = (
    "boot-init+post-guc+turn-start+first-lfm-retire+turn-end"
)
M3_ADMISSION_EXPECTED_TARGET = "8086:4680:0C"
M3_ADMISSION_BOOLEAN_FIELDS = (
    "available",
    "boot_seen",
    "boot_forcewake",
    "boot_pat",
    "boot_mocs",
    "post_guc_seen",
    "post_guc_cache",
    "first_retire_seen",
    "first_retire_cache",
    "start_pat_available",
    "start_pat",
    "start_mocs_available",
    "start_mocs",
    "start_cache",
    "end_pat_available",
    "end_pat",
    "end_mocs_available",
    "end_mocs",
    "end_cache",
    "guc_boot",
    "guc_firmware",
    "guc_submission",
)
M3_ADMISSION_INTEGER_FIELDS = (
    "schema",
    "vendor",
    "device",
    "revision",
    *M3_ADMISSION_BOOLEAN_FIELDS,
    "boot_before_global",
    "boot_before_l3cc_pair",
    "boot_after_global",
    "boot_after_l3cc_pair",
    "end_global",
    "end_l3cc_pair",
    "expected_global",
    "expected_l3cc_pair",
)

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
        first_token=1_098,
    ),
)

CAMPAIGN_IDENTITY_SEQUENCE = ("hi", "hi", "sky")
CANONICAL_REPLY_BY_NAME = {
    reply.name: reply for reply in CANONICAL_REPLIES
}


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
    rcs_probe: Record | None = None
    gt_state: Record | None = None
    admission: Record | None = None
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
class RcsProbeBucket:
    signature: str
    samples: int
    valid: int
    phase_us: Mapping[str, int]


@dataclass(frozen=True)
class GtStateBucket:
    signature: str
    samples: int
    start_active: int
    end_active: int
    start_ratio_sum: int
    end_ratio_sum: int


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
    campaign_disposition: str | None = None
    campaign_ordinal: int | None = None
    campaign_notes: tuple[str, ...] = ()

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


def parse_rcs_probe_phase_us(record: Record | None) -> dict[str, int] | None:
    if record is None:
        return None
    encoded = record.fields.get("phase_us")
    if encoded is None:
        return None
    phases: dict[str, int] = {}
    for item in encoded.split(","):
        name, separator, value = item.partition(":")
        if not separator or name in phases:
            return None
        try:
            phases[name] = int(value, 0)
        except ValueError:
            return None
    return phases if set(phases) == set(RCS_PROBE_PHASES) else None


def parse_rcs_probe_buckets(
    record: Record | None,
) -> dict[str, RcsProbeBucket] | None:
    if record is None:
        return None
    encoded = record.fields.get("buckets")
    if encoded is None:
        return None
    buckets: dict[str, RcsProbeBucket] = {}
    if not encoded:
        return buckets
    for item in encoded.split("|"):
        parts = item.split(":")
        if len(parts) != 9:
            return None
        signature = parts[0]
        if not signature or signature in buckets:
            return None
        try:
            values = [int(value, 0) for value in parts[1:]]
        except ValueError:
            return None
        buckets[signature] = RcsProbeBucket(
            signature=signature,
            samples=values[0],
            valid=values[1],
            phase_us=dict(zip(RCS_PROBE_PHASES, values[2:])),
        )
    return buckets


def parse_gt_state_active_avg_mhz(
    record: Record | None,
) -> dict[str, int] | None:
    if record is None:
        return None
    encoded = record.fields.get("active_avg_mhz")
    if encoded is None:
        return None
    averages: dict[str, int] = {}
    for item in encoded.split(","):
        name, separator, value = item.partition(":")
        if not separator or name in averages:
            return None
        try:
            averages[name] = int(value, 0)
        except ValueError:
            return None
    return averages if set(averages) == {"start", "end"} else None


def parse_gt_state_buckets(
    record: Record | None,
) -> dict[str, GtStateBucket] | None:
    if record is None:
        return None
    encoded = record.fields.get("buckets")
    if encoded is None:
        return None
    buckets: dict[str, GtStateBucket] = {}
    if not encoded:
        return buckets
    for item in encoded.split("|"):
        parts = item.split(":")
        if len(parts) != 6:
            return None
        signature = parts[0]
        if not signature or signature in buckets:
            return None
        try:
            values = [int(value, 0) for value in parts[1:]]
        except ValueError:
            return None
        buckets[signature] = GtStateBucket(
            signature=signature,
            samples=values[0],
            start_active=values[1],
            end_active=values[2],
            start_ratio_sum=values[3],
            end_ratio_sum=values[4],
        )
    return buckets


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

        if payload.startswith("lfm25: turn-rcs-probe "):
            fields = parse_fields(payload)
            if last_done is None:
                continue
            if fields.get("stage") != "done":
                last_done.parse_issues.append(
                    f"RCS probe stage is not done at line {line_number}"
                )
                continue
            declared = fields.get("turn")
            done_turn = (
                last_done.done.fields.get("turn")
                if last_done.done is not None
                else None
            )
            if declared != done_turn:
                last_done.parse_issues.append(
                    "RCS probe turn does not match preceding done record"
                )
                continue
            if last_done.rcs_probe is not None:
                last_done.parse_issues.append(
                    f"duplicate RCS probe record at line {line_number}"
                )
                continue
            last_done.rcs_probe = Record(
                source=source,
                line_number=line_number,
                fields=fields,
            )
            continue

        if payload.startswith("lfm25: turn-gt-state "):
            fields = parse_fields(payload)
            if last_done is None:
                continue
            if fields.get("stage") != "done":
                last_done.parse_issues.append(
                    f"GT state stage is not done at line {line_number}"
                )
                continue
            declared = fields.get("turn")
            done_turn = (
                last_done.done.fields.get("turn")
                if last_done.done is not None
                else None
            )
            if declared != done_turn:
                last_done.parse_issues.append(
                    "GT state turn does not match preceding done record"
                )
                continue
            if last_done.gt_state is not None:
                last_done.parse_issues.append(
                    f"duplicate GT state record at line {line_number}"
                )
                continue
            last_done.gt_state = Record(
                source=source,
                line_number=line_number,
                fields=fields,
            )
            continue

        if payload.startswith("lfm25: turn-admission "):
            fields = parse_fields(payload)
            if last_done is None:
                continue
            if fields.get("stage") != "done":
                last_done.parse_issues.append(
                    f"admission stage is not done at line {line_number}"
                )
                continue
            declared = fields.get("turn")
            done_turn = (
                last_done.done.fields.get("turn")
                if last_done.done is not None
                else None
            )
            if declared != done_turn:
                last_done.parse_issues.append(
                    "admission turn does not match preceding done record"
                )
                continue
            if last_done.admission is not None:
                last_done.parse_issues.append(
                    f"duplicate admission record at line {line_number}"
                )
                continue
            last_done.admission = Record(
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


def validate_rcs_probe(capture: TurnCapture, issues: list[str]) -> None:
    record = capture.rcs_probe
    if record is None:
        return

    schema = require_int(record, "schema", "RCS probe", issues)
    if schema is not None and schema != 1:
        issues.append(f"RCS probe schema={schema}, expected 1")
    if record.fields.get("scope") != "turn":
        issues.append(
            f"RCS probe scope={record.fields.get('scope')!r}, expected 'turn'"
        )
    if record.fields.get("bucket_schema") != RCS_PROBE_BUCKET_SCHEMA:
        issues.append("RCS probe has missing or unexpected bucket_schema")
    if record.fields.get("policy") != "first+power-of-two-per-signature":
        issues.append(
            f"RCS probe policy={record.fields.get('policy')!r}, expected "
            "'first+power-of-two-per-signature'"
        )
    if record.fields.get("clock") != "rcs-36bit":
        issues.append(
            f"RCS probe clock={record.fields.get('clock')!r}, "
            "expected 'rcs-36bit'"
        )

    aggregate: dict[str, int | None] = {
        key: require_int(record, key, "RCS probe", issues)
        for key in ("samples", "valid", "invalid", "gpu_hz")
    }
    for key, value in aggregate.items():
        if value is not None and value < 0:
            issues.append(f"RCS probe {key}={value}, expected non-negative")

    samples = aggregate["samples"]
    valid = aggregate["valid"]
    invalid = aggregate["invalid"]
    if samples is not None and valid is not None:
        if valid > samples:
            issues.append(
                f"RCS probe valid={valid}, exceeds samples={samples}"
            )
        if (
            invalid is not None
            and valid <= samples
            and invalid != samples - valid
        ):
            issues.append(
                f"RCS probe invalid={invalid}, expected samples-valid="
                f"{samples - valid}"
            )
    gpu_hz = aggregate["gpu_hz"]
    if samples and gpu_hz is not None and gpu_hz <= 0:
        issues.append(
            f"RCS probe gpu_hz={gpu_hz}, expected a positive clock"
        )

    phases = parse_rcs_probe_phase_us(record)
    if phases is None:
        issues.append("RCS probe has missing or invalid phase_us")
    else:
        for name, value in phases.items():
            if value < 0:
                issues.append(
                    f"RCS probe phase {name}={value}, expected non-negative"
                )
        if valid is not None and valid >= 0:
            component_sum = sum(
                phases[name] for name in RCS_PROBE_COMPONENT_PHASES
            )
            observed = phases["queue_to_observe"]
            tolerance = 2 * valid
            if abs(component_sum - observed) > tolerance:
                issues.append(
                    "RCS probe aggregate component sum="
                    f"{component_sum}, queue_to_observe={observed}, "
                    f"rounding tolerance={tolerance}"
                )

    buckets = parse_rcs_probe_buckets(record)
    if buckets is None:
        issues.append("RCS probe has missing or invalid buckets")
        return

    labels = set(buckets)
    expected_labels = set(SIGNATURE_LOGICAL_BYTES)
    missing = sorted(expected_labels - labels)
    extra = sorted(labels - expected_labels)
    if missing:
        issues.append("RCS probe buckets missing: " + ",".join(missing))
    if extra:
        issues.append("unexpected RCS probe buckets: " + ",".join(extra))

    bucket_samples = 0
    bucket_valid = 0
    bucket_phases = {name: 0 for name in RCS_PROBE_PHASES}
    for label, bucket in buckets.items():
        if bucket.samples < 0:
            issues.append(
                f"RCS probe bucket {label} samples={bucket.samples}, "
                "expected non-negative"
            )
        if bucket.valid < 0:
            issues.append(
                f"RCS probe bucket {label} valid={bucket.valid}, "
                "expected non-negative"
            )
        if bucket.valid > bucket.samples:
            issues.append(
                f"RCS probe bucket {label} valid={bucket.valid}, "
                f"exceeds samples={bucket.samples}"
            )
        for name, value in bucket.phase_us.items():
            if value < 0:
                issues.append(
                    f"RCS probe bucket {label} phase {name}={value}, "
                    "expected non-negative"
                )
            bucket_phases[name] += value
        if bucket.valid >= 0:
            component_sum = sum(
                bucket.phase_us[name] for name in RCS_PROBE_COMPONENT_PHASES
            )
            observed = bucket.phase_us["queue_to_observe"]
            tolerance = 2 * bucket.valid
            if abs(component_sum - observed) > tolerance:
                issues.append(
                    f"RCS probe bucket {label} component sum={component_sum}, "
                    f"queue_to_observe={observed}, "
                    f"rounding tolerance={tolerance}"
                )
        bucket_samples += bucket.samples
        bucket_valid += bucket.valid

    for name, observed in (
        ("samples", bucket_samples),
        ("valid", bucket_valid),
    ):
        expected = aggregate[name]
        if expected is not None and observed != expected:
            issues.append(
                f"RCS probe bucket {name} sum={observed}, "
                f"aggregate={expected}"
            )
    if phases is not None:
        for name, observed in bucket_phases.items():
            expected = phases[name]
            if observed != expected:
                issues.append(
                    f"RCS probe bucket {name} sum={observed}, "
                    f"aggregate={expected}"
                )


def ratio_to_nearest_mhz(ratio: int) -> int:
    """Match the Gen9+ 16.67 MHz ratio conversion used by the kernel."""

    return (ratio * 50 + 1) // 3


def active_ratio_average_mhz(ratio_sum: int, active: int) -> int:
    """Match Lumen's nearest-MHz average, including the zero-sample case."""

    if active == 0:
        return 0
    denominator = active * 3
    return (ratio_sum * 50 + denominator // 2) // denominator


def validate_gt_state(capture: TurnCapture, issues: list[str]) -> None:
    record = capture.gt_state
    if record is None:
        return

    schema = require_int(record, "schema", "GT state", issues)
    if schema is not None and schema != 1:
        issues.append(f"GT state schema={schema}, expected 1")
    if record.fields.get("scope") != "turn":
        issues.append(
            f"GT state scope={record.fields.get('scope')!r}, expected 'turn'"
        )
    for field_name, expected in (
        ("sampling", GT_STATE_SAMPLING),
        ("observation", GT_STATE_OBSERVATION),
        ("register", GT_STATE_REGISTER),
        ("bucket_schema", GT_STATE_BUCKET_SCHEMA),
    ):
        observed = record.fields.get(field_name)
        if observed != expected:
            issues.append(
                f"GT state {field_name}={observed!r}, expected {expected!r}"
            )

    integer_fields = (
        "available",
        "samples",
        "start_active",
        "end_active",
        "start_zero",
        "end_zero",
        "start_ratio_sum",
        "end_ratio_sum",
        "final_actual_ratio",
        "final_actual_mhz",
        "requested_ratio",
        "requested_mhz",
        "rp0_mhz",
        "rpe_mhz",
        "rpn_mhz",
        "throttle_reasons",
        "rpstat1_raw",
        "rpnswreq_raw",
    )
    aggregate = {
        key: require_int(record, key, "GT state", issues)
        for key in integer_fields
    }
    for key, value in aggregate.items():
        if value is not None and value < 0:
            issues.append(f"GT state {key}={value}, expected non-negative")
    available = aggregate["available"]
    if available is not None and available not in (0, 1):
        issues.append(
            f"GT state available={available}, expected 0 or 1"
        )

    samples = aggregate["samples"]
    averages = parse_gt_state_active_avg_mhz(record)
    if averages is None:
        issues.append("GT state has missing or invalid active_avg_mhz")
    else:
        for endpoint, value in averages.items():
            if value < 0:
                issues.append(
                    f"GT state {endpoint} active_avg_mhz={value}, "
                    "expected non-negative"
                )

    for endpoint in ("start", "end"):
        active = aggregate[f"{endpoint}_active"]
        zero = aggregate[f"{endpoint}_zero"]
        ratio_sum = aggregate[f"{endpoint}_ratio_sum"]
        if (
            samples is not None
            and samples >= 0
            and active is not None
            and active >= 0
        ):
            if active > samples:
                issues.append(
                    f"GT state {endpoint}_active={active}, "
                    f"exceeds samples={samples}"
                )
            elif zero is not None and zero != samples - active:
                issues.append(
                    f"GT state {endpoint}_zero={zero}, expected "
                    f"samples-active={samples - active}"
                )
        if active is not None and active >= 0 and ratio_sum is not None:
            if (active == 0) != (ratio_sum == 0):
                issues.append(
                    f"GT state {endpoint}_active={active} and "
                    f"{endpoint}_ratio_sum={ratio_sum} violate the "
                    "zero iff inactive contract"
                )
            if ratio_sum >= 0 and averages is not None:
                expected_average = active_ratio_average_mhz(
                    ratio_sum,
                    active,
                )
                observed_average = averages[endpoint]
                if observed_average != expected_average:
                    issues.append(
                        f"GT state {endpoint} active_avg_mhz="
                        f"{observed_average}, expected {expected_average}"
                    )

    for prefix, ratio_key, mhz_key in (
        ("final actual", "final_actual_ratio", "final_actual_mhz"),
        ("requested", "requested_ratio", "requested_mhz"),
    ):
        ratio = aggregate[ratio_key]
        mhz = aggregate[mhz_key]
        if ratio is not None and ratio >= 0 and mhz is not None:
            if ratio > GEN12_CAGF_MASK:
                issues.append(
                    f"GT state {prefix} ratio={ratio}, "
                    f"exceeds Gen12 field maximum={GEN12_CAGF_MASK}"
                )
            expected_mhz = ratio_to_nearest_mhz(ratio)
            if mhz != expected_mhz:
                issues.append(
                    f"GT state {prefix} MHz={mhz}, "
                    f"expected ratio conversion={expected_mhz}"
                )

    for prefix, ratio_key, raw_key, shift in (
        (
            "final actual",
            "final_actual_ratio",
            "rpstat1_raw",
            GEN12_CAGF_SHIFT,
        ),
        (
            "requested",
            "requested_ratio",
            "rpnswreq_raw",
            GEN9_SW_REQ_UNSLICE_RATIO_SHIFT,
        ),
    ):
        ratio = aggregate[ratio_key]
        raw = aggregate[raw_key]
        if ratio is not None and ratio >= 0 and raw is not None and raw >= 0:
            decoded = (raw >> shift) & GEN12_CAGF_MASK
            if ratio != decoded:
                issues.append(
                    f"GT state {prefix} ratio={ratio}, "
                    f"raw {raw_key} decodes to {decoded}"
                )

    throttle_reasons = aggregate["throttle_reasons"]
    if (
        throttle_reasons is not None
        and throttle_reasons >= 0
        and throttle_reasons & ~GEN12_GT0_PERF_LIMIT_REASONS_MASK
    ):
        issues.append(
            f"GT state throttle_reasons=0x{throttle_reasons:X}, "
            "contains bits outside the Gen12 GT0 reason mask"
        )

    rp0 = aggregate["rp0_mhz"]
    rpe = aggregate["rpe_mhz"]
    rpn = aggregate["rpn_mhz"]
    caps = (rpn, rpe, rp0)
    if all(value is not None and value >= 0 for value in caps):
        assert rpn is not None and rpe is not None and rp0 is not None
        if not all(value > 0 for value in caps):
            issues.append(
                f"GT state fused frequencies RPn={rpn}, RPe={rpe}, "
                f"RP0={rp0}, expected all nonzero"
            )
        if not rpn <= rpe <= rp0:
            issues.append(
                f"GT state fused frequencies RPn={rpn}, RPe={rpe}, "
                f"RP0={rp0}, expected RPn<=RPe<=RP0"
            )
        if any(value % 50 != 0 for value in caps):
            issues.append(
                f"GT state fused frequencies RPn={rpn}, RPe={rpe}, "
                f"RP0={rp0}, expected 50 MHz steps"
            )

    buckets = parse_gt_state_buckets(record)
    if buckets is None:
        issues.append("GT state has missing or invalid buckets")
        return

    labels = set(buckets)
    expected_labels = set(SIGNATURE_LOGICAL_BYTES)
    missing = sorted(expected_labels - labels)
    extra = sorted(labels - expected_labels)
    if missing:
        issues.append("GT state buckets missing: " + ",".join(missing))
    if extra:
        issues.append("unexpected GT state buckets: " + ",".join(extra))

    bucket_totals = {
        "samples": 0,
        "start_active": 0,
        "end_active": 0,
        "start_ratio_sum": 0,
        "end_ratio_sum": 0,
    }
    for label, bucket in buckets.items():
        values = {
            "samples": bucket.samples,
            "start_active": bucket.start_active,
            "end_active": bucket.end_active,
            "start_ratio_sum": bucket.start_ratio_sum,
            "end_ratio_sum": bucket.end_ratio_sum,
        }
        for key, value in values.items():
            if value < 0:
                issues.append(
                    f"GT state bucket {label} {key}={value}, "
                    "expected non-negative"
                )
            bucket_totals[key] += value
        for endpoint in ("start", "end"):
            active = values[f"{endpoint}_active"]
            ratio_sum = values[f"{endpoint}_ratio_sum"]
            if active > bucket.samples:
                issues.append(
                    f"GT state bucket {label} {endpoint}_active={active}, "
                    f"exceeds samples={bucket.samples}"
                )
            if active >= 0 and (active == 0) != (ratio_sum == 0):
                issues.append(
                    f"GT state bucket {label} {endpoint}_active={active} "
                    f"and {endpoint}_ratio_sum={ratio_sum} violate the "
                    "zero iff inactive contract"
                )

    for key, observed in bucket_totals.items():
        expected = aggregate[key]
        if expected is not None and observed != expected:
            issues.append(
                f"GT state bucket {key} sum={observed}, aggregate={expected}"
            )

    rcs_buckets = parse_rcs_probe_buckets(capture.rcs_probe)
    if capture.rcs_probe is not None:
        rcs_samples = parse_int(capture.rcs_probe, "samples")
        if (
            samples is not None
            and rcs_samples is not None
            and samples != rcs_samples
        ):
            issues.append(
                f"GT state samples={samples}, RCS probe samples={rcs_samples}"
            )
        if rcs_buckets is not None:
            for label in labels & set(rcs_buckets):
                gt_samples = buckets[label].samples
                rcs_samples = rcs_buckets[label].samples
                if gt_samples != rcs_samples:
                    issues.append(
                        f"GT state bucket {label} samples={gt_samples}, "
                        f"RCS probe has {rcs_samples}"
                    )


def validate_m3_admission(capture: TurnCapture, issues: list[str]) -> None:
    record = capture.admission
    if record is None:
        return

    schema = require_int(record, "schema", "M3 admission", issues)
    if schema is not None and schema != 1:
        issues.append(f"M3 admission schema={schema}, expected 1")
    if record.fields.get("scope") != "turn":
        issues.append(
            "M3 admission "
            f"scope={record.fields.get('scope')!r}, expected 'turn'"
        )

    for field_name, expected in (
        ("bdf", M3_ADMISSION_BDF),
        ("checkpoints", M3_ADMISSION_CHECKPOINTS),
        ("expected_target", M3_ADMISSION_EXPECTED_TARGET),
    ):
        observed = record.fields.get(field_name)
        if observed != expected:
            issues.append(
                f"M3 admission {field_name}={observed!r}, "
                f"expected {expected!r}"
            )

    values = {
        key: require_int(record, key, "M3 admission", issues)
        for key in M3_ADMISSION_INTEGER_FIELDS
        if key != "schema"
    }
    for field_name in M3_ADMISSION_BOOLEAN_FIELDS:
        observed = values[field_name]
        if observed is not None and observed != 1:
            issues.append(
                f"M3 admission {field_name}={observed}, expected 1"
            )

    for field_name in ("boot_before_global", "boot_before_l3cc_pair"):
        observed = values[field_name]
        if observed is not None and observed < 0:
            issues.append(
                f"M3 admission {field_name}={observed}, "
                "expected non-negative"
            )

    exact_integer_fields = {
        "vendor": M3_ADMISSION_VENDOR,
        "device": M3_ADMISSION_DEVICE,
        "revision": M3_ADMISSION_REVISION,
        "boot_after_global": M3_ADMISSION_GLOBAL,
        "boot_after_l3cc_pair": M3_ADMISSION_L3CC_PAIR,
        "end_global": M3_ADMISSION_GLOBAL,
        "end_l3cc_pair": M3_ADMISSION_L3CC_PAIR,
        "expected_global": M3_ADMISSION_GLOBAL,
        "expected_l3cc_pair": M3_ADMISSION_L3CC_PAIR,
    }
    for field_name, expected in exact_integer_fields.items():
        observed = values[field_name]
        if observed is not None and observed != expected:
            issues.append(
                f"M3 admission {field_name}={fmt_hex(observed)}, "
                f"expected {fmt_hex(expected)}"
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
    require_rcs_probe: bool = False,
    require_gt_state: bool = False,
    require_m3_admission: bool = False,
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
    if require_rcs_probe and capture.rcs_probe is None:
        issues.append("missing turn-rcs-probe record")
    if require_rcs_probe and capture.rcs_probe is not None:
        samples = parse_int(capture.rcs_probe, "samples")
        valid = parse_int(capture.rcs_probe, "valid")
        if samples is not None and samples <= 0:
            issues.append(
                f"turn-rcs-probe samples={samples}, expected positive"
            )
        if valid is not None and valid <= 0:
            issues.append(
                f"turn-rcs-probe valid={valid}, expected positive"
            )
    if require_gt_state and capture.gt_state is None:
        issues.append("missing turn-gt-state record")
    if require_gt_state and capture.gt_state is not None:
        available = parse_int(capture.gt_state, "available")
        if available != 1:
            issues.append(
                f"turn-gt-state available={available}, expected 1"
            )
        samples = parse_int(capture.gt_state, "samples")
        if samples is not None and samples <= 0:
            issues.append(
                f"turn-gt-state samples={samples}, expected positive"
            )
    if require_m3_admission and capture.admission is None:
        issues.append("missing turn-admission record")

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
    validate_rcs_probe(capture, issues)
    validate_gt_state(capture, issues)
    validate_m3_admission(capture, issues)

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


def validate_campaign_results(
    results: Sequence[TurnResult],
    *,
    require_rcs_probe: bool = False,
    require_gt_state: bool = False,
    require_m3_admission: bool = False,
) -> list[TurnResult]:
    """Select fresh sessions and apply the ``hi, hi, sky`` contract."""

    validated: list[TurnResult] = []
    resident_boundaries: set[tuple[str, int]] = set()
    campaign_ordinal = 0

    for result in results:
        capture = result.capture
        if not capture.has_done:
            validated.append(result)
            continue

        exclusion_reasons: list[str] = []
        for stage, record in (
            ("start", capture.start),
            ("prefill", capture.prefill),
            ("done", capture.done),
        ):
            if record is None:
                continue
            turn = parse_int(record, "turn")
            if turn != 1:
                exclusion_reasons.append(
                    f"{stage} turn={turn}, expected fresh turn=1"
                )
            context_before = parse_int(record, "context_before")
            if context_before != 0:
                exclusion_reasons.append(
                    f"{stage} context_before={context_before}, expected 0"
                )

        if capture.resident is None:
            exclusion_reasons.append(
                "no associated resident boundary"
            )
        else:
            resident_key = (
                capture.resident.source,
                capture.resident.line_number,
            )
            if resident_key in resident_boundaries:
                exclusion_reasons.append(
                    "resident boundary already belongs to an earlier turn: "
                    f"{capture.resident.source}:{capture.resident.line_number}"
                )
            resident_boundaries.add(resident_key)
            first_record = next(
                (
                    record
                    for record in (capture.start, capture.prefill, capture.done)
                    if record is not None
                ),
                None,
            )
            if (
                first_record is not None
                and (
                    capture.resident.source != first_record.source
                    or capture.resident.line_number >= first_record.line_number
                )
            ):
                exclusion_reasons.append(
                    "associated resident boundary is not before the turn"
                )

        if exclusion_reasons:
            validated.append(
                TurnResult(
                    capture=capture,
                    issues=result.issues,
                    canonical=result.canonical,
                    expected=result.expected,
                    metrics=result.metrics,
                    campaign_disposition="excluded",
                    campaign_notes=tuple(dict.fromkeys(exclusion_reasons)),
                )
            )
            continue

        campaign_ordinal += 1
        run_number = campaign_ordinal
        identity_name = CAMPAIGN_IDENTITY_SEQUENCE[
            (campaign_ordinal - 1) % len(CAMPAIGN_IDENTITY_SEQUENCE)
        ]
        identity = CANONICAL_REPLY_BY_NAME[identity_name]
        detailed = validate_turn(
            capture,
            require_detail=True,
            require_rcs_probe=require_rcs_probe,
            require_gt_state=require_gt_state,
            require_m3_admission=require_m3_admission,
        )
        issues = list(detailed.issues)
        done = capture.done
        if done is not None:
            observed = {
                "prompt_tokens": parse_int(done, "prompt_tokens"),
                "reply_tokens": parse_int(done, "reply_tokens"),
                "stop": done.fields.get("stop"),
                "first_token": parse_int(done, "first_token"),
                "raw_reply_sha256": done.fields.get("raw_reply_sha256"),
            }
            expected = {
                "prompt_tokens": identity.prompt_tokens,
                "reply_tokens": identity.reply_tokens,
                "stop": identity.stop,
                "first_token": identity.first_token,
                "raw_reply_sha256": identity.sha256,
            }
            for field_name, expected_value in expected.items():
                observed_value = observed[field_name]
                matches = (
                    isinstance(observed_value, str)
                    and isinstance(expected_value, str)
                    and observed_value.lower() == expected_value.lower()
                ) or observed_value == expected_value
                if not matches:
                    issues.append(
                        f"campaign run {run_number} identity={identity.name} "
                        f"{field_name}={observed_value!r}, "
                        f"expected {expected_value!r}"
                    )

        validated.append(
            TurnResult(
                capture=capture,
                issues=tuple(dict.fromkeys(issues)),
                canonical=detailed.canonical,
                expected=detailed.expected,
                metrics=detailed.metrics,
                campaign_disposition="selected",
                campaign_ordinal=campaign_ordinal,
                campaign_notes=(f"expected identity={identity.name}",),
            )
        )

    return validated


def fmt_int(value: int | None) -> str:
    return "-" if value is None else str(value)


def fmt_float(value: float | None, digits: int = 2) -> str:
    return "-" if value is None else f"{value:.{digits}f}"


def fmt_hex(value: int | None) -> str:
    return "-" if value is None else f"0x{value:X}"


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


def rcs_probe_dict(record: Record | None) -> dict[str, object] | None:
    if record is None:
        return None
    buckets = parse_rcs_probe_buckets(record)
    return {
        "line": record.line_number,
        "schema": parse_int(record, "schema"),
        "samples": parse_int(record, "samples"),
        "valid": parse_int(record, "valid"),
        "invalid": parse_int(record, "invalid"),
        "phase_us": parse_rcs_probe_phase_us(record),
        "gpu_hz": parse_int(record, "gpu_hz"),
        "policy": record.fields.get("policy"),
        "clock": record.fields.get("clock"),
        "bucket_schema": record.fields.get("bucket_schema"),
        "buckets": (
            [
                {
                    "signature": bucket.signature,
                    "samples": bucket.samples,
                    "valid": bucket.valid,
                    "invalid": bucket.samples - bucket.valid,
                    "phase_us": dict(bucket.phase_us),
                }
                for bucket in buckets.values()
            ]
            if buckets is not None
            else None
        ),
    }


def gt_state_dict(record: Record | None) -> dict[str, object] | None:
    if record is None:
        return None
    buckets = parse_gt_state_buckets(record)
    return {
        "line": record.line_number,
        "schema": parse_int(record, "schema"),
        "available": parse_int(record, "available"),
        "samples": parse_int(record, "samples"),
        "start_active": parse_int(record, "start_active"),
        "end_active": parse_int(record, "end_active"),
        "start_zero": parse_int(record, "start_zero"),
        "end_zero": parse_int(record, "end_zero"),
        "start_ratio_sum": parse_int(record, "start_ratio_sum"),
        "end_ratio_sum": parse_int(record, "end_ratio_sum"),
        "active_avg_mhz": parse_gt_state_active_avg_mhz(record),
        "final_actual_ratio": parse_int(record, "final_actual_ratio"),
        "final_actual_mhz": parse_int(record, "final_actual_mhz"),
        "requested_ratio": parse_int(record, "requested_ratio"),
        "requested_mhz": parse_int(record, "requested_mhz"),
        "rp0_mhz": parse_int(record, "rp0_mhz"),
        "rpe_mhz": parse_int(record, "rpe_mhz"),
        "rpn_mhz": parse_int(record, "rpn_mhz"),
        "throttle_reasons": parse_int(record, "throttle_reasons"),
        "rpstat1_raw": parse_int(record, "rpstat1_raw"),
        "rpnswreq_raw": parse_int(record, "rpnswreq_raw"),
        "sampling": record.fields.get("sampling"),
        "observation": record.fields.get("observation"),
        "register": record.fields.get("register"),
        "bucket_schema": record.fields.get("bucket_schema"),
        "buckets": (
            [
                {
                    "signature": bucket.signature,
                    "samples": bucket.samples,
                    "start_active": bucket.start_active,
                    "end_active": bucket.end_active,
                    "start_ratio_sum": bucket.start_ratio_sum,
                    "end_ratio_sum": bucket.end_ratio_sum,
                }
                for bucket in buckets.values()
            ]
            if buckets is not None
            else None
        ),
    }


def m3_admission_dict(record: Record | None) -> dict[str, object] | None:
    if record is None:
        return None
    return {
        "line": record.line_number,
        "stage": record.fields.get("stage"),
        "scope": record.fields.get("scope"),
        "turn": parse_int(record, "turn"),
        "bdf": record.fields.get("bdf"),
        **{
            field_name: parse_int(record, field_name)
            for field_name in M3_ADMISSION_INTEGER_FIELDS
        },
        "checkpoints": record.fields.get("checkpoints"),
        "expected_target": record.fields.get("expected_target"),
    }


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
            "excluded"
            if result.campaign_disposition == "excluded"
            else "pass"
            if result.passed
            else "fail"
            if capture.has_done
            else "incomplete"
        ),
        "campaign": {
            "disposition": result.campaign_disposition,
            "run": result.campaign_ordinal,
            "notes": list(result.campaign_notes),
        },
        "canonical": result.canonical.name if result.canonical else None,
        "issues": list(result.issues),
        "context": context_dict(capture),
        "observed": dict(done.fields) if done is not None else None,
        "cpu": (
            {"line": capture.cpu.line_number, **dict(capture.cpu.fields)}
            if capture.cpu is not None
            else None
        ),
        "rcs_probe": rcs_probe_dict(capture.rcs_probe),
        "gt_state": gt_state_dict(capture.gt_state),
        "admission": m3_admission_dict(capture.admission),
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
    *,
    campaign_mode: bool = False,
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
            "EXCLUDED"
            if result.campaign_disposition == "excluded"
            else "PASS"
            if result.passed
            else "FAIL"
            if capture.has_done
            else "INCOMPLETE"
        )
        canonical = result.canonical.name if result.canonical else "-"
        campaign_run = (
            str(result.campaign_ordinal)
            if result.campaign_ordinal is not None
            else "-"
        )
        campaign_label = (
            f" campaign_run={campaign_run}" if campaign_mode else ""
        )
        print(
            f"\nrun={run_number} status={status} source={capture.source} "
            f"source_run={capture.source_ordinal} turn={capture.declared_turn} "
            f"lines={capture.line_span} canonical={canonical}"
            f"{campaign_label}"
        )
        for note in result.campaign_notes:
            print(f"  campaign: {result.campaign_disposition}: {note}")

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

        if capture.rcs_probe is not None:
            probe = capture.rcs_probe
            phases = parse_rcs_probe_phase_us(probe)
            print(
                "  rcs_probe "
                f"line={probe.line_number} "
                f"schema={probe.fields.get('schema', '-')} "
                f"samples={probe.fields.get('samples', '-')} "
                f"valid={probe.fields.get('valid', '-')} "
                f"invalid={probe.fields.get('invalid', '-')} "
                f"queue_to_batch_us="
                f"{fmt_int(phases.get('queue_to_batch') if phases else None)} "
                f"preamble_us="
                f"{fmt_int(phases.get('preamble') if phases else None)} "
                f"walkers_us="
                f"{fmt_int(phases.get('walkers') if phases else None)} "
                f"epilogue_us="
                f"{fmt_int(phases.get('epilogue') if phases else None)} "
                f"release_to_observe_us="
                f"{fmt_int(phases.get('release_to_observe') if phases else None)} "
                f"queue_to_observe_us="
                f"{fmt_int(phases.get('queue_to_observe') if phases else None)}"
            )
            buckets = parse_rcs_probe_buckets(probe)
            if buckets is not None:
                print("  rcs_probe_buckets")
                for label in SIGNATURE_LOGICAL_BYTES:
                    bucket = buckets.get(label)
                    if bucket is None:
                        continue
                    print(
                        f"    {label:<13} "
                        f"samples={bucket.samples} valid={bucket.valid} "
                        f"q2b_us={bucket.phase_us['queue_to_batch']} "
                        f"pre_us={bucket.phase_us['preamble']} "
                        f"walk_us={bucket.phase_us['walkers']} "
                        f"epi_us={bucket.phase_us['epilogue']} "
                        f"release_us={bucket.phase_us['release_to_observe']} "
                        f"q2o_us={bucket.phase_us['queue_to_observe']}"
                    )

        if capture.gt_state is not None:
            gt_state = capture.gt_state
            averages = parse_gt_state_active_avg_mhz(gt_state)
            print(
                "  gt_state "
                f"line={gt_state.line_number} "
                f"schema={gt_state.fields.get('schema', '-')} "
                f"available={gt_state.fields.get('available', '-')} "
                f"samples={gt_state.fields.get('samples', '-')} "
                f"start_active={gt_state.fields.get('start_active', '-')} "
                f"start_zero={gt_state.fields.get('start_zero', '-')} "
                f"start_avg_mhz="
                f"{fmt_int(averages.get('start') if averages else None)} "
                f"end_active={gt_state.fields.get('end_active', '-')} "
                f"end_zero={gt_state.fields.get('end_zero', '-')} "
                f"end_avg_mhz="
                f"{fmt_int(averages.get('end') if averages else None)} "
                f"final_actual_ratio="
                f"{gt_state.fields.get('final_actual_ratio', '-')} "
                f"final_actual_mhz="
                f"{gt_state.fields.get('final_actual_mhz', '-')} "
                f"requested_ratio="
                f"{gt_state.fields.get('requested_ratio', '-')} "
                f"requested_mhz="
                f"{gt_state.fields.get('requested_mhz', '-')} "
                f"rp0_mhz={gt_state.fields.get('rp0_mhz', '-')} "
                f"rpe_mhz={gt_state.fields.get('rpe_mhz', '-')} "
                f"rpn_mhz={gt_state.fields.get('rpn_mhz', '-')} "
                f"throttle_reasons="
                f"{fmt_hex(parse_int(gt_state, 'throttle_reasons'))} "
                f"rpstat1_raw={fmt_hex(parse_int(gt_state, 'rpstat1_raw'))} "
                f"rpnswreq_raw={fmt_hex(parse_int(gt_state, 'rpnswreq_raw'))}"
            )
            buckets = parse_gt_state_buckets(gt_state)
            if buckets is not None:
                print("  gt_state_buckets")
                for label in SIGNATURE_LOGICAL_BYTES:
                    bucket = buckets.get(label)
                    if bucket is None:
                        continue
                    print(
                        f"    {label:<13} "
                        f"samples={bucket.samples} "
                        f"start_active={bucket.start_active} "
                        f"end_active={bucket.end_active} "
                        f"start_ratio_sum={bucket.start_ratio_sum} "
                        f"end_ratio_sum={bucket.end_ratio_sum}"
                    )

        if capture.admission is not None:
            admission = capture.admission
            print(
                "  admission "
                f"line={admission.line_number} "
                f"schema={admission.fields.get('schema', '-')} "
                f"bdf={admission.fields.get('bdf', '-')} "
                f"vendor={fmt_hex(parse_int(admission, 'vendor'))} "
                f"device={fmt_hex(parse_int(admission, 'device'))} "
                f"revision={fmt_hex(parse_int(admission, 'revision'))} "
                f"checkpoints={admission.fields.get('checkpoints', '-')} "
                f"expected_target="
                f"{admission.fields.get('expected_target', '-')}"
            )
            print(
                "  admission_evidence "
                + " ".join(
                    f"{field_name}="
                    f"{fmt_int(parse_int(admission, field_name))}"
                    for field_name in M3_ADMISSION_BOOLEAN_FIELDS
                )
            )
            print(
                "  admission_cache "
                f"boot_before_global="
                f"{fmt_hex(parse_int(admission, 'boot_before_global'))} "
                f"boot_before_l3cc_pair="
                f"{fmt_hex(parse_int(admission, 'boot_before_l3cc_pair'))} "
                f"boot_after_global="
                f"{fmt_hex(parse_int(admission, 'boot_after_global'))} "
                f"boot_after_l3cc_pair="
                f"{fmt_hex(parse_int(admission, 'boot_after_l3cc_pair'))} "
                f"end_global={fmt_hex(parse_int(admission, 'end_global'))} "
                f"end_l3cc_pair="
                f"{fmt_hex(parse_int(admission, 'end_l3cc_pair'))} "
                f"expected_global="
                f"{fmt_hex(parse_int(admission, 'expected_global'))} "
                f"expected_l3cc_pair="
                f"{fmt_hex(parse_int(admission, 'expected_l3cc_pair'))}"
            )

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
    if campaign_mode:
        selected = sum(
            result.campaign_disposition == "selected" for result in results
        )
        excluded = sum(
            result.campaign_disposition == "excluded" for result in results
        )
        passed = sum(
            result.campaign_disposition == "selected" and result.passed
            for result in results
        )
        failed = sum(
            result.campaign_disposition == "selected" and not result.passed
            for result in results
        )
    else:
        selected = 0
        excluded = 0
        passed = sum(result.passed for result in results)
        failed = sum(
            result.capture.has_done and not result.passed for result in results
        )
    incomplete = len(results) - completed
    campaign_suffix = (
        f" campaign_selected={selected} excluded={excluded}"
        if campaign_mode
        else ""
    )
    print(
        f"\nsummary runs={len(results)} completed={completed} passed={passed} "
        f"failed={failed} incomplete={incomplete}{campaign_suffix}"
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
        help=(
            "require exactly N completed fresh-session campaign runs in the "
            "repeating hi,hi,sky sequence (use 9 for the full campaign)"
        ),
    )
    parser.add_argument(
        "--require-rcs-probe",
        action="store_true",
        help=(
            "fail completed turns that do not include schema-1 "
            "turn-rcs-probe telemetry"
        ),
    )
    parser.add_argument(
        "--require-gt-state",
        action="store_true",
        help=(
            "fail completed turns that do not include schema-1 "
            "turn-gt-state telemetry"
        ),
    )
    parser.add_argument(
        "--require-m3-admission",
        action="store_true",
        help=(
            "fail completed turns that do not include valid schema-1 "
            "turn-admission evidence for the M3 target and cache policy"
        ),
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
        validate_turn(
            capture,
            require_rcs_probe=args.require_rcs_probe,
            require_gt_state=args.require_gt_state,
            require_m3_admission=args.require_m3_admission,
        )
        for capture in captures
    ]
    if args.expect_runs is not None:
        results = validate_campaign_results(
            results,
            require_rcs_probe=args.require_rcs_probe,
            require_gt_state=args.require_gt_state,
            require_m3_admission=args.require_m3_admission,
        )
    completed = sum(capture.has_done for capture in captures)
    campaign_selected = sum(
        result.campaign_disposition == "selected" for result in results
    )
    campaign_excluded = sum(
        result.campaign_disposition == "excluded" for result in results
    )
    if args.expect_runs is not None:
        failed_completed = any(
            result.campaign_disposition == "selected" and not result.passed
            for result in results
        )
    else:
        failed_completed = any(
            result.capture.has_done and not result.passed for result in results
        )
    requirement_issues: list[str] = []
    if args.require_turns and completed == 0:
        requirement_issues.append("no completed turn telemetry found")
    if args.expect_runs is not None and campaign_selected != args.expect_runs:
        requirement_issues.append(
            f"campaign runs={campaign_selected}, expected {args.expect_runs}"
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
                "rcs_probe_phase_rounding": (
                    "the sum of five independently rounded component totals "
                    "may differ from queue_to_observe_us by at most 2 us per "
                    "valid sample"
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
                "passed": sum(
                    (
                        result.campaign_disposition == "selected"
                        if args.expect_runs is not None
                        else result.capture.has_done
                    )
                    and result.passed
                    for result in results
                ),
                "failed": sum(
                    (
                        result.campaign_disposition == "selected"
                        if args.expect_runs is not None
                        else result.capture.has_done
                    )
                    and not result.passed
                    for result in results
                ),
                "incomplete": len(results) - completed,
                "campaign_selected": (
                    campaign_selected if args.expect_runs is not None else None
                ),
                "campaign_excluded": (
                    campaign_excluded if args.expect_runs is not None else None
                ),
                "requirement_issues": requirement_issues,
            },
        }
        print(json.dumps(document, indent=2, sort_keys=True))
    else:
        print_text_report(
            parsed_logs,
            results,
            campaign_mode=args.expect_runs is not None,
        )
        for issue in requirement_issues:
            print(f"requirement failure: {issue}", file=sys.stderr)

    return 1 if failed_completed or requirement_issues else 0


def main() -> int:
    return run()


if __name__ == "__main__":
    raise SystemExit(main())
