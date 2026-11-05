#!/usr/bin/env python3
"""Compare the complete production Conway shader under strict and fast host math.

OpenCL address spaces, vector constructors and builtins are adapted to host
C++; shader arithmetic and all six kernel branches remain unchanged. The
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

# Coordinates, glyph lookup and output alpha are integer-only. Floating
# lighting runs once per canonical pixel; fast arithmetic may move the final
# rounded RGB byte by one, never choose a different glyph/world pixel.
VISUAL_RGB_TOLERANCE = 1

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
[[maybe_unused]] static inline uint mul_hi(uint a, uint b) { return static_cast<uint>((std::uint64_t(a) * b) >> 32); }
typedef uint uint2 __attribute__((vector_size(8), aligned(4)));
typedef uint uint3 __attribute__((vector_size(16), aligned(4)));
typedef uint uint4 __attribute__((vector_size(16), aligned(4)));
typedef float float2 __attribute__((vector_size(8), aligned(4)));
typedef float float3 __attribute__((vector_size(16), aligned(4)));
static uint global_x, global_y;
[[maybe_unused]] static inline uint get_global_id(uint axis) { return axis == 0 ? global_x : global_y; }
[[maybe_unused]] static inline uint max(uint a, uint b) { return std::max(a, b); }
[[maybe_unused]] static inline uint2 min(uint2 a, uint2 b) { return uint2{std::min(a[0], b[0]), std::min(a[1], b[1])}; }
[[maybe_unused]] static inline float max(float a, float b) { return std::max(a, b); }
[[maybe_unused]] static inline int clamp(int v, int low, int high) { return std::clamp(v, low, high); }
[[maybe_unused]] static inline float clamp(float v, float low, float high) { return std::clamp(v, low, high); }
[[maybe_unused]] static inline float3 clamp(float3 v, float low, float high) {
    return float3{clamp(v[0], low, high), clamp(v[1], low, high), clamp(v[2], low, high)};
}
[[maybe_unused]] static inline float2 fabs(float2 v) { return float2{std::fabs(v[0]), std::fabs(v[1])}; }
[[maybe_unused]] static inline uint2 convert_uint2(float2 v) { return __builtin_convertvector(v, uint2); }
[[maybe_unused]] static inline uint3 convert_uint3(float3 v) { return __builtin_convertvector(v, uint3); }
[[maybe_unused]] static inline float2 convert_float2(uint2 v) { return __builtin_convertvector(v, float2); }
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
    const uint columns = (vp_width + cell_width - 1) / cell_width;
    const uint rows = (vp_height + cell_height - 1) / cell_height;
    const uint grid_width = columns * 6, grid_height = rows * 11;
    assert(grid_width * grid_height <= 2u * 1024u * 1024u);
    const uint source_pitch = width + 7, output_pitch = width + 11, canonical_pitch = grid_width + 5;
    uint p[32] = {0, 1, width, height, left, top, vp_width, vp_height,
        grid_width, grid_height, columns, rows, source_pitch * 4, output_pitch * 4,
        cell_width, cell_height};
    p[16] = static_cast<uint>(((std::uint64_t(1) << 32) + cell_width - 1) / cell_width);
    p[17] = static_cast<uint>(((std::uint64_t(1) << 32) + cell_height - 1) / cell_height);
    p[18] = canonical_pitch * 4;
    p[19] = (vp_width - 1) / cell_width * 6 + (vp_width - 1) % cell_width * 6 / cell_width + 1;
    p[20] = (vp_height - 1) / cell_height * 11 + (vp_height - 1) % cell_height * 11 / cell_height + 1;
    std::vector<uint> read(grid_width * grid_height * 2);
    std::vector<uint> write(read.size(), 0xd15ea5edu);
    std::vector<uint> glyphs(columns * rows * 4);
    std::vector<uint> previous(columns * rows * 2);
    std::vector<uint> source(source_pitch * height), output(output_pitch * height, 0xa5b6c7d8u);
    std::vector<uint> canonical(canonical_pitch * grid_height, 0xcac4e5edu);
    uint random = width * 71237u + height * 131u + 1;
    for (uint y = 0; y < grid_height; ++y) for (uint x = 0; x < grid_width; ++x) {
        const uint i = y * grid_width + x;
        if (x >= p[19] || y >= p[20]) continue;
        uint value = random_word(random);
        read[i * 2] = (value & 1u) | (((value >> 8) & 255u) << 8) | (((value >> 16) & 1u) << 16);
        read[i * 2 + 1] = random_word(random) & 0x00ffffffu;
    }
    for (uint& value : source) value = random_word(random);
    for (uint i = 0; i < columns * rows; ++i) {
        glyphs[i * 4] = glyph_ids[i % glyph_ids.size()];
        // Include default white, partly opaque syntax ink, invisible ink,
        // opaque selection/caret backgrounds and underlined cells.
        const uint alphas[] = {255, 255, 128, 0, 254, 1, 127};
        glyphs[i * 4 + 1] = (random_word(random) & 0x00ffffffu) | (alphas[i % 7] << 24);
        glyphs[i * 4 + 2] = (random_word(random) & 0x00ffffffu) | 0xff000000u;
        glyphs[i * 4 + 3] = i % 4;
        if (i % 7 == 0) glyphs[i * 4 + 1] = 0xffffffffu;
        if (i % 13 == 1) {
            glyphs[i * 4 + 1] = 0xff000000u;
            glyphs[i * 4 + 2] = 0xffffffffu;
            glyphs[i * 4 + 3] |= 1u;
        }
        previous[i * 2] = space;
        previous[i * 2 + 1] = 0;
    }
    const auto immutable_source = source, immutable_font = font;
    uint sequence = 0;
    auto run = [&](uint mode, uint reset, uint4 rect = uint4{0, 0, 0, 0}) {
        p[0] = mode; p[1] = reset;
        const auto before_read = read, before_write = write, before_previous = previous;
        const auto before_output = output, before_canonical = canonical, before_glyphs = glyphs;
        const std::vector<uint> before_params(p, p + 32);
        const uint domain_width = mode == 3 ? columns : mode == 5 ? (rect[2] + 1) / 2 : grid_width;
        const uint domain_height = mode == 3 ? rows : mode == 5 ? rect[3] : grid_height;
        const uint4 display_page = {width, height, left, top};
        const uint4 display_viewport = {vp_width, vp_height, cell_width, cell_height};
        const uint4 display_image = {canonical_pitch * 4, grid_width, grid_height, mode};
        const uint4 display_pitch = {source_pitch * 4, output_pitch * 4, 0, 0};
        const bool display = mode == 2 || mode == 5;
        // Hardware launches SIMD16-rounded X; extra Y invocations exercise
        // every branch's guard without trusting exact dispatch dimensions.
        for (global_y = 0; global_y < domain_height + 3; ++global_y) {
            for (global_x = 0; global_x < ((domain_width + 15) / 16) * 16; ++global_x) {
                // The display branches must depend only on their by-value
                // geometry and image/chrome inputs. Null unused resources
                // make an accidental glyph/world/global-param read fail.
                edit_life_rgba8(display ? nullptr : reinterpret_cast<const uint2*>(read.data()),
                    display ? nullptr : reinterpret_cast<uint2*>(write.data()),
                    display ? nullptr : reinterpret_cast<const uint4*>(glyphs.data()),
                    display ? nullptr : reinterpret_cast<uint2*>(previous.data()),
                    reinterpret_cast<const uint4*>(mode == 2 ? canonical.data() : mode == 5 ? nullptr : font.data()),
                    mode == 2 ? nullptr : source.data(), mode == 4 ? canonical.data() : output.data(),
                    display ? nullptr : p,
                    display_page, display_viewport, display_image, rect, display_pitch);
            }
        }
        assert(read == before_read && glyphs == before_glyphs && source == immutable_source && font == immutable_font);
        assert(std::equal(p, p + 32, before_params.begin()));
        if (mode != 0 && mode != 1) assert(write == before_write);
        if (mode != 3) assert(previous == before_previous);
        if (mode != 2 && mode != 5) assert(output == before_output);
        if (mode != 4) assert(canonical == before_canonical);
        if (mode == 0 || mode == 1) {
            for (uint y = 0; y < grid_height; ++y) for (uint x = 0; x < grid_width; ++x) {
                const uint at = y * grid_width + x;
                if (x >= p[19] || y >= p[20]) {
                    assert(write[at * 2] == 0 && write[at * 2 + 1] == 0);
                } else if (mode == 0) {
                    const uint glyph_at = y / 11 * columns + x / 6;
                    const uint coverage = glyph_coverage(reinterpret_cast<const uint4*>(font.data()),
                        uint2{glyphs[glyph_at * 4], glyphs[glyph_at * 4 + 3]}, x % 6, y % 11);
                    const uint old_coverage = glyph_coverage(reinterpret_cast<const uint4*>(font.data()),
                        uint2{before_previous[glyph_at * 2], before_previous[glyph_at * 2 + 1]}, x % 6, y % 11);
                    if (reset) {
                        assert(write[at * 2] == (coverage ? 0x0001ff01u : 0));
                        assert(write[at * 2 + 1] == (coverage ? glyphs[glyph_at * 4 + 1] & 0x00ffffffu : 0));
                    } else if (coverage <= old_coverage) {
                        assert(write[at * 2] == before_read[at * 2]);
                        assert(write[at * 2 + 1] == before_read[at * 2 + 1]);
                    } else assert((write[at * 2] & 0xffffu) == 0xff01u);
                }
            }
        }
        if (mode == 3) for (uint i = 0; i < columns * rows; ++i) {
            assert(previous[i * 2] == glyphs[i * 4]);
            assert(previous[i * 2 + 1] == glyphs[i * 4 + 3]);
        }
        if (mode == 4) {
            for (uint y = 0; y < grid_height; ++y) for (uint x = 0; x < canonical_pitch; ++x) {
                uint pixel = canonical[y * canonical_pitch + x];
                if (x >= grid_width) assert(pixel == 0xcac4e5edu);
                else if (x >= p[19] || y >= p[20]) assert(pixel == 0x99000000u);
                else {
                    uint ink = glyph_ink(reinterpret_cast<const uint4*>(glyphs.data()),
                        reinterpret_cast<const uint4*>(font.data()), p, x, y);
                    if ((ink >> 24) == 255) assert(pixel == ink); // Text/caret/selection exact RGB.
                    uint alpha = (ink >> 24) + (153 * (255 - (ink >> 24)) + 127) / 255;
                    assert((pixel >> 24) == alpha);
                    for (uint shift : {0, 8, 16}) assert(((pixel >> shift) & 255u) <= alpha);
                }
            }
        }
        if (mode == 2) {
            for (uint y = 0; y < height; ++y) for (uint x = 0; x < output_pitch; ++x) {
                uint pixel = output[y * output_pitch + x];
                if (x >= width) assert(pixel == 0xa5b6c7d8u);
                else if (x < left || y < top || x - left >= vp_width || y - top >= vp_height)
                    assert(pixel == before_output[y * output_pitch + x]);
                else {
                    uint px = x - left, py = y - top;
                    uint gx = px / cell_width * 6 + px % cell_width * 6 / cell_width;
                    uint gy = py / cell_height * 11 + py % cell_height * 11 / cell_height;
                    assert(pixel == canonical[gy * canonical_pitch + gx]); // Pure per-cell nearest copy.
                }
            }
            if (vp_width == 2560 && cell_width == 17) {
                assert(p[19] == 904 && grid_width == 906);
                assert((2559 / 17) * 6 + (2559 % 17) * 6 / 17 == 903);
                assert(2559 * grid_width / vp_width != 903); // Global scaling would visibly drift.
            }
        }
        if (mode == 5) {
            for (uint y = 0; y < height; ++y) for (uint x = 0; x < output_pitch; ++x) {
                const bool inside = x >= rect[0] && y >= rect[1]
                    && x - rect[0] < rect[2] && y - rect[1] < rect[3];
                assert(output[y * output_pitch + x] == (inside
                    ? source[y * source_pitch + x] | 0xff000000u : before_output[y * output_pitch + x]));
            }
        }
        uint header[] = {width, height, mode, sequence++};
        assert(std::fwrite(header, sizeof(uint), 4, stdout) == 4);
        words(write); words(previous);
        words(mode == 4 ? canonical : std::vector<uint>{});
        std::vector<uint> display_result;
        if (mode == 2) display_result = output;
        if (mode == 5) for (uint y = rect[1]; y < rect[1] + rect[3]; ++y)
            for (uint x = rect[0]; x < rect[0] + rect[2]; ++x)
                display_result.push_back(output[y * output_pitch + x]);
        words(display_result);
        if (mode == 0 || mode == 1) read.swap(write);
    };
    auto present = [&] {
        // Poison every presentation, so unchanged previous chrome or viewport
        // pixels cannot hide an omitted strip or incomplete expansion.
        std::fill(output.begin(), output.end(), 0xa5b6c7d8u);
        run(2, 0);
        const uint4 strips[] = {{0, 0, width, top}, {0, top + vp_height, width, height - top - vp_height},
            {0, top, left, vp_height}, {left + vp_width, top, width - left - vp_width, vp_height}};
        for (uint4 strip : strips) if (strip[2] && strip[3]) run(5, 0, strip);
        for (uint y = 0; y < height; ++y) for (uint x = 0; x < output_pitch; ++x) {
            uint expected = 0xa5b6c7d8u;
            if (x < width) {
                expected = source[y * source_pitch + x] | 0xff000000u;
                if (x >= left && y >= top && x - left < vp_width && y - top < vp_height) {
                    const uint px = x - left, py = y - top;
                    const uint gx = px / cell_width * 6 + px % cell_width * 6 / cell_width;
                    const uint gy = py / cell_height * 11 + py % cell_height * 11 / cell_height;
                    expected = canonical[gy * canonical_pitch + gx];
                }
            }
            assert(output[y * output_pitch + x] == expected); // Exact previous physical-domain copy.
        }
    };
    // Varied initial world exercises lighting even on tiny glyph inputs.
    run(4, 0); present();
    if (composite_only) return;
    run(0, 1); run(3, 0); run(4, 0); present();
    for (uint generation = 0; generation < 3; ++generation) { run(1, 0); run(4, 0); present(); }
    for (uint i = 0; i < columns * rows; i += 3) {
        glyphs[i * 4] = glyph_ids[(i + 5) % glyph_ids.size()];
        glyphs[i * 4 + 3] ^= 2u;
    }
    run(0, 0); run(3, 0); run(4, 0); present();
    // Whitespace-only changes advance the baseline without emitting; the
    // unchanged input then emits nothing, and reset clears the visible world.
    for (uint i = 0; i < columns * rows; ++i) { glyphs[i * 4] = space; glyphs[i * 4 + 3] = 0; }
    run(3, 0); run(0, 0); run(4, 0); present(); run(0, 1); run(3, 0); run(4, 0); present();
}
int main() {
    const std::vector<uint> font = {@FONT_WORDS@};
    const std::vector<uint> glyphs = {@GLYPH_IDS@};
    const uint space = @SPACE@;
    scenario(1, 1, 0, 0, 1, 1, 6, 11, font, space, glyphs);
    scenario(15, 27, 1, 1, 13, 25, 6, 11, font, space, glyphs);
    scenario(19, 29, 2, 3, 15, 23, 7, 13, font, space, glyphs);
    scenario(517, 515, 1, 1, 513, 513, 256, 256, font, space, glyphs);
    scenario(97, 63, 5, 7, 89, 51, 17, 31, font, space, glyphs);
    scenario(131, 69, 1, 2, 129, 65, 17, 31, font, space, glyphs);
    scenario(257, 143, 12, 11, 241, 127, 12, 22, font, space, glyphs);
    scenario(1281, 721, 17, 31, 1259, 677, 23, 42, font, space, glyphs);
    scenario(4093, 73, 1, 1, 4091, 71, 7, 13, font, space, glyphs);
    scenario(73, 2053, 1, 1, 71, 2051, 17, 31, font, space, glyphs);
    scenario(2560, 63, 0, 1, 2560, 62, 17, 31, font, space, glyphs);
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


def fixture_source(shader_source: str | None = None) -> str:
    spec = importlib.util.spec_from_file_location("edit_hex_font", ROOT / "tools/edit_hex_font.py")
    generator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(generator)
    masks, _, _ = generator.source_masks()
    ids = {codepoint: index for index, (codepoint, _) in enumerate(masks)}
    source = shader_source if shader_source is not None else KERNEL.read_text()
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
        for _ in range(4):
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
        outputs = {}
        strict_flags = ["-fno-fast-math", "-ffp-contract=off"]
        current_source = KERNEL.read_text()
        profiles = [("strict", strict_flags, current_source), ("fast", ["-ffast-math"], current_source)]
        for name, flags, shader in profiles:
            source = directory / (name + ".cpp")
            source.write_text(fixture_source(shader))
            binary = directory / name
            command = [compiler, "-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror",
                "-fsanitize=address,undefined", "-fno-sanitize-recover=all", "-I", str(KERNEL.parent),
                *flags, str(source), "-o", str(binary)]
            subprocess.run(command, check=True)
            output = directory / (name + ".bin")
            with output.open("wb") as stream:
                subprocess.run([str(binary)], stdout=stream, env={**os.environ, "ASAN_OPTIONS": "detect_leaks=1"}, check=True)
            outputs[name] = output
        compared, changed, largest, above_one = 0, 0, 0, 0
        strict, fast = list(records(outputs["strict"])), list(records(outputs["fast"]))
        assert len(strict) == len(fast)
        for (header, a), (other_header, b) in zip(strict, fast):
            assert header == other_header
            assert a[0] == b[0], (header, "integer simulation changed")
            assert a[1] == b[1], (header, "integer glyph baseline changed")
            rgba_a, rgba_b = bytes(a[2]) + bytes(a[3]), bytes(b[2]) + bytes(b[3])
            assert len(rgba_a) == len(rgba_b)
            compared += len(rgba_a)
            if rgba_a == rgba_b:
                continue
            for index, (left, right) in enumerate(zip(rgba_a, rgba_b)):
                if index % 4 == 3:
                    assert left == right, (header, index // 4, "alpha changed")
                else:
                    delta = abs(left - right)
                    largest = max(largest, delta)
                    changed += delta != 0
                    above_one += delta > 1
                    assert delta <= VISUAL_RGB_TOLERANCE, (header, index // 4, index % 4, left, right)
        print(f"{version}: strict vs -ffast-math; {len(strict)} six-mode snapshots, "
              f"{compared} output bytes, changed_rgb_bytes={changed}, max_rgb_delta={largest}, "
              f"rgb_deltas_above_one={above_one}; "
              "integer state/glyphs/alpha exact; opaque ink exact; pure nearest copy exact; "
              "ASan/UBSan and padded edge launches passed")


if __name__ == "__main__":
    main()
