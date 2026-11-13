#!/usr/bin/env python3
"""Exercise the production Pipe B WD route with a bounded MMIO device model."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    mirror = (ROOT / "src/intel/display/mirror_map_dp_engine.rs").read_text()
    regs = (ROOT / "src/intel/display/regs.rs").read_text()
    display = (ROOT / "src/intel/display.rs").read_text()
    constants = regs[:regs.index("#[derive(Copy, Clone)]")]
    constants = constants.replace("pub(in crate::intel)", "pub(crate)")
    helpers = [item(regs, "fn decode_pipe_src")]
    helpers += [item(display, signature) for signature in [
        "struct PipeScalerRegisters", "fn pipe_scaler_registers", "const fn pipe_scaler_binding",
        "fn plane_buf_cfg_value", "fn program_plane_watermark_boot_safe", "fn program_plane_buf_cfg",
    ]]
    template = (ROOT / "tools/testpy/fixtures/wd_pipe_b.rs").read_text()
    source = template.replace("@REGISTERS@", constants).replace("@HELPERS@", "\n".join(helpers))
    # Nest the entire production module, so startup, capture, failure cleanup,
    # and teardown run unchanged rather than testing a copied routing algorithm.
    source = source.replace("@MIRROR@", re.sub(r"^//!", "//", mirror, flags=re.M))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-wd-pipe-b-") as temporary:
        directory = Path(temporary)
        harness = directory / "tests.rs"
        harness.write_text(source)
        for mode, flags in [("debug", []), ("optimized", ["-O"])]:
            binary = directory / mode
            subprocess.run(["rustup", "run", channel, "rustc", "--edition=2024", "--test",
                            *flags, str(harness), "-o", str(binary)], check=True, timeout=60)
            subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=30)


if __name__ == "__main__":
    main()
