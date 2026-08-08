#!/usr/bin/env python3
"""Bake the WGSL captured in a HELIOA file to Mesa/ANV gfx125 ISA.

This is intentionally a narrow bridge.  Naga is taken from Helio's vendored
wgpu tree and the Intel compilation/extraction path is the one already used by
TRUEOS's xe_lp_shader_bake proof.  No shader source is synthesized here.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import zlib


TRUEOS = Path(__file__).resolve().parents[2]
HELIO = TRUEOS.parent / "Helio"
NAGA_MANIFEST = HELIO / "vendor/wgpu/naga-cli/Cargo.toml"
UPSTREAM_DUMPER = (
    TRUEOS / "crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c"
)
UPSTREAM_EXTRACTOR = (
    TRUEOS / "crates/trueos-shader/xe_lp_shader_bake/extract_from_pipeline_cache.py"
)
MAGIC = b"HELIOA\0\0"
OTHER_SECTION_KIND = 0xFFFF
RENDER_IR_SECTION = "render/ir-v1.bin"
RENDER_IR_KIND = 6
REPLAY_SECTION = "render/replay-v1.bin"
REPLAY_KIND = 7
REPLAY_MAGIC = b"HELIORP\0"
REPLAY_HEADER_LEN = 64
REPLAY_COMMAND_STRIDE = 20
CHURN_FORWARD_SECTION = "render/churn-forward-v1.bin"
CHURN_FORWARD_KIND = 8
CHURN_FORWARD_SOURCE = "render/churn-forward.wgsl"
CHURN_FORWARD_VS = "intel-xe-lp/churn-forward.vs.simd8.bin"
CHURN_FORWARD_PS = "intel-xe-lp/churn-forward.ps.simd8.bin"
CHURN_FORWARD_MAGIC = b"HCFWD\0\0\0"
CHURN_FORWARD_BYTES = 768
CHURN_LIGHT_SECTION = "scene/churn-light-v1.bin"
CHURN_LIGHT_MAGIC = b"HCHLIT\0\0"
CHURN_LIGHT_BYTES = 160
SPRITE_DIG_SECTION = "scene/sprite-dig-v1.bin"
PORTAL_ROOMS_SECTION = "scene/portal-rooms-v1.bin"
RETAINED_TRANSFORM_SECTION = "scene/retained-transform-template-v1.bin"
RETAINED_TRANSFORM_MAGIC = b"HRTXFM\0\0"
RETAINED_TRANSFORM_HEADER_BYTES = 80
RETAINED_TRANSFORM_BYTES = 128
RETAINED_TRANSFORM_FLAGS = 0xF


def _affine3x4_mul(
    left: tuple[float, ...], right: tuple[float, ...]
) -> tuple[float, ...]:
    """Compose two row-major affine 3x4 matrices as left * right."""
    if len(left) != 12 or len(right) != 12:
        raise SystemExit("affine3x4 operands must contain 12 floats")
    product: list[float] = []
    for row in range(3):
        for column in range(3):
            product.append(sum(
                left[row * 4 + axis] * right[axis * 4 + column]
                for axis in range(3)
            ))
        product.append(
            left[row * 4 + 3]
            + sum(
                left[row * 4 + axis] * right[axis * 4 + 3]
                for axis in range(3)
            )
        )
    return tuple(product)


def encode_retained_transform_template() -> bytes:
    """Fold the canonical authored root and encode its row-child template."""
    identity = (
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
    )
    # This is the build-time fold stage in its smallest useful form: two
    # authored constant operations become one retained affine root. Dynamic
    # render rows are declared by the header and instantiated at runtime.
    authored_constant_ops = (identity, identity)
    folded = identity
    for operation in authored_constant_ops:
        folded = _affine3x4_mul(folded, operation)
    if folded != identity:
        raise SystemExit("canonical retained-transform identity fold changed")

    out = bytearray(RETAINED_TRANSFORM_BYTES)
    out[:8] = RETAINED_TRANSFORM_MAGIC
    struct.pack_into(
        "<HH17I",
        out,
        8,
        1,                                  # format version
        RETAINED_TRANSFORM_HEADER_BYTES,
        RETAINED_TRANSFORM_BYTES,
        RETAINED_TRANSFORM_FLAGS,           # pointer-free/3x4/row-child/folded
        48,                                 # affine stride
        RETAINED_TRANSFORM_HEADER_BYTES,    # root affine byte offset
        1,                                  # root affine count
        len(authored_constant_ops),         # authored constant-op count
        1,                                  # maximal constant-run count
        1,                                  # emitted constant-affine count
        len(authored_constant_ops) - 1,     # constant ops removed by folding
        1,                                  # dynamic children per render row
        4096,                               # maximum render rows
        4097,                               # maximum instantiated nodes
        2,                                  # maximum root-to-leaf traversal depth
        0,                                  # root node index
        0,                                  # dynamic-child parent node index
        1,                                  # dynamic binding kind: render-row index
        0,                                  # reserved
    )
    struct.pack_into("<12f", out, RETAINED_TRANSFORM_HEADER_BYTES, *folded)
    return bytes(out)


def encode_replay_plan(render_ir: bytes) -> bytes:
    """Mechanically lower HELIOIR v1's indexed draw to Helio replay-v1."""
    if len(render_ir) < 256 or render_ir[:8] != b"HELIOIR\0":
        raise SystemExit("cannot lower replay-v1 from malformed HELIOIR")
    version, header_len, total_len = struct.unpack_from("<HHI", render_ir, 8)
    if version != 1 or header_len != 256 or total_len != len(render_ir):
        raise SystemExit("cannot lower replay-v1 from unsupported HELIOIR header")

    vertex_buffer_id = struct.unpack_from("<I", render_ir, 20)[0]
    index_buffer_id = struct.unpack_from("<I", render_ir, 36)[0]
    if (
        vertex_buffer_id == 0
        or index_buffer_id == 0
        or vertex_buffer_id == index_buffer_id
    ):
        raise SystemExit("cannot lower replay-v1 with invalid HELIOIR resource IDs")

    # HELIOIR v1 stores its one DrawIndexed call as the canonical five fields
    # at 212..232. Copying these bytes preserves wgpu's signed base_vertex bit
    # pattern and keeps this lowering independent of scene semantics.
    draw = render_ir[212:232]
    if len(draw) != REPLAY_COMMAND_STRIDE:
        raise SystemExit("cannot lower replay-v1 from truncated HELIOIR draw")
    index_count, instance_count, first_index, _, first_instance = struct.unpack(
        "<IIIII", draw
    )
    if (
        index_count == 0
        or instance_count == 0
        or first_index + index_count > 0xFFFF_FFFF
        or first_instance + instance_count > 0xFFFF_FFFF
    ):
        raise SystemExit("cannot lower replay-v1 from invalid HELIOIR draw")

    total_len = REPLAY_HEADER_LEN + REPLAY_COMMAND_STRIDE
    out = bytearray(total_len)
    out[:8] = REPLAY_MAGIC
    struct.pack_into(
        "<HHIIIIIII",
        out,
        8,
        1,
        REPLAY_HEADER_LEN,
        total_len,
        1,
        REPLAY_COMMAND_STRIDE,
        0,
        zlib.crc32(render_ir) & 0xFFFF_FFFF,
        vertex_buffer_id,
        index_buffer_id,
    )
    out[REPLAY_HEADER_LEN:] = draw
    return bytes(out)


def _put_f32s(out: bytearray, offset: int, values: tuple[float, ...]) -> None:
    struct.pack_into("<" + "f" * len(values), out, offset, *values)


