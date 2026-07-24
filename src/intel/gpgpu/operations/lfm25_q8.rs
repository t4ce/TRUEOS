#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Lfm25Q8ProjectError {
    UnsupportedTarget,
    InvalidModel,
    NonContiguousModel,
    InvalidShape,
    Allocation,
    RuntimeUnavailable,
    MappingFailed,
    EncodeFailed,
    SubmitFailed,
    CompletionTimeout,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25Q8ProjectStats {
    pub(crate) available: bool,
    pub(crate) ready: bool,
    pub(crate) launches: u64,
    pub(crate) failures: u64,
    pub(crate) total_submit_ms: u64,
    pub(crate) last_rows: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25Q8ModelMapping {
    mapping_phys: u64,
    mapping_gpu: u64,
    mapping_bytes: usize,
    model_gpu: u64,
    model_bytes: usize,
    model_virt: *mut u8,
}

unsafe impl Send for Lfm25Q8ModelMapping {}
unsafe impl Sync for Lfm25Q8ModelMapping {}

#[derive(Copy, Clone)]
struct Lfm25Q8Buffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for Lfm25Q8Buffer {}
unsafe impl Sync for Lfm25Q8Buffer {}

struct Lfm25Q8Runtime {
    activation: Lfm25Q8Buffer,
    output: Lfm25Q8Buffer,
    bound_model: Option<Lfm25Q8ModelMapping>,
    ready: bool,
}

unsafe impl Send for Lfm25Q8Runtime {}
unsafe impl Sync for Lfm25Q8Runtime {}

#[derive(Copy, Clone)]
struct Lfm25Q8ProjectParams {
    weights_gpu: u64,
    activation_gpu: u64,
    output_gpu: u64,
    model_bytes: usize,
    activation_bytes: usize,
    output_bytes: usize,
    weight_offset: u32,
    columns: u32,
    rows: u32,
}

static LFM25_Q8_RUNTIME: Mutex<Option<Lfm25Q8Runtime>> = Mutex::new(None);
static LFM25_Q8_READY: AtomicBool = AtomicBool::new(false);
static LFM25_Q8_LAUNCHES: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_FAILURES: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_TOTAL_SUBMIT_MS: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_LAST_ROWS: AtomicU32 = AtomicU32::new(0);

pub(crate) const LFM25_Q8_PROJECTIONS_PER_TOKEN: u64 = 93;

pub(crate) fn lfm25_q8_project_supported() -> bool {
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    LFM25_Q8_PROJECT_ADLS_CPP_ABI_CONTRACT
        .target
        .supports(dev.device_id, dev.revision_id)
        && !lfm25_rcs_context_is_quarantined()
}

pub(crate) fn lfm25_q8_project_stats() -> Lfm25Q8ProjectStats {
    Lfm25Q8ProjectStats {
        available: lfm25_q8_project_supported(),
        ready: LFM25_Q8_READY.load(Ordering::Acquire),
        launches: LFM25_Q8_LAUNCHES.load(Ordering::Relaxed),
        failures: LFM25_Q8_FAILURES.load(Ordering::Relaxed),
        total_submit_ms: LFM25_Q8_TOTAL_SUBMIT_MS.load(Ordering::Relaxed),
        last_rows: LFM25_Q8_LAST_ROWS.load(Ordering::Relaxed),
    }
}

pub(crate) fn bind_lfm25_q8_model(
    model: &[u8],
) -> Result<Lfm25Q8ModelMapping, Lfm25Q8ProjectError> {
    if !lfm25_q8_project_supported() {
        return Err(Lfm25Q8ProjectError::UnsupportedTarget);
    }
    if model.len() != trueos_fpga_abi::lfm25::PINNED_NATIVE_IMAGE_BYTES as usize
        || model.is_empty()
    {
        return Err(Lfm25Q8ProjectError::InvalidModel);
    }
    let model_virt = model.as_ptr() as *mut u8;
    let start_phys =
        crate::phys::virt_to_phys_checked(model_virt).ok_or(Lfm25Q8ProjectError::InvalidModel)?;
    let last_virt = unsafe { model_virt.add(model.len() - 1) };
    let last_phys =
        crate::phys::virt_to_phys_checked(last_virt).ok_or(Lfm25Q8ProjectError::InvalidModel)?;
    if last_phys != start_phys + (model.len() - 1) as u64 {
        return Err(Lfm25Q8ProjectError::NonContiguousModel);
    }
    let page_offset = (start_phys & 0xFFF) as usize;
    let mapping_bytes = page_offset
        .checked_add(model.len())
        .and_then(|bytes| bytes.checked_add(4095))
        .map(|bytes| bytes & !4095usize)
        .ok_or(Lfm25Q8ProjectError::InvalidModel)?;
    let mapping = Lfm25Q8ModelMapping {
        mapping_phys: start_phys & !0xFFF,
        mapping_gpu: LFM25_Q8_MODEL_MAPPING_GPU_BASE,
        mapping_bytes,
        model_gpu: LFM25_Q8_MODEL_MAPPING_GPU_BASE + page_offset as u64,
        model_bytes: model.len(),
        model_virt,
    };
    if mapping.model_gpu & 3 != 0
        || mapping.mapping_gpu + mapping.mapping_bytes as u64 > LFM25_Q8_ACTIVATION_GPU
    {
        return Err(Lfm25Q8ProjectError::InvalidModel);
    }

    // The sealed bytes become immutable for the lifetime of the decoder.
    // Publish them once before the persistent PPGTT mapping is installed.
    super::dma_flush(model_virt, model.len());
    let _guard = LFM25_RCS_SUBMIT_LOCK.lock();
    let mut runtime_slot = LFM25_Q8_RUNTIME.lock();
    ensure_lfm25_q8_runtime(&mut runtime_slot)?;
    prepare_lfm25_q8_runtime(runtime_slot.as_mut().unwrap(), mapping)?;
    Ok(mapping)
}

pub(crate) fn lfm25_q8_project(
    model: Lfm25Q8ModelMapping,
    weight_offset: u32,
    columns: u32,
    rows: u32,
    activation: &[u8],
    output: &mut [f32],
) -> Result<u64, Lfm25Q8ProjectError> {
    let row_bytes = usize::try_from(columns)
        .ok()
        .and_then(|columns| columns.checked_div(32))
        .and_then(|blocks| blocks.checked_mul(34))
        .ok_or(Lfm25Q8ProjectError::InvalidShape)?;
    let matrix_bytes = row_bytes
        .checked_mul(rows as usize)
        .ok_or(Lfm25Q8ProjectError::InvalidShape)?;
    let matrix_end = (weight_offset as usize)
        .checked_add(matrix_bytes)
        .ok_or(Lfm25Q8ProjectError::InvalidShape)?;
    if !lfm25_q8_admitted_shape(columns, rows)
        || activation.len() != row_bytes
        || output.len() != rows as usize
        || matrix_end > model.model_bytes
    {
        return Err(Lfm25Q8ProjectError::InvalidShape);
    }

    let _guard = LFM25_RCS_SUBMIT_LOCK.lock();
    let mut runtime_slot = LFM25_Q8_RUNTIME.lock();
    ensure_lfm25_q8_runtime(&mut runtime_slot)?;
    let runtime = runtime_slot.as_mut().unwrap();
    prepare_lfm25_q8_runtime(runtime, model)?;

    unsafe {
        core::ptr::copy_nonoverlapping(
            activation.as_ptr(),
            runtime.activation.virt,
            activation.len(),
        );
        core::ptr::write_bytes(runtime.output.virt, 0, output.len() * core::mem::size_of::<f32>());
    }
    super::dma_flush(runtime.activation.virt, activation.len());
    super::dma_flush(
        runtime.output.virt,
        output.len() * core::mem::size_of::<f32>(),
    );

    let params = Lfm25Q8ProjectParams {
        weights_gpu: model.model_gpu,
        activation_gpu: runtime.activation.gpu,
        output_gpu: runtime.output.gpu,
        model_bytes: model.model_bytes,
        activation_bytes: activation.len(),
        output_bytes: output.len() * core::mem::size_of::<f32>(),
        weight_offset,
        columns,
        rows,
    };
    let started = direct_rcs_now_tick();
    let result = submit_lfm25_q8_project(runtime, params);
    let elapsed_ms = direct_rcs_elapsed_ms_since(started);
    match result {
        Ok(()) => {
            super::dma_flush(runtime.output.virt, params.output_bytes);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    runtime.output.virt as *const f32,
                    output.as_mut_ptr(),
                    output.len(),
                );
            }
            let launches = LFM25_Q8_LAUNCHES
                .fetch_add(1, Ordering::Relaxed)
                .saturating_add(1);
            LFM25_Q8_TOTAL_SUBMIT_MS.fetch_add(elapsed_ms, Ordering::Relaxed);
            LFM25_Q8_LAST_ROWS.store(rows, Ordering::Relaxed);
            if launches == 1 || launches.is_power_of_two() {
                crate::log_info!(
                    target: "gpgpu";
                    "intel/gpgpu: lfm25-q8 projection ok launches={} columns={} rows={} weight_offset=0x{:X} submit_ms={} lane=lfm25 artifact={}\n",
                    launches,
                    columns,
                    rows,
                    weight_offset,
                    elapsed_ms,
                    LFM25_Q8_PROJECT_ADLS_ARTIFACT.name,
                );
            }
            Ok(elapsed_ms)
        }
        Err(error) => {
            LFM25_Q8_FAILURES.fetch_add(1, Ordering::Relaxed);
            Err(error)
        }
    }
}

