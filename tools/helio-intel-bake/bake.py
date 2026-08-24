#!/usr/bin/env python3
"""Bake WGSL captured in a HELIOA file to target-specific Mesa/ANV ISA.

This is intentionally a narrow bridge.  Naga is taken from Helio's vendored
wgpu tree and the Intel compilation/extraction path is the one already used by
TRUEOS's xe_lp_shader_bake proof.  No shader source is synthesized here.
"""

from __future__ import annotations

import argparse
from collections import Counter
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
HELIO_EXAMPLES = TRUEOS.parent / "Helio-Examples"
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

# HelioC is deliberately fed by the authored WebGPU sources, not the older
# C++/OpenCL compatibility experiments beside them. These identities are also
# sealed by src/intel/gpgpu/types/helioc_native.rs and src/gpu/vgpu.rs.
HELIOC_SOURCE_ROOT = HELIO_EXAMPLES / "cloud-engine-webgpu-linux-aligned/shaders"
HELIOC_SIMULATE_SOURCE = HELIOC_SOURCE_ROOT / "simulate.wgsl"
HELIOC_RENDER_SOURCE = HELIOC_SOURCE_ROOT / "render.wgsl"
HELIOC_SIMULATE_SHA256 = "f583d3c63e5f387a5926281df29b7688eb09eaa5f06119d74fffa70d592013f6"
HELIOC_RENDER_SHA256 = "5d536a468fcb698c3dca79faac0e5a4924fdc8ce2c9ce3a9b5d24c40a84cc9ff"
HELIOC_PACKAGE_SECTION = "compiler/helioc-native-volume-raymarch-v3.bin"
HELIOC_COMPUTE_SOURCE_SECTION = "authored/cloud-engine/simulate.wgsl"
HELIOC_GRAPHICS_SOURCE_SECTION = "authored/cloud-engine/render.wgsl"
HELIOC_VOLUME_SECTION = "resources/volume3d-rgba16f-v1.bin"
HELIOC_VOLUME_METADATA_SECTION = "compiler/cloud-volume-bindings-v1.json"
HELIOC_COMPUTE_ISA_SECTION = "intel-xe-lp/helioc-volume-update.bin"
HELIOC_VERTEX_ISA_SECTION = "intel-xe-lp/helioc-volume-raymarch.vs.bin"
HELIOC_FRAGMENT_ISA_SECTION = "intel-xe-lp/helioc-volume-raymarch.fs.bin"
HELIOC_DESCRIPTOR_BYTES = 384
HELIOC_GFX_VERX10 = 120
HELIOC_DEVICE_ID = 0x4680
HELIOC_REVISION = 0x0C
HELIOC_RELOC_STATE_SECTION = "compiler/helioc-relocatable-state-v2.bin"
HELIOC_RELOC_STATE_MAGIC = b"HELIOCRS"
HELIOC_RELOC_STATE_VERSION = 2
HELIOC_RELOC_HEADER_BYTES = 128
HELIOC_RELOC_OBJECT_BYTES = 64
HELIOC_RELOC_ENTRY_BYTES = 32
HELIOC_RELOC_MAX_OBJECTS = 64
HELIOC_RELOC_MAX_ENTRIES = 512
HELIOC_RELOC_MAX_BYTES = 0x70_000
HELIOC_RELOC_FLAGS = 0x0F
HELIOC_RELOC_WINDOWS = {1, 2, 3, 4}  # batch, surface, dynamic, indirect
HELIOC_RELOC_KINDS = {1, 2, 3, 4, 5, 6}  # batch, surface, sampler, binding, program, indirect
HELIOC_RELOC_VALUE_KINDS = {1, 2, 3, 4, 5}  # object offset/GPU, fixed GPU, runtime GPU/u32
# The Direct-RCS retirement profile is not inherited from the Vulkan capture.
# It replaces only the authenticated final BBE after all captured state has
# been normalized.  The address words remain zero in the template and are
# supplied through one typed 64-bit RESULT relocation at materialization.
HELIOC_RESULT_GPU_BASE = 0x0864_0000
HELIOC_RCS_COMPLETION_MARKER = 0xC0DE_C002
HELIOC_MI_BATCH_BUFFER_END = 0x0500_0000
HELIOC_TERMINAL_DWORDS = (
    0x7A00_0204, 0x4010_10A0, 0, 0, 0, 0,
    0x7A00_0004, 0x0010_4080, 0, 0,
    HELIOC_RCS_COMPLETION_MARKER, 0, HELIOC_MI_BATCH_BUFFER_END,
)
HELIOC_SYMBOL_RESULT = 8
HELIOC_SYMBOLIC_V2_REQUIRED_ROLES = {
    "volume_a", "volume_b", "sim_params", "render_params", "output_target",
    "shader_heap", "internal_surface_state_heap", "binding_table_heap",
    "dynamic_state_heap", "indirect_descriptor_heap", "command_bo",
}
HELIOC_SYMBOLIC_V2_KINDS = {
    "image", "buffer", "surface_state", "render_target_state", "descriptor_set_state", "state", "heap", "command",
}
HELIOC_SYMBOLIC_V2_DESCRIPTOR_SET_ROLES = {
    "compute_ping_a", "compute_ping_b", "graphics_a", "graphics_b",
}
HELIOC_SYMBOLIC_V2_INDIRECT_DESCRIPTOR_FIELDS = (
    "set_role", "resource_role", "kind", "binding", "raw_va", "bytes", "data_hex",
)
HELIOC_SYMBOLIC_V3_TABLE_FIELDS = (
    "table_kind", "set_role", "stage", "raw_va", "bytes", "entry_count", "entry_roles", "data_hex",
)
HELIOC_COMMAND_CATALOG_FIELDS = (
    "current", "step", "final_role", "dispatch_roles", "volume_layout", "inter_dispatch_visibility",
    "draw_vertices", "raw_va", "bytes", "data_hex",
)
HELIOC_ADDRESS_FREE_INDIRECT_FIELDS = (
    "set_role", "resource_role", "kind", "binding", "bytes", "data_hex", "reloc",
)
HELIOC_ADDRESS_FREE_SURFACE_FIELDS = (
    "role", "kind", "bytes", "state_offset", "data_hex", "reloc",
)
HELIOC_ADDRESS_FREE_V7_FIELDS = (
    "object", "kind", "resource_role", "set_role", "binding", "state_bytes",
    "state_alignment", "descriptor_payload_offset", "descriptor_bytes", "sampler_bytes",
    "descriptor_layout", "resource_offset", "resource_bytes", "data_hex", "reloc",
)
HELIOC_ADDRESS_FREE_V8_BINDING_ROLES = {
    "compute_ping_a": ("descriptor_set_state", "volume_b", "sim_params", "volume_a"),
    "compute_ping_b": ("descriptor_set_state", "volume_a", "sim_params", "volume_b"),
    "graphics_a": ("output_target", "descriptor_set_state", "volume_a", "render_params"),
    "graphics_b": ("output_target", "descriptor_set_state", "volume_b", "render_params"),
}
HELIOC_ADDRESS_FREE_V8_RELOC_SOURCES = {
    "compute_ping_a": ("descriptor_set_surface_compute_ping_a", "surface_volume_b_storage", "buffer_surface_sim_params_compute_ping_a", "surface_volume_a_sampled"),
    "compute_ping_b": ("descriptor_set_surface_compute_ping_b", "surface_volume_a_storage", "buffer_surface_sim_params_compute_ping_b", "surface_volume_b_sampled"),
    "graphics_a": ("surface_ui4_target", "descriptor_set_surface_graphics_a", "surface_volume_a_sampled", "buffer_surface_render_params_graphics_a"),
    "graphics_b": ("surface_ui4_target", "descriptor_set_surface_graphics_b", "surface_volume_b_sampled", "buffer_surface_render_params_graphics_b"),
}
HELIOC_ADDRESS_FREE_V9_PAYLOADS = {
    "compute_ping_a": (5, 64, 4, "sim_params", "volume_a", "volume_b"),
    "compute_ping_b": (5, 64, 4, "sim_params", "volume_b", "volume_a"),
    "graphics_a": (4, 32, 3, "render_params", "volume_a", None),
    "graphics_b": (4, 32, 3, "render_params", "volume_b", None),
}
HELIOC_V10A_VARIANTS = {
    ("volume_a", "0", "volume_a", "none"): 3,
    ("volume_a", "1", "volume_b", "a_to_b"): 3,
    ("volume_a", "2", "volume_a", "a_to_b,b_to_a"): 3,
    ("volume_b", "0", "volume_b", "none"): 3,
    ("volume_b", "1", "volume_a", "b_to_a"): 3,
    ("volume_b", "2", "volume_b", "b_to_a,a_to_b"): 3,
}
HELIOC_ADDRESS_FREE_V9_BLAKE3 = {
    "compute_ping_a": "b2cf78c0c8f6325bc444d9927d6c2452bbeac89e9323e70b8172cf47a309b6f4",
    "compute_ping_b": "b2cf78c0c8f6325bc444d9927d6c2452bbeac89e9323e70b8172cf47a309b6f4",
    "graphics_a": "f70c078d3e79d102c118961a3a341c5099e71506bb8086c6e0dc05ac3e116e47",
    "graphics_b": "f70c078d3e79d102c118961a3a341c5099e71506bb8086c6e0dc05ac3e116e47",
}
HELIOC_SYMBOLIC_V2_FIELDS = (
    "role", "kind", "stage", "binding", "raw_va", "allocation_bytes", "logical_bytes",
    "resource_offset", "state_heap_va", "state_heap_bytes", "state_offset", "state_bytes",
    "row_pitch", "array_pitch", "state_hex",
)


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


def helioc_authored_sources() -> tuple[bytes, bytes]:
    """Read the only two sources that a sealed HelioC package may name."""
    for path in (HELIOC_SIMULATE_SOURCE, HELIOC_RENDER_SOURCE):
        if not path.is_file():
            raise SystemExit(f"required authored HelioC source is absent: {path}")
    compute = HELIOC_SIMULATE_SOURCE.read_bytes()
    graphics = HELIOC_RENDER_SOURCE.read_bytes()
    if sha256(compute) != HELIOC_SIMULATE_SHA256:
        raise SystemExit("simulate.wgsl does not match the sealed HelioC source digest")
    if sha256(graphics) != HELIOC_RENDER_SHA256:
        raise SystemExit("render.wgsl does not match the sealed HelioC source digest")
    for marker in (
        b"@compute @workgroup_size(4, 4, 4)",
        b"fn main(@builtin(global_invocation_id)",
        b"texture_3d<f32>",
        b"texture_storage_3d<rgba16float, write>",
    ):
        if marker not in compute:
            raise SystemExit(f"sealed simulate.wgsl lacks required HelioC marker: {marker!r}")
    for marker in (
        b"@vertex\nfn vs_main",
        b"@fragment\nfn fs_main",
        b"texture_3d<f32>",
    ):
        if marker not in graphics:
            raise SystemExit(f"sealed render.wgsl lacks required HelioC marker: {marker!r}")
    return compute, graphics


def _helioc_u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def _helioc_u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def validate_helioc_resource_capture(resources: bytes, metadata: bytes) -> None:
    """Check that a future real capture supplied the sealed logical contract.

    This is intentionally a structural check only.  The capture, not this
    host script, must select native surface/sampler encodings and compiler
    table indices.  The TRUEOS parser repeats the full semantic validation.
    """
    if not metadata:
        raise SystemExit("HelioC capture has no compiler/resource metadata")
    if len(resources) < 96 or resources[:8] != b"HELV3D\0\0":
        raise SystemExit("HelioC capture has no HELV3D volume resource contract")
    if (
        _helioc_u16(resources, 8) != 1
        or _helioc_u16(resources, 10) != 96
        or _helioc_u32(resources, 12) != len(resources)
        or tuple(_helioc_u16(resources, offset) for offset in (16, 18, 20, 22, 24))
        != (2, 4, 1, 6, 4)
        or resources[48:80] != hashlib.sha256(metadata).digest()
        or any(resources[80:96])
    ):
        raise SystemExit("HelioC capture resource metadata is not the sealed cloud-volume contract")


def validate_helioc_relocatable_state(section: bytes) -> dict[str, object]:
    """Mirror the sealed Rust HELIOCRS v2 parser; JSON is never accepted."""
    if not isinstance(section, bytes) or len(section) < HELIOC_RELOC_HEADER_BYTES \
            or len(section) > HELIOC_RELOC_MAX_BYTES or section[:8] != HELIOC_RELOC_STATE_MAGIC:
        raise SystemExit("HelioC relocatable state is not HELIOCRS v2 binary")
    u16 = lambda offset: _helioc_u16(section, offset)
    u32 = lambda offset: _helioc_u32(section, offset)
    if (u16(8), u16(10), u16(16), u16(18), section[20:24], u16(24), u16(26), u32(48), section[52:56]) != (
        2, 128, 120, 0x4680, bytes((0x0c, 1, 64, 0)), 64, 32, 0x0f, bytes((6, 2, 0, 0))
    ) or any(section[64:128]):
        raise SystemExit("HelioC HELIOCRS v2 header is not the sealed ADL-S contract")
    total, object_count, reloc_count = u32(12), u16(28), u16(30)
    object_offset, reloc_offset, data_offset, data_bytes = u32(32), u32(36), u32(40), u32(44)
    if total != len(section) or not 0 < object_count <= HELIOC_RELOC_MAX_OBJECTS \
            or reloc_count > HELIOC_RELOC_MAX_ENTRIES or object_offset != 128 \
            or reloc_offset != object_offset + object_count * 64 \
            or data_offset != (reloc_offset + reloc_count * 32 + 63) & ~63 \
            or u32(56) != object_count * HELIOC_RELOC_OBJECT_BYTES \
            or u32(60) != reloc_count * HELIOC_RELOC_ENTRY_BYTES \
            or data_offset + data_bytes != total or any(section[reloc_offset + reloc_count * 32:data_offset]):
        raise SystemExit("HelioC HELIOCRS v2 table bounds are invalid")
    objects: dict[int, dict[str, int]] = {}
    variants: set[int] = set()
    for index in range(object_count):
        offset = object_offset + index * 64
        obj_id, semantic = u16(offset), u16(offset + 4)
        window, kind, variant, flags = section[offset + 2], section[offset + 3], section[offset + 6], section[offset + 7]
        dst, data_rel, size, alignment = u32(offset + 8), u32(offset + 12), u32(offset + 16), u16(offset + 20)
        first, count = u16(offset + 22), u16(offset + 24)
        if obj_id == 0 or obj_id in objects or semantic == 0 or window not in HELIOC_RELOC_WINDOWS \
                or kind not in HELIOC_RELOC_KINDS or flags != 0 or size == 0 or alignment == 0 \
                or alignment > 4096 or alignment & (alignment - 1) or data_rel % alignment \
                or data_rel + size > data_bytes or dst % alignment \
                or first + count > reloc_count or any(section[offset + 26:offset + 28]) \
                or any(section[offset + 60:offset + 64]):
            raise SystemExit("HelioC HELIOCRS v2 object is malformed")
        if (kind in {1, 4, 6} and alignment < 4) or (kind == 1 and window != 1) \
                or (kind == 2 and window != 2) or (kind == 3 and window != 3) \
                or (kind == 4 and window != 2) or (kind == 5 and window != 3) \
                or (kind == 6 and window != 4) or (variant != 0xff and window != 1) \
                or (window == 1 and variant > 5) or dst + size > (256 * 1024 if window == 1 else 64 * 1024):
            raise SystemExit("HelioC HELIOCRS v2 object/window contract is invalid")
        for prior in objects.values():
            if prior["window"] == window and prior["semantic"] == semantic:
                raise SystemExit("HelioC HELIOCRS v2 object semantic is duplicated")
            if data_rel < prior["data_rel"] + prior["size"] \
                    and prior["data_rel"] < data_rel + size:
                raise SystemExit("HelioC HELIOCRS v2 object data intervals overlap")
            if prior["window"] == window \
                    and dst < prior["dst"] + prior["size"] \
                    and prior["dst"] < dst + size:
                raise SystemExit("HelioC HELIOCRS v2 object window intervals overlap")
        data = section[data_offset + data_rel:data_offset + data_rel + size]
        digest = section[offset + 28:offset + 60]
        if not any(digest) or hashlib.sha256(data).digest() != digest:
            raise SystemExit("HelioC HELIOCRS v2 object hash is invalid")
        if kind == 1:
            if variant in variants:
                raise SystemExit("HelioC HELIOCRS v2 batch variant is duplicated")
            variants.add(variant)
        objects[obj_id] = {
            "offset": offset, "size": size, "first": first, "count": count,
            "semantic": semantic, "window": window, "dst": dst, "data_rel": data_rel,
        }
    if variants != set(range(6)):
        raise SystemExit("HelioC HELIOCRS v2 requires all six batch variants")
    groups: list[tuple[int, int]] = []
    for obj in objects.values():
        group = (obj["first"], obj["first"] + obj["count"])
        if any(group[0] < prior[1] and prior[0] < group[1] for prior in groups):
            raise SystemExit("HelioC HELIOCRS v2 relocation groups overlap")
        groups.append(group)
    if sum(obj["count"] for obj in objects.values()) != reloc_count:
        raise SystemExit("HelioC HELIOCRS v2 relocation groups are incomplete")
    relocs: list[tuple[int, int, int, int]] = []
    previous_key: tuple[int, int, int] | None = None
    for index in range(reloc_count):
        offset = reloc_offset + index * 32
        target, source, target_off, source_off = u16(offset), u16(offset + 2), u32(offset + 4), u32(offset + 8)
        width, value_kind, shift, flags = section[offset + 12:offset + 16]
        mask, addend = struct.unpack_from("<Qq", section, offset + 16)
        target_obj = objects.get(target)
        mask_shift = (mask & -mask).bit_length() - 1 if mask else 0
        normalized_mask = mask >> mask_shift
        if target_obj is None or width not in {4, 8} or value_kind not in HELIOC_RELOC_VALUE_KINDS \
                or shift >= 64 or flags != 0 or mask == 0 or (width == 4 and mask >> 32) \
                or normalized_mask & (normalized_mask + 1) \
                or target_off % 4 or target_off + width > target_obj["size"] \
                or not (target_obj["first"] <= index < target_obj["first"] + target_obj["count"]) \
                or not -(1 << 31) <= addend < (1 << 31):
            raise SystemExit("HelioC HELIOCRS v2 relocation is malformed")
        if value_kind in {1, 2}:
            if source not in objects or source_off >= objects[source]["size"]:
                raise SystemExit("HelioC HELIOCRS v2 object relocation source is invalid")
        elif value_kind == 3:
            if source not in set(range(1, 13)) or source_off != 0:
                raise SystemExit("HelioC HELIOCRS v2 fixed-GPU source is invalid")
        elif value_kind == 4:
            if source != 13 or source_off != 0 or addend != 0:
                raise SystemExit("HelioC HELIOCRS v2 runtime-GPU source is invalid")
        elif value_kind == 5:
            if source not in {14, 15, 16} or source_off != 0 or addend not in {0, -1}:
                raise SystemExit("HelioC HELIOCRS v2 runtime-u32 source is invalid")
        key = (target, target_off, mask)
        if previous_key is not None and previous_key > key:
            raise SystemExit("HelioC HELIOCRS v2 relocations are not strictly canonical")
        for prior_target, prior_offset, prior_width, prior_mask in relocs:
            if prior_target != target:
                continue
            byte_overlap = target_off < prior_offset + prior_width \
                and prior_offset < target_off + width
            if byte_overlap and (target_off != prior_offset or width != prior_width
                                 or prior_mask & mask):
                raise SystemExit("HelioC HELIOCRS v2 relocation fields overlap")
        relocs.append((target, target_off, width, mask))
        previous_key = key
    return {"object_count": object_count, "reloc_count": reloc_count, "bytes": len(section)}


