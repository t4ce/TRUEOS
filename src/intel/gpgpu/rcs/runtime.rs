#[derive(Copy, Clone, Debug)]
struct DirectRcsState {
    ring_phys: u64,
    ring_virt: *mut u8,
    context_phys: u64,
    context_virt: *mut u8,
    batch_phys: u64,
    batch_virt: *mut u8,
    result_phys: u64,
    result_virt: *mut u8,
    clear_test_phys: u64,
    clear_test_virt: *mut u8,
    ppgtt_phys: u64,
    ppgtt_virt: *mut u8,
    gpu_va: DirectRcsGpuVa,
}

#[derive(Copy, Clone, Debug)]
struct DirectRcsGpuVa {
    ring: u64,
    context: u64,
    batch: u64,
    result: u64,
    job_slots: usize,
    map_general_auxiliary: bool,
}

const DIRECT_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: DIRECT_RCS_GPU_VA_RING_BASE,
    context: DIRECT_RCS_GPU_VA_CONTEXT_BASE,
    batch: DIRECT_RCS_GPU_VA_BATCH_BASE,
    result: DIRECT_RCS_GPU_VA_RESULT_BASE,
    job_slots: 1,
    map_general_auxiliary: true,
};

const EXECUTION_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: EXECUTION_RCS_GPU_VA_RING_BASE,
    context: EXECUTION_RCS_GPU_VA_CONTEXT_BASE,
    batch: EXECUTION_RCS_GPU_VA_BATCH_BASE,
    result: EXECUTION_RCS_GPU_VA_RESULT_BASE,
    job_slots: 1,
    map_general_auxiliary: false,
};

const LFM25_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: LFM25_RCS_GPU_VA_RING_BASE,
    context: LFM25_RCS_GPU_VA_CONTEXT_BASE,
    batch: LFM25_RCS_GPU_VA_BATCH_BASE,
    result: LFM25_RCS_GPU_VA_RESULT_BASE,
    job_slots: 1,
    map_general_auxiliary: false,
};

const UI4_COMPOSITOR_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: UI4_COMPOSITOR_RCS_GPU_VA_RING_BASE,
    context: UI4_COMPOSITOR_RCS_GPU_VA_CONTEXT_BASE,
    batch: UI4_COMPOSITOR_RCS_GPU_VA_BATCH_BASE,
    result: UI4_COMPOSITOR_RCS_GPU_VA_RESULT_BASE,
    job_slots: UI4_COMPOSITOR_RCS_JOB_SLOTS,
    map_general_auxiliary: false,
};

const SCENE_AABB_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: SCENE_AABB_RCS_GPU_VA_RING_BASE,
    context: SCENE_AABB_RCS_GPU_VA_CONTEXT_BASE,
    batch: SCENE_AABB_RCS_GPU_VA_BATCH_BASE,
    result: SCENE_AABB_RCS_GPU_VA_RESULT_BASE,
    job_slots: 1,
    map_general_auxiliary: false,
};

#[derive(Copy, Clone, Debug)]
struct DirectRcsSubmitRuntime {
    context_initialized: bool,
    ring_tail_bytes: usize,
    pending: Option<crate::gpu::executor::KernelSubmission>,
}

