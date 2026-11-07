#!/usr/bin/env python3
"""Exercise the full vendored XML/HTML/MIME parsers through native I/O on a host."""
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
VENDOR = ROOT.parent / "TRUEOS-Buildins" / "vendor"
FIXTURE = Path(__file__).with_name("fixtures") / "share_xml_native.rs"


class NativeXml(unittest.TestCase):
    def test_native_parser_io_boundary(self):
        cache = ROOT / "tgt" / "share-xml-native-tests"
        cache.mkdir(parents=True, exist_ok=True)
        toolchain = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        with tempfile.TemporaryDirectory(prefix="fixture-", dir=cache) as directory:
            fixture = Path(directory)
            (fixture / "src").mkdir()
            (fixture / "tests").mkdir()
            (fixture / "src/lib.rs").write_text("#![no_std]\n")
            (fixture / "tests/native.rs").write_bytes(FIXTURE.read_bytes())
            packages = {"xml": "xml-1.4.0", "xml_legacy": "xml-rs-1.0.0", "xmltree": "xmltree-0.12.0", "htmlescape": "htmlescape-0.3.1", "mime": "mime-0.3.18"}
            manifest = '[package]\nname = "share-xml-native-boundary"\nversion = "0.0.0"\nedition = "2024"\n[workspace]\n[dependencies]\n'
            for alias, package in packages.items():
                rename = 'package = "xml-rs", ' if alias == "xml_legacy" else ""
                manifest += f'{alias} = {{ {rename}path = "{VENDOR / package}", features = ["trueos-native"] }}\n'
            manifest += f'trueos-io = {{ path = "{ROOT / "crates/trueos-io"}" }}\n[patch.crates-io]\nxml = {{ path = "{VENDOR / "xml-1.4.0"}" }}\n'
            (fixture / "Cargo.toml").write_text(manifest)
            # Run outside TRUEOS's kernel cargo configuration; the fixture explicitly
            # selects the host so its test harness can execute. Dependencies themselves
            # enable no_std and the actual shared core3 I/O traits.
            result = subprocess.run([os.environ.get("CARGO", "cargo"), "+" + toolchain, "test", "--offline", "--manifest-path", str(fixture / "Cargo.toml"), "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(cache / "build")], cwd=VENDOR.parent, check=False)
            self.assertEqual(result.returncode, 0, "native XML/HTML/MIME boundary fixture failed")


if __name__ == "__main__":
    unittest.main()