def encode_helioc_relocatable_state(objects: list[dict[str, object]]) -> bytes:
    """Encode one canonical address-free HELIOCRS v2 object graph.

    Object names exist only while baking and are converted to deterministic
    numeric IDs.  Every relocation is attached to its target object so an
    object cannot borrow another object's relocation range.  The template
    bits selected by each relocation must already be zero: this encoder never
    accepts a captured pointer and silently overwrites it.

    Required object fields are ``name``, ``window``, ``kind``, ``variant``,
    ``dst``, ``alignment``, ``data``, and ``relocations``.  Required
    relocation fields are ``target_offset``, ``source``, ``source_offset``,
    ``width``, ``value_kind``, ``right_shift``, ``mask``, and ``addend``.
    Object-relative relocation sources use another object's string name;
    fixed/runtime sources use the sealed numeric symbol ID.
    """
    object_fields = {
        "name", "window", "kind", "variant", "dst", "alignment", "data",
        "relocations",
    }
    relocation_fields = {
        "target_offset", "source", "source_offset", "width", "value_kind",
        "right_shift", "mask", "addend",
    }

    def integer(value: object, label: str) -> int:
        if type(value) is not int:
            raise SystemExit(f"HelioC HELIOCRS encoder requires integer {label}")
        return value

    if type(objects) is not list or not 0 < len(objects) <= HELIOC_RELOC_MAX_OBJECTS:
        raise SystemExit("HelioC HELIOCRS encoder requires 1..64 objects")

    normalized: list[dict[str, object]] = []
    names: set[str] = set()
    for raw in objects:
        if type(raw) is not dict or set(raw) != object_fields:
            raise SystemExit("HelioC HELIOCRS encoder object has an invalid schema")
        name = raw["name"]
        if type(name) is not str or re.fullmatch(r"[a-z][a-z0-9_]{0,95}", name) is None \
                or name in names:
            raise SystemExit("HelioC HELIOCRS encoder object name is invalid or duplicated")
        names.add(name)
        window = integer(raw["window"], f"{name}.window")
        kind = integer(raw["kind"], f"{name}.kind")
        variant = integer(raw["variant"], f"{name}.variant")
        dst = integer(raw["dst"], f"{name}.dst")
        alignment = integer(raw["alignment"], f"{name}.alignment")
        data = raw["data"]
        relocs = raw["relocations"]
        if type(data) is not bytes or not data or type(relocs) is not list:
            raise SystemExit(f"HelioC HELIOCRS encoder object {name} has invalid data/relocations")
        if window not in HELIOC_RELOC_WINDOWS or kind not in HELIOC_RELOC_KINDS \
                or alignment <= 0 or alignment > 4096 or alignment & (alignment - 1) \
                or dst < 0 or dst % alignment:
            raise SystemExit(f"HelioC HELIOCRS encoder object {name} has invalid placement")
        expected_window = {1: 1, 2: 2, 3: 3, 4: 2, 5: 3, 6: 4}[kind]
        if window != expected_window or (kind in {1, 4, 6} and alignment < 4):
            raise SystemExit(f"HelioC HELIOCRS encoder object {name} has invalid kind/window")
        if (kind == 1 and variant not in range(6)) or (kind != 1 and variant != 0xff):
            raise SystemExit(f"HelioC HELIOCRS encoder object {name} has invalid batch variant")
        window_bytes = 256 * 1024 if window == 1 else 64 * 1024
        if len(data) > window_bytes or dst + len(data) > window_bytes:
            raise SystemExit(f"HelioC HELIOCRS encoder object {name} exceeds its window")
        normalized.append({
            "name": name, "window": window, "kind": kind, "variant": variant,
            "dst": dst, "alignment": alignment, "data": data, "relocations": relocs,
        })

    normalized.sort(key=lambda item: (
        int(item["window"]), int(item["dst"]), int(item["kind"]),
        int(item["variant"]), str(item["name"]),
    ))
    if {int(item["variant"]) for item in normalized if int(item["kind"]) == 1} != set(range(6)):
        raise SystemExit("HelioC HELIOCRS encoder requires exactly all six batch variants")
    if sum(int(item["kind"]) == 1 for item in normalized) != 6:
        raise SystemExit("HelioC HELIOCRS encoder rejects extra batch objects")

    for index, item in enumerate(normalized):
        left_begin = int(item["dst"])
        left_end = left_begin + len(item["data"])
        for prior in normalized[:index]:
            if int(prior["window"]) != int(item["window"]):
                continue
            right_begin = int(prior["dst"])
            right_end = right_begin + len(prior["data"])
            if left_begin < right_end and right_begin < left_end:
                raise SystemExit("HelioC HELIOCRS encoder object windows overlap")

    object_ids = {str(item["name"]): index + 1 for index, item in enumerate(normalized)}
    encoded_relocs: list[dict[str, int]] = []
    object_groups: dict[str, tuple[int, int]] = {}
    for item in normalized:
        name = str(item["name"])
        target_id = object_ids[name]
        data = item["data"]
        assert isinstance(data, bytes)
        group: list[dict[str, int]] = []
        for raw in item["relocations"]:
            if type(raw) is not dict or set(raw) != relocation_fields:
                raise SystemExit(f"HelioC HELIOCRS encoder relocation for {name} has invalid schema")
            target_offset = integer(raw["target_offset"], f"{name}.target_offset")
            source_offset = integer(raw["source_offset"], f"{name}.source_offset")
            width = integer(raw["width"], f"{name}.width")
            value_kind = integer(raw["value_kind"], f"{name}.value_kind")
            right_shift = integer(raw["right_shift"], f"{name}.right_shift")
            mask = integer(raw["mask"], f"{name}.mask")
            addend = integer(raw["addend"], f"{name}.addend")
            source_raw = raw["source"]
            if width not in {4, 8} or value_kind not in HELIOC_RELOC_VALUE_KINDS \
                    or target_offset < 0 or target_offset % 4 \
                    or target_offset + width > len(data) or source_offset < 0 \
                    or not 0 <= right_shift < 64 or mask <= 0 \
                    or mask >> (width * 8) or not -(1 << 31) <= addend < (1 << 31):
                raise SystemExit(f"HelioC HELIOCRS encoder relocation for {name} is malformed")
            lsb = (mask & -mask).bit_length() - 1
            field_mask = mask >> lsb
            if field_mask & (field_mask + 1):
                raise SystemExit(f"HelioC HELIOCRS encoder relocation for {name} has split mask")
            template = int.from_bytes(data[target_offset:target_offset + width], "little")
            if template & mask:
                raise SystemExit(
                    f"HelioC HELIOCRS encoder relocation for {name} retained captured bits"
                )
            if value_kind in {1, 2}:
                if type(source_raw) is not str or source_raw not in object_ids:
                    raise SystemExit(f"HelioC HELIOCRS encoder relocation for {name} has bad object source")
                source_id = object_ids[source_raw]
                source_item = normalized[source_id - 1]
                if source_offset >= len(source_item["data"]):
                    raise SystemExit(f"HelioC HELIOCRS encoder relocation for {name} exceeds source")
            else:
                source_id = integer(source_raw, f"{name}.source")
                if source_offset != 0 \
                        or value_kind == 3 and source_id not in range(1, 13) \
                        or value_kind == 4 and (source_id != 13 or addend != 0) \
                        or value_kind == 5 and (source_id not in {14, 15, 16} or addend not in {0, -1}):
                    raise SystemExit(f"HelioC HELIOCRS encoder relocation for {name} has bad symbol")
            group.append({
                "target": target_id, "source": source_id,
                "target_offset": target_offset, "source_offset": source_offset,
                "width": width, "value_kind": value_kind, "right_shift": right_shift,
                "mask": mask, "addend": addend,
            })
        group.sort(key=lambda reloc: (reloc["target_offset"], reloc["mask"]))
        for index, reloc in enumerate(group):
            begin = reloc["target_offset"]
            end = begin + reloc["width"]
            for prior in group[:index]:
                prior_begin = prior["target_offset"]
                prior_end = prior_begin + prior["width"]
                if begin >= prior_end or prior_begin >= end:
                    continue
                if begin != prior_begin or reloc["width"] != prior["width"] \
                        or reloc["mask"] & prior["mask"]:
                    raise SystemExit(
                        f"HelioC HELIOCRS encoder relocations for {name} overlap"
                    )
        first = len(encoded_relocs)
        encoded_relocs.extend(group)
        object_groups[name] = (first, len(group))
    if len(encoded_relocs) > HELIOC_RELOC_MAX_ENTRIES:
        raise SystemExit("HelioC HELIOCRS encoder exceeds the relocation limit")

    data_blob = bytearray()
    data_offsets: dict[str, int] = {}
    for item in normalized:
        alignment = int(item["alignment"])
        aligned = (len(data_blob) + alignment - 1) & -alignment
        data_blob.extend(b"\0" * (aligned - len(data_blob)))
        data_offsets[str(item["name"])] = aligned
        data = item["data"]
        assert isinstance(data, bytes)
        data_blob.extend(data)

    object_count = len(normalized)
    reloc_count = len(encoded_relocs)
    object_offset = HELIOC_RELOC_HEADER_BYTES
    reloc_offset = object_offset + object_count * HELIOC_RELOC_OBJECT_BYTES
    reloc_end = reloc_offset + reloc_count * HELIOC_RELOC_ENTRY_BYTES
    data_offset = (reloc_end + 63) & ~63
    total = data_offset + len(data_blob)
    if total > HELIOC_RELOC_MAX_BYTES:
        raise SystemExit("HelioC HELIOCRS encoder exceeds the section byte limit")
    section = bytearray(total)
    section[:8] = HELIOC_RELOC_STATE_MAGIC
    struct.pack_into(
        "<HHIHH4BHHHHIIIII4BII", section, 8,
        HELIOC_RELOC_STATE_VERSION, HELIOC_RELOC_HEADER_BYTES, total,
        HELIOC_GFX_VERX10, HELIOC_DEVICE_ID, HELIOC_REVISION, 1, 64, 0,
        HELIOC_RELOC_OBJECT_BYTES, HELIOC_RELOC_ENTRY_BYTES,
        object_count, reloc_count, object_offset, reloc_offset, data_offset,
        len(data_blob), HELIOC_RELOC_FLAGS, 6, 2, 0, 0,
        object_count * HELIOC_RELOC_OBJECT_BYTES,
        reloc_count * HELIOC_RELOC_ENTRY_BYTES,
    )
    for index, item in enumerate(normalized):
        name = str(item["name"])
        offset = object_offset + index * HELIOC_RELOC_OBJECT_BYTES
        first, count = object_groups[name]
        data = item["data"]
        assert isinstance(data, bytes)
        struct.pack_into(
            "<HBBHBBIIIHHH", section, offset,
            object_ids[name], int(item["window"]), int(item["kind"]), index + 1,
            int(item["variant"]), 0, int(item["dst"]), data_offsets[name], len(data),
            int(item["alignment"]), first, count,
        )
        section[offset + 28:offset + 60] = hashlib.sha256(data).digest()
    for index, reloc in enumerate(encoded_relocs):
        struct.pack_into(
            "<HHIIBBBBQq", section, reloc_offset + index * HELIOC_RELOC_ENTRY_BYTES,
            reloc["target"], reloc["source"], reloc["target_offset"],
            reloc["source_offset"], reloc["width"], reloc["value_kind"],
            reloc["right_shift"], 0, reloc["mask"], reloc["addend"],
        )
    section[data_offset:] = data_blob
    encoded = bytes(section)
    validate_helioc_relocatable_state(encoded)
    return encoded


def encode_helioc_relocation_field(resolved: int, addend: int, right_shift: int,
                                   mask: int, width: int) -> int:
    """Encode one HELIOCRS field exactly as the Rust materializer does.

    Address/size values are shifted down first, then shifted into the lowest
    set mask bit.  This is deliberately not a raw truncated value: width and
    height fields often start above bit zero.  The caller performs the masked
    read-modify-write using this returned positioned value.
    """
    if width not in {4, 8} or right_shift >= 64 or mask <= 0 or mask >> (width * 8):
        raise SystemExit("HelioC relocation field has invalid width/shift/mask")
    lsb = (mask & -mask).bit_length() - 1
    field_mask = mask >> lsb
    if field_mask & (field_mask + 1):
        raise SystemExit("HelioC relocation field mask is not contiguous")
    value = resolved + addend
    if value < 0:
        raise SystemExit("HelioC relocation field underflows before shift")
    value >>= right_shift
    if value > field_mask:
        raise SystemExit("HelioC relocation field does not fit its mask")
    return value << lsb


def normalize_helioc_relocatable_state(section: bytes) -> bytes:
    """Accept only already-canonical address-free HELIOCRS v2 bytes."""
    validate_helioc_relocatable_state(section)
    return section


def append_helioc_terminal_template(batch: bytes) -> tuple[bytes, dict[str, int]]:
    """Replace a captured BBE with the exact Direct-RCS completion template.

    ANV's terminal BBE cannot name an authenticated TRUEOS completion target.
    This bounded rewrite preserves every preceding captured byte and replaces
    *only* that final BBE with the fixed flush/post-sync/BBE profile.  The
    address is deliberately zero in the immutable template; its returned
    HELIOCRS relocation describes the sole materialized field.
    """
    if not isinstance(batch, bytes) or len(batch) < 4 or len(batch) % 4:
        raise SystemExit("HelioC captured batch is not a non-empty dword stream")
    if len(batch) + (len(HELIOC_TERMINAL_DWORDS) - 1) * 4 > 256 * 1024:
        raise SystemExit("HelioC terminal epilogue exceeds the sealed batch window")
    if _helioc_u32(batch, len(batch) - 4) != HELIOC_MI_BATCH_BUFFER_END:
        raise SystemExit("HelioC terminal rewrite requires exactly one captured final BBE")
    out = batch[:-4] + struct.pack("<13I", *HELIOC_TERMINAL_DWORDS)
    # The address begins after the first six dwords plus the post-sync header
    # and flags.  It may be 4 mod 8 (for example A2 at offset 2588); Rust's
    # HELIOCRS parser therefore requires dword, not natural-qword, alignment.
    result_offset = len(batch) + 28
    relocation = {
        "target_offset": result_offset,
        "source_symbol": HELIOC_SYMBOL_RESULT,
        "source_offset": 0,
        "width": 8,
        "value_kind": 3,  # fixed GPU
        "right_shift": 0,
        "mask": 0xFFFF_FFFF_FFFF_FFFF,
        "addend": 0,
    }
    validate_helioc_terminal_template(out, relocation)
    return out, relocation


def validate_helioc_terminal_template(batch: bytes, relocation: dict[str, int]) -> None:
    """Validate the unmaterialized terminal profile and its single relocation."""
    if len(batch) < len(HELIOC_TERMINAL_DWORDS) * 4 or len(batch) % 4:
        raise SystemExit("HelioC terminal template has invalid length")
    terminal = list(struct.unpack_from("<13I", batch, len(batch) - 13 * 4))
    expected = list(HELIOC_TERMINAL_DWORDS)
    if terminal != expected:
        raise SystemExit("HelioC terminal template is not the sealed flush/marker/BBE profile")
    expected_offset = len(batch) - 13 * 4 + 8 * 4
    expected_relocation = {
        "target_offset": expected_offset,
        "source_symbol": HELIOC_SYMBOL_RESULT,
        "source_offset": 0,
        "width": 8,
        "value_kind": 3,
        "right_shift": 0,
        "mask": 0xFFFF_FFFF_FFFF_FFFF,
        "addend": 0,
    }
    if relocation != expected_relocation or expected_offset % 4:
        raise SystemExit("HelioC terminal template lacks the exact RESULT qword relocation")
    # This is a template check, not a captured-fence check: no ANV VA is
    # carried forward.  Confirm the materialized field resolves exactly.
    if encode_helioc_relocation_field(
        HELIOC_RESULT_GPU_BASE, relocation["addend"], relocation["right_shift"],
        relocation["mask"], relocation["width"],
    ) != HELIOC_RESULT_GPU_BASE:
        raise SystemExit("HelioC terminal RESULT relocation does not materialize the fixed GPU VA")


def encode_helioc_descriptor(
    compute: bytes,
    graphics: bytes,
    resources: bytes,
    compute_isa: bytes,
    vertex_isa: bytes,
    fragment_isa: bytes,
    reloc_state: bytes,
    *,
    compute_simd: int,
) -> bytes:
    """Encode the fixed HELIOC v3 descriptor from genuine captured bytes.

    Callers must supply the three ISA records and resource metadata captured
    from the actual Naga/Mesa/ANV pipeline.  This function has no fallback
    binary path: synthetic, empty, or unaligned ISA cannot become a package.
    """
    if sha256(compute) != HELIOC_SIMULATE_SHA256 or sha256(graphics) != HELIOC_RENDER_SHA256:
        raise SystemExit("HelioC descriptor refuses non-authored WGSL bytes")
    if compute_simd not in (16, 32):
        raise SystemExit("HelioC compute SIMD must be 16 or 32")
    for name, isa in (
        ("compute", compute_isa),
        ("vertex", vertex_isa),
        ("fragment", fragment_isa),
    ):
        if not isa or len(isa) % 4 or not any(isa):
            raise SystemExit(f"HelioC {name} ISA must be captured, non-zero, and dword aligned")
    validate_helioc_relocatable_state(reloc_state)

    descriptor = bytearray(HELIOC_DESCRIPTOR_BYTES)
    descriptor[:8] = b"HELIOC\0\0"
    struct.pack_into("<HHII", descriptor, 8, 3, HELIOC_DESCRIPTOR_BYTES,
                     HELIOC_DESCRIPTOR_BYTES, 0x0F)
    struct.pack_into("<HH", descriptor, 20, HELIOC_GFX_VERX10, HELIOC_DEVICE_ID)
    descriptor[24:28] = bytes((HELIOC_REVISION, HELIOC_REVISION, 1, 0))
    threads = 64 // compute_simd
    per_thread = 96 if compute_simd == 16 else 192
    struct.pack_into("<HH", descriptor, 28, compute_simd, threads)
    struct.pack_into("<3H", descriptor, 32, 4, 4, 4)
    struct.pack_into("<3H", descriptor, 38, 24, 12, 24)
    struct.pack_into("<3H", descriptor, 44, 96, per_thread, 480)
    struct.pack_into("<3H", descriptor, 50, 8, 16, 7)
    for index, binding in enumerate((
        (1, 0, 0, 4), (1, 0, 1, 1), (1, 0, 2, 2), (1, 0, 3, 3),
        (2, 0, 0, 4), (2, 0, 1, 1), (2, 0, 2, 2),
    )):
        descriptor[60 + index * 4:64 + index * 4] = bytes(binding)
    struct.pack_into("<4H", descriptor, 88, 112, 272, 3, 0x003F)
    for offset, data in (
        (96, compute), (100, graphics), (104, resources),
        (108, compute_isa), (112, vertex_isa), (116, fragment_isa),
    ):
        struct.pack_into("<I", descriptor, offset, len(data))
    for offset, data in (
        (128, compute), (160, graphics), (192, resources),
        (224, compute_isa), (256, vertex_isa), (288, fragment_isa),
    ):
        descriptor[offset:offset + 32] = hashlib.sha256(data).digest()
    struct.pack_into("<I", descriptor, 320, len(reloc_state))
    descriptor[324:356] = hashlib.sha256(reloc_state).digest()
    return bytes(descriptor)


