#!/usr/bin/env python3
"""Bake the procedural Dista Float3 vertex shader for ADL-S, without submitting GPU work."""
from __future__ import annotations

import argparse
import ast
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "vendor/microfont/shaders/dista_stamp.vert"
DEFAULT_MESA = ROOT / "bld/toolchains/microfont-dista-mesa"
OUT = ROOT / "crates/trueos-shader/microfont_dista"
GENERATED = ROOT / "crates/trueos-shader/generated_microfont_dista.rs"


def basic_seeds() -> list[tuple[int, bool]]:
    """Read source data only; glyph geometry and graph selection remain in GLSL."""
    source = (ROOT / "vendor/microfont/src/basic.rs").read_text()
    table = source.split("pub const BASIC_SYMBOLS:", 1)[1].split("];", 1)[0]
    result = []
    char = r"'((?:\\.|[^'\\])*)'"
    number = r"(?:0x[0-9a-fA-F_]+|0)"
    pattern = rf"^\s*\(\s*{char}\s*,\s*({number}|z\(\s*{number}\s*,\s*[\d_]+\s*\))\s*\),"
    for literal, expression in re.findall(pattern, table, re.MULTILINE):
        code = ord(rust_char(literal))
        if code != len(result):
            raise ValueError(f"Basic table entry {len(result)} has codepoint {code}; expected ordered U+0000..U+007F")
        if expression.startswith("z("):
            value, shift = re.fullmatch(rf"z\(\s*({number})\s*,\s*([\d_]+)\s*\)", expression).groups()
            shift = int(shift.replace("_", ""))
            if not 0 <= shift < 128:
                raise ValueError("Basic z() shift exceeds u128")
            pixels = int(value.replace("_", ""), 0) << shift
        else:
            pixels = int(expression.replace("_", ""), 0)
        result.append((pixels, False))
    if len(result) != 128:
        raise ValueError(f"Basic seed parser expected 128 entries, found {len(result)}")
    overrides = source.split("pub const BASIC_RASTER_OVERRIDES:", 1)[1].split("];", 1)[0]
    seen = set()
    entries = re.findall(
        rf"\(\s*{char}\s*,\s*crate::raster::GlyphSeed\s*\{{\s*pixels:\s*({number})\s*,\s*long_reach:\s*(true|false)\s*,?\s*\}}\s*\)", overrides)
    declared = re.search(r";\s*(\d+)\]\s*=\s*\[", overrides)
    if not declared or len(entries) != int(declared[1]):
        raise ValueError("Basic override parser count differs from its declared Rust array length")
    for literal, pixels, long_reach in entries:
        code = ord(rust_char(literal))
        if code >= 128 or code in seen:
            raise ValueError("Basic override has an unknown or duplicate codepoint")
        seen.add(code)
        result[code] = (int(pixels.replace("_", ""), 0), long_reach == "true")
    if any(pixels < 0 or pixels >> 66 for pixels, _ in result):
        raise ValueError("Basic seed exceeds its 6 by 11 cell")
    return result


def rust_char(literal: str) -> str:
    if literal.startswith(r"\u{") and literal.endswith("}"):
        value = chr(int(literal[3:-1].replace("_", ""), 16))
    else:
        value = ast.literal_eval("'" + literal + "'")
    if not isinstance(value, str) or len(value) != 1:
        raise ValueError("expected one Rust character")
    return value


def seed_include(seeds: list[tuple[int, bool]]) -> str:
    # Switch return values become immediate constants. An indexed const array
    # could introduce a compiler-owned constant buffer outside this Float3 ABI.
    cases = []
    for code, (pixels, _) in enumerate(seeds):
        if pixels:
            words = [(pixels >> (32 * index)) & 0xffffffff for index in range(3)]
            cases.append(f"case {code}u: return uvec3({words[0]}u,{words[1]}u,{words[2]}u);")
    flags = " || ".join(f"code == {code}u" for code, (_, flag) in enumerate(seeds) if flag) or "false"
    return ("// Generated from Basic data and raster overrides by bake.py.\n"
            "uvec3 basic_seed_mask(uint code) { switch(code) {\n" + "\n".join(cases)
            + "\ndefault: return uvec3(0u); } }\n"
            + f"bool basic_seed_long_reach(uint code) {{ return {flags}; }}\n")


