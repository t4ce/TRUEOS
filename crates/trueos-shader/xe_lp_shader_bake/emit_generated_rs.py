#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


DISPATCH_MODES = {"Simd8", "Simd16", "Simd32"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Package offline-baked Xe-LP triangle shaders into src/intel/shader/generated.rs"
    )
    parser.add_argument("--vs-bin", required=True, type=Path)
    parser.add_argument("--ps-bin", required=True, type=Path)
    parser.add_argument("--vs-meta", required=True, type=Path)
    parser.add_argument("--ps-meta", required=True, type=Path)
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("src/intel/shader/generated.rs"),
    )
    parser.add_argument(
        "--note-prefix",
        default="mesa-brw offline bake target=gfx125",
        help="Prefix for TRIANGLE_PIPELINE_NOTE provenance text",
    )
    parser.add_argument(
        "--verified",
        type=int,
        choices=(0, 1),
        default=0,
        help="Whether the baked metadata has been verified on hardware",
    )
    return parser.parse_args()


def load_bytes(path: Path) -> bytes:
    data = path.read_bytes()
    if len(data) % 4 != 0:
        raise SystemExit(f"binary size must be 4-byte aligned: {path}")
    return data


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise SystemExit(f"metadata root must be an object: {path}")
    return value


def require_int(meta: dict[str, Any], key: str) -> int:
    value = meta.get(key)
    if not isinstance(value, int):
        raise SystemExit(f"metadata key must be an integer: {key}")
    return value


def require_bool(meta: dict[str, Any], key: str) -> bool:
    value = meta.get(key)
    if not isinstance(value, bool):
        raise SystemExit(f"metadata key must be a boolean: {key}")
    return value


def require_dispatch_mode(meta: dict[str, Any]) -> str:
    value = meta.get("dispatch_mode")
    if value not in DISPATCH_MODES:
        raise SystemExit("dispatch_mode must be one of Simd8, Simd16, Simd32")
    return value


def words_from_bytes(data: bytes) -> list[int]:
    return [int.from_bytes(data[index : index + 4], "little") for index in range(0, len(data), 4)]


def rust_words(name: str, words: list[int]) -> str:
    if not words:
        return f"static {name}: [u32; 0] = [];"
    body = "\n".join(f"    0x{word:08X}," for word in words)
    return f"static {name}: [u32; {len(words)}] = [\n{body}\n];"


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()[:16]


def build_note(prefix: str, vs_data: bytes, ps_data: bytes, verified: int) -> str:
    return (
        f"{prefix} vs_sha={sha256_hex(vs_data)} ps_sha={sha256_hex(ps_data)} verified={verified}"
    )


def emit_kernel_meta(meta: dict[str, Any], code_size_bytes: int) -> str:
    return "\n".join(
        [
            "ShaderKernelMetadata {",
            f"            ksp_offset_bytes: {require_int(meta, 'ksp_offset_bytes')},",
            f"            code_offset_bytes: {require_int(meta, 'code_offset_bytes')},",
            f"            code_size_bytes: {code_size_bytes},",
            f"            code_alignment_bytes: {require_int(meta, 'code_alignment_bytes')},",
            f"            grf_start_register: {require_int(meta, 'grf_start_register')},",
            f"            dispatch_mode: DispatchMode::{require_dispatch_mode(meta)},",
            f"            sampler_count: {require_int(meta, 'sampler_count')},",
            f"            binding_table_entry_count: {require_int(meta, 'binding_table_entry_count')},",
            f"            push_constant_bytes: {require_int(meta, 'push_constant_bytes')},",
            f"            grf_used: {require_int(meta, 'grf_used')},",
            "        }",
        ]
    )


def emit_vs(meta: dict[str, Any], code_name: str, code_size_bytes: int) -> str:
    return "\n".join(
        [
            "static TRIANGLE_VS: BakedVertexShader = BakedVertexShader {",
            f"    code: &{code_name},",
            "    meta: VertexShaderMetadata {",
            f"        kernel: {emit_kernel_meta(meta, code_size_bytes).lstrip()},",
            f"        urb_entry_output_length: {require_int(meta, 'urb_entry_output_length')},",
            f"        max_threads: {require_int(meta, 'max_threads')},",
            "    },",
            "};",
        ]
    )


def emit_ps(meta: dict[str, Any], code_name: str, code_size_bytes: int) -> str:
    return "\n".join(
        [
            "static TRIANGLE_PS: BakedFragmentShader = BakedFragmentShader {",
            f"    code: &{code_name},",
            "    meta: FragmentShaderMetadata {",
            f"        kernel: {emit_kernel_meta(meta, code_size_bytes).lstrip()},",
            f"        num_varying_inputs: {require_int(meta, 'num_varying_inputs')},",
            f"        flat_inputs: {require_int(meta, 'flat_inputs')},",
            f"        uses_vmask: {str(require_bool(meta, 'uses_vmask')).lower()},",
            f"        computed_depth_mode: {require_int(meta, 'computed_depth_mode')},",
            f"        computed_stencil: {str(require_bool(meta, 'computed_stencil')).lower()},",
            f"        persample_dispatch: {str(require_bool(meta, 'persample_dispatch')).lower()},",
            "    },",
            "};",
        ]
    )


def emit_generated(vs_data: bytes, ps_data: bytes, vs_meta: dict[str, Any], ps_meta: dict[str, Any], note: str) -> str:
    vs_words = words_from_bytes(vs_data)
    ps_words = words_from_bytes(ps_data)
    return "\n".join(
        [
            "use super::{",
            "    BakedFragmentShader, BakedVertexShader, DispatchMode, FragmentShaderMetadata,",
            "    ShaderKernelMetadata, TRIANGLE_VERTEX_STRIDE_BYTES, TrianglePipeline, VertexShaderMetadata,",
            "};",
            "",
            "// @generated by tools/xe_lp_shader_bake/emit_generated_rs.py",
            "// See src/intel/shader/bake_format.md for the runtime contract.",
            "",
            f'pub(crate) const TRIANGLE_PIPELINE_NOTE: &str = "{note}";',
            "",
            rust_words("TRIANGLE_VS_CODE", vs_words),
            "",
            rust_words("TRIANGLE_PS_CODE", ps_words),
            "",
            emit_vs(vs_meta, "TRIANGLE_VS_CODE", len(vs_data)),
            "",
            emit_ps(ps_meta, "TRIANGLE_PS_CODE", len(ps_data)),
            "",
            "static TRIANGLE_PIPELINE: TrianglePipeline = TrianglePipeline {",
            "    vs: &TRIANGLE_VS,",
            "    ps: &TRIANGLE_PS,",
            "    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,",
            "    vertex_count: 3,",
            "    rt_binding_table_index: 0,",
            "};",
            "",
            "pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline {",
            "    &TRIANGLE_PIPELINE",
            "}",
            "",
        ]
    )


def main() -> None:
    args = parse_args()
    vs_data = load_bytes(args.vs_bin)
    ps_data = load_bytes(args.ps_bin)
    vs_meta = load_json(args.vs_meta)
    ps_meta = load_json(args.ps_meta)
    note = build_note(args.note_prefix, vs_data, ps_data, args.verified)
    generated = emit_generated(vs_data, ps_data, vs_meta, ps_meta, note)
    args.out.write_text(generated)


if __name__ == "__main__":
    main()