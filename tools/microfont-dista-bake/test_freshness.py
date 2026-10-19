#!/usr/bin/env python3
"""Exercise editable hex seeds and incremental native artifacts without a GPU.

Every mutation happens in an isolated temporary repository fixture. Native
compilation is mocked only at its boundary; seed parsing, recipe validation,
artifact hashing and generated Rust snapshots execute the production baker.
"""
from contextlib import ExitStack
from contextlib import redirect_stdout
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import shutil
import tempfile
import sys
import unittest
from unittest import mock


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
spec = importlib.util.spec_from_file_location("dista_freshness_baker", HERE / "bake.py")
bake = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bake)


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="trueos-dista-freshness-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        paths = [
            "vendor/microfont/src/basic.rs",
            "vendor/microfont/src/fontshape.rs",
            "vendor/microfont/src/raster.rs",
            "vendor/microfont/shaders/dista_stamp.vert",
            "tools/microfont-dista-bake/bake.py",
            "tools/helio-intel-bake/bake.py",
            "crates/trueos-shader/generated_microfont_dista.rs",
            "crates/trueos-shader/generated_triangle.rs",
            "crates/trueos-shader/xe_lp_shader_bake/simple_triangle.frag",
            "crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c",
        ]
        for relative in paths:
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, destination)
        shutil.copytree(ROOT / "crates/trueos-shader/microfont_dista",
                        self.root / "crates/trueos-shader/microfont_dista")
        self.basic = self.root / "vendor/microfont/src/basic.rs"
        self.source = self.root / "vendor/microfont/shaders/dista_stamp.vert"
        self.output = self.root / "crates/trueos-shader/microfont_dista"
        self.generated = self.root / "crates/trueos-shader/generated_microfont_dista.rs"
        stack = self.enterContext(ExitStack())
        for name, value in [("ROOT", self.root), ("SOURCE", self.source),
                            ("OUT", self.output), ("GENERATED", self.generated)]:
            stack.enter_context(mock.patch.object(bake, name, value))

    def replace_basic_value(self, character, value):
        source = self.basic.read_text()
        prefix = re.escape("    (" + repr(character) + ", ")
        source, count = re.subn(prefix + r"(?:0x[0-9a-fA-F_]+|0|z\([^)]*\))(?=\),)",
                                lambda found: found[0].split(", ", 1)[0] + ", " + value,
                                source, count=1)
        self.assertEqual(count, 1)
        self.basic.write_text(source)

    def assert_rejected(self):
        with self.assertRaises((SystemExit, ValueError)):
            bake.basic_seeds()

    def make_current(self):
        """Publish a consistent mocked bake using real artifact serialization."""
        seeds, include, source_sha = bake.current_inputs()
        native = (self.output / "dista_stamp.vs.simd8.bin").read_bytes()
        manifest_path = self.output / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest.update(source_sha256=source_sha, generator_sha256=bake.generator_fingerprint(),
                        vs_sha256=hashlib.sha256(native).hexdigest(), vs_bytes=len(native))
        (self.output / "basic_seeds.glsl").write_text(include)
        bake.emit_rust(native, manifest["vertex_compiler_state"], seeds, source_sha)
        manifest["generated_rust_sha256"] = hashlib.sha256(self.generated.read_bytes()).hexdigest()
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")

    def tree_fingerprints(self):
        return {str(path.relative_to(self.root)): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in self.root.rglob("*") if path.is_file()}

    def update_generated_digest(self):
        """Reach semantic checks independently of whole-file integrity."""
        path = self.output / "manifest.json"
        manifest = json.loads(path.read_text())
        manifest["generated_rust_sha256"] = hashlib.sha256(self.generated.read_bytes()).hexdigest()
        path.write_text(json.dumps(manifest))


