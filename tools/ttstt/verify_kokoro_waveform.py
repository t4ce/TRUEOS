#!/usr/bin/env python3
"""Fail-closed waveform parity gate for the pinned Kokoro KKAOT program.

The numerical reference is the exact-source RTen run, not the ONNX Runtime
compatibility run.  RTen and KKAOT execute the prepared graph with float
BiLSTMs; the older ORT model uses dynamically quantized LSTMs and is therefore
only a perceptual reference.

Pinned invocation contract:

* phonemes: ``həlˈoʊ fɹʌm tɹu oʊ ɛs. ðə kwɪk bɹaʊn fɑks dʒʌmps oʊvɚ ðə leɪzi dɔɡ. spɪtʃ
  sɪnθəsɪs ɪz naʊ ɹʌnɪŋ ɪn ðə kɜɹnəl, wɪð ə sɪɹiəlaɪzd eɪsɪŋk kju fɔɹ ðə ʃɛl.``
* voice ``af_heart``, style row 149, speed 1.0
* padded token shape ``[1, 151]`` (149 IPA tokens plus BOS/EOS zeroes)
* decoder frames ``F=824``
* waveform shape ``[247200]`` (300 samples per frame), mono f32 at 24 kHz

Only the Python standard library is required.  Both RIFF/WAVE IEEE-f32
(including WAVE_FORMAT_EXTENSIBLE) and headerless little-endian f32 inputs are
accepted.  A native repeat is required by default so deterministic native
payload hashes are part of the parity claim.
"""

from __future__ import annotations

import argparse
import array
from dataclasses import asdict, dataclass
import hashlib
import json
import math
from pathlib import Path
import struct
import sys
from typing import Sequence


REFERENCE_IPA = (
    "həlˈoʊ fɹʌm tɹu oʊ ɛs. ðə kwɪk bɹaʊn fɑks dʒʌmps oʊvɚ ðə leɪzi dɔɡ. "
    "spɪtʃ sɪnθəsɪs ɪz naʊ ɹʌnɪŋ ɪn ðə kɜɹnəl, wɪð ə sɪɹiəlaɪzd eɪsɪŋk kju fɔɹ ðə ʃɛl."
)
REFERENCE_IPA_SHA256 = "8a75aa1cd95cc4b3162952013521cb675e8251ed245e8715b370b1d6b460a18b"
REFERENCE_VOICE = "af_heart"
REFERENCE_SPEED = 1.0
REFERENCE_STYLE_ROW = 149
REFERENCE_TOKEN_COUNT = 149
REFERENCE_PADDED_TOKEN_COUNT = 151
REFERENCE_PADDED_I32LE_SHA256 = (
    "a456bb84dc5704dd80372df5dd14c3160e397435eec8bc789ac457c594998e7d"
)

EXPECTED_DECODER_FRAMES = 824
SAMPLES_PER_DECODER_FRAME = 300
EXPECTED_SAMPLE_COUNT = EXPECTED_DECODER_FRAMES * SAMPLES_PER_DECODER_FRAME
EXPECTED_SAMPLE_RATE = 24_000
EXPECTED_CHANNELS = 1
EXPECTED_SAMPLE_BYTES = 4

REFERENCE_WAV_BYTES = 988_868
REFERENCE_WAV_SHA256 = "754ce3b947dde9dbe99279a77a3b7ddf85a0be1bc2dc05663864e40bf8be4388"
REFERENCE_PAYLOAD_SHA256 = (
    "e9cde5b662b5ee34604fcfdebfbf2ffef2e6b8751f133f46b6b7cb1d2212f8e9"
)

KKAOT_BYTES = 124_081_360
KKAOT_FILE_SHA256 = "b7d4b9c62f3df01f71fc2585acd60190d1809dae9a6d916fe39c86d7dd4e3217"
KKAOT_ARTIFACT_SEAL_SHA256 = (
    "f1f5ccc668e171301e7220033992efcb2669f9d401591aace4f1025ac1e34998"
)
KKAOT_MODEL_SHA256 = "239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29"
KKAOT_VOICES_SHA256 = "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d"

