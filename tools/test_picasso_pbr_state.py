#!/usr/bin/env python3
"""Host regression tests for the production Picasso material state encoders."""
import subprocess
import tempfile
from pathlib import Path
from test_clip_position3_uv_texture import ROOT, item, constant


def main():
    state = "src/intel/render/state.rs"
    pipeline = "src/intel/render/pipeline.rs"
    constants = "src/intel/render/constants.rs"
    code = [item(state, name) for name in ("TriangleVertexFormat", "TriangleSampledTextureBinding")]
    code += [constant(constants, name) for name in (
        "CMD_3DSTATE_VERTEX_ELEMENTS_1", "SURFTYPE_2D", "RENDER_MOCS",
        "SURFACE_FORMAT_R8G8B8A8_UNORM", "SURFACE_FORMAT_R8G8B8A8_UNORM_SRGB",
        "SURFACE_HALIGN_4", "SURFACE_VALIGN_4", "SHADER_CHANNEL_ALPHA",
        "SHADER_CHANNEL_BLUE", "SHADER_CHANNEL_GREEN", "SHADER_CHANNEL_RED",
    )]
    code += [constant(pipeline, "SAMPLER_CACHE_LINE_DWORDS")]
    code += [item(pipeline, name) for name in (
        "ordinary_vf_vertex_element_count", "cmd_3dstate_vertex_elements",
        "write_pbr_sampler_cache_line", "write_triangle_sampled_surface_state",
        "write_triangle_sampled_rgba8_surface_state", "picasso_pbr_state_tests",
        "sampled_rgba8_surface_tests",
    )]
    with tempfile.TemporaryDirectory(prefix="trueos-picasso-pbr-tests-") as tmp:
        source = Path(tmp) / "tests.rs"
        executable = Path(tmp) / "tests"
        source.write_text("#![allow(dead_code)]\n" + "\n".join(code))
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], cwd=ROOT, check=True)
        subprocess.run([str(executable)], check=True)


if __name__ == "__main__":
    main()
