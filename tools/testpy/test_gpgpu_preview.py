#!/usr/bin/env python3
"""Run the surviving production preview routing and configuration tests."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item

SOURCE = "src/ui4/gpgpu_preview_consumer.rs"


def impl_item(name):
    source = (ROOT / SOURCE).read_text()
    start = source.index(f"impl {name} {{")
    end = re.search(r"^}\n", source[start:], re.MULTILINE)
    assert end is not None
    return source[start:start + end.end()]


def existing_test(name):
    source = (ROOT / SOURCE).read_text()
    start = source.index(f"    #[test]\n    fn {name}(")
    end = re.search(r"^    }\n", source[start:], re.MULTILINE)
    assert end is not None
    return source[start:start + end.end()]


def main():
    code = "#![allow(dead_code)]\n"
    code += "mod intel { pub mod gpgpu {\n"
    for name in ("PARTICLE_CRAFT_FRAME_WIDTH", "PARTICLE_CRAFT_FRAME_HEIGHT"):
        code += constant("src/intel/gpgpu/operations/particle_craft.rs", name)
    code += "}}\n"
    for name in ("DEFAULT_FRAME_WIDTH", "DEFAULT_FRAME_HEIGHT", "ALPHA_OVERLAY_PLANE_SLOT"):
        code += constant("src/ui4/mod.rs", name)
    code += "mod preview {\n"
    for name in ("PREVIEW_WIDTH", "PREVIEW_HEIGHT", "LAB256_PREVIEW_SIZE",
                 "GPGPU_PREVIEW_DEFAULT_DURATION_MS", "GPGPU_PREVIEW_DEFAULT_CADENCE_MS",
                 "GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY", "GPGPU_PREVIEW_MAX_CADENCE_MS",
                 "GPGPU_PREVIEW_MAX_PUBLISH_EVERY"):
        code += constant(SOURCE, name)
    for name in ("GpgpuPreviewPreset", "GpgpuPreviewConfig"):
        code += item(SOURCE, name) + impl_item(name)
    code += item(SOURCE, "preview_extent") + item(SOURCE, "preview_plane_slot")
    for name in ("lab256_preview_keeps_artifact_extent",
                 "cpp_modes_share_the_resizable_slot1_application_surface",
                 "preview_config_accepts_continuous_duration",
                 "preview_config_rejects_invalid_scheduler_values"):
        code += existing_test(name)
    code += "}\n"
    with tempfile.TemporaryDirectory(prefix="trueos-gpgpu-preview-") as directory:
        source = Path(directory) / "tests.rs"
        executable = Path(directory) / "tests"
        source.write_text(code)
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], check=True)
        subprocess.run([str(executable)], check=True)


if __name__ == "__main__":
    main()