DEFAULT_MIN_CORRELATION = 0.99
DEFAULT_MIN_SNR_DB = 20.0
DEFAULT_MAX_RMSE = 0.01
DEFAULT_MAX_ABS_ERROR = 0.15
ABSOLUTE_SAMPLE_LIMIT = 1.0
MIN_NATIVE_RMS = 1.0e-6

FLOAT_SUBFORMAT_GUID = bytes.fromhex("0300000000001000800000aa00389b71")


class VerificationError(ValueError):
    """An input cannot satisfy the pinned waveform contract."""


@dataclass(frozen=True)
class AudioFormat:
    container: str
    sample_rate: int | None
    channels: int | None
    sample_bytes: int
    data_offset: int


@dataclass(frozen=True)
class Waveform:
    path: Path
    samples: array.array[float]
    payload: bytes
    file_sha256: str
    payload_sha256: str
    audio_format: AudioFormat


@dataclass(frozen=True)
class SampleStats:
    count: int
    decoder_frames: int
    minimum: float
    maximum: float
    peak: float
    mean: float
    rms: float


@dataclass(frozen=True)
class Metrics:
    max_abs_error: float
    rmse: float
    correlation: float
    snr_db: float
    mean_error: float


@dataclass(frozen=True)
class Thresholds:
    min_correlation: float
    min_snr_db: float
    max_rmse: float
    max_abs_error: float


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _riff_payload(data: bytes, path: Path) -> tuple[bytes, AudioFormat]:
    if len(data) < 12 or data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise VerificationError(f"{path}: not a RIFF/WAVE file")
    declared_size = struct.unpack_from("<I", data, 4)[0] + 8
    if declared_size != len(data):
        raise VerificationError(
            f"{path}: RIFF byte count is {declared_size}, file has {len(data)}"
        )

    fmt: bytes | None = None
    payload: bytes | None = None
    data_offset = 0
    offset = 12
    while offset < len(data):
        if offset + 8 > len(data):
            raise VerificationError(f"{path}: truncated RIFF chunk header")
        chunk_id = data[offset : offset + 4]
        chunk_bytes = struct.unpack_from("<I", data, offset + 4)[0]
        body = offset + 8
        end = body + chunk_bytes
        if end > len(data):
            raise VerificationError(f"{path}: truncated {chunk_id!r} chunk")
        if chunk_id == b"fmt ":
            if fmt is not None:
                raise VerificationError(f"{path}: duplicate fmt chunk")
            fmt = data[body:end]
        elif chunk_id == b"data":
            if payload is not None:
                raise VerificationError(f"{path}: duplicate data chunk")
            payload = data[body:end]
            data_offset = body
        offset = end + (chunk_bytes & 1)

    if offset != len(data):
        raise VerificationError(f"{path}: invalid RIFF padding")
    if fmt is None or payload is None:
        raise VerificationError(f"{path}: missing fmt or data chunk")
    if len(fmt) < 16:
        raise VerificationError(f"{path}: truncated fmt chunk")

    format_tag, channels, rate, byte_rate, block_align, bits = struct.unpack_from(
        "<HHIIHH", fmt
    )
    if format_tag == 0xFFFE:
        if len(fmt) < 40:
            raise VerificationError(f"{path}: truncated extensible fmt chunk")
        extension_bytes, valid_bits = struct.unpack_from("<HH", fmt, 16)
        if extension_bytes < 22 or valid_bits != 32 or fmt[24:40] != FLOAT_SUBFORMAT_GUID:
            raise VerificationError(f"{path}: extensible format is not IEEE f32")
    elif format_tag != 3:
        raise VerificationError(
            f"{path}: WAVE format {format_tag:#06x} is not IEEE f32"
        )
    if bits != 32 or block_align != channels * 4 or byte_rate != rate * block_align:
        raise VerificationError(f"{path}: inconsistent f32 WAVE format fields")

    return payload, AudioFormat("wav-f32le", rate, channels, 4, data_offset)


