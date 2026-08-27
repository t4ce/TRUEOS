#include <cpuid.h>
#include <immintrin.h>
#include <stdint.h>
#include <stdio.h>

static uint64_t read_xcr0(void) {
    uint32_t low;
    uint32_t high;
    __asm__ volatile("xgetbv" : "=a"(low), "=d"(high) : "c"(0));
    return ((uint64_t)high << 32) | low;
}

static int avx_vnni_ready(void) {
    unsigned eax;
    unsigned ebx;
    unsigned ecx;
    unsigned edx;
    if (!__get_cpuid(1, &eax, &ebx, &ecx, &edx)) {
        return 0;
    }
    const unsigned fma = 1u << 12;
    const unsigned osxsave = 1u << 27;
    const unsigned avx = 1u << 28;
    if ((ecx & (fma | osxsave | avx)) != (fma | osxsave | avx)) {
        return 0;
    }
    if ((read_xcr0() & 0x6u) != 0x6u) {
        return 0;
    }
    if (__get_cpuid_max(0, 0) < 7) {
        return 0;
    }
    unsigned max_subleaf = 0;
    __cpuid_count(7, 0, max_subleaf, ebx, ecx, edx);
    if ((ebx & (1u << 5)) == 0 || max_subleaf < 1) {
        return 0;
    }
    __cpuid_count(7, 1, eax, ebx, ecx, edx);
    return (eax & (1u << 4)) != 0;
}

__attribute__((target("avx2,avxvnni,fma"), noinline))
static int run_probe(void) {
    int8_t activation[32];
    int8_t weight[32];
    uint8_t magnitude[32];
    int32_t expected[8] = {0};
    int32_t observed[8] = {0};

    for (int index = 0; index < 32; ++index) {
        activation[index] = (int8_t)(((index * 37 + 11) % 255) - 127);
        weight[index] = (int8_t)(((index * 53 + 7) % 255) - 127);
        const int q = activation[index];
        magnitude[index] = (uint8_t)(q < 0 ? -q : q);
        expected[index / 4] += q * (int)weight[index];
    }

    const __m256i q = _mm256_loadu_si256((const __m256i *)activation);
    const __m256i w = _mm256_loadu_si256((const __m256i *)weight);
    const __m256i abs_q = _mm256_loadu_si256((const __m256i *)magnitude);
    const __m256i signed_w = _mm256_sign_epi8(w, q);
    const __m256i dots = _mm256_dpbusd_avx_epi32(
        _mm256_setzero_si256(), abs_q, signed_w);
    _mm256_storeu_si256((__m256i *)observed, dots);

    for (int lane = 0; lane < 8; ++lane) {
        if (observed[lane] != expected[lane]) {
            fprintf(stderr, "lane %d: observed=%d expected=%d\n",
                    lane, observed[lane], expected[lane]);
            return 1;
        }
    }
    return 0;
}

int main(void) {
    if (!avx_vnni_ready()) {
        puts("AVX-VNNI runtime arithmetic probe: SKIP (unsupported host)");
        return 0;
    }
    if (run_probe() != 0) {
        puts("AVX-VNNI runtime arithmetic probe: FAIL");
        return 1;
    }
    puts("AVX-VNNI runtime arithmetic probe: PASS");
    return 0;
}
