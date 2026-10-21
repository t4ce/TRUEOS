/// Exact stride-one contract for the dominant Kokoro `ConvInteger` family.
///
/// Input and output tensors use standard batch-one NCW/NMW layout. Weights
/// are standard ONNX MCK before warm-job packing. The focused GPU lane admits
/// C=M in {128, 256}, K in {3, 7, 11}, dilation in {1, 3, 5}, group one, and
/// symmetric padding that preserves the temporal length.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KokoroConv1dSpec {
    pub(crate) input_length: u32,
    pub(crate) input_channels: u32,
    pub(crate) output_channels: u32,
    pub(crate) kernel_size: u32,
    pub(crate) dilation: u32,
    pub(crate) pad_left: u32,
    pub(crate) activation_zero_point: u8,
    pub(crate) weight_zero_point: u8,
#[derive(Copy, Clone)]
struct KokoroConv1dArena {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}
}

impl KokoroConv1dSpec {
    pub(crate) const fn dominant(
        input_length: u32,
        channels: u32,
        kernel_size: u32,
        dilation: u32,
        activation_zero_point: u8,
        weight_zero_point: u8,
    ) -> Option<Self> {
        if !kokoro_conv1d_admitted_weights(channels, channels, kernel_size)
            || !matches!(dilation, 1 | 3 | 5)
        {
            return None;
        }
        let pad_left = dilation * ((kernel_size - 1) / 2);
        let spec = Self {
            input_length,
            input_channels: channels,
            output_channels: channels,
            kernel_size,
            dilation,
            pad_left,
            activation_zero_point,
            weight_zero_point,
        };
        if kokoro_conv1d_admitted_spec(spec) {
            Some(spec)
        } else {
            None
        }
    }
}







unsafe impl Send for KokoroConv1dArena {}
unsafe impl Sync for KokoroConv1dArena {}

impl KokoroConv1dArena {
    const fn gpu_at(self, offset: usize) -> u64 {
        self.gpu + offset as u64
    }

    unsafe fn virt_at(self, offset: usize) -> *mut u8 {
        unsafe { self.virt.add(offset) }
    }
}



#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct KokoroConv1dTile {
    output_base: u32,
    tile_length: u32,
    activation_origin: u32,
    activation_rows: u32,
}



static KOKORO_CONV1D_ARENA: Mutex<Option<KokoroConv1dArena>> = Mutex::new(None);
static KOKORO_CONV1D_CALLS: AtomicU64 = AtomicU64::new(0);

















const fn kokoro_conv1d_admitted_weights(
    input_channels: u32,
    output_channels: u32,
    kernel_size: u32,
) -> bool {
    input_channels == output_channels
        && matches!(input_channels, 128 | 256)
        && matches!(kernel_size, 3 | 7 | 11)
}

const fn kokoro_conv1d_admitted_spec(spec: KokoroConv1dSpec) -> bool {
    spec.input_length > 0
        && spec.input_length as usize <= KOKORO_CONV1D_MAX_INPUT_TIMES
        && kokoro_conv1d_admitted_weights(
            spec.input_channels,
            spec.output_channels,
            spec.kernel_size,
        )
        && matches!(spec.dilation, 1 | 3 | 5)
        && spec.pad_left == spec.dilation * ((spec.kernel_size - 1) / 2)
}

const fn kokoro_conv1d_tile(
    spec: KokoroConv1dSpec,
    output_base: u32,
    tile_length: u32,
) -> Option<KokoroConv1dTile> {
    if !kokoro_conv1d_admitted_spec(spec)
        || tile_length == 0
        || tile_length as usize > KOKORO_CONV1D_MAX_TILE_TIMES
        || output_base >= spec.input_length
        || tile_length > spec.input_length - output_base
    {
        return None;
    }
    let radius = spec.pad_left;
    let activation_origin = output_base.saturating_sub(radius);
    let output_end = output_base + tile_length;
    let extended_end = output_end.saturating_add(radius);
    let activation_end = if extended_end < spec.input_length {
        extended_end
    } else {
        spec.input_length
    };
    Some(KokoroConv1dTile {
        output_base,
        tile_length,
        activation_origin,
        activation_rows: activation_end - activation_origin,
    })
}

const _: () = {
    let hotspot = KokoroConv1dSpec::dominant(8_281, 128, 11, 5, 96, 61).unwrap();
    assert!(hotspot.pad_left == 25);
    let first = kokoro_conv1d_tile(hotspot, 0, 8_192).unwrap();
    assert!(first.activation_origin == 0 && first.activation_rows == 8_217);
    let second = kokoro_conv1d_tile(hotspot, 8_192, 89).unwrap();
    assert!(second.activation_origin == 8_167 && second.activation_rows == 114);
};

#[cfg(test)]
mod kokoro_conv1d_tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn admits_exact_dominant_stride_one_family() {
        for channels in [128, 256] {
            for kernel in [3, 7, 11] {
                for dilation in [1, 3, 5] {
                    assert!(
                        KokoroConv1dSpec::dominant(8_281, channels, kernel, dilation, 96, 61)
                            .is_some()
                    );
                }
            }
        }
        assert!(KokoroConv1dSpec::dominant(8_281, 512, 11, 5, 96, 61).is_none());
        assert!(KokoroConv1dSpec::dominant(8_281, 128, 5, 1, 96, 61).is_none());
        assert!(KokoroConv1dSpec::dominant(8_281, 128, 11, 2, 96, 61).is_none());
    }

    #[test]
    fn onnx_mck_packer_round_trips_and_builds_per_tap_sums() {
        const C: usize = 128;
        const M: usize = 128;
        const K: usize = 3;
        let mut weights = vec![0u8; M * C * K];
        for m in 0..M {
            for c in 0..C {
                for k in 0..K {
                    weights[(m * C + c) * K + k] = ((m * 7 + c * 3 + k * 11) & 0xff) as u8;
                }
            }
        }
        let mut packed = vec![0u32; M * C * K / 4];
        let mut sums = vec![0u32; M * K];
        kokoro_conv1d_pack_onnx_weights(
            C as u32,
            M as u32,
            K as u32,
            &weights,
            &mut packed,
            &mut sums,
        )
        .unwrap();

        for m in 0..M {
            for k in 0..K {
                let expected_sum: u32 = (0..C)
                    .map(|c| u32::from(weights[(m * C + c) * K + k]))
                    .sum();
                assert_eq!(sums[k * M + m], expected_sum);
                for c in 0..C {
                    let word = c / 4;
                    let packed_word = packed[(((m / 16) * K + k) * (C / 4) + word) * 16 + m % 16];
                    assert_eq!(
                        ((packed_word >> ((c % 4) * 8)) & 0xff) as u8,
                        weights[(m * C + c) * K + k]
                    );
                }
            }
        }
    }
}