class SeedParsingTests(Fixture):
    def test_current_table_and_escaped_backslash_override_are_complete(self):
        seeds = bake.basic_seeds()
        self.assertEqual(len(seeds), 128)
        overrides = self.basic.read_text().split("pub const BASIC_RASTER_OVERRIDES:", 1)[1].split("];", 1)[0]
        literal = re.search(r"pixels:\s*(0x[0-9a-fA-F_]+).*?long_reach:\s*(true|false)",
                            next(line for line in overrides.splitlines() if "('\\\\'," in line))
        self.assertEqual(seeds[ord("\\")], (int(literal[1].replace("_", ""), 16), literal[2] == "true"))

    def test_raw_hex_preserves_unsigned_top_grid_bits(self):
        value = (1 << 65) | (1 << 64) | (1 << 33) | 1
        self.replace_basic_value("M", hex(value))
        self.assertEqual(bake.basic_seeds()[ord("M")], (value, False))
        include = bake.seed_include(bake.basic_seeds())
        self.assertIn("case 77u: return uvec3(1u,2u,3u);", include)

    def test_rust_hex_separators_do_not_change_the_mask(self):
        self.replace_basic_value("M", "0x3_0000_0002_0000_0001")
        self.assertEqual(bake.basic_seeds()[ord("M")], (0x30000000200000001, False))

    def test_z_expression_and_raw_hex_describe_the_same_mask(self):
        self.replace_basic_value("M", "z(0x3, 64)")
        shifted = bake.basic_seeds()[ord("M")]
        self.replace_basic_value("M", "0x30000000000000000")
        self.assertEqual(bake.basic_seeds()[ord("M")], shifted)

    def test_invalid_expression_cannot_silently_keep_a_previous_glyph(self):
        self.replace_basic_value("M", "other_seed(123)")
        self.assert_rejected()


    def test_mask_above_the_66_bit_grid_is_rejected(self):
        self.replace_basic_value("M", "0x40000000000000000")
        self.assert_rejected()

    def test_declared_override_count_must_match_parsed_entries(self):
        self.basic.write_text(re.sub(r"(pub const BASIC_RASTER_OVERRIDES:.*?;\s*)\d+(\]\s*=)",
                                    r"\g<1>99\2", self.basic.read_text()))
        self.assert_rejected()

    def test_duplicate_override_does_not_silently_replace_another_entry(self):
        source = self.basic.read_text()
        table, tail = source.split("pub const BASIC_RASTER_OVERRIDES:", 1)
        tail = tail.replace("('y', crate::raster::GlyphSeed", "('v', crate::raster::GlyphSeed", 1)
        self.basic.write_text(table + "pub const BASIC_RASTER_OVERRIDES:" + tail)
        self.assert_rejected()

    def test_wrong_basic_character_order_is_rejected(self):
        self.basic.write_text(self.basic.read_text().replace("('M',", "('N',", 1))
        self.assert_rejected()


