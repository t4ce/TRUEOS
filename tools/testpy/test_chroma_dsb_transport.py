#!/usr/bin/env python3
"""Exercise the production Pipe A DSB transport against modeled hardware."""

from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
TRANSPORT = ROOT / "src/intel/display/lut_dsb.rs"


def main() -> None:
    channel = re.search(r'channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text())[1]
    fixture = (ROOT / "tools/testpy/fixtures/chroma_dsb_transport.rs").read_text()
    fixture = fixture.replace("@TRANSPORT_PATH@", str(TRANSPORT))
    with tempfile.TemporaryDirectory(prefix="trueos-chroma-dsb-") as temporary:
        directory = Path(temporary)
        harness = directory / "tests.rs"
        harness.write_text(fixture)
        for name, flags in [("debug", []), ("optimized", ["-O"])]:
            binary = directory / name
            subprocess.run(
                ["rustup", "run", channel, "rustc", "--edition=2024", "--test",
                 *flags, str(harness), "-o", str(binary)],
                cwd=ROOT,
                check=True,
                timeout=60,
            )
            # Each test needs fresh static production DMA state, as on reboot.
            tests = subprocess.check_output([str(binary), "--list"], text=True, timeout=30)
            cases = [line.removesuffix(": test") for line in tests.splitlines()
                     if line.endswith(": test")]
            if not cases:
                raise RuntimeError("No DSB transport tests found")
            for case in cases:
                subprocess.run([str(binary), "--exact", case], check=True, timeout=30)


if __name__ == "__main__":
    main()
