#include <immintrin.h>
#include <stdint.h>

__attribute__((noinline))
__m256 lfm_q8_block_probe(
    __m256 accumulator,
    const uint8_t *magnitudes,
    const int8_t *activation_signs,
    const int8_t *weights,
    float combined_scale) {
    const __m256i magnitude = _mm256_loadu_si256((const __m256i *)magnitudes);
    const __m256i signs = _mm256_loadu_si256((const __m256i *)activation_signs);
    const __m256i weight = _mm256_loadu_si256((const __m256i *)weights);
    const __m256i signed_weight = _mm256_sign_epi8(weight, signs);
    const __m256i dots = _mm256_dpbusd_avx_epi32(
        _mm256_setzero_si256(), magnitude, signed_weight);
    return _mm256_fmadd_ps(
        _mm256_cvtepi32_ps(dots), _mm256_set1_ps(combined_scale), accumulator);
}