def encode_churn_light_scene() -> bytes:
    """Encode the hosted Helio churn benchmark's lighting/material contract."""
    # Provenance: Helio crates/examples/churn_benchmark.rs installs these two
    # point lights, while crates/examples/churn_scene.rs owns the ambient and
    # four material roughness/metallic values. Keep this build-time handoff
    # separate from TRUEOS runtime policy and pointer-free in the artifact.
    out = bytearray(CHURN_LIGHT_BYTES)
    out[:8] = CHURN_LIGHT_MAGIC
    struct.pack_into("<HHI", out, 8, 1, CHURN_LIGHT_BYTES, CHURN_LIGHT_BYTES)
    struct.pack_into("<II", out, 16, 2, 4)
    _put_f32s(out, 24, (0.12, 0.12, 0.14, 1.0))
    _put_f32s(out, 40, (-20.0, 5.0, -20.0, 40.0, 0.8, 0.7, 0.55, 7.0))
    _put_f32s(out, 72, (20.0, 5.0, 20.0, 40.0, 0.5, 0.7, 1.0, 7.0))
    _put_f32s(out, 104, (0.65, 0.0, 0.60, 0.0, 0.70, 0.0, 0.15, 0.80))
    return bytes(out)


def _encode_churn_stage(
    out: bytearray,
    offset: int,
    *,
    stage: int,
    code: bytes,
    entry_point: str,
    section_name: str,
) -> None:
    """Write one fixed-size native stage reference for churn-forward-v1."""
    entry = entry_point.encode("ascii")
    name = section_name.encode("ascii")
    if not code or len(code) % 4 or len(entry) > 16 or len(name) > 56:
        raise SystemExit("invalid churn-forward native stage")
    # ShaderKernelMetadata fields consumed by TRUEOS.  The captured ANV Churn
    # packet launches the SIMD8 VS at r2 and the SIMD8 PS at r4.  Keep those
    # compiler-authored payload starts with the extracted ISA instead of
    # applying one stage-agnostic default.
    struct.pack_into(
        "<HHIIIIHHHHHHHHIII",
        out,
        offset,
        stage,
        8,                  # dispatch width
        len(code),
        0,                  # code offset inside the named section
        64,                 # required upload alignment
        0,                  # KSP offset from uploaded code base
        2 if stage == 1 else 4,  # dispatch GRF start
        128,                # GRF allocation envelope
        64,                 # max threads
        4 if stage == 1 else 1,
        0,                  # samplers
        0,                  # push constants
        1 if stage == 1 else 0,  # VS URB output length
        0 if stage == 1 else 2,  # world normal and flat material ID
        0 if stage == 1 else 1,  # PS uses VMASK
        0 if stage == 1 else 2,  # material_id is flat location 1
        0,
    )
    out[offset + 48:offset + 80] = hashlib.sha256(code).digest()
    struct.pack_into("<HHI", out, offset + 80, len(entry), len(name), 0)
    out[offset + 88:offset + 88 + len(entry)] = entry
    out[offset + 104:offset + 104 + len(name)] = name


def encode_churn_forward_program(wgsl: bytes, vs: bytes, ps: bytes) -> bytes:
    """Encode Helio's pointer-free Churn transform/indirect/native ABI."""
    for marker in (
        b"struct GpuInstanceData",
        b"compacted_indices",
        b"@group(0) @binding(0)",
        b"@group(0) @binding(1)",
        b"@group(0) @binding(2)",
        b"fn vs_main",
        b"fn fs_main",
    ):
        if marker not in wgsl:
            raise SystemExit(f"churn-forward WGSL lacks required marker: {marker!r}")

    out = bytearray(CHURN_FORWARD_BYTES)
    out[:8] = CHURN_FORWARD_MAGIC
    struct.pack_into(
        "<HHIIHHH",
        out,
        8,
        1,
        CHURN_FORWARD_BYTES,
        CHURN_FORWARD_BYTES,
        0x3F,
        2,
        3,
        2,
    )

    # Exact current Helio GpuCameraUniforms layout (the old 256-byte comment in
    # camera.rs is stale; the eight fields below occupy 368 bytes).
    struct.pack_into("<10I", out, 32, 368, 0, 64, 128, 192, 256, 272, 288, 304, 0)
    # Exact current Helio GpuInstanceData layout and member sizes.
    struct.pack_into(
        "<12I", out, 72,
        208, 0, 64, 112, 128, 192, 196, 200, 204, 64, 48, 0,
    )
    # compacted u32 IDs followed by wgpu DrawIndexedIndirectArgs offsets;
    # the captured cube draw has 36 indices and zero base/first instance.
    struct.pack_into("<10I", out, 120, 4, 20, 0, 4, 8, 12, 16, 36, 0, 0)

    struct.pack_into("<IHH", out, 160, 24, 1, 2)  # Uint32 indices, two attrs
    struct.pack_into("<HHII", out, 168, 0, 1, 0, 0x7)   # position Float32x3
    struct.pack_into("<HHII", out, 180, 1, 1, 12, 0x7)  # normal Float32x3
    struct.pack_into("<HHI", out, 192, 0, 0, 24)        # VB0, per-vertex

    # ANV reserves BTI0; the three vertex-stage read-only storage buffers map
    # directly to BTI1..3 in bind-group order.
    for offset, binding, bti, size in (
        (208, 0, 1, 368),
        (224, 1, 2, 208),
        (240, 2, 3, 4),
    ):
        struct.pack_into(
            "<BBBBBBHII", out, offset,
            0, binding, bti, 1, 1, 1, 0, size, size,
        )

    # triangle-list, CCW, back-cull, BGRA8 sRGB, Depth32Float, Less, Uint32,
    # sample1; depth writes on, blend off, RGBA writes. SBE reads the two
    # forward varyings after the position header.
    struct.pack_into("<8HIIHBBHH", out, 256, *([1] * 8), 1, 0xF, 0, 1, 1, 2, 0)

    _encode_churn_stage(
        out, 288, stage=1, code=vs, entry_point="vs_main",
        section_name=CHURN_FORWARD_VS,
    )
    _encode_churn_stage(
        out, 448, stage=2, code=ps, entry_point="fs_main",
        section_name=CHURN_FORWARD_PS,
    )
    source_name = CHURN_FORWARD_SOURCE.encode("ascii")
    struct.pack_into("<IHH", out, 608, len(wgsl), len(source_name), 0)
    out[616:648] = hashlib.sha256(wgsl).digest()
    out[648:648 + len(source_name)] = source_name
    # Pinned Mesa genX_shader.c appends id_slot=2 for InstanceIndex. On gfx125
    # this packs InstanceID+BaseInstance as the exact SGVS words below, backed
    # by a synthetic VB31 R32G32_UINT element with STORE_0 component controls.
    struct.pack_into("<III", out, 704, 0xE002_4002, 0xB002_0002, 3)
    struct.pack_into("<HH", out, 716, 3, 0)
    struct.pack_into("<HBBI", out, 720, 0, 0, 0, 0)
    struct.pack_into("<HBBI", out, 728, 1, 0, 0, 0)
    struct.pack_into("<HBBI", out, 736, 2, 0, 0, 0)
    struct.pack_into("<HBBH4BH", out, 744, 2, 31, 0, 135, 2, 2, 2, 2, 0)
    # brw_nir_pack_vs_input packs position.xyz (7), normal.xyz (7), then the
    # synthetic base_instance.y + instance_id.w components (A): eight inputs.
    struct.pack_into("<IHH", out, 756, 0x0000_0A77, 8, 1)
    return bytes(out)


