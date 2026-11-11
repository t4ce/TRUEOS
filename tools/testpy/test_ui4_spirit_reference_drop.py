#!/usr/bin/env python3
"""Exercise production Spirit circle releases and copied Codex attachments.

The real drag registry, attachment admission function, path decoder and input
release blocks run on the host; only HUT authority, receiver liveness, glyph
rasterization, and native window services are adapted.
"""
from pathlib import Path
import json
import re
import subprocess
import tempfile

from test_edit_life_native import function
from test_vgpu_batch_overlay import block

ROOT = Path(__file__).resolve().parents[2]


def main():
    fixture = (ROOT / "tools/testpy/fixtures/ui4_spirit_reference_drop.rs").read_text()
    input_source = (ROOT / "src/ui4/input_broker.rs").read_text()
    circle = "\n".join("pub(crate) " + function(input_source, name) for name in [
        "spirit_dock_circle", "spirit_circle_contains", "dock_target_at", "signed_local",
    ])
    fixture = fixture.replace("@CIRCLE_FUNCTIONS@", circle)
    fixture = fixture.replace("@SECONDARY_RELEASE@", block(input_source, "if secondary_released {"))
    fixture = fixture.replace("@PRIMARY_RELEASE@", block(input_source, "if released & PRIMARY_BUTTON_MASK != 0 {"))
    fixture = fixture.replace("@REQUEST_ATTACHMENTS@", "pub(super) " + function(
        (ROOT / "src/ui4/codex_prompt.rs").read_text(), "request_attachments"))
    fixture = fixture.replace("@DRAG_DROP@", str(ROOT / "src/ui4/drag_drop.rs"))
    fixture = fixture.replace("@ATTACHMENTS@", str(ROOT / "src/ui4/codex_attachments.rs"))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-reference-drop-") as directory:
        directory = Path(directory)
        (directory / "src").mkdir()
        (directory / "src/lib.rs").write_text(fixture)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-spirit-reference-drop-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nspin="0.10"\n'
            f'microfont={{path={json.dumps(str(ROOT / "vendor/microfont"))},default-features=false,features=["raster"]}}\n'
        )
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/spirit-reference-drop"),
            "--", "--test-threads=1",
        ], cwd=directory, check=True)


if __name__ == "__main__":
    main()
