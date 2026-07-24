// AArch64 CPU semantic twin of the C++ for OpenCL copy_rect_rgba8 entry.
//
// The GPU entry obtains one two-pixel work item from get_global_id(). This
// AAPCS64 entry owns the complete dispatch and walks the same two-pixel shape
// explicitly. It is freestanding, allocation-free, and has no runtime symbols.

#include "include/trueos_arm_kernels.h"

extern "C" void trueos_arm_copy_rect_rgba8(
    const trueos_arm_u32 *source_rgba,
    trueos_arm_u32 *destination_rgba,
    trueos_arm_u32 source_pitch_bytes,
    trueos_arm_u32 destination_pitch_bytes,
    trueos_arm_u32 source_x,
    trueos_arm_u32 source_y,
    trueos_arm_u32 destination_x,
    trueos_arm_u32 destination_y,
    trueos_arm_u32 width,
    trueos_arm_u32 height)
{
    if (source_rgba == nullptr || destination_rgba == nullptr) {
        return;
    }

    const trueos_arm_u32 source_pitch_pixels = source_pitch_bytes / 4u;
    const trueos_arm_u32 destination_pitch_pixels =
        destination_pitch_bytes / 4u;

    for (trueos_arm_u32 y = 0; y < height; ++y) {
        for (trueos_arm_u32 base_x = 0; base_x < width; base_x += 2u) {
            for (trueos_arm_u32 pixel = 0; pixel < 2u; ++pixel) {
                const trueos_arm_u32 x = base_x + pixel;
                if (x < width) {
                    const trueos_arm_u32 source_index =
                        (source_y + y) * source_pitch_pixels + source_x + x;
                    const trueos_arm_u32 destination_index =
                        (destination_y + y) * destination_pitch_pixels
                        + destination_x + x;
                    destination_rgba[destination_index] =
                        source_rgba[source_index];
                }
            }
        }
    }
}
