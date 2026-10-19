#!/usr/bin/env python3
"""Numerical/topological checks for the independent Dista host oracle."""

import math
import unittest

import audit


class DistaAuditTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.seeds = audit.load_seeds()
        cls.parameters = audit.load_parameters()
        cls.offsets = audit.load_shader_offsets()

    def test_current_sparse_overrides_and_authored_basic_shapes(self):
        self.assertEqual(len(self.seeds), 128)
        self.assertEqual(self.seeds["/"].sites(), [(4, 0), (3, 2), (2, 4), (1, 6), (0, 8)])
        self.assertEqual(self.seeds["y"].sites(), [(0, 4), (4, 4), (0, 6), (4, 6), (1, 7), (3, 7), (2, 8), (1, 10)])
        self.assertTrue(self.seeds["y"].long_reach)
        self.assertFalse(self.seeds["A"].long_reach)
        self.assertEqual(self.seeds["A"].sites()[0], (2, 0))
        self.assertEqual(len(self.seeds["A"].sites()), 20)
        for char, pixels, sites in [("1", 0x6282082082082000, 11),
                                    ("A", 0x85228BE8A28A2000, 20),
                                    ("Y", 0x228A2894208208000, 14)]:
            self.assertEqual(self.seeds[char].pixels, pixels)
            self.assertEqual(len(self.seeds[char].sites()), sites)
            self.assertFalse(self.seeds[char].long_reach)

    def test_all_printable_seed_graphs_match_full_closed_disc_oracle(self):
        for code in range(33, 127):
            with self.subTest(glyph=chr(code)):
                seed = self.seeds[chr(code)]
                links = audit.canonical_links(seed, self.parameters)
                self.assertEqual(links, audit.shader_links(seed, self.parameters, self.offsets))
                for a, b in links:
                    self.assertEqual(audit.link_weight(a, b, self.parameters.reach(seed)), 1.0)

    def test_pruning_includes_all_diameter_ties_for_every_offset(self):
        # Search well beyond the shader's small box. Cardinal length-two links
        # specifically have boundary blockers outside the endpoint rectangle.
        for dx, dy in self.offsets:
            expected = {
                (x, y) for x in range(-6, 7) for y in range(-6, 7)
                if (x, y) not in [(0, 0), (dx, dy)]
                and (2 * x - dx) ** 2 + (2 * y - dy) ** 2 <= dx * dx + dy * dy
            }
            self.assertEqual(expected, audit.shader_blockers((dx, dy)))
        self.assertEqual(audit.shader_blockers((2, 0)), {(1, -1), (1, 0), (1, 1)})

    def test_lattice_candidates_cover_every_possible_admitted_displacement(self):
        for reach in [self.parameters.short_reach, self.parameters.long_reach]:
            expected = {
                (x, y) for x in range(-5, 6) for y in range(0, 11)
                if (y > 0 or x > 0) and x * x + y * y < reach**2
            }
            actual = {(x, y) for x, y in self.offsets if x * x + y * y < reach**2}
            self.assertEqual(expected, actual)

    def test_quality_bound_and_segment_counts(self):
        for error, expected in [(0.25, (12, 8)), (0.125, (16, 12)), (0.0625, (24, 16))]:
            result = audit.audit(audit.DEMO, error)
            for name, segments in zip(["Meso", "Lean"], expected):
                shape = result["shapes"][name]
                self.assertEqual(shape["circle_segments"], segments)
                self.assertLessEqual(shape["sagitta_px"], error)
                self.assertAlmostEqual(
                    audit.compact_gaussian(shape["radius_source"] ** 2, self.parameters.sigma),
                    shape["threshold"], places=14,
                )
        for invalid in [0, -1, math.inf, math.nan]:
            with self.assertRaises(ValueError):
                audit.circle_segments(5.0, invalid)

    def test_demo_counts_distinguish_surviving_and_submitted_triangles(self):
        result = audit.audit(audit.DEMO)
        self.assertEqual((result["sites"], result["links"]), (145, 138))
        self.assertEqual(result["active_triangles"], 4612)
        self.assertEqual(result["submitted_triangles"], 6692)
        self.assertEqual(result["shapes"]["Meso"]["active_triangles"], 2596)
        self.assertEqual(result["shapes"]["Lean"]["active_triangles"], 2016)

    def test_current_full_basic_counts_match_independent_graph_oracle(self):
        result = audit.audit("".join(chr(code) for code in range(0x20, 0x7f)))
        self.assertEqual((result["sites"], result["links"]), (1303, 1244))
        self.assertEqual(result["shapes"]["Meso"]["active_triangles"], 23336)
        self.assertEqual(result["shapes"]["Lean"]["active_triangles"], 18124)

    def test_analytic_union_matches_original_gaussian_field(self):
        for char in audit.DEMO:
            with self.subTest(glyph=char):
                result = audit.raster_compare(self.seeds[char], self.parameters, self.offsets, 0.25)
                self.assertEqual(result["classification_mismatches"], {"Meso": 0, "Lean": 0})
                self.assertEqual(result["samples_per_shape"], 1344)


if __name__ == "__main__":
    unittest.main()
