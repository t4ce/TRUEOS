#!/usr/bin/env python3
"""Run production terminal and Conway reference tests without kernel hardware."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-tests-") as directory:
        work = Path(directory)
        model = work / "model.rs"
        model.write_text(
            "extern crate alloc;\n"
            f'#[path = "{ROOT / "src/ui4/edit_life_model.rs"}"] mod model;\n'
        )
        for name, source in (
            ("terminal", ROOT / "crates/trueos-terminal/src/lib.rs"),
            ("model", model),
        ):
            binary = work / name
            subprocess.run(
                ["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)],
                cwd=ROOT, check=True,
            )
            subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
