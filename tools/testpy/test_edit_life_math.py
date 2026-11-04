#!/usr/bin/env python3
"""Compare the complete production Conway shader under strict and fast host math.

OpenCL address spaces, vector constructors and builtins are adapted to host
C++; shader arithmetic and all four kernel branches remain unchanged. Both
executables run with address/undefined-behavior sanitizers and padded launches.
This checks host floating-point sensitivity, not native Intel GPU performance
or exact equivalence to IGC's -cl-fast-relaxed-math implementation.
"""

import argparse
import importlib.util
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile

from test_edit_hex_shader import KERNEL, ROOT, host_vectors

# An FP-rounded cell boundary may select a neighboring halo while its own
# tile is dark. Four full-strength neighbors contribute at most
# 4 * 255 * .018 * .6 = 11.016 RGB units, plus final byte rounding.
VISUAL_RGB_TOLERANCE = 12


ADAPTER = r'''
#include <algorithm>
#include <cassert>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <vector>
#define __global
#define __kernel
using uint = std::uint32_t;
using ulong = std::uint64_t;
typedef uint uint2 __attribute__((vector_size(8), aligned(4)));
typedef uint uint3 __attribute__((vector_size(16), aligned(4)));
typedef uint uint4 __attribute__((vector_size(16), aligned(4)));
typedef float float2 __attribute__((vector_size(8), aligned(4)));
typedef float float3 __attribute__((vector_size(16), aligned(4)));
static uint global_x, global_y;
static inline uint get_global_id(uint axis) { return axis == 0 ? global_x : global_y; }
static inline uint max(uint a, uint b) { return std::max(a, b); }
static inline uint2 min(uint2 a, uint2 b) { return uint2{std::min(a[0], b[0]), std::min(a[1], b[1])}; }
static inline float max(float a, float b) { return std::max(a, b); }
static inline int clamp(int v, int low, int high) { return std::clamp(v, low, high); }
static inline float clamp(float v, float low, float high) { return std::clamp(v, low, high); }
static inline float3 clamp(float3 v, float low, float high) {
    return float3{clamp(v[0], low, high), clamp(v[1], low, high), clamp(v[2], low, high)};
}
static inline float2 fabs(float2 v) { return float2{std::fabs(v[0]), std::fabs(v[1])}; }
static inline uint2 convert_uint2(float2 v) { return __builtin_convertvector(v, uint2); }
static inline uint3 convert_uint3(float3 v) { return __builtin_convertvector(v, uint3); }
static inline float2 convert_float2(uint2 v) { return __builtin_convertvector(v, float2); }
'''


