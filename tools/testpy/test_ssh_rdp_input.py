#!/usr/bin/env python3
"""Exercise production SSH RDP input parsing and per-session device lifecycle."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    fixture = (ROOT / "tools/testpy/fixtures/ssh_rdp_input.rs").read_text()
    fixture = fixture.replace("@RDP@", str(ROOT / "src/shell3/rdp.rs"))
    fixture = fixture.replace("@RAW_REPORT@", item((ROOT / "src/r/services/keyboard_control_service.rs").read_text(), "pub(crate) fn submit_report("))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-ssh-rdp-input-") as directory:
        directory = Path(directory)
        (directory / "src").mkdir()
        (directory / "src/lib.rs").write_text(fixture)
        (directory / "Cargo.toml").write_text('[package]\nname="ssh-rdp-input-tests"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\nheapless="0.9"\nspin="0.10"\n')
        subprocess.run(["cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"), "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/ssh-rdp-input"), "--", "--test-threads=1"], check=True, cwd=directory)


if __name__ == "__main__":
    main()
