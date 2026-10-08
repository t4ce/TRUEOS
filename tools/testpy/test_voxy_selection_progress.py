#!/usr/bin/env python3
"""Run the selector observation scope test without linking the client/GPU."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT.parent / "voxy/src/selection_progress.rs"


def main():
    with tempfile.TemporaryDirectory(prefix="voxy-selection-progress-") as directory:
        folder = Path(directory)
        (folder / "src").mkdir()
        (folder / "Cargo.toml").write_text(
            '[package]\nname="voxy-selection-progress-tests"\nversion="0.1.0"\n'
            'edition="2024"\n[dependencies]\n'
            'tokio={version="=1.52.3",default-features=false,features=["rt","time"]}\n'
            'tracing="=0.1.44"\n[workspace]\n'
        )
        (folder / "src/lib.rs").write_text(
            f'#![allow(dead_code)]\n#[path="{SOURCE}"] mod selection_progress;\n'
        )
        subprocess.run([
            "cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--target-dir", str(ROOT / "bld/voxy-selection-progress-host-tests"),
        ], cwd=folder, check=True)


if __name__ == "__main__":
    main()