DRIVER = r'''
static void words(const std::vector<uint>& values) {
    uint count = static_cast<uint>(values.size());
    assert(std::fwrite(&count, sizeof(count), 1, stdout) == 1);
    assert(std::fwrite(values.data(), sizeof(uint), values.size(), stdout) == values.size());
}
static uint random_word(uint& state) {
    state ^= state << 13; state ^= state >> 17; state ^= state << 5;
    return state;
}
static void scenario(uint width, uint height, uint left, uint top, uint vp_width, uint vp_height,
    uint cell_width, uint cell_height, const std::vector<uint>& font,
    uint space, const std::vector<uint>& glyph_ids, bool composite_only = false)
{
    const uint grid_width = std::min(512u, (vp_width + 2) / 3);
    const uint grid_height = std::min(512u, (vp_height + 2) / 3);
    const uint columns = (vp_width + cell_width - 1) / cell_width;
    const uint rows = (vp_height + cell_height - 1) / cell_height;
    const uint source_pitch = width + 7, output_pitch = width + 11;
    uint p[16] = {0, 1, width, height, left, top, vp_width, vp_height,
        grid_width, grid_height, columns, rows, source_pitch * 4, output_pitch * 4,
        cell_width, cell_height};
    std::vector<uint> read(grid_width * grid_height * 2);
    std::vector<uint> write(read.size(), 0xd15ea5edu);
    std::vector<uint> glyphs(columns * rows * 4);
    std::vector<uint> previous(columns * rows * 2);
    std::vector<uint> source(source_pitch * height), output(output_pitch * height, 0xa5b6c7d8u);
    uint random = width * 71237u + height * 131u + 1;
    for (uint i = 0; i < grid_width * grid_height; ++i) {
        uint value = random_word(random);
        read[i * 2] = (value & 1u) | (((value >> 8) & 255u) << 8) | (((value >> 16) & 1u) << 16);
        read[i * 2 + 1] = random_word(random) & 0x00ffffffu;
    }
    for (uint& value : source) value = random_word(random);
    for (uint i = 0; i < columns * rows; ++i) {
        glyphs[i * 4] = glyph_ids[i % glyph_ids.size()];
        // Include opaque/default text, partially opaque syntax ink, invisible
        // ink, opaque selections/carets and underlined cells.
        const uint alphas[] = {255, 255, 128, 0, 254, 1, 127};
        glyphs[i * 4 + 1] = (random_word(random) & 0x00ffffffu) | (alphas[i % 7] << 24);
        glyphs[i * 4 + 2] = (random_word(random) & 0x00ffffffu) | 0xff000000u;
        glyphs[i * 4 + 3] = i % 4;
        if (i % 7 == 0) glyphs[i * 4 + 1] = 0xffffffffu; // Current default white.
        if (i % 13 == 1) { // Inverted block caret, independent of background lighting.
            glyphs[i * 4 + 1] = 0xff000000u;
            glyphs[i * 4 + 2] = 0xffffffffu;
            glyphs[i * 4 + 3] |= 1u;
        }
        previous[i * 2] = space;
        previous[i * 2 + 1] = 0;
    }
    const auto immutable_source = source;
    const auto immutable_font = font;
    uint sequence = 0;
    auto run = [&](uint mode, uint reset) {
        p[0] = mode; p[1] = reset;
        const auto before_read = read, before_write = write, before_previous = previous;
        const auto before_output = output, before_glyphs = glyphs;
        const std::vector<uint> before_params(p, p + 16);
        const uint domain_width = mode < 2 ? grid_width : mode == 3 ? columns : width;
        const uint domain_height = mode < 2 ? grid_height : mode == 3 ? rows : height;
        // Launch the same SIMD16-rounded X domain as hardware, plus edge Y
        // invocations which must return without reading or writing storage.
        for (global_y = 0; global_y < domain_height + 3; ++global_y) {
            for (global_x = 0; global_x < ((domain_width + 15) / 16) * 16; ++global_x) {
                edit_life_rgba8(reinterpret_cast<const uint2*>(read.data()),
                    reinterpret_cast<uint2*>(write.data()), reinterpret_cast<const uint4*>(glyphs.data()),
                    reinterpret_cast<uint2*>(previous.data()), reinterpret_cast<const uint4*>(font.data()),
                    source.data(), output.data(), p);
            }
        }
        assert(read == before_read && glyphs == before_glyphs && source == immutable_source && font == immutable_font);
        assert(std::equal(p, p + 16, before_params.begin()));
        if (mode != 0 && mode != 1) assert(write == before_write);
        if (mode != 3) assert(previous == before_previous);
        if (mode != 2) assert(output == before_output);
        if (mode == 2) {
            for (uint y = 0; y < height; ++y) {
                for (uint x = 0; x < output_pitch; ++x) {
                    uint pixel = output[y * output_pitch + x];
                    if (x >= width) assert(pixel == 0xa5b6c7d8u);
                    else if (x < left || y < top || x - left >= vp_width || y - top >= vp_height)
                        assert(pixel == (source[y * source_pitch + x] | 0xff000000u));
                    else {
                        uint ink = glyph_ink(reinterpret_cast<const uint4*>(glyphs.data()),
                            reinterpret_cast<const uint4*>(font.data()), p, x - left, y - top);
                        if ((ink >> 24) == 255) assert(pixel == ink);
                        assert((pixel & 255u) <= (pixel >> 24));
                        assert(((pixel >> 8) & 255u) <= (pixel >> 24));
                        assert(((pixel >> 16) & 255u) <= (pixel >> 24));
                    }
                }
            }
        }
        uint header[] = {width, height, mode, sequence++};
        assert(std::fwrite(header, sizeof(uint), 4, stdout) == 4);
        words(write); words(previous); words(mode == 2 ? output : std::vector<uint>{});
        if (mode == 0 || mode == 1) read.swap(write);
    };
    // Begin with varied occupancy/trails/births to exercise composition even
    // when a tiny page has too few glyph pixels to seed a live world.
    run(2, 0);
    if (composite_only) return;
    run(0, 1); run(3, 0); run(2, 0);
    for (uint generation = 0; generation < 3; ++generation) { run(1, 0); run(2, 0); }
    for (uint i = 0; i < columns * rows; i += 3) {
        glyphs[i * 4] = glyph_ids[(i + 5) % glyph_ids.size()];
        glyphs[i * 4 + 3] ^= 2u;
    }
    run(0, 0); run(3, 0); run(2, 0);
    // Whitespace-only updates still advance the compact baseline, followed by
    // an unchanged input emission and an explicit scroll-style reseed.
    for (uint i = 0; i < columns * rows; ++i) { glyphs[i * 4] = space; glyphs[i * 4 + 3] = 0; }
    run(3, 0); run(0, 0); run(2, 0); run(0, 1); run(3, 0); run(2, 0);
}
int main() {
    const std::vector<uint> font = {@FONT_WORDS@};
    const std::vector<uint> glyphs = {@GLYPH_IDS@};
    const uint space = @SPACE@;
    scenario(1, 1, 0, 0, 1, 1, 6, 11, font, space, glyphs);
    scenario(19, 29, 2, 3, 15, 23, 7, 13, font, space, glyphs);
    scenario(97, 63, 5, 7, 89, 51, 17, 31, font, space, glyphs);
    scenario(257, 143, 12, 11, 241, 127, 12, 22, font, space, glyphs);
    scenario(1281, 721, 17, 31, 1259, 677, 23, 42, font, space, glyphs);
    scenario(4093, 73, 1, 1, 4091, 71, 7, 13, font, space, glyphs);
    scenario(73, 2053, 1, 1, 71, 2051, 17, 31, font, space, glyphs);
    // Exercise every pixel and all capped grid indices at the native maximum
    // page dimensions without repeating the cold seed/emit scenarios there.
    scenario(4096, 2160, 3, 1, 4091, 2157, 23, 42, font, space, glyphs, true);
}
'''