def validate_helioc_descriptor(
    descriptor: bytes,
    compute: bytes,
    graphics: bytes,
    resources: bytes,
    compute_isa: bytes,
    vertex_isa: bytes,
    fragment_isa: bytes,
    reloc_state: bytes,
    *,
    compute_simd: int,
) -> None:
    """Assert every HELIOC v3 field before its HELIOA is emitted."""
    expected_threads = 64 // compute_simd
    expected_per_thread = 96 if compute_simd == 16 else 192
    if (
        len(descriptor) != HELIOC_DESCRIPTOR_BYTES
        or descriptor[:8] != b"HELIOC\0\0"
        or (_helioc_u16(descriptor, 8), _helioc_u16(descriptor, 10), _helioc_u32(descriptor, 12), _helioc_u32(descriptor, 16))
        != (3, HELIOC_DESCRIPTOR_BYTES, HELIOC_DESCRIPTOR_BYTES, 0x0F)
        or (_helioc_u16(descriptor, 20), _helioc_u16(descriptor, 22), descriptor[24:28])
        != (
            HELIOC_GFX_VERX10,
            HELIOC_DEVICE_ID,
            bytes((HELIOC_REVISION, HELIOC_REVISION, 1, 0)),
        )
        or (_helioc_u16(descriptor, 28), _helioc_u16(descriptor, 30))
        != (compute_simd, expected_threads)
        or tuple(_helioc_u16(descriptor, offset) for offset in (32, 34, 36, 38, 40, 42))
        != (4, 4, 4, 24, 12, 24)
        or tuple(_helioc_u16(descriptor, offset) for offset in (44, 46, 48))
        != (96, expected_per_thread, 480)
        or tuple(_helioc_u16(descriptor, offset) for offset in (50, 52, 54))
        != (8, 16, 7)
        or tuple(_helioc_u16(descriptor, offset) for offset in (88, 90, 92, 94))
        != (112, 272, 3, 0x003F)
        or any(descriptor[56:60])
        or any(descriptor[120:128]) or any(descriptor[356:384])
    ):
        raise SystemExit("HelioC descriptor self-check rejected fixed target/shape/payload state")
    expected_bindings = bytes((
        1, 0, 0, 4, 1, 0, 1, 1, 1, 0, 2, 2, 1, 0, 3, 3,
        2, 0, 0, 4, 2, 0, 1, 1, 2, 0, 2, 2,
    ))
    if descriptor[60:88] != expected_bindings:
        raise SystemExit("HelioC descriptor self-check rejected logical binding state")
    for size_offset, hash_offset, data in (
        (96, 128, compute), (100, 160, graphics), (104, 192, resources),
        (108, 224, compute_isa), (112, 256, vertex_isa), (116, 288, fragment_isa),
    ):
        if _helioc_u32(descriptor, size_offset) != len(data) \
                or descriptor[hash_offset:hash_offset + 32] != hashlib.sha256(data).digest():
            raise SystemExit("HelioC descriptor self-check rejected a sealed section reference")
    if descriptor[128:160].hex() != HELIOC_SIMULATE_SHA256 \
            or descriptor[160:192].hex() != HELIOC_RENDER_SHA256:
        raise SystemExit("HelioC descriptor self-check rejected authored source provenance")
    if _helioc_u32(descriptor, 320) != len(reloc_state) \
            or descriptor[324:356] != hashlib.sha256(reloc_state).digest():
        raise SystemExit("HelioC descriptor self-check rejected HELIOCRS v2 reference")
    validate_helioc_relocatable_state(reloc_state)


def assemble_helioc_package(
    compute: bytes,
    graphics: bytes,
    resources: bytes,
    resource_metadata: bytes,
    compute_isa: bytes,
    vertex_isa: bytes,
    fragment_isa: bytes,
    reloc_state: bytes,
    *,
    compute_simd: int,
) -> bytes:
    """Build a HELIOA only after a real capture has supplied every datum."""
    validate_helioc_resource_capture(resources, resource_metadata)
    descriptor = encode_helioc_descriptor(
        compute, graphics, resources, compute_isa, vertex_isa, fragment_isa,
        reloc_state, compute_simd=compute_simd,
    )
    validate_helioc_descriptor(
        descriptor, compute, graphics, resources, compute_isa, vertex_isa,
        fragment_isa, reloc_state, compute_simd=compute_simd,
    )
    manifest = {
        "schema": 3,
        "descriptor_version": 3,
        "producer": "helio-intel-bake/helioc",
        "frontend": "helio-vendored-naga",
        "backend": "mesa-anv-vulkan-pipeline-executable-cache",
        "target": "intel-gfx120-adl-s-uhd-770-rev-0c",
        "workload": "cloud-volume-update-plus-fullscreen-raymarch",
        "compute": {
            "entry": "main", "local_size": [4, 4, 4], "groups": [24, 12, 24],
            "simd_width": compute_simd, "hardware_threads": 64 // compute_simd,
        },
        "graphics": {
            "vertex_entry": "vs_main", "vertex_simd_width": 8,
            "fragment_entry": "fs_main", "fragment_simd_width": 16,
            "profile_flags": [
                "vertex_index", "no_vertex_buffer", "no_depth",
                "single_sample", "premultiplied_ui4", "fullscreen_triangle",
            ],
        },
        "schedule": {
            "sim_params_bytes": 112, "render_params_bytes": 272,
            "draw_vertex_count": 3,
        },
        "bindings": {
            "compute": ["uniform-read", "sampled", "sampler", "storage"],
            "graphics": ["uniform-read", "sampled", "sampler"],
        },
    }
    sections = {
        "manifest.json": (1, (json.dumps(manifest, sort_keys=True) + "\n").encode()),
        HELIOC_PACKAGE_SECTION: (5, descriptor),
        HELIOC_COMPUTE_SOURCE_SECTION: (3, compute),
        HELIOC_GRAPHICS_SOURCE_SECTION: (3, graphics),
        HELIOC_VOLUME_METADATA_SECTION: (5, resource_metadata),
        HELIOC_VOLUME_SECTION: (6, resources),
        HELIOC_COMPUTE_ISA_SECTION: (4, compute_isa),
        HELIOC_VERTEX_ISA_SECTION: (4, vertex_isa),
        HELIOC_FRAGMENT_ISA_SECTION: (4, fragment_isa),
        HELIOC_RELOC_STATE_SECTION: (5, reloc_state),
    }
    artifact = emit_helioa(sections)
    parsed = parse_helioa(artifact)
    for name, expected_kind, data in (
        (HELIOC_PACKAGE_SECTION, 5, descriptor),
        (HELIOC_COMPUTE_SOURCE_SECTION, 3, compute),
        (HELIOC_GRAPHICS_SOURCE_SECTION, 3, graphics),
        (HELIOC_VOLUME_METADATA_SECTION, 5, resource_metadata),
        (HELIOC_VOLUME_SECTION, 6, resources),
        (HELIOC_COMPUTE_ISA_SECTION, 4, compute_isa),
        (HELIOC_VERTEX_ISA_SECTION, 4, vertex_isa),
        (HELIOC_FRAGMENT_ISA_SECTION, 4, fragment_isa),
        (HELIOC_RELOC_STATE_SECTION, 5, reloc_state),
    ):
        if parsed.get(name) != (expected_kind, data):
            raise SystemExit(f"HelioC assembler failed to preserve {name}")
    return artifact


def helioc_linear_volume_requirements(log: str) -> list[dict[str, int]]:
    """Read the valid layout of the actual LINEAR images bound by HelioC."""
    pattern = re.compile(
        r"helioc_pipeline_dump: volume\[(\d+)\] linear_memory size=(\d+) "
        r"alignment=(\d+) type_bits=0x([0-9A-Fa-f]+) layout offset=(\d+) size=(\d+) "
        r"row_pitch=(\d+) array_pitch=(\d+) depth_pitch=(\d+)"
    )
    requirements = [
        {
            "index": int(match.group(1)), "allocation_bytes": int(match.group(2)),
            "alignment": int(match.group(3)), "memory_type_bits": int(match.group(4), 16),
            "offset": int(match.group(5)), "subresource_bytes": int(match.group(6)),
            "row_pitch_bytes": int(match.group(7)), "array_pitch_bytes": int(match.group(8)),
            "depth_pitch_bytes": int(match.group(9)),
        }
        for match in pattern.finditer(log)
    ]
    if len(requirements) != 2 or [item["index"] for item in requirements] != [0, 1]:
        raise SystemExit("HelioC Vulkan capture did not report two bound LINEAR 3D volume layouts")
    if requirements[0] != {**requirements[1], "index": 0}:
        raise SystemExit("HelioC ping-pong LINEAR volume layouts differ")
    required = {
        "allocation_bytes": 3_538_944, "offset": 0, "subresource_bytes": 3_538_944,
        "row_pitch_bytes": 768, "depth_pitch_bytes": 36_864,
    }
    if any(requirements[0][field] != value for field, value in required.items()):
        raise SystemExit("HelioC bound LINEAR volume layout is not the sealed tight 96x48x96 RGBA16F layout")
    return requirements


def helioc_linear_probe(log: str) -> dict[str, int | bool | str]:
    """Read only the valid LINEAR-image layout probe, if the driver supports it."""
    features = re.search(
        r"helioc_pipeline_dump: linear_probe features=0x([0-9A-Fa-f]+) required=0x([0-9A-Fa-f]+)",
        log,
    )
    if not features:
        raise SystemExit("HelioC Vulkan capture did not report linear-image format features")
    unsupported = "helioc_pipeline_dump: linear_probe unsupported=sampled_storage_3d" in log
    layout = re.search(
        r"helioc_pipeline_dump: linear_probe memory size=(\d+) alignment=(\d+) "
        r"type_bits=0x([0-9A-Fa-f]+) layout offset=(\d+) size=(\d+) "
        r"row_pitch=(\d+) array_pitch=(\d+) depth_pitch=(\d+)",
        log,
    )
    if unsupported:
        if layout:
            raise SystemExit("HelioC linear probe reported both unsupported and a layout")
        return {
            "supported": False, "linear_features": int(features.group(1), 16),
            "required_features": int(features.group(2), 16),
        }
    if not layout or "helioc_pipeline_dump: linear_probe create_result=0" not in log:
        raise SystemExit("HelioC linear probe did not create a valid sampled+storage 3D image")
    return {
        "supported": True, "linear_features": int(features.group(1), 16),
        "required_features": int(features.group(2), 16),
        "allocation_bytes": int(layout.group(1)), "alignment": int(layout.group(2)),
        "memory_type_bits": int(layout.group(3), 16), "offset": int(layout.group(4)),
        "subresource_bytes": int(layout.group(5)), "row_pitch_bytes": int(layout.group(6)),
        "array_pitch_bytes": int(layout.group(7)), "depth_pitch_bytes": int(layout.group(8)),
    }


def helioc_public_api_boundary(
    device: dict[str, object], volume_requirements: list[dict[str, int]], log: str,
    *, instrumented: dict[str, object] | None = None,
) -> list[str]:
    """List data the public capture APIs provably do not expose.

    Pipeline executable properties provide ISA statistics and textual internal
    representations. Pipeline-cache bytes are explicitly opaque; neither API
    returns ANV's descriptor-to-BTI/sampler mapping or compute program-data
    payload. Vulkan device properties also carry a PCI device ID, not the PCI
    revision needed by HELIOC's sealed ADL-S revision field.
    """
    if instrumented is not None:
        if instrumented.get("address_free_indirect_templates") is not None \
                and instrumented.get("address_free_surface_templates") is not None \
                and instrumented.get("address_free_buffer_descriptor_templates") is not None \
                and instrumented.get("address_free_binding_sampler_tables") is not None \
                and instrumented.get("address_free_descriptor_payloads") is not None:
            missing_state = (
                "symbolic ownership/typed relocations for command and program state "
                "(V5 indirect descriptors, V6 image surfaces, V7 buffer/descriptor-set surfaces, and "
                "V8 binding/sampler tables, and V9 descriptor payloads are address-free)"
            )
        else:
            missing_state = (
                "symbolic address/ownership map for the command, binding-table, sampler, "
                "indirect-descriptor, and render-target state (captured raw addresses are diagnostic only)"
            )
        return [
            missing_state,
            "physical UHD 770 PCI r0c retirement/ISA proof (the explicit no-op shim proves "
            "compiler identity, not execution on target silicon)",
        ]

    missing = [
        "ANV descriptor-to-BTI/sampler-table mapping and compute program-data payload "
        "(not returned by VK_KHR_pipeline_executable_properties; pipeline cache is opaque)",
        "PCI revision 0x0C for the sealed ADL-S target (not returned by VkPhysicalDeviceProperties)",
    ]
    if "Batch logging not supported" in log:
        missing.append(
            "ANV batch/SURFACE_STATE/SAMPLER_STATE dump (INTEL_DEBUG=bat reported batch logging unavailable)"
        )
    if device["device_id"] != 0x4680:
        missing.insert(
            0,
            f"sealed ADL-S UHD 770 device 0x4680 (capture selected 0x{device['device_id']:04X})",
        )
    return missing


def _parse_helioc_symbolic_v2_record(path: Path) -> dict[str, object]:
    """Parse one source-instrumented V2 record without accepting aliases."""
    try:
        lines = path.read_text(encoding="ascii").splitlines()
    except UnicodeDecodeError as error:
        raise SystemExit(f"HelioC V2 record is not ASCII: {path.name}") from error
    if not lines or lines[0] != "TRUEOS_HELIOC_RESOURCE_V2":
        raise SystemExit(f"HelioC V2 record has an invalid magic: {path.name}")
    fields: dict[str, str] = {}
    for line in lines[1:]:
        key, separator, value = line.partition("=")
        if not separator or key in fields:
            raise SystemExit(f"HelioC V2 record has malformed or duplicate field: {path.name}")
        fields[key] = value
    if tuple(fields) != HELIOC_SYMBOLIC_V2_FIELDS:
        raise SystemExit(f"HelioC V2 record has unknown or missing fields: {path.name}")
    if fields["kind"] not in HELIOC_SYMBOLIC_V2_KINDS:
        raise SystemExit(f"HelioC V2 record has an unsealed role/kind: {path.name}")
    if fields["kind"] == "descriptor_set_state":
        if fields["role"] not in HELIOC_SYMBOLIC_V2_DESCRIPTOR_SET_ROLES:
            raise SystemExit(f"HelioC V2 descriptor-set state has an unsealed role: {path.name}")
    elif fields["role"] not in HELIOC_SYMBOLIC_V2_REQUIRED_ROLES | {"sampler_state"}:
        raise SystemExit(f"HelioC V2 record has an unsealed role/kind: {path.name}")
    if not re.fullmatch(r"0x[0-9a-f]+", fields["raw_va"]) \
            or not re.fullmatch(r"0x(?:0|[0-9a-f]+)", fields["state_heap_va"]):
        raise SystemExit(f"HelioC V2 record has a non-canonical VA: {path.name}")
    values: dict[str, object] = {
        "file": path.name, "role": fields["role"], "kind": fields["kind"],
        "raw_va": int(fields["raw_va"], 16), "state_heap_va": int(fields["state_heap_va"], 16),
    }
    for key in ("stage", "binding", "allocation_bytes", "logical_bytes", "resource_offset",
                "state_heap_bytes", "state_offset", "state_bytes", "row_pitch", "array_pitch"):
        if not re.fullmatch(r"(?:0|[1-9][0-9]*)", fields[key]):
            raise SystemExit(f"HelioC V2 record has a non-canonical integer: {path.name}")
        values[key] = int(fields[key])
    state_hex = fields["state_hex"]
    state_bytes = int(values["state_bytes"])
    if state_hex == "-":
        if state_bytes and fields["kind"] in {
            "surface_state", "render_target_state", "descriptor_set_state", "state",
        }:
            raise SystemExit(f"HelioC V2 state record omits its resolved bytes: {path.name}")
        values["raw_state_byte_fingerprint_sha256"] = None
    else:
        if state_bytes == 0 or len(state_hex) != state_bytes * 2 \
                or re.fullmatch(r"[0-9a-f]+", state_hex) is None:
            raise SystemExit(f"HelioC V2 state bytes are malformed: {path.name}")
        # Surface and sampler encodings may contain capture-process VAs.  This
        # is a diagnostic fingerprint only, never an immutable template hash.
        values["raw_state_byte_fingerprint_sha256"] = sha256(bytes.fromhex(state_hex))
    if int(values["raw_va"]) == 0 or int(values["allocation_bytes"]) == 0 \
            or int(values["logical_bytes"]) == 0 \
            or int(values["logical_bytes"]) > int(values["allocation_bytes"]):
        raise SystemExit(f"HelioC V2 resource range is invalid: {path.name}")
    return values


def _parse_helioc_symbolic_v2_indirect_descriptor(path: Path) -> dict[str, object]:
    """Parse exactly one raw ANV shader-loaded descriptor diagnostic."""
    try:
        lines = path.read_text(encoding="ascii").splitlines()
    except UnicodeDecodeError as error:
        raise SystemExit(f"HelioC V2 indirect descriptor is not ASCII: {path.name}") from error
    if not lines or lines[0] != "TRUEOS_HELIOC_INDIRECT_DESCRIPTOR_V2":
        raise SystemExit(f"HelioC V2 indirect descriptor has an invalid magic: {path.name}")
    fields: dict[str, str] = {}
    for line in lines[1:]:
        key, separator, value = line.partition("=")
        if not separator or key in fields:
            raise SystemExit(f"HelioC V2 indirect descriptor is malformed: {path.name}")
        fields[key] = value
    if tuple(fields) != HELIOC_SYMBOLIC_V2_INDIRECT_DESCRIPTOR_FIELDS:
        raise SystemExit(f"HelioC V2 indirect descriptor has unknown or missing fields: {path.name}")
    if fields["set_role"] not in HELIOC_SYMBOLIC_V2_DESCRIPTOR_SET_ROLES \
            or fields["resource_role"] not in {"volume_a", "volume_b"} \
            or fields["kind"] not in {"sampled", "storage"} \
            or not re.fullmatch(r"[1-9][0-9]*", fields["binding"]) \
            or not re.fullmatch(r"0x[0-9a-f]+", fields["raw_va"]) \
            or not re.fullmatch(r"[1-9][0-9]*", fields["bytes"]):
        raise SystemExit(f"HelioC V2 indirect descriptor has a non-canonical field: {path.name}")
    byte_count = int(fields["bytes"])
    if (fields["kind"], byte_count) not in {("sampled", 8), ("storage", 32)} \
            or len(fields["data_hex"]) != byte_count * 2 \
            or re.fullmatch(r"[0-9a-f]+", fields["data_hex"]) is None:
        raise SystemExit(f"HelioC V2 indirect descriptor has the wrong payload shape: {path.name}")
    raw_va = int(fields["raw_va"], 16)
    if raw_va == 0:
        raise SystemExit(f"HelioC V2 indirect descriptor has a null descriptor VA: {path.name}")
    return {
        "file": path.name,
        "set_role": fields["set_role"],
        "resource_role": fields["resource_role"],
        "kind": fields["kind"],
        "binding": int(fields["binding"]),
        "raw_va": raw_va,
        "bytes": byte_count,
        "raw_data_hex": fields["data_hex"],
        # Address-bearing bytes are a diagnostic fingerprint, never a sealed blob hash.
        "raw_byte_fingerprint_sha256": sha256(bytes.fromhex(fields["data_hex"])),
    }


