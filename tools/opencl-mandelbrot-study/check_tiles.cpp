#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <vector>

using uint = uint32_t;
static size_t group_id, local_id, local_size, global_x, global_y;
static size_t get_group_id(uint) { return group_id; }
static size_t get_local_id(uint) { return local_id; }
static size_t get_local_size(uint) { return local_size; }
static size_t get_global_id(uint axis) { return axis == 0 ? global_x : global_y; }
#define __kernel
#define __global
#define __private
namespace flat {
#include "mandelbrot.cl"
}
namespace recursive {
#define RECURSIVE_PROBE
#include "mandelbrot_tiles.cl"
#undef RECURSIVE_PROBE
}
namespace stack {
#include "mandelbrot_tiles.cl"
}
namespace parallel {
#define PARALLEL_LEAVES
#include "mandelbrot_tiles.cl"
#undef PARALLEL_LEAVES
}

using Kernel = void (*)(uint *, uint, uint, float, float, float, uint);
constexpr uint poison = 0xdedededeu;
static void check(uint width, uint height, uint limit) {
    const size_t n = size_t(width) * height;
    const float ox = -2.0f, oy = -1.5f, step = 3.0f / 512.0f;
    std::vector<uint> expected(n, poison);
    for (global_y = 0; global_y < height; ++global_y)
        for (global_x = 0; global_x < width; ++global_x)
            flat::mandelbrot(expected.data(), width, height, ox, oy, step, limit);
    for (const auto &[name, kernel] : std::vector<std::pair<const char *, Kernel>>{
            {"recursive", recursive::mandelbrot}, {"stack", stack::mandelbrot},
            {"parallel", parallel::mandelbrot}}) {
        if (kernel == parallel::mandelbrot && (width != 512 || height != 512)) continue;
        std::vector<uint> result(n + 32, poison);
        local_size = 256;
        const size_t groups = kernel == parallel::mandelbrot ? 64 : 1;
        for (group_id = 0; group_id < groups; ++group_id)
            for (local_id = 0; local_id < local_size; ++local_id)
                kernel(result.data() + 16, width, height, ox, oy, step, limit);
        const bool guard_ok = std::all_of(result.begin(), result.begin() + 16, [](uint x) { return x == poison; })
            && std::all_of(result.end() - 16, result.end(), [](uint x) { return x == poison; });
        if (!guard_ok || !std::equal(expected.begin(), expected.end(), result.begin() + 16)) {
            std::fprintf(stderr, "FAIL %s %ux%u limit=%u\n", name, width, height, limit);
            std::exit(1);
        }
        uint64_t hash = 14695981039346656037ull;
        for (uint word : expected) {
            for (uint byte = 0; byte < 4; ++byte) { hash ^= (word >> (8 * byte)) & 255u; hash *= 1099511628211ull; }
        }
        std::printf("PASS %-9s %3ux%3u limit=%3u hash=%016llx guards=ok\n", name, width, height, limit,
                    (unsigned long long)hash);
    }
}

static void check_partition(uint width, uint height) {
    std::vector<stack::Tile> pending{{0, 0, width, height}};
    std::vector<uint8_t> coverage(size_t(width) * height, 0);
    size_t high_water = 1, leaves = 0, nodes = 0;
    while (!pending.empty()) {
        auto tile = pending.back(); pending.pop_back(); ++nodes;
        if (tile.w <= 64 && tile.h <= 64) {
            ++leaves;
            for (uint y = tile.y; y < tile.y + tile.h; ++y)
                for (uint x = tile.x; x < tile.x + tile.w; ++x)
                    if (x >= width || y >= height || ++coverage[size_t(y) * width + x] != 1)
                        std::abort();
        } else {
            stack::Tile first, second;
            stack::split_tile(tile, &first, &second);
            pending.push_back(second); pending.push_back(first);
            high_water = std::max(high_water, pending.size());
        }
    }
    if (high_water > 7 || !std::all_of(coverage.begin(), coverage.end(), [](uint8_t c){return c == 1;}))
        std::abort();
    if (width == 512 && height == 512) {
        if (nodes != 127 || leaves != 64 || high_water != 7) std::abort();
        std::printf("PASS partition 512x512 nodes=%zu leaves=%zu pending=%zu each_pixel_once=1\n",
                    nodes, leaves, high_water);
    }
}

int main() {
    for (uint w : {1u, 63u, 64u, 65u, 127u, 128u, 129u, 255u, 256u, 257u, 511u, 512u})
        for (uint h : {1u, 63u, 64u, 65u, 127u, 128u, 129u, 255u, 256u, 257u, 511u, 512u})
            check_partition(w, h);
    for (Kernel kernel : {recursive::mandelbrot, stack::mandelbrot, parallel::mandelbrot}) {
        uint unchanged[32]; std::fill_n(unchanged, 32, poison);
        group_id = local_id = 0; local_size = 256;
        for (auto [w, h] : {std::pair{0u,512u}, {512u,0u}, {513u,512u}, {512u,513u}})
            kernel(unchanged, w, h, -2.0f, -1.5f, 3.0f/512.0f, 64);
        if (!std::all_of(unchanged, unchanged + 32, [](uint c){return c == poison;})) std::abort();
    }

    check(512, 512, 64);
    check(512, 512, 0);
    check(512, 512, 1);
    check(1, 1, 64);
    check(64, 64, 64);
    check(65, 1, 64);
    check(1, 65, 64);
    check(65, 65, 64);
    check(511, 509, 64);
    check(512, 17, 64);
    std::puts("CPU source simulation only: this does not execute the generated EU binaries.");
}
