#!/usr/bin/env python3
"""Exercise the actual no-dependency font packer, validator, and embedded images."""

import io
import os
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest

from _microfont_build import build_microfont


ROOT = Path(__file__).resolve().parents[2]
CRATE = ROOT / "vendor/microfont"
TOPPERS = ("diaeresis", "acute", "grave", "circumflex", "ring", "tilde")
SELECTIONS = (("hex",), ("raster",), ("hex", "raster"))


def u32(data, at):
    return struct.unpack_from("<I", data, at)[0]


def put32(data, at, value):
    struct.pack_into("<I", data, at, value)


def entries(data):
    return [(24 + index * 24, u32(data, 24 + index * 24))
            for index in range(u32(data, 12))]


def sources():
    records = {}
    for path in (CRATE / "data/line-v1").rglob("*.bin"):
        if path.parent.name == "lowercase-toppers":
            identifier = 0x110000 + TOPPERS.index(path.stem)
        elif path.parent.name == "seven-segment":
            identifier = 0x110100 + int(path.stem.removeprefix("segment"))
        else:
            identifier = int(path.stem.removeprefix("u"), 16)
        records[identifier] = path.read_bytes()
    return records


class MicrofontBundle(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix="microfont-bundle-test-")
        cls.addClassCleanup(cls.temp.cleanup)
        cls.work = Path(cls.temp.name)
        cls.packer = cls.work / "build-script"
        subprocess.run(["rustc", "--edition=2024", "-A", "dead_code", str(CRATE / "build.rs"),
                        "-o", str(cls.packer)], check=True)
        validator = cls.work / "validator.rs"
        validator.write_text('#[path="' + str(CRATE / "src/bundle_format.rs") + '"] mod format;\n'
                             'fn main() { let args: Vec<_> = std::env::args().collect();\n'
                             'let data = std::fs::read(&args[1]).unwrap();\n'
                             'assert_eq!(format::validate(&data).is_ok(), args[2] == "valid"); }\n')
        cls.validator = cls.work / "validator"
        subprocess.run(["rustc", "--edition=2024", "-A", "dead_code", str(validator),
                        "-o", str(cls.validator)], check=True)
        cls.images = {selected: cls.run_packer(selected)[1] for selected in SELECTIONS}

    @classmethod
    def run_packer(cls, selected, target="none", output=None, check=True, source_dir=None):
        output = output or cls.work / (target + "-" + "-".join(selected or ("empty",)))
        output.mkdir(exist_ok=True)
        env = {key: value for key, value in os.environ.items()
               if not key.startswith("CARGO_FEATURE_") and key != "MICROFONT_SOURCE_DIR"}
        env.update(CARGO_MANIFEST_DIR=str(CRATE), OUT_DIR=str(output), CARGO_CFG_TARGET_OS=target)
        env.update({"CARGO_FEATURE_" + feature.upper(): "1" for feature in selected})
        if source_dir is not None:
            env["MICROFONT_SOURCE_DIR"] = str(source_dir)
        result = subprocess.run([str(cls.packer)], env=env, capture_output=True, text=True, check=check)
        return result, (output / "microfont.bin").read_bytes() if result.returncode == 0 else None

    def check_image(self, image, valid=True):
        path = self.work / "validate.bin"
        path.write_bytes(image)
        result = subprocess.run([str(self.validator), str(path), "valid" if valid else "invalid"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_selected_images_contain_exactly_the_requested_source_representations(self):
        stored = sources()
        for selected, image in self.images.items():
            with self.subTest(features=selected):
                flags = int("hex" in selected) | (int("raster" in selected) << 1)
                self.assertEqual(image[:8], b"MFBNDL2\0")
                self.assertEqual(struct.unpack_from("<HH", image, 8), (2, flags))
                self.assertEqual(u32(image, 16), 24)
                self.assertEqual(u32(image, 20), len(image))
                expected = {identifier: source for identifier, source in stored.items()
                            if (flags & 1 and source[16] & 1) or (flags & 2 and source[17])}
                self.assertEqual([identifier for _, identifier in entries(image)], sorted(expected))
                self.check_image(image)
                expected_size = 24 + len(expected) * 24
                for at, identifier in entries(image):
                    source = expected[identifier]
                    mask, raster, length = (u32(image, at + offset) for offset in (4, 8, 12))
                    if flags & 1 and source[16] & 1:
                        self.assertEqual(mask, expected_size)
                        self.assertEqual(image[mask:mask + 16], source[:16])
                        self.assertEqual(image[at + 17], 1)
                        expected_size += 16
                    else:
                        self.assertEqual((mask, image[at + 17]), (0, 0))
                    if flags & 2 and source[17]:
                        self.assertEqual((raster, length, image[at + 16]),
                                         (expected_size, len(source) - 20, source[17]))
                        self.assertEqual(image[raster:raster + length], source[20:])
                        expected_size += length
                    else:
                        self.assertEqual((raster, length, image[at + 16]), (0, 0, 0))
                self.assertEqual(len(image), expected_size)

    def test_neither_representation_is_rejected(self):
        result, image = self.run_packer((), check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(image)
        self.assertIn("hex, raster, or both", result.stderr)

    def test_invalid_sources_are_rejected_even_when_the_bad_representation_is_unselected(self):
        stored = sources()
        canonical = stored[ord("A")]
        self.assertEqual(canonical[17], 1)
        malformed = {}
        invalid_line = bytearray(canonical)
        invalid_line[21] = 6
        malformed["line outside cell"] = (ord("A"), invalid_line)
        malformed["missing alias target"] = next((identifier, source) for identifier, source in stored.items()
                                                 if source[17] == 3)
        invalid_mask = bytearray(canonical)
        invalid_mask[15] = 1
        malformed["bitmap outside cell"] = (ord("A"), invalid_mask)
        absent_mask = bytearray(canonical)
        absent_mask[16] = 0
        malformed["absent bitmap has bits"] = (ord("A"), absent_mask)
        trailing = bytearray(canonical)
        trailing[17] = 0
        malformed["no raster kind has payload"] = (ord("A"), trailing)
        for case_index, (name, (identifier, source)) in enumerate(malformed.items()):
            folder = self.work / ("malformed-source-" + str(case_index))
            folder.mkdir()
            (folder / f"u{identifier:04X}.bin").write_bytes(source)
            for selected in SELECTIONS:
                with self.subTest(case=name, features=selected):
                    output = self.work / ("reject-" + str(case_index) + "-" + "-".join(selected))
                    result, image = self.run_packer(selected, output=output, check=False, source_dir=folder)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIsNone(image)

    def test_trueos_archive_contains_exactly_one_selected_image_and_is_reproducible(self):
        for selected in SELECTIONS:
            with self.subTest(features=selected):
                output = self.work / ("trueos-" + "-".join(selected))
                _, raw = self.run_packer(selected, "trueos", output)
                self.assertEqual(raw, self.images[selected])
                archive = output / "microfont.tar.lz4"
                first = archive.read_bytes()
                self.run_packer(selected, "trueos", output)
                self.assertEqual(archive.read_bytes(), first)
                self.assertFalse((output / "microfont.tar").exists())
                decoded = subprocess.run(["lz4", "-q", "-d", "-c", str(archive)],
                                         check=True, capture_output=True).stdout
                self.assertLessEqual(len(decoded), len(raw) + 8192)
                with tarfile.open(fileobj=io.BytesIO(decoded), mode="r:") as tar:
                    members = tar.getmembers()
                    self.assertEqual(len(members), 1)
                    member = members[0]
                    self.assertEqual(member.name, "microfont.bin")
                    self.assertEqual((member.uid, member.gid, member.mtime), (0, 0, 0))
                    self.assertEqual(tar.extractfile(member).read(), raw)

    def test_validator_rejects_malformed_extents_payloads_and_references(self):
        good = self.images[("hex", "raster")]
        cases = {"empty": b"", "truncated header": good[:23], "truncated index": good[:30]}

        def change(name, at, replacement):
            data = bytearray(good)
            data[at:at + len(replacement)] = replacement
            cases[name] = bytes(data)

        def number(name, at, value):
            change(name, at, struct.pack("<I", value))

        change("magic", 0, b"badmagic")
        change("version", 8, b"\x03\0")
        change("no features", 10, b"\0\0")
        change("unknown feature", 10, b"\x07\0")
        change("upper feature bits", 11, b"\x01")
        number("index offset", 16, 0)
        number("declared size", 20, len(good) + 1)
        number("index count overflow", 12, 0xFFFFFFFF)
        first = entries(good)[0][0]
        second = entries(good)[1][0]
        number("duplicate id", second, u32(good, first))
        number("invalid id", first, 0xFFFFFFFF)
        change("reserved entry field", first + 20, b"\x01")
        change("unknown entry flags", first + 17, b"\x02")
        mask_at = next(at for at, _ in entries(good) if u32(good, at + 4))
        number("mask points into index", mask_at + 4, 24)
        change("mask flag omitted", mask_at + 17, b"\0")
        change("mask bit beyond cell", u32(good, mask_at + 4) + 8, b"\x04")
        change("mask reserved high byte", u32(good, mask_at + 4) + 15, b"\x01")
        raster_at = next(at for at, _ in entries(good) if good[at + 16])
        number("raster points into index", raster_at + 8, 24)
        number("raster extent", raster_at + 12, 0xFFFFFFFF)
        change("unknown recipe kind", raster_at + 16, b"\x06")
        line_at = next(at for at, _ in entries(good) if good[at + 16] == 1)
        point = u32(good, line_at + 8) + 1
        change("point reserved bit", point, b"\x80")
        change("point x outside cell", point, b"\x06")
        change("point y outside cell", point, b"\x58")
        quad_at = next(at for at, _ in entries(good) if good[at + 16] == 2)
        change("quad NaN", u32(good, quad_at + 8), struct.pack("<f", float("nan")))
        change("quad x outside cell", u32(good, quad_at + 8), struct.pack("<f", 7))
        change("quad y outside cell", u32(good, quad_at + 8) + 4, struct.pack("<f", 12))
        alias_at = next(at for at, _ in entries(good) if good[at + 16] == 3)
        alias = u32(good, alias_at + 8)
        number("alias missing source", alias, 0x10FFFF)
        number("alias references itself", alias, u32(good, alias_at))
        change("alias invalid transform", alias + 4, b"\0")
        composition_at = next(at for at, _ in entries(good) if good[at + 16] == 4)
        composition = u32(good, composition_at + 8)
        number("composition missing base", composition, 0x10FFFF)
        number("composition topper is alias", composition + 4, u32(good, alias_at))
        change("composition invalid dot flag", composition + 8, b"\x02")
        segment_at = next(at for at, _ in entries(good) if good[at + 16] == 5)
        number("segments missing source", u32(good, segment_at + 8), 0x10FFFF)
        number("segments noncanonical source", u32(good, segment_at + 8), u32(good, alias_at))
        extra = bytearray(good + b"\0")
        put32(extra, 20, len(extra))
        cases["trailing bytes"] = bytes(extra)
        for name, image in cases.items():
            with self.subTest(case=name):
                self.check_image(image, valid=False)

    def test_portable_embeds_raw_image_and_trueos_installs_only_archive(self):
        selected = ("hex", "raster")
        raw = self.images[selected]
        portable = self.work / "libportable.rlib"
        build_microfont(portable, selected)
        output = self.work / "runtime-trueos"
        self.run_packer(selected, "trueos", output)
        env = dict(os.environ, OUT_DIR=str(output))
        trueos = self.work / "libtrueos.rlib"
        subprocess.run(["rustc", "--edition=2024", "--crate-name", "microfont", "--crate-type", "rlib",
                        "-Adead_code", "-Aexplicit_builtin_cfgs_in_flags", "--cfg", 'target_os="trueos"',
                        "--cfg", 'feature="hex"', "--cfg", 'feature="raster"', str(CRATE / "src/lib.rs"),
                        "-o", str(trueos)], env=env, check=True)
        raw_path = self.work / "install.bin"
        raw_path.write_bytes(raw)
        for target, library in (("portable", portable), ("trueos", trueos)):
            source = self.work / (target + ".rs")
            source.write_text(r'''
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(microfont::bundle::is_initialized(), !cfg!(target_os = "trueos"));
    #[cfg(target_os = "trueos")] {
        assert!(microfont::bundle::bytes().is_none());
        std::fs::write(&args[2], microfont::EMBEDDED_ARCHIVE).unwrap();
    }
    #[cfg(not(target_os = "trueos"))]
    assert_eq!(microfont::bundle::bytes().unwrap(), microfont::bundle::EMBEDDED_BUNDLE);
    let data = Box::leak(std::fs::read(&args[1]).unwrap().into_boxed_slice());
    microfont::install_bundle(data).unwrap();
    assert!(microfont::bundle::is_initialized());
    assert_eq!(microfont::bundle::bytes().unwrap().as_ptr(), data.as_ptr());
    assert!(microfont::install_bundle(data).is_err());
    assert_ne!(microfont::bundle::bitmap('A').unwrap(), 0);
    assert!(microfont::raster_line::glyph_strips('A').is_some());
}
''')
            binary = self.work / (target + "-runtime")
            command = ["rustc", "--edition=2024", str(source), "--extern", "microfont=" + str(library),
                       "-o", str(binary)]
            if target == "trueos":
                command.extend(["-Aexplicit_builtin_cfgs_in_flags", "--cfg", 'target_os="trueos"'])
            subprocess.run(command, check=True)
            dumped_archive = self.work / "embedded.tar.lz4"
            subprocess.run([str(binary), str(raw_path), str(dumped_archive)], check=True)
            if target == "portable":
                self.assertIn(raw, binary.read_bytes())
            else:
                self.assertNotIn(raw, binary.read_bytes())
                self.assertIn(dumped_archive.read_bytes(), binary.read_bytes())
                self.assertEqual(dumped_archive.read_bytes(), (output / "microfont.tar.lz4").read_bytes())

    def test_distribution_cli_outputs_one_feature_selected_file(self):
        cli = ROOT / "tools/microfont-pack/build.py"
        for selected in SELECTIONS:
            for trueos in (False, True):
                with self.subTest(features=selected, trueos=trueos):
                    output = self.work / ("cli-" + "-".join(selected) + (".tar.lz4" if trueos else ".bin"))
                    command = ["python3", str(cli), "--features", ",".join(selected), "--output", str(output)]
                    if trueos:
                        command.append("--trueos")
                    subprocess.run(command, check=True, capture_output=True, text=True)
                    if trueos:
                        decoded = subprocess.run(["lz4", "-q", "-d", "-c", str(output)],
                                                 check=True, capture_output=True).stdout
                        with tarfile.open(fileobj=io.BytesIO(decoded), mode="r:") as tar:
                            self.assertEqual(tar.getnames(), ["microfont.bin"])
                            self.assertEqual(tar.extractfile("microfont.bin").read(), self.images[selected])
                    else:
                        self.assertEqual(output.read_bytes(), self.images[selected])
        invalid = subprocess.run(["python3", str(cli), "--features", "", "--output", str(self.work / "empty")],
                                 capture_output=True, text=True)
        self.assertNotEqual(invalid.returncode, 0)

    def test_competing_installers_publish_one_complete_static_image(self):
        library = self.work / "libconcurrent.rlib"
        build_microfont(library, ("hex", "raster"))
        # A second valid image with a very different length exposes publication
        # of a winner's pointer together with a losing installer's length.
        small = struct.pack("<8sHHIII", b"MFBNDL2\0", 2, 3, 1, 24, 64)
        small += struct.pack("<IIIIBBHI", ord("A"), 48, 0, 0, 0, 1, 0, 0)
        small += (1).to_bytes(16, "little")
        self.check_image(small)
        small_path = self.work / "small.bin"
        small_path.write_bytes(small)
        large_path = self.work / "large.bin"
        large_path.write_bytes(self.images[("hex", "raster")])
        source = self.work / "concurrent.rs"
        source.write_text(r'''
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let small: &'static [u8] = Box::leak(std::fs::read(&args[1]).unwrap().into_boxed_slice());
    let large: &'static [u8] = Box::leak(std::fs::read(&args[2]).unwrap().into_boxed_slice());
    let mut incompatible = small.to_vec();
    incompatible[10] = 1;
    assert_eq!(microfont::install_bundle(Box::leak(incompatible.into_boxed_slice())),
               Err("font feature mismatch"));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(32));
    let threads: Vec<_> = (0..32).map(|index| {
        let barrier = barrier.clone();
        let data = if index % 2 == 0 { small } else { large };
        std::thread::spawn(move || {
            barrier.wait();
            microfont::install_bundle(data).is_ok().then_some(data)
        })
    }).collect();
    let winners: Vec<_> = threads.into_iter().filter_map(|thread| thread.join().unwrap()).collect();
    assert_eq!(winners.len(), 1);
    let installed = microfont::bundle::bytes().unwrap();
    assert_eq!(installed.as_ptr(), winners[0].as_ptr());
    assert_eq!(installed.len(), winners[0].len());
    microfont::bundle::validate(installed).unwrap();
}
''')
        binary = self.work / "concurrent"
        subprocess.run(["rustc", "--edition=2024", str(source), "--extern", "microfont=" + str(library),
                        "-o", str(binary)], check=True)
        for _ in range(8):
            subprocess.run([str(binary), str(small_path), str(large_path)], check=True)


if __name__ == "__main__":
    unittest.main()
