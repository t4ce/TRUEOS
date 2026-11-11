#!/usr/bin/env python3
"""Exercise the production frame permit and trusted input-bucket policy.

Only the HUT, station ownership, live broker snapshots, and VM lifecycle are
host adapters. The real Spirit access registry and all admission decisions run
unchanged, including the production module's own unit tests.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    fixture = (ROOT / "tools/testpy/fixtures/ui4_spirit_access.rs").read_text()
    fixture = fixture.replace("@PRODUCTION@", str(ROOT / "src/ui4/spirit_access.rs"))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-access-") as directory:
        directory = Path(directory)
        (directory / "src").mkdir()
        (directory / "src/lib.rs").write_text(fixture)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-spirit-frame-access-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\n'
            'heapless={version="0.9",default-features=false}\nspin="0.10"\n'
        )
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/spirit-frame-access"),
            "--", "--test-threads=1",
        ], cwd=directory, check=True)


if __name__ == "__main__":
    main()