def _parse_helioc_symbolic_v3_table(path: Path) -> dict[str, object]:
    """Parse one raw binding/sampler table record without treating it as ABI."""
    try:
        lines = path.read_text(encoding="ascii").splitlines()
    except UnicodeDecodeError as error:
        raise SystemExit(f"HelioC V3 table is not ASCII: {path.name}") from error
    if not lines or lines[0] != "TRUEOS_HELIOC_TABLE_V3":
        raise SystemExit(f"HelioC V3 table has an invalid magic: {path.name}")
    fields: dict[str, str] = {}
    for line in lines[1:]:
        key, separator, value = line.partition("=")
        if not separator or key in fields:
            raise SystemExit(f"HelioC V3 table is malformed: {path.name}")
        fields[key] = value
    if tuple(fields) != HELIOC_SYMBOLIC_V3_TABLE_FIELDS:
        raise SystemExit(f"HelioC V3 table has unknown or missing fields: {path.name}")
    if fields["table_kind"] not in {"binding", "sampler"} \
            or fields["set_role"] not in HELIOC_SYMBOLIC_V2_DESCRIPTOR_SET_ROLES \
            or not re.fullmatch(r"[1-9][0-9]*", fields["stage"]) \
            or not re.fullmatch(r"0x[0-9a-f]+", fields["raw_va"]) \
            or not re.fullmatch(r"[1-9][0-9]*", fields["bytes"]) \
            or not re.fullmatch(r"[1-9][0-9]*", fields["entry_count"]):
        raise SystemExit(f"HelioC V3 table has a non-canonical field: {path.name}")
    entry_count = int(fields["entry_count"])
    entry_roles = tuple(fields["entry_roles"].split(","))
    if len(entry_roles) != entry_count or any(not re.fullmatch(
            r"(?:descriptor_set_state|volume_a|volume_b|sim_params|render_params|output_target|sampler_state)",
            role) for role in entry_roles):
        raise SystemExit(f"HelioC V3 table has malformed entry roles: {path.name}")
    byte_count = int(fields["bytes"])
    expected_bytes = entry_count * (4 if fields["table_kind"] == "binding" else 16)
    if byte_count != expected_bytes or len(fields["data_hex"]) != byte_count * 2 \
            or re.fullmatch(r"[0-9a-f]+", fields["data_hex"]) is None \
            or int(fields["raw_va"], 16) == 0:
        raise SystemExit(f"HelioC V3 table has an invalid raw payload: {path.name}")
    return {
        "file": path.name,
        "table_kind": fields["table_kind"],
        "set_role": fields["set_role"],
        "stage": int(fields["stage"]),
        "raw_va": int(fields["raw_va"], 16),
        "bytes": byte_count,
        "entry_count": entry_count,
        "entry_roles": entry_roles,
        "raw_data_hex": fields["data_hex"],
        "raw_byte_fingerprint_sha256": sha256(bytes.fromhex(fields["data_hex"])),
    }


def collect_helioc_address_free_indirect_templates(exec_dir: Path) -> list[dict[str, object]] | None:
    """Read the V5 descriptor templates without retaining process addresses."""
    paths = sorted(exec_dir.glob("helioc-anv-v5-indirect-[0-9]*.txt"))
    if not paths:
        return None
    if len(paths) != 6:
        raise SystemExit("HelioC V5 must contain exactly six indirect templates")
    expected = {
        ("compute_ping_a", "volume_a", "sampled", "1", "8"),
        ("compute_ping_a", "volume_b", "storage", "3", "32"),
        ("compute_ping_b", "volume_b", "sampled", "1", "8"),
        ("compute_ping_b", "volume_a", "storage", "3", "32"),
        ("graphics_a", "volume_a", "sampled", "1", "8"),
        ("graphics_b", "volume_b", "sampled", "1", "8"),
    }
    templates: list[dict[str, object]] = []
    for path in paths:
        lines = path.read_text(encoding="ascii").splitlines()
        if not lines or lines[0] != "TRUEOS_HELIOC_INDIRECT_TEMPLATE_V5":
            raise SystemExit(f"HelioC V5 indirect template has invalid magic: {path.name}")
        fields: dict[str, str] = {}
        relocs: list[str] = []
        for line in lines[1:]:
            key, separator, value = line.partition("=")
            if not separator:
                raise SystemExit(f"HelioC V5 indirect template is malformed: {path.name}")
            if key == "reloc":
                relocs.append(value)
            elif key in fields:
                raise SystemExit(f"HelioC V5 indirect template duplicates a field: {path.name}")
            else:
                fields[key] = value
        if tuple(fields) != HELIOC_ADDRESS_FREE_INDIRECT_FIELDS[:-1] \
                or (fields["set_role"], fields["resource_role"], fields["kind"],
                    fields["binding"], fields["bytes"]) not in expected \
                or not re.fullmatch(r"(?:[0-9a-f]{2})+", fields["data_hex"]) \
                or len(fields["data_hex"]) != int(fields["bytes"]) * 2:
            raise SystemExit(f"HelioC V5 indirect template has invalid fields: {path.name}")
        resource = fields["resource_role"]
        expected_relocs = (
            [f"0,4,object_offset,surface_{resource}_sampled,0,0xffffffc0,0",
             "4,4,object_offset,sampler_state,0,0xffffffff,0"]
            if fields["kind"] == "sampled" else
            [f"0,4,object_offset,surface_{resource}_storage,0,0xffffffc0,0",
             f"8,8,fixed_gpu,{resource},0,0xffffffffffffffff,0"]
        )
        if relocs != expected_relocs:
            raise SystemExit(f"HelioC V5 indirect template has unexpected typed relocations: {path.name}")
        templates.append({
            "set_role": fields["set_role"], "resource_role": resource,
            "kind": fields["kind"], "binding": int(fields["binding"]),
            "data_hex": fields["data_hex"], "relocations": relocs,
        })
    if {(str(t["set_role"]), str(t["resource_role"]), str(t["kind"]),
         str(t["binding"]), str(len(bytes.fromhex(str(t["data_hex"])))) ) for t in templates} != expected:
        raise SystemExit("HelioC V5 indirect templates are duplicate or incomplete")
    return templates


def collect_helioc_address_free_surface_templates(exec_dir: Path) -> dict[str, object] | None:
    """Validate gfx120 ISL image-state templates and their typed fields.

    V6 is intentionally narrower than a complete HELIOCRS object: it proves
    only the two sampled/storage volume encodings and the dynamic UI4 render
    target encoding. Capture allocation offsets are checked for alignment but
    never retained as package data.
    """
    paths = sorted(exec_dir.glob("helioc-anv-v6-surface-[0-9]*.txt"))
    if not paths:
        return None
    expected_counts: Counter[tuple[str, str]] = Counter({
        ("volume_a", "sampled"): 1,
        ("volume_a", "storage"): 1,
        ("volume_b", "sampled"): 1,
        ("volume_b", "storage"): 1,
        # One target is packed for each of the six named command variants.
        ("ui4", "target"): 6,
    })
    if len(paths) != sum(expected_counts.values()):
        raise SystemExit(
            f"HelioC V6 must contain exactly {sum(expected_counts.values())} surface templates"
        )

    observed: Counter[tuple[str, str]] = Counter()
    canonical: dict[tuple[str, str], tuple[str, tuple[str, ...]]] = {}
    for path in paths:
        try:
            lines = path.read_text(encoding="ascii").splitlines()
        except UnicodeDecodeError as error:
            raise SystemExit(f"HelioC V6 surface template is not ASCII: {path.name}") from error
        if not lines or lines[0] != "TRUEOS_HELIOC_SURFACE_TEMPLATE_V6":
            raise SystemExit(f"HelioC V6 surface template has invalid magic: {path.name}")
        fields: dict[str, str] = {}
        relocs: list[str] = []
        for line in lines[1:]:
            key, separator, value = line.partition("=")
            if not separator:
                raise SystemExit(f"HelioC V6 surface template is malformed: {path.name}")
            if key == "reloc":
                relocs.append(value)
            elif key in fields:
                raise SystemExit(f"HelioC V6 surface template duplicates a field: {path.name}")
            else:
                fields[key] = value
        key = (fields.get("role", ""), fields.get("kind", ""))
        if tuple(fields) != HELIOC_ADDRESS_FREE_SURFACE_FIELDS[:-1] \
                or key not in expected_counts \
                or fields["bytes"] != "64" \
                or re.fullmatch(r"[0-9]+", fields["state_offset"]) is None \
                or int(fields["state_offset"]) % 64 \
                or re.fullmatch(r"(?:[0-9a-f]{2}){64}", fields["data_hex"]) is None:
            raise SystemExit(f"HelioC V6 surface template has invalid fields: {path.name}")
        expected_relocs = (
            [
                "32,8,runtime_gpu,ui4,0,0xffffffffffffffff,0",
                "8,4,runtime_u32,width,0,0x00003fff,-1",
                "8,4,runtime_u32,height,0,0x3fff0000,-1",
                "12,4,runtime_u32,pitch,0,0x0003ffff,-1",
            ]
            if key == ("ui4", "target")
            else [f"32,8,fixed_gpu,{key[0]},0,0xffffffffffffffff,0"]
        )
        if relocs != expected_relocs:
            raise SystemExit(f"HelioC V6 surface template has unexpected typed relocations: {path.name}")
        data = bytes.fromhex(fields["data_hex"])
        if any(data[32:40]):
            raise SystemExit(f"HelioC V6 surface template retained an image address: {path.name}")
        if key == ("ui4", "target") and (
            _helioc_u32(data, 8) & (0x0000_3FFF | 0x3FFF_0000)
            or _helioc_u32(data, 12) & 0x0003_FFFF
        ):
            raise SystemExit(f"HelioC V6 UI4 template retained dynamic extent state: {path.name}")
        value = (fields["data_hex"], tuple(relocs))
        if key in canonical and canonical[key] != value:
            raise SystemExit(f"HelioC V6 repeated surface template disagrees: {key}")
        canonical[key] = value
        observed[key] += 1

    if observed != expected_counts or set(canonical) != set(expected_counts):
        raise SystemExit("HelioC V6 surface templates are duplicate or incomplete")
    return {
        "schema": 6,
        "status": "address_free_image_surface_templates",
        "templates": [
            {
                "role": role,
                "kind": kind,
                "bytes": 64,
                "capture_instances": observed[(role, kind)],
                "data_hex": canonical[(role, kind)][0],
                "relocations": list(canonical[(role, kind)][1]),
            }
            for role, kind in sorted(canonical)
        ],
        "boundary": (
            "volume sampled/storage and runtime-sized UI4 target states are normalized; "
            "buffer/descriptor-set state, tables, program state, and command packets still require "
            "source-level typed relocation capture"
        ),
    }


def collect_helioc_address_free_v7_templates(exec_dir: Path) -> dict[str, object] | None:
    """Validate the eight named gfx120 buffer/descriptor-set templates.

    V7 records no ANV virtual address or pool allocation offset.  It is still
    only a narrow source-level slice: the typed descriptor-payload object names
    are not package objects until later command/table ownership capture binds
    them into HELIOCRS v2.
    """
    paths = sorted(exec_dir.glob("helioc-anv-v7-surface-[0-9]*.txt"))
    if not paths:
        return None
    compute_layout = "0:0:16:0:0,1:16:8:0:0,2:24:8:0:0,3:32:32:0:0"
    graphics_layout = "0:0:16:0:0,1:16:8:0:0,2:24:8:0:0"
    expected: dict[str, dict[str, str]] = {}
    for set_role in ("compute_ping_a", "compute_ping_b"):
        expected[f"buffer_surface_sim_params_{set_role}"] = {
            "kind": "buffer_surface", "resource_role": "sim_params", "set_role": set_role,
            "binding": "0", "descriptor_bytes": "64", "sampler_bytes": "0",
            "descriptor_layout": compute_layout, "resource_offset": "0", "resource_bytes": "112",
            "reloc": "32,8,fixed_gpu,sim_params,0,0xffffffffffffffff,0",
        }
        expected[f"descriptor_set_surface_{set_role}"] = {
            "kind": "descriptor_set_surface", "resource_role": f"descriptor_payload_{set_role}",
            "set_role": set_role, "binding": "4294967295", "descriptor_bytes": "64",
            "sampler_bytes": "0", "descriptor_layout": compute_layout,
            "resource_offset": "0", "resource_bytes": "64",
            "reloc": f"32,8,object_gpu,descriptor_payload_{set_role},0,0xffffffffffffffff,0",
        }
    for set_role in ("graphics_a", "graphics_b"):
        expected[f"buffer_surface_render_params_{set_role}"] = {
            "kind": "buffer_surface", "resource_role": "render_params", "set_role": set_role,
            "binding": "0", "descriptor_bytes": "32", "sampler_bytes": "0",
            "descriptor_layout": graphics_layout, "resource_offset": "128", "resource_bytes": "272",
            "reloc": "32,8,fixed_gpu,render_params,0,0xffffffffffffffff,0",
        }
        expected[f"descriptor_set_surface_{set_role}"] = {
            "kind": "descriptor_set_surface", "resource_role": f"descriptor_payload_{set_role}",
            "set_role": set_role, "binding": "4294967295", "descriptor_bytes": "32",
            "sampler_bytes": "0", "descriptor_layout": graphics_layout,
            "resource_offset": "0", "resource_bytes": "32",
            "reloc": f"32,8,object_gpu,descriptor_payload_{set_role},0,0xffffffffffffffff,0",
        }
    if len(paths) != len(expected):
        raise SystemExit(f"HelioC V7 must contain exactly {len(expected)} surface templates")

    records: dict[str, dict[str, object]] = {}
    repeated: dict[tuple[str, str], tuple[str, str, str, str, str, str]] = {}
    for path in paths:
        try:
            lines = path.read_text(encoding="ascii").splitlines()
        except UnicodeDecodeError as error:
            raise SystemExit(f"HelioC V7 surface template is not ASCII: {path.name}") from error
        if not lines or lines[0] != "TRUEOS_HELIOC_SURFACE_TEMPLATE_V7":
            raise SystemExit(f"HelioC V7 surface template has invalid magic: {path.name}")
        fields: dict[str, str] = {}
        relocs: list[str] = []
        for line in lines[1:]:
            key, separator, value = line.partition("=")
            if not separator:
                raise SystemExit(f"HelioC V7 surface template is malformed: {path.name}")
            if key == "reloc":
                relocs.append(value)
            elif key in fields:
                raise SystemExit(f"HelioC V7 surface template duplicates a field: {path.name}")
            else:
                fields[key] = value
        object_name = fields.get("object", "")
        wanted = expected.get(object_name)
        if tuple(fields) != HELIOC_ADDRESS_FREE_V7_FIELDS[:-1] or wanted is None \
                or fields["state_bytes"] != "64" or fields["state_alignment"] != "64" \
                or fields["descriptor_payload_offset"] != "0" \
                or not re.fullmatch(r"(?:[0-9a-f]{2}){64}", fields["data_hex"]):
            raise SystemExit(f"HelioC V7 surface template has invalid fields: {path.name}")
        if any(fields[key] != value for key, value in wanted.items() if key != "reloc") \
                or relocs != [wanted["reloc"]]:
            raise SystemExit(f"HelioC V7 surface template has an unexpected contract: {path.name}")
        data = bytes.fromhex(fields["data_hex"])
        if any(data[32:40]):
            raise SystemExit(f"HelioC V7 surface template retained a packed address: {path.name}")
        if object_name in records:
            raise SystemExit(f"HelioC V7 surface template is duplicated: {path.name}")
        repeated_key = (
            fields["kind"],
            ("compute" if fields["set_role"].startswith("compute_") else "graphics")
            if fields["kind"] == "descriptor_set_surface" else fields["resource_role"],
        )
        equivalence = (
            fields["descriptor_bytes"], fields["sampler_bytes"], fields["descriptor_layout"],
            fields["resource_offset"], fields["resource_bytes"], fields["data_hex"],
        )
        if repeated_key in repeated and repeated[repeated_key] != equivalence:
            raise SystemExit(f"HelioC V7 repeated template disagrees: {object_name}")
        repeated[repeated_key] = equivalence
        records[object_name] = {
            "object": object_name, "kind": fields["kind"],
            "resource_role": fields["resource_role"], "set_role": fields["set_role"],
            "binding": int(fields["binding"]), "state_bytes": 64,
            "state_alignment": 64, "descriptor_bytes": int(fields["descriptor_bytes"]),
            "sampler_bytes": int(fields["sampler_bytes"]),
            "descriptor_layout": fields["descriptor_layout"],
            "resource_offset": int(fields["resource_offset"]),
            "resource_bytes": int(fields["resource_bytes"]), "data_hex": fields["data_hex"],
            "relocations": relocs,
        }
    if set(records) != set(expected):
        raise SystemExit("HelioC V7 surface templates are duplicate or incomplete")
    return {
        "schema": 7,
        "status": "address_free_buffer_descriptor_surface_templates",
        "templates": [records[object_name] for object_name in sorted(records)],
        "boundary": (
            "buffer and descriptor-set surface states are normalized; descriptor payload contents, tables, "
            "sampler/program state, SBA, and command packets still require source-level typed relocation capture"
        ),
    }


def collect_helioc_address_free_v8_tables(exec_dir: Path) -> dict[str, object] | None:
    """Validate exactly four address-free binding tables and samplers."""
    binding_paths = sorted(exec_dir.glob("helioc-anv-v8-binding-[0-9]*.txt"))
    sampler_paths = sorted(exec_dir.glob("helioc-anv-v8-sampler-[0-9]*.txt"))
    if not binding_paths and not sampler_paths:
        return None
    if len(binding_paths) != 4 or len(sampler_paths) != 4:
        raise SystemExit("HelioC V8 requires exactly four binding and four sampler records")
    bindings: dict[str, dict[str, object]] = {}
    stages = {"compute_ping_a": 5, "compute_ping_b": 5, "graphics_a": 4, "graphics_b": 4}
    for path in binding_paths:
        fields: dict[str, str] = {}
        relocs: list[str] = []
        try:
            lines = path.read_text(encoding="ascii").splitlines()
        except UnicodeDecodeError as error:
            raise SystemExit(f"HelioC V8 binding record is not ASCII: {path.name}") from error
        if not lines or lines[0] != "TRUEOS_HELIOC_BINDING_TABLE_V8":
            raise SystemExit(f"HelioC V8 binding record has invalid magic: {path.name}")
        for line in lines[1:]:
            if line.startswith("reloc="):
                relocs.append(line.removeprefix("reloc="))
                continue
            key, separator, value = line.partition("=")
            if not separator:
                raise SystemExit(f"HelioC V8 binding record is malformed: {path.name}")
            elif key in fields:
                raise SystemExit(f"HelioC V8 binding record duplicates {key}: {path.name}")
            else:
                fields[key] = value
        role = fields.get("table_role", "")
        expected = HELIOC_ADDRESS_FREE_V8_BINDING_ROLES.get(role)
        if expected is None or role in bindings:
            raise SystemExit(f"HelioC V8 has an unexpected or duplicate table role: {path.name}")
        if (fields.get("stage") != str(stages[role]) or fields.get("bytes") != "16"
                or fields.get("live_entry_count") != "4" or fields.get("entry_count") != "4"
                or tuple(fields.get("entry_roles", "").split(",")) != expected
                or fields.get("data_hex") != "00" * 16):
            raise SystemExit(f"HelioC V8 binding contract mismatch: {path.name}")
        expected_relocs = [f"{offset * 4},4,object_offset,{name},0,0xffffffff,0"
                           for offset, name in enumerate(HELIOC_ADDRESS_FREE_V8_RELOC_SOURCES[role])]
        if relocs != expected_relocs:
            raise SystemExit(f"HelioC V8 binding relocations mismatch: {path.name}")
        bindings[role] = {"role": role, "stage": stages[role], "bytes": 16,
                          "entry_roles": list(expected), "data_hex": fields["data_hex"],
                          "relocations": relocs}
    sampler_hex: str | None = None
    samplers: list[dict[str, object]] = []
    sampler_roles: set[str] = set()
    for path in sampler_paths:
        fields: dict[str, str] = {}
        try:
            lines = path.read_text(encoding="ascii").splitlines()
        except UnicodeDecodeError as error:
            raise SystemExit(f"HelioC V8 sampler record is not ASCII: {path.name}") from error
        if not lines or lines[0] != "TRUEOS_HELIOC_SAMPLER_STATE_V8":
            raise SystemExit(f"HelioC V8 sampler record has invalid magic: {path.name}")
        for line in lines[1:]:
            key, separator, value = line.partition("=")
            if not separator or key in fields:
                raise SystemExit(f"HelioC V8 sampler record is malformed: {path.name}")
            fields[key] = value
        if set(fields) != {"table_role", "stage", "bytes", "canonical_group", "data_hex"}:
            raise SystemExit(f"HelioC V8 sampler record has unexpected fields: {path.name}")
        role = fields.get("table_role", "")
        data_hex = fields.get("data_hex", "")
        if (role not in HELIOC_ADDRESS_FREE_V8_BINDING_ROLES
                or role in sampler_roles
                or fields.get("stage") != str(stages[role]) or fields.get("bytes") != "16"
                or fields.get("canonical_group") != "helioc-cloud-sampler"
                or data_hex != "00401250010000000082040010e00700"):
            raise SystemExit(f"HelioC V8 sampler contract mismatch: {path.name}")
        sampler_roles.add(role)
        if sampler_hex is None:
            sampler_hex = data_hex
        elif sampler_hex != data_hex:
            raise SystemExit(f"HelioC V8 sampler instances disagree: {path.name}")
        samplers.append({"table_role": role, "stage": stages[role], "bytes": 16,
                         "data_hex": data_hex})
    if sampler_roles != set(HELIOC_ADDRESS_FREE_V8_BINDING_ROLES):
        raise SystemExit("HelioC V8 sampler roles are incomplete")
    return {"schema": 8, "status": "address_free_binding_sampler_tables",
            "bindings": [bindings[role] for role in sorted(bindings)],
            "samplers": sorted(samplers, key=lambda item: str(item["table_role"]))}


