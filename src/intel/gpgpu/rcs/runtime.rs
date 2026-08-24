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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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

const FONT_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: FONT_RCS_GPU_VA_RING_BASE,
    context: FONT_RCS_GPU_VA_CONTEXT_BASE,
    batch: FONT_RCS_GPU_VA_BATCH_BASE,
    result: FONT_RCS_GPU_VA_RESULT_BASE,
    job_slots: 1,
    map_general_auxiliary: false,
};

const EXECUTION_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: EXECUTION_RCS_GPU_VA_RING_BASE,
    context: EXECUTION_RCS_GPU_VA_CONTEXT_BASE,
    batch: EXECUTION_RCS_GPU_VA_BATCH_BASE,
    result: EXECUTION_RCS_GPU_VA_RESULT_BASE,
    job_slots: EXECUTION_RCS_JOB_SLOTS,
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

const HELIOC_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: HELIOC_RCS_GPU_VA_RING_BASE,
    context: HELIOC_RCS_GPU_VA_CONTEXT_BASE,
    batch: HELIOC_RCS_GPU_VA_BATCH_BASE,
    result: HELIOC_RCS_GPU_VA_RESULT_BASE,
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

#[derive(Copy, Clone, Debug)]
struct DirectRcsSubmitRuntime {
    context_initialized: bool,
    ring_tail_bytes: usize,
    submissions: u64,
    retire_deferrals: u64,
    pending: Option<crate::gpu::executor::KernelSubmission>,
}

