import unittest

import numpy as np

from lilly_film.pipeline import prediction_metrics


class MetricTests(unittest.TestCase):
    def test_identical_frame_scores_are_perfect(self):
        frame = np.zeros((128, 128, 4), dtype=np.uint8)
        frame[32:96, 40:88] = (20, 30, 40, 255)
        metrics = prediction_metrics(frame, frame)
        self.assertEqual(metrics["alpha_iou"], 1.0)
        self.assertEqual(metrics["alpha_area_ratio"], 1.0)
        self.assertEqual(metrics["edge_f1_with_1px_tolerance"], 1.0)
        self.assertEqual(metrics["rgb_mae_on_shared_opaque"], 0.0)
        self.assertEqual(metrics["rgba_exact_fraction"], 1.0)

    def test_missing_sprite_scores_zero_iou(self):
        prediction = np.zeros((128, 128, 4), dtype=np.uint8)
        target = prediction.copy()
        target[32:96, 40:88] = (20, 30, 40, 255)
        metrics = prediction_metrics(prediction, target)
        self.assertEqual(metrics["alpha_iou"], 0.0)
        self.assertEqual(metrics["alpha_area_ratio"], 0.0)


if __name__ == "__main__":
    unittest.main()

