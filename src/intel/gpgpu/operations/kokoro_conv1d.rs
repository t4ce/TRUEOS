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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KokoroConv1dError {
    UnsupportedTarget,
    InvalidShape,
    Allocation,
    RuntimeUnavailable,
    MappingFailed,
    EncodeFailed,
    SubmitFailed,
    CompletionTimeout,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct KokoroConv1dResult {
    pub(crate) input_length: u32,
    pub(crate) channels: u32,
    pub(crate) tile_dispatches: u32,
    pub(crate) marker: u32,
    pub(crate) submit_ms: u64,
}

#[derive(Copy, Clone)]
struct KokoroConv1dArena {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
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
struct KokoroConv1dRequirements {
    packed_weight_words: usize,
    weight_tap_sums: usize,
    tensor_elements: usize,
    tile_dispatches: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct KokoroConv1dTile {
    output_base: u32,
    tile_length: u32,
    activation_origin: u32,
    activation_rows: u32,
}

#[derive(Copy, Clone)]
struct KokoroConv1dParams {
    packed_weights_gpu: u64,
    weight_tap_sums_gpu: u64,
    packed_activations_gpu: u64,
    output_gpu: u64,
    packed_weights_bytes: usize,
    weight_tap_sums_bytes: usize,
    packed_activations_bytes: usize,
    output_bytes: usize,
    input_length: u32,
    output_base: u32,
    tile_length: u32,
    activation_origin: u32,
    activation_rows: u32,
    input_channels: u32,
    output_channels: u32,
    kernel_size: u32,
    dilation: u32,
    pad_left: u32,
    activation_zero_point: u32,
    weight_zero_point: u32,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
}

static KOKORO_CONV1D_ARENA: Mutex<Option<KokoroConv1dArena>> = Mutex::new(None);
static KOKORO_CONV1D_CALLS: AtomicU64 = AtomicU64::new(0);

/// Whether the exact Alder Lake-S GT1 ConvInteger artifact can run now.
///
/// Nearby PCI revisions are intentionally rejected: this artifact was baked
/// and inspected only for device 0x4680 revision 0x0c.
pub(crate) fn kokoro_conv1d_u8_u8_supported() -> bool {
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    KOKORO_CONV1D_U8_U8_ADLS_ARTIFACT
        .target_policy
        .supports(dev.device_id, dev.revision_id)
        && !direct_rcs_context_is_quarantined()
}

/// Pack standard ONNX MCK unsigned weights into M16/tap/Cword order.
///
/// The second output records one unsigned C-channel sum for each `[tap, M]`
/// pair. The caller supplies exact-sized buffers, so model warming performs no
/// hidden allocation.
pub(crate) fn kokoro_conv1d_pack_onnx_weights(
    input_channels: u32,
    output_channels: u32,
    kernel_size: u32,
    weights_mck: &[u8],
    packed_weights: &mut [u32],
    weight_tap_sums: &mut [u32],
) -> Result<(), KokoroConv1dError> {
    if !kokoro_conv1d_admitted_weights(input_channels, output_channels, kernel_size) {
        return Err(KokoroConv1dError::InvalidShape);
    }
    let c = input_channels as usize;
    let m = output_channels as usize;
    let k = kernel_size as usize;
    let channel_words = c / 4;
    let output_tiles = m / 16;
    let source_elements = m
        .checked_mul(c)
        .and_then(|elements| elements.checked_mul(k))
        .ok_or(KokoroConv1dError::InvalidShape)?;
    let packed_words = output_tiles
        .checked_mul(k)
        .and_then(|words| words.checked_mul(channel_words))
        .and_then(|words| words.checked_mul(16))
        .ok_or(KokoroConv1dError::InvalidShape)?;
    let sum_elements = k.checked_mul(m).ok_or(KokoroConv1dError::InvalidShape)?;
    if weights_mck.len() != source_elements
        || packed_weights.len() != packed_words
        || weight_tap_sums.len() != sum_elements
    {
        return Err(KokoroConv1dError::InvalidShape);
    }

    weight_tap_sums.fill(0);
    for output_tile in 0..output_tiles {
        for tap in 0..k {
            for word in 0..channel_words {
                for lane in 0..16usize {
                    let output_channel = output_tile * 16 + lane;
                    let mut packed = 0u32;
                    for byte in 0..4usize {
                        let input_channel = word * 4 + byte;
                        let weight = weights_mck[(output_channel * c + input_channel) * k + tap];
                        packed |= u32::from(weight) << (byte * 8);
                        weight_tap_sums[tap * m + output_channel] += u32::from(weight);
                    }
                    packed_weights[((output_tile * k + tap) * channel_words + word) * 16 + lane] =
                        packed;
                }
            }
        }
    }
    Ok(())
}

/// Run one batch-one NCW ConvInteger and return an NMW i32 tensor.
///
/// All ordinary Rust slices remain CPU-owned. Static weights are copied once
/// into a persistent DMA arena; each temporal tile is packed, submitted, fully
/// retired, and transposed back before the next tile reuses that arena.
pub(crate) fn kokoro_conv1d_u8_u8(
    spec: KokoroConv1dSpec,
    packed_weights: &[u32],
    weight_tap_sums: &[u32],
    activations_ncw: &[u8],
    output_nmw: &mut [i32],
) -> Result<KokoroConv1dResult, KokoroConv1dError> {
    let requirements = kokoro_conv1d_validate_call(
        spec,
        packed_weights.len(),
        weight_tap_sums.len(),
        activations_ncw.len(),
        output_nmw.len(),
    )?;
    if !kokoro_conv1d_u8_u8_supported() {
        return Err(KokoroConv1dError::UnsupportedTarget);
    }

    // The system-service direct-RCS context owns one mutable batch/result/ring
    // timeline. Keep the complete multi-tile call serialized and synchronous.
    let _submit_guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let dev = super::claimed_device().ok_or(KokoroConv1dError::RuntimeUnavailable)?;
    if !KOKORO_CONV1D_U8_U8_ADLS_ARTIFACT
        .target_policy
        .supports(dev.device_id, dev.revision_id)
        || direct_rcs_context_is_quarantined()
    {
        return Err(KokoroConv1dError::UnsupportedTarget);
    }
    let arena = kokoro_conv1d_arena_once()?;
    kokoro_conv1d_stage_static(arena, packed_weights, weight_tap_sums);

    let upload =
        upload_kokoro_conv1d_u8_u8_kernel().ok_or(KokoroConv1dError::RuntimeUnavailable)?;
    let state = direct_rcs_state_once(dev).ok_or(KokoroConv1dError::RuntimeUnavailable)?;
    if !direct_rcs_forcewake(dev) {
        return Err(KokoroConv1dError::RuntimeUnavailable);
    }
    if !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
        || !direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        || !direct_rcs_map_ppgtt_kernel(state, arena.gpu, arena.phys, arena.bytes)
    {
        return Err(KokoroConv1dError::MappingFailed);
    }

    let started = direct_rcs_now_tick();
    let mut marker = 0u32;
    for tile_index in 0..requirements.tile_dispatches {
        let output_base = tile_index * KOKORO_CONV1D_MAX_TILE_TIMES as u32;
        let tile_length =
            (spec.input_length - output_base).min(KOKORO_CONV1D_MAX_TILE_TIMES as u32);
        let tile = kokoro_conv1d_tile(spec, output_base, tile_length)
            .ok_or(KokoroConv1dError::InvalidShape)?;
        kokoro_conv1d_stage_activation_tile(arena, spec, tile, activations_ncw);

        let channel_words = spec.input_channels as usize / 4;
        let params = KokoroConv1dParams {
            packed_weights_gpu: arena.gpu_at(KOKORO_CONV1D_PACKED_WEIGHTS_OFFSET_BYTES),
            weight_tap_sums_gpu: arena.gpu_at(KOKORO_CONV1D_WEIGHT_TAP_SUMS_OFFSET_BYTES),
            packed_activations_gpu: arena.gpu_at(KOKORO_CONV1D_ACTIVATIONS_OFFSET_BYTES),
            output_gpu: arena.gpu_at(KOKORO_CONV1D_OUTPUT_OFFSET_BYTES),
            packed_weights_bytes: requirements.packed_weight_words * core::mem::size_of::<u32>(),
            weight_tap_sums_bytes: requirements.weight_tap_sums * core::mem::size_of::<u32>(),
            packed_activations_bytes: tile.activation_rows as usize
                * channel_words
                * core::mem::size_of::<u32>(),
            output_bytes: tile.tile_length as usize
                * spec.output_channels as usize
                * core::mem::size_of::<i32>(),
            input_length: spec.input_length,
            output_base: tile.output_base,
            tile_length: tile.tile_length,
            activation_origin: tile.activation_origin,
            activation_rows: tile.activation_rows,
            input_channels: spec.input_channels,
            output_channels: spec.output_channels,
            kernel_size: spec.kernel_size,
            dilation: spec.dilation,
            pad_left: spec.pad_left,
            activation_zero_point: u32::from(spec.activation_zero_point),
            weight_zero_point: u32::from(spec.weight_zero_point),
            group_x: spec.output_channels / 16,
            group_y: tile.tile_length,
            right_mask: GPGPU_WALKER_SIMD16_MASK,
        };
        if !direct_rcs_encode_kokoro_conv1d_batch(state, upload, params) {
            return Err(KokoroConv1dError::EncodeFailed);
        }
        if !direct_rcs_submit_batch(dev, state) {
            return Err(KokoroConv1dError::SubmitFailed);
        }
        marker = direct_rcs_poll_result_slot_timeout_ms(
            state,
            KOKORO_CONV1D_POST_MARKER_SLOT,
            KOKORO_CONV1D_POST_MARKER,
            KOKORO_CONV1D_COMPLETION_TIMEOUT_MS,
        );
        if marker != KOKORO_CONV1D_POST_MARKER {
            return Err(KokoroConv1dError::CompletionTimeout);
        }
        kokoro_conv1d_copy_output_tile(arena, spec, tile, output_nmw);
    }

    let submit_ms = direct_rcs_elapsed_ms_since(started);
    let calls = KOKORO_CONV1D_CALLS
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if calls == 1 || calls.is_power_of_two() {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: kokoro-conv1d complete calls={} input=[1,{},{}] weights=[{},{},{}] stride=1 dilation={} pads={},{} group=1 tiles={} submit_ms={} marker=0x{:08X} simd=16 layout=m16-tap-cword artifact={} device=0x{:04X} revision=0x{:02X}\n",
            calls,
            spec.input_channels,
            spec.input_length,
            spec.output_channels,
            spec.input_channels,
            spec.kernel_size,
            spec.dilation,
            spec.pad_left,
            spec.pad_left,
            requirements.tile_dispatches,
            submit_ms,
            marker,
            KOKORO_CONV1D_U8_U8_ADLS_ARTIFACT.name,
            dev.device_id,
            dev.revision_id,
        );
    }
    Ok(KokoroConv1dResult {
        input_length: spec.input_length,
        channels: spec.input_channels,
        tile_dispatches: requirements.tile_dispatches,
        marker,
        submit_ms,
    })
}

fn kokoro_conv1d_arena_once() -> Result<KokoroConv1dArena, KokoroConv1dError> {
    let mut slot = KOKORO_CONV1D_ARENA.lock();
    if let Some(arena) = *slot {
        return Ok(arena);
    }
    let (phys, virt) =
        crate::dma::alloc(KOKORO_CONV1D_ARENA_BYTES, 4096).ok_or(KokoroConv1dError::Allocation)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, KOKORO_CONV1D_ARENA_BYTES);
    }
    super::dma_flush(virt, KOKORO_CONV1D_ARENA_BYTES);
    let arena = KokoroConv1dArena {
        phys,
        gpu: KOKORO_CONV1D_ARENA_GPU,
        virt,
        bytes: KOKORO_CONV1D_ARENA_BYTES,
    };
    *slot = Some(arena);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: kokoro-conv1d staging ready phys=0x{:X} gpu=0x{:X} bytes=0x{:X} regions=4 persistent=1 tile_times={}\n",
        arena.phys,
        arena.gpu,
        arena.bytes,
        KOKORO_CONV1D_MAX_TILE_TIMES,
    );
    Ok(arena)
}