impl DirectRcsSubmitRuntime {
    const fn new() -> Self {
        Self {
            context_initialized: false,
            ring_tail_bytes: 0,
            submissions: 0,
            retire_deferrals: 0,
            pending: None,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct FontRcsPpgttRuntime {
    root_phys: u64,
    generation: u64,
    initialization_attempted: bool,
    initialized: bool,
    retired_ranges: u64,
}

impl FontRcsPpgttRuntime {
    const fn new() -> Self {
        Self {
            root_phys: 0,
            generation: 0,
            initialization_attempted: false,
            initialized: false,
            retired_ranges: 0,
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

fn font_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if font_rcs_context_is_quarantined() {
        return None;
    }

    let mut state_slot = FONT_RCS_STATE.lock();
    if font_rcs_context_is_quarantined() {
        return None;
    }
    if let Some(state) = *state_slot {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(FONT_RCS_GPU_VA)?;
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
    if !super::gen12_lumen_mocs_ready() || lfm25_rcs_context_is_quarantined() {
        return None;
    }

    let mut state_slot = LFM25_RCS_STATE.lock();
    if !super::gen12_lumen_mocs_ready() || lfm25_rcs_context_is_quarantined() {
        return None;
    }
    if let Some(state) = *state_slot {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(LFM25_RCS_GPU_VA)?;
    *state_slot = Some(state);
    Some(state)
}

fn helioc_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if helioc_rcs_context_is_quarantined() {
        return None;
    }

    let mut state_slot = HELIOC_RCS_STATE.lock();
    if helioc_rcs_context_is_quarantined() {
        return None;
    }
    if let Some(state) = *state_slot {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(HELIOC_RCS_GPU_VA)?;
    *state_slot = Some(state);
    Some(state)
}

fn ui4_compositor_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if ui4_compositor_rcs_context_is_quarantined() {
        return None;
    }

    let mut state_slot = UI4_COMPOSITOR_RCS_STATE.lock();
    if ui4_compositor_rcs_context_is_quarantined() {
        return None;
    }
    if let Some(state) = *state_slot {
        return Some(state);
    }

    let state = allocate_direct_rcs_state(UI4_COMPOSITOR_RCS_GPU_VA)?;
    *state_slot = Some(state);
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

/// Select the command/result generation that is safe to rewrite next.
///
/// Execution submissions are strictly ordered and limited to one pending
/// request. Its pending token is cleared only after both the producer marker
/// and the saved LRC head prove context retirement, so alternating slots never
/// treats the marker alone as ownership transfer.
fn execution_rcs_next_job_slot(state: DirectRcsState) -> Option<DirectRcsState> {
    if state.gpu_va.job_slots != EXECUTION_RCS_JOB_SLOTS {
        return None;
    }
    let runtime = EXECUTION_RCS_SUBMIT_RUNTIME.lock();
    if runtime.pending.is_some() {
        return None;
    }
    let slot = (runtime.submissions as usize) % EXECUTION_RCS_JOB_SLOTS;
    drop(runtime);
    direct_rcs_job_slot(state, slot)
}

/// Runtime compatibility gate used by existing consumers. Despite the legacy
/// name, this is deliberately read-only: only boot may install global PTEs.
fn direct_rcs_map_state(_dev: super::Dev, state: DirectRcsState) -> bool {
    direct_rcs_control_ggtt_ready(state)
}

fn install_direct_rcs_control_ggtt_for_boot(dev: super::Dev, state: DirectRcsState) -> bool {
    let Some(mapping) = direct_rcs_ggtt_mapping(state.gpu_va) else {
        return false;
    };
    let mapped = *mapping.call_once(|| {
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
                && super::map_ggtt(
                    dev,
                    state.result_phys,
                    result_alloc_bytes,
                    state.gpu_va.result,
                );
        let auxiliary_mapped = !state.gpu_va.map_general_auxiliary
            || super::map_ggtt(
                dev,
                state.clear_test_phys,
                CLEAR_RECT_TEST_BYTES,
                DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
            );
        let accepted = core_mapped && auxiliary_mapped;
        if accepted {
            super::ggtt_invalidate(dev);
            crate::log_info!(target: "gpgpu";
                "intel/gpgpu: direct-rcs control mapping accepted=1 lane={} ring=0x{:X} context=0x{:X} batch=0x{:X} result=0x{:X} ownership=process-lifetime install=exact-once runtime-remap=forbidden\n",
                direct_rcs_mapping_name(state.gpu_va),
                state.gpu_va.ring,
                state.gpu_va.context,
                state.gpu_va.batch,
                state.gpu_va.result,
            );
        }
        accepted
    });
    if !mapped {
        quarantine_direct_rcs_mapping_failure(state.gpu_va, "ggtt-control-map-failed");
    }
    mapped
}

fn direct_rcs_ggtt_mapping(gpu_va: DirectRcsGpuVa) -> Option<&'static spin::Once<bool>> {
    match gpu_va {
        DIRECT_RCS_GPU_VA => Some(&DIRECT_RCS_GGTT_MAPPING),
        FONT_RCS_GPU_VA => Some(&FONT_RCS_GGTT_MAPPING),
        EXECUTION_RCS_GPU_VA => Some(&EXECUTION_RCS_GGTT_MAPPING),
        LFM25_RCS_GPU_VA => Some(&LFM25_RCS_GGTT_MAPPING),
        HELIOC_RCS_GPU_VA => Some(&HELIOC_RCS_GGTT_MAPPING),
        UI4_COMPOSITOR_RCS_GPU_VA => Some(&UI4_COMPOSITOR_RCS_GGTT_MAPPING),
        _ => None,
    }
}

fn direct_rcs_mapping_name(gpu_va: DirectRcsGpuVa) -> &'static str {
    match gpu_va {
        DIRECT_RCS_GPU_VA => "system-service",
        FONT_RCS_GPU_VA => "font",
        EXECUTION_RCS_GPU_VA => "execution",
        LFM25_RCS_GPU_VA => "lfm25",
        HELIOC_RCS_GPU_VA => "helioc",
        UI4_COMPOSITOR_RCS_GPU_VA => "ui4-compositor",
        _ => "invalid",
    }
}

fn direct_rcs_control_ggtt_ready(state: DirectRcsState) -> bool {
    // Job-slot views change only batch/result offsets; the owning control
    // window is identified by its immutable ring address.
    let mapping = match state.gpu_va.ring {
        DIRECT_RCS_GPU_VA_RING_BASE => &DIRECT_RCS_GGTT_MAPPING,
        FONT_RCS_GPU_VA_RING_BASE => &FONT_RCS_GGTT_MAPPING,
        EXECUTION_RCS_GPU_VA_RING_BASE => &EXECUTION_RCS_GGTT_MAPPING,
        LFM25_RCS_GPU_VA_RING_BASE => &LFM25_RCS_GGTT_MAPPING,
        HELIOC_RCS_GPU_VA_RING_BASE => &HELIOC_RCS_GGTT_MAPPING,
        UI4_COMPOSITOR_RCS_GPU_VA_RING_BASE => &UI4_COMPOSITOR_RCS_GGTT_MAPPING,
        _ => return false,
    };
    mapping.get().copied() == Some(true)
}

fn quarantine_direct_rcs_mapping_failure(gpu_va: DirectRcsGpuVa, reason: &'static str) {
    match gpu_va {
        DIRECT_RCS_GPU_VA => quarantine_direct_rcs_context(reason),
        FONT_RCS_GPU_VA => quarantine_font_rcs_context(reason),
        EXECUTION_RCS_GPU_VA => quarantine_execution_rcs_context(reason),
        LFM25_RCS_GPU_VA => quarantine_lfm25_rcs_context(reason),
        HELIOC_RCS_GPU_VA => quarantine_helioc_rcs_context(reason),
        UI4_COMPOSITOR_RCS_GPU_VA => quarantine_ui4_compositor_rcs_context(reason),
        _ => {}
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct DirectRcsControlGgttPrewarmReport {
    pub(crate) system_service: bool,
    pub(crate) font: bool,
    pub(crate) execution: bool,
    pub(crate) lfm25: bool,
    pub(crate) helioc: bool,
    pub(crate) ui4_compositor: bool,
}

impl DirectRcsControlGgttPrewarmReport {
    pub(crate) const fn accepted(self) -> bool {
        self.system_service
            && self.font
            && self.execution
            && self.lfm25
            && self.helioc
            && self.ui4_compositor
    }
}

fn prewarm_direct_rcs_control_ggtt(
    dev: super::Dev,
    gpu_va: DirectRcsGpuVa,
    state: Option<DirectRcsState>,
) -> bool {
    let Some(state) = state else {
        // Allocation failure must close the lane just as permanently as a
        // partial map failure. Otherwise a later consumer could retry here and
        // become an unplanned writer of the process-global page table.
        if let Some(mapping) = direct_rcs_ggtt_mapping(gpu_va) {
            let ready = *mapping.call_once(|| false);
            if !ready {
                quarantine_direct_rcs_mapping_failure(
                    gpu_va,
                    "boot-control-backing-allocation-failed",
                );
            }
        }
        return false;
    };
    install_direct_rcs_control_ggtt_for_boot(dev, state)
}

/// Install every persistent RCS control window while physical GT bring-up owns
/// the global GGTT boundary. Consumer launch may populate only its private
/// PPGTT; it can neither install nor repair a global ring/HWLRCA mapping. The
/// Font topology is also installed here because its persistent VM cache must
/// exist before any Font consumer starts adding dynamic leaves.
pub(crate) fn prewarm_direct_rcs_controls_ggtt(
    dev: super::Dev,
) -> DirectRcsControlGgttPrewarmReport {
    DirectRcsControlGgttPrewarmReport {
        system_service: prewarm_direct_rcs_control_ggtt(
            dev,
            DIRECT_RCS_GPU_VA,
            direct_rcs_state_once(dev),
        ),
        font: {
            let state = font_rcs_state_once(dev);
            prewarm_direct_rcs_control_ggtt(dev, FONT_RCS_GPU_VA, state)
                && state.is_some_and(font_rcs_init_ppgtt_once)
        },
        execution: prewarm_direct_rcs_control_ggtt(
            dev,
            EXECUTION_RCS_GPU_VA,
            execution_rcs_state_once(dev),
        ),
        lfm25: prewarm_direct_rcs_control_ggtt(dev, LFM25_RCS_GPU_VA, lfm25_rcs_state_once(dev)),
        helioc: prewarm_direct_rcs_control_ggtt(
            dev,
            HELIOC_RCS_GPU_VA,
            helioc_rcs_state_once(dev),
        ),
        ui4_compositor: prewarm_direct_rcs_control_ggtt(
            dev,
            UI4_COMPOSITOR_RCS_GPU_VA,
            ui4_compositor_rcs_state_once(dev),
        ),
    }
}

fn direct_rcs_init_ppgtt(state: DirectRcsState) -> bool {
    // A few Font-owned operations live outside submission_2d (notably the
    // final scanout release packet).  Dispatch on the immutable lane identity
    // here as well as using the explicit helper at Font call sites so none of
    // them can accidentally restore the old whole-PPGTT reset behavior.
    if state.gpu_va == FONT_RCS_GPU_VA {
        return font_rcs_init_ppgtt_once(state);
    }
    direct_rcs_rebuild_ppgtt(state)
}

/// Install the Font context's page-table topology and immutable control leaves
/// exactly once.  Dynamic glyph, recipe, destination, and kernel leaves are
/// incrementally mapped afterwards and remain valid across Font submissions.
fn font_rcs_init_ppgtt_once(state: DirectRcsState) -> bool {
    if state.gpu_va != FONT_RCS_GPU_VA || font_rcs_context_is_quarantined() {
        return false;
    }

    let mut runtime = FONT_RCS_PPGTT_RUNTIME.lock();
    if runtime.initialized {
        let same_generation = runtime.root_phys == state.ppgtt_phys;
        drop(runtime);
        if !same_generation {
            quarantine_font_rcs_context("font-ppgtt-root-generation-mismatch");
        }
        return same_generation;
    }
    if runtime.initialization_attempted {
        return false;
    }

    runtime.initialization_attempted = true;
    runtime.root_phys = state.ppgtt_phys;
    runtime.generation = runtime.generation.saturating_add(1);
    let generation = runtime.generation;
    let initialized = direct_rcs_rebuild_ppgtt(state);
    runtime.initialized = initialized;
    drop(runtime);

    if initialized {
        crate::log_info!(target: "gpgpu";
            "intel/gpgpu: font-ppgtt initialized=1 generation={} root=0x{:X} topology=exact-once dynamic-leaves=incremental whole-table-reset-per-submit=0 isolation=font-context-only\n",
            generation,
            state.ppgtt_phys,
        );
    } else {
        quarantine_font_rcs_context("font-ppgtt-initialization-failed");
    }
    initialized
}

fn direct_rcs_rebuild_ppgtt(state: DirectRcsState) -> bool {
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
    direct_rcs_map_ppgtt_region_and_publish(state, gpu, phys, len, direct_rcs_ppgtt_pte_flags())
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
/// resident-scene direct targets; ordinary kernels and resources remain PAT0/WB.
fn direct_rcs_map_ppgtt_scanout(state: DirectRcsState, gpu: u64, phys: u64, len: usize) -> bool {
    if !super::gen12_integrated_pat_ready() {
        return false;
    }
    let pte_present_rw_pat3_uc = direct_rcs_ppgtt_pte_flags() | GEN8_PAGE_PWT | GEN8_PAGE_PCD;
    let ok = direct_rcs_map_ppgtt_region_and_publish(state, gpu, phys, len, pte_present_rw_pat3_uc);
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DirectRcsPpgttMapResult {
    Unchanged,
    Updated,
    Rejected,
}

impl DirectRcsPpgttMapResult {
    const fn accepted(self) -> bool {
        !matches!(self, Self::Rejected)
    }
}

fn direct_rcs_map_ppgtt_region_and_publish(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    entry_flags: u64,
) -> bool {
    match direct_rcs_update_ppgtt_region(state, gpu, phys, len, entry_flags) {
        DirectRcsPpgttMapResult::Unchanged => true,
        DirectRcsPpgttMapResult::Updated => direct_rcs_flush_ppgtt_pte_range(state, gpu, len),
        DirectRcsPpgttMapResult::Rejected => false,
    }
}

fn direct_rcs_map_ppgtt_region(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    entry_flags: u64,
) -> bool {
    direct_rcs_update_ppgtt_region(state, gpu, phys, len, entry_flags).accepted()
}

fn direct_rcs_update_ppgtt_region(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    entry_flags: u64,
) -> DirectRcsPpgttMapResult {
    if len == 0 || !gpu.is_multiple_of(4096) || !phys.is_multiple_of(4096) {
        return DirectRcsPpgttMapResult::Rejected;
    }
    let Some(end) = u64::try_from(len).ok().and_then(|len| gpu.checked_add(len)) else {
        return DirectRcsPpgttMapResult::Rejected;
    };
    if end > DIRECT_RCS_PPGTT_LIMIT_BYTES {
        return DirectRcsPpgttMapResult::Rejected;
    }

    let pt_off = 12288usize;
    let pages = len.div_ceil(4096);
    let cache_policy_mask = GEN8_PAGE_PWT | GEN8_PAGE_PCD;
    let mut changed = false;
    // Preflight the entire range before changing any leaf. A rejected policy
    // transition must not leave a partially rewritten mapping behind.
    for page in 0..pages {
        let va_page = (gpu >> 12) + page as u64;
        let pd_index = (va_page >> 9) as usize;
        let pt_index = (va_page & 0x1FF) as usize;
        if pd_index >= DIRECT_RCS_PPGTT_PT_COUNT {
            return DirectRcsPpgttMapResult::Rejected;
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
            return DirectRcsPpgttMapResult::Rejected;
        }
        let Some(expected) = (page as u64)
            .checked_mul(4096)
            .and_then(|offset| phys.checked_add(offset))
            .map(|address| (address & !0xFFF) | entry_flags)
        else {
            return DirectRcsPpgttMapResult::Rejected;
        };
        changed |= previous != expected;
    }
    if !changed {
        return DirectRcsPpgttMapResult::Unchanged;
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
            return DirectRcsPpgttMapResult::Rejected;
        };
        let pte_ptr = unsafe { state.ppgtt_virt.add(pte_off) as *mut u64 };
        if unsafe { core::ptr::read_volatile(pte_ptr) } != pte | entry_flags {
            unsafe {
                core::ptr::write_volatile(pte_ptr, pte | entry_flags);
            }
        }
    }
    DirectRcsPpgttMapResult::Updated
}

/// Retire an exact persistent Font VM range after all Font work referencing it
/// has completed.  The expected physical base prevents a stale owner from
/// unmapping a VA already recycled to another allocation.  This touches only
/// the Font context's private PPGTT leaves: no GGTT write, global invalidate,
/// engine reset, or other RCS/Spirit state is involved.
#[allow(dead_code)]
pub(crate) fn retire_font_rcs_ppgtt_range(gpu: u64, phys: u64, len: usize) -> bool {
    let Some(end) = u64::try_from(len).ok().and_then(|len| gpu.checked_add(len)) else {
        return false;
    };
    let in_primary = gpu >= DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE
        && end <= DIRECT_RCS_GPU_VA_FONT_COVERAGE_PRIMARY_LIMIT;
    let in_secondary = gpu >= DIRECT_RCS_GPU_VA_FONT_COVERAGE_SECONDARY_BASE
        && end <= DIRECT_RCS_GPU_VA_FONT_COVERAGE_LIMIT;
    let is_rush_atlas = is_exact_font_rush_rgba8_atlas_range(gpu, len);
    if len == 0
        || !gpu.is_multiple_of(4096)
        || !phys.is_multiple_of(4096)
        || !len.is_multiple_of(4096)
        || (!in_primary && !in_secondary && !is_rush_atlas)
    {
        return false;
    }

    // Every current Font submission keeps this lock through its retirement
    // proof.  Acquiring it therefore excludes both page-table writers and any
    // batch which could still dereference the retired range.
    let _guard = FONT_RCS_SUBMIT_LOCK.lock();
    if font_rcs_context_is_quarantined() || FONT_RCS_SUBMIT_RUNTIME.lock().pending.is_some() {
        return false;
    }
    let Some(state) = *FONT_RCS_STATE.lock() else {
        // No Font PPGTT has ever existed, so this VA has no translation to
        // retire.  Treat teardown as idempotently complete.
        return true;
    };
    let runtime = FONT_RCS_PPGTT_RUNTIME.lock();
    if !runtime.initialization_attempted {
        return true;
    }
    if !runtime.initialized || runtime.root_phys != state.ppgtt_phys {
        return false;
    }
    drop(runtime);

    if !direct_rcs_unmap_ppgtt_region_exact(state, gpu, phys, len)
        || !direct_rcs_flush_ppgtt_pte_range(state, gpu, len)
    {
        return false;
    }
    core::sync::atomic::fence(Ordering::SeqCst);

    let mut runtime = FONT_RCS_PPGTT_RUNTIME.lock();
    runtime.retired_ranges = runtime.retired_ranges.saturating_add(1);
    let retired_ranges = runtime.retired_ranges;
    let generation = runtime.generation;
    drop(runtime);
    if retired_ranges == 1 || retired_ranges.is_power_of_two() {
        crate::log_info!(target: "gpgpu";
            "intel/gpgpu: font-ppgtt range-retired generation={} retired_ranges={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X} ownership=font-context-only pte_flush=range tlb=next-font-batch-prologue\n",
            generation,
            retired_ranges,
            gpu,
            phys,
            len,
        );
    }
    true
}

fn direct_rcs_unmap_ppgtt_region_exact(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
) -> bool {
    if len == 0
        || !gpu.is_multiple_of(4096)
        || !phys.is_multiple_of(4096)
        || !len.is_multiple_of(4096)
    {
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
        if previous & super::GEN8_PAGE_PRESENT == 0 {
            continue;
        }
        let Some(expected_phys) = (page as u64)
            .checked_mul(4096)
            .and_then(|offset| phys.checked_add(offset))
            .map(|address| address & !0xFFF)
        else {
            return false;
        };
        if previous & !0xFFF != expected_phys {
            return false;
        }
    }

    for page in 0..pages {
        let va_page = (gpu >> 12) + page as u64;
        let pd_index = (va_page >> 9) as usize;
        let pt_index = (va_page & 0x1FF) as usize;
        let pte_off = pt_off + pd_index * 4096 + pt_index * core::mem::size_of::<u64>();
        let pte_ptr = unsafe { state.ppgtt_virt.add(pte_off) as *mut u64 };
        unsafe {
            core::ptr::write_volatile(pte_ptr, 0);
        }
    }
    true
}

fn direct_rcs_forcewake(dev: super::Dev) -> bool {
    // Forcewake ownership and device-global RCS workarounds belong to the GT
    // boot boundary. Runtime clients may observe and reject a lost contract,
    // but must never repair shared registers beneath another live context.
    super::physical_gt_ready(dev)
}
