#!/usr/bin/env python3
"""Bake Picasso's retained transform + sampled-material Intel program."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile


TRUEOS = Path(__file__).resolve().parents[2]
BAKER_PATH = TRUEOS / "tools/helio-intel-bake/bake.py"
WGSL = Path(__file__).resolve().parent / "shaders/retained_textured_forward.wgsl"
OUT = TRUEOS / "assets/helio/picasso-retained-textured-forward"
ADLS_DEVICE_ID = 0x4680
ADLS_NOOP_SHIM_IDENTITY = "noop-drm-shim:8086:4680:r0c"


def load_baker():
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("trueos_helio_baker", BAKER_PATH)
    if spec is None or spec.loader is None:
        raise SystemExit("cannot load Helio Intel baker")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def replace_once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise SystemExit(f"compiler dumper drift for {old[:48]!r}")
    return source.replace(old, new)


def canonical_vertex_program(exec_dir: Path, fallback: bytes) -> bytes:
    """Prefer ANV's complete shader serialization when an instrumented ICD emits it.

    The normal graphics bake only exposes the opaque pipeline cache, for which
    `extract_native` retains its established assembly-bounded recovery.  The
    pinned ADL-S no-op shim also exposes ANV's canonical program serialization;
    use that exact `program_size` record when available so a cache subrecord
    cannot silently truncate a larger vertex stage.
    """
    serialized = sorted(
        exec_dir.glob("*_vertex_*_TRUEOS_HelioC_shader_serialize.bin")
    )
    if not serialized:
        return fallback
    if len(serialized) != 1:
        raise SystemExit("Picasso retained texture bake has ambiguous vertex serializations")
    data = serialized[0].read_bytes()
    if len(data) < 8:
        raise SystemExit("Picasso retained texture vertex serialization is truncated")
    stage, program_bytes = struct.unpack_from("<II", data)
    if stage != 0 or program_bytes == 0 or 8 + program_bytes > len(data):
        raise SystemExit("Picasso retained texture vertex serialization has invalid stage/size")
    program = data[8:8 + program_bytes]
    if len(program) % 8 != 0:
        raise SystemExit("Picasso retained texture vertex program is not EU instruction aligned")
    return program


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        type=Path,
        default=OUT,
        help="output directory (default: the checked-in Picasso retained-texture asset)",
    )
    parser.add_argument(
        "--adls-noop-shim-candidate",
        action="store_true",
        help=(
            "allow the pinned no-op DRM shim to emit a source-level ADL-S candidate; "
            "the result is never physical-hardware admission evidence"
        ),
    )
    args = parser.parse_args()
    out = args.out.resolve()
    if args.adls_noop_shim_candidate:
        if os.environ.get("TRUEOS_PICASSO_ADLS_CANDIDATE_IDENTITY") != ADLS_NOOP_SHIM_IDENTITY:
            raise SystemExit("Picasso ADL-S shim candidate requires its authenticated no-op identity")
        provenance = "adls-noop-drm-shim-compiler-candidate"
    else:
        provenance = "physical-adl-s-vulkan-pipeline-executable"

    baker = load_baker()
    with tempfile.TemporaryDirectory(
        prefix="picasso-retained-texture-bake-",
        dir=TRUEOS / "bld",
    ) as raw:
        work = Path(raw)
        vs_spv = work / "retained_textured_forward.vs.spv"
        fs_spv = work / "retained_textured_forward.fs.spv"
        if baker.HELIO.is_dir() and baker.NAGA_MANIFEST.is_file():
            baker.naga_compile(WGSL, "vs_main", vs_spv)
            baker.naga_compile(WGSL, "fs_main", fs_spv)
            frontend = "wgsl-via-helio-vendored-naga"
        else:
            wgsl_spv = TRUEOS / "tools/wgsl-spv/Cargo.toml"
            subprocess.run(
                [
                    "cargo", "run", "--offline", "-q", "--target", "x86_64-unknown-linux-gnu",
                    "--manifest-path", str(wgsl_spv),
                    "--", "vs_main", str(WGSL), str(vs_spv),
                ],
                check=True,
                cwd=TRUEOS.parent,
            )
            subprocess.run(
                [
                    "cargo", "run", "--offline", "-q", "--target", "x86_64-unknown-linux-gnu",
                    "--manifest-path", str(wgsl_spv),
                    "--", "fs_main", str(WGSL), str(fs_spv),
                ],
                check=True,
                cwd=TRUEOS.parent,
            )
            frontend = "wgsl-via-checked-in-naga-wrapper"

        source_path = work / "pipeline_dump.c"
        baker.make_churn_compile_only_dumper(source_path)
        source = source_path.read_text()
        source = replace_once(source, """    const VkVertexInputBindingDescription binding = {
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
    };""", """    const VkVertexInputBindingDescription binding = {
        .binding = 0,
        .stride = 32,
        .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription attributes[2] = {
        { .location = 0, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 0 },
        { .location = 1, .binding = 0, .format = VK_FORMAT_R32G32_SFLOAT, .offset = 24 },
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1,
        .pVertexBindingDescriptions = &binding,
        .vertexAttributeDescriptionCount = 2,
        .pVertexAttributeDescriptions = attributes,
    };""")
        source = replace_once(source, """    const VkDescriptorSetLayoutBinding storage_bindings[3] = {
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
    };""", """    const VkDescriptorSetLayoutBinding retained_bindings[6] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 3, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 4, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 5, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
    };
    const VkDescriptorSetLayoutCreateInfo set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 6,
        .pBindings = retained_bindings,
    };""")
        source_path.write_text(source)

        dumper = work / "pipeline_dump"
        subprocess.run(
            ["cc", str(source_path), "-o", str(dumper), *baker.vulkan_compile_flags()],
            check=True,
        )
        exec_dir = work / "intel"
        exec_dir.mkdir()
        log = work / "compile.log"
        env = os.environ.copy()
        env["TRUEOS_EXECUTABLE_DUMP_DIR"] = str(exec_dir)
        # The target shader is valid only for the physical Picasso GPU.  A
        # workstation RPL compile may help inspect code generation, but is not
        # a substitute for an ADL-S graphics pipeline executable.
        env["TRUEOS_VK_DEVICE_ID"] = f"0x{ADLS_DEVICE_ID:04X}"
        baker.run([str(dumper), str(vs_spv), str(fs_spv)], env=env, log=log)
        device, executables = baker.parse_compile_log(log.read_text())
        if device.get("vendor_id") != 0x8086 or device.get("device_id") != ADLS_DEVICE_ID:
            raise SystemExit(
                "Picasso retained texture bake requires Intel ADL-S device 0x4680; "
                f"compiler selected vendor=0x{device.get('vendor_id', 0):04X} "
                f"device=0x{device.get('device_id', 0):04X}"
            )
        cache_vs, ps8, ps16 = baker.extract_native(exec_dir, work / "native")
        vs = canonical_vertex_program(exec_dir, cache_vs)
        vertex_sizes = [
            executable["statistics"].get("Code size")
            for executable in executables.values()
            if executable["stage"] == "vertex" and executable["simd_width"] == 8
        ]
        if len(vertex_sizes) != 1 or not isinstance(vertex_sizes[0], int):
            raise SystemExit("Picasso retained texture bake lacks one SIMD8 vertex code-size record")
        if len(vs) != vertex_sizes[0]:
            raise SystemExit(
                "Picasso retained texture vertex recovery disagrees with Mesa's code size: "
                f"recovered={len(vs)} reported={vertex_sizes[0]}"
            )
        if ps16 is None or len(ps16) == 0 or len(ps16) % 8:
            raise SystemExit("Picasso retained material bake lacks an aligned SIMD16 fragment")

        out.mkdir(parents=True, exist_ok=True)
        (out / "retained_textured_forward.vs.simd8.bin").write_bytes(vs)
        (out / "retained_textured_forward.ps.simd8.bin").write_bytes(ps8)
        if ps16 is not None:
            (out / "retained_textured_forward.ps.simd16.bin").write_bytes(ps16)
        shutil.copy2(WGSL, out / WGSL.name)
        shutil.copy2(log, out / "compile.log")
        for assembly in exec_dir.glob("*_GEN_Assembly.txt"):
            shutil.copy2(assembly, out / assembly.name)
        metadata = {
            "schema": 1,
            "contract": "retained-transform-filtered-base-color-plus-emissive",
            "frontend": frontend,
            "device": device,
            "target": "intel-adl-s-uhd-770-0x4680",
            "provenance": provenance,
            "executables": list(executables.values()),
            "vertex_stride": 32,
            "runtime_dispatch": {"vertex": "simd8", "fragment": "simd16"},
            "fragment_proof": "adls-noop-drm-shim-compiler-candidate",
            "storage_attributes": ["float32x3@0", "float32x3@12", "float32x2@24"],
            "shader_attributes": ["float32x3@0", "float32x2@24"],
            "bindings": [
                "storage-camera@vs-bti1",
                "storage-instances@vs-bti2",
                "storage-compacted-indices@vs-bti3",
                "sampled-base-color-rgba8-2d@ps-bti2",
                "sampler@ps-sampler0",
                "sampled-emissive-rgba8-2d@ps-bti3",
            ],
            "native_bytes": {"vs": len(vs), "ps8": len(ps8), "ps16": len(ps16 or b"")},
        }
        (out / "metadata.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
        print(json.dumps(metadata, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
