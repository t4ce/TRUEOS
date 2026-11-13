//! Shared raw AVX-VNNI operations: four instruction families at 128/256 bits.
//!
//! Reconstructed from the eight entry points documented by
//! `trueos-lumen-cpu-vnni/reference/vnni_factorio_demo.html`; the original
//! `vnni.rs` was not recovered from Git history. This file uses only `core`
//! and can also be included by host experiments without linking the kernel.
//!
//! These wrappers do not detect CPU support or enable register state. Callers
//! own that admission check. They expose VEX AVX-VNNI, not AVX-512 variants.

use core::arch::x86_64::*;

/// Accumulate four unsigned-byte × signed-byte products into each of 4 i32 lanes.
/// Wraps each lane modulo 2³².
///
/// # Safety
/// The executing CPU must support AVX2 and AVX-VNNI, and the OS must enable
/// and preserve XMM/YMM state (including across interrupts and task switches).
#[inline]
#[target_feature(enable = "avx2,avxvnni")]
pub unsafe fn dpbusd_128(acc: __m128i, a: __m128i, b: __m128i) -> __m128i {
    _mm_dpbusd_avx_epi32(acc, a, b)
}

/// Accumulate four unsigned-byte × signed-byte products into each of 4 i32 lanes.
/// Saturates each lane to the i32 range.
///
/// # Safety
/// The executing CPU must support AVX2 and AVX-VNNI, and the OS must enable
/// and preserve XMM/YMM state (including across interrupts and task switches).
#[inline]
#[target_feature(enable = "avx2,avxvnni")]
pub unsafe fn dpbusds_128(acc: __m128i, a: __m128i, b: __m128i) -> __m128i {
    _mm_dpbusds_avx_epi32(acc, a, b)
}

/// Accumulate two signed-word × signed-word products into each of 4 i32 lanes.
/// Wraps each lane modulo 2³².
///
/// # Safety
/// The executing CPU must support AVX2 and AVX-VNNI, and the OS must enable
/// and preserve XMM/YMM state (including across interrupts and task switches).
#[inline]
#[target_feature(enable = "avx2,avxvnni")]
pub unsafe fn dpwssd_128(acc: __m128i, a: __m128i, b: __m128i) -> __m128i {
    _mm_dpwssd_avx_epi32(acc, a, b)
}

/// Accumulate two signed-word × signed-word products into each of 4 i32 lanes.
/// Saturates each lane to the i32 range.
///
/// # Safety
/// The executing CPU must support AVX2 and AVX-VNNI, and the OS must enable
/// and preserve XMM/YMM state (including across interrupts and task switches).
#[inline]
#[target_feature(enable = "avx2,avxvnni")]
pub unsafe fn dpwssds_128(acc: __m128i, a: __m128i, b: __m128i) -> __m128i {
    _mm_dpwssds_avx_epi32(acc, a, b)
}

/// Accumulate four unsigned-byte × signed-byte products into each of 8 i32 lanes.
/// Wraps each lane modulo 2³².
///
/// # Safety
/// The executing CPU must support AVX2 and AVX-VNNI, and the OS must enable
/// and preserve XMM/YMM state (including across interrupts and task switches).
#[inline]
#[target_feature(enable = "avx2,avxvnni")]
pub unsafe fn dpbusd_256(acc: __m256i, a: __m256i, b: __m256i) -> __m256i {
    _mm256_dpbusd_avx_epi32(acc, a, b)
}

/// Accumulate four unsigned-byte × signed-byte products into each of 8 i32 lanes.
/// Saturates each lane to the i32 range.
///
/// # Safety
/// The executing CPU must support AVX2 and AVX-VNNI, and the OS must enable
/// and preserve XMM/YMM state (including across interrupts and task switches).
#[inline]
#[target_feature(enable = "avx2,avxvnni")]
pub unsafe fn dpbusds_256(acc: __m256i, a: __m256i, b: __m256i) -> __m256i {
    _mm256_dpbusds_avx_epi32(acc, a, b)
}

/// Accumulate two signed-word × signed-word products into each of 8 i32 lanes.
/// Wraps each lane modulo 2³².
///
/// # Safety
/// The executing CPU must support AVX2 and AVX-VNNI, and the OS must enable
/// and preserve XMM/YMM state (including across interrupts and task switches).
#[inline]
#[target_feature(enable = "avx2,avxvnni")]
pub unsafe fn dpwssd_256(acc: __m256i, a: __m256i, b: __m256i) -> __m256i {
    _mm256_dpwssd_avx_epi32(acc, a, b)
}