def collect_helioc_address_free_v9_payloads(exec_dir: Path) -> dict[str, object] | None:
    """Validate the four normalized descriptor-payload objects emitted by V9."""
    paths = sorted(exec_dir.glob("helioc-anv-v9-payload-[0-9]*.txt"))
    if not paths:
        return None
    if len(paths) != 4:
        raise SystemExit("HelioC V9 requires exactly four descriptor payload records")
    records: dict[str, dict[str, object]] = {}
    for path in paths:
        try:
            lines = path.read_text(encoding="ascii").splitlines()
        except UnicodeDecodeError as error:
            raise SystemExit(f"HelioC V9 payload is not ASCII: {path.name}") from error
        if not lines or lines[0] != "TRUEOS_HELIOC_DESCRIPTOR_PAYLOAD_V9":
            raise SystemExit(f"HelioC V9 payload has invalid magic: {path.name}")
        fields: dict[str, str] = {}
        relocs: list[str] = []
        for line in lines[1:]:
            if line.startswith("reloc="):
                relocs.append(line.removeprefix("reloc="))
                continue
            key, separator, value = line.partition("=")
            if not separator or key in fields:
                raise SystemExit(f"HelioC V9 payload is malformed: {path.name}")
            fields[key] = value
        required = {"object", "set_role", "stage", "bytes", "binding_count", "data_blake3", "data_hex"}
        if set(fields) != required:
            raise SystemExit(f"HelioC V9 payload has unexpected fields: {path.name}")
        role = fields["set_role"]
        spec = HELIOC_ADDRESS_FREE_V9_PAYLOADS.get(role)
        if spec is None or role in records or fields["object"] != f"descriptor_payload_{role}":
            raise SystemExit(f"HelioC V9 payload has unknown or duplicate role: {path.name}")
        stage, payload_bytes, binding_count, params, sampled, storage = spec
        expected_data = bytearray(payload_bytes)
        if storage is None:
            struct.pack_into("<I", expected_data, 8, 272)
        else:
            struct.pack_into("<I", expected_data, 8, 112)
            expected_data[32:64] = bytes.fromhex(
                "0000000060000000000000000000000000000000000300003000000084000000"
            )
        if (fields["stage"] != str(stage) or fields["bytes"] != str(payload_bytes)
                or fields["binding_count"] != str(binding_count)
                or fields["data_blake3"] != HELIOC_ADDRESS_FREE_V9_BLAKE3[role]
                or not re.fullmatch(rf"[0-9a-f]{{{payload_bytes * 2}}}", fields["data_hex"])
                or bytes.fromhex(fields["data_hex"]) != bytes(expected_data)):
            raise SystemExit(f"HelioC V9 payload contract mismatch: {path.name}")
        expected = [
            f"0,8,fixed_gpu,{params},0,0xffffffffffffffff,0",
            f"16,4,object_offset,surface_{sampled}_sampled,0,0xffffffc0,0",
            "28,4,object_offset,sampler_state,0,0xffffffff,0",
        ]
        if storage is not None:
            expected += [
                f"32,4,object_offset,surface_{storage}_storage,0,0xffffffc0,0",
                f"40,8,fixed_gpu,{storage},0,0xffffffffffffffff,0",
            ]
        if relocs != expected:
            raise SystemExit(f"HelioC V9 payload relocations mismatch: {path.name}")
        records[role] = {"object": fields["object"], "set_role": role, "stage": stage,
                         "bytes": payload_bytes, "binding_count": binding_count,
                         "data_blake3": fields["data_blake3"], "data_hex": fields["data_hex"],
                         "relocations": relocs}
    if set(records) != set(HELIOC_ADDRESS_FREE_V9_PAYLOADS):
        raise SystemExit("HelioC V9 descriptor payload roles are incomplete")
    for pair in (("compute_ping_a", "compute_ping_b"), ("graphics_a", "graphics_b")):
        left, right = (records[role] for role in pair)
        if left["data_hex"] != right["data_hex"] or left["data_blake3"] != right["data_blake3"]:
            raise SystemExit(f"HelioC V9 {pair[0]}/{pair[1]} payloads disagree")
    return {"schema": 9, "status": "address_free_descriptor_payloads",
            "payloads": [records[role] for role in sorted(records)]}


def collect_helioc_workload_slices_v10a(exec_dir: Path) -> dict[str, object] | None:
    """Collect raw V10A workload evidence; never treat it as package state."""
    paths = sorted(exec_dir.glob("helioc-anv-v10a-slice-[0-9]*.txt"))
    if not paths:
        return None
    if len(paths) != 6:
        raise SystemExit("HelioC V10A requires exactly six workload slice records")
    records: dict[tuple[str, str, str, str], dict[str, object]] = {}
    for path in paths:
        lines = path.read_text(encoding="ascii").splitlines()
        if not lines or lines[0] != "TRUEOS_HELIOC_WORKLOAD_SLICE_V10A":
            raise SystemExit(f"HelioC V10A slice has invalid magic: {path.name}")
        fields: dict[str, str] = {}
        for line in lines[1:]:
            key, separator, value = line.partition("=")
            if not separator or key in fields:
                raise SystemExit(f"HelioC V10A slice is malformed: {path.name}")
            fields[key] = value
        allowed = {"current", "step", "final_role", "dispatch_roles", "draw_vertices", "raw_va", "bytes", "data_hex"}
        if set(fields) != allowed:
            raise SystemExit(f"HelioC V10A slice has unexpected fields: {path.name}")
        key = (fields["current"], fields["step"], fields["final_role"], fields["dispatch_roles"])
        expected_draw = HELIOC_V10A_VARIANTS.get(key)
        if expected_draw is None or key in records or fields["draw_vertices"] != str(expected_draw):
            raise SystemExit(f"HelioC V10A slice is not an exact V4 variant: {path.name}")
        if (not re.fullmatch(r"0x[0-9a-f]+", fields["raw_va"])
                or int(fields["raw_va"], 16) == 0
                or not re.fullmatch(r"[1-9][0-9]{0,5}", fields["bytes"])
                or not re.fullmatch(r"[0-9a-f]+", fields["data_hex"])
                or int(fields["bytes"]) == 0 or int(fields["bytes"]) > 256 * 1024
                or int(fields["bytes"]) % 4 != 0
                or len(fields["data_hex"]) != int(fields["bytes"]) * 2):
            raise SystemExit(f"HelioC V10A slice has invalid raw diagnostic payload: {path.name}")
        data = bytes.fromhex(fields["data_hex"])
        if any(int.from_bytes(data[offset:offset + 4], "little") == 0x05000000
                for offset in range(0, len(data), 4)):
            raise SystemExit(f"HelioC V10A slice contains a batch-buffer-end dword: {path.name}")
        records[key] = {"current": key[0], "step": int(key[1]), "final_role": key[2],
                        "dispatch_roles": key[3], "draw_vertices": 3,
                        "raw_va": fields["raw_va"], "bytes": int(fields["bytes"]),
                        "data_hex": fields["data_hex"],
                        "sha256": hashlib.sha256(data).hexdigest()}
    return {"schema": "v10a", "status": "diagnostic_only_raw_workload_slices",
            "package_eligible": False, "slices": [records[key] for key in sorted(records)],
            "boundary": "raw_va and data_hex are diagnostic evidence only; not HELIOCRS state"}


def collect_helioc_command_catalog(exec_dir: Path) -> dict[str, object] | None:
    """Validate the six named raw command variants without normalizing bytes."""
    paths = sorted(exec_dir.glob("helioc-anv-v4-command-[0-9]*.txt"))
    if not paths:
        return None
    if len(paths) != 6:
        raise SystemExit("HelioC command catalog must contain exactly six variants")
    records: list[dict[str, object]] = []
    for path in paths:
        try:
            lines = path.read_text(encoding="ascii").splitlines()
        except UnicodeDecodeError as error:
            raise SystemExit(f"HelioC command catalog record is not ASCII: {path.name}") from error
        if not lines or lines[0] != "TRUEOS_HELIOC_COMMAND_V4":
            raise SystemExit(f"HelioC command catalog has an invalid magic: {path.name}")
        fields: dict[str, str] = {}
        for line in lines[1:]:
            key, separator, value = line.partition("=")
            if not separator or key in fields:
                raise SystemExit(f"HelioC command catalog record is malformed: {path.name}")
            fields[key] = value
        if tuple(fields) != HELIOC_COMMAND_CATALOG_FIELDS \
                or fields["current"] not in {"volume_a", "volume_b"} \
                or fields["final_role"] not in {"volume_a", "volume_b"} \
                or not re.fullmatch(r"[0-2]", fields["step"]) \
                or fields["dispatch_roles"] not in {"none", "a_to_b", "b_to_a", "a_to_b,b_to_a", "b_to_a,a_to_b"} \
                or fields["volume_layout"] != "general_to_general" \
                or fields["inter_dispatch_visibility"] not in {"none", "compute_write_to_compute_read"} \
                or fields["draw_vertices"] != "3" \
                or not re.fullmatch(r"0x[0-9a-f]+", fields["raw_va"]) \
                or not re.fullmatch(r"[1-9][0-9]*", fields["bytes"]):
            raise SystemExit(f"HelioC command catalog record has an invalid field: {path.name}")
        byte_count = int(fields["bytes"])
        if byte_count > 256 * 1024 or byte_count % 4 or len(fields["data_hex"]) != byte_count * 2 \
                or re.fullmatch(r"[0-9a-f]+", fields["data_hex"]) is None \
                or int(fields["raw_va"], 16) == 0:
            raise SystemExit(f"HelioC command catalog record has an invalid raw batch: {path.name}")
        raw = bytes.fromhex(fields["data_hex"])
        if struct.unpack_from("<I", raw, len(raw) - 4)[0] != 0x0500_0000:
            raise SystemExit(f"HelioC command catalog record does not end in MI_BATCH_BUFFER_END: {path.name}")
        records.append({
            "file": path.name, "current": fields["current"], "step": int(fields["step"]),
            "final_role": fields["final_role"], "dispatch_roles": fields["dispatch_roles"],
            "volume_layout": fields["volume_layout"],
            "inter_dispatch_visibility": fields["inter_dispatch_visibility"],
            "draw_vertices": 3, "raw_va": int(fields["raw_va"], 16), "bytes": byte_count,
            "raw_data_hex": fields["data_hex"], "raw_byte_fingerprint_sha256": sha256(raw),
        })
    expected = {
        ("volume_a", 0, "volume_a", "none", "none"),
        ("volume_a", 1, "volume_b", "a_to_b", "none"),
        ("volume_a", 2, "volume_a", "a_to_b,b_to_a", "compute_write_to_compute_read"),
        ("volume_b", 0, "volume_b", "none", "none"),
        ("volume_b", 1, "volume_a", "b_to_a", "none"),
        ("volume_b", 2, "volume_b", "b_to_a,a_to_b", "compute_write_to_compute_read"),
    }
    observed = {(str(record["current"]), int(record["step"]), str(record["final_role"]),
                 str(record["dispatch_roles"]), str(record["inter_dispatch_visibility"])) for record in records}
    if observed != expected or len(observed) != len(records):
        raise SystemExit("HelioC command catalog has an unexpected or duplicate variant")
    return {
        "schema": 4,
        "status": "diagnostic_raw_commands_require_symbolic_relocations",
        "variants": records,
        "boundary": (
            "raw command bytes remain address-bearing diagnostics; MI_BATCH_BUFFER_END and semantic labels do not "
            "prove relocatability, ownership, or physical execution"
        ),
    }


def collect_helioc_symbolic_v2(exec_dir: Path) -> dict[str, object] | None:
    """Validate V2 symbolic evidence, retaining raw VAs as diagnostic-only.

    This deliberately does not create relocations: runtime contents and output
    backing are broker resources, while only the captured state templates may
    be hashed.  Any missing role or ambiguous identity rejects the whole V2
    slice rather than allowing the prior raw-state path to look relocatable.
    """
    paths = sorted(exec_dir.glob("helioc-anv-v2-[0-9]*.txt"))
    if not paths:
        return None
    records = [_parse_helioc_symbolic_v2_record(path) for path in paths]
    descriptor_paths = sorted(exec_dir.glob("helioc-anv-v2-descriptor-[0-9]*.txt"))
    if len(descriptor_paths) != 6:
        raise SystemExit("HelioC V2 capture does not contain exactly six indirect descriptors")
    indirect_descriptors = [
        _parse_helioc_symbolic_v2_indirect_descriptor(path) for path in descriptor_paths
    ]
    expected_indirect_descriptors: Counter[tuple[str, str, str, int, int]] = Counter({
        ("compute_ping_a", "volume_a", "sampled", 1, 8): 1,
        ("compute_ping_a", "volume_b", "storage", 3, 32): 1,
        ("compute_ping_b", "volume_b", "sampled", 1, 8): 1,
        ("compute_ping_b", "volume_a", "storage", 3, 32): 1,
        ("graphics_a", "volume_a", "sampled", 1, 8): 1,
        ("graphics_b", "volume_b", "sampled", 1, 8): 1,
    })
    if Counter(
        (str(record["set_role"]), str(record["resource_role"]), str(record["kind"]),
         int(record["binding"]), int(record["bytes"]))
        for record in indirect_descriptors
    ) != expected_indirect_descriptors:
        raise SystemExit("HelioC V2 capture has an unexpected indirect-descriptor multiplicity")
    table_paths = sorted(exec_dir.glob("helioc-anv-v3-table-[0-9]*.txt"))
    if len(table_paths) != 8:
        raise SystemExit("HelioC V3 capture does not contain exactly eight binding/sampler tables")
    tables = [_parse_helioc_symbolic_v3_table(path) for path in table_paths]
    expected_tables: Counter[tuple[str, str, int, int, tuple[str, ...]]] = Counter({
        ("binding", "compute_ping_a", 5, 16,
         ("descriptor_set_state", "volume_b", "sim_params", "volume_a")): 1,
        ("sampler", "compute_ping_a", 5, 16, ("sampler_state",)): 1,
        ("binding", "compute_ping_b", 5, 16,
         ("descriptor_set_state", "volume_a", "sim_params", "volume_b")): 1,
        ("sampler", "compute_ping_b", 5, 16, ("sampler_state",)): 1,
        ("binding", "graphics_a", 4, 16,
         ("output_target", "descriptor_set_state", "volume_a", "render_params")): 1,
        ("sampler", "graphics_a", 4, 16, ("sampler_state",)): 1,
        ("binding", "graphics_b", 4, 16,
         ("output_target", "descriptor_set_state", "volume_b", "render_params")): 1,
        ("sampler", "graphics_b", 4, 16, ("sampler_state",)): 1,
    })
    if Counter(
        (str(record["table_kind"]), str(record["set_role"]), int(record["stage"]),
         int(record["bytes"]), tuple(record["entry_roles"]))
        for record in tables
    ) != expected_tables:
        raise SystemExit("HelioC V3 capture has an unexpected table semantic mapping")
    resource_records = [record for record in records if record["kind"] in {"image", "buffer", "heap", "command"}]
    roles = {str(record["role"]) for record in resource_records}
    if roles != HELIOC_SYMBOLIC_V2_REQUIRED_ROLES:
        raise SystemExit("HelioC V2 capture has missing or unexpected semantic resources")
    resources: dict[str, dict[str, object]] = {}
    for role in sorted(HELIOC_SYMBOLIC_V2_REQUIRED_ROLES):
        matches = [record for record in resource_records if record["role"] == role]
        identities = {
            (record["kind"], record["raw_va"], record["allocation_bytes"], record["logical_bytes"],
             record["resource_offset"], record["row_pitch"], record["array_pitch"])
            for record in matches
        }
        if len(identities) != 1:
            raise SystemExit(f"HelioC V2 capture has ambiguous {role} identity")
        resources[role] = {key: matches[0][key] for key in (
            "kind", "raw_va", "allocation_bytes", "logical_bytes", "resource_offset",
            "row_pitch", "array_pitch",
        )}
    for role in ("volume_a", "volume_b"):
        resource = resources[role]
        if resource["kind"] != "image" or (resource["logical_bytes"], resource["row_pitch"], resource["array_pitch"]) \
                != (3_538_944, 768, 36_864):
            raise SystemExit(f"HelioC V2 {role} is not the sealed tight LINEAR volume")
    for role, stage, logical_bytes in (("sim_params", 5, 112), ("render_params", 4, 272)):
        resource = resources[role]
        if resource["kind"] != "buffer" or resource["logical_bytes"] != logical_bytes \
                or not any(record["role"] == role and record["stage"] == stage and record["binding"] == 0
                           for record in records):
            raise SystemExit(f"HelioC V2 {role} has the wrong semantic binding/range")
    resource_bindings = Counter(
        (str(record["role"]), str(record["kind"]), int(record["stage"]), int(record["binding"]))
        for record in resource_records
    )
    expected_resource_bindings: Counter[tuple[str, str, int, int]] = Counter({
        ("volume_a", "image", 5, 1): 1,
        ("volume_a", "image", 5, 3): 1,
        ("volume_a", "image", 4, 1): 1,
        ("volume_b", "image", 5, 1): 1,
        ("volume_b", "image", 5, 3): 1,
        ("sim_params", "buffer", 5, 0): 2,
        ("render_params", "buffer", 4, 0): 1,
        # Color-attachment bindings use ANV_DESCRIPTOR_SET_COLOR_ATTACHMENTS;
        # its map's binding field is the UINT32_MAX sentinel, not descriptor 0.
        ("output_target", "image", 4, 0xFFFF_FFFF): 1,
        ("shader_heap", "heap", 0xFFFF_FFFF, 0): 1,
        ("internal_surface_state_heap", "heap", 0xFFFF_FFFF, 0): 1,
        ("binding_table_heap", "heap", 0xFFFF_FFFF, 0): 1,
        ("dynamic_state_heap", "heap", 0xFFFF_FFFF, 0): 1,
        ("indirect_descriptor_heap", "heap", 0xFFFF_FFFF, 0): 1,
        ("command_bo", "command", 0xFFFF_FFFF, 0): 1,
    })
    if resource_bindings != expected_resource_bindings:
        raise SystemExit("HelioC V2 capture has an unexpected semantic binding multiplicity")
    if resources["output_target"]["kind"] != "image":
        raise SystemExit("HelioC V2 output target is not an image")
    state_records = [record for record in records if record["kind"] in {
        "surface_state", "render_target_state", "descriptor_set_state", "state",
    }]
    if not any(record["kind"] == "render_target_state" and record["role"] == "output_target" for record in state_records):
        raise SystemExit("HelioC V2 capture has no resolved output-target state")
    for role in ("volume_a", "volume_b", "sim_params", "render_params"):
        if not any(record["kind"] == "surface_state" and record["role"] == role for record in state_records):
            raise SystemExit(f"HelioC V2 capture has no resolved {role} surface state")
    if not any(record["role"] == "sampler_state" and record["kind"] == "state" for record in records):
        raise SystemExit("HelioC V2 capture has no resolved sampler state")
    descriptor_set_states = [record for record in records if record["kind"] == "descriptor_set_state"]
    expected_descriptor_set_states: Counter[tuple[str, int, int, int]] = Counter({
        # ANV_DESCRIPTOR_SET_DESCRIPTORS encodes the set-buffer surface in
        # bind-map surface[0]; its binding is the UINT32_MAX special-set
        # sentinel rather than logical Vulkan binding 0.
        ("compute_ping_a", 5, 0xFFFF_FFFF, 0): 1,
        ("compute_ping_b", 5, 0xFFFF_FFFF, 128): 1,
        ("graphics_a", 4, 0xFFFF_FFFF, 256): 1,
    })
    if Counter(
        (str(record["role"]), int(record["stage"]), int(record["binding"]), int(record["state_offset"]))
        for record in descriptor_set_states
    ) != expected_descriptor_set_states:
        raise SystemExit("HelioC V2 capture has no exact descriptor-set-buffer SURFACE_STATE trace")
    for record in descriptor_set_states:
        if int(record["logical_bytes"]) != 64 or int(record["state_bytes"]) != 64 \
                or record["raw_state_byte_fingerprint_sha256"] is None \
                or int(record["state_heap_va"]) == 0 \
                or int(record["raw_va"]) != int(record["state_heap_va"]) + int(record["state_offset"]) \
                or int(record["allocation_bytes"]) != int(record["state_heap_bytes"]) \
                or int(record["resource_offset"]) != 0 \
                or int(record["row_pitch"]) != 0 or int(record["array_pitch"]) != 0:
            raise SystemExit("HelioC V2 descriptor-set-buffer SURFACE_STATE is incomplete")
    return {
        "schema": 2,
        "status": "diagnostic_raw_va_requires_broker_ownership_and_patch_sites",
        "resources": resources,
        "raw_address_bearing_state_records": [
            {key: record[key] for key in ("role", "kind", "stage", "binding", "raw_va", "state_heap_va",
                                           "state_offset", "state_bytes", "raw_state_byte_fingerprint_sha256")}
            for record in state_records
        ],
        "raw_indirect_descriptor_records": indirect_descriptors,
        "raw_table_records": tables,
        "raw_state_boundary": (
            "state bytes and their fingerprints are capture-process diagnostics only; they must be decoded and "
            "all address fields symbolically rewritten before any immutable template can exist"
        ),
        "indirect_descriptor_boundary": (
            "six shader-loaded 8-byte sampled/32-byte storage payloads and their descriptor-set-buffer "
            "SURFACE_STATE are captured only as raw diagnostics; decode and rewrite all embedded handles/VAs "
            "before any immutable template can exist"
        ),
        "table_boundary": (
            "binding/sampler table bytes and entry roles are raw diagnostics only; state offsets and embedded "
            "addresses still require decoding, symbolic patch sites, and broker ownership before packaging"
        ),
        "command_dependency_boundary": (
            "command BO and state heaps are identified, but ANV relocation tracking is a BO-dependency bitset "
            "without symbolic patch sites or broker ownership proof"
        ),
        "runtime_resource_boundary": (
            "volume/parameter/UI4 contents are dynamic semantic broker resources and are intentionally unhashed; "
            "UI4 capture VA is not a permitted runtime address"
        ),
    }


