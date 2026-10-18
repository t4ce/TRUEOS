#!/usr/bin/env python3
"""Execute the production GLSL in Lavapipe and compare its stamp with an analytic oracle.

This is shader image validation on a software Vulkan implementation. It neither
executes nor measures the checked-in Intel native executable.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def glyphs(segments: tuple[int, int], atlas_shape: int | None = None):
    if atlas_shape is not None:
        for slot, code in enumerate(range(0x20, 0x7f)):
            yield atlas_shape, slot % 16, slot // 16, chr(code), segments[atlas_shape]
    else:
        for shape, count in enumerate(segments):
            for column, character in enumerate("ABC123/Yy%"):
                yield shape, column, shape, character, count


def scene(seeds, segments: tuple[int, int], atlas_shape: int | None = None,
          direct: bool = False) -> bytes:
    # Same generic fan/quad task stream and clockwise-source winding as
    # src/r/services/microfont_dista_tasks.rs. No positions are computed here.
    descriptors, indices = [], []
    for shape, column, row, character, count in glyphs(segments, atlas_shape):
        mask, long_reach = seeds[ord(character)]
        stamp = ord(character) | column << 7 | shape << 11
        if atlas_shape is not None:
            stamp |= row << 12 | 1 << 15
        if direct:
            stamp |= 1 << 16
        for site in range(66):
            if not mask & (1 << (65-site)):
                continue
            base = len(descriptors)
            descriptors.extend((site*(count+1)+corner, stamp, count) for corner in range(count+1))
            for ring in range(count):
                indices.extend((base, base+1+(ring+1)%count, base+1+ring))
            for offset in range(10 if long_reach else 4):
                base = len(descriptors)
                token = 66*(count+1)+(site*10+offset)*4
                descriptors.extend((token+corner, stamp, count) for corner in range(4))
                indices.extend((base,base+2,base+1,base,base+3,base+2))
    # Deindex for the existing host dumper's vkCmdDraw entry point. This does
    # not alter the vertex descriptors, shader geometry, or triangle winding.
    return b"".join(struct.pack("<3f", *descriptors[i]) for i in indices)


def render_source(baker, destination: Path, width: int = 960, height: int = 352):
    source = (ROOT / "crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c").read_text()
    source = baker.replace_once(source, "if (props.vendorID != 0x8086)", "if (0)")
    start = source.index("        if (!physical_device_has_extension(")
    end = source.index("        uint32_t queue_count", start)
    source = source[:start]+source[end:]
    start = source.index("        .pNext = &(VkPhysicalDevicePipelineExecutablePropertiesFeaturesKHR)")
    end = source.index("        .queueCreateInfoCount", start)
    source = source[:start]+"        .pNext = NULL,\n"+source[end:]
    source = baker.replace_once(source, ".enabledExtensionCount = 1,", ".enabledExtensionCount = 0,")
    source = baker.replace_once(source, ".ppEnabledExtensionNames = device_extensions,", ".ppEnabledExtensionNames = NULL,")
    source = baker.replace_once(source, ".flags = VK_PIPELINE_CREATE_CAPTURE_STATISTICS_BIT_KHR |\n                 VK_PIPELINE_CREATE_CAPTURE_INTERNAL_REPRESENTATIONS_BIT_KHR,", ".flags = 0,")
    source = baker.replace_once(source, "    dump_pipeline_cache_blob(device, pipeline_cache);\n    dump_pipeline_executables(device, pipeline);", "")
    source = source.replace("{ 64, 64, 1 }", f"{{ {width}, {height}, 1 }}")
    source = source.replace("{ 64, 64 }", f"{{ {width}, {height} }}")
    source = source.replace(".width = 64,", f".width = {width},").replace(".height = 64,", f".height = {height},")
    source = source.replace(".width = 64.0f,", f".width = {width}.0f,").replace(".height = 64.0f,", f".height = {height}.0f,")
    source = source.replace("64u * 64u * 4u", f"{width}u * {height}u * 4u")
    # Back-face culling verifies the exact submitted winding too. Vulkan's
    # positive-height viewport maps this GL-style clip-space stamp upside down;
    # the readback is flipped below, without modifying the production shader.
    source = source.replace(".cullMode = VK_CULL_MODE_NONE,", ".cullMode = VK_CULL_MODE_BACK_BIT,")
    source = source.replace(".frontFace = VK_FRONT_FACE_COUNTER_CLOCKWISE,", ".frontFace = VK_FRONT_FACE_CLOCKWISE,")
    start = source.index("    const float ordinary_vertices[9] = {")
    end = source.index("    const VkBufferCreateInfo buffer_info", start)
    source = source[:start]+'''    FileData scene = read_spirv(getenv("DISTA_HOST_SCENE"));
    const float *vertices = (const float *)scene.words;
    const size_t vertex_bytes = scene.word_count * 4;
'''+source[end:]
    source = baker.replace_once(source, "vkCmdDraw(command_buffer, line_adjacency ? 4u : (geometry_enabled ? 6u : 3u), 1, 0, 0);",
                                "vkCmdDraw(command_buffer, vertex_bytes / 12, 1, 0, 0);")
    start = source.index("    const uint32_t center = pixels[32 * 64 + 32];")
    end = source.index("    vkDestroyBuffer(device, vertex_buffer, NULL);", start)
    source = source[:start]+'''    FILE *image_file = fopen(getenv("DISTA_HOST_RGBA"), "wb");
    if (!image_file || fwrite(pixels, 1, readback_size, image_file) != readback_size) return 2;
    fclose(image_file);
    vkUnmapMemory(device, readback_memory);
'''+source[end:]
    destination.write_text(source)


def compare(pixels: bytes, audit, segments: tuple[int, int], atlas_shape: int | None = None,
            oracle_step: int = 1) -> dict:
    seeds, parameters = audit.load_seeds(), audit.load_parameters()
    extras, missing, expected_count, actual_count, chords = 0, 0, 0, 0, 0
    per_glyph = []
    width, height = (1536, 1056) if atlas_shape is not None else (960, 352)
    for shape, column, row, character, count in glyphs(segments, atlas_shape):
        name = ("Meso", "Lean")[shape]
        radius = audit.radius(parameters, parameters.thresholds[name])
        error = radius*(1-math.cos(math.pi/count))
        seed = seeds[character]
        sites = [(x+.5,y+.5) for x,y in seed.sites()]
        links = audit.canonical_links(seed, parameters)
        glyph_pixels = 0
        for y in range(0, 176, oracle_step):
            for x in range(0, 96, oracle_step):
                p = ((x+.5)/16,(y+.5)/16)
                distance2 = min([audit.squared(p,site) for site in sites]
                                + [audit.segment_distance2(p,a,b) for a,b in links], default=math.inf)
                # Vulkan positive-height viewport reverses production Y.
                at = ((height-1-(row*176+y))*width+column*96+x)*4
                actual = pixels[at+1] != 0
                expected = distance2 <= radius*radius
                actual_count += actual
                expected_count += expected
                glyph_pixels += actual
                if actual and distance2 > (radius+1e-5)**2:
                    extras += 1
                if expected and not actual:
                    if distance2 < (radius-error-1e-5)**2:
                        missing += 1
                    else:
                        chords += 1
        per_glyph.append({"shape": name,"glyph":character,"filled_pixels":glyph_pixels})
    return {"unexpected_filled_pixels":extras,"missing_pixels_beyond_chord_error":missing,
            "missing_pixels_within_chord_error":chords,"ideal_envelope_pixels":expected_count,
            "rendered_pixels":actual_count,"per_glyph":per_glyph,
            "oracle_pixel_stride":oracle_step,
            "oracle_scope":"every pixel" if oracle_step == 1 else "sampled pixel centers"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work-dir", type=Path, default=ROOT / "bld/microfont-dista-host")
    parser.add_argument("--quality", choices=("draft","fine","high"), default="fine")
    parser.add_argument("--atlas", action="store_true",
                        help="validate all 95 printable Basic glyphs in both weights and blocker variants")
    parser.add_argument("--oracle-step", type=int,
                        help="sample every Nth pixel center for the independent oracle (default: atlas 4, stamp 1)")
    args = parser.parse_args()
    if args.oracle_step is not None and not 1 <= args.oracle_step <= 16:
        parser.error("--oracle-step must be between 1 and 16")
    oracle_step = args.oracle_step or (4 if args.atlas else 1)
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    bake, audit = load("bake"), load("audit")
    subprocess.run([sys.executable,str(HERE / "bake.py"),"--source-only","--work-dir",str(work)],check=True)
    segments = {"draft":(12,8),"fine":(16,12),"high":(24,16)}[args.quality]
    width, height = (1536, 1056) if args.atlas else (960, 352)
    baker = bake.load_baker()
    source_path = work / "render.c"
    render_source(baker, source_path, width, height)
    program = work / "render"
    subprocess.run(["cc",str(source_path),"-o",str(program),*baker.vulkan_compile_flags()],check=True)
    fragment = work / "constant.frag.spv"
    subprocess.run(["glslc",str(ROOT / "crates/trueos-shader/xe_lp_shader_bake/simple_triangle.frag"),"-o",str(fragment)],check=True)
    env = os.environ.copy()
    env.pop("LD_PRELOAD",None)
    env.pop("TRUEOS_VK_DEVICE_ID",None)
    env.update({"VK_DRIVER_FILES":"/usr/share/vulkan/icd.d/lvp_icd.json",
                "VK_ICD_FILENAMES":"/usr/share/vulkan/icd.d/lvp_icd.json"})
    seeds = bake.basic_seeds()
    records = []
    failed = False
    for atlas_shape in ((0, 1) if args.atlas else (None,)):
        baseline = None
        for direct in ((False, True) if args.atlas else (False,)):
            variant = "direct" if direct else "loop"
            name = f"atlas-{('meso', 'lean')[atlas_shape]}-{variant}" if args.atlas else "stamp"
            scene_path, rgba = work / f"{name}.float3", work / f"{name}.rgba"
            scene_path.write_bytes(scene(seeds,segments,atlas_shape,direct))
            env.update({"DISTA_HOST_SCENE":str(scene_path),"DISTA_HOST_RGBA":str(rgba)})
            result = subprocess.run([str(program),str(work / "dista_stamp.vert.spv"),str(fragment)],env=env,
                                    text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
            (work / f"{name}.log").write_text(result.stdout)
            if result.returncode:
                raise SystemExit(result.stdout)
            pixels = rgba.read_bytes()
            if len(pixels) != width*height*4:
                raise SystemExit(f"{name}: RGBA readback size mismatch: {len(pixels)}")
            comparison = compare(pixels,audit,segments,atlas_shape,oracle_step)
            comparison.update({"variant":variant,"shape":('Meso','Lean')[atlas_shape] if args.atlas else "both",
                               "triangles_submitted":scene_path.stat().st_size//36,
                               "rgba_sha256":hashlib.sha256(pixels).hexdigest()})
            if args.atlas:
                unused = sum(pixels[((height-1-y)*width+x)*4+1] != 0
                             for y in range(5*176,6*176) for x in range(15*96,16*96))
                comparison["unused_cell_filled_pixels"] = unused
                failed |= bool(unused)
            if baseline is None:
                baseline = pixels
            else:
                comparison["rgba_equal_to_loop"] = pixels == baseline
                comparison["differing_rgba_bytes"] = sum(a != b for a,b in zip(pixels,baseline))
                failed |= pixels != baseline
            failed |= bool(comparison["unexpected_filled_pixels"] or comparison["missing_pixels_beyond_chord_error"])
            records.append(comparison)
            try:
                from PIL import Image
                Image.frombytes("RGBA",(width,height),pixels).transpose(Image.Transpose.FLIP_TOP_BOTTOM).save(work / f"{name}.png")
            except ImportError:
                pass
    report = {"backend":"Lavapipe software Vulkan","quality":args.quality,
              "scope":"production GLSL execution; does not execute Intel native artifact or measure iGPU time",
              "culling":"back, clockwise Vulkan positive-height viewport", "size":[width,height],
              "spirv_sha256":hashlib.sha256((work / "dista_stamp.vert.spv").read_bytes()).hexdigest(),
              "layout":"95 printable Basic glyphs U+0020..U+007E, 16 columns x 6 rows" if args.atlas else "ABC123/Yy% in two weights",
              "passed":not failed,"renders":records}
    # Preserve the original stamp report fields for existing consumers.
    if not args.atlas:
        report.update(records[0])
    (work / "validation.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps(report))
    if failed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