def active_link_count(seed: tuple[int, bool]) -> int:
    """Full-pair graph diagnostic at bake time, independent of shader offsets."""
    pixels, long_reach = seed
    points = [(i % 6, i // 6) for i in range(66) if pixels & (1 << (65-i))]
    reach = 2.76 if long_reach else 1.76
    count = 0
    for index, a in enumerate(points):
        for b in points[index+1:]:
            length2 = sum((a[k]-b[k])**2 for k in (0, 1))
            if math.sqrt(length2) >= reach:
                continue
            if any(p != a and p != b and sum((2*p[k]-a[k]-b[k])**2 for k in (0, 1)) <= length2
                   for p in points):
                continue
            count += 1
    return count


def verify_recipe() -> None:
    """Reject a bake if the Rust reference recipe and GLSL literals drift."""
    shape = (ROOT / "vendor/microfont/src/fontshape.rs").read_text()
    raster = (ROOT / "vendor/microfont/src/raster.rs").read_text()
    shader = SOURCE.read_text()
    reference = [float(re.search(r"INFLUENCE_WIDTH:\s*f64\s*=\s*([\d.]+)",shape)[1])]
    reference += [float(re.search(rf"Self::{name}\s*=>\s*([\d.]+)",shape)[1]) for name in ("Meso","Lean")]
    reference += [float(re.search(rf"{name}_CONNECTION_REACH:\s*f64\s*=\s*([\d.]+)",raster)[1]) for name in ("LONG","SHORT")]
    baked = [float(re.search(r"SIGMA\s*=\s*([\d.]+)",shader)[1])]
    baked += [float(v) for v in re.search(r"float threshold\s*=\s*row == 0u\s*\?\s*([\d.]+)\s*:\s*([\d.]+)",shader).groups()]
    baked += [float(v) for v in re.search(r"float reach\s*=\s*basic_seed_long_reach\(code\)\s*\?\s*([\d.]+)\s*:\s*([\d.]+)",shader).groups()]
    if reference != baked:
        raise SystemExit(f"GLSL Dista recipe differs from Rust reference: GLSL={baked}, Rust={reference}")
    verify_blocker_recipe(shader)


def verify_blocker_recipe(shader: str) -> None:
    """Prove each fixed blocker set equals the loop's closed-disc predicate.

    Every occupancy subset is equivalent once its possible blocker coordinates
    are identical. Translation and the occupied() grid bounds cover every site,
    including perimeter sites; no CPU graph is emitted for the runtime path.
    """
    body = shader.split("bool blocked_fast(", 1)[1].split("\n}", 1)[0]
    offsets = [(1,0),(0,1),(1,1),(-1,1),(2,0),(0,2),(2,1),(-2,1),(1,2),(-1,2)]
    packed_x = int(re.search(r"0x([0-9a-f]+)u >> \(3u\*index\)", shader)[1], 16)
    packed_y = int(re.search(r"0x([0-9a-f]+)u >> \(2u\*index\)", shader)[1], 16)
    for index, delta in enumerate(offsets):
        decoded = (((packed_x >> (3*index)) & 7)-2, (packed_y >> (2*index)) & 3)
        if decoded != delta:
            raise SystemExit(f"GLSL candidate offset {index} drifted: {decoded} != {delta}")
        case = re.search(rf"case {index}u:(.*?);", body, re.DOTALL)[1]
        actual = {(int(x), int(y)) for x,y in re.findall(r"ivec2\((-?\d+),(-?\d+)\)", case)}
        length2 = delta[0]**2 + delta[1]**2
        expected = {(x,y) for y in range(-3,4) for x in range(-3,4)
                    if (x,y) not in ((0,0), delta)
                    and (2*x-delta[0])**2 + (2*y-delta[1])**2 <= length2}
        if actual != expected:
            raise SystemExit(f"GLSL exact blocker set {index} drifted: {actual} != {expected}")


def current_inputs():
    verify_recipe()
    seeds = basic_seeds()
    include = seed_include(seeds)
    return seeds, include, hashlib.sha256(SOURCE.read_bytes() + include.encode()).hexdigest()


def generator_fingerprint() -> str:
    digest = hashlib.sha256()
    for relative in ("tools/microfont-dista-bake/bake.py", "tools/helio-intel-bake/bake.py",
                     "crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c",
                     "crates/trueos-shader/xe_lp_shader_bake/simple_triangle.frag"):
        digest.update(relative.encode() + b"\0")
        digest.update((ROOT / relative).read_bytes())
    return digest.hexdigest()


def artifact_freshness() -> tuple[bool, str]:
    """Check contents, including the exact Rust bytes Cargo will embed.

    This path never loads the native compiler or runs external programs. A
    normal unchanged make therefore needs only the checked-in artifact.
    """
    try:
        seeds, include, source_sha = current_inputs()
        manifest = json.loads((OUT / "manifest.json").read_text())
        if manifest.get("source_sha256") != source_sha:
            return False, "source-or-seeds-changed"
        if manifest.get("generator_sha256") != generator_fingerprint():
            return False, "generator-changed"
        if manifest.get("device", {}).get("device_id") != 0x4680:
            return False, "native-device-mismatch"
        if manifest.get("scratch_bytes") != 0 or manifest.get("memory_sends") != []:
            return False, "native-contract-mismatch"
        native = (OUT / "dista_stamp.vs.simd8.bin").read_bytes()
        if len(native) != manifest.get("vs_bytes") or hashlib.sha256(native).hexdigest() != manifest.get("vs_sha256"):
            return False, "native-binary-mismatch"
        rust_bytes = GENERATED.read_bytes()
        if hashlib.sha256(rust_bytes).hexdigest() != manifest.get("generated_rust_sha256"):
            return False, "generated-rust-mismatch"
        rust = rust_bytes.decode()
        if re.search(r'SOURCE_SHA256: &str = "([a-f0-9]+)"', rust)[1] != source_sha:
            return False, "generated-source-mismatch"
        table = rust.split("pub(crate) const SEEDS:", 1)[1].split("];", 1)[0]
        baked_seeds = [(int(bits.replace("_", ""), 16), flag == "true") for bits, flag in
                       re.findall(r"\(0x([a-fA-F0-9_]+),\s*(true|false)\)", table)]
        if baked_seeds != seeds:
            return False, "generated-seeds-mismatch"
        counts = rust.split("pub(crate) const ACTIVE_LINK_COUNTS:", 1)[1].split("= [", 1)[1].split("];", 1)[0]
        if [int(value) for value in re.findall(r"\d+", counts)] != [active_link_count(seed) for seed in seeds]:
            return False, "generated-links-mismatch"
        words = rust.split("static DISTA_VS_CODE:", 1)[1].split("= [", 1)[1].split("];", 1)[0]
        embedded = b"".join(struct.pack("<I", int(word, 16)) for word in re.findall(r"0x([a-fA-F0-9]+)", words))
        if embedded != native:
            return False, "embedded-native-mismatch"
        if (OUT / "basic_seeds.glsl").read_text() != include:
            return False, "generated-include-mismatch"
    except (OSError, ValueError, KeyError, IndexError, TypeError, AttributeError, struct.error) as error:
        return False, f"invalid-artifact: {error}"
    except SystemExit as error:
        return False, f"recipe-mismatch: {error}"
    return True, "current"


def resolve_mesa_build(override: Path | None = None) -> Path:
    configured = override or os.environ.get("MICROFONT_DISTA_MESA_BUILD")
    candidates = [Path(configured)] if configured else [
        DEFAULT_MESA, ROOT / ".codex_tmp/trueos-adj-instrumented-rpls/mesa-build"]
    for candidate in candidates:
        if all((candidate / path).is_file() for path in
               ("src/intel/tools/libintel_noop_drm_shim.so", "src/intel/vulkan/libvulkan_intel.so")):
            return candidate.resolve()
    raise SystemExit("Dista shader needs a refresh, but its native compiler is unavailable. "
                     "Set MICROFONT_DISTA_MESA_BUILD to the compatible instrumented Mesa build; "
                     "the ISO build has stopped before compiling a stale atlas.")


def ensure_current(mesa_build: Path | None = None, work_dir: Path | None = None):
    # Invalid source data or a changed reconstruction recipe needs an explicit
    # source fix; invoking the native compiler cannot make it valid.
    current_inputs()
    current, reason = artifact_freshness()
    if current:
        print(json.dumps({"artifact": "microfont-dista", "status": "current", "native_baked": False}))
        return
    print(f"microfont-dista: refreshing shader ({reason})", file=sys.stderr)
    bake_native(mesa_build, work_dir)
    current, reason = artifact_freshness()
    if not current:
        raise SystemExit(f"Dista artifact still stale after bake ({reason}); ISO build stopped")


def load_baker():
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("trueos_helio_baker", ROOT / "tools/helio-intel-bake/bake.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def decode(binary: Path) -> str:
    result = subprocess.run(["iga64", "-d", "-p=12p1", "-Xprint-pc", str(binary)],
                            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode or "illegal" in result.stdout.lower():
        raise SystemExit(f"invalid ADL-S EU ISA:\n{result.stdout}")
    return "\n".join(line.rstrip() for line in result.stdout.splitlines()) + "\n"


def emit_rust(vs: bytes, state: dict[str, int], seeds: list[tuple[int, bool]], source_sha: str):
    words = struct.unpack(f"<{len(vs)//4}I", vs)
    rows = ["    " + ", ".join(f"0x{word:08x}" for word in words[i:i+8]) + ","
            for i in range(0, len(words), 8)]
    text = """// @generated by tools/microfont-dista-bake/bake.py; ADL-S 8086:4680.
// Compile-only native artifact; shader constructs every body position on GPU.
use super::{DispatchMode, ShaderKernelMetadata, TrianglePipeline, TriangleVertexShader, TriangleVertexShaderMetadata};
pub(crate) const COMPILED_DEVICE_ID: u16 = 0x4680;
"""
    text += f'pub(crate) const SOURCE_SHA256: &str = "{source_sha}";\n'
    text += "pub(crate) const SEEDS: [(u128, bool); 128] = [\n"
    text += "".join(f"    (0x{pixels:x}, {str(flag).lower()}),\n" for pixels, flag in seeds) + "];\n"
    text += "// Exact accepted graph edges; diagnostic only, never a runtime link list.\n"
    text += "pub(crate) const ACTIVE_LINK_COUNTS: [u8; 128] = [\n"
    counts = [active_link_count(seed) for seed in seeds]
    text += "".join("    " + ", ".join(str(n) for n in counts[i:i+16]) + ",\n" for i in range(0, 128, 16)) + "];\n"
    text += f"static DISTA_VS_CODE: [u32; {len(words)}] = [\n" + "\n".join(rows) + "\n];\n"
    text += f"""
static PIPELINE: spin::Lazy<TrianglePipeline> = spin::Lazy::new(|| {{
    // Preserve the existing SIMD16 PS code slice identity: the renderer's
    // constant-color specialization recognizes and copies this exact program.
    let mut pipeline = *super::triangle_pipeline_simd16();
    pipeline.vs = TriangleVertexShader {{
        code: &DISTA_VS_CODE,
        meta: TriangleVertexShaderMetadata {{
            kernel: ShaderKernelMetadata {{
                code_offset_bytes: 0, code_size_bytes: {len(vs)},
                code_alignment_bytes: 64, ksp_offset_bytes: 0,
                dispatch_mode: DispatchMode::Simd8,
                grf_start_register: {state['dispatch_grf_start']}, grf_used: 128,
                push_constant_bytes: 0, binding_table_entry_count: 0, sampler_count: 0,
            }},
            max_threads: {state['max_threads']},
            urb_entry_output_length: {state['urb_entry_64b']},
        }},
    }};
    pipeline.ps.meta.kernel.code_offset_bytes = {(len(vs)+63)&~63};
    pipeline
}});
pub(crate) fn microfont_dista_pipeline() -> &'static TrianglePipeline {{ &PIPELINE }}
"""
    GENERATED.write_text(text)


def bake_native(mesa_build: Path | None = None, work_dir: Path | None = None, source_only: bool = False):
    seeds, include, source_sha = current_inputs()
    generator_sha = generator_fingerprint()
    # Resolve the compiler before doing work, except for explicit source-only
    # validation. Nothing is loaded on ensure-current's unchanged fast path.
    mesa = None if source_only else resolve_mesa_build(mesa_build)
    work = (work_dir or ROOT / "bld/microfont-dista-bake").resolve()
    work.mkdir(parents=True, exist_ok=True)
    (work / "basic_seeds.glsl").write_text(include)
    spirv = work / "dista_stamp.vert.spv"
    subprocess.run(["glslc", "--target-env=vulkan1.1", "-O", "-I", str(work), str(SOURCE), "-o", str(spirv)], check=True)
    if shutil.which("spirv-val"):
        subprocess.run(["spirv-val", "--target-env", "vulkan1.1", str(spirv)], check=True)
    if source_only:
        print(json.dumps({"source_sha256": source_sha, "seeds": len(seeds), "spirv_bytes": spirv.stat().st_size,
                          "native_baked": False, "gpu_submitted": False}))
        return

    shim = mesa / "src/intel/tools/libintel_noop_drm_shim.so"
    driver = mesa / "src/intel/vulkan/libvulkan_intel.so"
    if not shim.is_file() or not driver.is_file():
        raise SystemExit("--mesa-build must contain instrumented ANV and libintel_noop_drm_shim.so")
    baker = load_baker()
    fragment = work / "constant.frag.spv"
    subprocess.run(["glslc", str(ROOT / "crates/trueos-shader/xe_lp_shader_bake/simple_triangle.frag"), "-o", str(fragment)], check=True)
    dumper_source = work / "pipeline_dump.c"
    source = (ROOT / "crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c").read_text()
    source = baker.replace_once(source, "    dump_pipeline_executables(device, pipeline);\n\n    const float ordinary_vertices[9] = {",
                                '    dump_pipeline_executables(device, pipeline);\n    printf("microfont_dista: compiled_only=1\\n");\n    return 0;\n\n    const float ordinary_vertices[9] = {')
    dumper_source.write_text(source)
    dumper = work / "pipeline_dump"
    subprocess.run(["cc", str(dumper_source), "-o", str(dumper), *baker.vulkan_compile_flags()], check=True)
    icd = work / "adls-icd.json"
    icd.write_text(json.dumps({"file_format_version": "1.0.1", "ICD": {"api_version": "1.4.346", "library_path": str(driver)}}))
    executable_dir = work / "intel"
    executable_dir.mkdir(exist_ok=True)
    for path in executable_dir.iterdir():
        if path.is_file():
            path.unlink()
    env = os.environ.copy()
    env.update({"LD_PRELOAD": str(shim), "VK_DRIVER_FILES": str(icd), "VK_ICD_FILENAMES": str(icd),
                "INTEL_STUB_GPU_DEVICE_ID": "4680", "TRUEOS_VK_DEVICE_ID": "0x4680",
                "MESA_SHADER_CACHE_DISABLE": "true", "TRUEOS_EXECUTABLE_DUMP_DIR": str(executable_dir)})
    log = work / "compile.log"
    baker.run([str(dumper), str(spirv), str(fragment)], env=env, log=log)
    device, executables = baker.parse_compile_log(log.read_text())
    if device["device_id"] != 0x4680 or "microfont_dista: compiled_only=1" not in log.read_text():
        raise SystemExit("missing target-matched compile-only proof")
    vs, _, _ = baker.extract_native(executable_dir, work / "native")
    state_path = executable_dir / "vertex_TRUEOS_VS_state_v1.txt"
    state = {key: int(value, 0) for key, value in re.findall(r"(\w+)=(0x[0-9a-fA-F]+|\d+)", state_path.read_text())}
    expected = dict(ver=12, verx10=120, dispatch_grf_start=2, urb_read_length=1, urb_entry_64b=1,
                    vue_slots=2, position_slot=1, vf_packing0=7, uses_vertexid=0, uses_instanceid=0,
                    binding_table_entries=0, sampler_count=0, scratch_bytes=0)
    if any(state.get(key) != value for key, value in expected.items()):
        raise SystemExit(f"Dista VS Float3 payload contract drift: {state}")
    binary = work / "dista_stamp.vs.simd8.bin"
    binary.write_bytes(vs)
    isa = decode(binary)
    sends = re.findall(r"send\w*\.(\w+)\s", isa)
    if not sends or set(sends) != {"urb"}:
        raise SystemExit(f"Dista VS requires unsupported memory/scratch sends: {sends}")
    assembly, = executable_dir.glob("*_vertex_*_GEN_Assembly.txt")
    if len(vs) != baker.assembly_code_size(assembly):
        raise SystemExit("VS extraction size differs from compiler assembly")
    if not re.search(r"send\.urb.*\{[^}\n]*EOT", isa):
        raise SystemExit("VS lacks complete URB EOT")
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / binary.name).write_bytes(vs)
    (OUT / "dista_stamp.vs.iga.txt").write_text(isa)
    shutil.copy2(assembly, OUT / "dista_stamp.vs.mesa.txt")
    shutil.copy2(state_path, OUT / state_path.name)
    shutil.copy2(log, OUT / log.name)
    (OUT / "basic_seeds.glsl").write_text(include)
    manifest = {"schema": 1, "contract": "microfont-dista-float3-procedural", "device": device,
                "backend": "mesa-anv-noop-drm-shim-compile-only", "gpu_submitted": False,
                "baremetal_verified": False, "source_sha256": source_sha, "generator_sha256": generator_sha,
                "vs_sha256": hashlib.sha256(vs).hexdigest(), "vs_bytes": len(vs),
                "vertex_compiler_state": state, "memory_sends": [], "scratch_bytes": 0,
                "scratch_evidence": "complete independently decoded VS contains only URB sends",
                "executables": list(executables.values()), "ps_reused": "triangle_pipeline_simd16"}
    emit_rust(vs, state, seeds, source_sha)
    manifest["generated_rust_sha256"] = hashlib.sha256(GENERATED.read_bytes()).hexdigest()
    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=2)+"\n")
    print(json.dumps({"vs_bytes": len(vs), "source_sha256": source_sha, "native_baked": True,
                      "gpu_submitted": False, "generated": str(GENERATED)}))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mesa-build", type=Path)
    parser.add_argument("--work-dir", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--source-only", action="store_true", help="validate SPIR-V without native bake")
    mode.add_argument("--ensure-current", action="store_true", help="rebake changed inputs before the kernel build")
    mode.add_argument("--check-current", action="store_true", help="check contents without loading compiler tools")
    args = parser.parse_args()
    if args.check_current:
        current, reason = artifact_freshness()
        print(json.dumps({"artifact": "microfont-dista", "current": current, "reason": reason}))
        raise SystemExit(0 if current else 1)
    if args.ensure_current:
        ensure_current(args.mesa_build, args.work_dir)
    else:
        bake_native(args.mesa_build, args.work_dir, args.source_only)


if __name__ == "__main__":
    main()