def collect_instrumented_anv_capture(exec_dir: Path) -> dict[str, object] | None:
    """Parse the exact, opt-in capture records; reject any stage ambiguity."""
    trace_files = sorted(exec_dir.glob("helioc-anv-*.bin"))
    if not trace_files:
        return None
    records: list[dict[str, object]] = []
    for path in trace_files:
        data = path.read_bytes()
        if len(data) < 28:
            raise SystemExit(f"instrumented ANV record is truncated: {path.name}")
        magic, version, kind, stage_or_type, binding, element, payload = struct.unpack_from("<7I", data)
        if magic != 0x48434D56 or version != 1 or payload != len(data) - 28:
            raise SystemExit(f"instrumented ANV record has invalid header/length: {path.name}")
        records.append({"file": path.name, "kind": kind, "stage_or_type": stage_or_type,
                        "binding": binding, "element": element, "bytes": payload,
                        "sha256": sha256(data[28:])})
    _validate_instrumented_anv_records(records)
    reps: dict[str, dict[str, list[dict[str, object]]]] = {}
    expected_variants = {"compute": 1, "vertex": 1, "fragment": 2}
    for stage, expected_count in expected_variants.items():
        reps[stage] = {}
        for label, suffix in (("bind_map", "TRUEOS_HelioC_bind_map.txt"),
                              ("shader_serialize", "TRUEOS_HelioC_shader_serialize.bin"),
                              # This raw struct remains diagnostic only.  The
                              # serializer above is ANV's canonical complete record.
                              ("diagnostic_raw_prog_data", "TRUEOS_HelioC_prog_data.bin"),
                              ("devinfo", "TRUEOS_HelioC_devinfo.txt")):
            matches = sorted(exec_dir.glob(f"*_{stage}_*_{suffix}"))
            if len(matches) != expected_count:
                raise SystemExit(
                    f"instrumented ANV capture needs exactly {expected_count} {stage} {label} "
                    f"representation(s), got {len(matches)}"
                )
            entries: list[dict[str, object]] = []
            for path in matches:
                data = path.read_bytes()
                if not data:
                    raise SystemExit(f"instrumented ANV {stage} {label} is empty: {path.name}")
                entries.append({"file": path.name, "bytes": len(data), "sha256": sha256(data)})
            if len({entry["file"] for entry in entries}) != expected_count:
                raise SystemExit(f"instrumented ANV {stage} {label} filenames are ambiguous")
            reps[stage][label] = entries
            if label == "devinfo":
                for path in matches:
                    raw = path.read_bytes()
                    if not raw.endswith(b"\0") or b"\0" in raw[:-1]:
                        raise SystemExit(
                            f"instrumented ANV {stage} devinfo has an invalid text terminator"
                        )
                    text = raw[:-1].decode("ascii", errors="strict").strip()
                    match = re.fullmatch(
                        r"TRUEOS_HELIOC_DEVINFO_V1 pci_device_id=0x([0-9a-fA-F]{4}) "
                        r"pci_revision_id=0x([0-9a-fA-F]{2}) revision=(\d+) kmd_type=(\d+) "
                        r"ver=(\d+) verx10=(\d+)", text,
                    )
                    if not match or (
                        int(match.group(1), 16), int(match.group(2), 16), int(match.group(3)),
                        int(match.group(5)), int(match.group(6)),
                    ) != (0x4680, 0x0C, 0, 12, 120):
                        raise SystemExit(
                            "instrumented ANV devinfo is not shim-injected ADL GT1 "
                            "(0x4680, PCI r0c, KMD revision 0, ver=12, verx10=120)"
                        )
    return {
        "records": records,
        "stages": reps,
        "symbolic_v2": (symbolic_v2 := collect_helioc_symbolic_v2(exec_dir)),
        "command_catalog": (command_catalog := collect_helioc_command_catalog(exec_dir)),
        "address_free_indirect_templates": collect_helioc_address_free_indirect_templates(exec_dir),
        "address_free_surface_templates": collect_helioc_address_free_surface_templates(exec_dir),
        "address_free_buffer_descriptor_templates": collect_helioc_address_free_v7_templates(exec_dir),
        "address_free_binding_sampler_tables": collect_helioc_address_free_v8_tables(exec_dir),
        "address_free_descriptor_payloads": collect_helioc_address_free_v9_payloads(exec_dir),
        "diagnostic_workload_slices": collect_helioc_workload_slices_v10a(exec_dir),
        # V2/V3 captures are intentionally one explicit named catalog anchor,
        # rather than an order-dependent first record.  The raw command
        # catalog remains diagnostic only and no address-bearing bytes are
        # normalized here.
        "symbolic_v2_catalog_anchor": (
            "current=volume_a step=2 final_role=volume_a "
            "dispatch_roles=a_to_b,b_to_a"
            if symbolic_v2 is not None and command_catalog is not None else None
        ),
    }


def validate_instrumented_shader_serializations(
    exec_dir: Path, compute_isa: bytes, vertex_isa: bytes, fragment_simd16_isa: bytes,
) -> None:
    """Cross-check cache recovery against ANV's canonical shader serializer."""

    def serialized_code(path: Path, expected_stage: int) -> tuple[bytes, bytes]:
        data = path.read_bytes()
        if len(data) < 8:
            raise SystemExit(f"instrumented ANV shader serialization is truncated: {path.name}")
        stage, program_bytes = struct.unpack_from("<II", data)
        if stage != expected_stage or program_bytes == 0 or 8 + program_bytes > len(data):
            raise SystemExit(f"instrumented ANV shader serialization has invalid stage/size: {path.name}")
        return data[8:8 + program_bytes], data

    compute_files = sorted(exec_dir.glob("*_compute_*_TRUEOS_HelioC_shader_serialize.bin"))
    vertex_files = sorted(exec_dir.glob("*_vertex_*_TRUEOS_HelioC_shader_serialize.bin"))
    fragment_files = sorted(exec_dir.glob("*_fragment_*_TRUEOS_HelioC_shader_serialize.bin"))
    if (len(compute_files), len(vertex_files), len(fragment_files)) != (1, 1, 2):
        raise SystemExit("instrumented ANV canonical shader serialization set is incomplete")

    compute_code, _ = serialized_code(compute_files[0], 5)
    vertex_code, _ = serialized_code(vertex_files[0], 0)
    fragment_code_a, fragment_serial_a = serialized_code(fragment_files[0], 4)
    fragment_code_b, fragment_serial_b = serialized_code(fragment_files[1], 4)
    if compute_code != compute_isa or vertex_code != vertex_isa:
        raise SystemExit("instrumented ANV canonical CS/VS bytes disagree with cache/assembly recovery")
    # Both fragment executable indices are views into the same ANV shader
    # object; its serializer must therefore be byte-identical for SIMD8/16.
    if fragment_serial_a != fragment_serial_b:
        raise SystemExit("instrumented ANV fragment executable serializations disagree")

    fragment_assemblies = sorted(exec_dir.glob("*_fragment_*_GEN_Assembly.txt"))
    if len(fragment_assemblies) != 2:
        raise SystemExit("instrumented ANV capture needs exact SIMD8/16 fragment assemblies")
    first_bytes = assembly_code_size(fragment_assemblies[0])
    simd16_offset = (first_bytes + 63) & ~63
    if (
        simd16_offset + len(fragment_simd16_isa) != len(fragment_code_a)
        or fragment_code_a[simd16_offset:] != fragment_simd16_isa
    ):
        raise SystemExit("instrumented ANV canonical FS bytes disagree with SIMD16 recovery")


def _validate_instrumented_anv_records(records: list[dict[str, object]]) -> None:
    """Require the complete one-pipeline ANV descriptor/state trace.

    Gfx120's indirect descriptor path does not visit the ANV_DESCRIPTOR_SURFACE
    or ANV_DESCRIPTOR_SAMPLER hooks, so kinds 1/2 are not required here.  The
    observed complete trace has two compute binding-table/sampler-map flushes,
    one fragment flush of each, and one final command batch (kind 5). A partial
    trace is evidence of an incomplete capture, never a valid subset from which
    runtime state may be inferred.
    """
    key_counts = Counter(
        (
            int(record["kind"]),
            int(record["stage_or_type"]),
            int(record["binding"]),
            int(record["element"]),
        )
        for record in records
        if int(record["kind"]) != 5
    )
    expected: Counter[tuple[int, int, int, int]] = Counter({
        # Mesa shader stages: fragment=4, compute=5.  The compute pipeline
        # flushes twice (one per ping-pong descriptor set); the graphics
        # pipeline has no vertex binding-table/sampler state in this capture.
        (3, 5, 0, 0): 2,
        (3, 4, 0, 0): 1,
        (4, 4, 0, 0): 1,
        (4, 5, 0, 0): 2,
    })
    for key, count in key_counts.items():
        if key not in expected:
            raise SystemExit(f"instrumented ANV capture contains unexpected record key {key}")
        if count != expected[key]:
            raise SystemExit(
                f"instrumented ANV capture record {key} count={count}, expected {expected[key]}"
            )
    if any(int(record["kind"]) not in (1, 2, 3, 4, 5) for record in records):
        raise SystemExit("instrumented ANV capture contains an unknown record kind")
    missing = expected - key_counts
    if missing:
        raise SystemExit(f"instrumented ANV capture is missing required records: {dict(missing)}")

    command_records = [record for record in records if int(record["kind"]) == 5]
    if len(command_records) != 1:
        raise SystemExit(
            f"instrumented ANV capture needs exactly one completed command record, got {len(command_records)}"
        )
    command = command_records[0]
    # MESA_SHADER_NONE is intentionally not interpreted as a shader stage;
    # reject a command record that was mislabeled as vertex/fragment/compute.
    if int(command["stage_or_type"]) in (0, 4, 5) or command["binding"] != 0 or command["element"] != 0:
        raise SystemExit("instrumented ANV command record has an invalid non-shader stage/binding header")


def bake_helioc(args: argparse.Namespace) -> None:
    """Capture genuine native stages, then fail closed on unavailable ABI data."""
    compute, graphics = helioc_authored_sources()
    output = (args.out or Path("helioc-native.adl-s.gfx120.helio")).resolve()
    work = (args.work_dir or output.with_suffix(output.suffix + ".work")).resolve()
    work.mkdir(parents=True, exist_ok=True)
    compute_spv = work / "helioc-volume-update.main.spv"
    vertex_spv = work / "helioc-volume-raymarch.vs.spv"
    fragment_spv = work / "helioc-volume-raymarch.fs.spv"
    naga_compile(HELIOC_SIMULATE_SOURCE, "main", compute_spv)
    naga_compile(HELIOC_RENDER_SOURCE, "vs_main", vertex_spv)
    naga_compile(HELIOC_RENDER_SOURCE, "fs_main", fragment_spv)
    for name, spv in (("compute", compute_spv), ("vertex", vertex_spv), ("fragment", fragment_spv)):
        if not spv.is_file() or not spv.read_bytes():
            raise SystemExit(f"pinned Naga emitted no {name} SPIR-V for HelioC")
    exec_dir = work / f"helioc-pipeline-exec-{os.getpid()}"
    native_dir = work / "helioc-native"
    if exec_dir.exists() or native_dir.exists():
        raise SystemExit(
            f"HelioC capture session directories already exist: {exec_dir} or {native_dir}"
        )
    exec_dir.mkdir()
    native_dir.mkdir()
    dumper_source = work / "helioc_pipeline_dump.c"
    dumper = work / "helioc_pipeline_dump"
    make_helioc_compile_only_dumper(dumper_source)
    run(["cc", str(dumper_source), "-o", str(dumper), *vulkan_compile_flags()])
    env = os.environ.copy()
    env["TRUEOS_EXECUTABLE_DUMP_DIR"] = str(exec_dir)
    # Raw state records and executable representations must share one exact
    # capture directory so the parser cannot accidentally validate only half
    # of an instrumented run.
    env["TRUEOS_HELIOC_ANV_DUMP_DIR"] = str(exec_dir)
    # ANV's batch trace is diagnostic evidence only; native bytes still come
    # from the cache/assembly cross-check above. Preserve any caller flags.
    debug = env.get("INTEL_DEBUG", "")
    if "bat" not in {flag.strip() for flag in debug.split(",") if flag.strip()}:
        env["INTEL_DEBUG"] = f"{debug},bat".strip(",")
    if args.device_id:
        env["TRUEOS_VK_DEVICE_ID"] = args.device_id
    compile_log = work / "helioc-compile.log"
    run([str(dumper), str(compute_spv), str(vertex_spv), str(fragment_spv)], env=env, log=compile_log)
    log_text = compile_log.read_text()
    resource_marker = (
        "helioc_pipeline_dump: resources volumes=2 format=R16G16B16A16_SFLOAT "
        "extent=96x48x96 sampled=1 storage=1 "
        "sampler=repeat/clamp-to-edge/repeat linear/linear/nearest normalized=1"
    )
    descriptor_marker = (
        "helioc_pipeline_dump: descriptor_sets compute_ping_pong=2 graphics=2 "
        "bindings=compute[0:uniform,1:sampled3d,2:sampler,3:storage3d] "
        "graphics[0:uniform,1:sampled3d,2:sampler]"
    )
    if (
        resource_marker not in log_text
        or descriptor_marker not in log_text
        or "helioc_pipeline_dump: command_catalog=6 recorded_only=1" not in log_text
    ):
        raise SystemExit("HelioC Vulkan capture did not create the required 3D resource/pipeline state")
    volume_requirements = helioc_linear_volume_requirements(log_text)
    linear_probe = helioc_linear_probe(log_text)
    device, executables = parse_compile_log(log_text)
    compute_executables = [
        item for item in executables.values() if item["stage"] == "compute"
    ]
    if len(compute_executables) != 1:
        raise SystemExit(
            f"Mesa exposed {len(compute_executables)} compute executables; expected one"
        )
    compute_simd = int(compute_executables[0]["simd_width"])
    if compute_simd not in (16, 32):
        raise SystemExit(f"HelioC requires compiler-selected compute SIMD16/32, got SIMD{compute_simd}")
    cs, vs, fs = extract_helioc_native(exec_dir, native_dir)
    instrumented = collect_instrumented_anv_capture(exec_dir)
    if instrumented is not None:
        validate_instrumented_shader_serializations(exec_dir, cs, vs, fs)
    capture_metadata = {
        "schema": 1,
        "producer": "helio-intel-bake/helioc-capture",
        "api": "VK_KHR_pipeline_executable_properties",
        "compile_device": device,
        "resource_state": resource_marker.removeprefix("helioc_pipeline_dump: "),
        "bound_linear_volume_requirements": volume_requirements,
        "linear_volume_probe": linear_probe,
        "batch_capture": (
            "unavailable: ANV reported batch logging unsupported"
            if "Batch logging not supported" in log_text else "requested"
        ),
        "compute": {
            "entry": "main", "local_size": [4, 4, 4], "groups": [24, 12, 24],
            "simd_width": compute_simd, "hardware_threads": 64 // compute_simd,
            "isa_sha256": sha256(cs), "isa_bytes": len(cs),
        },
        "graphics": {
            "vertex_entry": "vs_main", "fragment_entry": "fs_main",
            "vertex_simd_width": 8, "fragment_simd_width": 16,
            "vertex_isa_sha256": sha256(vs), "fragment_isa_sha256": sha256(fs),
        },
        "executables": list(executables.values()),
        "public_api_boundary": "pipeline-cache bytes are opaque; executable APIs expose no ANV bind map or program data",
        "relocatable_state": {
            "section": HELIOC_RELOC_STATE_SECTION,
            "status": "missing",
            "reason": (
                "capture has no complete address-free HELIOCRS v2 object/typed-relocation map; "
                "raw process addresses are diagnostic only"
            ),
        },
        "source_level_capture_route": {
            "bind_map": (
                "instrument the matched ANV anv_shader_get_executable_internal_representations "
                "path to export anv_shader.bind_map and prog_data"
            ),
            "state": (
                "instrument matched genX_cmd_buffer emit_binding_table/emit_samplers and "
                "anv_image_fill_surface_state after relocations"
            ),
            "constraint": "rebuild and run that exact ANV source as the Vulkan ICD; do not parse opaque cache bytes",
        },
    }
    if instrumented is not None:
        identity = env.get("TRUEOS_HELIOC_CAPTURE_IDENTITY")
        if identity != "noop-drm-shim:8086:4680:r0c":
            raise SystemExit("instrumented ANV capture must identify the no-op shim; it is not physical-device proof")
        capture_metadata["instrumented_anv"] = instrumented
        capture_metadata["instrumented_identity"] = identity
        capture_metadata["public_api_boundary"] = (
            "resolved by exact source instrumentation for canonical shader serialization and "
            "diagnostic command/state records"
        )
        capture_metadata["remaining_capture_boundary"] = (
            "raw state still embeds capture-process addresses and lacks an address-free broker-owned "
            "HELIOCRS v2 object/typed-relocation map; shim execution is not physical target proof"
        )
        if instrumented.get("address_free_indirect_templates") is not None \
                and instrumented.get("address_free_surface_templates") is not None \
                and instrumented.get("address_free_buffer_descriptor_templates") is not None \
                and instrumented.get("address_free_binding_sampler_tables") is not None \
                and instrumented.get("address_free_descriptor_payloads") is not None:
            capture_metadata["relocatable_state"] = {
                "section": HELIOC_RELOC_STATE_SECTION,
                "status": "partial-v9",
                "proven_slices": [
                    "indirect-descriptor-v5", "image-surface-v6", "buffer-descriptor-surface-v7",
                    "binding-table-sampler-v8", "descriptor-payload-v9",
                ],
                "reason": (
                    "command and program objects still "
                    "lack a complete typed relocation map"
                ),
            }
            capture_metadata["remaining_capture_boundary"] = (
                "V5 indirect descriptors, V6 image surfaces, V7 buffer/descriptor-set surfaces, and V8 "
                "binding/sampler tables and descriptor payloads are address-free; command and program state remain incomplete, and "
                "shim execution is not physical target proof"
            )
    (work / "helioc-capture-metadata.json").write_bytes(
        (json.dumps(capture_metadata, indent=2, sort_keys=True) + "\n").encode()
    )
    print(f"HelioC captured genuine compute ISA: {len(cs)} bytes sha256={sha256(cs)}")
    print(f"HelioC captured genuine fullscreen VS: {len(vs)} bytes sha256={sha256(vs)}")
    print(f"HelioC captured genuine fullscreen FS SIMD16: {len(fs)} bytes sha256={sha256(fs)}")
    raise SystemExit(
        "HelioC preflight stopped; no HELIOA emitted: missing capture datum(s): "
        + "; ".join(helioc_public_api_boundary(
            device, volume_requirements, log_text, instrumented=instrumented,
        ))
    )


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