def encode_shape_battle_scene() -> bytes:
    """Encode the stable no_std port contract for shape_battle_royale.rs."""
    out = bytearray(320)
    out[:8] = b"HBATTLE\0"
    struct.pack_into("<HHI", out, 8, 1, len(out), len(out))
    struct.pack_into("<IIIII", out, 16, 4, 4, 16, 4, 4)
    struct.pack_into("<I", out, 36, 16)
    struct.pack_into("<Q", out, 40, 0x4BA7_71E5_2026_0801)
    _put_f32s(out, 48, (0.0, 16.0, 32.0, 0.0, -0.45,
                       0.7853981633974483, 0.1, 200.0))
    _put_f32s(out, 80, (0.0, 0.0, 0.0, 0.0))
    _put_f32s(out, 96, (0.15, 0.15, 0.18, 1.0))
    _put_f32s(out, 112, (17.5, 6.0, 1.0, 0.95))
    for index, rgba in enumerate((
        (0.84, 0.14, 0.14, 1.0),
        (0.18, 0.85, 0.25, 1.0),
        (0.20, 0.38, 0.90, 1.0),
        (0.95, 0.85, 0.17, 1.0),
    )):
        _put_f32s(out, 128 + index * 16, rgba)
    for index, extents in enumerate((
        (0.40, 0.40, 0.40),
        (0.35, 0.55, 0.25),
        (0.35, 0.55, 0.35),
        (0.30, 0.60, 0.30),
    )):
        _put_f32s(out, 192 + index * 12, extents)
    _put_f32s(out, 240, (0.45, 0.50, 0.75, 0.66))
    _put_f32s(out, 256, (
        1.0 / 60.0, 16.0, 2.0, -9.81, 0.8, 4.0,
    ))
    struct.pack_into("<II", out, 280, 42, 120)
    return bytes(out)


def encode_pendulum_bigcloth_scene() -> bytes:
    """Encode the stable no_std port contract for rapier_pendulum_bigcloth.rs."""
    out = bytearray(192)
    out[:8] = b"HPENDUL\0"
    struct.pack_into("<HHI", out, 8, 1, len(out), len(out))
    struct.pack_into("<HHHH", out, 16, 14, 24, 8, 0)
    # The hosted demo camera starts below the y=18 cloth and looks down, so
    # its entire first frame is above the viewport. Center the artifact camera
    # on the authored x=1..24, y=0..18 motion envelope instead.
    _put_f32s(out, 24, (12.5, 9.0, 42.0, 0.0, 0.0,
                       0.7853981633974483, 0.1, 300.0))
    _put_f32s(out, 60, (
        1.35, 1.0, 18.0, 0.4, 0.8, -9.81, 1.0 / 60.0,
        0.995, -0.2, 0.4, 0.2,
    ))
    _put_f32s(out, 104, (0.20, 0.50, 0.80, 1.0))
    _put_f32s(out, 120, (0.25, 0.25, 0.30, 1.0))
    struct.pack_into("<f", out, 136, 50.0)
    _put_f32s(out, 140, (0.0, 0.0, 0.0, 0.0))
    return bytes(out)


def encode_sprite_dig_scene() -> bytes:
    """Encode the stable no_std gameplay contract for sprite_dig_demo.rs."""
    out = bytearray(256)
    out[:8] = b"HDIG2D\0\0"
    struct.pack_into("<HHI", out, 8, 1, len(out), len(out))
    # WORLD_COLS, DIRT_ROWS, STONE_ROWS, POOL_CAPACITY, hotbar/placed caps,
    # and the authored lake band. The first four values are copied directly
    # from Helio's demo; the two caps bound TRUEOS-owned dynamic state.
    struct.pack_into("<8H", out, 16, 240, 8, 14, 7500, 8, 64, 42, 50)
    _put_f32s(out, 32, (
        48.0, 1.5, 2.0, -2400.0, 260.0, 780.0, 0.22,
        56.0, 40.0, 46.0, 100.0,
    ))
    struct.pack_into("<I", out, 76, 3)
    # Retained color-quad visualization for the atlas-backed hosted scene:
    # grass, water, dirt, stone, placed block, player, crack overlay, three
    # hotbar material icons, and selected-slot highlight.
    for index, rgba in enumerate((
        (0.32, 0.55, 0.12, 1.0),
        (0.12, 0.45, 0.65, 1.0),
        (0.22, 0.17, 0.07, 1.0),
        (0.35, 0.37, 0.40, 1.0),
        (0.70, 0.50, 0.20, 1.0),
        (0.95, 0.65, 0.12, 1.0),
        (0.80, 0.12, 0.08, 0.75),
        (0.45, 0.72, 0.18, 1.0),
        (0.42, 0.30, 0.12, 1.0),
        (0.58, 0.61, 0.66, 1.0),
        (1.00, 0.85, 0.20, 0.45),
    )):
        _put_f32s(out, 80 + index * 16, rgba)
    return bytes(out)