fn kokoro_conv1d_stage_static(
    arena: KokoroConv1dArena,
    packed_weights: &[u32],
    weight_tap_sums: &[u32],
) {
    unsafe {
        core::ptr::copy_nonoverlapping(
            packed_weights.as_ptr().cast::<u8>(),
            arena.virt_at(KOKORO_CONV1D_PACKED_WEIGHTS_OFFSET_BYTES),
            core::mem::size_of_val(packed_weights),
        );
        core::ptr::copy_nonoverlapping(
            weight_tap_sums.as_ptr().cast::<u8>(),
            arena.virt_at(KOKORO_CONV1D_WEIGHT_TAP_SUMS_OFFSET_BYTES),
            core::mem::size_of_val(weight_tap_sums),
        );
    }
    super::dma_flush(
        unsafe { arena.virt_at(KOKORO_CONV1D_PACKED_WEIGHTS_OFFSET_BYTES) },
        core::mem::size_of_val(packed_weights),
    );
    super::dma_flush(
        unsafe { arena.virt_at(KOKORO_CONV1D_WEIGHT_TAP_SUMS_OFFSET_BYTES) },
        core::mem::size_of_val(weight_tap_sums),
    );
}

fn kokoro_conv1d_stage_activation_tile(
    arena: KokoroConv1dArena,
    spec: KokoroConv1dSpec,
    tile: KokoroConv1dTile,
    activations_ncw: &[u8],
) {
    let input_length = spec.input_length as usize;
    let channels = spec.input_channels as usize;
    let channel_words = channels / 4;
    let rows = tile.activation_rows as usize;
    let origin = tile.activation_origin as usize;
    let destination = unsafe { arena.virt_at(KOKORO_CONV1D_ACTIVATIONS_OFFSET_BYTES) as *mut u32 };
    for row in 0..rows {
        let source_time = origin + row;
        for word in 0..channel_words {
            let mut packed = 0u32;
            for byte in 0..4usize {
                let channel = word * 4 + byte;
                packed |=
                    u32::from(activations_ncw[channel * input_length + source_time]) << (byte * 8);
            }
            unsafe {
                core::ptr::write(destination.add(row * channel_words + word), packed);
            }
        }
    }
    super::dma_flush(destination.cast::<u8>(), rows * channel_words * core::mem::size_of::<u32>());
}