/// Accumulate two signed-word × signed-word products into each of 8 i32 lanes.
/// Saturates each lane to the i32 range.
///
/// # Safety
/// The executing CPU must support AVX2 and AVX-VNNI, and the OS must enable
/// and preserve XMM/YMM state (including across interrupts and task switches).
#[inline]
#[target_feature(enable = "avx2,avxvnni")]
pub unsafe fn dpwssds_256(acc: __m256i, a: __m256i, b: __m256i) -> __m256i {
    _mm256_dpwssds_avx_epi32(acc, a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish(acc: i32, dot: i64, saturating: bool) -> i32 {
        let value = i64::from(acc) + dot;
        if saturating {
            value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
        } else {
            value as i32
        }
    }

    // Nonzero accumulators, mixed signs, full-range operands and overflowing
    // lanes distinguish wrapping from saturation and byte from word grouping.
    #[target_feature(enable = "avx2,avxvnni")]
    unsafe fn check_all() {
        let acc = [
            i32::MAX - 1,
            i32::MIN + 1,
            17,
            -19,
            i32::MAX - 1,
            i32::MIN + 1,
            23,
            -29,
        ];
        let mut bytes_a = [0_u8; 32];
        let mut bytes_b = [0_i8; 32];
        let mut words_a = [0_i16; 16];
        let mut words_b = [0_i16; 16];
        for lane in 0..8 {
            for j in 0..4 {
                bytes_a[lane * 4 + j] = [255, 200, 0, 137][j];
                bytes_b[lane * 4 + j] = if lane % 2 == 0 {
                    [127, 64, -128, 1][j]
                } else {
                    [-128, -64, 127, -1][j]
                };
            }
            words_a[lane * 2..lane * 2 + 2].fill(i16::MIN);
            words_b[lane * 2..lane * 2 + 2].fill(if lane % 2 == 0 { i16::MIN } else { i16::MAX });
        }
        for width in [128, 256] {
            for word in [false, true] {
                for saturating in [false, true] {
                    let mut got = [0_i32; 8];
                    let a = if word {
                        words_a.as_ptr().cast::<u8>()
                    } else {
                        bytes_a.as_ptr()
                    };
                    let b = if word {
                        words_b.as_ptr().cast::<u8>()
                    } else {
                        bytes_b.as_ptr().cast::<u8>()
                    };
                    // All array sizes match the loads/stores; all calls share
                    // this function's admitted AVX2/AVX-VNNI feature contract.
                    unsafe {
                        if width == 128 {
                            let c = _mm_loadu_si128(acc.as_ptr().cast());
                            let a = _mm_loadu_si128(a.cast());
                            let b = _mm_loadu_si128(b.cast());
                            let r = match (word, saturating) {
                                (false, false) => dpbusd_128(c, a, b),
                                (false, true) => dpbusds_128(c, a, b),
                                (true, false) => dpwssd_128(c, a, b),
                                (true, true) => dpwssds_128(c, a, b),
                            };
                            _mm_storeu_si128(got.as_mut_ptr().cast(), r);
                        } else {
                            let c = _mm256_loadu_si256(acc.as_ptr().cast());
                            let a = _mm256_loadu_si256(a.cast());
                            let b = _mm256_loadu_si256(b.cast());
                            let r = match (word, saturating) {
                                (false, false) => dpbusd_256(c, a, b),
                                (false, true) => dpbusds_256(c, a, b),
                                (true, false) => dpwssd_256(c, a, b),
                                (true, true) => dpwssds_256(c, a, b),
                            };
                            _mm256_storeu_si256(got.as_mut_ptr().cast(), r);
                        }
                    }
                    for lane in 0..width / 32 {
                        let dot = if word {
                            (0..2)
                                .map(|j| {
                                    i64::from(words_a[lane * 2 + j])
                                        * i64::from(words_b[lane * 2 + j])
                                })
                                .sum()
                        } else {
                            (0..4)
                                .map(|j| {
                                    i64::from(bytes_a[lane * 4 + j])
                                        * i64::from(bytes_b[lane * 4 + j])
                                })
                                .sum()
                        };
                        assert_eq!(
                            got[lane],
                            finish(acc[lane], dot, saturating),
                            "width={width} word={word} saturating={saturating} lane={lane}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn all_eight_operations_match_integer_oracles() {
        if !std::is_x86_feature_detected!("avx2") || !std::is_x86_feature_detected!("avxvnni") {
            eprintln!("AVX-VNNI unavailable: hardware test skipped");
            return;
        }
        unsafe {
            check_all();
        }
    }
}
