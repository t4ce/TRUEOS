#include "../../crates/trueos-shader/cpu/kernels/include/trueos_arm_kernels.h"

int main()
{
    constexpr trueos_arm_u32 source_pitch = 9u;
    constexpr trueos_arm_u32 destination_pitch = 11u;
    constexpr trueos_arm_u32 rows = 7u;
    constexpr trueos_arm_u32 untouched = 0xDEADBEEFu;
    trueos_arm_u32 source[source_pitch * rows] = {};
    trueos_arm_u32 destination[destination_pitch * rows] = {};

    for (trueos_arm_u32 index = 0; index < source_pitch * rows; ++index) {
        source[index] = 0xA0000000u + index;
    }
    for (trueos_arm_u32 index = 0; index < destination_pitch * rows; ++index) {
        destination[index] = untouched;
    }

    trueos_arm_copy_rect_rgba8(
        source,
        destination,
        source_pitch * 4u,
        destination_pitch * 4u,
        2u,
        1u,
        3u,
        2u,
        5u,
        3u);

    for (trueos_arm_u32 y = 0; y < rows; ++y) {
        for (trueos_arm_u32 x = 0; x < destination_pitch; ++x) {
            const bool inside = y >= 2u && y < 5u && x >= 3u && x < 8u;
            const trueos_arm_u32 observed = destination[y * destination_pitch + x];
            if (inside) {
                const trueos_arm_u32 source_x = 2u + (x - 3u);
                const trueos_arm_u32 source_y = 1u + (y - 2u);
                const trueos_arm_u32 expected =
                    source[source_y * source_pitch + source_x];
                if (observed != expected) {
                    return 1;
                }
            } else if (observed != untouched) {
                return 2;
            }
        }
    }

    trueos_arm_copy_rect_rgba8(
        nullptr,
        destination,
        source_pitch * 4u,
        destination_pitch * 4u,
        0u,
        0u,
        0u,
        0u,
        1u,
        1u);
    trueos_arm_copy_rect_rgba8(
        source,
        nullptr,
        source_pitch * 4u,
        destination_pitch * 4u,
        0u,
        0u,
        0u,
        0u,
        1u,
        1u);
    return 0;
}
