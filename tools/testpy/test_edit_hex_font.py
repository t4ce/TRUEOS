#!/usr/bin/env python3
"""Check shared canonical bitmap data and run actual Rust/OpenCL font helpers.

The OpenCL helper is compiled as C++ with only its address-space and uint4
spelling adapted, so no GPU or OpenCL runtime is needed for bit parity.
"""

import ctypes
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    subprocess.run([sys.executable, str(ROOT / "tools/edit_hex_font.py"), "--check"], check=True)
    spec = importlib.util.spec_from_file_location("edit_hex_font", ROOT / "tools/edit_hex_font.py")
    generator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(generator)
    masks, _, composed = generator.source_masks()
    assert set(composed) == {ord(ch) for ch in "ẽỹŷẙńĩůỳũǹ"}
    ids = {codepoint: index for index, (codepoint, _) in enumerate(masks)}
    with tempfile.TemporaryDirectory(prefix="trueos-edit-hex-font-") as temporary:
        directory = Path(temporary)
        tests = directory / "tests"
        subprocess.run([
            "rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "--test", "-Adead_code",
            "-C", "linker=" + str(ROOT / "tools/rust-host-ld"),
            str(ROOT / "src/r/services/shell3_hex_font.rs"), "-o", str(tests),
        ], cwd=ROOT, check=True)
        subprocess.run([str(tests)], check=True)

        wrapper = directory / "sample.cpp"
        wrapper.write_text("""#include <cstdint>
#define __global
using uint = std::uint32_t;
struct uint4 {
    uint values[4];
    uint operator[](uint index) const { return values[index]; }
};
""" + "#include " + json.dumps(str(generator.OPENCL)) + "\n" + """
extern "C" uint sample(const uint4 *masks, uint glyph, uint x, uint y) {
    return trueos_edit_hex_coverage(masks, glyph, x, y);
}
""")
        library = directory / "sample.so"
        subprocess.run(["c++", "-std=c++17", "-O2", "-shared", "-fPIC", str(wrapper), "-o", str(library)], check=True)
        module = ctypes.CDLL(str(library))
        sample = module.sample
        sample.argtypes = [ctypes.POINTER(ctypes.c_uint32)] + [ctypes.c_uint32] * 3
        sample.restype = ctypes.c_uint32
        words = [(mask >> (word * 32)) & 0xFFFFFFFF for _, mask in masks for word in range(4)]
        table = (ctypes.c_uint32 * len(words))(*words)
        for index, (codepoint, mask) in enumerate(masks):
            for y in range(11):
                for x in range(6):
                    expected = 255 if mask & (1 << (65 - y * 6 - x)) else 0
                    assert sample(table, index, x, y) == expected, (hex(codepoint), x, y)
            assert sample(table, index, 6, 10) == 0
            assert sample(table, index, 5, 11) == 0
        for y in range(11):
            for x in range(6):
                assert sample(table, 0xFFFFFFFF, x, y) == sample(table, ids[ord("?")], x, y)
        print(f"OpenCL/Rust font parity passed: {len(masks)} glyphs, full 66-bit cells; {len(composed)} canonical compositions")


if __name__ == "__main__":
    main()
