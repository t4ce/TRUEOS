#!/usr/bin/env python3
"""Strict stdlib-only validation for the runtime Helio cube artifact."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import struct
import sys
import zlib


HELIOA_MAGIC = b"HELIOA\0\0"
HELIOIR_MAGIC = b"HELIOIR\0"
HELIORP_MAGIC = b"HELIORP\0"
IR_SECTION = "render/ir-v1.bin"
REPLAY_SECTION = "render/replay-v1.bin"
CHURN_FORWARD_SECTION = "render/churn-forward-v1.bin"
CHURN_FORWARD_SOURCE = "render/churn-forward.wgsl"
CHURN_FORWARD_VS = "intel-xe-lp/churn-forward.vs.simd8.bin"
CHURN_FORWARD_PS = "intel-xe-lp/churn-forward.ps.simd8.bin"
CHURN_LIGHT_SECTION = "scene/churn-light-v1.bin"
BATTLE_SECTION = "scene/shape-battle-v1.bin"
BIGCLOTH_SECTION = "scene/pendulum-bigcloth-v1.bin"
RETAINED_TRANSFORM_SECTION = "scene/retained-transform-template-v1.bin"
RETAINED_TRANSFORM_MAGIC = b"HRTXFM\0\0"
RETAINED_TRANSFORM_HEADER_BYTES = 80
RETAINED_TRANSFORM_BYTES = 128
RETAINED_TRANSFORM_FLAGS = 0xF


def fail(message: str) -> "None":
    raise SystemExit(f"invalid Helio runtime artifact: {message}")


def u16(data: bytes, offset: int) -> int:
    if offset + 2 > len(data):
        fail("truncated u16")
    return struct.unpack_from("<H", data, offset)[0]


def u32(data: bytes, offset: int) -> int:
    if offset + 4 > len(data):
        fail("truncated u32")
    return struct.unpack_from("<I", data, offset)[0]


def checked_slice(data: bytes, offset: int, size: int, floor: int = 0) -> bytes:
    end = offset + size
    if offset < floor or end < offset or end > len(data):
        fail(f"out-of-range payload at {offset}+{size}")
    return data[offset:end]


def parse_helioa(data: bytes) -> dict[str, tuple[int, bytes]]:
    if len(data) < 32 or data[:8] != HELIOA_MAGIC:
        fail("bad HELIOA magic")
    version, header_size, count = struct.unpack_from("<HHI", data, 8)
    toc_size, payload_offset = struct.unpack_from("<QQ", data, 16)
    if version != 1 or header_size != 32 or payload_offset != 32 + toc_size:
        fail("unsupported HELIOA header")
    if payload_offset > len(data) or count > toc_size // 32:
        fail("impossible HELIOA table")

    cursor = 32
    sections: dict[str, tuple[int, bytes]] = {}
    ranges: list[tuple[int, int, str]] = []
    for _ in range(count):
        fixed = checked_slice(data, cursor, 32)
        name_size, kind, reserved = struct.unpack_from("<HHI", fixed, 0)
        offset, size = struct.unpack_from("<QQ", fixed, 8)
        checksum, reserved2 = struct.unpack_from("<II", fixed, 24)
        if reserved != 0 or reserved2 != 0:
            fail("nonzero HELIOA reserved field")
        name_bytes = checked_slice(data, cursor + 32, name_size)
        try:
            name = name_bytes.decode("utf-8")
        except UnicodeDecodeError:
            fail("non-UTF-8 section name")
        if not name or name.startswith("/") or ".." in name.split("/") or "\\" in name:
            fail(f"unsafe section name {name!r}")
        if name in sections:
            fail(f"duplicate section {name}")
        payload = checked_slice(data, offset, size, payload_offset)
        if zlib.crc32(payload) != checksum:
            fail(f"CRC mismatch in {name}")
        sections[name] = (kind, payload)
        ranges.append((offset, offset + size, name))
        cursor = (cursor + 32 + name_size + 7) & ~7

    if cursor != payload_offset:
        fail("HELIOA table length mismatch")
    ranges.sort()
    for previous, current in zip(ranges, ranges[1:]):
        if current[0] < previous[1]:
            fail(f"overlapping sections {previous[2]} and {current[2]}")
    return sections


def section(
    sections: dict[str, tuple[int, bytes]], name: str, expected_kind: int
) -> bytes:
    try:
        kind, data = sections[name]
    except KeyError:
        fail(f"missing section {name}")
    if kind != expected_kind:
        fail(f"wrong kind for {name}: {kind}")
    return data


def validate_manifest(data: bytes) -> None:
    try:
        manifest = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("malformed manifest.json")
    expected = {
        "schema": 1,
        "engine": "Helio",
        "program": "simple-cube",
        "graph": "helio_default_graphs::build_simple_graph",
        "target_api": "trueos-render",
        "target_architecture": "intel-xe-lp",
        "surface_format": "Bgra8UnormSrgb",
        "width": 320,
        "height": 180,
    }
    for key, value in expected.items():
        if manifest.get(key) != value:
            fail(f"manifest {key} is not {value!r}")
    slots = {slot.get("name"): slot.get("kind") for slot in manifest.get("dynamic_slots", [])}
    if slots != {
        "camera.view_proj": "mat4x4-f32",
        "output.surface": "ui4-bgra8-srgb",
    }:
        fail("unexpected dynamic-slot contract")


def validate_churn_manifest(data: bytes) -> None:
    try:
        manifest = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("malformed Churn manifest.json")
    expected = {
        "schema": 1,
        "engine": "Helio",
        "program": "churn-forward",
        "graph": "Helio ForwardLit-derived single pass",
        "capture": "wgpu-native-trace-v30",
        "target_api": "trueos-render",
        "target_architecture": "intel-xe-lp",
        "surface_format": "Bgra8UnormSrgb",
        "width": 320,
        "height": 180,
    }
    for key, value in expected.items():
        if manifest.get(key) != value:
            fail(f"Churn manifest {key} is not {value!r}")
    slots = {slot.get("name"): slot.get("kind") for slot in manifest.get("dynamic_slots", [])}
    if slots != {
        "camera": "libhelio::GpuCameraUniforms[1]",
        "scene.instances": "libhelio::GpuInstanceData[]",
        "scene.compacted_indices": "u32[]",
        "draw.indexed_indirect": "libhelio::DrawIndexedIndirectArgs",
        "output.surface": "ui4-bgra8-srgb-alpha",
    }:
        fail("unexpected Churn dynamic-slot contract")


def validate_churn_scene(sections: dict[str, tuple[int, bytes]]) -> None:
    scene = section(sections, "scene/churn-forward-v1.bin", 0xFFFF)
    if len(scene) != 7988 or scene[:8] != b"HCFWD1\0\0":
        fail("bad churn-forward seed image")
    if struct.unpack_from("<HHII", scene, 8) != (1, 96, 320, 180):
        fail("unsupported churn-forward seed header")
    layouts = tuple(struct.unpack_from("<II", scene, 20 + index * 8) for index in range(6))
    if layouts != ((24, 24), (4, 36), (368, 1), (208, 32), (4, 32), (20, 1)):
        fail("churn-forward seed ABI/count drift")
    offsets = struct.unpack_from("<6I", scene, 68)
    if offsets != (96, 672, 816, 1184, 7840, 7968):
        fail("churn-forward seed payload offsets changed")
    compacted = struct.unpack_from("<32I", scene, offsets[4])
    if compacted != tuple(range(32)):
        fail("churn-forward compacted indices are not the canonical identity list")
    if struct.unpack_from("<5I", scene, offsets[5]) != (36, 32, 0, 0, 0):
        fail("churn-forward canonical DrawIndexedIndirectArgs changed")


def validate_churn_light_only(sections: dict[str, tuple[int, bytes]]) -> None:
    light = section(sections, CHURN_LIGHT_SECTION, 0xFFFF)
    expected = bytearray(160)
    expected[:8] = b"HCHLIT\0\0"
    struct.pack_into("<HHIII", expected, 8, 1, 160, 160, 2, 4)
    struct.pack_into("<4f", expected, 24, 0.12, 0.12, 0.14, 1.0)
    struct.pack_into(
        "<8f", expected, 40, -20.0, 5.0, -20.0, 40.0, 0.8, 0.7, 0.55, 7.0
    )
    struct.pack_into(
        "<8f", expected, 72, 20.0, 5.0, 20.0, 40.0, 0.5, 0.7, 1.0, 7.0
    )
    struct.pack_into(
        "<8f", expected, 104, 0.65, 0.0, 0.60, 0.0, 0.70, 0.0, 0.15, 0.80
    )
    if light != bytes(expected):
        fail("churn-light payload changed")


def validate_retained_transform_template(
    sections: dict[str, tuple[int, bytes]],
) -> None:
    """Validate the complete pointer-free build-time affine fold contract."""
    data = section(sections, RETAINED_TRANSFORM_SECTION, 0xFFFF)
    if len(data) != RETAINED_TRANSFORM_BYTES or data[:8] != RETAINED_TRANSFORM_MAGIC:
        fail("bad retained-transform template header")

    version, header_bytes, *fields = struct.unpack_from("<HH17I", data, 8)
    (
        total_bytes,
        flags,
        affine_stride,
        root_affine_offset,
        root_affine_count,
        authored_constant_ops,
        constant_runs,
        emitted_constant_affines,
        folded_constant_ops,
        dynamic_children_per_row,
        max_render_rows,
        max_runtime_nodes,
        traversal_depth,
        root_node_index,
        dynamic_parent_node_index,
        dynamic_binding_kind,
        reserved,
    ) = fields
    if (version, header_bytes, total_bytes) != (
        1, RETAINED_TRANSFORM_HEADER_BYTES, RETAINED_TRANSFORM_BYTES,
    ):
        fail("unsupported retained-transform template version")
    if flags != RETAINED_TRANSFORM_FLAGS:
        fail("retained-transform pointer-free/fold/template flags changed")
    if (affine_stride, root_affine_offset, root_affine_count) != (48, 80, 1):
        fail("retained-transform root-affine layout changed")
    if (
        authored_constant_ops,
        constant_runs,
        emitted_constant_affines,
        folded_constant_ops,
    ) != (2, 1, 1, 1):
        fail("retained-transform constant-fold report changed")
    if (
        dynamic_children_per_row,
        max_render_rows,
        max_runtime_nodes,
        traversal_depth,
    ) != (1, 4096, 4097, 2):
        fail("retained-transform dynamic-row template changed")
    if (
        root_node_index,
        dynamic_parent_node_index,
        dynamic_binding_kind,
        reserved,
    ) != (0, 0, 1, 0):
        fail("retained-transform node/binding declaration changed")

    expected_identity = struct.pack(
        "<12f",
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
    )
    if data[root_affine_offset:] != expected_identity:
        fail("retained-transform folded root is not row-major 3x4 identity")


def ir_long(ir: bytes, offset_at: int, size_at: int) -> bytes:
    return checked_slice(ir, u32(ir, offset_at), u32(ir, size_at), 256)


def ir_short(ir: bytes, offset_at: int, size_at: int) -> bytes:
    return checked_slice(ir, u32(ir, offset_at), u16(ir, size_at), 256)


def validate_ir(ir: bytes) -> None:
    if len(ir) < 256 or ir[:8] != HELIOIR_MAGIC:
        fail("bad HELIOIR magic")
    if u16(ir, 8) != 1 or u16(ir, 10) != 256 or u32(ir, 12) != len(ir):
        fail("unsupported HELIOIR header")
    for field, actual, expected in [
        ("vertex id", u32(ir, 20), 1),
        ("vertex stride", u32(ir, 32), 36),
        ("index id", u32(ir, 36), 2),
        ("index format", u32(ir, 48), 1),
        ("camera id", u32(ir, 52), 3),
        ("camera size", u32(ir, 56), 192),
        ("color format", u32(ir, 92), 1),
        ("depth format", u32(ir, 96), 1),
        ("index count", u32(ir, 212), 36),
        ("instance count", u32(ir, 216), 1),
    ]:
        if actual != expected:
            fail(f"HELIOIR {field} is {actual}, expected {expected}")
    if len(ir_long(ir, 24, 28)) != 864 or len(ir_long(ir, 40, 44)) != 72:
        fail("HELIOIR cube buffer sizes changed")
    text_fields = [
        (68, 72, False, None),
        (60, 64, True, "camera.view_proj"),
        (76, 80, True, "vs_main"),
        (84, 88, True, "fs_main"),
        (232, 236, True, "output.surface"),
        (240, 244, True, "SimpleCube"),
    ]
    for offset_at, size_at, short, expected in text_fields:
        raw = ir_short(ir, offset_at, size_at) if short else ir_long(ir, offset_at, size_at)
        try:
            value = raw.decode("utf-8")
        except UnicodeDecodeError:
            fail("non-UTF-8 HELIOIR string")
        if expected is not None and value != expected:
            fail(f"unexpected HELIOIR string {value!r}")
    wgsl = ir_long(ir, 68, 72)
    if b"fn vs_main" not in wgsl or b"fn fs_main" not in wgsl:
        fail("HELIOIR has no captured SimpleCube shader")


def validate_replay(replay: bytes, ir: bytes) -> None:
    if len(replay) < 64 or replay[:8] != HELIORP_MAGIC:
        fail("bad HELIORP magic")
    version, header_size, total_size = struct.unpack_from("<HHI", replay, 8)
    command_count, command_stride, flags = struct.unpack_from("<III", replay, 16)
    if version != 1 or header_size != 64 or total_size != len(replay):
        fail("unsupported HELIORP header")
    if command_count != 1 or command_stride != 20:
        fail("HELIORP command layout does not match HELIOIR v1")
    if flags != 0 or any(replay[40:64]):
        fail("nonzero HELIORP flags or reserved bytes")
    if len(replay) != 64 + command_count * command_stride:
        fail("HELIORP length does not match command table")

    source_crc, vertex_id, index_id = struct.unpack_from("<III", replay, 28)
    if source_crc != (zlib.crc32(ir) & 0xFFFF_FFFF):
        fail("HELIORP source CRC does not match render/ir-v1.bin")
    if vertex_id != u32(ir, 20) or index_id != u32(ir, 36):
        fail("HELIORP resource IDs do not match render/ir-v1.bin")
    if vertex_id == 0 or index_id == 0 or vertex_id == index_id:
        fail("invalid HELIORP resource IDs")

    # This exact byte comparison is intentional: it proves the replay record
    # is the canonical 20-byte wgpu DrawIndexedIndirectArgs representation of
    # HELIOIR v1's draw, including the signed base_vertex bit pattern.
    if replay[64:84] != ir[212:232]:
        fail("HELIORP draw does not match render/ir-v1.bin")
    index_count, instance_count, first_index, _, first_instance = struct.unpack_from(
        "<IIIII", replay, 64
    )
    if index_count == 0 or instance_count == 0:
        fail("HELIORP contains an empty indexed draw")
    if first_index + index_count > 0xFFFF_FFFF:
        fail("HELIORP indexed range overflows u32")
    if first_instance + instance_count > 0xFFFF_FFFF:
        fail("HELIORP instance range overflows u32")


def validate_native(sections: dict[str, tuple[int, bytes]]) -> None:
    metadata_data = section(sections, "compiler/intel-xe-lp.json", 5)
    try:
        metadata = json.loads(metadata_data)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("malformed Intel compiler metadata")
    if metadata.get("schema") != 1 or metadata.get("producer") != "helio-intel-bake":
        fail("unsupported Intel compiler metadata")
    if metadata.get("compile_device", {}).get("vendor_id") != 0x8086:
        fail("native shaders were not compiled by an Intel device")

    stages = metadata.get("stages")
    if not isinstance(stages, list):
        fail("Intel stage list absent")
    seen: set[tuple[str, int]] = set()
    for stage in stages:
        stage_name = stage.get("stage")
        width = stage.get("simd_width")
        name = stage.get("section")
        if not isinstance(name, str):
            fail("Intel stage section absent")
        binary = section(sections, name, 4)
        if not binary or len(binary) % 4:
            fail(f"empty or unaligned native stage {name}")
        if stage.get("code_size_bytes") != len(binary):
            fail(f"native stage size mismatch for {name}")
        if stage.get("sha256") != hashlib.sha256(binary).hexdigest():
            fail(f"native stage hash mismatch for {name}")
        seen.add((stage_name, width))
    if ("vertex", 8) not in seen or ("fragment", 8) not in seen:
        fail("required SIMD8 VS/PS pair absent")


def validate_churn_forward(sections: dict[str, tuple[int, bytes]]) -> None:
    """Validate the binary-only ABI handoff and every referenced payload."""
    if CHURN_FORWARD_SECTION not in sections:
        if CHURN_FORWARD_SOURCE in sections:
            fail("captured Churn source has no native churn-forward-v1 descriptor")
        return
    data = section(sections, CHURN_FORWARD_SECTION, 8)
    if len(data) != 768 or data[:8] != b"HCFWD\0\0\0":
        fail("bad churn-forward descriptor header")
    if struct.unpack_from("<HHIIHHH", data, 8) != (1, 768, 768, 0x3F, 2, 3, 2):
        fail("unsupported churn-forward descriptor version")
    if any(data[26:32]):
        fail("nonzero churn-forward header reserved bytes")
    if struct.unpack_from("<10I", data, 32) != (
        368, 0, 64, 128, 192, 256, 272, 288, 304, 0,
    ):
        fail("churn-forward GpuCameraUniforms ABI drift")
    if struct.unpack_from("<12I", data, 72) != (
        208, 0, 64, 112, 128, 192, 196, 200, 204, 64, 48, 0,
    ):
        fail("churn-forward GpuInstanceData ABI drift")
    if struct.unpack_from("<10I", data, 120) != (
        4, 20, 0, 4, 8, 12, 16, 36, 0, 0,
    ):
        fail("churn-forward compacted/DrawIndexedIndirectArgs ABI drift")
    expected_vertex = bytearray(48)
    struct.pack_into("<IHH", expected_vertex, 0, 24, 1, 2)
    struct.pack_into("<HHII", expected_vertex, 8, 0, 1, 0, 0x7)
    struct.pack_into("<HHII", expected_vertex, 20, 1, 1, 12, 0x7)
    struct.pack_into("<HHI", expected_vertex, 32, 0, 0, 24)
    if data[160:208] != expected_vertex:
        fail("churn-forward vertex fetch/VF-mask contract drift")
    expected_bindings = bytearray(48)
    for offset, binding, bti, size in (
        (0, 0, 1, 368), (16, 1, 2, 208), (32, 2, 3, 4),
    ):
        struct.pack_into(
            "<BBBBBBHII", expected_bindings, offset,
            0, binding, bti, 1, 1, 1, 0, size, size,
        )
    if data[208:256] != expected_bindings:
        fail("churn-forward storage binding/BTI contract drift")
    fixed = struct.unpack_from("<8HIIHBBHH", data, 256)
    if fixed[:8] != (1,) * 8 or fixed[8:11] != (1, 0xF, 0):
        fail("churn-forward fixed-function state drift")
    if fixed[11:] != (1, 1, 2, 0):
        fail("invalid churn-forward SBE state")

    def stage_ref(
        offset: int, expected_stage: int, expected_entry: bytes, expected_name: str,
    ) -> None:
        values = struct.unpack_from("<HHIIIIHHHHHHHHIII", data, offset)
        (
            stage, simd, code_size, code_offset, alignment, ksp_offset,
            grf_start, grf_used, max_threads, bt_count, samplers, push_bytes,
            urb_length, varying_count, ps_flags, flat_inputs, reserved,
        ) = values
        if (
            stage != expected_stage or simd != 8 or not code_size
            or code_size % 4 or code_offset != 0 or alignment != 64
            or ksp_offset != 0
            or grf_start != (2 if expected_stage == 1 else 4)
            or grf_used != 128
            or max_threads != 64 or samplers or push_bytes or reserved
        ):
            fail(f"invalid churn-forward stage metadata for {expected_name}")
        if expected_stage == 1:
            if bt_count != 4 or urb_length != 1 or varying_count or ps_flags or flat_inputs:
                fail("invalid churn-forward VS payload metadata")
        elif bt_count != 1 or urb_length or varying_count != 2 \
                or ps_flags != 1 or flat_inputs != 2:
            fail("invalid churn-forward PS payload metadata")
        entry_len, name_len, reserved2 = struct.unpack_from("<HHI", data, offset + 80)
        if reserved2 or not entry_len or entry_len > 16 or not name_len or name_len > 56:
            fail("invalid churn-forward stage string lengths")
        entry = data[offset + 88:offset + 88 + entry_len]
        name_bytes = data[offset + 104:offset + 104 + name_len]
        if entry != expected_entry or name_bytes != expected_name.encode():
            fail("churn-forward stage reference changed")
        if any(data[offset + 88 + entry_len:offset + 104]) \
                or any(data[offset + 104 + name_len:offset + 160]):
            fail("nonzero churn-forward stage string padding")
        binary = section(sections, expected_name, 4)
        if len(binary) != code_size:
            fail(f"churn-forward stage size mismatch for {expected_name}")
        if hashlib.sha256(binary).digest() != data[offset + 48:offset + 80]:
            fail(f"churn-forward stage hash mismatch for {expected_name}")

    stage_ref(288, 1, b"vs_main", CHURN_FORWARD_VS)
    stage_ref(448, 2, b"fs_main", CHURN_FORWARD_PS)

    source_size, source_name_len, source_reserved = struct.unpack_from("<IHH", data, 608)
    if not source_size or source_reserved or not source_name_len or source_name_len > 56:
        fail("invalid churn-forward source reference")
    source_name = data[648:648 + source_name_len]
    if source_name != CHURN_FORWARD_SOURCE.encode() or any(data[648 + source_name_len:704]):
        fail("churn-forward source section name changed")
    source_data = section(sections, CHURN_FORWARD_SOURCE, 3)
    if len(source_data) != source_size:
        fail("churn-forward source size mismatch")
    if hashlib.sha256(source_data).digest() != data[616:648]:
        fail("churn-forward source hash mismatch")
    if struct.unpack_from("<III", data, 704) != (0xE002_4002, 0xB002_0002, 3):
        fail("churn-forward InstanceIndex SGVS contract drift")
    if struct.unpack_from("<HH", data, 716) != (3, 0):
        fail("churn-forward VF instancing count drift")
    if struct.unpack_from("<HBBI", data, 720) != (0, 0, 0, 0) \
            or struct.unpack_from("<HBBI", data, 728) != (1, 0, 0, 0) \
            or struct.unpack_from("<HBBI", data, 736) != (2, 0, 0, 0):
        fail("churn-forward per-element VF instancing contract drift")
    if struct.unpack_from("<HBBH4BH", data, 744) != (2, 31, 0, 135, 2, 2, 2, 2, 0):
        fail("churn-forward synthetic InstanceIndex vertex element drift")
    if struct.unpack_from("<IHH", data, 756) != (0x0000_0A77, 8, 1):
        fail("churn-forward VF component packing/URB input contract drift")
    if any(data[764:]):
        fail("nonzero churn-forward descriptor reserved bytes")


def validate_scene_contracts(sections: dict[str, tuple[int, bytes]]) -> None:
    light = section(sections, CHURN_LIGHT_SECTION, 0xFFFF)
    if len(light) != 160 or light[:8] != b"HCHLIT\0\0":
        fail("bad churn-light scene header")
    if u16(light, 8) != 1 or u16(light, 10) != 160 or u32(light, 12) != 160:
        fail("unsupported churn-light scene version")
    if (u32(light, 16), u32(light, 20)) != (2, 4):
        fail("churn-light count contract changed")

    # Provenance: Helio crates/examples/churn_benchmark.rs owns the two point
    # lights; crates/examples/churn_scene.rs owns ambient and the four material
    # roughness/metallic pairs. Validate the complete little-endian payload so
    # build-time drift cannot silently change TRUEOS's ported lighting contract.
    expected_light = bytearray(160)
    expected_light[:8] = b"HCHLIT\0\0"
    struct.pack_into("<HHIII", expected_light, 8, 1, 160, 160, 2, 4)
    struct.pack_into("<4f", expected_light, 24, 0.12, 0.12, 0.14, 1.0)
    struct.pack_into(
        "<8f", expected_light, 40, -20.0, 5.0, -20.0, 40.0, 0.8, 0.7, 0.55, 7.0
    )
    struct.pack_into(
        "<8f", expected_light, 72, 20.0, 5.0, 20.0, 40.0, 0.5, 0.7, 1.0, 7.0
    )
    struct.pack_into(
        "<8f", expected_light, 104, 0.65, 0.0, 0.60, 0.0, 0.70, 0.0, 0.15, 0.80
    )
    if light != bytes(expected_light):
        fail("churn-light payload changed")

    battle = section(sections, BATTLE_SECTION, 0xFFFF)
    if len(battle) != 320 or battle[:8] != b"HBATTLE\0":
        fail("bad shape-battle scene header")
    if u16(battle, 8) != 1 or u16(battle, 10) != 320 or u32(battle, 12) != 320:
        fail("unsupported shape-battle scene version")
    if tuple(u32(battle, offset) for offset in (16, 20, 24, 28, 32, 36)) != (
        4, 4, 16, 4, 4, 16,
    ):
        fail("shape-battle count contract changed")
    if any(battle[288:]):
        fail("nonzero shape-battle reserved bytes")

    cloth = section(sections, BIGCLOTH_SECTION, 0xFFFF)
    if len(cloth) != 192 or cloth[:8] != b"HPENDUL\0":
        fail("bad pendulum-bigcloth scene header")
    if u16(cloth, 8) != 1 or u16(cloth, 10) != 192 or u32(cloth, 12) != 192:
        fail("unsupported pendulum-bigcloth scene version")
    if tuple(u16(cloth, offset) for offset in (16, 18, 20, 22)) != (14, 24, 8, 0):
        fail("pendulum-bigcloth topology contract changed")
    if any(cloth[56:60]) or any(cloth[156:]):
        fail("nonzero pendulum-bigcloth reserved bytes")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} artifact.helio")
    path = Path(sys.argv[1])
    try:
        data = path.read_bytes()
    except OSError as error:
        raise SystemExit(f"cannot read {path}: {error}") from error
    sections = parse_helioa(data)
    manifest_data = section(sections, "manifest.json", 1)
    try:
        program = json.loads(manifest_data).get("program")
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("malformed manifest.json")
    if program == "churn-forward":
        validate_churn_manifest(manifest_data)
        validate_native(sections)
        validate_churn_forward(sections)
        validate_churn_scene(sections)
        validate_churn_light_only(sections)
        validate_retained_transform_template(sections)
        print(f"validated {path} ({len(data)} bytes, {len(sections)} sections)")
        return

    validate_manifest(manifest_data)
    ir = section(sections, IR_SECTION, 6)
    validate_ir(ir)
    validate_replay(section(sections, REPLAY_SECTION, 7), ir)
    validate_native(sections)
    validate_churn_forward(sections)
    validate_scene_contracts(sections)
    validate_retained_transform_template(sections)
    print(f"validated {path} ({len(data)} bytes, {len(sections)} sections)")


if __name__ == "__main__":
    main()
