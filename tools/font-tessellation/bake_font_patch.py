#!/usr/bin/env python3
"""Build and import the font-specific HS/DS wedge probe artifact.

Source-only mode is deterministic and needs only Python. Import mode consumes
an already instrumented Mesa capture; it never submits GPU work and therefore
must not be reported as a rendering proof.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import struct


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_FIXTURE = Path(__file__).with_name("fixtures") / "hand-authored-wedges.json"
SCHEMA = "trueos.font-tessellation-fixture/v1"
CONTRACT_VERSION = 1
PATCH_STRUCT = struct.Struct("<10f6I")
FLAG_BITS = {"begin_contour": 1, "end_contour": 2, "reversed": 4}
CAPTURES = (
    "vertex_TRUEOS_VS_state_v1.txt",
    "fragment_TRUEOS_PS_state_v1.txt",
    "tess_control_TRUEOS_HS_state_v1.txt",
    "tess_eval_TRUEOS_DS_state_v1.txt",
    "tessellation_TRUEOS_URB_state_v1.txt",
    "tessellation_TRUEOS_TE_state_v1.txt",
    "tessellation_TRUEOS_TE_packet_v1.txt",
)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def load_fixture(path: Path) -> dict:
    fixture = json.loads(path.read_text())
    if fixture.get("schema") != SCHEMA:
        raise ValueError("unsupported fixture schema")
    target = fixture.get("target")
    if not isinstance(target, dict) or target.get("width") != 64 or target.get("height") != 64:
        raise ValueError("the v1 probe target must be exactly 64x64")
    patches = fixture.get("patches")
    if not isinstance(patches, list) or not 1 <= len(patches) <= 4096:
        raise ValueError("fixture patch count outside the frozen M0 limit")
    last_contour = -1
    previous_ended = True
    for index, patch in enumerate(patches):
        if set(patch) != {"p0", "p1", "p2", "p3", "anchor", "contour_index", "flags"}:
            raise ValueError(f"patch {index}: fields do not match FontCurvePatchV1")
        for name in ("p0", "p1", "p2", "p3", "anchor"):
            point = patch[name]
            if not isinstance(point, list) or len(point) != 2:
                raise ValueError(f"patch {index}: {name} must contain two coordinates")
            if any(not isinstance(value, (int, float)) or not math.isfinite(value) for value in point):
                raise ValueError(f"patch {index}: {name} contains a non-finite coordinate")
        contour = patch["contour_index"]
        if not isinstance(contour, int) or not 0 <= contour < 127:
            raise ValueError(f"patch {index}: contour ordering or winding bound rejected")
        flags = patch["flags"]
        if not isinstance(flags, list) or len(set(flags)) != len(flags) or set(flags) - FLAG_BITS.keys():
            raise ValueError(f"patch {index}: unknown or duplicate flags")
        flag_set = set(flags)
        if contour != last_contour:
            if contour != last_contour + 1 or not previous_ended or "begin_contour" not in flag_set:
                raise ValueError(f"patch {index}: invalid contour begin/order")
        elif previous_ended or "begin_contour" in flag_set:
            raise ValueError(f"patch {index}: invalid repeated contour begin")
        previous_ended = "end_contour" in flag_set
        last_contour = contour
    if not previous_ended:
        raise ValueError("final contour is not closed")
    return fixture


def encode_patches(fixture: dict) -> bytes:
    encoded = bytearray()
    for patch in fixture["patches"]:
        coords = [float(value) for name in ("p0", "p1", "p2", "p3", "anchor") for value in patch[name]]
        flags = sum(FLAG_BITS[name] for name in patch["flags"])
        encoded += PATCH_STRUCT.pack(*coords, patch["contour_index"], flags, 0, 0, 0, 0)
    return bytes(encoded)


def shader_sources() -> dict[str, str]:
    vertex = """#version 450
layout(location=0) in vec2 inP0;
layout(location=1) in vec2 inP1;
layout(location=2) in vec2 inP2;
layout(location=3) in vec2 inP3;
layout(location=4) in vec2 inAnchor;
layout(location=0) out vec2 p1;
layout(location=1) out vec2 p2;
layout(location=2) out vec2 p3;
layout(location=3) out vec2 anchor;
void main() {
    gl_Position = vec4(inP0, 0.0, 1.0);
    p1 = inP1; p2 = inP2; p3 = inP3; anchor = inAnchor;
}
"""
    control = """#version 450
