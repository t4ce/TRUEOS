from __future__ import annotations

import random
import tempfile
import unittest
from pathlib import Path

from cloud_verify import BUILD_ROOT, validate_output_root, verify_ir
from semantic import (
    RADIX_BINS,
    SUBGROUP_WIDTH,
    TILE_ITEMS,
    U32_MASK,
    collective_probe_report,
    exclusive_scan_u32,
    histogram_16_u32,
    normalize_flags_u32,
    radix_sort_u32,
    reduce_sum_u32,
    rle_u32,
    scan_plan,
    segmented_exclusive_scan_u32,
    segmented_reduce_sum_u32,
    select_indices_u32,
    tiled_exclusive_scan_u32,
    tiled_reduce_sum_u32,
    tiled_segmented_exclusive_scan_u32,
    u32,
)


class CloudProofTests(unittest.TestCase):
    def test_output_root_is_confined_to_repository_build_tree(self) -> None:
        admitted = BUILD_ROOT / "intel-gpu-primitives-unit-test"
        self.assertEqual(validate_output_root(admitted), admitted.resolve())
        with self.assertRaises(RuntimeError):
            validate_output_root(BUILD_ROOT)
        with self.assertRaises(RuntimeError):
            validate_output_root(BUILD_ROOT.parent)

    def test_ir_gate_rejects_non_simd16_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            bad_ir = Path(directory) / "bad.ll"
            bad_ir.write_text(
                "define dso_local spir_kernel void @probe() "
                "!intel_reqd_sub_group_size !1 {\n"
                "  ret void\n"
                "}\n"
                "!1 = !{i32 8}\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RuntimeError, "expected 16"):
                verify_ir(bad_ir, ["probe"])

            good_ir = Path(directory) / "good.ll"
            good_ir.write_text(
                "define dso_local spir_kernel void @probe() "
                "!intel_reqd_sub_group_size !1 {\n"
                "  ret void\n"
                "}\n"
                "!1 = !{i32 16}\n",
                encoding="utf-8",
            )
            verify_ir(good_ir, ["probe"])


class CollectiveProbeTests(unittest.TestCase):
    def test_report_shape_and_collectives(self) -> None:
        values = [u32((lane + 1) * 0x1020_3041) for lane in range(SUBGROUP_WIDTH)]
        report = collective_probe_report(values)
        exclusive = exclusive_scan_u32(values)
        inclusive = [u32(prefix + value) for prefix, value in zip(exclusive, values)]
        self.assertEqual(len(report), 9 * SUBGROUP_WIDTH)
        self.assertEqual(report[:SUBGROUP_WIDTH], values)
        self.assertEqual(
            report[SUBGROUP_WIDTH : 2 * SUBGROUP_WIDTH],
            exclusive,
        )
        self.assertEqual(
            report[2 * SUBGROUP_WIDTH : 3 * SUBGROUP_WIDTH],
            inclusive,
        )
        self.assertEqual(
            report[3 * SUBGROUP_WIDTH : 4 * SUBGROUP_WIDTH],
            [reduce_sum_u32(values)] * SUBGROUP_WIDTH,
        )
        self.assertEqual(
            report[4 * SUBGROUP_WIDTH : 5 * SUBGROUP_WIDTH],
            [min(values)] * SUBGROUP_WIDTH,
        )
        self.assertEqual(
            report[5 * SUBGROUP_WIDTH : 6 * SUBGROUP_WIDTH],
            [max(values)] * SUBGROUP_WIDTH,
        )
        self.assertEqual(
            report[6 * SUBGROUP_WIDTH : 7 * SUBGROUP_WIDTH],
            [values[0]] * SUBGROUP_WIDTH,
        )
        self.assertEqual(
            report[7 * SUBGROUP_WIDTH : 8 * SUBGROUP_WIDTH],
            list(reversed(values)),
        )
        self.assertEqual(
            report[8 * SUBGROUP_WIDTH :],
            [(SUBGROUP_WIDTH << 16) | lane for lane in range(SUBGROUP_WIDTH)],
        )


class ScanAndSelectionTests(unittest.TestCase):
    def test_boundary_lengths_and_overflow(self) -> None:
        randomizer = random.Random(0x5452_5545)
        for length in (0, 1, 15, 16, 17, 255, 256, 257, 511, 512, 513, 4097):
            values = [randomizer.getrandbits(32) for _ in range(length)]
            with self.subTest(length=length):
                self.assertEqual(
                    tiled_exclusive_scan_u32(values), exclusive_scan_u32(values)
                )
                self.assertEqual(tiled_reduce_sum_u32(values), reduce_sum_u32(values))

    def test_explicit_modular_overflow(self) -> None:
        values = [U32_MASK, 2, U32_MASK, 7]
        self.assertEqual(tiled_exclusive_scan_u32(values), [0, U32_MASK, 1, 0])
        self.assertEqual(tiled_reduce_sum_u32(values), 7)

    def test_stable_selection(self) -> None:
        flags = [0, 7, 0, 1, 3, 0, 0, U32_MASK]
        self.assertEqual(normalize_flags_u32(flags), [0, 1, 0, 1, 1, 0, 0, 1])
        self.assertEqual(select_indices_u32(flags), [1, 3, 4, 7])

    def test_recursive_plan(self) -> None:
        self.assertEqual(scan_plan(0).levels, ())
        self.assertEqual(scan_plan(1).levels, ())
        plan = scan_plan(TILE_ITEMS * TILE_ITEMS + 1)
        self.assertEqual(plan.levels[0].tile_count, TILE_ITEMS + 1)
        self.assertEqual(plan.levels[1].tile_count, 2)
        self.assertEqual(plan.levels[2].tile_count, 1)
        self.assertGreater(plan.temporary_words, 0)


class RadixAndHistogramTests(unittest.TestCase):
    def test_histogram(self) -> None:
        values = [0, 1, 15, 16, 17, 31, U32_MASK]
        histogram = histogram_16_u32(values, 0)
        self.assertEqual(sum(histogram), len(values))
        self.assertEqual(histogram[0], 2)
        self.assertEqual(histogram[1], 2)
        self.assertEqual(histogram[15], 3)
        self.assertEqual(len(histogram), RADIX_BINS)

    def test_stable_key_value_radix_sort(self) -> None:
        keys = [7, 3, 7, 1, 3, 7, 0, U32_MASK, 0]
        values = list(range(len(keys)))
        sorted_keys, sorted_values = radix_sort_u32(keys, values)
        expected = sorted(enumerate(keys), key=lambda pair: pair[1])
        self.assertEqual(sorted_keys, [key for _, key in expected])
        self.assertEqual(sorted_values, [index for index, _ in expected])

    def test_random_radix_sort_boundaries(self) -> None:
        randomizer = random.Random(0x5041_5241)
        for length in (0, 1, 15, 16, 17, 255, 256, 257, 513, 1025):
            keys = [randomizer.getrandbits(32) for _ in range(length)]
            values = list(range(length))
            with self.subTest(length=length):
                sorted_keys, sorted_values = radix_sort_u32(keys, values)
                expected = sorted(enumerate(keys), key=lambda pair: pair[1])
                self.assertEqual(sorted_keys, [key for _, key in expected])
                self.assertEqual(sorted_values, [index for index, _ in expected])


class GroupingTests(unittest.TestCase):
    def test_rle(self) -> None:
        keys = [4, 4, 4, 7, 9, 9, 2, 2, 2, 2]
        self.assertEqual(
            rle_u32(keys),
            ([4, 7, 9, 2], [0, 3, 4, 6], [3, 1, 2, 4]),
        )
        self.assertEqual(rle_u32([]), ([], [], []))

    def test_segmented_scan_crosses_tiles(self) -> None:
        length = TILE_ITEMS * 3 + 19
        values = [u32(index * 17 + 5) for index in range(length)]
        heads = [0] * length
        heads[0] = 1
        for index in (3, TILE_ITEMS - 1, TILE_ITEMS + 7, TILE_ITEMS * 2 + 1):
            heads[index] = 1
        expected = segmented_exclusive_scan_u32(values, heads)
        self.assertEqual(tiled_segmented_exclusive_scan_u32(values, heads), expected)

    def test_segmented_random_and_reduction(self) -> None:
        randomizer = random.Random(0x5345_474D)
        for length in (0, 1, 15, 16, 17, 255, 256, 257, 1027):
            values = [randomizer.getrandbits(32) for _ in range(length)]
            heads = [0] * length
            if length:
                heads[0] = 1
                for index in range(1, length):
                    heads[index] = 1 if randomizer.randrange(11) == 0 else 0
            with self.subTest(length=length):
                expected = segmented_exclusive_scan_u32(values, heads)
                self.assertEqual(
                    tiled_segmented_exclusive_scan_u32(values, heads), expected
                )
                totals = segmented_reduce_sum_u32(values, heads)
                tail_indices = [
                    index
                    for index in range(length)
                    if index + 1 == length or heads[index + 1]
                ]
                observed = [
                    u32(expected[index] + values[index]) for index in tail_indices
                ]
                self.assertEqual(totals, observed)

    def test_segmented_requires_binary_heads_and_first_head(self) -> None:
        with self.assertRaises(ValueError):
            segmented_exclusive_scan_u32([1, 2], [0, 1])
        with self.assertRaises(ValueError):
            segmented_exclusive_scan_u32([1, 2], [1, 2])


if __name__ == "__main__":
    unittest.main()
