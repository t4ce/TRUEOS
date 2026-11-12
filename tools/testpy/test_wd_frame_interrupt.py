#!/usr/bin/env python3
"""Execute production WD IRQ demultiplexing and wake-registration races."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-wd-irq-") as directory:
        directory = Path(directory)
        source = directory / "tests.rs"
        source.write_text(
            '#![allow(dead_code)]\n'
            f'#[path="{ROOT}/src/intel/interrupts/demux.rs"] mod demux;\n'
            f'#[path="{ROOT}/src/intel/interrupts/generation.rs"] mod generation;\n'
        )
        binary = directory / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