impl DirectRcsSubmitRuntime {
    const fn new() -> Self {
        Self {
            context_initialized: false,
            ring_tail_bytes: 0,
            pending: None,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct Ui4CompositorPending {
    submission: Ui4CompositorSubmission,
    job_slot: usize,
    queue_depth_at_admission: usize,
    started_tick: u64,
    admitted_tick: u64,
    marker_slot: usize,
    marker_value: u32,
    kernel: &'static str,
    stats: GpgpuWorklistSubmitStats,
    overdue_logged: bool,
}

#[derive(Debug)]
struct Ui4CompositorRuntime {
    submit: DirectRcsSubmitRuntime,
    next_serial: u64,
    pending: VecDeque<Ui4CompositorPending>,
    completions: VecDeque<(Ui4CompositorSubmission, Ui4CompositorCompletion)>,
    state_mapped: bool,
    ppgtt_initialized: bool,
}

impl Ui4CompositorRuntime {
    const fn new() -> Self {
        Self {
            submit: DirectRcsSubmitRuntime::new(),
            next_serial: 0,
            pending: VecDeque::new(),
            completions: VecDeque::new(),
            state_mapped: false,
            ppgtt_initialized: false,
        }
    }
}

unsafe impl Send for DirectRcsState {}
unsafe impl Sync for DirectRcsState {}

fn direct_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    // A timed-out system-service request can still fetch or execute the
    // persistent batch after its software timeline has been failed. Never hand
    // out the shared state again until reboot: callers would otherwise rewrite
    // its batch, result page, PPGTT, or scratch allocations under that request.
    if direct_rcs_context_is_quarantined() {
        return None;
    }

    let mut state_slot = DIRECT_RCS_STATE.lock();
    // Catch quarantine published while this accessor waited for the state
    // slot. The submit lock remains the serialization contract that prevents a
    // new quarantine from racing the remainder of a normal call.
    if direct_rcs_context_is_quarantined() {
        return None;
    }
    if let Some(state) = *state_slot {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(DIRECT_RCS_GPU_VA)?;
    *state_slot = Some(state);
    Some(state)
}

fn execution_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire) != 0
        || execution_rcs_context_is_quarantined()
    {
        return None;
    }

    let mut state_slot = EXECUTION_RCS_STATE.lock();
    if EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire) != 0
        || execution_rcs_context_is_quarantined()
    {
        return None;
    }
    if let Some(state) = *state_slot {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(EXECUTION_RCS_GPU_VA)?;
    *state_slot = Some(state);
    Some(state)
}

fn lfm25_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if lfm25_rcs_context_is_quarantined() {
        return None;
    }

    let mut state_slot = LFM25_RCS_STATE.lock();
    if lfm25_rcs_context_is_quarantined() {
        return None;
    }
    if let Some(state) = *state_slot {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(LFM25_RCS_GPU_VA)?;
    *state_slot = Some(state);
    Some(state)
}

fn ui4_compositor_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if let Some(state) = *UI4_COMPOSITOR_RCS_STATE.lock() {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(UI4_COMPOSITOR_RCS_GPU_VA)?;
    *UI4_COMPOSITOR_RCS_STATE.lock() = Some(state);
    Some(state)
}

fn scene_aabb_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if let Some(state) = *SCENE_AABB_RCS_STATE.lock() {
        return Some(state);
    }
    let state = allocate_direct_rcs_state(SCENE_AABB_RCS_GPU_VA)?;
    *SCENE_AABB_RCS_STATE.lock() = Some(state);
    Some(state)
}

