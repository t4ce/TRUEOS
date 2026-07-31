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
BATTLE_SECTION = "scene/shape-battle-v1.bin"
BIGCLOTH_SECTION = "scene/pendulum-bigcloth-v1.bin"


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


def validate_scene_contracts(sections: dict[str, tuple[int, bytes]]) -> None:
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
    validate_manifest(section(sections, "manifest.json", 1))
    ir = section(sections, IR_SECTION, 6)
    validate_ir(ir)
    validate_replay(section(sections, REPLAY_SECTION, 7), ir)
    validate_native(sections)
    validate_scene_contracts(sections)
    print(f"validated {path} ({len(data)} bytes, {len(sections)} sections)")


if __name__ == "__main__":
    main()
