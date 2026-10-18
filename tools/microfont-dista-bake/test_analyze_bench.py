#!/usr/bin/env python3
"""Check experiment statistics and prevent misleading timing substitutions."""
import json
import unittest

import analyze_bench as bench


def sample(index, wall, **extra):
    return {"kind":"sample", "variant":"loop", "quality":"Fine", "shape":"Meso",
            "sample":index, "wall_ns":wall, **extra}


class AnalyzerTests(unittest.TestCase):
    def test_first_submission_is_not_in_warm_distribution(self):
        rows = [sample(0, 1000, first_rgba_ready_ns=1500)]
        rows += [sample(i, value) for i, value in enumerate([10, 20, 30, 40, 50, 60, 70], 1)]
        group = bench.summarize(rows)["groups"][0]
        self.assertEqual(group["first_count"], 1)
        self.assertEqual(group["wall_ns"]["first"]["median_ns"], 1000)
        self.assertEqual(group["first_rgba_ready_ns"]["first"]["median_ns"], 1500)
        self.assertEqual(group["wall_ns"]["warm"]["median_ns"], 40)
        self.assertEqual(group["wall_ns"]["warm"]["p95_ns"], 70)

    def test_missing_gpu_values_remain_missing(self):
        report = bench.summarize([sample(0, 100, gpu_draw_ns=None), sample(1, 90)])
        gpu = report["groups"][0]["gpu_draw_ns"]
        self.assertEqual(gpu["first"]["count"], 0)
        self.assertIsNone(gpu["warm"]["median_ns"])
        self.assertEqual(report["groups"][0]["wall_ns"]["warm"]["median_ns"], 90)

    def test_variants_shapes_and_qualities_stay_separate(self):
        rows = [sample(1, 10), sample(1, 20, variant="direct"),
                sample(1, 30, shape="Lean"), sample(1, 40, quality="High")]
        report = bench.summarize(rows)
        self.assertEqual(len(report["groups"]), 4)
        self.assertEqual(sum(g["warm_count"] for g in report["groups"]), 4)

    def test_formats_and_stages_stay_separate_and_report_their_buffer_sizes(self):
        rows = [sample(1, 10, destination_bytes=6488064, destination_pitch_bytes=6144),
                sample(1, 20, output_format="r8", destination_bytes=1622016,
                       destination_pitch_bytes=1536, gpu_target_ready_ns=15),
                sample(1, 30, stage="retained-copy")]
        groups = bench.summarize(rows)["groups"]
        self.assertEqual(len(groups), 3)
        r8 = next(g for g in groups if g["output_format"] == "r8")
        self.assertEqual(r8["destination_bytes"], [1622016])
        self.assertEqual(r8["destination_pitch_bytes"], [1536])
        self.assertEqual(r8["gpu_target_ready_ns"]["warm"]["median_ns"], 15)
        self.assertNotIn("gpu_rgba_ready_ns", r8)

    def test_old_rgba_names_and_default_format_remain_compatible(self):
        group = bench.summarize([sample(0, 20, gpu_rgba_ready_ns=10, first_rgba_ready_ns=30)])["groups"][0]
        self.assertEqual((group["stage"], group["output_format"]), ("atlas-rebuild", "rgba8"))
        self.assertEqual(group["gpu_target_ready_ns"]["first"]["median_ns"], 10)
        self.assertEqual(group["first_target_ready_ns"]["first"]["median_ns"], 30)

    def test_canonical_target_names_and_validation_are_preserved(self):
        rows = [sample(0, 20, output_format="r8", gpu_target_ready_ns=10, first_target_ready_ns=30),
                {"kind":"validation", "passed":False, "mismatched_pixels":1}]
        report = bench.summarize(rows)
        self.assertFalse(report["validation"][0]["passed"])
        self.assertIn("0/1 pairs passed", bench.markdown(report))
        for extra in [{"output_format":"rgb16"}, {"stage":False}, {"destination_bytes":True},
                      {"destination_pitch_bytes":0}, {"gpu_target_ready_ns":-1}]:
            self.assertEqual(len(bench.summarize([sample(1, 10, **extra)])["invalid_samples"]), 1)

    def test_prefixed_json_and_invalid_records(self):
        lines = ["noise", "[global] microfont-dista-bench: " + json.dumps(sample(1, 10)),
                 "microfont-dista-bench: {bad"]
        rows, bad = bench.read_records(lines)
        self.assertEqual(rows, [sample(1, 10)])
        self.assertEqual(bad[0]["line"], 3)
        for invalid in [sample(True, 10), sample(1, -1), sample(1, 10, gpu_draw_ns=float("nan"))]:
            self.assertEqual(len(bench.summarize([invalid])["invalid_samples"]), 1)

    def test_mixed_builds_and_errors_are_preserved(self):
        rows = [{"kind":"begin", "source_sha256":"first"},
                {"kind":"begin", "source_sha256":"second"},
                sample(1, 10), {"kind":"error", "reason":"render-busy"}]
        report = bench.summarize(rows)
        self.assertTrue(report["mixed_builds"])
        self.assertEqual(len(report["begin"]), 2)
        self.assertEqual(report["errors"][0]["reason"], "render-busy")
        self.assertIn("Mixed shader builds", bench.markdown(report))


if __name__ == "__main__":
    unittest.main()