def make_helioc_compile_only_dumper(destination: Path) -> None:
    """Specialize the proven dumper for HelioC's real compute + 3D workload."""
    make_compile_only_dumper(destination)
    source = destination.read_text()
    source = replace_once(source, '''    const VkInstanceCreateInfo instance_info = {
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &app_info,
    };''', '''    const char *instance_extensions[] = { VK_EXT_DEBUG_UTILS_EXTENSION_NAME };
    const VkInstanceCreateInfo instance_info = {
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &app_info,
        .enabledExtensionCount = 1,
        .ppEnabledExtensionNames = instance_extensions,
    };''')
    source = replace_once(source, '''    VkDevice device;
    CHECK_VK(vkCreateDevice(physical_device, &device_info, NULL, &device));''', '''    VkDevice device;
    CHECK_VK(vkCreateDevice(physical_device, &device_info, NULL, &device));
    PFN_vkSetDebugUtilsObjectNameEXT helioc_set_debug_name =
        (PFN_vkSetDebugUtilsObjectNameEXT)vkGetDeviceProcAddr(
            device, "vkSetDebugUtilsObjectNameEXT"
        );
    if (helioc_set_debug_name == NULL) {
        fprintf(stderr, "helioc_pipeline_dump: VK_EXT_debug_utils naming unavailable\\n");
        return 1;
    }''')
    source = replace_once(source, '''    VkImage image;
    CHECK_VK(vkCreateImage(device, &image_info, NULL, &image));''', '''    VkImage image;
    CHECK_VK(vkCreateImage(device, &image_info, NULL, &image));
    const VkDebugUtilsObjectNameInfoEXT target_name = {
        .sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT,
        .objectType = VK_OBJECT_TYPE_IMAGE, .objectHandle = (uint64_t)image,
        .pObjectName = "trueos.helioc.output_target",
    };
    CHECK_VK(helioc_set_debug_name(device, &target_name));''')
    source = replace_once(source, '''    if (argc != 3) {
        fprintf(stderr, "usage: %s simple_triangle.vert.spv simple_triangle.frag.spv\\n", argv[0]);
        return 1;
    }''', '''    if (argc != 4) {
        fprintf(stderr, "usage: %s simulate.comp.spv render.vert.spv render.frag.spv\\n", argv[0]);
        return 1;
    }''')
    source = replace_once(source, '''        case VK_SHADER_STAGE_FRAGMENT_BIT:
            return "fragment";
        default:''', '''        case VK_SHADER_STAGE_FRAGMENT_BIT:
            return "fragment";
        case VK_SHADER_STAGE_COMPUTE_BIT:
            return "compute";
        default:''')
    source = replace_once(source, '''            if (queues[q].queueFlags & VK_QUEUE_GRAPHICS_BIT) {''', '''            if ((queues[q].queueFlags & (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT))
                == (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) {''')
    source = replace_once(source, '''    FileData vs_spirv = read_spirv(argv[1]);
    FileData fs_spirv = read_spirv(argv[2]);
    const VkShaderModuleCreateInfo vs_info = {''', '''    FileData cs_spirv = read_spirv(argv[1]);
    FileData vs_spirv = read_spirv(argv[2]);
    FileData fs_spirv = read_spirv(argv[3]);
    const VkShaderModuleCreateInfo cs_info = {
        .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = cs_spirv.word_count * sizeof(uint32_t),
        .pCode = cs_spirv.words,
    };
    const VkShaderModuleCreateInfo vs_info = {''')
    source = replace_once(source, '''    VkShaderModule vs_module;
    VkShaderModule fs_module;
    CHECK_VK(vkCreateShaderModule(device, &vs_info, NULL, &vs_module));''', '''    VkShaderModule cs_module;
    VkShaderModule vs_module;
    VkShaderModule fs_module;
    CHECK_VK(vkCreateShaderModule(device, &cs_info, NULL, &cs_module));
    CHECK_VK(vkCreateShaderModule(device, &vs_info, NULL, &vs_module));''')
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
    };''', '''    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
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
    };
    VkDescriptorSetLayout set_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &set_layout_info, NULL, &set_layout));
    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 1,
        .pSetLayouts = &set_layout,
        .pushConstantRangeCount = 0,
        .pPushConstantRanges = NULL,
    };''', '''    const VkDescriptorSetLayoutBinding compute_bindings[4] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT },
        { .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT },
        { .binding = 3, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT },
    };
    const VkDescriptorSetLayoutCreateInfo compute_set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 4, .pBindings = compute_bindings,
    };
    VkDescriptorSetLayout compute_set_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &compute_set_layout_info, NULL, &compute_set_layout));
    const VkDescriptorSetLayoutBinding graphics_bindings[3] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
    };
    const VkDescriptorSetLayoutCreateInfo graphics_set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 3, .pBindings = graphics_bindings,
    };
    VkDescriptorSetLayout graphics_set_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &graphics_set_layout_info, NULL, &graphics_set_layout));
    const VkPipelineLayoutCreateInfo compute_pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 1, .pSetLayouts = &compute_set_layout,
    };
    VkPipelineLayout compute_pipeline_layout;
    CHECK_VK(vkCreatePipelineLayout(device, &compute_pipeline_layout_info, NULL, &compute_pipeline_layout));
    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 1, .pSetLayouts = &graphics_set_layout,
    };

    const VkImageCreateInfo volume_info = {
        .sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
        .imageType = VK_IMAGE_TYPE_3D,
        .format = VK_FORMAT_R16G16B16A16_SFLOAT,
        .extent = { 96, 48, 96 }, .mipLevels = 1, .arrayLayers = 1,
        .samples = VK_SAMPLE_COUNT_1_BIT, .tiling = VK_IMAGE_TILING_LINEAR,
        .usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_STORAGE_BIT,
        .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
        .initialLayout = VK_IMAGE_LAYOUT_UNDEFINED,
    };
    VkFormatProperties linear_volume_format_properties;
    vkGetPhysicalDeviceFormatProperties(
        physical_device, VK_FORMAT_R16G16B16A16_SFLOAT, &linear_volume_format_properties
    );
    const VkFormatFeatureFlags required_linear_volume_features =
        VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT | VK_FORMAT_FEATURE_STORAGE_IMAGE_BIT;
    printf("helioc_pipeline_dump: bound_linear features=0x%08X required=0x%08X\\n",
           linear_volume_format_properties.linearTilingFeatures, required_linear_volume_features);
    if ((linear_volume_format_properties.linearTilingFeatures & required_linear_volume_features)
        != required_linear_volume_features) {
        fprintf(stderr, "helioc_pipeline_dump: bound_linear unsupported=sampled_storage_3d\\n");
        return 1;
    }
    VkImage volumes[2];
    VkDeviceMemory volume_memories[2];
    VkImageView volume_views[2];
    for (uint32_t i = 0; i < 2; ++i) {
        CHECK_VK(vkCreateImage(device, &volume_info, NULL, &volumes[i]));
        VkMemoryRequirements volume_reqs;
        vkGetImageMemoryRequirements(device, volumes[i], &volume_reqs);
        const VkMemoryAllocateInfo volume_alloc = {
            .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            .allocationSize = volume_reqs.size,
            .memoryTypeIndex = find_memory_type(physical_device, volume_reqs.memoryTypeBits,
                                                VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT),
        };
        CHECK_VK(vkAllocateMemory(device, &volume_alloc, NULL, &volume_memories[i]));
        CHECK_VK(vkBindImageMemory(device, volumes[i], volume_memories[i], 0));
        const VkImageSubresource volume_subresource = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT, .mipLevel = 0, .arrayLayer = 0,
        };
        VkSubresourceLayout volume_layout;
        vkGetImageSubresourceLayout(device, volumes[i], &volume_subresource, &volume_layout);
        printf("helioc_pipeline_dump: volume[%u] linear_memory size=%llu alignment=%llu type_bits=0x%08X layout offset=%llu size=%llu row_pitch=%llu array_pitch=%llu depth_pitch=%llu\\n",
               i, (unsigned long long)volume_reqs.size, (unsigned long long)volume_reqs.alignment,
               volume_reqs.memoryTypeBits, (unsigned long long)volume_layout.offset,
               (unsigned long long)volume_layout.size, (unsigned long long)volume_layout.rowPitch,
               (unsigned long long)volume_layout.arrayPitch, (unsigned long long)volume_layout.depthPitch);
        const VkImageViewCreateInfo volume_view_info = {
            .sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO, .image = volumes[i],
            .viewType = VK_IMAGE_VIEW_TYPE_3D, .format = VK_FORMAT_R16G16B16A16_SFLOAT,
            .subresourceRange = { .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
                .baseMipLevel = 0, .levelCount = 1, .baseArrayLayer = 0, .layerCount = 1 },
        };
        CHECK_VK(vkCreateImageView(device, &volume_view_info, NULL, &volume_views[i]));
    }
    VkFormatProperties volume_format_properties;
    vkGetPhysicalDeviceFormatProperties(
        physical_device, VK_FORMAT_R16G16B16A16_SFLOAT, &volume_format_properties
    );
    const VkFormatFeatureFlags linear_required_features =
        VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT | VK_FORMAT_FEATURE_STORAGE_IMAGE_BIT;
    printf("helioc_pipeline_dump: linear_probe features=0x%08X required=0x%08X\\n",
           volume_format_properties.linearTilingFeatures, linear_required_features);
    if ((volume_format_properties.linearTilingFeatures & linear_required_features)
        == linear_required_features) {
        VkImageCreateInfo linear_probe_info = volume_info;
        linear_probe_info.tiling = VK_IMAGE_TILING_LINEAR;
        VkImage linear_probe;
        const VkResult linear_probe_result = vkCreateImage(device, &linear_probe_info, NULL, &linear_probe);
        printf("helioc_pipeline_dump: linear_probe create_result=%d\\n", linear_probe_result);
        if (linear_probe_result == VK_SUCCESS) {
            VkMemoryRequirements linear_reqs;
            vkGetImageMemoryRequirements(device, linear_probe, &linear_reqs);
            const VkMemoryAllocateInfo linear_alloc = {
                .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, .allocationSize = linear_reqs.size,
                .memoryTypeIndex = find_memory_type(physical_device, linear_reqs.memoryTypeBits,
                                                    VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT),
            };
            VkDeviceMemory linear_memory;
            CHECK_VK(vkAllocateMemory(device, &linear_alloc, NULL, &linear_memory));
            CHECK_VK(vkBindImageMemory(device, linear_probe, linear_memory, 0));
            const VkImageSubresource linear_subresource = {
                .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT, .mipLevel = 0, .arrayLayer = 0,
            };
            VkSubresourceLayout linear_layout;
            vkGetImageSubresourceLayout(device, linear_probe, &linear_subresource, &linear_layout);
            printf("helioc_pipeline_dump: linear_probe memory size=%llu alignment=%llu type_bits=0x%08X layout offset=%llu size=%llu row_pitch=%llu array_pitch=%llu depth_pitch=%llu\\n",
                   (unsigned long long)linear_reqs.size, (unsigned long long)linear_reqs.alignment,
                   linear_reqs.memoryTypeBits, (unsigned long long)linear_layout.offset,
                   (unsigned long long)linear_layout.size, (unsigned long long)linear_layout.rowPitch,
                   (unsigned long long)linear_layout.arrayPitch, (unsigned long long)linear_layout.depthPitch);
            vkFreeMemory(device, linear_memory, NULL);
            vkDestroyImage(device, linear_probe, NULL);
        }
    } else {
        printf("helioc_pipeline_dump: linear_probe unsupported=sampled_storage_3d\\n");
    }
    const VkSamplerCreateInfo volume_sampler_info = {
        .sType = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO,
        .magFilter = VK_FILTER_LINEAR, .minFilter = VK_FILTER_LINEAR,
        .mipmapMode = VK_SAMPLER_MIPMAP_MODE_NEAREST,
        .addressModeU = VK_SAMPLER_ADDRESS_MODE_REPEAT,
        .addressModeV = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
        .addressModeW = VK_SAMPLER_ADDRESS_MODE_REPEAT,
        .maxAnisotropy = 1.0f, .unnormalizedCoordinates = VK_FALSE,
    };
    VkSampler volume_sampler;
    CHECK_VK(vkCreateSampler(device, &volume_sampler_info, NULL, &volume_sampler));
    const VkBufferCreateInfo uniform_buffer_info = {
        .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO, .size = 512,
        .usage = VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT, .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
    };
    VkBuffer uniform_buffer;
    CHECK_VK(vkCreateBuffer(device, &uniform_buffer_info, NULL, &uniform_buffer));
    VkMemoryRequirements uniform_reqs;
    vkGetBufferMemoryRequirements(device, uniform_buffer, &uniform_reqs);
    const VkMemoryAllocateInfo uniform_alloc = {
        .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, .allocationSize = uniform_reqs.size,
        .memoryTypeIndex = find_memory_type(physical_device, uniform_reqs.memoryTypeBits,
                                            VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT |
                                            VK_MEMORY_PROPERTY_HOST_COHERENT_BIT),
    };
    VkDeviceMemory uniform_memory;
    CHECK_VK(vkAllocateMemory(device, &uniform_alloc, NULL, &uniform_memory));
    CHECK_VK(vkBindBufferMemory(device, uniform_buffer, uniform_memory, 0));
    void *uniform_map = NULL;
    CHECK_VK(vkMapMemory(device, uniform_memory, 0, 512, 0, &uniform_map));
    memset(uniform_map, 0, 512);
    vkUnmapMemory(device, uniform_memory);
    const VkDescriptorPoolSize pool_sizes[4] = {
        { VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, 4 }, { VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE, 4 },
        { VK_DESCRIPTOR_TYPE_SAMPLER, 4 }, { VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, 2 },
    };
    const VkDescriptorPoolCreateInfo descriptor_pool_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO, .maxSets = 4,
        .poolSizeCount = 4, .pPoolSizes = pool_sizes,
    };
    VkDescriptorPool descriptor_pool;
    CHECK_VK(vkCreateDescriptorPool(device, &descriptor_pool_info, NULL, &descriptor_pool));
    const VkDescriptorSetLayout descriptor_layouts[4] = {
        compute_set_layout, compute_set_layout, graphics_set_layout, graphics_set_layout,
    };
    const VkDescriptorSetAllocateInfo descriptor_allocate_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO, .descriptorPool = descriptor_pool,
        .descriptorSetCount = 4, .pSetLayouts = descriptor_layouts,
    };
    VkDescriptorSet descriptor_sets[4];
    CHECK_VK(vkAllocateDescriptorSets(device, &descriptor_allocate_info, descriptor_sets));
    const VkDescriptorBufferInfo compute_buffers[2] = {
        { .buffer = uniform_buffer, .offset = 0, .range = 112 },
        { .buffer = uniform_buffer, .offset = 0, .range = 112 },
    };
    const VkDescriptorBufferInfo graphics_buffer = {
        .buffer = uniform_buffer, .offset = 128, .range = 272,
    };
    VkDescriptorImageInfo sampled_volumes[4] = {
        { .imageView = volume_views[0], .imageLayout = VK_IMAGE_LAYOUT_GENERAL },
        { .imageView = volume_views[1], .imageLayout = VK_IMAGE_LAYOUT_GENERAL },
        { .imageView = volume_views[0], .imageLayout = VK_IMAGE_LAYOUT_GENERAL },
        { .imageView = volume_views[1], .imageLayout = VK_IMAGE_LAYOUT_GENERAL },
    };
    VkDescriptorImageInfo storage_volumes[2] = {
        { .imageView = volume_views[1], .imageLayout = VK_IMAGE_LAYOUT_GENERAL },
        { .imageView = volume_views[0], .imageLayout = VK_IMAGE_LAYOUT_GENERAL },
    };
    const VkDescriptorImageInfo sampler_infos[4] = {
        { .sampler = volume_sampler }, { .sampler = volume_sampler }, { .sampler = volume_sampler },
        { .sampler = volume_sampler },
    };
    VkWriteDescriptorSet writes[14];
    uint32_t write_count = 0;
    for (uint32_t i = 0; i < 2; ++i) {
        writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
            .dstSet = descriptor_sets[i], .dstBinding = 0, .descriptorCount = 1,
            .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, .pBufferInfo = &compute_buffers[i] };
        writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
            .dstSet = descriptor_sets[i], .dstBinding = 1, .descriptorCount = 1,
            .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE, .pImageInfo = &sampled_volumes[i] };
        writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
            .dstSet = descriptor_sets[i], .dstBinding = 2, .descriptorCount = 1,
            .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER, .pImageInfo = &sampler_infos[i] };
        writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
            .dstSet = descriptor_sets[i], .dstBinding = 3, .descriptorCount = 1,
            .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, .pImageInfo = &storage_volumes[i] };
    }
    writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        .dstSet = descriptor_sets[2], .dstBinding = 0, .descriptorCount = 1,
        .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, .pBufferInfo = &graphics_buffer };
    writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        .dstSet = descriptor_sets[2], .dstBinding = 1, .descriptorCount = 1,
        .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE, .pImageInfo = &sampled_volumes[2] };
    writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        .dstSet = descriptor_sets[2], .dstBinding = 2, .descriptorCount = 1,
        .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER, .pImageInfo = &sampler_infos[2] };
    writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        .dstSet = descriptor_sets[3], .dstBinding = 0, .descriptorCount = 1,
        .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, .pBufferInfo = &graphics_buffer };
    writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        .dstSet = descriptor_sets[3], .dstBinding = 1, .descriptorCount = 1,
        .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE, .pImageInfo = &sampled_volumes[3] };
    writes[write_count++] = (VkWriteDescriptorSet) { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        .dstSet = descriptor_sets[3], .dstBinding = 2, .descriptorCount = 1,
        .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER, .pImageInfo = &sampler_infos[3] };
    vkUpdateDescriptorSets(device, write_count, writes, 0, NULL);
    printf("helioc_pipeline_dump: descriptor_sets compute_ping_pong=2 graphics=2 bindings=compute[0:uniform,1:sampled3d,2:sampler,3:storage3d] graphics[0:uniform,1:sampled3d,2:sampler]\\n");
    printf("helioc_pipeline_dump: resources volumes=2 format=R16G16B16A16_SFLOAT extent=96x48x96 sampled=1 storage=1 sampler=repeat/clamp-to-edge/repeat linear/linear/nearest normalized=1\\n");''')
    source = replace_once(source, '''    for (uint32_t i = 0; i < 2; ++i) {
        CHECK_VK(vkCreateImage(device, &volume_info, NULL, &volumes[i]));''', '''    for (uint32_t i = 0; i < 2; ++i) {
        CHECK_VK(vkCreateImage(device, &volume_info, NULL, &volumes[i]));
        const VkDebugUtilsObjectNameInfoEXT volume_name = {
            .sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT,
            .objectType = VK_OBJECT_TYPE_IMAGE, .objectHandle = (uint64_t)volumes[i],
            .pObjectName = i == 0 ? "trueos.helioc.volume_a" : "trueos.helioc.volume_b",
        };
        CHECK_VK(helioc_set_debug_name(device, &volume_name));''')
    source = replace_once(source, '''    VkBuffer uniform_buffer;
    CHECK_VK(vkCreateBuffer(device, &uniform_buffer_info, NULL, &uniform_buffer));''', '''    VkBuffer uniform_buffer;
    CHECK_VK(vkCreateBuffer(device, &uniform_buffer_info, NULL, &uniform_buffer));
    const VkDebugUtilsObjectNameInfoEXT params_name = {
        .sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT,
        .objectType = VK_OBJECT_TYPE_BUFFER, .objectHandle = (uint64_t)uniform_buffer,
        .pObjectName = "trueos.helioc.params_arena",
    };
    CHECK_VK(helioc_set_debug_name(device, &params_name));''')
    source = replace_once(source, '''    VkDescriptorSet descriptor_sets[4];
    CHECK_VK(vkAllocateDescriptorSets(device, &descriptor_allocate_info, descriptor_sets));''', '''    VkDescriptorSet descriptor_sets[4];
    CHECK_VK(vkAllocateDescriptorSets(device, &descriptor_allocate_info, descriptor_sets));
    const char *descriptor_set_names[4] = {
        "trueos.helioc.compute_ping_a", "trueos.helioc.compute_ping_b",
        "trueos.helioc.graphics_a", "trueos.helioc.graphics_b",
    };
    for (uint32_t i = 0; i < 4; ++i) {
        const VkDebugUtilsObjectNameInfoEXT descriptor_set_name = {
            .sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT,
            .objectType = VK_OBJECT_TYPE_DESCRIPTOR_SET, .objectHandle = (uint64_t)descriptor_sets[i],
            .pObjectName = descriptor_set_names[i],
        };
        CHECK_VK(helioc_set_debug_name(device, &descriptor_set_name));
    }''')
    source = replace_once(source, '''    const VkCommandBufferAllocateInfo command_alloc = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = command_pool,
        .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        .commandBufferCount = 1,
    };
    VkCommandBuffer command_buffer;
    CHECK_VK(vkAllocateCommandBuffers(device, &command_alloc, &command_buffer));''', '''    const VkCommandBufferAllocateInfo command_alloc = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = command_pool,
        .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        .commandBufferCount = 1,
    };
    VkCommandBuffer command_buffer;
    CHECK_VK(vkAllocateCommandBuffers(device, &command_alloc, &command_buffer));
    VkCommandBufferAllocateInfo command_catalog_alloc = command_alloc;
    command_catalog_alloc.commandBufferCount = 6;
    VkCommandBuffer command_buffers[6];
    CHECK_VK(vkAllocateCommandBuffers(device, &command_catalog_alloc, command_buffers));
    const char *command_names[6] = {
        "trueos.helioc.command.current_a.step0.final_a.dispatch_none",
        "trueos.helioc.command.current_a.step1.final_b.dispatch_a_to_b",
        "trueos.helioc.command.current_a.step2.final_a.dispatch_a_to_b_b_to_a",
        "trueos.helioc.command.current_b.step0.final_b.dispatch_none",
        "trueos.helioc.command.current_b.step1.final_a.dispatch_b_to_a",
        "trueos.helioc.command.current_b.step2.final_b.dispatch_b_to_a_a_to_b",
    };
    for (uint32_t i = 0; i < 6; ++i) {
        const VkDebugUtilsObjectNameInfoEXT command_name = {
            .sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT,
            .objectType = VK_OBJECT_TYPE_COMMAND_BUFFER, .objectHandle = (uint64_t)command_buffers[i],
            .pObjectName = command_names[i],
        };
        CHECK_VK(helioc_set_debug_name(device, &command_name));
    }''')
    source = replace_once(source, '''    VkPipelineCache pipeline_cache;
    CHECK_VK(vkCreatePipelineCache(device, &pipeline_cache_info, NULL, &pipeline_cache));

    const VkGraphicsPipelineCreateInfo pipeline_info = {''', '''    VkPipelineCache pipeline_cache;
    CHECK_VK(vkCreatePipelineCache(device, &pipeline_cache_info, NULL, &pipeline_cache));

    const VkComputePipelineCreateInfo compute_pipeline_info = {
        .sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
        .flags = VK_PIPELINE_CREATE_CAPTURE_STATISTICS_BIT_KHR |
                 VK_PIPELINE_CREATE_CAPTURE_INTERNAL_REPRESENTATIONS_BIT_KHR,
        .stage = { .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            .stage = VK_SHADER_STAGE_COMPUTE_BIT, .module = cs_module, .pName = "main" },
        .layout = compute_pipeline_layout,
    };
    VkPipeline compute_pipeline;
    CHECK_VK(vkCreateComputePipelines(device, pipeline_cache, 1, &compute_pipeline_info, NULL, &compute_pipeline));
    dump_pipeline_executables(device, compute_pipeline);

    const VkGraphicsPipelineCreateInfo pipeline_info = {''')
    source = replace_once(source, '''    dump_pipeline_cache_blob(device, pipeline_cache);
    dump_pipeline_executables(device, pipeline);
    printf("helio_pipeline_dump: compiled_only=1\\n");''', '''    printf("helioc_pipeline_dump: recording command_catalog=6 dispatch=24x12x24 fullscreen_draw=3\\n");
    const VkCommandBufferBeginInfo helioc_begin = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        /* Disable ANV's submit-time command-buffer chaining optimization so
         * this capture has one immutable first-level batch ending in BBE. */
        .flags = VK_COMMAND_BUFFER_USAGE_SIMULTANEOUS_USE_BIT,
    };
    VkImageMemoryBarrier volume_barriers[2] = { 0 };
    for (uint32_t i = 0; i < 2; ++i) {
        volume_barriers[i].sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
        volume_barriers[i].srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT;
        volume_barriers[i].dstAccessMask = VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT;
        /* The catalog models persistent ping-pong storage: it never
         * discards either volume with an UNDEFINED-to-GENERAL transition. */
        volume_barriers[i].oldLayout = VK_IMAGE_LAYOUT_GENERAL;
        volume_barriers[i].newLayout = VK_IMAGE_LAYOUT_GENERAL;
        volume_barriers[i].srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        volume_barriers[i].dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        volume_barriers[i].image = volumes[i];
        volume_barriers[i].subresourceRange = (VkImageSubresourceRange) {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT, .baseMipLevel = 0, .levelCount = 1,
            .baseArrayLayer = 0, .layerCount = 1,
        };
    }
    const VkImageMemoryBarrier target_barrier = {
        .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        .dstAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
        .oldLayout = VK_IMAGE_LAYOUT_UNDEFINED,
        .newLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .image = image,
        .subresourceRange = { .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .baseMipLevel = 0, .levelCount = 1, .baseArrayLayer = 0, .layerCount = 1 },
    };
    const VkClearValue helioc_clear = { .color = { .float32 = { 0.f, 0.f, 0.f, 1.f } } };
    const VkRenderPassBeginInfo helioc_render_begin = {
        .sType = VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO, .renderPass = render_pass,
        .framebuffer = framebuffer, .renderArea = { .offset = { 0, 0 }, .extent = { 64, 64 } },
        .clearValueCount = 1, .pClearValues = &helioc_clear,
    };
    for (uint32_t current = 0; current < 2; ++current) {
        for (uint32_t step = 0; step < 3; ++step) {
            const uint32_t command_index = current * 3 + step;
            VkCommandBuffer command_buffer = command_buffers[command_index];
            CHECK_VK(vkBeginCommandBuffer(command_buffer, &helioc_begin));
            vkCmdPipelineBarrier(command_buffer, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT |
                                 VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
                                 VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
                                 0, 0, NULL, 0, NULL, 2, volume_barriers);
            vkCmdPipelineBarrier(command_buffer, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                                 VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                                 0, 0, NULL, 0, NULL, 1, &target_barrier);
            if (step != 0) {
                vkCmdBindPipeline(command_buffer, VK_PIPELINE_BIND_POINT_COMPUTE, compute_pipeline);
                for (uint32_t dispatch = 0; dispatch < step; ++dispatch) {
                    const uint32_t compute_set = (current + dispatch) & 1;
                    vkCmdBindDescriptorSets(command_buffer, VK_PIPELINE_BIND_POINT_COMPUTE,
                                            compute_pipeline_layout, 0, 1,
                                            &descriptor_sets[compute_set], 0, NULL);
                    vkCmdDispatch(command_buffer, 24, 12, 24);
                    if (dispatch + 1 < step) {
                        const uint32_t written_volume = (compute_set + 1) & 1;
                        VkImageMemoryBarrier ping_pong_barrier = volume_barriers[written_volume];
                        ping_pong_barrier.srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT;
                        ping_pong_barrier.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
                        vkCmdPipelineBarrier(command_buffer, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
                                             VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
                                             0, 0, NULL, 0, NULL, 1, &ping_pong_barrier);
                    }
                }
            }
            const uint32_t final_volume = (current + step) & 1;
            VkImageMemoryBarrier render_volume_barrier = volume_barriers[final_volume];
            render_volume_barrier.srcAccessMask = step ? VK_ACCESS_SHADER_WRITE_BIT : 0;
            render_volume_barrier.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
            render_volume_barrier.oldLayout = VK_IMAGE_LAYOUT_GENERAL;
            render_volume_barrier.newLayout = VK_IMAGE_LAYOUT_GENERAL;
            vkCmdPipelineBarrier(command_buffer,
                                 step ? VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT : VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                                 VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT, 0, 0, NULL, 0, NULL,
                                 1, &render_volume_barrier);
            vkCmdBeginRenderPass(command_buffer, &helioc_render_begin, VK_SUBPASS_CONTENTS_INLINE);
            vkCmdBindPipeline(command_buffer, VK_PIPELINE_BIND_POINT_GRAPHICS, pipeline);
            vkCmdBindDescriptorSets(command_buffer, VK_PIPELINE_BIND_POINT_GRAPHICS,
                                    pipeline_layout, 0, 1, &descriptor_sets[2 + final_volume], 0, NULL);
            vkCmdDraw(command_buffer, 3, 1, 0, 0);
            vkCmdEndRenderPass(command_buffer);
            CHECK_VK(vkEndCommandBuffer(command_buffer));
        }
    }
    dump_pipeline_cache_blob(device, pipeline_cache);
    dump_pipeline_executables(device, pipeline);
    printf("helioc_pipeline_dump: public_api=VK_KHR_pipeline_executable_properties pipeline_cache=opaque\\n");
    printf("helioc_pipeline_dump: command_catalog=6 recorded_only=1\\n");''')
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
    executable_keys: dict[int, int] = {}
    pipeline_serial = -1
    for line in log.splitlines():
        executable_match = re.search(
            r'executable\[(\d+)\] stage=(\w+) name="([^"]+)" desc="([^"]+)" subgroup=(\d+)', line
        )
        if executable_match:
            index = int(executable_match.group(1))
            # Each call to vkGetPipelineExecutablePropertiesKHR numbers from
            # zero. HelioC deliberately captures a compute and a graphics
            # pipeline in one log, so preserve both rather than overwriting
            # compute executable[0] with graphics executable[0].
            if index == 0:
                pipeline_serial += 1
                executable_keys = {}
            key = pipeline_serial * 1000 + index
            executable_keys[index] = key
            executables[key] = {
                "stage": executable_match.group(2), "name": executable_match.group(3),
                "description": executable_match.group(4),
                "simd_width": int(executable_match.group(5)), "statistics": {},
            }
            continue
        statistic_match = re.search(
            r'stat\[(\d+)\]\[\d+\] name="([^"]+)" value=([^\n]+)', line
        )
        if statistic_match:
            key = executable_keys.get(int(statistic_match.group(1)))
            if key is not None:
                value = statistic_match.group(3).strip()
                executables[key]["statistics"][statistic_match.group(2)] = (
                    int(value) if value.isdigit() else value
                )
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


def assembly_instruction_count(path: Path) -> int:
    """Count the executable instruction rows in Mesa's offset-free assembly."""
    count = 0
    for line in path.read_text(errors="replace").splitlines():
        if not line or line[0].isspace() or line == "\0" or line.startswith("LABEL"):
            continue
        count += 1
    if count == 0:
        raise SystemExit(f"no Intel instructions found in {path}")
    return count


def find_cache_stage(
    cache: bytes,
    stage: int,
    size: int,
    instruction_count: int | None = None,
) -> bytes:
    candidates: list[tuple[int, int, bytes]] = []
    for offset in range(0, len(cache) - 8):
        found_stage, found_size = struct.unpack_from("<II", cache, offset)
        if found_stage != stage or found_size == 0 or offset + 8 + found_size > len(cache):
            continue
        data = cache[offset + 8:offset + 8 + found_size]
        if sum(byte != 0 for byte in data) > found_size // 4:
            candidates.append((offset, found_size, data))
    exact = [data for _, found_size, data in candidates if found_size == size]
    if exact:
        # This is the same first-dense-candidate rule as TRUEOS's existing
        # extract_from_pipeline_cache.py. Require determinism if cache duplicates
        # the actual machine code.
        return exact[0]

    # Mesa 26's textual assembly no longer carries byte offsets, and SEND/sync
    # rows do not have a one-to-one 8/16-byte relationship in every large
    # graphics shader. The compacted-row estimate can therefore miss the real
    # cache record by a small amount. A real Xe instruction occupies 8..16
    # bytes, so the first dense stage record inside that strict envelope is the
    # native program. The much larger outer ANV cache-object record is excluded.
    if instruction_count is not None:
        minimum = instruction_count * 8
        maximum = instruction_count * 16
        bounded = [
            data
            for _, found_size, data in candidates
            if minimum <= found_size <= maximum
        ]
        if bounded:
            return bounded[0]
    raise SystemExit(f"native stage absent from ANV cache: stage={stage} size={size}")


def extract_native(exec_dir: Path, native_dir: Path) -> tuple[bytes, bytes, bytes | None]:
    assemblies = sorted(exec_dir.glob("*_GEN_Assembly.txt"))
    vertex = [path for path in assemblies if "_vertex_" in path.name]
    fragments = [path for path in assemblies if "_fragment_" in path.name]
    if len(vertex) != 1 or not fragments:
        raise SystemExit(
            f"expected one VS and at least one PS assembly, got {len(vertex)} and {len(fragments)}"
        )
    vs_size = assembly_code_size(vertex[0])
    vs_instruction_count = assembly_instruction_count(vertex[0])
    fragment_sizes = [assembly_code_size(path) for path in fragments]
    fragment_instruction_count = sum(assembly_instruction_count(path) for path in fragments)
    fragment_offsets: list[int] = []
    cursor = 0
    for size in fragment_sizes:
        cursor = (cursor + 63) & ~63
        fragment_offsets.append(cursor)
        cursor += size
    fragment_span = cursor
    cache = (exec_dir / "pipeline_cache.bin").read_bytes()
    vs = find_cache_stage(cache, 0, vs_size, vs_instruction_count)
    combined = find_cache_stage(cache, 4, fragment_span, fragment_instruction_count)
    if len(fragments) == 1 and len(combined) != fragment_sizes[0]:
        # Preserve the complete cache record when the offset-free assembly
        # estimate exercised the bounded fallback above. Trimming that record
        # to the textual estimate would silently discard valid tail bytes.
        slices = [combined]
    else:
        slices = [
            combined[offset:offset + size]
            for offset, size in zip(fragment_offsets, fragment_sizes)
        ]
    native_dir.mkdir(exist_ok=True)
    (native_dir / "simple_cube_vs.bin").write_bytes(vs)
    (native_dir / "simple_cube_ps_simd8.bin").write_bytes(slices[0])
    ps16 = slices[1] if len(slices) > 1 else None
    if ps16 is not None:
        (native_dir / "simple_cube_ps_simd16.bin").write_bytes(ps16)
    return vs, slices[0], ps16


def extract_helioc_native(exec_dir: Path, native_dir: Path) -> tuple[bytes, bytes, bytes]:
    """Recover genuine compute, fullscreen VS, and SIMD16 FS EU code.

    ANV's cache is opaque except for the stage/assembly-size cross-check used
    by the established graphics path. This function applies that same bounded
    recovery rule and rejects missing native records instead of inventing one.
    """
    assemblies = sorted(exec_dir.glob("*_GEN_Assembly.txt"))
    compute = [path for path in assemblies if "_compute_" in path.name]
    if len(compute) != 1:
        raise SystemExit(f"expected exactly one compute assembly, got {len(compute)}")
    compute_size = assembly_code_size(compute[0])
    compute_instructions = assembly_instruction_count(compute[0])
    cache = (exec_dir / "pipeline_cache.bin").read_bytes()
    cs = find_cache_stage(cache, 5, compute_size, compute_instructions)
    vs, _, ps16 = extract_native(exec_dir, native_dir)
    if ps16 is None:
        raise SystemExit("Mesa exposed no SIMD16 fullscreen fragment executable")
    native_dir.mkdir(exist_ok=True)
    (native_dir / "helioc_compute.bin").write_bytes(cs)
    (native_dir / "helioc_vertex.bin").write_bytes(vs)
    (native_dir / "helioc_fragment_simd16.bin").write_bytes(ps16)
    return cs, vs, ps16


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
    parser.add_argument("artifact", type=Path, nargs="?")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--work-dir", type=Path)
    parser.add_argument("--device-id", help="Force the Intel Vulkan compiler device, e.g. 0xA780")
    parser.add_argument(
        "--helioc", action="store_true",
        help=(
            "capture sealed authored cloud WGSL through pinned Naga/ANV and "
            "fail closed unless native state and backing layout are proven"
        ),
    )
    args = parser.parse_args()

    for path in (NAGA_MANIFEST, UPSTREAM_DUMPER, UPSTREAM_EXTRACTOR):
        if not path.exists():
            raise SystemExit(f"required pinned input is absent: {path}")
    for tool in ("cargo", "cc", "python3"):
        if shutil.which(tool) is None:
            raise SystemExit(f"required tool not found: {tool}")

    if args.helioc:
        if args.artifact is not None:
            parser.error("artifact is not accepted with --helioc")
        bake_helioc(args)
        return
    if args.artifact is None:
        parser.error("artifact is required unless --helioc is supplied")

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