fn ensure_lfm25_q8_runtime(
    slot: &mut Option<Lfm25Q8Runtime>,
) -> Result<(), Lfm25Q8ProjectError> {
    if slot.is_some() {
        return Ok(());
    }
    let (activation_phys, activation_virt) =
        crate::dma::alloc(LFM25_Q8_ACTIVATION_ALLOC_BYTES, 4096)
            .ok_or(Lfm25Q8ProjectError::Allocation)?;
    let (output_phys, output_virt) = crate::dma::alloc(LFM25_Q8_OUTPUT_ALLOC_BYTES, 4096)
        .ok_or(Lfm25Q8ProjectError::Allocation)?;
    unsafe {
        core::ptr::write_bytes(activation_virt, 0, LFM25_Q8_ACTIVATION_ALLOC_BYTES);
        core::ptr::write_bytes(output_virt, 0, LFM25_Q8_OUTPUT_ALLOC_BYTES);
    }
    super::dma_flush(activation_virt, LFM25_Q8_ACTIVATION_ALLOC_BYTES);
    super::dma_flush(output_virt, LFM25_Q8_OUTPUT_ALLOC_BYTES);
    *slot = Some(Lfm25Q8Runtime {
        activation: Lfm25Q8Buffer {
            phys: activation_phys,
            gpu: LFM25_Q8_ACTIVATION_GPU,
            virt: activation_virt,
            bytes: LFM25_Q8_ACTIVATION_ALLOC_BYTES,
        },
        output: Lfm25Q8Buffer {
            phys: output_phys,
            gpu: LFM25_Q8_OUTPUT_GPU,
            virt: output_virt,
            bytes: LFM25_Q8_OUTPUT_ALLOC_BYTES,
        },
        bound_model: None,
        ready: false,
    });
    Ok(())
}

