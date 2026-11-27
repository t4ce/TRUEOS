#!/usr/bin/env python3
"""Run the kernel's Gen12 multisample packet and layout tests on the host."""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


PIPELINE = "src/intel/render/pipeline.rs"
STATE = "src/intel/render/state.rs"
CONSTANTS = "src/intel/render/constants.rs"


def harness_source() -> str:
    state_source = (ROOT / STATE).read_text()
    state_methods = []
    for name in (
        "force_sample_generation",
        "raster_hammer",
        "multisample_dw1",
        "sample_mask_dw",
        "disable_ps_contract",
        "disable_ps_blend_writeable_rt",
    ):
        matches = re.findall(
            rf"^    fn {name}\b.*?^    }}$", state_source, re.MULTILINE | re.DOTALL
        )
        if len(matches) != 1:
            raise ValueError(f"{STATE}: expected one backend mode method {name}")
        state_methods.append(matches[0])

    declarations = [
        "#![allow(dead_code)]",
        "mod intel {",
        item("src/intel/mod.rs", "align_up"),
        "}",
        f'#[path = "{(ROOT / "src/intel/render/hiz.rs").as_posix()}"]',
        "mod hiz;",
        *(item(STATE, name) for name in (
            "TriangleDepthConfig", "BackendProbeMode", "TriangleBlendProbeMode",
        )),
        "impl BackendProbeMode {",
        *state_methods,
        "}",
        *(item("src/intel/render/submit.rs", name) for name in (
            "device_is_gfx12", "device_is_gfx125",
        )),
    ]
    declarations.extend(constant(CONSTANTS, name) for name in (
        "RESIDENT_SCENE_TARGET_WIDTH", "RESIDENT_SCENE_TARGET_HEIGHT",
        "SURFACE_FORMAT_R8G8B8A8_UNORM", "SURFACE_FORMAT_B8G8R8A8_UNORM",
        "SURFACE_HALIGN_4", "SURFACE_HALIGN_128_GFX125", "SURFACE_VALIGN_4",
        "SURFTYPE_2D", "RENDER_MOCS", "GPU_VA_RESIDENT_SCENE_MSAA_COLOR_BASE",
        "GPU_VA_RESIDENT_SCENE_MSAA_DEPTH_BASE", "SHADER_CHANNEL_RED",
        "SHADER_CHANNEL_GREEN", "SHADER_CHANNEL_BLUE", "SHADER_CHANNEL_ALPHA",
        "COMPARE_FUNCTION_LEQUAL",
    ))
    declarations.append(constant(PIPELINE, "MESA_POINT_SAMPLE_MASK_DW"))
    declarations.extend(item(PIPELINE, name) for name in (
        "Gen12MsaaLayout", "gen12_msaa_device", "gen12_msaa_layout",
        "gen12_msaa_depth_matches_target", "gen12_msaa_raster_dw1",
        "triangle_sample_count_encoding", "triangle_sample_state", "triangle_sample_pattern",
        "triangle_ps_blend_dw1",
        "TriangleRenderTargetSurface", "triangle_render_target_surface_state",
        "mesa_sample_mask_dw", "native_raster_dw1", "gen12_msaa_tests",
    ))
    return "\n".join(declarations)


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-gen12-msaa-packets-") as temporary:
        directory = Path(temporary)
        source = directory / "packets.rs"
        binary = directory / "packets"
        source.write_text(harness_source())
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "-Dwarnings", str(source), "-o", str(binary)],
            check=True,
            cwd=ROOT,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
