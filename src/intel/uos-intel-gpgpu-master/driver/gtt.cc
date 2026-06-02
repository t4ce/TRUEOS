#include "gtt.h"

uint32_t roundUp(uint32_t x, uint32_t y)
{
    uint32_t div = x / y;
    return x % y == 0 ? div : ++div;
}
