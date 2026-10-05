#!/usr/bin/env python3
"""Run the native clip-position/UV shader contract tests on the host.

The freestanding kernel has ``test = false``. Compile its real shader module
and the pure render helpers with their existing Rust tests, without linking
hardware code or maintaining a second implementation of the state encoding.
Run from any directory with ``python3 tools/test_clip_position3_uv_texture.py``.
"""

from pathlib import Path
import hashlib
import json
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def verify_voxy_texture_composition_metadata() -> None:
    manifest = json.loads((ROOT / "crates/trueos-shader/voxy_headless_texture/metadata.json").read_text())
    source = (ROOT.parent / "veloren-voxygen/src/headless/render_textured.wgsl").read_bytes()
    digest = 0xCBF29CE484222325
    for byte in source:
        digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    assert digest == int(manifest["source_fnv1a64"], 0)
    assert hashlib.sha256(source).hexdigest() == manifest["source_sha256"]
    for stage, path in [
        ("vertex", ROOT.parent / "veloren-voxygen/src/headless/shaders/tgl/vs.bin"),
        ("fragment", ROOT / "picasso/picasso-retained-textured-forward/retained_textured_forward.ps.simd16.bin"),
    ]:
        code = path.read_bytes()
        assert len(code) == manifest[stage]["bytes"]
        assert hashlib.sha256(code).hexdigest() == manifest[stage]["sha256"]
    assert manifest["new_native_compilation"] is False
    assert manifest["host_render_verified"] is False
    assert manifest["baremetal_render_verified"] is False
    assert [(int(t["vendor_id"], 0), int(t["device_id"], 0), int(t["revision"], 0))
            for t in manifest["targets"]] == [(0x8086, 0x4680, 0x0C)]


def item(path: str, name: str) -> str:
    """Read a complete, unindented production item and its attributes.

    These source files use rustfmt's top-level closing-brace convention.
    Requiring exactly one declaration makes renames/removal fail explicitly,
    rather than silently skipping the associated regression tests.
    """
    source = (ROOT / path).read_text()
    declarations = list(
        re.finditer(
            rf"^(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?"
            rf"(?:fn|enum|struct|mod)\s+{re.escape(name)}\b",
            source,
            re.MULTILINE,
        )
    )
    if len(declarations) != 1:
        raise ValueError(f"{path}: expected one item {name}, got {len(declarations)}")
    start = declarations[0].start()
    attributes = re.search(r"(?:^#\[[^\n]*\]\n)+\Z", source[:start], re.MULTILINE)
    if attributes:
        start = attributes.start()
    ending = re.search(r"^}\s*\n", source[declarations[0].end() :], re.MULTILINE)
    if not ending:
        raise ValueError(f"{path}: no top-level closing brace for {name}")
    end = declarations[0].end() + ending.end()
    return source[start:end]


def constant(path: str, name: str) -> str:
    source = (ROOT / path).read_text()
    matches = list(
        re.finditer(
            rf"^(?:pub(?:\([^)]*\))?\s+)?const\s+{re.escape(name)}\b.*?;$",
            source,
            re.MULTILINE | re.DOTALL,
        )
    )
    if len(matches) != 1:
        raise ValueError(f"{path}: expected one constant {name}, got {len(matches)}")
    return matches[0].group()


def harness_source() -> str:
    state = "src/intel/render/state.rs"
    primary = "src/intel/render/primary.rs"
    pipeline = "src/intel/render/pipeline.rs"
    declarations = [
        item(state, "TriangleVertexFormat"),
        item(primary, "ResidentSceneFragmentContract"),
        item(primary, "resident_scene_shader_pipeline"),
        item(primary, "resident_scene_shader_pipeline_tests"),
        constant("src/intel/render/constants.rs", "CMD_3DSTATE_VERTEX_ELEMENTS_1"),
    ]
    # Additional state helpers and their production tests are kept here so
    # changing a kernel item name makes this harness fail at extraction time.
    declarations.extend(
        constant(pipeline, name)
        for name in ("SAMPLER_CACHE_LINE_DWORDS", "NEAREST_REPEAT_SAMPLER_STATE", "NEAREST_CLAMP_SAMPLER_STATE",
                     "SF_POINT_WIDTH_MASK", "MESA_SF_DW3", "RESIDENT_POINT_WIDTH_U8_3",
                     "MESA_CLIP_DW2", "MESA_POINT_CLIP_DW2", "MESA_POINT_SAMPLE_MASK_DW",
                     "MESA_POINT_WM_DEPTH_STENCIL_DW1")
    )
    declarations.extend(
        item(pipeline, name)
        for name in (
            "write_nearest_repeat_sampler_cache_line",
            "write_nearest_clamp_sampler_cache_line",
            "voxy_headless_shared_binding_table_entries",
            "resident_scene_sampler_flags_valid",
            "voxy_headless_atlas_state_tests",
            "ordinary_vf_vertex_element_count",
            "cmd_3dstate_vertex_elements",
            "mesa_vf_component_packing",
            "sbe_swiz_payload",
            "wm_barycentric_mode",
            "ordinary_pos_uv_state_tests",
            "churn_sbe_swiz_tests",
            "resident_point_width_raw",
            "mesa_sf_dw3",
            "mesa_clip_dw2",
            "mesa_sample_mask_dw",
            "mesa_wm_depth_stencil_dw1",
            "uses_host_point_tail_index_buffer",
            "point_raster_state_tests",
        )
    )
    shader_path = (ROOT / "src/intel/shader.rs").as_posix()
    return (
        "#![allow(dead_code, unfulfilled_lint_expectations)]\n"
        "mod intel {\n"
        f'    #[path = "{shader_path}"]\n'
        "    pub(crate) mod shader;\n"
        "    mod render {\n"
        + "\n".join(declarations)
        + "\n    }\n}\n"
    )


def main() -> None:
    verify_voxy_texture_composition_metadata()
    source = harness_source()
    with tempfile.TemporaryDirectory(prefix="trueos-clip-uv-tests-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        rust_source.write_text(source)
        subprocess.run(
            [
                "rustc",
                "--edition=2024",
                "--test",
                str(rust_source),
                "-o",
                str(executable),
            ],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