layout(vertices=5) out;
layout(location=0) in vec2 p1[];
layout(location=1) in vec2 p2[];
layout(location=2) in vec2 p3[];
layout(location=3) in vec2 anchor[];
layout(location=0) out vec2 patchPoint[];
float cubicFactor(vec2 a, vec2 b, vec2 c, vec2 d) {
    vec2 d0 = a - 2.0*b + c;
    vec2 d1 = b - 2.0*c + d;
    float curvature = max(length(d0), length(d1));
    return max(ceil(sqrt(0.75 * curvature / 0.125)), 1.0);
}
void main() {
    vec2 point = gl_in[0].gl_Position.xy;
    if (gl_InvocationID == 1) point = p1[0];
    if (gl_InvocationID == 2) point = p2[0];
    if (gl_InvocationID == 3) point = p3[0];
    if (gl_InvocationID == 4) point = anchor[0];
    patchPoint[gl_InvocationID] = point;
    gl_out[gl_InvocationID].gl_Position = vec4(point, 0.0, 1.0);
    barrier();
    if (gl_InvocationID == 0) {
        float segments = cubicFactor(patchPoint[0], patchPoint[1], patchPoint[2], patchPoint[3]);
        gl_TessLevelOuter[0] = 1.0;
        gl_TessLevelOuter[1] = segments;
        gl_TessLevelOuter[2] = 1.0;
        gl_TessLevelOuter[3] = segments;
        gl_TessLevelInner[0] = segments;
        gl_TessLevelInner[1] = 1.0;
    }
}
"""
    evaluation = """#version 450
