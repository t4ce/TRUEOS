from __future__ import annotations

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

import numpy as np

from lilly_exp.pipeline import (
    _composite_face_only,
    _face_region_mask,
    _passes_quality_gate,
    _prediction_metrics,
    discover_frame_sets,
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


class FaceOnlyTests(unittest.TestCase):
    def test_only_opaque_inner_face_rgb_can_change(self) -> None:
        carrier = np.zeros((128, 128, 4), dtype=np.uint8)
        carrier[20:110, 20:110] = (10, 20, 30, 255)
        generated = np.full((128, 128, 4), (90, 80, 70, 255), dtype=np.uint8)

        result, face_mask = _composite_face_only(generated, carrier)

        self.assertTrue(np.array_equal(result[~face_mask], carrier[~face_mask]))
        self.assertTrue(np.array_equal(result[:, :, 3], carrier[:, :, 3]))
        self.assertTrue(np.all(result[face_mask, :3] == (90, 80, 70)))

    def test_face_mask_is_conservative_and_canonical(self) -> None:
        mask = _face_region_mask(128, 128)
        y, x = np.nonzero(mask)
        self.assertGreater(len(x), 800)
        self.assertGreaterEqual(int(x.min()), 44)
        self.assertLessEqual(int(x.max()), 84)
        self.assertGreaterEqual(int(y.min()), 43)
        self.assertLessEqual(int(y.max()), 69)


class LibraryDiscoveryTests(unittest.TestCase):
    def test_discovers_only_exact_four_frame_sets(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame_dir = root / "Waving" / "wave_frames"
            frame_dir.mkdir(parents=True)
            for index in range(1, 5):
                (frame_dir / f"frame_{index:02d}.png").touch()

            self.assertEqual(discover_frame_sets(root), [frame_dir])

    def test_rejects_noncanonical_frame_names(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame_dir = root / "broken_frames"
            frame_dir.mkdir()
            for index in (1, 2, 3, 5):
                (frame_dir / f"frame_{index:02d}.png").touch()

            with self.assertRaises(ValueError):
                discover_frame_sets(root)

    def test_accepts_existing_seven_frame_refresh_layout(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame_dir = root / "wave_frames"
            frame_dir.mkdir()
            for index in range(1, 8):
                (frame_dir / f"frame_{index:02d}.png").touch()

            self.assertEqual(discover_frame_sets(root), [frame_dir])


if __name__ == "__main__":
    unittest.main()