def compiler_path(requested: str | None) -> str:
    choices = [requested] if requested else [shutil.which("clang++"),
        "/tmp/trueos-edit-life-toolchain/root/usr/lib/llvm-21/bin/clang++", shutil.which("c++")]
    for choice in choices:
        if choice and Path(choice).is_file():
            return str(choice)
    raise RuntimeError("No host C++ compiler found; pass --compiler /path/to/clang++")


def fixture_source() -> str:
    spec = importlib.util.spec_from_file_location("edit_hex_font", ROOT / "tools/edit_hex_font.py")
    generator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(generator)
    masks, _, _ = generator.source_masks()
    ids = {codepoint: index for index, (codepoint, _) in enumerate(masks)}
    source = KERNEL.read_text()
    # Reuse the existing integer literal/swizzle adapter. Float constructors
    # receive the same syntax-only conversion, without changing vector types.
    integer = host_vectors(source)
    float_literals = re.compile(r"\((float[23])\)\(")
    while match := float_literals.search(integer):
        position, depth = match.end(), 1
        while depth:
            depth += (integer[position] == "(") - (integer[position] == ")")
            position += 1
        integer = integer[:match.start()] + match[1] + "{" + integer[match.end():position - 1] + "}" + integer[position:]
    shader = re.sub(r"__attribute__\(\(intel_reqd_sub_group_size\(16\)\)\)\s*", "", integer)
    font_words = [(mask >> (word * 32)) & 0xffffffff for _, mask in masks for word in range(4)]
    glyphs = [ids[ord(character)] for character in "Ag █ĩ?─"] + [0xffffffff]
    driver = DRIVER.replace("@FONT_WORDS@", ",".join(str(word) + "u" for word in font_words))
    driver = driver.replace("@GLYPH_IDS@", ",".join(str(glyph) + "u" for glyph in glyphs))
    driver = driver.replace("@SPACE@", str(ids[ord(" ")]) + "u")
    return ADAPTER + shader + driver


def records(path: Path):
    data = path.read_bytes()
    offset = 0
    while offset < len(data):
        header = struct.unpack_from("<4I", data, offset)
        offset += 16
        buffers = []
        for _ in range(3):
            count, = struct.unpack_from("<I", data, offset)
            offset += 4
            buffers.append(memoryview(data)[offset:offset + count * 4])
            offset += count * 4
        yield header, buffers
    assert offset == len(data)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler")
    args = parser.parse_args()
    compiler = compiler_path(args.compiler)
    version = subprocess.check_output([compiler, "--version"], text=True).splitlines()[0]
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-math-") as temporary:
        directory = Path(temporary)
        source = directory / "shader.cpp"
        source.write_text(fixture_source())
        outputs = []
        for name, flags in [("strict", ["-fno-fast-math", "-ffp-contract=off"]), ("fast", ["-ffast-math"])]:
            binary = directory / name
            command = [compiler, "-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror",
                "-fsanitize=address,undefined", "-fno-sanitize-recover=all", "-I", str(KERNEL.parent),
                *flags, str(source), "-o", str(binary)]
            subprocess.run(command, check=True)
            output = directory / (name + ".bin")
            with output.open("wb") as stream:
                subprocess.run([str(binary)], stdout=stream, env={**os.environ, "ASAN_OPTIONS": "detect_leaks=1"}, check=True)
            outputs.append(output)
        compared, changed, largest, above_one = 0, 0, 0, 0
        strict, fast = list(records(outputs[0])), list(records(outputs[1]))
        assert len(strict) == len(fast)
        for (header, a), (other_header, b) in zip(strict, fast):
            assert header == other_header
            assert a[0] == b[0], (header, "integer simulation changed")
            assert a[1] == b[1], (header, "integer glyph baseline changed")
            assert len(a[2]) == len(b[2])
            compared += len(a[2])
            if a[2] == b[2]:
                continue
            for index, (left, right) in enumerate(zip(a[2], b[2])):
                if index % 4 == 3:
                    assert left == right, (header, index // 4, "alpha changed")
                else:
                    delta = abs(left - right)
                    largest = max(largest, delta)
                    changed += delta != 0
                    above_one += delta > 1
                    assert delta <= VISUAL_RGB_TOLERANCE, (header, index // 4, index % 4, left, right)
        print(f"{version}: strict vs -ffast-math; {len(strict)} four-mode snapshots, "
              f"{compared} output bytes, changed_rgb_bytes={changed}, max_rgb_delta={largest}, "
              f"rgb_deltas_above_one={above_one}; "
              "integer state/glyphs/alpha exact; ASan/UBSan and padded edge launches passed")


if __name__ == "__main__":
    main()