layout(quads, equal_spacing, ccw) in;
layout(location=0) in vec2 patchPoint[];
vec2 cubic(float u) {
    float t = 1.0-u;
    return t*t*t*patchPoint[0] + 3.0*t*t*u*patchPoint[1]
         + 3.0*t*u*u*patchPoint[2] + u*u*u*patchPoint[3];
}
void main() {
    vec2 pixel = mix(patchPoint[4], cubic(gl_TessCoord.x), gl_TessCoord.y);
    vec2 ndc = vec2(pixel.x / 32.0 - 1.0, 1.0 - pixel.y / 32.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"""
    fragment = """#version 450
layout(location=0) out vec4 color;
void main() { color = vec4(1.0); }
"""
    return {"vert": vertex, "tesc": control, "tese": evaluation, "frag": fragment}


def write_sources(fixture_path: Path, out: Path) -> dict:
    fixture = load_fixture(fixture_path)
    out.mkdir(parents=True, exist_ok=True)
    packed = encode_patches(fixture)
    if len(packed) != len(fixture["patches"]) * 64:
        raise AssertionError("FontCurvePatchV1 packing drift")
    (out / "patches.fcp1").write_bytes(packed)
    sources = shader_sources()
    source_hashes = {}
    for stage, source in sources.items():
        encoded = source.encode()
        (out / f"font_patch.{stage}").write_bytes(encoded)
        source_hashes[stage] = digest(encoded)
    canonical_fixture = canonical_json(fixture)
    source_bundle = b"".join(sources[stage].encode() for stage in ("vert", "tesc", "tese", "frag"))
    manifest = {
        "schema": "trueos.font-tessellation-bake/v1",
        "contract_version": CONTRACT_VERSION,
        "fixture_sha256": digest(canonical_fixture),
        "patch_bytes_sha256": digest(packed),
        "patch_count": len(fixture["patches"]),
        "patch_stride_bytes": 64,
        "target": fixture["target"],
        "source_sha256": source_hashes,
        "source_bundle_sha256": digest(source_bundle),
        "compiled_device_id": None,
        "native_capture": False,
        "gpu_submitted": False,
        "runtime_integrated": False,
        "baremetal_verified": False,
    }
    (out / "manifest.json").write_bytes(canonical_json(manifest))
    return manifest


def fields(path: Path) -> dict[str, int]:
    return {key: int(value, 0) for key, value in re.findall(r"(\w+)=(0x[0-9a-fA-F]+|\d+)", path.read_text())}


def packet(path: Path, name: str, count: int) -> list[int]:
    lines = path.read_text().splitlines()
    matching = [line for line in lines if line.startswith(name + " ")]
    if len(matching) != 1:
        raise ValueError(f"{path}: expected one {name} packet")
    words = [int(word, 16) for word in matching[0].split()[1:]]
    if len(words) != count:
        raise ValueError(f"{path}: expected {count} {name} dwords")
    return words


def serialized_code(capture: Path, stage: str, stage_id: int) -> list[int]:
    matches = sorted(capture.glob(f"*_{stage}_*_shader_serialize.bin"))
    if len(matches) != 1:
        raise ValueError(f"expected one serialized {stage} shader")
    blob = matches[0].read_bytes()
    if len(blob) < 52:
        raise ValueError(f"truncated serialized {stage} shader")
    actual, size = struct.unpack_from("<II", blob)
    if actual != stage_id or size == 0 or size % 4 or 8 + size + 44 > len(blob):
        raise ValueError(f"invalid serialized {stage} shader")
    program = struct.unpack_from("<11I", blob, 8 + size)
    if program[0] != stage_id or program[7] != size or any(program[1:7]) or any(program[8:11]):
        raise ValueError(f"{stage} uses unsupported push, scratch, constants, or relocations")
    return list(struct.unpack_from(f"<{size // 4}I", blob, 8))


def rust_words(name: str, words: list[int]) -> str:
    body = "".join("    " + ", ".join(f"0x{word:08x}" for word in words[i:i + 8]) + ",\n" for i in range(0, len(words), 8))
    return f"pub(crate) static {name}: [u32; {len(words)}] = [\n{body}];\n"


def import_capture(out: Path, capture: Path, generated_rs: Path, device_id: str) -> dict:
    manifest_path = out / "manifest.json"
    if not manifest_path.is_file():
        raise ValueError("source-only output must be generated before capture import")
    manifest = json.loads(manifest_path.read_text())
    missing = [name for name in CAPTURES if not (capture / name).is_file()]
    if missing:
        raise ValueError("missing capture files: " + ", ".join(missing))
    if not re.fullmatch(r"0x[0-9a-fA-F]{4}", device_id):
        raise ValueError("device id must be a four-digit PCI id")
    capture_manifest_path = capture / "manifest.json"
    if not capture_manifest_path.is_file():
        raise ValueError("missing capture manifest")
    capture_manifest = json.loads(capture_manifest_path.read_text())
    expected_identity = {
        "schema": "trueos.font-tessellation-capture/v1",
        "contract_version": CONTRACT_VERSION,
        "compiled_device_id": device_id.lower(),
        "fixture_sha256": manifest["fixture_sha256"],
        "patch_bytes_sha256": manifest["patch_bytes_sha256"],
        "source_sha256": manifest["source_sha256"],
        "source_bundle_sha256": manifest["source_bundle_sha256"],
        "commands_recorded": True,
        "gpu_submitted": False,
    }
    if any(capture_manifest.get(key) != value for key, value in expected_identity.items()):
        raise ValueError("capture manifest does not match the source-only bake")
    sealed_files = capture_manifest.get("files_sha256")
    if not isinstance(sealed_files, dict):
        raise ValueError("capture manifest is missing sealed file hashes")
    required_seals = set(CAPTURES)
    for stage in ("vert", "tesc", "tese", "frag"):
        required_seals.add(f"font_patch.{stage}.spv")
    for stage in ("vertex", "tess_control", "tess_eval", "fragment"):
        matches = sorted(capture.glob(f"*_{stage}_*_shader_serialize.bin"))
        if len(matches) != 1:
            raise ValueError(f"expected one serialized {stage} shader")
        required_seals.add(matches[0].name)
    if not required_seals.issubset(sealed_files):
        raise ValueError("capture manifest does not seal every required artifact")
    for name in required_seals:
        if Path(name).name != name or not re.fullmatch(r"[0-9a-f]{64}", sealed_files[name]):
            raise ValueError("invalid capture file seal")
        path = capture / name
        if not path.is_file() or digest(path.read_bytes()) != sealed_files[name]:
            raise ValueError(f"capture file hash mismatch: {name}")
    for stage, kind in (("tess_control", "HS"), ("tess_eval", "DS")):
        meta = fields(capture / f"{stage}_TRUEOS_{kind}_state_v1.txt")
        if meta.get("verx10") != 120 or meta.get("scratch_bytes") != 0:
            raise ValueError(f"unsupported {stage} device generation or scratch")
    if fields(capture / "tess_control_TRUEOS_HS_state_v1.txt").get("output_control_points") != 5:
        raise ValueError("hull shader did not emit five wedge control points")
    te = fields(capture / "tessellation_TRUEOS_TE_state_v1.txt")
    if te.get("domain") != 0 or te.get("triangle_domain") != 0 or te.get("partitioning") != 0:
        raise ValueError("capture is not the required equal-spacing quad domain")
    urb = fields(capture / "tessellation_TRUEOS_URB_state_v1.txt")
    if urb.get("deref_block_size") != 0:
        raise ValueError("unsupported URB dereference block")
    urb_rows = [(urb[f"stage{i}_size"], urb[f"stage{i}_start"], urb[f"stage{i}_entries"]) for i in range(4)]
    codes = {
        "VERTEX": serialized_code(capture, "vertex", 0),
        "TESS_CONTROL": serialized_code(capture, "tess_control", 1),
        "TESS_EVAL": serialized_code(capture, "tess_eval", 2),
        "FRAGMENT": serialized_code(capture, "fragment", 4),
    }
    hs = packet(capture / "tess_control_TRUEOS_HS_state_v1.txt", "3DSTATE_HS", 9)
    ds = packet(capture / "tess_eval_TRUEOS_DS_state_v1.txt", "3DSTATE_DS", 11)
    te_packet = packet(capture / "tessellation_TRUEOS_TE_packet_v1.txt", "3DSTATE_TE", 5)
    if hs[0] != 0x781B0007 or te_packet[0] != 0x781C0003 or ds[0] != 0x781D0009:
        raise ValueError("capture contains the wrong tessellation stage packets")
    hs[3] = 0
    ds[1] = 0
    vs = fields(capture / "vertex_TRUEOS_VS_state_v1.txt")
    ps = fields(capture / "fragment_TRUEOS_PS_state_v1.txt")
    generated = "// Generated by tools/font-tessellation/bake_font_patch.py; do not hand edit.\nuse super::*;\n"
    generated += f"pub(crate) const CONTRACT_VERSION: u32 = {CONTRACT_VERSION};\n"
    generated += "pub(crate) const AVAILABLE: bool = true;\n"
    generated += f"pub(crate) const COMPILED_DEVICE_ID: u16 = {device_id};\n"
    generated += f"pub(crate) const SOURCE_SHA256: &str = \"{manifest['source_bundle_sha256']}\";\n"
    for name, words in codes.items():
        generated += rust_words(name, words)
    generated += f"pub(crate) const HS_PACKET: [u32; 9] = {hs!r};\n"
    generated += f"pub(crate) const DS_PACKET: [u32; 11] = {ds!r};\n"
    generated += f"pub(crate) const TE_PACKET: [u32; 5] = {te_packet!r};\n"
    generated += f"pub(crate) const URB: [(u32,u32,u32); 4] = {urb_rows!r};\n"
    generated += """
const fn kernel(offset: u32, size: u32, grf: u8, bindings: u8) -> ShaderKernelMetadata {
    ShaderKernelMetadata { code_offset_bytes: offset, code_size_bytes: size,
        code_alignment_bytes: 64, ksp_offset_bytes: 0, dispatch_mode: DispatchMode::Simd8,
        grf_start_register: grf, grf_used: 128, push_constant_bytes: 0,
        binding_table_entry_count: bindings, sampler_count: 0 }
}
"""
    ps_offset = (len(codes["VERTEX"]) * 4 + 63) & ~63
    generated += f"""pub(crate) static PIPELINE: TrianglePipeline = TrianglePipeline {{
    vs: TriangleVertexShader {{ code: &VERTEX, meta: TriangleVertexShaderMetadata {{
        kernel: kernel(0, VERTEX.len() as u32 * 4, {vs['dispatch_grf_start']}, {vs.get('binding_table_entries', 0)}),
        max_threads: {vs['max_threads']}, urb_entry_output_length: {vs['urb_entry_64b']} }} }},
    ps: TrianglePixelShader {{ code: &FRAGMENT, meta: TrianglePixelShaderMetadata {{
        kernel: kernel({ps_offset}, FRAGMENT.len() as u32 * 4, {ps['grf_start8']}, {ps.get('binding_table_entries', 0)}),
        num_varying_inputs: {ps.get('num_varying_inputs', 0)}, uses_vmask: true,
        computed_stencil: false, persample_dispatch: false, computed_depth_mode: 0,
        flat_inputs: {ps.get('flat_inputs', 0)} }} }},
}};
pub(crate) fn matches(pipeline: &TrianglePipeline) -> bool {{
    core::ptr::eq(pipeline.vs.code.as_ptr(), VERTEX.as_ptr())
        && core::ptr::eq(pipeline.ps.code.as_ptr(), FRAGMENT.as_ptr())
}}
"""
    generated_rs.parent.mkdir(parents=True, exist_ok=True)
    generated_rs.write_text(generated)
    manifest.update({
        "compiled_device_id": device_id.lower(),
        "native_capture": True,
        "capture_sha256": {name: digest((capture / name).read_bytes()) for name in CAPTURES},
        "gpu_submitted": False,
    })
    manifest_path.write_bytes(canonical_json(manifest))
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, default=DEFAULT_FIXTURE)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--source-only", action="store_true")
    parser.add_argument("--capture-dir", type=Path)
    parser.add_argument("--generated-rs", type=Path)
    parser.add_argument("--device-id", default="0xa780")
    args = parser.parse_args()
    if args.source_only and (args.capture_dir or args.generated_rs):
        parser.error("--source-only cannot import a capture")
    if bool(args.capture_dir) != bool(args.generated_rs):
        parser.error("--capture-dir and --generated-rs must be provided together")
    manifest = write_sources(args.fixture, args.out)
    if args.capture_dir:
        manifest = import_capture(args.out, args.capture_dir, args.generated_rs, args.device_id)
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
