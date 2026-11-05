#!/usr/bin/env python3
"""Run the production OpenCL glyph sampler/tint on the host against MicroFont.

Only address spaces and OpenCL vector spelling are adapted for the host C++
compiler. The actual coverage and ink helper bodies are extracted unchanged.
"""

import ctypes
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
KERNEL = ROOT / "crates/trueos-shader/gpgpu/kernels/edit_life_rgba8.clcpp"
PARAM_WORDS = 32


def cell_reciprocal(divisor: int) -> int:
    assert 1 <= divisor <= 256
    return (((1 << 32) + divisor - 1) // divisor) & 0xffffffff


def c_function(source: str, name: str) -> str:
    start = source.index("static inline uint " + name + "(")
    position = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[position] == "{") - (source[position] == "}")
        position += 1
    return source[start:position] + "\n"


def host_vectors(source: str) -> str:
    """Translate OpenCL vector literals/swizzles without changing arithmetic."""
    pattern = re.compile(r"\((uint[234])\)\(")
    while match := pattern.search(source):
        position = match.end()
        depth = 1
        while depth:
            if source[position] == "(":
                depth += 1
            elif source[position] == ")":
                depth -= 1
            position += 1
        source = (source[:match.start()] + match[1] + "{" + source[match.end():position - 1]
                  + "}" + source[position:])
    return re.sub(r"\.([xyzw])\b", lambda match: "[" + str("xyzw".index(match[1])) + "]", source)


def main() -> None:
    spec = importlib.util.spec_from_file_location("edit_hex_font", ROOT / "tools/edit_hex_font.py")
    generator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(generator)
    masks, _, _ = generator.source_masks()
    ids = {codepoint: index for index, (codepoint, _) in enumerate(masks)}
    shader = KERNEL.read_text()
    coverage = c_function(shader, "glyph_coverage")
    ink = shader[shader.index("static inline uint glyph_ink("):shader.index("static inline float3 halo_at(")]
    with tempfile.TemporaryDirectory(prefix="trueos-edit-hex-shader-") as temporary:
        directory = Path(temporary)
        wrapper = directory / "shader.cpp"
        wrapper.write_text("""#include <cstdint>
#define __global
using uint = std::uint32_t;
typedef uint uint2 __attribute__((vector_size(8), aligned(4)));
// GCC requires a power-of-two vector size; the unused fourth uint3 lane does
// not participate in either the OpenCL helper's result or its arithmetic.
typedef uint uint3 __attribute__((vector_size(16), aligned(4)));
typedef uint uint4 __attribute__((vector_size(16), aligned(4)));
""" + "#include " + json.dumps(str(generator.OPENCL)) + "\n" + host_vectors(coverage + ink) + """
extern "C" uint sample_coverage(const uint4 *font, uint index, uint flags, uint x, uint y) {
    return glyph_coverage(font, uint2{index, flags}, x, y);
}
extern "C" void sample_ink(const uint4 *cells, const uint4 *font, const uint *params,
    uint width, uint height, uint *output) {
    for (uint y = 0; y < height; ++y) {
        for (uint x = 0; x < width; ++x) {
            output[y * width + x] = glyph_ink(cells, font, params, x, y);
        }
    }
}
""")
        library = directory / "shader.so"
        subprocess.run(["c++", "-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-shared", "-fPIC", str(wrapper), "-o", str(library)], check=True)
        module = ctypes.CDLL(str(library))
        uint = ctypes.c_uint32
        pointer = ctypes.POINTER(uint)
        sample_coverage = module.sample_coverage
        sample_coverage.argtypes = [pointer, uint, uint, uint, uint]
        sample_coverage.restype = uint
        sample_ink = module.sample_ink
        sample_ink.argtypes = [pointer, pointer, pointer, uint, uint, pointer]
        sample_ink.restype = None
        words = [(mask >> (word * 32)) & 0xFFFFFFFF for _, mask in masks for word in range(4)]
        table = (uint * len(words))(*words)

        for index, (codepoint, mask) in enumerate(masks):
            for y in range(11):
                for x in range(6):
                    expected = 255 if mask & (1 << (65 - y * 6 - x)) else 0
                    assert sample_coverage(table, index, 0, x, y) == expected, (hex(codepoint), x, y)
                    assert sample_coverage(table, index, 2, x, y) == (255 if y == 10 else expected)

        styles = [
            ([255, 255, 255, 255], [0, 0, 0, 255]),
            ([17, 129, 251, 0], [40, 90, 150, 255]),
            ([17, 129, 251, 1], [40, 90, 150, 0]),
            ([17, 129, 251, 127], [40, 90, 150, 1]),
            ([17, 129, 251, 128], [40, 90, 150, 127]),
            ([17, 129, 251, 254], [40, 90, 150, 128]),
            ([17, 129, 251, 255], [40, 90, 150, 254]),
            ([251, 17, 129, 129], [150, 40, 90, 255]),
        ]
        glyphs = [ids[ord(character)] for character in "Ag █ĩ?─"] + [0xFFFFFFFF]
        tested = 0
        # This helper is canonical: two 6x11 cells per axis, with the last
        # column/row clipped. Physical nearest expansion is covered by the
        # complete five-mode shader test, independently of font sampling.
        width, height = 9, 17
        params = (uint * PARAM_WORDS)()
        params[10], params[11], params[14], params[15] = 2, 2, 6, 11
        result = (uint * (width * height))()
        for start in range(0, len(glyphs), 4):
            for foreground, background in styles:
                for flags in range(4):
                    packed_fg = int.from_bytes(bytes(foreground), "little")
                    packed_bg = int.from_bytes(bytes(background), "little")
                    cells = [(glyphs[(start + index) % len(glyphs)], packed_fg, packed_bg, flags)
                             for index in range(4)]
                    records = (uint * 16)(*(word for cell in cells for word in cell))
                    sample_ink(records, table, params, width, height, result)
                    for y in range(height):
                        for x in range(width):
                            cell = cells[(y // 11) * 2 + x // 6]
                            glyph = cell[0] if cell[0] < len(masks) else ids[ord("?")]
                            native_x = x % 6
                            native_y = y % 11
                            mask = masks[glyph][1]
                            coverage = 255 if ((flags & 2 and native_y == 10)
                                                  or mask & (1 << (65 - native_y * 6 - native_x))) else 0
                            alpha = (coverage * foreground[3] + 127) // 255
                            bg = background if flags & 1 else [0, 0, 0, 0]
                            expected = [
                                (foreground[channel] * alpha
                                 + ((bg[channel] * bg[3] + 127) // 255) * (255 - alpha) + 127) // 255
                                for channel in range(3)
                            ]
                            expected.append(alpha + (bg[3] * (255 - alpha) + 127) // 255)
                            actual = int(result[y * width + x]).to_bytes(4, "little")
                            assert actual == bytes(expected), (x, y, flags, foreground, bg,
                                                                actual, expected)
                            tested += 1
        print(f"OpenCL glyph helpers passed: {len(masks)} full masks, {tested} canonical/clipped tint pixels")


if __name__ == "__main__":
    main()
