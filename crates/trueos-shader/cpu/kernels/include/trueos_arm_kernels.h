#ifndef TRUEOS_ARM_KERNELS_H
#define TRUEOS_ARM_KERNELS_H

// Freestanding AAPCS64 entry contracts for the optional AArch64 CPU backend.
// This header deliberately has no libc or C++ standard-library dependency.

typedef __UINT32_TYPE__ trueos_arm_u32;

#ifdef __cplusplus
static_assert(sizeof(trueos_arm_u32) == 4u);
extern "C" {
#else
_Static_assert(sizeof(trueos_arm_u32) == 4u, "trueos_arm_u32 must be 32-bit");
#endif

void trueos_arm_copy_rect_rgba8(
    const trueos_arm_u32 *source_rgba,
    trueos_arm_u32 *destination_rgba,
    trueos_arm_u32 source_pitch_bytes,
    trueos_arm_u32 destination_pitch_bytes,
    trueos_arm_u32 source_x,
    trueos_arm_u32 source_y,
    trueos_arm_u32 destination_x,
    trueos_arm_u32 destination_y,
    trueos_arm_u32 width,
    trueos_arm_u32 height);

#ifdef __cplusplus
}
#endif

#endif
