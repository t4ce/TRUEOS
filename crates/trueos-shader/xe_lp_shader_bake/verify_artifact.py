#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import re
from pathlib import Path


NOTE_RE = re.compile(r'pub\(crate\) const TRIANGLE_PIPELINE_NOTE: &str = "([^"]*)";')
ARRAY_RE_TEMPLATE = r"static {name}: \[u32; (\d+)\] = \[(.*?)\];"
U32_RE = re.compile(r"0x[0-9A-Fa-f]{8}|\d+")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Validate a generated TRUEOS triangle shader artifact for structural "
            "correctness, provenance hashes, and expected trivial-triangle metadata."
        )
    )
    parser.add_argument(
        "generated",
        type=Path,
        help="Path to generated Rust artifact, e.g. .codex_tmp/generated_simple.rs",
    )
    parser.add_argument(
        "--dump-log",
        type=Path,
        help=(
            "Optional simple_triangle_dump stdout/stderr log. When provided, "
            "verification requires 'simple_triangle_dump: verified=1'."
        ),
    )
    parser.add_argument(
        "--require-verified",
        action="store_true",
        help="Fail unless TRIANGLE_PIPELINE_NOTE contains verified=1.",
    )
    return parser.parse_args()


def parse_note(source: str) -> str:
    match = NOTE_RE.search(source)
    if not match:
        raise SystemExit("missing TRIANGLE_PIPELINE_NOTE")
    return match.group(1)


