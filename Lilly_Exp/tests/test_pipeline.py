from __future__ import annotations

import unittest

import numpy as np

from lilly_exp.pipeline import extend_opaque_rgb


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


if __name__ == "__main__":
    unittest.main()

