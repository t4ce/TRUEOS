#!/usr/bin/env python3
"""Import raw authoring exports without losing independent bitmap fields."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
IMPORTER = ROOT / "tools/microfont-pack/import-lines.py"
LINES = bytes.fromhex("40 28 2C C1 23 2C 33")


def record(mask=0, flags=1, kind=0, payload=b""):
    return mask.to_bytes(16, "little") + bytes([flags, kind, 0, 0]) + payload


class LineImport(unittest.TestCase):
    def test_named_homebutton_import_keeps_it_private_and_line_only(self):
        with tempfile.TemporaryDirectory(prefix="microfont-import-home-") as tmp:
            root = Path(tmp)
            data = root / "data"
            raw = root / "homebutton.bin"
            raw.write_bytes(LINES)
            result = self.run_import(data, raw)
            self.assertEqual(result.returncode, 0, result.stderr)
            target = data / "homebutton.bin"
            self.assertEqual(target.read_bytes(), record(0, 0, 1, LINES))
            raw.write_bytes(bytes.fromhex('C0 00 40'))
            result = self.run_import(data, raw)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(target.read_bytes(), record(0, 0, 1, raw.read_bytes()))

    def run_import(self, data, *sources, recover=False, glyph=None):
        args = ["python3", str(IMPORTER), "--data-dir", str(data)]
        if recover:
            args.append("--recover-header-from-git")
        if glyph:
            args += ["--glyph", glyph]
        return subprocess.run(args + [str(p) for p in sources], capture_output=True, text=True)

    def test_preserves_bitmaps_and_turns_placeholder_or_alias_into_lines(self):
        with tempfile.TemporaryDirectory(prefix="microfont-import-") as tmp:
            root = Path(tmp)
            data = root / "data"
            data.mkdir()
            for cp, before in [(0x2194, record(0, 1)),
                               (0x2192, record(0x102FC2100000, 1, 1, bytes.fromhex("C0 28 2D"))),
                               (0x2190, record(0x123, 1, 3, (0x2192).to_bytes(4, "little") + b"\x01"))]:
                target = data / f"u{cp:04X}.bin"
                target.write_bytes(before)
                raw = root / target.name
                raw.write_bytes(LINES)
                result = self.run_import(data, raw)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(target.read_bytes(), before[:17] + b"\x01\0\0" + LINES)
                self.assertEqual(raw.read_bytes(), LINES)

    def test_new_glyph_and_custom_export_name(self):
        with tempfile.TemporaryDirectory(prefix="microfont-import-new-") as tmp:
            root = Path(tmp)
            raw = root / "export.bin"
            raw.write_bytes(LINES)
            result = self.run_import(root / "data", raw, glyph="U+2194")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / "data/u2194.bin").read_bytes(), record(0, 0, 1, LINES))

    def test_invalid_raw_inputs_and_corrupt_target_do_not_modify_the_target(self):
        with tempfile.TemporaryDirectory(prefix="microfont-import-invalid-") as tmp:
            root = Path(tmp)
            data = root / "data"
            data.mkdir()
            target = data / "u2194.bin"
            before = record(0x123, 1)
            target.write_bytes(before)
            raw = root / target.name
            for bad in [b"", bytes.fromhex("C1 00 40"), bytes.fromhex("C0 06 40"),
                        bytes.fromhex("C0 80 40"), bytes.fromhex("C0 58 40"), record(0, 1, 1, LINES)]:
                raw.write_bytes(bad)
                result = self.run_import(data, raw)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(target.read_bytes(), before)
            raw.write_bytes(LINES)
            damaged = bytearray(before)
            damaged[18] = 1
            target.write_bytes(damaged)
            result = self.run_import(data, raw)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(target.read_bytes(), damaged)

    def test_batch_validates_every_source_before_writing(self):
        with tempfile.TemporaryDirectory(prefix="microfont-import-batch-") as tmp:
            root = Path(tmp)
            data = root / "data"
            data.mkdir()
            before = record(0x123, 1)
            (data / "u2192.bin").write_bytes(before)
            good, bad = root / "u2192.bin", root / "u2194.bin"
            good.write_bytes(LINES)
            bad.write_bytes(bytes.fromhex("C1 00 40"))
            result = self.run_import(data, good, bad)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((data / good.name).read_bytes(), before)
            self.assertFalse((data / bad.name).exists())

    def test_explicit_history_recovery_for_overwritten_and_new_in_place_exports(self):
        with tempfile.TemporaryDirectory(prefix="microfont-import-history-") as tmp:
            root = Path(tmp)
            data = root / "data"
            data.mkdir()
            target = data / "u2192.bin"
            before = record(0x102FC2100000, 1)
            target.write_bytes(before)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "add", "data/u2192.bin"], check=True)
            subprocess.run(["git", "-C", str(root), "-c", "user.name=Font Test",
                            "-c", "user.email=font-test@example.invalid", "-c", "commit.gpgsign=false",
                            "commit", "-qm", "fixture"], check=True)
            target.write_bytes(LINES)
            self.assertNotEqual(self.run_import(data, target).returncode, 0)
            result = self.run_import(data, target, recover=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(target.read_bytes(), before[:17] + b"\x01\0\0" + LINES)
            new = data / "u2194.bin"
            new.write_bytes(LINES)
            result = self.run_import(data, new, recover=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(new.read_bytes(), record(0, 0, 1, LINES))


if __name__ == "__main__":
    unittest.main()