def note_fields(note: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for token in note.split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        fields[key] = value
    return fields


def parse_u32_array(source: str, name: str) -> list[int]:
    pattern = re.compile(ARRAY_RE_TEMPLATE.format(name=re.escape(name)), re.S)
    match = pattern.search(source)
    if not match:
        raise SystemExit(f"missing array: {name}")
    declared_len = int(match.group(1))
    body = match.group(2)
    words = [int(token, 0) for token in U32_RE.findall(body)]
    if declared_len != len(words):
        raise SystemExit(
            f"{name} declared length {declared_len} does not match parsed word count {len(words)}"
        )
    return words


def parse_field_block(source: str, anchor: str) -> str:
    start = source.find(anchor)
    if start < 0:
        raise SystemExit(f"missing block anchor: {anchor}")
    brace_start = source.find("{", start)
    if brace_start < 0:
        raise SystemExit(f"missing '{{' after anchor: {anchor}")
    depth = 0
    for index in range(brace_start, len(source)):
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[brace_start : index + 1]
    raise SystemExit(f"unterminated block for anchor: {anchor}")


def parse_int_field(block: str, field: str) -> int:
    value = parse_field_value(block, field)
    return int(value, 0)


def parse_field_value(block: str, field: str) -> str:
    match = re.search(rf"\b{re.escape(field)}:\s*([^,\n]+)", block)
    if not match:
        raise SystemExit(f"missing field {field}")
    return match.group(1).strip()


def parse_bool_field(block: str, field: str) -> bool:
    match = re.search(rf"\b{re.escape(field)}:\s*(true|false)", block)
    if not match:
        raise SystemExit(f"missing boolean field {field}")
    return match.group(1) == "true"


def parse_dispatch_mode(block: str) -> str:
    match = re.search(r"\bdispatch_mode:\s*DispatchMode::(Simd8|Simd16|Simd32)", block)
    if not match:
        raise SystemExit("missing dispatch_mode")
    return match.group(1)


def words_to_bytes(words: list[int]) -> bytes:
    return b"".join(word.to_bytes(4, "little") for word in words)


def sha256_short(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()[:16]


def sha1_hex(data: bytes) -> str:
    return hashlib.sha1(data).hexdigest()


def stage_end(offset: int, size: int) -> int:
    end = offset + size
    if end < offset:
        raise SystemExit("integer overflow computing stage end")
    return end


def validate_stage(
    stage_name: str,
    block: str,
    words: list[int],
    expected_binding_entries: int,
) -> dict[str, int | str]:
    code_size = parse_int_field(block, "code_size_bytes")
    code_offset = parse_int_field(block, "code_offset_bytes")
    code_alignment = parse_int_field(block, "code_alignment_bytes")
    ksp_offset = parse_int_field(block, "ksp_offset_bytes")
    sampler_count = parse_int_field(block, "sampler_count")
    binding_entries = parse_int_field(block, "binding_table_entry_count")
    push_constant_bytes = parse_int_field(block, "push_constant_bytes")
    grf_used = parse_int_field(block, "grf_used")
    dispatch_mode = parse_dispatch_mode(block)

    if not words:
        raise SystemExit(f"{stage_name}: code array is empty")
    if code_size != len(words) * 4:
        raise SystemExit(
            f"{stage_name}: code_size_bytes={code_size} does not match code words={len(words)}"
        )
    if code_alignment == 0 or code_offset % code_alignment != 0:
        raise SystemExit(
            f"{stage_name}: code offset 0x{code_offset:X} is not aligned to {code_alignment}"
        )
    if ksp_offset % 64 != 0:
        raise SystemExit(f"{stage_name}: ksp_offset_bytes={ksp_offset} is not 64-byte aligned")
    if ksp_offset >= code_size:
        raise SystemExit(f"{stage_name}: ksp_offset_bytes={ksp_offset} is outside code size {code_size}")
    if grf_used <= 0:
        raise SystemExit(f"{stage_name}: grf_used must be nonzero")
    if binding_entries != expected_binding_entries:
        raise SystemExit(
            f"{stage_name}: expected binding_table_entry_count={expected_binding_entries}, got {binding_entries}"
        )
    if sampler_count != 0:
        raise SystemExit(f"{stage_name}: expected sampler_count=0, got {sampler_count}")
    if push_constant_bytes != 0:
        raise SystemExit(
            f"{stage_name}: expected push_constant_bytes=0, got {push_constant_bytes}"
        )

    return {
        "code_size": code_size,
        "code_offset": code_offset,
        "grf_used": grf_used,
        "dispatch_mode": dispatch_mode,
    }


def validate_dump_log(path: Path) -> None:
    text = path.read_text()
    if "simple_triangle_dump: verified=1" not in text:
        raise SystemExit(
            f"dump log does not prove host-side render correctness: {path}"
        )


def main() -> None:
    args = parse_args()
    source = args.generated.read_text()
    note = parse_note(source)
    fields = note_fields(note)

    vs_words = parse_u32_array(source, "TRIANGLE_VS_CODE")
    ps_words = parse_u32_array(source, "TRIANGLE_PS_CODE")
    vs_block = parse_field_block(source, "static TRIANGLE_VS: BakedVertexShader =")
    ps_block = parse_field_block(source, "static TRIANGLE_PS: BakedFragmentShader =")
    pipeline_block = parse_field_block(source, "static TRIANGLE_PIPELINE: TrianglePipeline =")

    vs_info = validate_stage("vs", vs_block, vs_words, expected_binding_entries=0)
    ps_info = validate_stage("ps", ps_block, ps_words, expected_binding_entries=1)

    vs_max_threads = parse_int_field(vs_block, "max_threads")
    vs_urb_length = parse_int_field(vs_block, "urb_entry_output_length")
    ps_num_varying_inputs = parse_int_field(ps_block, "num_varying_inputs")
    ps_flat_inputs = parse_int_field(ps_block, "flat_inputs")
    ps_uses_vmask = parse_bool_field(ps_block, "uses_vmask")
    ps_computed_depth_mode = parse_int_field(ps_block, "computed_depth_mode")
    ps_computed_stencil = parse_bool_field(ps_block, "computed_stencil")
    ps_persample_dispatch = parse_bool_field(ps_block, "persample_dispatch")
    vertex_stride_expr = parse_field_value(pipeline_block, "vertex_stride_bytes")
    vertex_count = parse_int_field(pipeline_block, "vertex_count")
    rt_binding_table_index = parse_int_field(pipeline_block, "rt_binding_table_index")

    if vs_max_threads <= 0:
        raise SystemExit("vs: max_threads must be nonzero")
    if vs_urb_length <= 0:
        raise SystemExit("vs: urb_entry_output_length must be nonzero")
    if ps_num_varying_inputs != 0:
        raise SystemExit(
            f"ps: expected num_varying_inputs=0 for trivial triangle, got {ps_num_varying_inputs}"
        )
    if ps_flat_inputs != 0:
        raise SystemExit(f"ps: expected flat_inputs=0, got {ps_flat_inputs}")
    if ps_uses_vmask:
        raise SystemExit("ps: expected uses_vmask=false")
    if ps_computed_depth_mode != 0:
        raise SystemExit(f"ps: expected computed_depth_mode=0, got {ps_computed_depth_mode}")
    if ps_computed_stencil:
        raise SystemExit("ps: expected computed_stencil=false")
    if ps_persample_dispatch:
        raise SystemExit("ps: expected persample_dispatch=false")
    expected_stride_exprs = {"12", "TRIANGLE_VERTEX_STRIDE_BYTES as u32"}
    if vertex_stride_expr not in expected_stride_exprs:
        raise SystemExit(
            "pipeline: expected vertex_stride_bytes to be 12 or "
            f"'TRIANGLE_VERTEX_STRIDE_BYTES as u32', got {vertex_stride_expr}"
        )
    if vertex_count != 3:
        raise SystemExit(f"pipeline: expected vertex_count=3, got {vertex_count}")
    if rt_binding_table_index != 0:
        raise SystemExit(
            f"pipeline: expected rt_binding_table_index=0, got {rt_binding_table_index}"
        )

    vs_end = stage_end(int(vs_info["code_offset"]), int(vs_info["code_size"]))
    ps_end = stage_end(int(ps_info["code_offset"]), int(ps_info["code_size"]))
    if int(vs_info["code_offset"]) < ps_end and int(ps_info["code_offset"]) < vs_end:
        raise SystemExit("shader code ranges overlap")

    vs_bytes = words_to_bytes(vs_words)
    ps_bytes = words_to_bytes(ps_words)
    vs_sha256 = sha256_short(vs_bytes)
    ps_sha256 = sha256_short(ps_bytes)
    vs_sha1 = sha1_hex(vs_bytes)
    ps_sha1 = sha1_hex(ps_bytes)
    if fields.get("vs_sha") not in {vs_sha256, vs_sha1}:
        raise SystemExit(
            "note vs_sha mismatch: "
            f"note={fields.get('vs_sha')} actual_sha1={vs_sha1} actual_sha256_16={vs_sha256}"
        )
    if fields.get("ps_sha") not in {ps_sha256, ps_sha1}:
        raise SystemExit(
            "note ps_sha mismatch: "
            f"note={fields.get('ps_sha')} actual_sha1={ps_sha1} actual_sha256_16={ps_sha256}"
        )
    if "target" not in fields:
        raise SystemExit("note is missing target=...")
    if "verified" not in fields:
        raise SystemExit("note is missing verified=...")
    if args.require_verified and fields["verified"] != "1":
        raise SystemExit("note says verified!=1")

    if args.dump_log is not None:
        validate_dump_log(args.dump_log)

    print(f"artifact ok: {args.generated}")
    print(
        "  note target={target} verified={verified} vs_sha={vs_sha} ps_sha={ps_sha}".format(
            target=fields["target"],
            verified=fields["verified"],
            vs_sha=fields["vs_sha"],
            ps_sha=fields["ps_sha"],
        )
    )
    print(
        "  vs offset=0x{vs_off:X} size={vs_size} dispatch={vs_dispatch} grf_used={vs_grf} max_threads={vs_threads}".format(
            vs_off=int(vs_info["code_offset"]),
            vs_size=int(vs_info["code_size"]),
            vs_dispatch=str(vs_info["dispatch_mode"]),
            vs_grf=int(vs_info["grf_used"]),
            vs_threads=vs_max_threads,
        )
    )
    print(
        "  ps offset=0x{ps_off:X} size={ps_size} dispatch={ps_dispatch} grf_used={ps_grf} varyings=0".format(
            ps_off=int(ps_info["code_offset"]),
            ps_size=int(ps_info["code_size"]),
            ps_dispatch=str(ps_info["dispatch_mode"]),
            ps_grf=int(ps_info["grf_used"]),
        )
    )
    if args.dump_log is not None:
        print(f"  dump log verified: {args.dump_log}")


if __name__ == "__main__":
    main()
