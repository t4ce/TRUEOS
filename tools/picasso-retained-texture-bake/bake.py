#!/usr/bin/env python3
"""Bake Picasso's retained transform + sampled-material Intel program."""

from __future__ import annotations

import importlib.util
from contextlib import nullcontext
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


TRUEOS = Path(__file__).resolve().parents[2]
BAKER_PATH = TRUEOS / "tools/helio-intel-bake/bake.py"
WGSL = Path(__file__).resolve().parent / "shaders/retained_textured_forward.wgsl"
OUT = TRUEOS / "picasso/picasso-retained-textured-forward"


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


def validate_vertex_isa(baker, work: Path, assembly: Path, binary: bytes) -> None:
    """Require the recovered VS payload to be decodable gfx12.5 EU ISA."""
    expected_bytes = baker.assembly_code_size(assembly)
    expected_instructions = baker.assembly_instruction_count(assembly)
    if len(binary) != expected_bytes:
        raise SystemExit(
            f"retained vertex ISA size mismatch: {len(binary)} != {expected_bytes}"
        )
    iga = shutil.which("iga64")
    if iga is None:
        raise SystemExit("iga64 is required to validate retained vertex ISA")
    candidate = work / "retained_textured_forward.vs.recovered.bin"
    candidate.write_bytes(binary)
    decoded = subprocess.run(
        [iga, "-d", "-p=12p5", "-Xprint-pc", str(candidate)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    rows = re.findall(r"^/\* \[[0-9A-Fa-f]+\]", decoded.stdout, re.MULTILINE)
    if (
        decoded.returncode != 0
        or len(rows) != expected_instructions
        or not re.search(r"send\.urb.*\{EOT[,}]", decoded.stdout)
    ):
        sys.stderr.write(decoded.stdout)
        raise SystemExit(
            "retained vertex cache payload is reflection/trailer data, not complete EU ISA"
        )


def validate_fragment_isa(work: Path, binary: bytes) -> None:
    """Require independently decodable SIMD16 sampler and render sends."""
    iga = shutil.which("iga64")
    if iga is None:
        raise SystemExit("iga64 is required to validate retained fragment ISA")
    candidate = work / "retained_textured_forward.ps.simd16.recovered.bin"
    candidate.write_bytes(binary)
    decoded = subprocess.run(
        [iga, "-d", "-p=12p5", "-Xprint-pc", str(candidate)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    rows = re.findall(r"^/\* \[[0-9A-Fa-f]+\]", decoded.stdout, re.MULTILINE)
    sampler_send = re.search(
        r"\bsend\.smpl\s+\(16\|[^\n]*\bsimd16 sample\b",
        decoded.stdout,
        re.IGNORECASE,
    )
    render_eot_send = re.search(
        r"\bsendc?\.rc\s+\(16\|[^\n]*\{[^}\n]*\bEOT\b[^}\n]*\}"
        r"[^\n]*\brender target write SIMD16\b",
        decoded.stdout,
        re.IGNORECASE,
    )
    if (
        decoded.returncode != 0
        or not rows
        or sampler_send is None
        or render_eot_send is None
    ):
        sys.stderr.write(decoded.stdout)
        raise SystemExit("retained fragment ISA lacks the required sampler/RT messages")


def main() -> None:
    baker = load_baker()
    retained_work = os.environ.get("TRUEOS_PICASSO_BAKE_WORK_DIR")
    work_context = (
        nullcontext(retained_work)
        if retained_work
        else tempfile.TemporaryDirectory(
            prefix="picasso-retained-texture-bake-",
            dir=TRUEOS / "bld",
        )
    )
    with work_context as raw:
        work = Path(raw)
        work.mkdir(parents=True, exist_ok=True)
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
    };""", """    const VkDescriptorSetLayoutBinding retained_bindings[5] = {
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
    };
    const VkDescriptorSetLayoutCreateInfo set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 5,
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
        baker.run([str(dumper), str(vs_spv), str(fs_spv)], env=env, log=log)
        device, executables = baker.parse_compile_log(log.read_text())
        vs, ps8, ps16 = baker.extract_native(exec_dir, work / "native")
        vertex_assemblies = sorted(exec_dir.glob("*_vertex_*_GEN_Assembly.txt"))
        if len(vertex_assemblies) != 1:
            raise SystemExit("retained pipeline exposed no unique vertex assembly")
        validate_vertex_isa(baker, work, vertex_assemblies[0], vs)
        if ps16 is None:
            raise SystemExit("retained pipeline exposed no SIMD16 fragment executable")
        validate_fragment_isa(work, ps16)

        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / "retained_textured_forward.vs.simd8.bin").write_bytes(vs)
        (OUT / "retained_textured_forward.ps.simd8.bin").write_bytes(ps8)
        if ps16 is not None:
            (OUT / "retained_textured_forward.ps.simd16.bin").write_bytes(ps16)
        shutil.copy2(WGSL, OUT / WGSL.name)
        shutil.copy2(log, OUT / "compile.log")
        for assembly in exec_dir.glob("*_GEN_Assembly.txt"):
            shutil.copy2(assembly, OUT / assembly.name)
        metadata = {
            "schema": 1,
            "contract": "retained-transform-filtered-base-color",
            "frontend": frontend,
            "device": device,
            "executables": list(executables.values()),
            "vertex_stride": 32,
            "runtime_dispatch": {"vertex": "simd8", "fragment": "simd16"},
            "fragment_validation": "iga64-decode-simd16-sampler-send-and-rt-eot",
            "storage_attributes": ["float32x3@0", "float32x3@12", "float32x2@24"],
            "shader_attributes": ["float32x3@0", "float32x2@24"],
            "bindings": [
                "storage-camera@vs-bti1",
                "storage-instances@vs-bti2",
                "storage-compacted-indices@vs-bti3",
                "sampled-rgba8-2d@ps-bti2",
                "sampler@ps-sampler0",
            ],
            "native_bytes": {"vs": len(vs), "ps8": len(ps8), "ps16": len(ps16 or b"")},
        }
        (OUT / "metadata.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
        print(json.dumps(metadata, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
