#!/usr/bin/env python3
"""Test the shared firmware PCI BAR wire codec without firmware or PCI access."""
from pathlib import Path
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    with tempfile.TemporaryDirectory(prefix="trueos-firmware-pci-bars-") as directory:
        binary = Path(directory) / "tests"
        subprocess.run([
            "rustc", "+" + channel, "--edition=2024", "--test",
            str(ROOT / "src/pci/firmware_bar_wire.rs"), "-o", str(binary),
        ], cwd=ROOT, check=True)
        subprocess.run([str(binary)], cwd=directory, check=True)


if __name__ == "__main__":
    main()
