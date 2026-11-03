#!/usr/bin/env python3
"""Run production compact-icon fallback code against mocked frame/atlas storage."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/ui4/collapse.rs").read_text()
    fallback = source[
        source.index("pub(super) fn ensure_window_collapse("):
        source.index("/// Copies a pitched straight-alpha source")
    ]
    with tempfile.TemporaryDirectory(prefix="trueos-collapse-fallback-") as directory:
        work = Path(directory)
        (work / "fallback.rs").write_text(fallback)
        (work / "harness.rs").write_bytes(
            (ROOT / "tools/testpy/ui4_collapse/fallback_harness.rs").read_bytes()
        )
        executable = work / "test"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "--target",
             "x86_64-unknown-linux-gnu", str(work / "harness.rs"), "-o", str(executable)],
            check=True,
        )
        subprocess.run([str(executable)], check=True)


if __name__ == "__main__":
    main()
