#!/usr/bin/env python3
"""Compile the five-float clip-position/UV VS for ADL-S without GPU submission.

Reuse the checked-in Picasso sampled SIMD16 fragment executable. Both stages
are independently decoded by IGA; the vertex payload is also checked against
the compiler's assembly. The Mesa no-op DRM shim is mandatory for this lane.
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
DEFAULT_MESA = ROOT / ".codex_tmp/trueos-adj-instrumented-rpls/mesa-build"
OUT = ROOT / "crates/trueos-shader/clip_position3_uv_texture"
GENERATED = ROOT / "crates/trueos-shader/generated_clip_position3_uv_texture.rs"
REUSED_PS = ROOT / "picasso/picasso-retained-textured-forward/retained_textured_forward.ps.simd16.bin"


def load_baker():
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("trueos_helio_baker", ROOT / "tools/helio-intel-bake/bake.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def decode(binary: Path) -> str:
    result = subprocess.run(
        ["iga64", "-d", "-p=12p1", "-Xprint-pc", str(binary)],
        text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False,
    )
    if result.returncode or "illegal" in result.stdout.lower():
        raise SystemExit(f"invalid ADL-S EU ISA in {binary}:\n{result.stdout}")
    return result.stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mesa-build", type=Path, default=DEFAULT_MESA)
    parser.add_argument("--work-dir", type=Path, default=ROOT / "bld/clip-position3-uv-bake")
    args = parser.parse_args()
    mesa = args.mesa_build.resolve()
    shim = mesa / "src/intel/tools/libintel_noop_drm_shim.so"
    driver = mesa / "src/intel/vulkan/libvulkan_intel.so"
    if not shim.is_file() or not driver.is_file():
        raise SystemExit("--mesa-build must contain ANV and libintel_noop_drm_shim.so")
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    baker = load_baker()

    stages = []
    for stage in ("vert", "frag"):
        spv = work / f"clip_position3_uv.{stage}.spv"
        subprocess.run(["glslc", "--target-env=vulkan1.1", str(HERE / "shaders" / f"clip_position3_uv.{stage}"), "-o", str(spv)], check=True)
        stages.append(spv)

    source_path = work / "pipeline_dump.c"
    baker.make_churn_compile_only_dumper(source_path)
    source = source_path.read_text()
    source = baker.replace_once(source, '.pName = "vs_main",', '.pName = "main",')
    source = baker.replace_once(source, '.pName = "fs_main",', '.pName = "main",')
    source = baker.replace_once(source, ".stride = 24,", ".stride = 20,")
    source = baker.replace_once(source,
        "{ .location = 1, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 12 },",
        "{ .location = 1, .binding = 0, .format = VK_FORMAT_R32G32_SFLOAT, .offset = 12 },")
    source = baker.replace_once(source, "const VkDescriptorSetLayoutBinding storage_bindings[3]", "const VkDescriptorSetLayoutBinding storage_bindings[5]")
    source = baker.replace_once(source,
        "{ .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,\n          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },",
        "{ .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,\n          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },\n"
        "        { .binding = 3, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,\n          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },\n"
        "        { .binding = 4, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER,\n          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },")
    source = baker.replace_once(source, ".bindingCount = 3,", ".bindingCount = 5,")
    source_path.write_text(source)
    dumper = work / "pipeline_dump"
    subprocess.run(["cc", str(source_path), "-o", str(dumper), *baker.vulkan_compile_flags()], check=True)
    icd = work / "adls-icd.json"
    icd.write_text(json.dumps({"file_format_version": "1.0.1", "ICD": {"api_version": "1.4.346", "library_path": str(driver)}}))
    executable_dir = work / "intel"
    executable_dir.mkdir(exist_ok=True)
    env = os.environ.copy()
    env.update({
        "LD_PRELOAD": str(shim), "VK_DRIVER_FILES": str(icd),
        "VK_ICD_FILENAMES": str(icd), "INTEL_STUB_GPU_DEVICE_ID": "4680",
        "TRUEOS_VK_DEVICE_ID": "0x4680", "MESA_SHADER_CACHE_DISABLE": "true",
        "TRUEOS_EXECUTABLE_DUMP_DIR": str(executable_dir),
    })
    log = work / "compile.log"
    baker.run([str(dumper), *(str(path) for path in stages)], env=env, log=log)
    log_text = log.read_text()
    device, executables = baker.parse_compile_log(log_text)
    if device["device_id"] != 0x4680 or "helio_pipeline_dump: compiled_only=1" not in log_text:
        raise SystemExit("missing target-matched compile-only proof")
    vs, _, _ = baker.extract_native(executable_dir, work / "native")
    ps = REUSED_PS.read_bytes()
    vs_path = work / "clip_position3_uv.vs.simd8.bin"
    vs_path.write_bytes(vs)
    vs_isa, ps_isa = decode(vs_path), decode(REUSED_PS)
    vertex_assembly, = executable_dir.glob("*_vertex_*_GEN_Assembly.txt")
    if (len(vs) != baker.assembly_code_size(vertex_assembly)
        or len(re.findall(r"^/\* \[[0-9A-Fa-f]+\]", vs_isa, re.MULTILINE)) != baker.assembly_instruction_count(vertex_assembly)
        or re.search(r"send\.urb.*\{[^}\n]*EOT", vs_isa) is None):
        raise SystemExit("VS cache extraction did not match complete compiler assembly")
    if (re.search(r"send\.smpl\s+\(16\|[^\n]*simd16 sample", ps_isa, re.IGNORECASE) is None
        or re.search(r"sendc?\.rc\s+\(16\|[^\n]*\{[^}\n]*EOT[^\n]*render target write SIMD16", ps_isa, re.IGNORECASE) is None):
        raise SystemExit("reused PS does not contain SIMD16 sampler and RT EOT messages")

    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / vs_path.name).write_bytes(vs)
    (OUT / "clip_position3_uv.vs.iga.txt").write_text(vs_isa)
    (OUT / "reused_picasso.ps.simd16.iga.txt").write_text(ps_isa)
    shutil.copy2(vertex_assembly, OUT / "clip_position3_uv.vs.mesa.txt")
    shutil.copy2(log, OUT / "compile.log")
    metadata = {
        "schema": 1, "contract": "clip-position3-uv-texture", "device": device,
        "backend": "mesa-anv-noop-drm-shim-compile-only", "iga_platform": "12p1",
        "host_render_verified": False, "baremetal_verified": False,
        "vertex_stride_bytes": 20, "vertex_attributes": ["float32x3@0", "float32x2@12"],
        "vertex_input_components": "xyz,uv", "vertex_varyings": ["smooth-perspective-vec2-location0"],
        "reused_fragment": str(REUSED_PS.relative_to(ROOT)),
        "vs_sha256": hashlib.sha256(vs).hexdigest(), "ps_sha256": hashlib.sha256(ps).hexdigest(),
        "vs_bytes": len(vs), "ps_bytes": len(ps), "executables": list(executables.values()),
    }
    (OUT / "metadata.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    print(json.dumps(metadata, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
