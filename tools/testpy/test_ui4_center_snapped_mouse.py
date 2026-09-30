#!/usr/bin/env python3
"""Host regression for UI4 center-snapped physical mouse motion."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


SOURCE = "src/ui4/input_broker.rs"


def main():
    harness = item(SOURCE, "signed_delta")
    harness += item(SOURCE, "pointer_motion_delta")
    harness += item(SOURCE, "pointer_motion_delta_tests")
    with tempfile.TemporaryDirectory(prefix="trueos-center-snapped-mouse-") as directory:
        root = Path(directory)
        rust = root / "test.rs"
        binary = root / "test"
        rust.write_text(harness)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(rust), "-o", str(binary)],
            check=True,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
