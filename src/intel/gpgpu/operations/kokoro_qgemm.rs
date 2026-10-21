/// Exact launch description for one Kokoro `MatMulInteger` projection.
///
/// Activations and weights contain four adjacent quantized values per `u32`
/// in little-endian byte order. `packed_weights` uses the bake-time SIMD16
/// layout documented by `kokoro_qgemm_u8_i8.clcpp`: K words are nested inside
/// sixteen-column output tiles. Strides are expressed in ABI element units,
/// not bytes.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct KokoroQgemmSpec {
    pub(crate) matrix_rows: u32,
    pub(crate) output_columns: u32,
    pub(crate) reduction_words: u32,
    pub(crate) activation_stride_words: u32,
    pub(crate) output_stride: u32,
    pub(crate) activation_zero_point: u8,
    pub(crate) activation_scale: f32,
}

impl KokoroQgemmSpec {
    pub(crate) const fn contiguous(
        matrix_rows: u32,
        reduction_values: u32,
        output_columns: u32,
        activation_zero_point: u8,
        activation_scale: f32,
    ) -> Option<Self> {
        if !reduction_values.is_multiple_of(4) {
            return None;
        }
        Some(Self {
            matrix_rows,
            output_columns,
            reduction_words: reduction_values / 4,
            activation_stride_words: reduction_values / 4,
            output_stride: output_columns,
            activation_zero_point,
            activation_scale,
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KokoroQgemmError {
    UnsupportedTarget,
    InvalidShape,
    InvalidQuantization,
    Allocation,
    RuntimeUnavailable,
    MappingFailed,
    EncodeFailed,
    SubmitFailed,
    CompletionTimeout,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct KokoroQgemmResult {
    pub(crate) matrix_rows: u32,
    pub(crate) output_columns: u32,
    pub(crate) marker: u32,
    pub(crate) submit_ms: u64,
}

#[derive(Copy, Clone)]
struct KokoroQgemmArena {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for KokoroQgemmArena {}
unsafe impl Sync for KokoroQgemmArena {}

impl KokoroQgemmArena {
    const fn gpu_at(self, offset: usize) -> u64 {
        self.gpu + offset as u64
    }

    unsafe fn virt_at(self, offset: usize) -> *mut u8 {
        unsafe { self.virt.add(offset) }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct KokoroQgemmRequirements {
    packed_weight_words: usize,
    vector_elements: usize,
    activation_words: usize,
    output_elements: usize,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
}

#[derive(Copy, Clone)]
struct KokoroQgemmParams {
    packed_weights_gpu: u64,
    weight_sums_gpu: u64,
    weight_scales_gpu: u64,
    activations_gpu: u64,
    bias_gpu: u64,
    output_gpu: u64,
    packed_weights_bytes: usize,
    weight_sums_bytes: usize,
    weight_scales_bytes: usize,
    activations_bytes: usize,
    bias_bytes: usize,
    output_bytes: usize,
    matrix_rows: u32,
    output_columns: u32,
    reduction_words: u32,
    activation_stride_words: u32,
    output_stride: u32,
    activation_zero_point: u32,
    activation_scale: f32,
    has_bias: u32,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
}

static KOKORO_QGEMM_ARENA: Mutex<Option<KokoroQgemmArena>> = Mutex::new(None);
static KOKORO_QGEMM_SUBMISSIONS: AtomicU64 = AtomicU64::new(0);

/// Whether the exact Alder Lake-S GT1 artifact can run on the claimed GPU.
///
/// This deliberately rejects nearby Intel revisions. The artifact was baked
/// and inspected only for PCI device 0x4680 revision 0x0c.
pub(crate) fn kokoro_qgemm_u8_i8_supported() -> bool {
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    KOKORO_QGEMM_U8_I8_ADLS_ARTIFACT
        .target_policy
        .supports(dev.device_id, dev.revision_id)
        && !direct_rcs_context_is_quarantined()
}

/// Pack a standard ONNX row-major `[K, N]` signed-int8 matrix for the SIMD16
/// QGEMM kernel and compute its signed per-output-channel sums.
///
/// The caller supplies both destinations, so this warm-job helper performs no
/// allocation. Four consecutive K values become one little-endian `u32`; K
/// words are nested inside sixteen-column N tiles. Lanes beyond a partial N
/// tile are written as zero. All slices must have their exact required size.
pub(crate) fn kokoro_qgemm_pack_onnx_weights(
    reduction_values: u32,
    output_columns: u32,
    weights_kn: &[i8],
    packed_weights: &mut [u32],
    weight_sums: &mut [i32],
) -> Result<(), KokoroQgemmError> {
    if !kokoro_qgemm_admitted_shape(reduction_values, output_columns)
        || !reduction_values.is_multiple_of(4)
    {
        return Err(KokoroQgemmError::InvalidShape);
    }
    let k = reduction_values as usize;
    let n = output_columns as usize;
    let reduction_words = k / 4;
    let output_tiles = n.div_ceil(16);
    let source_elements = k.checked_mul(n).ok_or(KokoroQgemmError::InvalidShape)?;
    let packed_words = output_tiles
        .checked_mul(reduction_words)
        .and_then(|words| words.checked_mul(16))
        .ok_or(KokoroQgemmError::InvalidShape)?;
    if weights_kn.len() != source_elements
        || packed_weights.len() != packed_words
        || weight_sums.len() != n
    {
        return Err(KokoroQgemmError::InvalidShape);
    }

    weight_sums.fill(0);
    for output_tile in 0..output_tiles {
        for reduction_word in 0..reduction_words {
            for lane in 0..16usize {
                let output_column = output_tile * 16 + lane;
                let packed_index = (output_tile * reduction_words + reduction_word) * 16 + lane;
                if output_column >= n {
                    packed_weights[packed_index] = 0;
                    continue;
                }

                let mut word = 0u32;
                for byte in 0..4usize {
                    let reduction_index = reduction_word * 4 + byte;
                    let weight = weights_kn[reduction_index * n + output_column];
                    word |= u32::from(weight as u8) << (byte * 8);
                    weight_sums[output_column] += i32::from(weight);
                }
                packed_weights[packed_index] = word;
            }
        }
    }
    Ok(())
}

/// Execute one admitted Kokoro quantized matrix multiplication synchronously.
///
/// The boundary is safe for ordinary Rust slices: all caller-owned data is
/// copied into a persistent DMA arena before submission and the result is
/// copied out only after an ordered GPU completion marker. Output padding
/// (`output_stride - output_columns`) is intentionally left unchanged.
pub(crate) fn kokoro_qgemm_u8_i8(
    spec: KokoroQgemmSpec,
    packed_weights: &[u32],
    weight_sums: &[i32],
    weight_scales: &[f32],
    activations: &[u32],
    bias: Option<&[f32]>,
    output: &mut [f32],
) -> Result<KokoroQgemmResult, KokoroQgemmError> {
    let requirements = kokoro_qgemm_validate_call(
        spec,
        packed_weights.len(),
        weight_sums.len(),
        weight_scales.len(),
        activations.len(),
        bias.map(|values| values.len()),
        output.len(),
    )?;
    if !spec.activation_scale.is_finite() || spec.activation_scale <= 0.0 {
        return Err(KokoroQgemmError::InvalidQuantization);
    }
    if weight_scales
        .iter()
        .any(|scale| !scale.is_finite() || *scale <= 0.0)
        || bias.is_some_and(|values| values.iter().any(|value| !value.is_finite()))
    {
        return Err(KokoroQgemmError::InvalidQuantization);
    }
    if !kokoro_qgemm_u8_i8_supported() {
        return Err(KokoroQgemmError::UnsupportedTarget);
    }

    // The common system-service direct-RCS context has one mutable batch,
    // result page, ring timeline, and PPGTT root. Keep staging, PTE updates,
    // submission, completion, and copy-out under its established lock.
    let _submit_guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let dev = super::claimed_device().ok_or(KokoroQgemmError::RuntimeUnavailable)?;
    if !KOKORO_QGEMM_U8_I8_ADLS_ARTIFACT
        .target_policy
        .supports(dev.device_id, dev.revision_id)
        || direct_rcs_context_is_quarantined()
    {
        return Err(KokoroQgemmError::UnsupportedTarget);
    }
    let arena = kokoro_qgemm_arena_once()?;
    kokoro_qgemm_stage_inputs(arena, packed_weights, weight_sums, weight_scales, activations, bias);

    let params = KokoroQgemmParams {
        packed_weights_gpu: arena.gpu_at(KOKORO_QGEMM_PACKED_WEIGHTS_OFFSET_BYTES),
        weight_sums_gpu: arena.gpu_at(KOKORO_QGEMM_WEIGHT_SUMS_OFFSET_BYTES),
        weight_scales_gpu: arena.gpu_at(KOKORO_QGEMM_WEIGHT_SCALES_OFFSET_BYTES),
        activations_gpu: arena.gpu_at(KOKORO_QGEMM_ACTIVATIONS_OFFSET_BYTES),
        bias_gpu: arena.gpu_at(KOKORO_QGEMM_BIAS_OFFSET_BYTES),
        output_gpu: arena.gpu_at(KOKORO_QGEMM_OUTPUT_OFFSET_BYTES),
        packed_weights_bytes: requirements.packed_weight_words * core::mem::size_of::<u32>(),
        weight_sums_bytes: requirements.vector_elements * core::mem::size_of::<i32>(),
        weight_scales_bytes: requirements.vector_elements * core::mem::size_of::<f32>(),
        activations_bytes: requirements.activation_words * core::mem::size_of::<u32>(),
        bias_bytes: requirements.vector_elements * core::mem::size_of::<f32>(),
        output_bytes: requirements.output_elements * core::mem::size_of::<f32>(),
        matrix_rows: spec.matrix_rows,
        output_columns: spec.output_columns,
        reduction_words: spec.reduction_words,
        activation_stride_words: spec.activation_stride_words,
        output_stride: spec.output_stride,
        activation_zero_point: u32::from(spec.activation_zero_point),
        activation_scale: spec.activation_scale,
        has_bias: u32::from(bias.is_some()),
        group_x: requirements.group_x,
        group_y: requirements.group_y,
        right_mask: requirements.right_mask,
    };

    let upload = upload_kokoro_qgemm_u8_i8_kernel().ok_or(KokoroQgemmError::RuntimeUnavailable)?;
    let state = direct_rcs_state_once(dev).ok_or(KokoroQgemmError::RuntimeUnavailable)?;
    if !direct_rcs_forcewake(dev) {
        return Err(KokoroQgemmError::RuntimeUnavailable);
    }
    if !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
        || !direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        || !direct_rcs_map_ppgtt_kernel(state, arena.gpu, arena.phys, arena.bytes)
    {
        return Err(KokoroQgemmError::MappingFailed);
    }
    if !direct_rcs_encode_kokoro_qgemm_batch(state, upload, params) {
        return Err(KokoroQgemmError::EncodeFailed);
    }

    let started = direct_rcs_now_tick();
    if !direct_rcs_submit_batch(dev, state) {
        return Err(KokoroQgemmError::SubmitFailed);
    }
    let observed = direct_rcs_poll_result_slot_timeout_ms(
        state,
        KOKORO_QGEMM_POST_MARKER_SLOT,
        KOKORO_QGEMM_POST_MARKER,
        KOKORO_QGEMM_COMPLETION_TIMEOUT_MS,
    );
    if observed != KOKORO_QGEMM_POST_MARKER {
        return Err(KokoroQgemmError::CompletionTimeout);
    }

    let output_source = unsafe { arena.virt_at(KOKORO_QGEMM_OUTPUT_OFFSET_BYTES) };
    super::dma_flush(output_source, params.output_bytes);
    let rows = spec.matrix_rows as usize;
    let columns = spec.output_columns as usize;
    let stride = spec.output_stride as usize;
    for row in 0..rows {
        unsafe {
            core::ptr::copy_nonoverlapping(
                (output_source as *const f32).add(row * stride),
                output.as_mut_ptr().add(row * stride),
                columns,
            );
        }
    }

    let submit_ms = direct_rcs_elapsed_ms_since(started);
    let submissions = KOKORO_QGEMM_SUBMISSIONS
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if submissions == 1 || submissions.is_power_of_two() {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: kokoro-qgemm complete submissions={} rows={} k={} columns={} bias={} submit_ms={} marker=0x{:08X} geometry={}x{} simd=16 layout=kword-inside-n16 artifact={} device=0x{:04X} revision=0x{:02X}\n",
            submissions,
            spec.matrix_rows,
            spec.reduction_words * 4,
            spec.output_columns,
            params.has_bias,
            submit_ms,
            observed,
            params.group_x,
            params.group_y,
            KOKORO_QGEMM_U8_I8_ADLS_ARTIFACT.name,
            dev.device_id,
            dev.revision_id,
        );
    }
    Ok(KokoroQgemmResult {
        matrix_rows: spec.matrix_rows,
        output_columns: spec.output_columns,
        marker: observed,
        submit_ms,
    })
}

fn kokoro_qgemm_arena_once() -> Result<KokoroQgemmArena, KokoroQgemmError> {
    let mut slot = KOKORO_QGEMM_ARENA.lock();
    if let Some(arena) = *slot {
        return Ok(arena);
    }
    let (phys, virt) =
        crate::dma::alloc(KOKORO_QGEMM_ARENA_BYTES, 4096).ok_or(KokoroQgemmError::Allocation)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, KOKORO_QGEMM_ARENA_BYTES);
    }
    super::dma_flush(virt, KOKORO_QGEMM_ARENA_BYTES);
    let arena = KokoroQgemmArena {
        phys,
        gpu: KOKORO_QGEMM_ARENA_GPU,
        virt,
        bytes: KOKORO_QGEMM_ARENA_BYTES,
    };
    *slot = Some(arena);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: kokoro-qgemm staging ready phys=0x{:X} gpu=0x{:X} bytes=0x{:X} regions=6 persistent=1\n",
        arena.phys,
        arena.gpu,
        arena.bytes,
    );
    Ok(arena)
}

fn kokoro_qgemm_stage_inputs(
    arena: KokoroQgemmArena,
    packed_weights: &[u32],
    weight_sums: &[i32],
    weight_scales: &[f32],
    activations: &[u32],
    bias: Option<&[f32]>,
) {
    unsafe {
        core::ptr::copy_nonoverlapping(
            packed_weights.as_ptr().cast::<u8>(),
            arena.virt_at(KOKORO_QGEMM_PACKED_WEIGHTS_OFFSET_BYTES),
            core::mem::size_of_val(packed_weights),
        );
        core::ptr::copy_nonoverlapping(
            weight_sums.as_ptr().cast::<u8>(),
            arena.virt_at(KOKORO_QGEMM_WEIGHT_SUMS_OFFSET_BYTES),
            core::mem::size_of_val(weight_sums),
        );
        core::ptr::copy_nonoverlapping(
            weight_scales.as_ptr().cast::<u8>(),
            arena.virt_at(KOKORO_QGEMM_WEIGHT_SCALES_OFFSET_BYTES),
            core::mem::size_of_val(weight_scales),
        );
        core::ptr::copy_nonoverlapping(
            activations.as_ptr().cast::<u8>(),
            arena.virt_at(KOKORO_QGEMM_ACTIVATIONS_OFFSET_BYTES),
            core::mem::size_of_val(activations),
        );
        if let Some(bias) = bias {
            core::ptr::copy_nonoverlapping(
                bias.as_ptr().cast::<u8>(),
                arena.virt_at(KOKORO_QGEMM_BIAS_OFFSET_BYTES),
                core::mem::size_of_val(bias),
            );
        }
    }
    super::dma_flush(
        unsafe { arena.virt_at(KOKORO_QGEMM_PACKED_WEIGHTS_OFFSET_BYTES) },
        core::mem::size_of_val(packed_weights),
    );
    super::dma_flush(
        unsafe { arena.virt_at(KOKORO_QGEMM_WEIGHT_SUMS_OFFSET_BYTES) },
        core::mem::size_of_val(weight_sums),
    );
    super::dma_flush(
        unsafe { arena.virt_at(KOKORO_QGEMM_WEIGHT_SCALES_OFFSET_BYTES) },
        core::mem::size_of_val(weight_scales),
    );
    super::dma_flush(
        unsafe { arena.virt_at(KOKORO_QGEMM_ACTIVATIONS_OFFSET_BYTES) },
        core::mem::size_of_val(activations),
    );
    if let Some(bias) = bias {
        super::dma_flush(
            unsafe { arena.virt_at(KOKORO_QGEMM_BIAS_OFFSET_BYTES) },
            core::mem::size_of_val(bias),
        );
    }
}

fn kokoro_qgemm_validate_call(
    spec: KokoroQgemmSpec,
    packed_weight_words: usize,
    weight_sum_elements: usize,
    weight_scale_elements: usize,
    activation_words: usize,
    bias_elements: Option<usize>,
    output_elements: usize,
) -> Result<KokoroQgemmRequirements, KokoroQgemmError> {
    let rows = spec.matrix_rows as usize;
    let columns = spec.output_columns as usize;
    let reduction_words = spec.reduction_words as usize;
    let activation_stride_words = spec.activation_stride_words as usize;
    let output_stride = spec.output_stride as usize;
    let reduction_values = reduction_words
        .checked_mul(4)
        .ok_or(KokoroQgemmError::InvalidShape)?;
    if rows == 0
        || rows > KOKORO_QGEMM_MAX_MATRIX_ROWS
        || columns == 0
        || columns > KOKORO_QGEMM_MAX_OUTPUT_COLUMNS
        || reduction_words == 0
        || reduction_words > KOKORO_QGEMM_MAX_REDUCTION_WORDS
        || !kokoro_qgemm_admitted_shape(reduction_values as u32, spec.output_columns)
        || activation_stride_words < reduction_words
        || activation_stride_words > KOKORO_QGEMM_MAX_REDUCTION_WORDS
        || output_stride < columns
        || output_stride > KOKORO_QGEMM_MAX_OUTPUT_COLUMNS
    {
        return Err(KokoroQgemmError::InvalidShape);
    }

    let output_tiles = columns.div_ceil(16);
    let required_packed_weight_words = output_tiles
        .checked_mul(reduction_words)
        .and_then(|words| words.checked_mul(16))
        .ok_or(KokoroQgemmError::InvalidShape)?;
    let required_activation_words = rows
        .checked_mul(activation_stride_words)
        .ok_or(KokoroQgemmError::InvalidShape)?;
    let required_output_elements = rows
        .checked_mul(output_stride)
        .ok_or(KokoroQgemmError::InvalidShape)?;
    let packed_weight_bytes = required_packed_weight_words
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or(KokoroQgemmError::InvalidShape)?;
    let activation_bytes = required_activation_words
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or(KokoroQgemmError::InvalidShape)?;
    let output_bytes = required_output_elements
        .checked_mul(core::mem::size_of::<f32>())
        .ok_or(KokoroQgemmError::InvalidShape)?;
    if packed_weight_words != required_packed_weight_words
        || weight_sum_elements != columns
        || weight_scale_elements != columns
        || bias_elements.is_some_and(|elements| elements != columns)
        || activation_words != required_activation_words
        || output_elements != required_output_elements
        || packed_weight_bytes > KOKORO_QGEMM_PACKED_WEIGHTS_ALLOC_BYTES
        || columns * core::mem::size_of::<u32>() > KOKORO_QGEMM_VECTOR_ALLOC_BYTES
        || activation_bytes > KOKORO_QGEMM_ACTIVATIONS_ALLOC_BYTES
        || output_bytes > KOKORO_QGEMM_OUTPUT_ALLOC_BYTES
    {
        return Err(KokoroQgemmError::InvalidShape);
    }

    let group_x = spec.output_columns.div_ceil(16);
    let last_group_lanes = ((spec.output_columns - 1) % 16) + 1;
    let right_mask = if last_group_lanes == 16 {
        GPGPU_WALKER_SIMD16_MASK
    } else {
        (1u32 << last_group_lanes) - 1
    };
    Ok(KokoroQgemmRequirements {
        packed_weight_words: required_packed_weight_words,
        vector_elements: columns,
        activation_words: required_activation_words,
        output_elements: required_output_elements,
        group_x,
        group_y: spec.matrix_rows,
        right_mask,
    })
}

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
