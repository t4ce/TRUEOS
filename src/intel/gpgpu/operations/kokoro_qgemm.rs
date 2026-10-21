















const fn kokoro_qgemm_admitted_shape(reduction_values: u32, output_columns: u32) -> bool {
    match reduction_values {
        128 => matches!(output_columns, 256 | 512 | 768 | 1_024 | 1_028 | 2_048 | 2_180),
        512 => output_columns == 50,
        768 => matches!(output_columns, 512 | 768 | 2_048),
        2_048 => output_columns == 768,
        _ => false,
    }
}

const _: () = {
    assert!(kokoro_qgemm_admitted_shape(128, 256));
    assert!(kokoro_qgemm_admitted_shape(128, 512));
    assert!(kokoro_qgemm_admitted_shape(128, 768));
    assert!(kokoro_qgemm_admitted_shape(128, 1_024));
    assert!(kokoro_qgemm_admitted_shape(128, 1_028));
    assert!(kokoro_qgemm_admitted_shape(128, 2_048));
    assert!(kokoro_qgemm_admitted_shape(128, 2_180));
    assert!(kokoro_qgemm_admitted_shape(512, 50));
    assert!(kokoro_qgemm_admitted_shape(768, 512));
    assert!(kokoro_qgemm_admitted_shape(768, 768));
    assert!(kokoro_qgemm_admitted_shape(768, 2_048));
    assert!(kokoro_qgemm_admitted_shape(2_048, 768));
    assert!(!kokoro_qgemm_admitted_shape(128, 50));
    assert!(!kokoro_qgemm_admitted_shape(256, 768));
    assert!(!kokoro_qgemm_admitted_shape(2_048, 2_048));
};

#[cfg(test)]
mod kokoro_qgemm_tests {
    use alloc::vec;

    use super::*;

    fn requirements(k: u32, n: u32) -> KokoroQgemmRequirements {
        let spec = KokoroQgemmSpec::contiguous(512, k, n, 127, 0.01).unwrap();
        let packed_words = n.div_ceil(16) as usize * (k / 4) as usize * 16;
        kokoro_qgemm_validate_call(
            spec,
            packed_words,
            n as usize,
            n as usize,
            512 * (k / 4) as usize,
            Some(n as usize),
            512 * n as usize,
        )
        .unwrap()
    }

    #[test]
    fn admits_only_the_twelve_model_projection_shapes() {
        let admitted = [
            (128, 256),
            (128, 512),
            (128, 768),
            (128, 1_024),
            (128, 1_028),
            (128, 2_048),
            (128, 2_180),
            (512, 50),
            (768, 512),
            (768, 768),
            (768, 2_048),
            (2_048, 768),
        ];
        for (k, n) in admitted {
            assert!(kokoro_qgemm_admitted_shape(k, n));
            let _ = requirements(k, n);
        }
        assert!(!kokoro_qgemm_admitted_shape(128, 50));
        assert!(!kokoro_qgemm_admitted_shape(256, 768));
        assert!(!kokoro_qgemm_admitted_shape(2_048, 2_048));
    }

    #[test]
    fn partial_simd16_tile_has_exact_weight_padding_and_mask() {
        let requirements = requirements(128, 2_180);
        assert_eq!(requirements.group_x, 137);
        assert_eq!(requirements.right_mask, 0x0000_000f);
        assert_eq!(requirements.packed_weight_words, 137 * 32 * 16);
    }

    #[test]
    fn largest_reduction_fits_the_persistent_weight_region_exactly() {
        let requirements = requirements(2_048, 768);
        assert_eq!(
            requirements.packed_weight_words * core::mem::size_of::<u32>(),
            KOKORO_QGEMM_PACKED_WEIGHTS_ALLOC_BYTES
        );
    }

    #[test]
    fn onnx_kn_packer_zeroes_tail_n_lanes_and_computes_signed_sums() {
        const K: usize = 512;
        const N: usize = 50;
        let mut weights = vec![0i8; K * N];
        for k in 0..K {
            for n in 0..N {
                weights[k * N + n] = ((k * 3 + n * 5) % 127) as i8 - 63;
            }
        }
        let reduction_words = K / 4;
        let tiles = N.div_ceil(16);
        let mut packed = vec![u32::MAX; tiles * reduction_words * 16];
        let mut sums = vec![i32::MAX; N];
        kokoro_qgemm_pack_onnx_weights(K as u32, N as u32, &weights, &mut packed, &mut sums)
            .unwrap();

        for n in 0..N {
            let expected_sum = (0..K).map(|k| i32::from(weights[k * N + n])).sum();
            assert_eq!(sums[n], expected_sum);
            for word in 0..reduction_words {
                let packed_word = packed[((n / 16) * reduction_words + word) * 16 + n % 16];
                for byte in 0..4usize {
                    assert_eq!(
                        ((packed_word >> (byte * 8)) & 0xff) as u8,
                        weights[(word * 4 + byte) * N + n] as u8
                    );
                }
            }
        }
        for n in N..tiles * 16 {
            for word in 0..reduction_words {
                assert_eq!(packed[((n / 16) * reduction_words + word) * 16 + n % 16], 0);
            }
        }
    }
}
