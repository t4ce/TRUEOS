from __future__ import annotations

import unittest

import numpy as np

from lilly_exp.pipeline import (
    _passes_quality_gate,
    _prediction_metrics,
    extend_opaque_rgb,
)


class EdgeExtensionTests(unittest.TestCase):
    def test_transparent_rgb_is_filled_from_opaque_pixels(self) -> None:
        rgba = np.zeros((5, 5, 4), dtype=np.uint8)
        rgba[2, 2] = (20, 40, 60, 255)
        filled = extend_opaque_rgb(rgba)
        self.assertTrue(np.all(filled == np.array([20, 40, 60], dtype=np.uint8)))

    def test_opaque_rgb_is_unchanged(self) -> None:
        rgba = np.zeros((3, 3, 4), dtype=np.uint8)
        rgba[1, 1] = (1, 2, 3, 255)
        filled = extend_opaque_rgb(rgba)
        self.assertEqual(filled[1, 1].tolist(), [1, 2, 3])


class MetricTests(unittest.TestCase):
    def test_identical_frame_passes_quality_gate(self) -> None:
        rgba = np.zeros((8, 8, 4), dtype=np.uint8)
        rgba[2:6, 2:6] = (10, 20, 30, 255)
        metrics = _prediction_metrics(rgba, rgba)
        self.assertEqual(metrics["alpha_iou"], 1.0)
        self.assertEqual(metrics["edge_f1_with_1px_tolerance"], 1.0)
        self.assertTrue(_passes_quality_gate(metrics))

    def test_missing_sprite_fails_quality_gate(self) -> None:
        prediction = np.zeros((8, 8, 4), dtype=np.uint8)
        target = np.zeros((8, 8, 4), dtype=np.uint8)
        target[2:6, 2:6] = (10, 20, 30, 255)
        self.assertFalse(_passes_quality_gate(_prediction_metrics(prediction, target)))


if __name__ == "__main__":
    unittest.main()