fn allocate_direct_rcs_state(gpu_va: DirectRcsGpuVa) -> Option<DirectRcsState> {
    let (ring_phys, ring_virt) = crate::dma::alloc(DIRECT_RCS_RING_BYTES, super::WARM_ALIGN)?;
    let (context_phys, context_virt) =
        crate::dma::alloc(DIRECT_RCS_CONTEXT_BYTES, super::WARM_ALIGN)?;
    let batch_alloc_bytes = DIRECT_RCS_BATCH_BYTES.checked_mul(gpu_va.job_slots)?;
    let result_alloc_bytes = DIRECT_RCS_RESULT_BYTES.checked_mul(gpu_va.job_slots)?;
    let (batch_phys, batch_virt) = crate::dma::alloc(batch_alloc_bytes, super::WARM_ALIGN)?;
    let (result_phys, result_virt) = crate::dma::alloc(result_alloc_bytes, super::WARM_ALIGN)?;
    let (clear_test_phys, clear_test_virt) =
        crate::dma::alloc(CLEAR_RECT_TEST_BYTES, super::WARM_ALIGN)?;
    let (ppgtt_phys, ppgtt_virt) = crate::dma::alloc(DIRECT_RCS_PPGTT_BYTES, super::WARM_ALIGN)?;

    unsafe {
        core::ptr::write_bytes(ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(context_virt, 0, DIRECT_RCS_CONTEXT_BYTES);
        core::ptr::write_bytes(batch_virt, 0, batch_alloc_bytes);
        core::ptr::write_bytes(result_virt, 0, result_alloc_bytes);
        core::ptr::write_bytes(clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::write_bytes(ppgtt_virt, 0, DIRECT_RCS_PPGTT_BYTES);
    }

    let state = DirectRcsState {
        ring_phys,
        ring_virt,
        context_phys,
        context_virt,
        batch_phys,
        batch_virt,
        result_phys,
        result_virt,
        clear_test_phys,
        clear_test_virt,
        ppgtt_phys,
        ppgtt_virt,
        gpu_va,
    };
    Some(state)
}

/// Select one immutable batch/result pair while retaining the compositor's
/// shared ring, HWLRCA, and PPGTT root.
fn direct_rcs_job_slot(state: DirectRcsState, slot: usize) -> Option<DirectRcsState> {
    if slot >= state.gpu_va.job_slots {
        return None;
    }
    let mut selected = state;
    let batch_offset = slot.checked_mul(DIRECT_RCS_BATCH_BYTES)?;
    let result_offset = slot.checked_mul(DIRECT_RCS_RESULT_BYTES)?;
    selected.batch_phys = selected.batch_phys.checked_add(batch_offset as u64)?;
    selected.batch_virt = unsafe { selected.batch_virt.add(batch_offset) };
    selected.result_phys = selected.result_phys.checked_add(result_offset as u64)?;
    selected.result_virt = unsafe { selected.result_virt.add(result_offset) };
    selected.gpu_va.batch = selected.gpu_va.batch.checked_add(batch_offset as u64)?;
    selected.gpu_va.result = selected.gpu_va.result.checked_add(result_offset as u64)?;
    selected.gpu_va.job_slots = 1;
    Some(selected)
}

fn direct_rcs_map_state(dev: super::Dev, state: DirectRcsState) -> bool {
    let batch_alloc_bytes = DIRECT_RCS_BATCH_BYTES.saturating_mul(state.gpu_va.job_slots);
    let result_alloc_bytes = DIRECT_RCS_RESULT_BYTES.saturating_mul(state.gpu_va.job_slots);
    let core_mapped =
        super::map_ggtt(dev, state.ring_phys, DIRECT_RCS_RING_BYTES, state.gpu_va.ring)
            && super::map_ggtt(
                dev,
                state.context_phys,
                DIRECT_RCS_CONTEXT_BYTES,
                state.gpu_va.context,
            )
            && super::map_ggtt(dev, state.batch_phys, batch_alloc_bytes, state.gpu_va.batch)
            && super::map_ggtt(dev, state.result_phys, result_alloc_bytes, state.gpu_va.result);
    let auxiliary_mapped = !state.gpu_va.map_general_auxiliary
        || (super::map_ggtt(
            dev,
            state.clear_test_phys,
            CLEAR_RECT_TEST_BYTES,
            DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        ) && super::map_ggtt(
            dev,
            state.font_outline_mesh_out_phys,
            FONT_OUTLINE_MESH_OUT_ALLOC_BYTES,
            DIRECT_RCS_GPU_VA_FONT_OUTLINE_MESH_BASE,
        ));
    let mapped = core_mapped && auxiliary_mapped;
    if mapped {
        super::ggtt_invalidate(dev);
    }
    mapped
}

fn direct_rcs_init_ppgtt(state: DirectRcsState) -> bool {
    let pml4_off = 0usize;
    let pdp_off = 4096usize;
    let pd_off = 8192usize;
    let pt_off = 12288usize;
    let pte_present_rw = super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW;
    let pde_present_rw_uc = pte_present_rw | GEN8_PAGE_PWT | GEN8_PAGE_PCD;
    let batch_alloc_bytes = DIRECT_RCS_BATCH_BYTES.saturating_mul(state.gpu_va.job_slots);
    let result_alloc_bytes = DIRECT_RCS_RESULT_BYTES.saturating_mul(state.gpu_va.job_slots);

    unsafe {
        core::ptr::write_bytes(state.ppgtt_virt, 0, DIRECT_RCS_PPGTT_BYTES);
        let pml4 = state.ppgtt_virt.add(pml4_off) as *mut u64;
        let pdp = state.ppgtt_virt.add(pdp_off) as *mut u64;
        let pd = state.ppgtt_virt.add(pd_off) as *mut u64;
        core::ptr::write_volatile(pml4, (state.ppgtt_phys + pdp_off as u64) | pde_present_rw_uc);
        core::ptr::write_volatile(pdp, (state.ppgtt_phys + pd_off as u64) | pde_present_rw_uc);
        for index in 0..DIRECT_RCS_PPGTT_PT_COUNT {
            let pt_phys = state.ppgtt_phys + pt_off as u64 + (index as u64) * 4096;
            core::ptr::write_volatile(pd.add(index), pt_phys | pde_present_rw_uc);
        }
    }

    let ok = direct_rcs_map_ppgtt_region(
        state,
        state.gpu_va.ring,
        state.ring_phys,
        DIRECT_RCS_RING_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        state.gpu_va.context,
        state.context_phys,
        DIRECT_RCS_CONTEXT_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        state.gpu_va.batch,
        state.batch_phys,
        batch_alloc_bytes,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        state.gpu_va.result,
        state.result_phys,
        result_alloc_bytes,
        pte_present_rw,
    );

    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    ok
}

fn direct_rcs_map_ppgtt_kernel(state: DirectRcsState, gpu: u64, phys: u64, len: usize) -> bool {
    let ok = direct_rcs_map_ppgtt_region(state, gpu, phys, len, direct_rcs_ppgtt_pte_flags());
    ok && direct_rcs_flush_ppgtt_pte_range(state, gpu, len)
}

fn direct_rcs_map_ppgtt_destination(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    direct_scanout: bool,
) -> bool {
    if direct_scanout {
        direct_rcs_map_ppgtt_scanout(state, gpu, phys, len)
    } else {
        direct_rcs_map_ppgtt_kernel(state, gpu, phys, len)
    }
}

/// Map a full-surface compute destination that will transfer directly to the
/// display engine. PAT3/UC is the same producer-side cache contract used by
/// Draw3D direct targets; ordinary kernels and resources remain PAT0/WB.
fn direct_rcs_map_ppgtt_scanout(state: DirectRcsState, gpu: u64, phys: u64, len: usize) -> bool {
    if !super::gen12_integrated_pat_ready() {
        return false;
    }
    let pte_present_rw_pat3_uc = direct_rcs_ppgtt_pte_flags() | GEN8_PAGE_PWT | GEN8_PAGE_PCD;
    let ok = direct_rcs_map_ppgtt_region(state, gpu, phys, len, pte_present_rw_pat3_uc)
        && direct_rcs_flush_ppgtt_pte_range(state, gpu, len);
    if ok && !DIRECT_RCS_SCANOUT_PPGTT_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: direct-rgba8 scanout target mapped gpu=0x{:X} phys=0x{:X} bytes=0x{:X} ppgtt_pat=3 ppgtt_cache=uc ordinary_resources=pat0-wb\n",
            gpu,
            phys,
            len,
        );
    }
    ok
}

