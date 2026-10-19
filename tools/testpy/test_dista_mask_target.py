#!/usr/bin/env python3
"""Compile production R8/RGBA target packets and their tests without a GPU."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, item, constant


def main():
    constants = "src/intel/render/constants.rs"
    declarations = [constant(constants, name) for name in (
        "SURFACE_FORMAT_R8_UNORM", "SURFACE_FORMAT_R8G8B8A8_UNORM", "SURFTYPE_2D",
        "RENDER_MOCS", "SURFACE_VALIGN_4", "SHADER_CHANNEL_ALPHA", "SHADER_CHANNEL_BLUE",
        "SHADER_CHANNEL_GREEN", "SHADER_CHANNEL_RED",
    )]
    declarations += [constant("src/intel/render/dista_mask.rs", name) for name in (
        "RESIDENT_DISTA_MASK_MAX_WIDTH", "RESIDENT_DISTA_MASK_MAX_HEIGHT",
    )]
    declarations += [item("src/intel/render/submit.rs", name)
                     for name in ("device_is_gfx12", "device_is_gfx125")]
    declarations += [item("src/intel/render/pipeline.rs", name)
                     for name in ("TriangleRenderTargetSurface", "triangle_render_target_surface_state")]
    declarations += [item("src/intel/render/dista_mask.rs", name)
                     for name in ("resident_dista_mask_shape_is_valid", "resident_dista_mask_tests",
                                  "dista_render_target_packet_tests")]
    surfaces = "src/intel/gpgpu/types/surfaces.rs"
    declarations += ["mod intel {" + item("src/intel/mod.rs", "align_up") + "\n"
                     + "pub(crate) mod gpgpu {\n"
                     + item(surfaces, "GpgpuRect") + "\n"
                     + item(surfaces, "GpgpuMask8Surface") + "\n"
                     + "impl GpgpuRect { pub const fn new(x:i32,y:i32,width:u32,height:u32)->Self { Self{x,y,width,height} } }\n"
                     + extract_mask_impl() + "\n} }\n"]
    with tempfile.TemporaryDirectory(prefix="trueos-dista-mask-tests-") as directory:
        source, executable = Path(directory) / "tests.rs", Path(directory) / "tests"
        source.write_text("#![allow(dead_code)]\n" + "\n".join(declarations))
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
                       cwd=ROOT, check=True)
        subprocess.run([str(executable)], check=True)


def extract_mask_impl():
    source = (ROOT / "src/intel/gpgpu/types/surfaces.rs").read_text()
    start = source.index("impl GpgpuMask8Surface {")
    return source[start:source.index("\n}\n", start) + 3]


if __name__ == "__main__":
    main()
