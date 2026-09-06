#!/usr/bin/env python3
"""Run the production JPEG bitstream-bound regression tests without the kernel."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item


def main():
    source = "src/intel/media/pic_backend.rs"
    harness = item(source, "jpeg_bitstream_upper_bound") + item(source, "jpeg_bitstream_bound_tests")
    with tempfile.TemporaryDirectory(prefix="trueos-jpeg-bounds-") as directory:
        root = Path(directory)
        (root / "test.rs").write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", str(root / "test.rs"),
                        "-o", str(root / "test")], check=True)
        subprocess.run([str(root / "test")], check=True)


if __name__ == "__main__":
    main()
