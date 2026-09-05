#!/usr/bin/env python3
"""Bake the retained tangent-space five-map PBR shader for ADL-S, without submission.

Requires the instrumented ANV build used by clip-position3-uv-bake. The old
base-color-only binaries remain immutable; PBR is a distinct artifact package.
"""
from __future__ import annotations
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
SOURCE = HERE / "shaders/retained_pbr_forward.wgsl"
OUT = ROOT / "picasso/picasso-retained-pbr-forward"
DEFAULT_MESA = ROOT / ".codex_tmp/trueos-adj-instrumented-rpls/mesa-build"


def module(path: Path, name: str):
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def numbers(path: Path) -> dict[str, int]:
    return {key: int(value, 0) for key, value in re.findall(r"(\w+)=(0x[0-9a-fA-F]+|\d+)", path.read_text())}


def serialized_code(path: Path, stage: int) -> bytes:
    raw = path.read_bytes()
    found_stage, count = struct.unpack_from("<II", raw)
    if found_stage != stage or count == 0 or count % 4 or count + 8 > len(raw):
        raise SystemExit(f"invalid ANV shader serialize record: {path}")
    # Pinned ANV anv_shader_serialize: stage, program_size, complete native code.
    return raw[8:8 + count]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mesa-build", type=Path, default=DEFAULT_MESA)
    parser.add_argument("--work-dir", type=Path, default=ROOT / "bld/picasso-retained-pbr-bake")
    args = parser.parse_args()
    mesa = args.mesa_build.resolve()
    shim = mesa / "src/intel/tools/libintel_noop_drm_shim.so"
    driver = mesa / "src/intel/vulkan/libvulkan_intel.so"
    if not shim.is_file() or not driver.is_file():
        raise SystemExit("instrumented ANV and no-op DRM shim required")
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    baker = module(ROOT / "tools/helio-intel-bake/bake.py", "helio_baker")
    decoder = module(ROOT / "tools/clip-position3-uv-bake/bake.py", "clip_baker")
    stages = []
    for entry in ("vs_main", "fs_main"):
        spv = work / f"{entry}.spv"
        subprocess.run(["cargo", "run", "--offline", "-q", "--target", "x86_64-unknown-linux-gnu", "--manifest-path", str(ROOT / "tools/wgsl-spv/Cargo.toml"), "--", entry, str(SOURCE), str(spv)], cwd=ROOT.parent, check=True)
        stages.append(spv)
    c_path = work / "pipeline_dump.c"
    baker.make_churn_compile_only_dumper(c_path)
    c = c_path.read_text()
    start = c.index("    const VkVertexInputBindingDescription binding = {")
    end = c.index("    };", c.index("    const VkPipelineVertexInputStateCreateInfo vertex_input = {", start)) + len("    };")
    c = c[:start] + '''    const VkVertexInputBindingDescription binding = {
        .binding = 0, .stride = 48, .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription attributes[4] = {
        { .location = 0, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 0 },
        { .location = 1, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 12 },
        { .location = 2, .binding = 0, .format = VK_FORMAT_R32G32_SFLOAT, .offset = 24 },
        { .location = 3, .binding = 0, .format = VK_FORMAT_R32G32B32A32_SFLOAT, .offset = 32 },
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1, .pVertexBindingDescriptions = &binding,
        .vertexAttributeDescriptionCount = 4, .pVertexAttributeDescriptions = attributes,
    };''' + c[end:]
    start = c.index("    const VkDescriptorSetLayoutBinding storage_bindings[3] = {")
    end = c.index("    };", start) + len("    };")
    bindings = []
    for index in range(10):
        kind = "STORAGE_BUFFER" if index in (0, 1, 2, 9) else "SAMPLER" if index == 4 else "SAMPLED_IMAGE"
        stage = "VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT" if index == 0 else "VK_SHADER_STAGE_VERTEX_BIT" if index in (1, 2) else "VK_SHADER_STAGE_FRAGMENT_BIT"
        bindings.append(f"        {{ .binding = {index}, .descriptorType = VK_DESCRIPTOR_TYPE_{kind}, .descriptorCount = 1, .stageFlags = {stage} }},")
    c = c[:start] + "    const VkDescriptorSetLayoutBinding storage_bindings[10] = {\n" + "\n".join(bindings) + "\n    };" + c[end:]
    c = baker.replace_once(c, ".bindingCount = 3,", ".bindingCount = 10,")
    c_path.write_text(c)
    dumper = work / "pipeline_dump"
    subprocess.run(["cc", str(c_path), "-o", str(dumper), *baker.vulkan_compile_flags()], check=True)
    icd = work / "adls-icd.json"
    icd.write_text(json.dumps({"file_format_version": "1.0.1", "ICD": {"api_version": "1.4.346", "library_path": str(driver)}}))
    capture = work / f"intel-{os.getpid()}"
    capture.mkdir()
    env = os.environ.copy()
    env.update({"LD_PRELOAD": str(shim), "VK_DRIVER_FILES": str(icd), "VK_ICD_FILENAMES": str(icd), "INTEL_STUB_GPU_DEVICE_ID": "4680", "TRUEOS_VK_DEVICE_ID": "0x4680", "MESA_SHADER_CACHE_DISABLE": "true", "TRUEOS_EXECUTABLE_DUMP_DIR": str(capture)})
    log = work / "compile.log"
    baker.run([str(dumper), *(str(path) for path in stages)], env=env, log=log)
    device, executables = baker.parse_compile_log(log.read_text())
    if device["device_id"] != 0x4680 or "compiled_only=1" not in log.read_text():
        raise SystemExit("not an ADL-S compile-only capture")
    vs_state = numbers(capture / "vertex_TRUEOS_VS_state_v1.txt")
    ps_state = numbers(capture / "fragment_TRUEOS_PS_state_v1.txt")
    vs_serialized, = capture.glob("*_vertex_*_shader_serialize.bin")
    fs_serialized = sorted(capture.glob("*_fragment_*_shader_serialize.bin"))[0]
    vs = serialized_code(vs_serialized, 0)
    fs = serialized_code(fs_serialized, 4)
    fragment_assemblies = sorted(capture.glob("*_fragment_*_GEN_Assembly.txt"))
    if not ps_state["dispatch16"]:
        raise SystemExit("SIMD16 PBR executable missing")
    ps16_assembly = fragment_assemblies[1] if ps_state["dispatch8"] else fragment_assemblies[0]
    ps_size = baker.assembly_code_size(ps16_assembly)
    ps = fs[ps_state["offset16"]:ps_state["offset16"] + ps_size]
    if ps_state["scratch_bytes"] != 0:
        raise SystemExit(f"PBR PS requires unimplemented scratch: {ps_state['scratch_bytes']}")
    OUT.mkdir(parents=True, exist_ok=True)
    for stage, code in (("vs.simd8", vs), ("ps.simd16", ps)):
        binary = work / f"retained_pbr_forward.{stage}.bin"
        binary.write_bytes(code)
        isa = decoder.decode(binary)
        if "EOT" not in isa:
            raise SystemExit(f"no EOT in {stage}")
        if stage.startswith("ps") and len(re.findall(r"send\.smpl\s+\(16\|", isa)) != 5:
            raise SystemExit("PBR fragment shader must have five independently decoded sampler sends")
        shutil.copy2(binary, OUT / binary.name)
        (OUT / f"retained_pbr_forward.{stage}.iga.txt").write_text(isa)
    for pattern in ("*_GEN_Assembly.txt", "*_bind_map.txt", "*_devinfo.txt", "*_TRUEOS_*_state_v1.txt"):
        for path in capture.glob(pattern):
            shutil.copy2(path, OUT / path.name)
    shutil.copy2(SOURCE, OUT / SOURCE.name)
    shutil.copy2(log, OUT / log.name)
    metadata = {"schema": 1, "contract": "retained-gltf-metallic-roughness-five-map-tangent48", "device": device, "backend": "instrumented-mesa-anv-noop-drm-shim", "vertex_stride": 48, "vertex_attributes": ["float32x3@0", "float32x3@12", "float32x2@24", "float32x4@32"], "varyings": ["world_position", "world_normal", "uv", "world_tangent"], "vs_state": vs_state, "ps_state": ps_state, "vs_bytes": len(vs), "ps_bytes": len(ps), "vs_sha256": hashlib.sha256(vs).hexdigest(), "ps_sha256": hashlib.sha256(ps).hexdigest(), "executables": list(executables.values()), "material_bytes": 64, "color_surface_formats": {"base_color": "R8G8B8A8_UNORM_SRGB", "emissive": "R8G8B8A8_UNORM_SRGB", "metallic_roughness": "R8G8B8A8_UNORM", "normal": "R8G8B8A8_UNORM", "occlusion": "R8G8B8A8_UNORM"}, "host_render_verified": False, "baremetal_verified": False}
    (OUT / "metadata.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    print(json.dumps(metadata, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