/// Publish only the PTEs changed by one mapping. The PML4/PDP/PD topology is
/// initialized and flushed once for the persistent context; flushing the full
/// PPGTT allocation for every source and destination remap made UI4 submit
/// preparation scale with page-table capacity instead of with changed PTEs.
fn direct_rcs_flush_ppgtt_pte_range(state: DirectRcsState, gpu: u64, len: usize) -> bool {
    if len == 0 || gpu & 0xFFF != 0 {
        return false;
    }
    let pages = len.div_ceil(4096);
    let va_page = gpu >> 12;
    let pd_index = (va_page >> 9) as usize;
    let pt_index = (va_page & 0x1FF) as usize;
    if pd_index >= DIRECT_RCS_PPGTT_PT_COUNT {
        return false;
    }
    let pt_off = 12288usize;
    let Some(start) = pt_off
        .checked_add(pd_index.saturating_mul(4096))
        .and_then(|offset| {
            offset.checked_add(pt_index.saturating_mul(core::mem::size_of::<u64>()))
        })
    else {
        return false;
    };
    let Some(bytes) = pages.checked_mul(core::mem::size_of::<u64>()) else {
        return false;
    };
    let Some(end) = start.checked_add(bytes) else {
        return false;
    };
    if end > DIRECT_RCS_PPGTT_BYTES {
        return false;
    }
    super::dma_flush(unsafe { state.ppgtt_virt.add(start) }, bytes);
    true
}

fn direct_rcs_ppgtt_pte_flags() -> u64 {
    super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW
}

fn direct_rcs_ppgtt_limit_bytes() -> u64 {
    DIRECT_RCS_PPGTT_LIMIT_BYTES
}

