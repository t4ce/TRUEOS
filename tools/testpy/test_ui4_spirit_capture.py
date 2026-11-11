#!/usr/bin/env python3
"""Exercise exact-frame Spirit PNG capture with the production capture/encoder.

Only frame-pool allocation/leases and presentation geometry are host adapters.
No display writeback, screen crop, compositor pixels, rig, or filesystem writes
are available to the extracted capture path.
"""
from pathlib import Path
import json
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/ui4/screenshot.rs").read_text()
    signatures = [
        "pub(super) enum CaptureError",
        "pub(crate) struct CompactWindowObservation",
        "pub(crate) enum CompactWindowObservationError",
        "impl From<CaptureError> for CompactWindowObservationError",
        "impl From<crate::graphics::encoder::png::PngEncodeError> for CompactWindowObservationError",
        "impl From<FramePoolError> for CaptureError",
        "struct CapturedWindowRgba",
        "pub(crate) fn capture_compact_window_observation(",
        "fn capture_window_rgba(",
        "fn copy_window_rgba_rows(",
        "fn encode_compact_window_png(",
        "fn compact_dimensions(",
        "fn resample_premultiplied_rgba_to_rgb(",
        "fn overlay_normalized_grid(",
        "fn grid_coordinate(",
        "fn decimal_label",
        "fn compact_text_width(",
        "fn draw_compact_text(",
        "fn compact_digit_glyph(",
        "fn blend_contrast_rgb_pixel(",
        "fn multiply_u8(",
    ]
    production = "\n".join(
        ("#[derive(Debug)]\n" if signature.startswith(("pub(super) enum", "pub(crate) enum", "pub(crate) struct", "struct Captured")) else "")
        + item(source, signature)
        for signature in signatures
    )
    constants = "\n".join(re.findall(
        r"^(?:pub\(crate\) )?const COMPACT_WINDOW_[^\n]+;", source, re.MULTILINE,
    ))
    fixture = (ROOT / "tools/testpy/fixtures/ui4_spirit_capture.rs").read_text()
    fixture = fixture.replace("@PNG@", str(ROOT / "crates/trueos-graphics/encoder/png.rs"))
    fixture = fixture.replace("@PRODUCTION@", constants + "\n" + production)
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-capture-") as directory:
        directory = Path(directory)
        (directory / "src").mkdir()
        (directory / "src/lib.rs").write_text(fixture)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-spirit-frame-capture-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\n'
            f'crc32fast={{path={json.dumps(str(ROOT / "vendor/crc32fast-1.5.0"))}}}\n'
            'miniz_oxide={version="=0.8.9",default-features=false,features=["with-alloc"]}\n'
        )
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/spirit-frame-capture"),
        ], cwd=directory, check=True)


if __name__ == "__main__":
    main()
