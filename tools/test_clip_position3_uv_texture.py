#!/usr/bin/env python3
"""Run the native clip-position/UV shader contract tests on the host.

The freestanding kernel has ``test = false``. Compile its real shader module
and the pure render helpers with their existing Rust tests, without linking
hardware code or maintaining a second implementation of the state encoding.
Run from any directory with ``python3 tools/test_clip_position3_uv_texture.py``.
"""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


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
        for name in ("SAMPLER_CACHE_LINE_DWORDS", "NEAREST_REPEAT_SAMPLER_STATE")
    )
    declarations.extend(
        item(pipeline, name)
        for name in (
            "write_nearest_repeat_sampler_cache_line",
            "ordinary_vf_vertex_element_count",
            "cmd_3dstate_vertex_elements",
            "mesa_vf_component_packing",
            "sbe_swiz_payload",
            "wm_barycentric_mode",
            "ordinary_pos_uv_state_tests",
            "churn_sbe_swiz_tests",
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
