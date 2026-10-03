#!/usr/bin/env python3
"""Clone the production carrier page tables against explicit host/guest fixtures."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class CarrierTableTests(unittest.TestCase):
    def run_fixture(self, fixture_name, module_name, source):
        fixture = (ROOT / "tools/rust-std/tests" / fixture_name).read_text()
        declaration = f"pub mod {module_name};"
        if declaration in fixture:
            fixture = fixture.replace(declaration, f'#[path="{source}"] {declaration}')
        else:
            declaration = f"mod {module_name};"
            fixture = fixture.replace(declaration, f'#[path="{source}"] {declaration}')
        with tempfile.TemporaryDirectory(prefix="trueos-carrier-tables-") as directory:
            path = Path(directory) / "tests.rs"
            binary = Path(directory) / "tests"
            path.write_text(fixture)
            subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True)

    def test_pointer_stable_host_overlay(self):
        self.run_fixture("carrier_tables.rs", "tables", ROOT / "src/hv/memory/carrier/tables.rs")

    def test_owned_carrier_table_lifetime(self):
        self.run_fixture("carrier_address_space.rs", "carrier", ROOT / "src/hv/memory/carrier.rs")


if __name__ == "__main__":
    unittest.main()
