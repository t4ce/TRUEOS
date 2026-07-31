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
    _put_f32s(out, 24, (0.0, 8.0, 35.0, 0.0, -0.2,
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
        if name.endswith(".wgsl") and b"@vertex" in data and b"@fragment" in data
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


def naga_compile(wgsl: Path, entry: str, output: Path) -> None:
    run([
        "cargo", "run", "-q", "--manifest-path", str(NAGA_MANIFEST), "--",
        "--entry-point", entry, str(wgsl), str(output),
    ], cwd=HELIO)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def vulkan_compile_flags() -> list[str]:
    include_roots = [
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
        if not line or line[0].isspace() or line == "\0":
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
    # These scene contracts are the build-time handoff from the hosted Helio
    # demos to TRUEOS's no_std retained renderer. They deliberately share the
    # captured Helio/WGPU graph and native shader pair in this artifact.
    sections["scene/shape-battle-v1.bin"] = (
        OTHER_SECTION_KIND, encode_shape_battle_scene(),
    )
    sections["scene/pendulum-bigcloth-v1.bin"] = (
        OTHER_SECTION_KIND, encode_pendulum_bigcloth_scene(),
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
    exec_dir = work / "pipeline_exec"
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