fn prepare_lfm25_q8_runtime(
    runtime: &mut Lfm25Q8Runtime,
    model: Lfm25Q8ModelMapping,
) -> Result<(), Lfm25Q8ProjectError> {
    if runtime.ready && runtime.bound_model == Some(model) {
        return Ok(());
    }
    let dev = super::claimed_device().ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let upload =
        upload_lfm25_q8_project_kernel().ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let state = lfm25_rcs_state_once(dev).ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let mapped = direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && direct_rcs_init_ppgtt(state)
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        && direct_rcs_map_ppgtt_kernel(
            state,
            model.mapping_gpu,
            model.mapping_phys,
            model.mapping_bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            state,
            runtime.activation.gpu,
            runtime.activation.phys,
            runtime.activation.bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            state,
            runtime.output.gpu,
            runtime.output.phys,
            runtime.output.bytes,
        );
    if !mapped {
        runtime.ready = false;
        LFM25_Q8_READY.store(false, Ordering::Release);
        return Err(Lfm25Q8ProjectError::MappingFailed);
    }
    runtime.bound_model = Some(model);
    runtime.ready = true;
    LFM25_Q8_READY.store(true, Ordering::Release);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: lfm25-q8 runtime ready model_phys=0x{:X} model_gpu=0x{:X} model_bytes=0x{:X} mapped_bytes=0x{:X} activation_gpu=0x{:X} output_gpu=0x{:X} lane=lfm25 target={} device=0x{:04X} revision=0x{:02X}\n",
        model.mapping_phys,
        model.model_gpu,
        model.model_bytes,
        model.mapping_bytes,
        runtime.activation.gpu,
        runtime.output.gpu,
        LFM25_Q8_PROJECT_ADLS_ARTIFACT.target,
        dev.device_id,
        dev.revision_id,
    );
    Ok(())
}

fn submit_lfm25_q8_project(
    runtime: &Lfm25Q8Runtime,
    params: Lfm25Q8ProjectParams,
) -> Result<(), Lfm25Q8ProjectError> {
    let dev = super::claimed_device().ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let upload =
        upload_lfm25_q8_project_kernel().ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let state = lfm25_rcs_state_once(dev).ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    if !runtime.ready {
        return Err(Lfm25Q8ProjectError::RuntimeUnavailable);
    }
    if !direct_rcs_forcewake(dev) {
        return Err(Lfm25Q8ProjectError::RuntimeUnavailable);
    }
    if !direct_rcs_encode_lfm25_q8_batch(state, upload, params) {
        return Err(Lfm25Q8ProjectError::EncodeFailed);
    }
    if !lfm25_rcs_submit_batch(dev, state) {
        return Err(Lfm25Q8ProjectError::SubmitFailed);
    }
    let observed = lfm25_rcs_poll_result_slot_timeout_ms(
        state,
        LFM25_Q8_POST_MARKER_SLOT,
        LFM25_Q8_POST_MARKER,
        LFM25_Q8_COMPLETION_TIMEOUT_MS,
    );
    if observed != LFM25_Q8_POST_MARKER {
        return Err(Lfm25Q8ProjectError::CompletionTimeout);
    }
    Ok(())
}

const fn lfm25_q8_admitted_shape(columns: u32, rows: u32) -> bool {
    (columns == 1_024
        && matches!(rows, 512 | 1_024 | 3_072 | 4_608 | 65_536))
        || (columns == 4_608 && rows == 1_024)
}