fn direct_rcs_map_ppgtt_region(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    entry_flags: u64,
) -> bool {
    if len == 0 || !gpu.is_multiple_of(4096) || !phys.is_multiple_of(4096) {
        return false;
    }
    let Some(end) = u64::try_from(len).ok().and_then(|len| gpu.checked_add(len)) else {
        return false;
    };
    if end > DIRECT_RCS_PPGTT_LIMIT_BYTES {
        return false;
    }

    let pt_off = 12288usize;
    let pages = len.div_ceil(4096);
    let cache_policy_mask = GEN8_PAGE_PWT | GEN8_PAGE_PCD;
    // Preflight the entire range before changing any leaf. A rejected policy
    // transition must not leave a partially rewritten mapping behind.
    for page in 0..pages {
        let va_page = (gpu >> 12) + page as u64;
        let pd_index = (va_page >> 9) as usize;
        let pt_index = (va_page & 0x1FF) as usize;
        if pd_index >= DIRECT_RCS_PPGTT_PT_COUNT {
            return false;
        }
        let pte_off = pt_off + pd_index * 4096 + pt_index * core::mem::size_of::<u64>();
        let pte_ptr = unsafe { state.ppgtt_virt.add(pte_off) as *mut u64 };
        let previous = unsafe { core::ptr::read_volatile(pte_ptr) };
        if previous & super::GEN8_PAGE_PRESENT != 0
            && previous & cache_policy_mask != entry_flags & cache_policy_mask
        {
            let occurrence = DIRECT_RCS_PPGTT_POLICY_REJECTIONS
                .fetch_add(1, Ordering::Relaxed)
                .saturating_add(1);
            if occurrence == 1 || occurrence.is_power_of_two() {
                crate::log_error!(target: "gpgpu";
                    "intel/gpgpu: PPGTT cache-policy remap rejected occurrence={} gpu=0x{:X} page={} previous_pat={} requested_pat={} action=reject-before-submit same_va_pat_transition=forbidden\n",
                    occurrence,
                    gpu,
                    page,
                    (previous & cache_policy_mask) >> 3,
                    (entry_flags & cache_policy_mask) >> 3,
                );
            }
            return false;
        }
    }
    for page in 0..pages {
        let va_page = (gpu >> 12) + page as u64;
        let pd_index = (va_page >> 9) as usize;
        let pt_index = (va_page & 0x1FF) as usize;
        let pte_off = pt_off + pd_index * 4096 + pt_index * core::mem::size_of::<u64>();
        let Some(pte) = (page as u64)
            .checked_mul(4096)
            .and_then(|offset| phys.checked_add(offset))
            .map(|address| address & !0xFFF)
        else {
            return false;
        };
        let pte_ptr = unsafe { state.ppgtt_virt.add(pte_off) as *mut u64 };
        unsafe {
            core::ptr::write_volatile(pte_ptr, pte | entry_flags);
        }
    }
    true
}

fn direct_rcs_forcewake(dev: super::Dev) -> bool {
    // TRUEOS retains the kernel forcewake request for the lifetime of the
    // claimed GT.  Releasing and reacquiring it for every tiny LFM projection
    // forced each submission through an avoidable render power transition.
    // Match a refcounted forcewake_get(): an already-awake domain is the hot
    // path, while a lost request is asserted again without first dropping the
    // other live domain.
    let render_awake =
        super::mmio_read(dev, FORCEWAKE_ACK_RENDER) & FORCEWAKE_KERNEL == FORCEWAKE_KERNEL;
    let gt_awake = super::mmio_read(dev, FORCEWAKE_ACK_GT) & FORCEWAKE_KERNEL == FORCEWAKE_KERNEL;

    let render_ok = render_awake || {
        super::mmio_write(dev, FORCEWAKE_RENDER, super::mask_en(FORCEWAKE_KERNEL));
        direct_rcs_wait_eq(
            dev,
            FORCEWAKE_ACK_RENDER,
            FORCEWAKE_KERNEL,
            FORCEWAKE_KERNEL,
            FORCEWAKE_POLL_ITERS,
        )
    };
    let gt_ok = gt_awake || {
        super::mmio_write(dev, FORCEWAKE_GT, super::mask_en(FORCEWAKE_KERNEL));
        direct_rcs_wait_eq(
            dev,
            FORCEWAKE_ACK_GT,
            FORCEWAKE_KERNEL,
            FORCEWAKE_KERNEL,
            FORCEWAKE_POLL_ITERS,
        )
    };
    if render_ok && super::mmio_read(dev, RCS_CS_DEBUG_MODE1) & FF_DOP_CLOCK_GATE_DISABLE == 0 {
        super::mmio_write(
            dev,
            RCS_CS_DEBUG_MODE1,
            direct_rcs_masked_bit_enable(FF_DOP_CLOCK_GATE_DISABLE),
        );
    }
    render_ok && gt_ok
}