def read_waveform(path: Path) -> Waveform:
    try:
        data = path.read_bytes()
    except OSError as error:
        raise VerificationError(f"{path}: cannot read waveform: {error}") from error
    if data[:4] == b"RIFF":
        payload, audio_format = _riff_payload(data, path)
    else:
        payload = data
        audio_format = AudioFormat("raw-f32le", None, None, 4, 0)
    if not payload or len(payload) % EXPECTED_SAMPLE_BYTES != 0:
        raise VerificationError(
            f"{path}: f32 payload byte count {len(payload)} is invalid"
        )
    samples = array.array("f")
    samples.frombytes(payload)
    if sys.byteorder != "little":
        samples.byteswap()
    return Waveform(
        path=path,
        samples=samples,
        payload=payload,
        file_sha256=sha256(data),
        payload_sha256=sha256(payload),
        audio_format=audio_format,
    )


def validate_shape(waveform: Waveform, role: str) -> SampleStats:
    fmt = waveform.audio_format
    if fmt.sample_bytes != EXPECTED_SAMPLE_BYTES:
        raise VerificationError(f"{role}: sample width is not f32")
    if fmt.container == "wav-f32le":
        if fmt.sample_rate != EXPECTED_SAMPLE_RATE or fmt.channels != EXPECTED_CHANNELS:
            raise VerificationError(
                f"{role}: expected mono {EXPECTED_SAMPLE_RATE}-Hz WAVE, got "
                f"channels={fmt.channels} rate={fmt.sample_rate}"
            )
    count = len(waveform.samples)
    if count != EXPECTED_SAMPLE_COUNT:
        raise VerificationError(
            f"{role}: expected {EXPECTED_SAMPLE_COUNT} samples, got {count}"
        )
    if count % SAMPLES_PER_DECODER_FRAME != 0:
        raise VerificationError(f"{role}: waveform is not frame-exact")
    frames = count // SAMPLES_PER_DECODER_FRAME
    if frames != EXPECTED_DECODER_FRAMES:
        raise VerificationError(
            f"{role}: expected F={EXPECTED_DECODER_FRAMES}, got F={frames}"
        )
    if not all(math.isfinite(value) for value in waveform.samples):
        raise VerificationError(f"{role}: waveform contains NaN or infinity")

    minimum = min(waveform.samples)
    maximum = max(waveform.samples)
    peak = max(abs(minimum), abs(maximum))
    mean = math.fsum(waveform.samples) / count
    rms = math.sqrt(math.fsum(value * value for value in waveform.samples) / count)
    if peak > ABSOLUTE_SAMPLE_LIMIT:
        raise VerificationError(
            f"{role}: peak {peak:.9g} exceeds absolute f32 audio limit "
            f"{ABSOLUTE_SAMPLE_LIMIT}"
        )
    return SampleStats(count, frames, minimum, maximum, peak, mean, rms)


def validate_reference(waveform: Waveform) -> None:
    if waveform.payload_sha256 != REFERENCE_PAYLOAD_SHA256:
        raise VerificationError(
            "reference: payload SHA-256 does not match the pinned RTen oracle"
        )
    if waveform.audio_format.container == "wav-f32le":
        if waveform.file_sha256 != REFERENCE_WAV_SHA256:
            raise VerificationError(
                "reference: WAVE SHA-256 does not match the pinned RTen oracle"
            )
        try:
            size = waveform.path.stat().st_size
        except OSError as error:
            raise VerificationError(
                f"reference: cannot stat {waveform.path}: {error}"
            ) from error
        if size != REFERENCE_WAV_BYTES:
            raise VerificationError(
                f"reference: expected {REFERENCE_WAV_BYTES} WAVE bytes, got {size}"
            )


