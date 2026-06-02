#ifndef WGSIZE_H
#define WGSIZE_H

#include <stdint.h>

#define MAX_WORKGROUP_SIZE 256

/**
 * @brief tries to find an optimal workgroupsize
 *
 * @param range the range of the kernel (primenumbers are always bad!)
 * @param simd the simd size of the kernel
 * @return uint32_t the workgroupsize
 */
uint32_t findWorkGroupSize(uint32_t range, uint8_t simd)
{
    // use one workgroup for all ranges less max workgroup size
    if (range <= MAX_WORKGROUP_SIZE)
        return range;

    // find divider with max active channels
    int max = 0;
    int divider = 1;
    for (int i = MAX_WORKGROUP_SIZE; i >= 2; i--)
    {
        // is i a divider of range?
        if (range % i == 0)
        {
            // get amount of active channels or 0 for all active
            int c = i % simd;

            // all channels active?
            if (c == 0)
                return i;

            // get number of workgroups
            int w = range / i;

            // get all active channels
            c *= w;

            // save the divider with most active channels
            if (c > max)
            {
                max = c;
                divider = i;
            }
        }
    }

    return divider;
}

#endif /* !WGSIZE_H */
