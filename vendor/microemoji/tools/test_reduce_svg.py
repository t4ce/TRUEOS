"""Geometry checks for the offline SVG stroke extraction pass."""

from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from shapely.geometry import LineString, Point, box

from fill_geometry import Glyph, Paint
from reduce import make_strip, reduce_glyph
from svg_geometry import read_svg


SIZE = 17
BLACK = (0, 0, 0)
WHITE = (255, 255, 255)
SQUARE = box(1, 1, 16, 16)


def reduce(*paints):
    return reduce_glyph(Glyph(list(paints), 16), SIZE, 0.25, 3.0)


def read_path(commands):
    with TemporaryDirectory() as directory:
        svg = Path(directory) / "u0000.svg"
        svg.write_text(
            '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"'
            ' viewBox="0 -100 100 100">'
            '<g transform="scale(1 -1)" fill-rule="nonzero" fill-opacity="1" stroke="none">'
            f'<path fill="#000000" d="{commands}"/></g></svg>',
            encoding="utf-8",
        )
        return read_svg(svg, SIZE)


class StripTests(unittest.TestCase):
    def test_open_strip_repeats_endpoints_for_adjacency(self):
        vertices = [(1, 1), (2, 3), (4, 4)]
        strip = make_strip(vertices, False, SIZE, role="test")
        self.assertEqual(strip["adjacency_indices"], [0, 0, 1, 2, 2])

    def test_closed_strip_wraps_adjacency_and_closes_once(self):
        vertices = [(1, 1), (1, 4), (4, 4), (4, 1)]
        strip = make_strip(vertices, True, SIZE, role="test")
        self.assertEqual(strip["adjacency_indices"], [3, 0, 1, 2, 3, 0, 1])


class ReduceTests(unittest.TestCase):
    def assert_outer_square(self, result):
        self.assertEqual(result["strips"], 1)
        self.assertEqual(result["dots"], 0)
        self.assertEqual(len(result["strokes"]), 1)
        strip = result["strokes"][0]
        self.assertTrue(strip["closed"])
        self.assertEqual(set(map(tuple, strip["vertices"])), set(SQUARE.exterior.coords))

    def test_opposite_winding_preserves_hole(self):
        glyph = read_path("M0 0L100 0L100 100L0 100Z M25 25L25 75L75 75L75 25Z")
        self.assertFalse(glyph.paints[0].geometry.covers(Point(8.5, 8.5)))
        result = reduce_glyph(glyph, SIZE, 0.25, 3.0)
        self.assertEqual(result["strips"], 2)
        self.assertTrue(all(stroke["closed"] for stroke in result["strokes"]))

    def test_same_winding_fills_nested_contour(self):
        glyph = read_path("M0 0L100 0L100 100L0 100Z M25 25L75 25L75 75L25 75Z")
        self.assertTrue(glyph.paints[0].geometry.covers(Point(8.5, 8.5)))
        result = reduce_glyph(glyph, SIZE, 0.25, 3.0)
        self.assertEqual(result["strips"], 1)

    def test_fully_covered_contrast_edge_disappears(self):
        hidden = Paint(box(4, 4, 7, 7), BLACK, 1.0)
        visible = Paint(SQUARE, WHITE, 1.0)
        self.assert_outer_square(reduce(hidden, visible))

    def test_adjacent_equal_paint_has_one_outline(self):
        left = Paint(box(1, 1, 8, 16), BLACK, 1.0)
        right = Paint(box(8, 1, 16, 16), BLACK, 1.0)
        self.assert_outer_square(reduce(left, right))

    def test_low_contrast_region_contributes_no_stroke(self):
        base = Paint(SQUARE, (128, 128, 128), 1.0)
        feature = Paint(box(4, 4, 7, 7), (129, 128, 128), 1.0)
        self.assert_outer_square(reduce(base, feature))

    def test_contrast_edge_attached_to_outline_becomes_open_strip(self):
        left = Paint(box(1, 1, 8, 16), BLACK, 1.0)
        right = Paint(box(8, 1, 16, 16), WHITE, 1.0)
        result = reduce(left, right)
        self.assertEqual(result["strips"], 2)
        seam = next(stroke for stroke in result["strokes"] if not stroke["closed"])
        self.assertEqual(set(map(tuple, seam["vertices"])), {(8, 1), (8, 16)})

    def test_small_contrasting_disk_becomes_dot(self):
        center = Point(8.5, 8.5)
        for background, foreground in [(WHITE, BLACK), (BLACK, WHITE)]:
            with self.subTest(background=background):
                base = Paint(SQUARE, background, 1.0)
                feature = Paint(center.buffer(0.8), foreground, 1.0)
                result = reduce(base, feature)
                self.assertEqual(result["strips"], 1)
                self.assertEqual(result["dots"], 1)
                dot = next(stroke for stroke in result["strokes"] if stroke["kind"] == "dot")
                self.assertLess(Point(dot["center"]).distance(center), 0.1)

    def test_narrow_contrasting_band_becomes_open_strip(self):
        base = Paint(SQUARE, WHITE, 1.0)
        feature = Paint(LineString([(4, 5), (11, 12)]).buffer(0.45), BLACK, 1.0)
        result = reduce(base, feature)
        self.assertEqual(result["strips"], 2)
        self.assertEqual(result["dots"], 0)
        strip = next(stroke for stroke in result["strokes"] if not stroke["closed"])
        self.assertEqual(strip["adjacency_indices"][0:2], [0, 0])
        self.assertEqual(strip["adjacency_indices"][-1], len(strip["vertices"]) - 1)

    def test_translucent_region_is_flagged(self):
        result = reduce(Paint(SQUARE, WHITE, 1.0), Paint(box(4, 4, 7, 7), BLACK, 0.5))
        self.assertEqual(result["status"], "unsupported")
        self.assertEqual(result["strokes"], [])
        self.assertTrue(result["review"])

    def test_transparent_paint_contributes_no_outline(self):
        transparent = Paint(box(0.5, 0.5, 16.5, 16.5), BLACK, 0.0)
        self.assert_outer_square(reduce(Paint(SQUARE, WHITE, 1.0), transparent))


if __name__ == "__main__":
    unittest.main()