def validate_kkaot(path: Path) -> dict[str, object]:
    try:
        data = path.read_bytes()
    except OSError as error:
        raise VerificationError(f"KKAOT: cannot read {path}: {error}") from error
    failures: list[str] = []
    if len(data) != KKAOT_BYTES:
        failures.append(f"bytes={len(data)} expected={KKAOT_BYTES}")
    observed_file = sha256(data)
    if observed_file != KKAOT_FILE_SHA256:
        failures.append(f"file_sha256={observed_file}")
    if len(data) < 160 or data[:8] != b"KKAOTV1\0":
        failures.append("header/magic rejected")
        seal = model = voices = "unavailable"
    else:
        seal = data[64:96].hex()
        model = data[96:128].hex()
        voices = data[128:160].hex()
        if seal != KKAOT_ARTIFACT_SEAL_SHA256:
            failures.append(f"artifact_seal={seal}")
        if model != KKAOT_MODEL_SHA256:
            failures.append(f"model_sha256={model}")
        if voices != KKAOT_VOICES_SHA256:
            failures.append(f"voices_sha256={voices}")
    if failures:
        raise VerificationError("KKAOT: pinned contract rejected: " + "; ".join(failures))
    return {
        "path": str(path),
        "bytes": len(data),
        "file_sha256": observed_file,
        "artifact_seal_sha256": seal,
        "model_sha256": model,
        "voices_sha256": voices,
    }


def compute_metrics(reference: Sequence[float], candidate: Sequence[float]) -> Metrics:
    if len(reference) != len(candidate) or not reference:
        raise VerificationError("metrics: waveform lengths differ or are empty")
    count = len(reference)
    reference_mean = math.fsum(reference) / count
    candidate_mean = math.fsum(candidate) / count
    errors = [native - oracle for oracle, native in zip(reference, candidate, strict=True)]
    squared_error = math.fsum(error * error for error in errors)
    rmse = math.sqrt(squared_error / count)
    max_abs_error = max(abs(error) for error in errors)

    reference_energy = math.fsum(value * value for value in reference)
    if squared_error == 0.0:
        snr_db = math.inf
    elif reference_energy == 0.0:
        snr_db = -math.inf
    else:
        snr_db = 10.0 * math.log10(reference_energy / squared_error)

    covariance = math.fsum(
        (oracle - reference_mean) * (native - candidate_mean)
        for oracle, native in zip(reference, candidate, strict=True)
    )
    reference_variance = math.fsum(
        (value - reference_mean) ** 2 for value in reference
    )
    candidate_variance = math.fsum(
        (value - candidate_mean) ** 2 for value in candidate
    )
    denominator = math.sqrt(reference_variance * candidate_variance)
    correlation = covariance / denominator if denominator else math.nan
    if math.isfinite(correlation):
        correlation = max(-1.0, min(1.0, correlation))
    return Metrics(
        max_abs_error=max_abs_error,
        rmse=rmse,
        correlation=correlation,
        snr_db=snr_db,
        mean_error=candidate_mean - reference_mean,
    )


def quality_failures(
    metrics: Metrics, candidate_stats: SampleStats, thresholds: Thresholds
) -> list[str]:
    failures: list[str] = []
    if candidate_stats.rms < MIN_NATIVE_RMS:
        failures.append(
            f"native RMS {candidate_stats.rms:.9g} is below {MIN_NATIVE_RMS:.9g}"
        )
    if not math.isfinite(metrics.correlation) or metrics.correlation < thresholds.min_correlation:
        failures.append(
            f"correlation {metrics.correlation:.9g} < {thresholds.min_correlation:.9g}"
        )
    if metrics.snr_db < thresholds.min_snr_db:
        failures.append(f"SNR {metrics.snr_db:.9g} dB < {thresholds.min_snr_db:.9g} dB")
    if metrics.rmse > thresholds.max_rmse:
        failures.append(f"RMSE {metrics.rmse:.9g} > {thresholds.max_rmse:.9g}")
    if metrics.max_abs_error > thresholds.max_abs_error:
        failures.append(
            f"max absolute error {metrics.max_abs_error:.9g} > "
            f"{thresholds.max_abs_error:.9g}"
        )
    return failures


