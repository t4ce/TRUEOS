#include <stdint.h>
#include <stdio.h>

static uint64_t append(uint64_t hash, uint32_t pixel) {
    for (unsigned i = 0; i < 4; ++i)
        hash = (hash ^ ((pixel >> (i * 8)) & 255)) * UINT64_C(1099511628211);
    return hash;
}

static uint64_t mandelbrot(unsigned iterations) {
    uint64_t hash = UINT64_C(14695981039346656037);
    const float step = 3.0f / 320.0f;
    for (unsigned y = 0; y < 240; ++y) {
        for (unsigned x = 0; x < 320; ++x) {
            float cx = -2.0f + (float)x * step;
            float cy = -1.125f + (float)y * step;
            float zx = 0, zy = 0, depth = 0;
            for (unsigned i = 0; i < iterations; ++i) {
                float nx = zx * zx - zy * zy + cx;
                float ny = 2.0f * zx * zy + cy;
                zx = nx;
                zy = ny;
                depth += 1;
                if (zx * zx + zy * zy > 4.0f) break;
            }
            float shade = depth * 0.04f;
            uint32_t gray = (uint32_t)(shade * 255.0f);
            hash = append(hash, UINT32_C(0xff000000) | gray | gray << 8 | gray << 16);
        }
    }
    return hash;
}

int main(void) {
    uint64_t smoke = UINT64_C(14695981039346656037);
    for (unsigned i = 0; i < 64 * 64; ++i) smoke = append(smoke, UINT32_C(0xff7f7f7f));
    printf("{\"smoke\":\"%016llx\",\"iterations_25\":\"%016llx\",\"iterations_16\":\"%016llx\"}\n",
           (unsigned long long)smoke, (unsigned long long)mandelbrot(25),
           (unsigned long long)mandelbrot(16));
    return 0;
}
