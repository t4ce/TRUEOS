"""Check direct outline input against its SVG representation."""

from pathlib import Path
from struct import pack
from tempfile import TemporaryDirectory
import unittest

from meoj_geometry import read_bundle
from reduce import reduce_glyph
from svg_geometry import read_svg


SIZE = 17
WHITE = (255, 255, 255, 255)
BLACK = (0, 0, 0, 255)


def path_bytes(*commands):
    path = bytearray()
    for name, *coordinates in commands:
        path.append("MLQCZ".index(name))
        path.extend(pack(f"<{len(coordinates)}i", *(round(value * 64) for value in coordinates)))
    return path


def bundle_bytes(*glyphs):
    index = bytearray()
    payload = bytearray()
    for scalar, advance, layers in glyphs:
        glyph_data = b"".join(pack("<4BI", *rgba, len(path)) + path for rgba, path in layers)
        offset = 16 + 16 * len(glyphs) + len(payload)
        index.extend(pack("<IIIHH", scalar, offset, len(glyph_data), advance, len(layers)))
        payload.extend(glyph_data)
    return pack("<4sHHhhI", b"MEOJ", 1, 16, 8, -2, len(glyphs)) + index + payload


def svg_text(left, top, width, height, *paths):
    elements = "".join(f'<path fill="#{color}" d="{commands}"/>' for color, commands in paths)
    return (
        '<svg xmlns="http://www.w3.org/2000/svg"'
        f' viewBox="{left} {top} {width} {height}" width="{width}" height="{height}">'
        '<g transform="scale(1 -1)" fill-rule="nonzero" fill-opacity="1" stroke="none">'
        f"{elements}</g></svg>"
    )


class DirectOutlineTests(unittest.TestCase):
    def setUp(self):
        directory = TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.binary = Path(directory.name) / "microemoji.bin"
        self.svg = Path(directory.name) / "u0041.svg"

    def read(self, contents):
        self.binary.write_bytes(contents)
        return read_bundle(self.binary)

    def assert_svg_parity(self, source, svg):
        self.svg.write_text(svg, encoding="utf-8")
        direct = source.project(SIZE)
        exported = read_svg(self.svg, SIZE)
        self.assertEqual(direct.source_segments, exported.source_segments)
        self.assertEqual(len(direct.paints), len(exported.paints))
        for actual, expected in zip(direct.paints, exported.paints, strict=True):
            self.assertEqual((actual.rgb, actual.alpha), (expected.rgb, expected.alpha))
            self.assertTrue(actual.geometry.equals_exact(expected.geometry, tolerance=0))
        result = reduce_glyph(direct, SIZE, 0.25, 3.0)
        self.assertEqual(result, reduce_glyph(exported, SIZE, 0.25, 3.0))
        return result

    def test_metrics_fractional_overhangs_and_paint_order_match_svg(self):
        outline = path_bytes(
            ("M", -0.015625, 0), ("Q", -0.015625, 9.015625, 6, 8),
            ("C", 11, 8, 13.125, 2, 11, -2.015625), ("L", 2, -2), ("Z",),
        )
        stripe = path_bytes(
            ("M", 3, 1), ("L", 9, 1), ("L", 9, 1.75), ("L", 3, 1.75), ("Z",),
        )
        sources = self.read(bundle_bytes((0x41, 12, [(WHITE, outline), (BLACK, stripe)])))
        [(scalar, source)] = sources
        self.assertEqual(scalar, 0x41)
        result = self.assert_svg_parity(source, svg_text(
            -1, -10, 15, 13,
            ("ffffff", "M-0.015625 0Q-0.015625 9.015625 6 8C11 8 13.125 2 11 -2.015625L2 -2Z"),
            ("000000", "M3 1L9 1L9 1.75L3 1.75Z"),
        ))
        self.assertEqual(result["source_segments"], 8)
        self.assertEqual(result["status"], "candidate")

    def test_circle_and_thin_feature_emit_adjacency_strips(self):
        circle = path_bytes(
            ("M", 10.5, 3),
            ("C", 10.5, 5.5, 8.5, 7.5, 6, 7.5), ("C", 3.5, 7.5, 1.5, 5.5, 1.5, 3),
            ("C", 1.5, 0.5, 3.5, -1.5, 6, -1.5), ("C", 8.5, -1.5, 10.5, 0.5, 10.5, 3),
            ("Z",),
        )
        stripe = path_bytes(
            ("M", 3, 2.75), ("L", 9, 2.75), ("L", 9, 3.25), ("L", 3, 3.25), ("Z",),
        )
        [(_, source)] = self.read(bundle_bytes((0x41, 12, [(WHITE, circle), (BLACK, stripe)])))
        result = self.assert_svg_parity(source, svg_text(
            0, -8, 12, 10,
            ("ffffff", "M10.5 3C10.5 5.5 8.5 7.5 6 7.5C3.5 7.5 1.5 5.5 1.5 3"
             "C1.5 0.5 3.5 -1.5 6 -1.5C8.5 -1.5 10.5 0.5 10.5 3Z"),
            ("000000", "M3 2.75L9 2.75L9 3.25L3 3.25Z"),
        ))
        self.assertEqual(result["circles"], 1)
        outer, feature = result["strokes"]
        self.assertTrue(outer["closed"])
        self.assertLess(len(outer["vertices"]), 32)
        self.assertEqual(feature["role"], "feature")
        self.assertFalse(feature["closed"])
        self.assertEqual(feature["adjacency_indices"], [0, 0, 1, 1])

    def test_rejects_malformed_header_index_or_payload_extent(self):
        path = path_bytes(("M", 1, 1), ("L", 2, 1), ("L", 1, 2), ("Z",))
        valid = bundle_bytes((0x41, 12, [(WHITE, path)]))
        malformed = {
            "header": b"BOGUS" + valid[5:],
            "version": valid[:4] + pack("<H", 2) + valid[6:],
            "scalar": valid[:16] + pack("<I", 0xD800) + valid[20:],
            "payload gap": valid[:20] + pack("<I", 33) + valid[24:],
            "truncated payload": valid[:-1],
            "trailing bytes": valid + b"\0",
        }
        for case, contents in malformed.items():
            with self.subTest(case=case), self.assertRaises(ValueError):
                self.read(contents)

    def test_rejects_invalid_paths_when_projecting(self):
        paths = {
            "unclosed contour": path_bytes(("M", 1, 1), ("L", 2, 1)),
            "truncated command": path_bytes(("M", 1, 1), ("Q", 2, 1, 3, 1))[:-3],
            "unknown command": b"\xff",
            "close without move": path_bytes(("Z",)),
        }
        for case, path in paths.items():
            with self.subTest(case=case), self.assertRaises(ValueError):
                [(_, source)] = self.read(bundle_bytes((0x41, 12, [(WHITE, path)])))
                source.project(SIZE)


if __name__ == "__main__":
    unittest.main()