def contract() -> dict[str, object]:
    return {
        "input": {
            "ipa": REFERENCE_IPA,
            "ipa_utf8_sha256": REFERENCE_IPA_SHA256,
            "voice": REFERENCE_VOICE,
            "speed": REFERENCE_SPEED,
            "style_row": REFERENCE_STYLE_ROW,
            "token_count": REFERENCE_TOKEN_COUNT,
            "padded_token_shape": [1, REFERENCE_PADDED_TOKEN_COUNT],
            "padded_tokens_i32le_sha256": REFERENCE_PADDED_I32LE_SHA256,
        },
        "output": {
            "decoder_frames": EXPECTED_DECODER_FRAMES,
            "samples_per_decoder_frame": SAMPLES_PER_DECODER_FRAME,
            "sample_count": EXPECTED_SAMPLE_COUNT,
            "sample_rate": EXPECTED_SAMPLE_RATE,
            "channels": EXPECTED_CHANNELS,
            "dtype": "f32le",
            "reference_wav_bytes": REFERENCE_WAV_BYTES,
            "reference_wav_sha256": REFERENCE_WAV_SHA256,
            "reference_payload_sha256": REFERENCE_PAYLOAD_SHA256,
        },
        "kkaot": {
            "bytes": KKAOT_BYTES,
            "file_sha256": KKAOT_FILE_SHA256,
            "artifact_seal_sha256": KKAOT_ARTIFACT_SEAL_SHA256,
            "model_sha256": KKAOT_MODEL_SHA256,
            "voices_sha256": KKAOT_VOICES_SHA256,
        },
    }


def _json_number(value: float) -> float | str:
    if math.isinf(value):
        return "inf" if value > 0.0 else "-inf"
    if math.isnan(value):
        return "nan"
    return value


def _serializable_dataclass(value: object) -> dict[str, object]:
    result = asdict(value)
    return {
        key: _json_number(item) if isinstance(item, float) else item
        for key, item in result.items()
    }


def _waveform_report(waveform: Waveform, stats: SampleStats) -> dict[str, object]:
    return {
        "path": str(waveform.path),
        "container": waveform.audio_format.container,
        "file_sha256": waveform.file_sha256,
        "payload_sha256": waveform.payload_sha256,
        "stats": _serializable_dataclass(stats),
    }


