#ifndef PARSEGEN_H
#define PARSEGEN_H

#include <stdint.h>

#include "gpgpu_driver.h"

struct kernel_config;

struct CrossThreadData_info
{
    /// Offset for workgroup dimensions
    // uint32_t workOffset[3]; // not used atm

    /// dimensions of workgroup
    uint32_t workDim[3];

    /// dimensions of workgroup (seems to be always the same as workDim)
    uint32_t enqueuedWorkDim[3];

#ifdef DEBUG
    /// error code (0 = success)
    uint8_t err = 0;
#endif // DEBUG
};

/**
 * @brief parses a GEN Binary: sets SIMD Mode, useBarrier flag and copies instructions
 *
 * @param info info struct for cross-thread-data structure
 * @param kconf the kernel struct
 * @param instr pointer to instruction memory
 */
extern "C" void parseGEN(CrossThreadData_info &info, kernel_config &kconf, uint8_t *instr);

#endif /* !PARSEGEN_H */