class ArtifactFreshnessTests(Fixture):
    def setUp(self):
        super().setUp()
        self.make_current()
        current, reason = bake.artifact_freshness()
        self.assertTrue(current, reason)

    def assert_stale(self):
        current, reason = bake.artifact_freshness()
        self.assertFalse(current)
        self.assertTrue(reason)

    def test_unchanged_artifact_reuses_native_bytes_without_invoking_compiler(self):
        before = self.tree_fingerprints()
        with mock.patch.object(bake, "bake_native", side_effect=AssertionError("unexpected compiler")):
            bake.ensure_current()
        self.assertEqual(before, self.tree_fingerprints())

    def test_equivalent_hex_formatting_reuses_the_same_native_artifact(self):
        value, long_reach = bake.basic_seeds()[ord("M")]
        literal = f"{value:032X}"
        literal = "0x" + "_".join(literal[index:index + 8] for index in range(0, len(literal), 8))
        self.replace_basic_value("M", literal)
        self.assertEqual(bake.basic_seeds()[ord("M")], (value, long_reach))
        current, reason = bake.artifact_freshness()
        self.assertTrue(current, reason)
        with mock.patch.object(bake, "bake_native", side_effect=AssertionError("formatting invoked compiler")):
            bake.ensure_current()

    def test_plain_basic_hex_edit_triggers_one_rebake_then_becomes_current(self):
        self.replace_basic_value("M", "0x30000000200000001")
        self.assert_stale()
        with mock.patch.object(bake, "bake_native", side_effect=lambda *a, **k: self.make_current()) as compiler:
            bake.ensure_current()
        self.assertEqual(compiler.call_count, 1)
        current, reason = bake.artifact_freshness()
        self.assertTrue(current, reason)
        with mock.patch.object(bake, "bake_native", side_effect=AssertionError("second compiler invocation")):
            bake.ensure_current()

    def test_changed_override_reach_invalidates_artifact_even_when_pixels_match(self):
        source = self.basic.read_text()
        table, tail = source.split("pub const BASIC_RASTER_OVERRIDES:", 1)
        tail = tail.replace("long_reach: true", "long_reach: false", 1)
        self.basic.write_text(table + "pub const BASIC_RASTER_OVERRIDES:" + tail)
        self.assert_stale()

    def test_shader_edit_invalidates_even_when_all_seeds_stay_the_same(self):
        self.source.write_text(self.source.read_text() + "\n// shader input changed\n")
        self.assert_stale()

    def test_rust_recipe_drift_stops_before_native_compilation(self):
        recipe = self.root / "vendor/microfont/src/fontshape.rs"
        recipe.write_text(recipe.read_text().replace("INFLUENCE_WIDTH: f64 = 0.20", "INFLUENCE_WIDTH: f64 = 0.21"))
        with mock.patch.object(bake, "bake_native", side_effect=AssertionError("recipe mismatch reached compiler")):
            with self.assertRaises((SystemExit, ValueError)):
                bake.ensure_current()

    def test_generator_changes_invalidate_the_previous_native_artifact(self):
        generator = self.root / "tools/microfont-dista-bake/bake.py"
        generator.write_text(generator.read_text() + "\n# generator input changed\n")
        self.assert_stale()

    def test_compiler_helper_dumper_and_fragment_inputs_are_fingerprinted(self):
        for relative in ["tools/helio-intel-bake/bake.py",
                         "crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c",
                         "crates/trueos-shader/xe_lp_shader_bake/simple_triangle.frag"]:
            with self.subTest(input=relative):
                path = self.root / relative
                previous = path.read_text()
                path.write_text(previous + "\n// compiler input changed\n")
                self.assert_stale()
                path.write_text(previous)

    def test_native_binary_corruption_is_not_accepted_as_a_current_bake(self):
        path = self.output / "dista_stamp.vs.simd8.bin"
        native = bytearray(path.read_bytes())
        native[len(native) // 2] ^= 1
        path.write_bytes(native)
        self.assert_stale()

    def test_generated_seed_corruption_is_detected_with_unchanged_source_hash(self):
        source = self.generated.read_text()
        source, count = re.subn(r"(?m)^(    \(0x)([0-9a-f]+)(, (?:true|false)\),)$",
                                lambda match: match[1] + format(int(match[2], 16) ^ 1, "x") + match[3],
                                source, count=1)
        self.assertEqual(count, 1)
        self.generated.write_text(source)
        self.update_generated_digest()
        self.assert_stale()
        self.assertEqual(bake.artifact_freshness()[1], "generated-seeds-mismatch")

    def test_generated_native_words_must_match_the_integrity_checked_binary(self):
        source = self.generated.read_text()
        before, code = source.split("static DISTA_VS_CODE:", 1)
        code, count = re.subn(r"0x([0-9a-f]{8})", lambda match: f"0x{int(match[1], 16) ^ 1:08x}", code, count=1)
        self.assertEqual(count, 1)
        self.generated.write_text(before + "static DISTA_VS_CODE:" + code)
        self.update_generated_digest()
        self.assert_stale()
        self.assertEqual(bake.artifact_freshness()[1], "embedded-native-mismatch")

    def test_generated_link_count_corruption_is_detected(self):
        source = self.generated.read_text()
        before, counts = source.split("pub(crate) const ACTIVE_LINK_COUNTS:", 1)
        counts, count = re.subn(r"(= \[\n\s*)(\d+)", lambda match: match[1] + str(int(match[2]) + 1), counts, count=1)
        self.assertEqual(count, 1)
        self.generated.write_text(before + "pub(crate) const ACTIVE_LINK_COUNTS:" + counts)
        self.update_generated_digest()
        self.assert_stale()
        self.assertEqual(bake.artifact_freshness()[1], "generated-links-mismatch")

    def test_manifest_native_identity_corruption_is_detected(self):
        path = self.output / "manifest.json"
        manifest = json.loads(path.read_text())
        manifest["vs_sha256"] = "0" * 64
        path.write_text(json.dumps(manifest))
        self.assert_stale()

    def test_missing_generated_output_is_stale_and_rebuilt(self):
        self.generated.unlink()
        self.assert_stale()
        with mock.patch.object(bake, "bake_native", side_effect=lambda *a, **k: self.make_current()) as compiler:
            bake.ensure_current()
        self.assertEqual(compiler.call_count, 1)
        self.assertTrue(self.generated.is_file())

    def test_compile_failure_leaves_a_stale_artifact_explicitly_stale(self):
        self.replace_basic_value("M", "0x30000000200000001")
        with mock.patch.object(bake, "bake_native", side_effect=RuntimeError("compiler unavailable")):
            with self.assertRaises((RuntimeError, SystemExit)):
                bake.ensure_current()
        self.assert_stale()

    def test_incomplete_mocked_rebake_fails_post_build_verification(self):
        self.replace_basic_value("M", "0x30000000200000001")
        with mock.patch.object(bake, "bake_native", return_value=None):
            with self.assertRaises((RuntimeError, SystemExit)):
                bake.ensure_current()
        self.assert_stale()

    def test_check_current_cli_is_read_only_and_does_not_require_compiler(self):
        before = self.tree_fingerprints()
        output = io.StringIO()
        with mock.patch.object(sys, "argv", ["bake.py", "--check-current"]), redirect_stdout(output), \
             mock.patch.object(bake, "bake_native", side_effect=AssertionError("check invoked compiler")):
            with self.assertRaises(SystemExit) as error:
                bake.main()
        self.assertEqual(error.exception.code, 0)
        self.assertIsInstance(json.loads(output.getvalue().splitlines()[-1]), dict)
        self.assertEqual(before, self.tree_fingerprints())

    def test_check_current_cli_fails_stale_without_rebuilding(self):
        self.replace_basic_value("M", "0x30000000200000001")
        before = self.tree_fingerprints()
        output = io.StringIO()
        with mock.patch.object(sys, "argv", ["bake.py", "--check-current"]), redirect_stdout(output), \
             mock.patch.object(bake, "bake_native", side_effect=AssertionError("stale check invoked compiler")):
            with self.assertRaises(SystemExit) as error:
                bake.main()
        self.assertNotEqual(error.exception.code, 0)
        self.assertIsInstance(json.loads(output.getvalue().splitlines()[-1]), dict)
        self.assertEqual(before, self.tree_fingerprints())



if __name__ == "__main__":
    unittest.main()
