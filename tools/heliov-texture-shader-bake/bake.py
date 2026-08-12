#!/usr/bin/env python3
"""Bake HelioV's exact WGPU textured-mesh shader to authenticated Intel ISA."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


TRUEOS = Path(__file__).resolve().parents[2]
HELIO = TRUEOS.parent / "Helio"
BLUEPRINTS = TRUEOS.parent / "TRUEOS-Blueprints"
WGSL = BLUEPRINTS / "apps/HelioV/src/voxel_textured.wgsl"
OUT = TRUEOS / "assets/helio/heliov-textured-mesh"
BAKER_PATH = TRUEOS / "tools/helio-intel-bake/bake.py"


def load_baker():
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


def main() -> None:
    baker = load_baker()
    with tempfile.TemporaryDirectory(prefix="heliov-texture-bake-") as raw:
        work = Path(raw)
        vs_spv = work / "voxel-textured.vs.spv"
        fs_spv = work / "voxel-textured.fs.spv"
        baker.naga_compile(WGSL, "vs_main", vs_spv)
        baker.naga_compile(WGSL, "fs_main", fs_spv)

        dumper_source = work / "textured_pipeline_dump.c"
        baker.make_compile_only_dumper(dumper_source)
        source = dumper_source.read_text()
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
        .stride = 20,
        .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription attributes[2] = {
        { .location = 0, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 0 },
        { .location = 1, .binding = 0, .format = VK_FORMAT_R32G32_SFLOAT, .offset = 12 },
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
    };''', '''    const VkDescriptorSetLayoutBinding texture_bindings[2] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
    };
    const VkDescriptorSetLayoutCreateInfo set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 2,
        .pBindings = texture_bindings,
    };''')
        dumper_source.write_text(source)

        dumper = work / "textured_pipeline_dump"
        subprocess.run(
            ["cc", str(dumper_source), "-o", str(dumper), *baker.vulkan_compile_flags()],
            check=True,
        )
        exec_dir = work / "intel"
        exec_dir.mkdir()
        log = work / "compile.log"
        env = dict(**__import__("os").environ)
        env["TRUEOS_EXECUTABLE_DUMP_DIR"] = str(exec_dir)
        baker.run([str(dumper), str(vs_spv), str(fs_spv)], env=env, log=log)
        device, executables = baker.parse_compile_log(log.read_text())
        vs, ps8, ps16 = baker.extract_native(exec_dir, work / "native")

        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / "voxel_textured.vs.bin").write_bytes(vs)
        (OUT / "voxel_textured.ps.simd8.bin").write_bytes(ps8)
        if ps16 is not None:
            (OUT / "voxel_textured.ps.simd16.bin").write_bytes(ps16)
        shutil.copy2(WGSL, OUT / "voxel_textured.wgsl")
        shutil.copy2(log, OUT / "compile.log")
        for assembly in exec_dir.glob("*_GEN_Assembly.txt"):
            shutil.copy2(assembly, OUT / assembly.name)
        metadata = {
            "device": device,
            "executables": list(executables.values()),
            "wgsl_fnv1a64": f"0x{fnv1a64(WGSL.read_bytes()):016x}",
            "vertex_stride": 20,
            "attributes": ["float32x3@0", "float32x2@12"],
            "bindings": ["sampled-rgba8-2d@0", "sampler@1"],
            "native_bytes": {"vs": len(vs), "ps8": len(ps8), "ps16": len(ps16 or b"")},
        }
        (OUT / "metadata.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
        print(json.dumps(metadata, indent=2, sort_keys=True))


def fnv1a64(data: bytes) -> int:
    value = 0xCBF29CE484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return value


if __name__ == "__main__":
    main()