def _controlled_perturbation(
    reference: Waveform, reference_stats: SampleStats, thresholds: Thresholds
) -> dict[str, object]:
    # Polarity inversion retains a legal, finite amplitude range but must fail
    # waveform identity.  This catches a vacuous length/hash-only verifier.
    perturbed = array.array("f", (-value for value in reference.samples))
    metrics = compute_metrics(reference.samples, perturbed)
    perturbed_stats = SampleStats(
        count=reference_stats.count,
        decoder_frames=reference_stats.decoder_frames,
        minimum=-reference_stats.maximum,
        maximum=-reference_stats.minimum,
        peak=reference_stats.peak,
        mean=-reference_stats.mean,
        rms=reference_stats.rms,
    )
    failures = quality_failures(metrics, perturbed_stats, thresholds)
    if not failures:
        raise VerificationError("controlled polarity perturbation was not rejected")
    return {
        "result": "rejected",
        "metrics": _serializable_dataclass(metrics),
        "failed_gates": failures,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, help="pinned RTen f32 WAVE/raw file")
    parser.add_argument("--native", type=Path, help="native KKAOT f32 WAVE/raw file")
    parser.add_argument(
        "--native-repeat",
        type=Path,
        action="append",
        default=[],
        help="independent native run; at least one is required unless diagnostic mode is used",
    )
    parser.add_argument("--kkaot", type=Path, help="also validate the exact KKAOT artifact")
    parser.add_argument(
        "--allow-single-native",
        action="store_true",
        help="diagnostic metrics only; deterministic native inference remains unproven",
    )
    parser.add_argument(
        "--min-correlation", type=float, default=DEFAULT_MIN_CORRELATION
    )
    parser.add_argument("--min-snr-db", type=float, default=DEFAULT_MIN_SNR_DB)
    parser.add_argument("--max-rmse", type=float, default=DEFAULT_MAX_RMSE)
    parser.add_argument(
        "--max-abs-error", type=float, default=DEFAULT_MAX_ABS_ERROR
    )
    parser.add_argument(
        "--controlled-perturbation",
        action="store_true",
        help="also prove that an in-range polarity perturbation is rejected",
    )
    parser.add_argument("--print-contract", action="store_true")
    parser.add_argument("--json", action="store_true", help="emit a JSON report")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.print_contract and args.reference is None and args.native is None:
        print(json.dumps(contract(), indent=2, sort_keys=True, ensure_ascii=False))
        return 0
    if args.reference is None or args.native is None:
        raise SystemExit("--reference and --native are required for verification")
    threshold_values = (
        args.min_correlation,
        args.min_snr_db,
        args.max_rmse,
        args.max_abs_error,
    )
    if not all(math.isfinite(value) for value in threshold_values):
        raise SystemExit("all numerical thresholds must be finite")
    if not -1.0 <= args.min_correlation <= 1.0:
        raise SystemExit("--min-correlation must be in [-1, 1]")
    if args.max_rmse < 0.0 or args.max_abs_error < 0.0:
        raise SystemExit("error thresholds must be non-negative")
    thresholds = Thresholds(
        args.min_correlation, args.min_snr_db, args.max_rmse, args.max_abs_error
    )

    try:
        reference = read_waveform(args.reference)
        reference_stats = validate_shape(reference, "reference")
        validate_reference(reference)
        native = read_waveform(args.native)
        native_stats = validate_shape(native, "native")
        metrics = compute_metrics(reference.samples, native.samples)
        failures = quality_failures(metrics, native_stats, thresholds)

        repeat_reports: list[dict[str, object]] = []
        if not args.native_repeat and not args.allow_single_native:
            failures.append(
                "native determinism unproven: provide at least one --native-repeat"
            )
        for index, repeat_path in enumerate(args.native_repeat, start=1):
            repeat = read_waveform(repeat_path)
            repeat_stats = validate_shape(repeat, f"native repeat {index}")
            if repeat.payload_sha256 != native.payload_sha256:
                failures.append(
                    f"native repeat {index} payload SHA-256 differs: "
                    f"{repeat.payload_sha256} != {native.payload_sha256}"
                )
            repeat_reports.append(_waveform_report(repeat, repeat_stats))

        kkaot_report = validate_kkaot(args.kkaot) if args.kkaot is not None else None
        perturbation_report = (
            _controlled_perturbation(reference, reference_stats, thresholds)
            if args.controlled_perturbation
            else None
        )
    except VerificationError as error:
        if args.json:
            print(json.dumps({"pass": False, "error": str(error)}, indent=2))
        else:
            print(f"kokoro-waveform-parity: FAIL: {error}")
        return 1

    report: dict[str, object] = {
        "pass": not failures,
        "contract": contract() if args.print_contract else None,
        "thresholds": _serializable_dataclass(thresholds),
        "reference": _waveform_report(reference, reference_stats),
        "native": _waveform_report(native, native_stats),
        "native_repeats": repeat_reports,
        "metrics": _serializable_dataclass(metrics),
        "kkaot": kkaot_report,
        "controlled_perturbation": perturbation_report,
        "failures": failures,
    }
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True, ensure_ascii=False))
    else:
        status = "PASS" if not failures else "FAIL"
        snr = "inf" if math.isinf(metrics.snr_db) else f"{metrics.snr_db:.6f}"
        print(
            f"kokoro-waveform-parity: {status} "
            f"F={native_stats.decoder_frames} samples={native_stats.count} "
            f"native_sha256={native.payload_sha256}"
        )
        print(
            "  metrics: "
            f"max_abs={metrics.max_abs_error:.9g} rmse={metrics.rmse:.9g} "
            f"correlation={metrics.correlation:.9g} snr_db={snr} "
            f"mean_error={metrics.mean_error:.9g}"
        )
        print(
            f"  native: finite=yes peak={native_stats.peak:.9g} "
            f"rms={native_stats.rms:.9g} repeats={len(repeat_reports)}"
        )
        if kkaot_report is not None:
            print(
                f"  KKAOT: bytes={kkaot_report['bytes']} "
                f"sha256={kkaot_report['file_sha256']}"
            )
        if perturbation_report is not None:
            print("  controlled polarity perturbation: REJECTED")
        for failure in failures:
            print(f"  gate: {failure}")
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