fn kokoro_conv1d_copy_output_tile(
    arena: KokoroConv1dArena,
    spec: KokoroConv1dSpec,
    tile: KokoroConv1dTile,
    output_nmw: &mut [i32],
) {
    let source = unsafe { arena.virt_at(KOKORO_CONV1D_OUTPUT_OFFSET_BYTES) as *const i32 };
    let output_channels = spec.output_channels as usize;
    let input_length = spec.input_length as usize;
    let tile_length = tile.tile_length as usize;
    let output_base = tile.output_base as usize;
    super::dma_flush(
        source.cast_mut().cast::<u8>(),
        tile_length * output_channels * core::mem::size_of::<i32>(),
    );
    for time in 0..tile_length {
        for channel in 0..output_channels {
            output_nmw[channel * input_length + output_base + time] =
                unsafe { core::ptr::read(source.add(time * output_channels + channel)) };
        }
    }
}

fn kokoro_conv1d_validate_call(
    spec: KokoroConv1dSpec,
    packed_weight_words: usize,
    weight_tap_sum_elements: usize,
    activation_elements: usize,
    output_elements: usize,
) -> Result<KokoroConv1dRequirements, KokoroConv1dError> {
    if !kokoro_conv1d_admitted_spec(spec) {
        return Err(KokoroConv1dError::InvalidShape);
    }
    let length = spec.input_length as usize;
    let c = spec.input_channels as usize;
    let m = spec.output_channels as usize;
    let k = spec.kernel_size as usize;
    let tensor_elements = c
        .checked_mul(length)
        .ok_or(KokoroConv1dError::InvalidShape)?;
    let required_weight_words = m
        .checked_mul(c)
        .and_then(|elements| elements.checked_mul(k))
        .map(|elements| elements / 4)
        .ok_or(KokoroConv1dError::InvalidShape)?;
    let required_sums = m.checked_mul(k).ok_or(KokoroConv1dError::InvalidShape)?;
    if packed_weight_words != required_weight_words
        || weight_tap_sum_elements != required_sums
        || activation_elements != tensor_elements
        || output_elements != tensor_elements
        || required_weight_words * core::mem::size_of::<u32>()
            > KOKORO_CONV1D_PACKED_WEIGHTS_ALLOC_BYTES
        || required_sums * core::mem::size_of::<u32>() > KOKORO_CONV1D_WEIGHT_TAP_SUMS_ALLOC_BYTES
    {
        return Err(KokoroConv1dError::InvalidShape);
    }
    let tile_dispatches = spec
        .input_length
        .div_ceil(KOKORO_CONV1D_MAX_TILE_TIMES as u32);
    Ok(KokoroConv1dRequirements {
        packed_weight_words: required_weight_words,
        weight_tap_sums: required_sums,
        tensor_elements,
        tile_dispatches,
    })
}

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
