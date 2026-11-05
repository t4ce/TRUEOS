#!/usr/bin/env python3
"""Check production canonical expansion against the physical font's sampler.

Compile the actual mode-2 rectangle calculations, then prove their clipped
intervals partition every output pixel with its original per-cell floor owner.
Exercise every supported cell width/height, clipped glyphs, padded workgroups,
and representative full two-dimensional output rectangles under sanitizers.
"""

from pathlib import Path
import subprocess
import tempfile

from test_edit_hex_shader import KERNEL
from test_edit_life_math import compiler_path


DRIVER = r'''
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <vector>
using uint = std::uint32_t;
struct uint4 { uint x, y, z, w; };
struct Rectangle { uint left = 0, right = 0, top = 0, bottom = 0; bool visible = false; };
static void production_rectangle(uint x, uint y, uint gw, uint gh,
        uint4 display_viewport, Rectangle &result) {
@GEOMETRY@
    result = Rectangle{left, right, top, bottom, true};
}

int main() {
    constexpr uint missing = 0xffffffffu;
    std::uint64_t rectangles = 0, pixels = 0, cases = 0;
    auto check_axis = [&](uint extent, uint physical, uint native, bool horizontal) {
        const uint grid = ((extent + physical - 1u) / physical) * native;
        const uint4 viewport = horizontal ? uint4{extent, 11, physical, 11}
                                          : uint4{6, extent, 6, physical};
        std::vector<uint> owners(extent, missing);
        // Include one complete padded SIMD16 group: no extra invocation may
        // manufacture a rectangle outside the logical canonical dimensions.
        for (uint canonical = 0; canonical < grid + 16u; ++canonical) {
            Rectangle actual;
            production_rectangle(horizontal ? canonical : 0, horizontal ? 0 : canonical,
                horizontal ? grid : 6, horizontal ? 11 : grid, viewport, actual);
            ++rectangles;
            if (!actual.visible) continue;
            assert(canonical < grid);
            assert(actual.left < actual.right && actual.top < actual.bottom);
            assert(actual.right <= viewport.x && actual.bottom <= viewport.y);
            if (horizontal) { assert(actual.top == 0 && actual.bottom == 1); }
            else { assert(actual.left == 0 && actual.right == 1); }
            const uint begin = horizontal ? actual.left : actual.top;
            const uint end = horizontal ? actual.right : actual.bottom;
            for (uint at = begin; at < end; ++at) {
                assert(owners[at] == missing); // Rectangles never overlap.
                owners[at] = canonical;
            }
        }
        for (uint at = 0; at < extent; ++at) {
            const uint expected = (at / physical) * native + (at % physical) * native / physical;
            assert(owners[at] == expected); // No hole or cumulative drift.
            ++pixels;
        }
        ++cases;
    };
    for (uint physical = 6; physical <= 256; ++physical) {
        for (uint extent = 1; extent <= 513; ++extent) check_axis(extent, physical, 6, true);
        for (uint extent : {1699u, 1700u, 2559u, 2560u, 4096u}) check_axis(extent, physical, 6, true);
    }
    for (uint physical = 11; physical <= 256; ++physical) {
        for (uint extent = 1; extent <= 513; ++extent) check_axis(extent, physical, 11, false);
        for (uint extent : {799u, 800u, 1439u, 1440u, 2160u}) check_axis(extent, physical, 11, false);
    }

    struct Scenario { uint width, height, cell_width, cell_height; };
    for (Scenario scenario : {Scenario{1, 1, 6, 11}, {2, 2, 7, 13},
            {1699, 799, 17, 32}, {2560, 31, 17, 31}, {4096, 2160, 256, 256}}) {
        const uint gw = ((scenario.width + scenario.cell_width - 1u) / scenario.cell_width) * 6u;
        const uint gh = ((scenario.height + scenario.cell_height - 1u) / scenario.cell_height) * 11u;
        const uint4 viewport{scenario.width, scenario.height, scenario.cell_width, scenario.cell_height};
        std::vector<uint> owners(std::size_t(scenario.width) * scenario.height, missing);
        for (uint y = 0; y < gh + 1; ++y) for (uint x = 0; x < gw + 16; ++x) {
            Rectangle actual;
            production_rectangle(x, y, gw, gh, viewport, actual);
            ++rectangles;
            if (!actual.visible) continue;
            assert(x < gw && y < gh);
            assert(actual.left < actual.right && actual.top < actual.bottom);
            assert(actual.right <= scenario.width && actual.bottom <= scenario.height);
            for (uint py = actual.top; py < actual.bottom; ++py) {
                for (uint px = actual.left; px < actual.right; ++px) {
                    uint &owner = owners[std::size_t(py) * scenario.width + px];
                    assert(owner == missing);
                    owner = y * gw + x;
                }
            }
        }
        for (uint py = 0; py < scenario.height; ++py) for (uint px = 0; px < scenario.width; ++px) {
            const uint x = (px / scenario.cell_width) * 6u + (px % scenario.cell_width) * 6u / scenario.cell_width;
            const uint y = (py / scenario.cell_height) * 11u + (py % scenario.cell_height) * 11u / scenario.cell_height;
            assert(owners[std::size_t(py) * scenario.width + px] == y * gw + x);
            ++pixels;
        }
        ++cases;
    }
    std::printf("Production canonical expansion exact: %llu cases, %llu rectangles, %llu pixels; no overlaps, holes, or font drift\n",
        static_cast<unsigned long long>(cases), static_cast<unsigned long long>(rectangles),
        static_cast<unsigned long long>(pixels));
}
'''


def main() -> None:
    kernel = KERNEL.read_text()
    branch = kernel.index("if (mode == 2u) {", kernel.index("__kernel void edit_life_rgba8("))
    start = kernel.index("{", branch) + 1
    end = kernel.index("__global const uint *canonical", start)
    geometry = kernel[start:end]
    assert "if (x >= gw || y >= gh) return;" in geometry
    assert "uint left =" in geometry and "uint bottom =" in geometry
    assert "params[" not in geometry, "Display geometry must come from the direct launch payload"
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-division-") as temporary:
        directory = Path(temporary)
        source, binary = directory / "division.cpp", directory / "division"
        source.write_text(DRIVER.replace("@GEOMETRY@", geometry))
        subprocess.run([compiler_path(None), "-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror",
            "-fsanitize=address,undefined", "-fno-sanitize-recover=all", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
