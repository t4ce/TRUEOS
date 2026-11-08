#!/usr/bin/env python3
"""Exercise live HID bucket moves and one-cursor pairing on the host."""
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    vinput = (ROOT / "crates/trueos-v/src/vinput.rs").read_text()
    # Keep production source kinds and palette definitions without linking the
    # kernel ABI or VLayer's unrelated hardware-facing modules.
    definitions = vinput[vinput.index("/// Origin of an input collection."):
                         vinput.index("/// Address of one independently clocked input device.")]
    flag = re.search(r"^pub const INPUT_COMBO_FLAG_AUTO_ASSIGNED:.*?;$", vinput, re.MULTILINE)
    assert flag
    with tempfile.TemporaryDirectory(prefix="trueos-hid-link-buckets-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text(
            '[package]\nname="hid-link-bucket-check"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nheapless="0.9"\nspin="0.10"\n'
        )
        (work / "src/lib.rs").write_text(
            '#![allow(dead_code)]\nextern crate self as v;\npub mod vinput {\n'
            + flag.group() + '\n' + definitions + '\n}\n'
            + f'#[path={json.dumps(str(ROOT / "src/usb3/hid/hut.rs"))}] mod hut;\n'
        )
        subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--quiet",
            "--target", "x86_64-unknown-linux-gnu",
        ], cwd=work, check=True)


if __name__ == "__main__":
    main()