def encode_portal_rooms_scene() -> bytes:
    """Encode portal_rooms.rs's texture-free portals, rooms, and furniture."""
    hub_half = 6.0
    wall_t = 0.15
    room_half = 6.0
    portals = (
        ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        ((-1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        ((0.0, 1.0, 0.0), (0.0, 0.0, 1.0)),
        ((0.0, -1.0, 0.0), (0.0, 0.0, 1.0)),
        ((0.0, 0.0, 1.0), (0.0, 1.0, 0.0)),
        ((0.0, 0.0, -1.0), (0.0, 1.0, 0.0)),
    )
    themes = (
        ((0.55, 0.12, 0.08, 1.0), (1.0, 0.35, 0.1, 1.0)),
        ((0.08, 0.4, 0.14, 1.0), (0.25, 1.0, 0.35, 1.0)),
        ((0.55, 0.45, 0.05, 1.0), (1.0, 0.85, 0.15, 1.0)),
        ((0.05, 0.1, 0.4, 1.0), (0.2, 0.4, 1.0, 1.0)),
        ((0.42, 0.08, 0.48, 1.0), (0.9, 0.25, 1.0, 1.0)),
        ((0.08, 0.4, 0.45, 1.0), (0.2, 0.95, 1.0, 1.0)),
    )
    # material: linear RGBA plus an emissive contribution used by the
    # retained compatibility lighting. IDs 0/1 are shared wood/metal;
    # each room then owns its wall/base and accent pair.
    materials = [
        ((0.32, 0.2, 0.11, 1.0), 0.0),
        ((0.5, 0.51, 0.54, 1.0), 0.0),
    ]
    for base, accent in themes:
        materials.extend(((base, 0.0), (accent, 0.8)))

    # object: portal, material, shape (0 box / 1 octa-sphere), local center
    # (right, height relative to room center, depth from portal), half extent.
    objects: list[tuple[int, int, int, tuple[float, ...], tuple[float, ...]]] = []
    def add(portal: int, material: int, shape: int, rx: float, height: float,
            depth: float, hr: float, hu: float, hn: float) -> None:
        objects.append((portal, material, shape, (rx, height - room_half, depth), (hr, hu, hn)))

    for portal in range(6):
        base = 2 + portal * 2
        # Five real shell panels; the entrance at depth zero stays open.
        add(portal, base, 0, -room_half, room_half, room_half, wall_t, room_half, room_half)
        add(portal, base, 0, room_half, room_half, room_half, wall_t, room_half, room_half)
        add(portal, base, 0, 0.0, 0.0, room_half, room_half, wall_t, room_half)
        add(portal, base, 0, 0.0, 2.0 * room_half, room_half, room_half, wall_t, room_half)
        add(portal, base, 0, 0.0, room_half, 2.0 * room_half, room_half, room_half, wall_t)

    # Furniture copied from portal_rooms.rs's six furnish_room branches.
    b, s = 0, 1
    base, accent = 2, 3
    for args in (
        (0, 0, b, -1.5, .7, 8.5, 2.4, .7, 3.0), (0, base, b, -1.5, 1.5, 8.5, 2.2, .3, 2.8),
        (0, base, b, -1.5, 1.9, 10.3, 1.0, .25, .7), (0, 0, b, 1.5, .5, 9.5, .6, .5, .6),
        (0, accent, s, 1.5, 1.4, 9.5, .4, .4, .4), (0, 0, b, -4.8, 1.8, 2.2, .8, 1.8, 1.0),
        (0, base, b, -1.0, .04, 6.0, 2.6, .04, 3.2),
    ):
        add(*args)
    base, accent = 4, 5
    add(1, 0, b, 0., .5, 5., 1.8, .5, .9)
    for rx in (-2.5, 0.0, 2.5):
        add(1, 0, b, rx, .5, 10.5, .6, .5, .6); add(1, accent, s, rx, 1.4, 10.5, .55, .55, .55)
    add(1, 0, b, -4.5, .4, 2., .5, .4, 1.8)
    base, accent = 6, 7
    for args in (
        (2, 1, b, 0., .7, 10.5, 4.5, .7, .8), (2, 1, b, -1.5, 1.55, 10.5, .8, .15, .6),
        (2, accent, s, -1.8, 1.72, 10.5, .16, .16, .16), (2, accent, s, -1.2, 1.72, 10.5, .16, .16, .16),
        (2, 0, b, 0., .75, 5., 1.5, .1, 1.5), (2, 0, b, -2., .4, 5., .45, .4, .45),
        (2, 0, b, 2., .4, 5., .45, .4, .45), (2, accent, s, 0., 11., 6., .5, .5, .5),
    ):
        add(*args)
    base, accent = 8, 9
    add(3, 0, b, 0., 3., 11.3, 4.5, 3., .5)
    for index, rx in enumerate((-3., -1.5, 0., 1.5, 3.)):
        add(3, accent if index % 2 == 0 else base, b, rx, 4.2, 11., .4, .9, .15)
    add(3, 0, b, 0., .75, 4.5, 1.6, .1, .9); add(3, base, b, 0., .45, 3., .5, .45, .5)
    add(3, accent, s, 1.2, 1.5, 4.5, .3, .3, .3)
    base, accent = 10, 11
    for args in (
        (4, base, b, 0., .45, 9.5, 2.6, .45, 1.), (4, base, b, 0., 1.2, 10.4, 2.6, .5, .25),
        (4, accent, b, 0., 3.4, 11.7, 1.8, 1., .12), (4, 0, b, 0., .4, 6.5, 1.2, .15, .7),
        (4, 1, b, -4.5, 1.9, 4.5, .1, 1.9, .1), (4, accent, s, -4.5, 4., 4.5, .5, .5, .5),
    ):
        add(*args)
    base, accent = 12, 13
    for args in (
        (5, 1, b, 0., .55, 8.5, 1.8, .55, 1.3), (5, 1, b, -4., .75, 2.5, 1., .1, .7),
        (5, accent, b, -4., 2., 11.7, .8, .9, .12), (5, 0, b, 3., .35, 3.5, .4, .35, .4),
        (5, accent, s, 3.5, 1.1, 7., .4, .4, .4), (5, accent, s, -3.5, 1.1, 8., .35, .35, .35),
    ):
        add(*args)

    header_bytes, portal_bytes, material_bytes, object_bytes = 64, 32, 32, 32
    total = header_bytes + len(portals) * portal_bytes + len(materials) * material_bytes + len(objects) * object_bytes
    out = bytearray(total)
    out[:8] = b"HPORTAL\0"
    struct.pack_into("<HHI4H", out, 8, 1, header_bytes, total, len(portals), len(materials), len(objects), object_bytes)
    _put_f32s(out, 24, (hub_half, wall_t, room_half, 4.0, 0.002, 0.7853981633974483, 0.1, 300.0))
    portal_offset = header_bytes
    material_offset = portal_offset + len(portals) * portal_bytes
    object_offset = material_offset + len(materials) * material_bytes
    # Table offsets are derived from the fixed header/record strides. Keep
    # the final header words reserved so malformed layouts cannot redirect a
    # no_std decoder into unrelated container bytes.
    struct.pack_into("<II", out, 56, 0, 0)
    for index, (normal, up) in enumerate(portals):
        offset = portal_offset + index * portal_bytes
        _put_f32s(out, offset, normal + up)
        struct.pack_into("<HHI", out, offset + 24, 2 + index * 2, 3 + index * 2, 0)
    for index, (color, emissive) in enumerate(materials):
        offset = material_offset + index * material_bytes
        _put_f32s(out, offset, color + (emissive,))
    for index, (portal, material, shape, center, half) in enumerate(objects):
        offset = object_offset + index * object_bytes
        struct.pack_into("<HHB3x", out, offset, portal, material, shape)
        _put_f32s(out, offset + 8, center + half)
    return bytes(out)


def run(
    command: list[str], *, env: dict[str, str] | None = None,
    log: Path | None = None, cwd: Path | None = None,
) -> None:
    print("+", " ".join(command))
    result = subprocess.run(
        command,
        env=env,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if log:
        log.write_text(result.stdout)
    else:
        sys.stdout.write(result.stdout)
    if result.returncode:
        if log:
            sys.stderr.write(result.stdout)
        raise SystemExit(result.returncode)


def parse_helioa(raw: bytes) -> dict[str, tuple[int, bytes]]:
    if len(raw) < 32 or raw[:8] != MAGIC:
        raise SystemExit("input is not a HELIOA artifact")
    version, header_len, count = struct.unpack_from("<HHI", raw, 8)
    toc_len, payload = struct.unpack_from("<QQ", raw, 16)
    if version != 1 or header_len != 32 or payload != 32 + toc_len or payload > len(raw):
        raise SystemExit("unsupported or malformed HELIOA header")
    cursor = 32
    sections: dict[str, tuple[int, bytes]] = {}
    for _ in range(count):
        if cursor + 32 > payload:
            raise SystemExit("truncated HELIOA table")
        name_len, kind = struct.unpack_from("<HH", raw, cursor)
        offset, size = struct.unpack_from("<QQ", raw, cursor + 8)
        crc = struct.unpack_from("<I", raw, cursor + 24)[0]
        name_end = cursor + 32 + name_len
        if name_end > payload or offset < payload or offset + size > len(raw):
            raise SystemExit("out-of-range HELIOA section")
        name = raw[cursor + 32:name_end].decode("utf-8")
        data = raw[offset:offset + size]
        if zlib.crc32(data) != crc:
            raise SystemExit(f"checksum mismatch in {name}")
        if name in sections:
            raise SystemExit(f"duplicate HELIOA section {name}")
        sections[name] = (kind, data)
        cursor = (name_end + 7) & ~7
    if cursor != payload or "manifest.json" not in sections:
        raise SystemExit("malformed HELIOA table")
    return sections


def emit_helioa(sections: dict[str, tuple[int, bytes]]) -> bytes:
    ordered = sorted(sections.items())
    toc_len = sum((32 + len(name.encode()) + 7) & ~7 for name, _ in ordered)
    payload = 32 + toc_len
    total = payload + sum(len(data) for _, (_, data) in ordered)
    out = bytearray(total)
    out[:8] = MAGIC
    struct.pack_into("<HHIQQ", out, 8, 1, 32, len(ordered), toc_len, payload)
    toc_cursor = 32
    data_cursor = payload
    for name, (kind, data) in ordered:
        encoded = name.encode("utf-8")
        struct.pack_into(
            "<HHIQQII", out, toc_cursor, len(encoded), kind, 0,
            data_cursor, len(data), zlib.crc32(data), 0,
        )
        out[toc_cursor + 32:toc_cursor + 32 + len(encoded)] = encoded
        toc_cursor = (toc_cursor + 32 + len(encoded) + 7) & ~7
        out[data_cursor:data_cursor + len(data)] = data
        data_cursor += len(data)
    return bytes(out)


def captured_wgsl(sections: dict[str, tuple[int, bytes]]) -> tuple[str, bytes]:
    candidates = [
        (name, data) for name, (_, data) in sections.items()
        if name != CHURN_FORWARD_SOURCE
        and name.endswith(".wgsl")
        and b"@vertex" in data
        and b"@fragment" in data
    ]
    if len(candidates) != 1:
        raise SystemExit(f"expected one captured vertex+fragment WGSL section, found {len(candidates)}")
    name, data = candidates[0]
    text = data.decode("utf-8")
    for required in ("fn vs_main", "fn fs_main", "@location(2)", "@binding(0)"):
        if required not in text:
            raise SystemExit(f"captured WGSL lacks required SimpleCube marker: {required}")
    return name, data


def replace_once(source: str, old: str, new: str) -> str:
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"upstream dumper drift: expected one replacement, found {count}: {old[:50]!r}")
    return source.replace(old, new)


def make_compile_only_dumper(destination: Path) -> None:
    source = UPSTREAM_DUMPER.read_text()
    first = '.pName = "main",'
    pos = source.find(first)
    if pos < 0:
        raise SystemExit("upstream dumper drift: vertex entry point not found")
    source = source[:pos] + source[pos:].replace(first, '.pName = "vs_main",', 1)
    pos = source.find(first, pos + 1)
    if pos < 0:
        raise SystemExit("upstream dumper drift: fragment entry point not found")
    source = source[:pos] + source[pos:].replace(first, '.pName = "fs_main",', 1)

    source = replace_once(source, '''    const VkVertexInputBindingDescription binding = {
        .binding = 0,
        .stride = 12,
        .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription attribute = {
        .location = 0,
        .binding = 0,
        .format = VK_FORMAT_R32G32B32_SFLOAT,
        .offset = 0,
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1,
        .pVertexBindingDescriptions = &binding,
        .vertexAttributeDescriptionCount = 1,
        .pVertexAttributeDescriptions = &attribute,
    };''', '''    const VkVertexInputBindingDescription binding = {
        .binding = 0,
        .stride = 36,
        .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription attributes[3] = {
        { .location = 0, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 0 },
        { .location = 1, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 12 },
        { .location = 2, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 24 },
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1,
        .pVertexBindingDescriptions = &binding,
        .vertexAttributeDescriptionCount = 3,
        .pVertexAttributeDescriptions = attributes,
    };''')

    source = replace_once(source, '''    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .pushConstantRangeCount = push_color_enabled ? 1u : 0u,
        .pPushConstantRanges = push_color_enabled ? &push_constant_range : NULL,
    };''', '''    const VkDescriptorSetLayoutBinding camera_binding = {
        .binding = 0,
        .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
        .descriptorCount = 1,
        .stageFlags = VK_SHADER_STAGE_VERTEX_BIT,
    };
    const VkDescriptorSetLayoutCreateInfo set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 1,
        .pBindings = &camera_binding,
    };
    VkDescriptorSetLayout set_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &set_layout_info, NULL, &set_layout));
    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 1,
        .pSetLayouts = &set_layout,
        .pushConstantRangeCount = 0,
        .pPushConstantRanges = NULL,
    };''')

    source = replace_once(source, '''    dump_pipeline_cache_blob(device, pipeline_cache);
    dump_pipeline_executables(device, pipeline);

    const float vertices[9] = {''', '''    dump_pipeline_cache_blob(device, pipeline_cache);
    dump_pipeline_executables(device, pipeline);
    printf("helio_pipeline_dump: compiled_only=1\\n");
    return 0;

    const float vertices[9] = {''')
    destination.write_text(source)


def make_churn_compile_only_dumper(destination: Path) -> None:
    """Specialize the proven ANV compile dumper for Helio's Churn ABI."""
    make_compile_only_dumper(destination)
    source = destination.read_text()
    source = replace_once(source, '''    const VkVertexInputBindingDescription binding = {
        .binding = 0,
        .stride = 36,
        .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription attributes[3] = {
        { .location = 0, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 0 },
        { .location = 1, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 12 },
        { .location = 2, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 24 },
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1,
        .pVertexBindingDescriptions = &binding,
        .vertexAttributeDescriptionCount = 3,
        .pVertexAttributeDescriptions = attributes,
    };''', '''    const VkVertexInputBindingDescription binding = {
        .binding = 0,
        .stride = 24,
        .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription attributes[2] = {
        { .location = 0, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 0 },
        { .location = 1, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 12 },
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1,
        .pVertexBindingDescriptions = &binding,
        .vertexAttributeDescriptionCount = 2,
        .pVertexAttributeDescriptions = attributes,
    };''')
    source = replace_once(source, '''    const VkDescriptorSetLayoutBinding camera_binding = {
        .binding = 0,
        .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
        .descriptorCount = 1,
        .stageFlags = VK_SHADER_STAGE_VERTEX_BIT,
    };
    const VkDescriptorSetLayoutCreateInfo set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 1,
        .pBindings = &camera_binding,
    };''', '''    const VkDescriptorSetLayoutBinding storage_bindings[3] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
    };
    const VkDescriptorSetLayoutCreateInfo set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 3,
        .pBindings = storage_bindings,
    };''')
    destination.write_text(source)


def naga_compile(wgsl: Path, entry: str, output: Path) -> None:
    run([
        "cargo", "run", "-q", "--manifest-path", str(NAGA_MANIFEST), "--",
        "--entry-point", entry, str(wgsl), str(output),
    ], cwd=HELIO)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def vulkan_compile_flags() -> list[str]:
    # The in-repo copy is searched first so a bake is reproducible from a clean
    # checkout and does not depend on host packages or sibling working trees.
    # The remaining roots stay as fallbacks for machines provisioned earlier.
    include_roots = [
        TRUEOS / "vendor/vulkan-headers/include",
        Path("/usr/include"),
        TRUEOS.parent / "bak/reference/mesa/include",
        TRUEOS.parent / "blender-default-cube-toggle/lib/linux_x64/vulkan/include",
    ]
    include = next((root for root in include_roots if (root / "vulkan/vulkan.h").exists()), None)
    if include is None:
        raise SystemExit("Vulkan headers not found (need vulkan/vulkan.h)")
    # The runtime-only Ubuntu install has libvulkan.so.1 but not always the
    # development libvulkan.so symlink. GNU ld's -l: spelling handles both.
    return [f"-I{include}", "-l:libvulkan.so.1"]


def parse_compile_log(log: str) -> tuple[dict[str, object], dict[int, dict[str, object]]]:
    selected = re.search(
        r'selected vendor=0x([0-9A-Fa-f]+) device=0x([0-9A-Fa-f]+).*name="([^"]+)"', log
    )
    if not selected:
        raise SystemExit("Intel compiler device was not recorded")
    device = {
        "vendor_id": int(selected.group(1), 16),
        "device_id": int(selected.group(2), 16),
        "name": selected.group(3),
    }
    executables: dict[int, dict[str, object]] = {}
    for match in re.finditer(
        r'executable\[(\d+)\] stage=(\w+) name="([^"]+)" desc="([^"]+)" subgroup=(\d+)', log
    ):
        index = int(match.group(1))
        executables[index] = {
            "stage": match.group(2), "name": match.group(3),
            "description": match.group(4), "simd_width": int(match.group(5)),
            "statistics": {},
        }
    for match in re.finditer(
        r'stat\[(\d+)\]\[\d+\] name="([^"]+)" value=([^\n]+)', log
    ):
        index = int(match.group(1))
        if index in executables:
            value = match.group(3).strip()
            executables[index]["statistics"][match.group(2)] = int(value) if value.isdigit() else value
    if not any(item["stage"] == "vertex" for item in executables.values()):
        raise SystemExit("Mesa exposed no native vertex executable")
    if not any(item["stage"] == "fragment" for item in executables.values()):
        raise SystemExit("Mesa exposed no native fragment executable")
    return device, executables


def assembly_code_size(path: Path) -> int:
    size = 0
    for line in path.read_text(errors="replace").splitlines():
        # Mesa 26 omits instruction offsets. Continuation lines for SEND
        # descriptors are indented; each other non-empty line is an EU op.
        # Branch labels are assembler annotations, not EU instructions. Tiny
        # SimpleCube had no labels; Churn's material switch exposed this old
        # over-count and therefore the wrong combined-fragment cache size.
        if not line or line[0].isspace() or line == "\0" or line.startswith("LABEL"):
            continue
        size += 8 if "compacted" in line else 16
    if size == 0:
        raise SystemExit(f"no Intel instructions found in {path}")
    return size


def find_cache_stage(cache: bytes, stage: int, size: int) -> bytes:
    candidates: list[bytes] = []
    for offset in range(0, len(cache) - 8):
        found_stage, found_size = struct.unpack_from("<II", cache, offset)
        if found_stage != stage or found_size != size or offset + 8 + size > len(cache):
            continue
        data = cache[offset + 8:offset + 8 + size]
        if sum(byte != 0 for byte in data) > size // 4:
            candidates.append(data)
    if not candidates:
        raise SystemExit(f"native stage absent from ANV cache: stage={stage} size={size}")
    # This is the same first-dense-candidate rule as TRUEOS's existing
    # extract_from_pipeline_cache.py. Require determinism if cache duplicates
    # the actual machine code.
    return candidates[0]


def extract_native(exec_dir: Path, native_dir: Path) -> tuple[bytes, bytes, bytes | None]:
    assemblies = sorted(exec_dir.glob("*_GEN_Assembly.txt"))
    vertex = [path for path in assemblies if "_vertex_" in path.name]
    fragments = [path for path in assemblies if "_fragment_" in path.name]
    if len(vertex) != 1 or not fragments:
        raise SystemExit(
            f"expected one VS and at least one PS assembly, got {len(vertex)} and {len(fragments)}"
        )
    vs_size = assembly_code_size(vertex[0])
    fragment_sizes = [assembly_code_size(path) for path in fragments]
    fragment_offsets: list[int] = []
    cursor = 0
    for size in fragment_sizes:
        cursor = (cursor + 63) & ~63
        fragment_offsets.append(cursor)
        cursor += size
    fragment_span = cursor
    cache = (exec_dir / "pipeline_cache.bin").read_bytes()
    vs = find_cache_stage(cache, 0, vs_size)
    combined = find_cache_stage(cache, 4, fragment_span)
    slices = [combined[offset:offset + size] for offset, size in zip(fragment_offsets, fragment_sizes)]
    native_dir.mkdir(exist_ok=True)
    (native_dir / "simple_cube_vs.bin").write_bytes(vs)
    (native_dir / "simple_cube_ps_simd8.bin").write_bytes(slices[0])
    ps16 = slices[1] if len(slices) > 1 else None
    if ps16 is not None:
        (native_dir / "simple_cube_ps_simd16.bin").write_bytes(ps16)
    return vs, slices[0], ps16


def bake_churn_only(
    args: argparse.Namespace,
    artifact_path: Path,
    sections: dict[str, tuple[int, bytes]],
) -> None:
    """Compile/package the separate Helio Churn capture without SimpleCube."""
    source_kind, wgsl_data = sections[CHURN_FORWARD_SOURCE]
    if source_kind != 3:
        raise SystemExit(
            f"wrong section kind for {CHURN_FORWARD_SOURCE}: {source_kind}"
        )
    sections.setdefault(
        CHURN_LIGHT_SECTION,
        (OTHER_SECTION_KIND, encode_churn_light_scene()),
    )
    output = (args.out or artifact_path.with_name(
        artifact_path.stem + ".intel.helio"
    )).resolve()
    work = (args.work_dir or output.with_suffix(output.suffix + ".work")).resolve()
    work.mkdir(parents=True, exist_ok=True)
    exec_dir = work / f"pipeline_exec-{os.getpid()}"
    native_dir = work / "native"
    exec_dir.mkdir(exist_ok=True)
    native_dir.mkdir(exist_ok=True)

    wgsl = work / "captured-churn-forward.wgsl"
    vs_spv = work / "churn-forward.vert.spv"
    fs_spv = work / "churn-forward.frag.spv"
    wgsl.write_bytes(wgsl_data)
    naga_compile(wgsl, "vs_main", vs_spv)
    naga_compile(wgsl, "fs_main", fs_spv)

    dumper_source = work / "helio_churn_pipeline_dump.c"
    dumper = work / "helio_churn_pipeline_dump"
    make_churn_compile_only_dumper(dumper_source)
    run(["cc", str(dumper_source), "-o", str(dumper), *vulkan_compile_flags()])
    env = os.environ.copy()
    env["TRUEOS_EXECUTABLE_DUMP_DIR"] = str(exec_dir)
    if args.device_id:
        env["TRUEOS_VK_DEVICE_ID"] = args.device_id
    compile_log = work / "compile.log"
    run([str(dumper), str(vs_spv), str(fs_spv)], env=env, log=compile_log)
    log_text = compile_log.read_text()
    if "helio_pipeline_dump: compiled_only=1" not in log_text:
        raise SystemExit("Intel Churn pipeline compilation did not complete")
    vs, ps, _ = extract_native(exec_dir, native_dir)
    if not vs or not ps or len(vs) % 4 or len(ps) % 4:
        raise SystemExit("extracted Churn Intel ISA is empty or misaligned")
    device, executables = parse_compile_log(log_text)
    sections[CHURN_FORWARD_VS] = (4, vs)
    sections[CHURN_FORWARD_PS] = (4, ps)
    descriptor = encode_churn_forward_program(wgsl_data, vs, ps)
    sections[CHURN_FORWARD_SECTION] = (CHURN_FORWARD_KIND, descriptor)

    stages = [
        {
            "stage": "vertex", "entry_point": "vs_main", "simd_width": 8,
            "section": CHURN_FORWARD_VS, "code_size_bytes": len(vs),
            "sha256": sha256(vs), "binding_table_entry_count": 4,
            "sampler_count": 0, "scratch_bytes": 0,
            "grf_start_register": 2, "grf_used": 128, "max_threads": 64,
            "urb_entry_output_length": 1,
        },
        {
            "stage": "fragment", "entry_point": "fs_main", "simd_width": 8,
            "section": CHURN_FORWARD_PS, "code_size_bytes": len(ps),
            "sha256": sha256(ps), "binding_table_entry_count": 1,
            "sampler_count": 0, "scratch_bytes": 0,
            "grf_start_register": 4, "grf_used": 128, "max_threads": 64,
            "num_varying_inputs": 2, "uses_vmask": True, "flat_inputs": 2,
        },
    ]
    metadata = {
        "schema": 1,
        "producer": "helio-intel-bake",
        "frontend": "captured-wgsl-via-helio-vendored-naga",
        "backend": "mesa-anv-vulkan-pipeline-cache",
        "architecture": "intel-gfx125-xe-lp-compatible",
        "requested_trueos_device": "8086:4680-r0C",
        "compile_device": device,
        "program": "churn-forward",
        "descriptor_section": CHURN_FORWARD_SECTION,
        "wgsl_section": CHURN_FORWARD_SOURCE,
        "wgsl_sha256": sha256(wgsl_data),
        "spirv": {
            "vs_main_sha256": sha256(vs_spv.read_bytes()),
            "fs_main_sha256": sha256(fs_spv.read_bytes()),
        },
        "runtime_abi": "mesa-anv-gfx125-eu",
        "vertex_layout": {
            "stride": 24,
            "attributes": [
                {"location": 0, "format": "float32x3", "offset": 0,
                 "vf_component_mask": 7},
                {"location": 1, "format": "float32x3", "offset": 12,
                 "vf_component_mask": 7},
            ],
        },
        "bindings": [
            {"group": 0, "binding": 0, "type": "read-only-storage-buffer",
             "stages": ["vertex"], "intel_bti": 1, "element_stride": 368},
            {"group": 0, "binding": 1, "type": "read-only-storage-buffer",
             "stages": ["vertex"], "intel_bti": 2, "element_stride": 208},
            {"group": 0, "binding": 2, "type": "read-only-storage-buffer",
             "stages": ["vertex"], "intel_bti": 3, "element_stride": 4},
            {"type": "render-target", "stages": ["fragment"], "intel_bti": 0},
        ],
        "stages": stages,
        "compiler_executables": list(executables.values()),
    }
    sections["compiler/intel-xe-lp.json"] = (
        5, (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode(),
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(emit_helioa(sections))

    validated = parse_helioa(output.read_bytes())
    if validated.get(CHURN_FORWARD_SECTION) != (CHURN_FORWARD_KIND, descriptor):
        raise SystemExit("packaged churn-forward-v1 descriptor changed")
    for stage in stages:
        binary = validated[stage["section"]][1]
        if sha256(binary) != stage["sha256"]:
            raise SystemExit(f"packaged ISA hash mismatch: {stage['section']}")
    print(f"baked {output}")
    print(f"  {CHURN_FORWARD_SOURCE}: sha256={sha256(wgsl_data)}")
    print(f"  {CHURN_FORWARD_SECTION}: {len(descriptor)} bytes")
    print(f"  Intel compiler: {device['name']} 8086:{device['device_id']:04X}")
    for stage in stages:
        print(
            f"  {stage['section']}: {stage['code_size_bytes']} bytes "
            f"sha256={stage['sha256']}"
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--work-dir", type=Path)
    parser.add_argument("--device-id", help="Force the Intel Vulkan compiler device, e.g. 0xA780")
    args = parser.parse_args()

    for path in (NAGA_MANIFEST, UPSTREAM_DUMPER, UPSTREAM_EXTRACTOR):
        if not path.exists():
            raise SystemExit(f"required pinned input is absent: {path}")
    for tool in ("cargo", "cc", "python3"):
        if shutil.which(tool) is None:
            raise SystemExit(f"required tool not found: {tool}")

    artifact_path = args.artifact.resolve()
    sections = parse_helioa(artifact_path.read_bytes())
    # Both the SimpleCube and Churn-only lanes carry the same canonical graph
    # seed. Assignment (rather than setdefault) prevents stale input sections
    # from surviving a new bake under the versioned name.
    sections[RETAINED_TRANSFORM_SECTION] = (
        OTHER_SECTION_KIND, encode_retained_transform_template(),
    )
    if CHURN_FORWARD_SOURCE in sections and RENDER_IR_SECTION not in sections:
        bake_churn_only(args, artifact_path, sections)
        return
    # These scene contracts are the build-time handoff from the hosted Helio
    # demos to TRUEOS's no_std retained renderer. They deliberately share the
    # captured Helio/WGPU graph and native shader pair in this artifact.
    sections[CHURN_LIGHT_SECTION] = (
        OTHER_SECTION_KIND, encode_churn_light_scene(),
    )
    sections["scene/shape-battle-v1.bin"] = (
        OTHER_SECTION_KIND, encode_shape_battle_scene(),
    )
    sections["scene/pendulum-bigcloth-v1.bin"] = (
        OTHER_SECTION_KIND, encode_pendulum_bigcloth_scene(),
    )
    sections[SPRITE_DIG_SECTION] = (
        OTHER_SECTION_KIND, encode_sprite_dig_scene(),
    )
    sections[PORTAL_ROOMS_SECTION] = (
        OTHER_SECTION_KIND, encode_portal_rooms_scene(),
    )
    try:
        render_ir_kind, render_ir = sections[RENDER_IR_SECTION]
    except KeyError:
        raise SystemExit(f"required HELIOA section absent: {RENDER_IR_SECTION}") from None
    if render_ir_kind != RENDER_IR_KIND:
        raise SystemExit(
            f"wrong section kind for {RENDER_IR_SECTION}: {render_ir_kind}"
        )
    replay = encode_replay_plan(render_ir)
    sections[REPLAY_SECTION] = (REPLAY_KIND, replay)
    wgsl_section, wgsl_data = captured_wgsl(sections)
    output = (args.out or artifact_path.with_name(artifact_path.stem + ".intel.helio")).resolve()
    work = (args.work_dir or output.with_suffix(output.suffix + ".work")).resolve()
    work.mkdir(parents=True, exist_ok=True)
    exec_dir = work / f"pipeline_exec-{os.getpid()}"
    native_dir = work / "native"
    exec_dir.mkdir(exist_ok=True)
    native_dir.mkdir(exist_ok=True)

    wgsl = work / "captured-simple-cube.wgsl"
    vs_spv = work / "simple-cube.vert.spv"
    fs_spv = work / "simple-cube.frag.spv"
    wgsl.write_bytes(wgsl_data)
    naga_compile(wgsl, "vs_main", vs_spv)
    naga_compile(wgsl, "fs_main", fs_spv)

    dumper_source = work / "helio_pipeline_dump.c"
    dumper = work / "helio_pipeline_dump"
    make_compile_only_dumper(dumper_source)
    run(["cc", str(dumper_source), "-o", str(dumper), *vulkan_compile_flags()])
    env = os.environ.copy()
    env["TRUEOS_EXECUTABLE_DUMP_DIR"] = str(exec_dir)
    if args.device_id:
        env["TRUEOS_VK_DEVICE_ID"] = args.device_id
    compile_log = work / "compile.log"
    run([str(dumper), str(vs_spv), str(fs_spv)], env=env, log=compile_log)
    log_text = compile_log.read_text()
    if "helio_pipeline_dump: compiled_only=1" not in log_text:
        raise SystemExit("Intel pipeline compilation did not complete")

    vs, ps8, ps16 = extract_native(exec_dir, native_dir)
    if not vs or not ps8 or len(vs) % 4 or len(ps8) % 4:
        raise SystemExit("extracted Intel ISA is empty or misaligned")

    device, executables = parse_compile_log(log_text)
    stages = [
        {
            "stage": "vertex", "entry_point": "vs_main", "simd_width": 8,
            "section": "intel-xe-lp/vs.simd8.bin", "code_size_bytes": len(vs),
            "sha256": sha256(vs), "binding_table_entry_count": 2,
            "sampler_count": 0, "scratch_bytes": 0, "grf_start_register": 2,
            "urb_writes": [{"offset": 0, "grfs": 8}, {"offset": 2, "grfs": 4}],
        },
        {
            "stage": "fragment", "entry_point": "fs_main", "simd_width": 8,
            "section": "intel-xe-lp/ps.simd8.bin", "code_size_bytes": len(ps8),
            "sha256": sha256(ps8), "binding_table_entry_count": 1,
            "sampler_count": 0, "scratch_bytes": 0, "grf_start_register": 2,
            "num_varying_inputs": 1,
        },
    ]
    sections["intel-xe-lp/vs.simd8.bin"] = (4, vs)
    sections["intel-xe-lp/ps.simd8.bin"] = (4, ps8)
    if ps16 is not None:
        sections["intel-xe-lp/ps.simd16.bin"] = (4, ps16)
        stages.append({
            "stage": "fragment", "entry_point": "fs_main", "simd_width": 16,
            "section": "intel-xe-lp/ps.simd16.bin", "code_size_bytes": len(ps16),
            "sha256": sha256(ps16), "binding_table_entry_count": 1,
            "sampler_count": 0, "scratch_bytes": 0, "grf_start_register": 2,
            "num_varying_inputs": 1,
        })

    churn_metadata = None
    churn_executables: list[dict[str, object]] = []
    if CHURN_FORWARD_SOURCE in sections:
        churn_source_kind, churn_wgsl_data = sections[CHURN_FORWARD_SOURCE]
        if churn_source_kind != 3:
            raise SystemExit(
                f"wrong section kind for {CHURN_FORWARD_SOURCE}: {churn_source_kind}"
            )
        churn_exec_dir = work / f"churn_pipeline_exec-{os.getpid()}"
        churn_native_dir = work / "churn_native"
        churn_exec_dir.mkdir(exist_ok=True)
        churn_native_dir.mkdir(exist_ok=True)
        churn_wgsl = work / "captured-churn-forward.wgsl"
        churn_vs_spv = work / "churn-forward.vert.spv"
        churn_fs_spv = work / "churn-forward.frag.spv"
        churn_wgsl.write_bytes(churn_wgsl_data)
        naga_compile(churn_wgsl, "vs_main", churn_vs_spv)
        naga_compile(churn_wgsl, "fs_main", churn_fs_spv)

        churn_dumper_source = work / "helio_churn_pipeline_dump.c"
        churn_dumper = work / "helio_churn_pipeline_dump"
        make_churn_compile_only_dumper(churn_dumper_source)
        run([
            "cc", str(churn_dumper_source), "-o", str(churn_dumper),
            *vulkan_compile_flags(),
        ])
        churn_env = os.environ.copy()
        churn_env["TRUEOS_EXECUTABLE_DUMP_DIR"] = str(churn_exec_dir)
        if args.device_id:
            churn_env["TRUEOS_VK_DEVICE_ID"] = args.device_id
        churn_log = work / "churn-compile.log"
        run(
            [str(churn_dumper), str(churn_vs_spv), str(churn_fs_spv)],
            env=churn_env,
            log=churn_log,
        )
        churn_log_text = churn_log.read_text()
        if "helio_pipeline_dump: compiled_only=1" not in churn_log_text:
            raise SystemExit("Intel Churn pipeline compilation did not complete")
        churn_vs, churn_ps, _ = extract_native(churn_exec_dir, churn_native_dir)
        churn_device, churn_compiler_executables = parse_compile_log(churn_log_text)
        if churn_device["vendor_id"] != device["vendor_id"] \
                or churn_device["device_id"] != device["device_id"]:
            raise SystemExit("SimpleCube and Churn native stages used different devices")
        churn_executables = list(churn_compiler_executables.values())
        sections[CHURN_FORWARD_VS] = (4, churn_vs)
        sections[CHURN_FORWARD_PS] = (4, churn_ps)
        descriptor = encode_churn_forward_program(
            churn_wgsl_data, churn_vs, churn_ps,
        )
        sections[CHURN_FORWARD_SECTION] = (CHURN_FORWARD_KIND, descriptor)
        churn_metadata = {
            "schema": 1,
            "descriptor_section": CHURN_FORWARD_SECTION,
            "wgsl_section": CHURN_FORWARD_SOURCE,
            "wgsl_sha256": sha256(churn_wgsl_data),
            "spirv": {
                "vs_main_sha256": sha256(churn_vs_spv.read_bytes()),
                "fs_main_sha256": sha256(churn_fs_spv.read_bytes()),
            },
            "stages": [
                {
                    "stage": "vertex", "entry_point": "vs_main", "simd_width": 8,
                    "section": CHURN_FORWARD_VS, "code_size_bytes": len(churn_vs),
                    "sha256": sha256(churn_vs), "binding_table_entry_count": 4,
                    "sampler_count": 0, "scratch_bytes": 0,
                    "grf_start_register": 2, "grf_used": 128, "max_threads": 64,
                    "urb_entry_output_length": 1,
                },
                {
                    "stage": "fragment", "entry_point": "fs_main", "simd_width": 8,
                    "section": CHURN_FORWARD_PS, "code_size_bytes": len(churn_ps),
                    "sha256": sha256(churn_ps), "binding_table_entry_count": 1,
                    "sampler_count": 0, "scratch_bytes": 0,
                    "grf_start_register": 4, "grf_used": 128, "max_threads": 64,
                    "num_varying_inputs": 2, "uses_vmask": True, "flat_inputs": 2,
                },
            ],
        }
        stages.extend(churn_metadata["stages"])

    metadata = {
        "schema": 1,
        "producer": "helio-intel-bake",
        "frontend": "captured-wgsl-via-helio-vendored-naga",
        "backend": "mesa-anv-vulkan-pipeline-cache",
        "architecture": "intel-gfx125-xe-lp-compatible",
        "requested_trueos_device": "8086:4680-r0C",
        "compile_device": device,
        "wgsl_section": wgsl_section,
        "wgsl_sha256": sha256(wgsl_data),
        "spirv": {
            "vs_main_sha256": sha256(vs_spv.read_bytes()),
            "fs_main_sha256": sha256(fs_spv.read_bytes()),
        },
        "runtime_abi": "mesa-anv-gfx125-eu",
        "runtime_abi_status": (
            "shader binding ABI identified; TRUEOS still must program matching "
            "vertex-fetch, URB, SBE and PS payload state"
        ),
        "vertex_layout": {
            "stride": 36,
            "attributes": [
                {"location": 0, "format": "float32x3", "offset": 0},
                {"location": 1, "format": "float32x3", "offset": 12},
                {"location": 2, "format": "float32x3", "offset": 24},
            ],
        },
        "bindings": [
            {
                "group": 0, "binding": 0, "type": "read-only-storage-buffer",
                "stages": ["vertex"], "intel_bti": 1,
                "camera_view_proj_byte_offset": 128, "read_bytes": 64,
            },
            {"type": "render-target", "stages": ["fragment"], "intel_bti": 0},
        ],
        "stages": stages,
        "compiler_executables": list(executables.values()),
    }
    if churn_metadata is not None:
        metadata["churn_forward"] = churn_metadata
        metadata["compiler_executables"].extend(churn_executables)
    metadata_data = (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode()
    sections["compiler/intel-xe-lp.json"] = (5, metadata_data)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(emit_helioa(sections))

    # Reparse and validate every CRC plus native section hash before success.
    validated = parse_helioa(output.read_bytes())
    if validated.get(REPLAY_SECTION) != (REPLAY_KIND, replay):
        raise SystemExit("packaged replay-v1 differs from HELIOIR lowering")
    for stage in stages:
        actual = validated[stage["section"]][1]
        if sha256(actual) != stage["sha256"]:
            raise SystemExit(f"packaged ISA hash mismatch: {stage['section']}")
    print(f"baked {output}")
    print(f"  captured WGSL: {wgsl_section} sha256={sha256(wgsl_data)}")
    print(
        f"  {REPLAY_SECTION}: 1 x {REPLAY_COMMAND_STRIDE}-byte indexed draw "
        f"from HELIOIR crc32={zlib.crc32(render_ir) & 0xFFFF_FFFF:08x}"
    )
    print(f"  Intel compiler: {device['name']} 8086:{device['device_id']:04X}")
    for stage in stages:
        print(f"  {stage['section']}: {stage['code_size_bytes']} bytes sha256={stage['sha256']}")
    print("  runtime ABI: BTI1 camera / BTI0 RT identified; TRUEOS fixed-function state remains")


if __name__ == "__main__":
    main()
