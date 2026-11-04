#!/usr/bin/env python3
"""Check the production bounded integer reciprocal divider against exact /.

All font divisors 1..256 are exercised at every coordinate through 4096,
quotient boundaries across the full page-size bound, and 100,000 deterministic
random inputs. Only the OpenCL mul_hi builtin receives a host implementation.
"""

from pathlib import Path
import subprocess
import tempfile

from test_edit_hex_shader import KERNEL, c_function
from test_edit_life_math import compiler_path


DRIVER = r'''
#include <cassert>
#include <cstdint>
#include <cstdio>
using uint = std::uint32_t;
static inline uint mul_hi(uint a, uint b) { return static_cast<uint>((std::uint64_t(a) * b) >> 32); }
@HELPER@
int main() {
    constexpr uint bound = 4096u * 2160u;
    std::uint64_t cases = 0;
    auto check = [&](uint n, uint d) {
        assert(n <= bound && d >= 1 && d <= 256);
        assert(std::uint64_t(n) * (d - 1u) < (std::uint64_t(1) << 32));
        uint magic = static_cast<uint>(((std::uint64_t(1) << 32) + d - 1u) / d);
        assert(fast_cell_div(n, d, magic) == n / d);
        ++cases;
    };
    for (uint d = 1; d <= 256; ++d) {
        for (uint n = 0; n <= 4096; ++n) check(n, d);
        for (uint n = bound - 8; n <= bound; ++n) check(n, d);
        const uint quotients[] = {0, 1, 2, 3, 6, 7, 8, 15, 16, 31, 32, 255, 256,
            1023, 1024, (bound / d) / 2, bound / d - 1, bound / d};
        for (uint q : quotients) {
            uint multiple = q * d;
            if (multiple > 0) check(multiple - 1u, d);
            check(multiple, d);
            if (multiple < bound) check(multiple + 1u, d);
        }
    }
    uint random = 0x15c011abu;
    auto next = [&] { random ^= random << 13; random ^= random >> 17; random ^= random << 5; return random; };
    for (uint i = 0; i < 100000; ++i) {
        uint n = next() % (bound + 1u), d = next() % 256u + 1u;
        check(n, d);
    }
    std::printf("Production reciprocal divider exact: %llu cases; divisors1..256, n<=%u\n",
        static_cast<unsigned long long>(cases), bound);
}
'''


def main() -> None:
    helper = c_function(KERNEL.read_text(), "fast_cell_div")
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-division-") as temporary:
        directory = Path(temporary)
        source, binary = directory / "division.cpp", directory / "division"
        source.write_text(DRIVER.replace("@HELPER@", helper))
        subprocess.run([compiler_path(None), "-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror",
            "-fsanitize=address,undefined", "-fno-sanitize-recover=all", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
