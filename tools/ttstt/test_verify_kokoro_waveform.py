#!/usr/bin/env python3
"""Unit tests for the host-only Kokoro waveform parity gate."""

from __future__ import annotations

import hashlib
import importlib.util
import math
from pathlib import Path
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
TOOL_PATH = ROOT / "tools/ttstt/verify_kokoro_waveform.py"
SPEC = importlib.util.spec_from_file_location("verify_kokoro_waveform", TOOL_PATH)
assert SPEC is not None and SPEC.loader is not None
TOOL = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = TOOL
SPEC.loader.exec_module(TOOL)


class KokoroWaveformParityTests(unittest.TestCase):
    def test_pinned_input_and_output_contract_is_self_consistent(self) -> None:
        self.assertEqual(len(TOOL.REFERENCE_IPA), 149)
        self.assertEqual(
            hashlib.sha256(TOOL.REFERENCE_IPA.encode("utf-8")).hexdigest(),
            TOOL.REFERENCE_IPA_SHA256,
        )
        self.assertEqual(TOOL.REFERENCE_TOKEN_COUNT, TOOL.REFERENCE_STYLE_ROW)
        self.assertEqual(
            TOOL.REFERENCE_PADDED_TOKEN_COUNT,
            TOOL.REFERENCE_TOKEN_COUNT + 2,
        )
        self.assertEqual(
            TOOL.EXPECTED_SAMPLE_COUNT,
            TOOL.EXPECTED_DECODER_FRAMES * TOOL.SAMPLES_PER_DECODER_FRAME,
        )
        self.assertEqual(
            TOOL.NATIVE_ACCEPTED_SAMPLE_COUNT,
            TOOL.NATIVE_ACCEPTED_DECODER_FRAMES
            * TOOL.NATIVE_ACCEPTED_SAMPLES_PER_FRAME,
        )

    def test_native_whisper_transcript_gate_is_word_exact(self) -> None:
        TOOL.validate_native_transcript(TOOL.NATIVE_ACCEPTED_TRANSCRIPT)
        TOOL.validate_native_transcript(
            "HELLO from True OS! The quick brown fox jumps over the lazy dog; "
            "spitch synthesis is now running in the kernel with a serialized "
            "async queue for the shell."
        )
        with self.assertRaises(TOOL.VerificationError):
            TOOL.validate_native_transcript("unintelligible output")

    def test_exact_waveform_has_perfect_metrics(self) -> None:
        samples = [0.0, 0.125, -0.25, 0.5, -0.375]
        metrics = TOOL.compute_metrics(samples, samples)
        self.assertEqual(metrics.max_abs_error, 0.0)
        self.assertEqual(metrics.rmse, 0.0)
        self.assertEqual(metrics.correlation, 1.0)
        self.assertTrue(math.isinf(metrics.snr_db) and metrics.snr_db > 0.0)

    def test_controlled_in_range_polarity_perturbation_is_rejected(self) -> None:
        reference = [0.125, -0.25, 0.5, -0.375]
        perturbed = [-value for value in reference]
        metrics = TOOL.compute_metrics(reference, perturbed)
        stats = TOOL.SampleStats(
            count=len(perturbed),
            decoder_frames=1,
            minimum=min(perturbed),
            maximum=max(perturbed),
            peak=max(abs(value) for value in perturbed),
            mean=sum(perturbed) / len(perturbed),
            rms=math.sqrt(sum(value * value for value in perturbed) / len(perturbed)),
        )
        thresholds = TOOL.Thresholds(
            TOOL.DEFAULT_MIN_CORRELATION,
            TOOL.DEFAULT_MIN_SNR_DB,
            TOOL.DEFAULT_MAX_RMSE,
            TOOL.DEFAULT_MAX_ABS_ERROR,
        )
        failures = TOOL.quality_failures(metrics, stats, thresholds)
        self.assertLess(metrics.correlation, 0.0)
        self.assertTrue(failures)


if __name__ == "__main__":
    unittest.main()
