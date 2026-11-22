"""Geometry tests for circular runs and their emitted native line vertices."""

from math import cos, hypot, pi, sin, tau
import unittest

from shapely import hausdorff_distance
from shapely.geometry import LineString, Point, Polygon

from circular_fit import fold_circular
from fill_geometry import Glyph, Paint
from reduce import reduce_glyph


TOLERANCE = 0.25


def arc_points(center, radius, start, sweep, segments):
    angles = (start + sweep * i / segments for i in range(segments + 1))
    return [
        (center[0] + radius * cos(angle), center[1] + radius * sin(angle))
        for angle in angles
    ]


def line_string(points, closed):
    if closed and points[0] != points[-1]:
        points = [*points, points[0]]
    return LineString(points)


class CircularFitTests(unittest.TestCase):
    def fold(self, source, closed):
        result = fold_circular(source, closed, TOLERANCE)
        self.assertIsNotNone(result)
        vertices, descriptions = result
        self.assertLessEqual(
            hausdorff_distance(
                line_string(source, closed), line_string(vertices, closed), densify=0.25
            ),
            TOLERANCE,
        )
        return vertices, descriptions

    def assert_arc_vertices(self, vertices, description):
        samples = arc_points(
            description["center"],
            description["radius"],
            description["start_angle"],
            description["sweep"],
            description["segment_count"],
        )
        for offset, point in enumerate(samples):
            index = (description["first_vertex"] + offset) % len(vertices)
            actual = vertices[index]
            self.assertLess(hypot(actual[0] - point[0], actual[1] - point[1]), 1e-5)

    def test_16_pixel_circle_takes_13_segments_with_either_winding(self):
        for sweep in (tau, -tau):
            with self.subTest(sweep=sweep):
                source = arc_points((8.5, 8.5), 8, 0, sweep, 256)[:-1]
                # The reader can supply an explicitly repeated closing point.
                vertices, descriptions = self.fold(source + source[:1], True)
                self.assertEqual(len(vertices), 13)
                self.assertNotEqual(vertices[0], vertices[-1])
                self.assertEqual(len(descriptions), 1)
                circle = descriptions[0]
                self.assertEqual(circle["kind"], "circle")
                self.assertEqual(circle["segment_count"], 13)
                self.assertAlmostEqual(circle["radius"], 8)
                self.assertAlmostEqual(circle["sweep"], sweep)
                self.assert_arc_vertices(vertices, circle)

    def test_open_arc_preserves_endpoints_across_angle_wrap(self):
        source = arc_points((10, 10), 8, 3 * pi / 4, 2 * pi / 3, 80)
        vertices, descriptions = self.fold(source, False)
        self.assertEqual(tuple(vertices[0]), source[0])
        self.assertEqual(tuple(vertices[-1]), source[-1])
        self.assertEqual(len(descriptions), 1)
        arc = descriptions[0]
        self.assertEqual(arc["kind"], "arc")
        self.assertEqual(arc["segment_count"], 5)
        self.assertAlmostEqual(arc["sweep"], 2 * pi / 3)
        self.assert_arc_vertices(vertices, arc)

    def test_small_open_arc_can_use_one_chord(self):
        source = arc_points((4, 4), 2, 0, pi / 4, 24)
        vertices, descriptions = self.fold(source, False)
        self.assertEqual(list(map(tuple, vertices)), [source[0], source[-1]])
        self.assertEqual(len(descriptions), 1)
        self.assertEqual(descriptions[0]["segment_count"], 1)
        self.assert_arc_vertices(vertices, descriptions[0])

    def test_mixed_strip_folds_only_its_circular_run(self):
        source = [
            (1, 3),
            (5, 3),
            *arc_points((10, 8), 5, -pi / 2, pi / 2, 64),
            (15, 12),
            (15, 16),
        ]
        vertices, descriptions = self.fold(source, False)
        self.assertLess(len(vertices), len(source))
        self.assertEqual(tuple(vertices[0]), source[0])
        self.assertEqual(tuple(vertices[-1]), source[-1])
        self.assertTrue(descriptions)
        for arc in descriptions:
            self.assertEqual(arc["kind"], "arc")
            self.assertGreater(arc["first_vertex"], 0)
            self.assertLess(arc["first_vertex"] + arc["segment_count"], len(vertices) - 1)
            self.assert_arc_vertices(vertices, arc)

    def test_straight_polyline_has_no_circular_run(self):
        self.assertIsNone(fold_circular([(x, x / 2) for x in range(17)], False, TOLERANCE))

    def test_square_corners_do_not_make_a_circle(self):
        square = [(1, 1), (16, 1), (16, 16), (1, 16)]
        self.assertIsNone(fold_circular(square, True, TOLERANCE))

    def test_reducer_preserves_the_circular_hole_of_a_ring(self):
        center = Point(8.5, 8.5)
        ring = center.buffer(8, quad_segs=64).difference(center.buffer(4, quad_segs=64))
        result = reduce_glyph(Glyph([Paint(ring, (0, 0, 0), 1.0)], 512), 17, TOLERANCE, 3.0)
        self.assertEqual(result["dots"], 0)
        self.assertEqual(result["strips"], 2)
        strokes = {stroke["role"]: stroke for stroke in result["strokes"]}
        self.assertEqual(set(strokes), {"silhouette", "hole"})
        for stroke in strokes.values():
            self.assertTrue(stroke["closed"])
            self.assertEqual([part["kind"] for part in stroke["circular_parts"]], ["circle"])
            self.assertEqual(len(stroke["vertices"]), stroke["circular_parts"][0]["segment_count"])
        contour = Polygon(strokes["silhouette"]["vertices"], [strokes["hole"]["vertices"]])
        self.assertTrue(contour.is_valid)
        self.assertFalse(contour.covers(center))


if __name__ == "__main__":
    unittest.main()
