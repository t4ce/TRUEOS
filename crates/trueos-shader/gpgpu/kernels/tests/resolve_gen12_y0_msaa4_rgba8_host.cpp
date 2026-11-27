#include <cassert>
#include <cstdint>
#include <vector>

using uint = std::uint32_t;

static uint global_id[2];
static uint get_global_id(uint dimension)
{
    return global_id[dimension];
}

#define __global
#define __kernel
#include "../resolve_gen12_y0_msaa4_rgba8.clcpp"
#undef __global
#undef __kernel

static uint component(uint sample, uint x, uint y, uint channel)
{
    return (97u * sample + 43u * x + 19u * y + 67u * channel + x * y) & 0xffu;
}

static void check_resolve(uint width, uint height, uint tile_columns, uint tile_rows)
{
    const uint src_pitch = tile_columns * 128u;
    const uint slice_bytes = src_pitch * tile_rows * 32u;
    const uint dst_stride = width + 5u;
    const uint guard = 0xdeadbeefu;
    std::vector<uint> src;
    src.reserve(slice_bytes);
    std::vector<uint> dst(dst_stride * (height + 1u), guard);

    // Walk the tiled allocation in physical order without calling the shader
    // address helper. Each tile contains eight 16-byte-wide OWord columns.
    for (uint sample = 0; sample < 4; ++sample) {
        for (uint tile_y = 0; tile_y < tile_rows; ++tile_y) {
            for (uint tile_x = 0; tile_x < tile_columns; ++tile_x) {
                for (uint column = 0; column < 8; ++column) {
                    for (uint row = 0; row < 32; ++row) {
                        for (uint pixel = 0; pixel < 4; ++pixel) {
                            const uint x = tile_x * 32u + column * 4u + pixel;
                            const uint y = tile_y * 32u + row;
                            uint word = 0;
                            for (uint channel = 0; channel < 4; ++channel) {
                                word |= component(sample, x, y, channel) << (8u * channel);
                            }
                            src.push_back(word);
                        }
                    }
                }
            }
        }
    }

    for (const uint swap : {0u, 1u}) {
        for (uint y = 0; y <= height; ++y) {
            for (uint x = 0; x < dst_stride; ++x) {
                global_id[0] = x;
                global_id[1] = y;
                resolve_gen12_y0_msaa4_rgba8(
                    src.data(), dst.data(), src_pitch, dst_stride * 4u,
                    slice_bytes, swap, width, height);

                uint expected = guard;
                if (x < width && y < height) {
                    expected = 0;
                    for (uint channel = 0; channel < 4; ++channel) {
                        uint sum = 0;
                        for (uint sample = 0; sample < 4; ++sample) {
                            sum += component(sample, x, y, channel);
                        }
                        const uint output_channel = swap != 0 && channel % 2u == 0
                            ? 2u - channel : channel;
                        expected |= ((sum + 2u) / 4u) << (8u * output_channel);
                    }
                }
                assert(dst[y * dst_stride + x] == expected);
            }
        }
    }
}

int main()
{
    assert(y0_pixel_offset(0, 0, 256) == 0);
    assert(y0_pixel_offset(1, 0, 256) == 4);
    assert(y0_pixel_offset(4, 0, 256) == 512);
    assert(y0_pixel_offset(31, 31, 256) == 4092);
    assert(y0_pixel_offset(32, 0, 256) == 4096);
    assert(y0_pixel_offset(0, 32, 256) == 8192);
    assert(y0_pixel_offset(32, 32, 256) == 12288);
    check_resolve(1, 1, 1, 1);
    check_resolve(37, 36, 3, 3);
    check_resolve(640, 560, 20, 18);
}
