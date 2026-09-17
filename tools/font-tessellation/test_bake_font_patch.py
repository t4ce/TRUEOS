import importlib.util
import json
import math
from pathlib import Path
import tempfile
import unittest


TOOL_PATH = Path(__file__).with_name("bake_font_patch.py")
SPEC = importlib.util.spec_from_file_location("bake_font_patch", TOOL_PATH)
TOOL = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(TOOL)


class FontPatchBakeTests(unittest.TestCase):
    def test_source_fixture_is_deterministic_and_uses_hardware_stages(self):
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            one = TOOL.write_sources(TOOL.DEFAULT_FIXTURE, Path(first))
            two = TOOL.write_sources(TOOL.DEFAULT_FIXTURE, Path(second))
            self.assertEqual(one, two)
            packed = (Path(first) / "patches.fcp1").read_bytes()
            self.assertEqual(len(packed), one["patch_count"] * 64)
            self.assertEqual(packed, (Path(second) / "patches.fcp1").read_bytes())
            hs = (Path(first) / "font_patch.tesc").read_text()
            ds = (Path(first) / "font_patch.tese").read_text()
            self.assertIn("layout(vertices=5) out", hs)
            self.assertIn("ceil(sqrt(", hs)
            self.assertNotIn("clamp(ceil", hs)
            self.assertIn("layout(quads, equal_spacing, ccw) in", ds)
            self.assertIn("mix(patchPoint[4], cubic(gl_TessCoord.x)", ds)
            self.assertFalse(one["native_capture"])
            self.assertFalse(one["gpu_submitted"])
            self.assertFalse(one["runtime_integrated"])
            self.assertFalse(one["baremetal_verified"])

    def test_non_finite_and_unknown_flags_fail_closed(self):
        fixture = TOOL.load_fixture(TOOL.DEFAULT_FIXTURE)
        for mutation, message in (
            (("p0", [math.inf, 0.0]), "non-finite"),
            (("flags", ["future_flag"]), "unknown"),
        ):
            with self.subTest(message=message), tempfile.TemporaryDirectory() as directory:
                copy = json.loads(json.dumps(fixture))
                copy["patches"][0][mutation[0]] = mutation[1]
                path = Path(directory) / "fixture.json"
                path.write_text(json.dumps(copy))
                with self.assertRaisesRegex(ValueError, message):
                    TOOL.load_fixture(path)

    def test_invalid_contour_boundaries_fail_closed(self):
        fixture = TOOL.load_fixture(TOOL.DEFAULT_FIXTURE)
        with tempfile.TemporaryDirectory() as directory:
            fixture["patches"][0]["flags"] = ["end_contour"]
            path = Path(directory) / "fixture.json"
            path.write_text(json.dumps(fixture))
            with self.assertRaisesRegex(ValueError, "contour begin"):
                TOOL.load_fixture(path)

    def test_capture_import_requires_complete_state(self):
        with tempfile.TemporaryDirectory() as output, tempfile.TemporaryDirectory() as capture:
            out = Path(output)
            TOOL.write_sources(TOOL.DEFAULT_FIXTURE, out)
            with self.assertRaisesRegex(ValueError, "missing capture files"):
                TOOL.import_capture(out, Path(capture), out / "generated.rs", "0xa780")

    def test_m0_json_files_are_well_formed(self):
        root = TOOL_PATH.parent
        contract = json.loads((root / "m0-contract.json").read_text())
        schema = json.loads((root / "result.schema.json").read_text())
        baseline = json.loads((root / "baseline.json").read_text())
        self.assertEqual(contract["abi"]["size_bytes"], TOOL.PATCH_STRUCT.size)
        self.assertEqual(contract["abi"]["domain"], "quad")
        self.assertEqual(contract["default_mode"], "legacy-only")
        self.assertEqual(schema["properties"]["schema"]["const"], "trueos.font-hardware-tessellation-result/v1")
        self.assertEqual(baseline["evidence"]["bare_metal_render"], "not-run")


if __name__ == "__main__":
    unittest.main()
