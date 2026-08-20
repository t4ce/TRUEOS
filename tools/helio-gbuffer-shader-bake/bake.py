#!/usr/bin/env python3
"""Compile Helio's exact G-buffer WGSL through Naga and Intel Vulkan."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


TRUEOS = Path(__file__).resolve().parents[2]
HELIO = TRUEOS.parent / "Helio"
DEFAULT_WGSL = HELIO / "crates/passes/3d/helio-pass-gbuffer/shaders/gbuffer.wgsl"
DEFAULT_OUT = TRUEOS / "assets/helio/helio-gbuffer"
BAKER_PATH = TRUEOS / "tools/helio-intel-bake/bake.py"
DUMPER_SOURCE = Path(__file__).with_name("gbuffer_pipeline_dump.c")

WGSL_FILE = "gbuffer.wgsl"
VERTEX_SPIRV_FILE = "gbuffer.vs.spv"
FRAGMENT_SPIRV_FILE = "gbuffer.fs.spv"
VERTEX_NATIVE_FILE = "gbuffer.vs.simd8.bin"
FRAGMENT_NATIVE_FILE = "gbuffer.fs.simd8.bin"
METADATA_FILE = "metadata.json"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_baker():
    spec = importlib.util.spec_from_file_location("trueos_helio_baker", BAKER_PATH)
    if spec is None or spec.loader is None:
        raise SystemExit("cannot load Helio Intel baker")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wgsl", type=Path, default=DEFAULT_WGSL)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--device-id", help="select one Intel Vulkan device, e.g. 0xA780")
    parser.add_argument("--validate-only", action="store_true")
    return parser.parse_args()


def require_tool(name: str) -> None:
    if shutil.which(name) is None:
        raise SystemExit(f"required tool not found: {name}")


def validate_artifact(output: Path) -> dict[str, object]:
    metadata_path = output / METADATA_FILE
    if not metadata_path.is_file():
        raise SystemExit(f"missing G-buffer metadata: {metadata_path}")
    metadata = json.loads(metadata_path.read_text())
    expected_contract = {
        "vertex_stride": 40,
        "bind_group_count": 2,
        "material_texture_count": 256,
        "color_target_count": 8,
        "depth_format": "D32_SFLOAT",
        "depth_compare": "LESS_OR_EQUAL",
    }
    if metadata.get("pipeline_contract") != expected_contract:
        raise SystemExit("G-buffer pipeline contract metadata drifted")
    files = metadata.get("files")
    if not isinstance(files, dict):
        raise SystemExit("G-buffer metadata has no file table")
    for name in (
        WGSL_FILE,
        VERTEX_SPIRV_FILE,
        FRAGMENT_SPIRV_FILE,
        VERTEX_NATIVE_FILE,
        FRAGMENT_NATIVE_FILE,
    ):
        record = files.get(name)
        path = output / name
        if not isinstance(record, dict) or not path.is_file():
            raise SystemExit(f"missing G-buffer artifact file: {name}")
        data = path.read_bytes()
        if record.get("bytes") != len(data) or record.get("sha256") != sha256(data):
            raise SystemExit(f"G-buffer artifact hash/size mismatch: {name}")
    if not (output / "compile.log").is_file():
        raise SystemExit("G-buffer artifact has no Intel compile log")
    if not list(output.glob("*_GEN_Assembly.txt")):
        raise SystemExit("G-buffer artifact has no Intel assembly evidence")
    print(f"validated {output}")
    print(
        f"  WGSL sha256={files[WGSL_FILE]['sha256']} "
        f"VS={files[VERTEX_NATIVE_FILE]['bytes']} bytes "
        f"FS={files[FRAGMENT_NATIVE_FILE]['bytes']} bytes"
    )
    return metadata


def main() -> None:
    args = parse_args()
    output = args.out.expanduser().resolve()
    if args.validate_only:
        validate_artifact(output)
        return

    wgsl = args.wgsl.expanduser().resolve()
    for path in (wgsl, BAKER_PATH, DUMPER_SOURCE):
        if not path.is_file():
            raise SystemExit(f"missing required input: {path}")
    for tool in ("cargo", "cc"):
        require_tool(tool)

    baker = load_baker()
    with tempfile.TemporaryDirectory(prefix="trueos-helio-gbuffer-bake.") as raw:
        work = Path(raw)
        vertex_spirv = work / VERTEX_SPIRV_FILE
        fragment_spirv = work / FRAGMENT_SPIRV_FILE
        baker.naga_compile(wgsl, "vs_main", vertex_spirv)
        baker.naga_compile(wgsl, "fs_main", fragment_spirv)

        dumper = work / "helio_gbuffer_pipeline_dump"
        subprocess.run(
            [
                "cc",
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Werror",
                str(DUMPER_SOURCE),
                "-o",
                str(dumper),
                *baker.vulkan_compile_flags(),
            ],
            check=True,
        )
        executable_dir = work / "pipeline-executables"
        executable_dir.mkdir()
        compile_log = work / "compile.log"
        environment = os.environ.copy()
        environment["TRUEOS_EXECUTABLE_DUMP_DIR"] = str(executable_dir)
        if args.device_id:
            environment["TRUEOS_VK_DEVICE_ID"] = args.device_id
        baker.run(
            [str(dumper), str(vertex_spirv), str(fragment_spirv)],
            env=environment,
            log=compile_log,
        )
        log_text = compile_log.read_text()
        completion = (
            "helio_gbuffer_dump: compiled_only=1 vertex_stride=40 "
            "sets=2 textures=256 color_targets=8 depth=VK_FORMAT_D32_SFLOAT"
        )
        if completion not in log_text:
            raise SystemExit("Intel G-buffer pipeline compilation did not complete")

        native_dir = work / "native"
        vertex_native, fragment_native, unexpected_fragment = baker.extract_native(
            executable_dir, native_dir
        )
        if unexpected_fragment is not None:
            raise SystemExit("G-buffer capture unexpectedly emitted multiple fragment variants")
        if not vertex_native or not fragment_native:
            raise SystemExit("G-buffer native stages are empty")
        if len(vertex_native) % 8 or len(fragment_native) % 8:
            raise SystemExit("G-buffer native stages are not Xe instruction aligned")
        device, executables = baker.parse_compile_log(log_text)

        source = wgsl.read_bytes()
        artifact_files = {
            WGSL_FILE: source,
            VERTEX_SPIRV_FILE: vertex_spirv.read_bytes(),
            FRAGMENT_SPIRV_FILE: fragment_spirv.read_bytes(),
            VERTEX_NATIVE_FILE: vertex_native,
            FRAGMENT_NATIVE_FILE: fragment_native,
        }
        metadata = {
            "schema": 1,
            "producer": "helio-gbuffer-shader-bake",
            "source": "Helio crates/passes/3d/helio-pass-gbuffer/shaders/gbuffer.wgsl",
            "frontend": "Helio vendored Naga WGSL-to-SPIR-V",
            "backend": "Mesa ANV Vulkan graphics pipeline",
            "compile_device": device,
            "entry_points": {"vertex": "vs_main", "fragment": "fs_main"},
            "pipeline_contract": {
                "vertex_stride": 40,
                "bind_group_count": 2,
                "material_texture_count": 256,
                "color_target_count": 8,
                "depth_format": "D32_SFLOAT",
                "depth_compare": "LESS_OR_EQUAL",
            },
            "vertex_attributes": [
                {"location": 0, "format": "R32G32B32_SFLOAT", "offset": 0},
                {"location": 1, "format": "R32_SFLOAT", "offset": 12},
                {"location": 2, "format": "R32G32_SFLOAT", "offset": 16},
                {"location": 5, "format": "R32G32_SFLOAT", "offset": 24},
                {"location": 3, "format": "R32_UINT", "offset": 32},
                {"location": 4, "format": "R32_UINT", "offset": 36},
            ],
            "color_targets": [
                "R8G8B8A8_UNORM",
                "R16G16B16A16_SFLOAT",
                "R8G8B8A8_UNORM",
                "R16G16B16A16_SFLOAT",
                "R16G16_SFLOAT",
                "R16G16B16A16_SFLOAT",
                "R16G16B16A16_SFLOAT",
                "R16G16_SFLOAT",
            ],
            "bindings": [
                {"group": 0, "binding": 0, "type": "storage-buffer", "count": 1},
                {"group": 0, "binding": 1, "type": "uniform-buffer", "count": 1},
                *[
                    {"group": 0, "binding": binding, "type": "storage-buffer", "count": 1}
                    for binding in range(2, 9)
                ],
                {"group": 1, "binding": 0, "type": "storage-buffer", "count": 1},
                {"group": 1, "binding": 1, "type": "storage-buffer", "count": 1},
                {
                    "group": 1,
                    "binding": 2,
                    "type": "sampled-image",
                    "count": 256,
                    "update_after_bind": True,
                },
                {
                    "group": 1,
                    "binding": 3,
                    "type": "sampler",
                    "count": 256,
                    "update_after_bind": True,
                },
            ],
            "required_vulkan_features": [
                "shaderSampledImageArrayNonUniformIndexing",
                "descriptorBindingSampledImageUpdateAfterBind",
            ],
            "compiler_executables": list(executables.values()),
            "files": {
                name: {"bytes": len(data), "sha256": sha256(data)}
                for name, data in artifact_files.items()
            },
        }

        output.mkdir(parents=True, exist_ok=True)
        for name, data in artifact_files.items():
            (output / name).write_bytes(data)
        shutil.copy2(compile_log, output / "compile.log")
        for assembly in executable_dir.glob("*_GEN_Assembly.txt"):
            shutil.copy2(assembly, output / assembly.name)
        push_map = next(executable_dir.glob("*_Shader_push_map.txt"), None)
        if push_map is not None:
            shutil.copy2(push_map, output / push_map.name)
        (output / METADATA_FILE).write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n"
        )

    validate_artifact(output)


if __name__ == "__main__":
    main()
