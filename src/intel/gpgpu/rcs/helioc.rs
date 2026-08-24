/// Exact physical UI4 producer description consumed by the HelioC plan
/// preflight. It deliberately has no tenant GPU address field.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelioCloudSurfaceDesc {
    pub(crate) producer_gpu: u64,
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelioCloudDispatchPlan {
    pub(crate) source_volume_gpu: u64,
    pub(crate) destination_volume_gpu: u64,
    pub(crate) sim_params_gpu: u64,
    pub(crate) render_params_gpu: u64,
    pub(crate) dispatch_groups: [u32; 3],
}

/// Fixed instruction-heap placement for the three authenticated native
/// stages. Offsets are relative to the HelioC instruction base; GPU addresses
/// therefore never inherit an ANV process VA.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelioCloudShaderLayout {
    pub(crate) compute_offset: u32,
    pub(crate) vertex_offset: u32,
    pub(crate) fragment_offset: u32,
    pub(crate) used_bytes: u32,
}

/// Proof that all four broker-owned page lists passed immutable HelioC
/// admission. Fields are private so callers cannot manufacture this token.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelioCloudBackingAdmission {
    volume_page_count: u32,
    parameter_page_count: u8,
    disjoint: bool,
}

/// Immutable lowering result for one bounded Cloud frame. The native encoder
/// may consume this plan after the broker lock is dropped; this type performs
/// no mapping, submission, telemetry, or allocation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelioCloudFramePlan {
    pub(crate) dispatches: [HelioCloudDispatchPlan; 2],
    pub(crate) dispatch_count: u8,
    pub(crate) final_volume_gpu: u64,
    pub(crate) surface: HelioCloudSurfaceDesc,
}

/// Type proof that the HelioC lane is exclusively owned from PPGTT updates
/// through the eventual completion/retirement proof. Only this module can
/// construct it, and it always wraps the HelioC-specific lock. Acquisition is
/// deliberately non-blocking: a live Cloud frame yields `Busy` at the broker
/// seam instead of spinning a CPU while the GPU retires it.
struct HelioCloudSubmitGuard {
    _guard: spin::MutexGuard<'static, ()>,
}

fn try_lock_helioc_submit_lane() -> Option<HelioCloudSubmitGuard> {
    Some(HelioCloudSubmitGuard {
        _guard: HELIOC_RCS_SUBMIT_LOCK.try_lock()?,
    })
}

const HELIOC_SIM_PARAMS_GPU: u64 = 0x0900_0000;
const HELIOC_RENDER_PARAMS_GPU: u64 = 0x0901_0000;
const HELIOC_DISPATCH_GROUPS: [u32; 3] = [24, 12, 24];

const HELIOC_PAGE_BYTES: usize = 4096;
const HELIOC_SHADER_ALIGNMENT_BYTES: usize = 64;
const HELIOC_VOLUME_PAGE_COUNT: usize = HELIOC_VOLUME_RGBA16F_BYTES / HELIOC_PAGE_BYTES;
const HELIOC_PARAM_PAGE_COUNT: usize = 1;

fn helioc_align_up(value: usize, alignment: usize) -> Option<usize> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return None;
    }
    let mask = alignment - 1;
    Some(value.checked_add(mask)? & !mask)
}

/// Place the already-authenticated CS/VS/FS byte ranges in the persistent
/// HelioC shader window. The state package records stage-relative KSP offsets;
/// this function owns only the bounded TRUEOS heap placement.
pub(crate) fn plan_helioc_shader_layout(
    compute_bytes: usize,
    vertex_bytes: usize,
    fragment_bytes: usize,
) -> Option<HelioCloudShaderLayout> {
    if [compute_bytes, vertex_bytes, fragment_bytes]
        .into_iter()
        .any(|bytes| bytes == 0 || !bytes.is_multiple_of(core::mem::size_of::<u32>()))
    {
        return None;
    }
    let compute_offset = 0usize;
    let vertex_offset = helioc_align_up(compute_bytes, HELIOC_SHADER_ALIGNMENT_BYTES)?;
    let vertex_end = vertex_offset.checked_add(vertex_bytes)?;
    let fragment_offset = helioc_align_up(vertex_end, HELIOC_SHADER_ALIGNMENT_BYTES)?;
    let used_bytes = fragment_offset.checked_add(fragment_bytes)?;
    if used_bytes > HELIOC_RCS_SHADER_BYTES {
        return None;
    }
    Some(HelioCloudShaderLayout {
        compute_offset: u32::try_from(compute_offset).ok()?,
        vertex_offset: u32::try_from(vertex_offset).ok()?,
        fragment_offset: u32::try_from(fragment_offset).ok()?,
        used_bytes: u32::try_from(used_bytes).ok()?,
    })
}

fn helioc_terminal_epilogue_valid(batch: &[u32]) -> bool {
    if batch.len() < HELIOC_RCS_TERMINAL_EPILOGUE_DWORDS {
        return false;
    }
    let terminal = &batch[batch.len() - HELIOC_RCS_TERMINAL_EPILOGUE_DWORDS..];
    terminal
        == [
            PIPE_CONTROL_CMD | PIPE_CONTROL_HDC_PIPELINE_FLUSH,
            PIPE_CONTROL_FLUSH_BITS | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH,
            0,
            0,
            0,
            0,
            PIPE_CONTROL_CMD,
            PIPE_CONTROL_FLUSH_ENABLE
                | PIPE_CONTROL_CS_STALL
                | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,
            HELIOC_RCS_GPU_VA_RESULT_BASE as u32,
            (HELIOC_RCS_GPU_VA_RESULT_BASE >> 32) as u32,
            HELIOC_RCS_COMPLETION_MARKER,
            0,
            MI_BATCH_BUFFER_END,
        ]
}

/// Select one authenticated HELIOCRS batch object inside the already-mapped
/// HelioC batch allocation. The selected view is submit-only: PPGTT topology
/// must be initialized from the owning base state before this offset is
/// applied. Requiring the fixed terminal release/marker sequence makes a
/// marker observation useful without borrowing a fence address from ANV.
fn select_helioc_materialized_batch(
    state: DirectRcsState,
    batch: helioc_native_package::HelioCloudMaterializedBatch,
) -> Option<DirectRcsState> {
    let offset = usize::try_from(batch.offset).ok()?;
    let length = usize::try_from(batch.length).ok()?;
    let end = offset.checked_add(length)?;
    if state.gpu_va != HELIOC_RCS_GPU_VA
        || state.gpu_va.job_slots != 1
        || state.batch_virt.is_null()
        || !offset.is_multiple_of(8)
        || !length.is_multiple_of(core::mem::size_of::<u32>())
        || end > DIRECT_RCS_BATCH_BYTES
    {
        return None;
    }
    let dwords = unsafe {
        core::slice::from_raw_parts(
            state.batch_virt.add(offset).cast::<u32>(),
            length / core::mem::size_of::<u32>(),
        )
    };
    if !helioc_terminal_epilogue_valid(dwords) {
        return None;
    }

    let mut selected = state;
    selected.batch_phys = selected.batch_phys.checked_add(offset as u64)?;
    selected.batch_virt = unsafe { selected.batch_virt.add(offset) };
    selected.gpu_va.batch = selected.gpu_va.batch.checked_add(offset as u64)?;
    Some(selected)
}

fn helioc_exact_pages_valid(pages: &[u64], expected_count: usize) -> bool {
    pages.len() == expected_count
        && pages
            .iter()
            .all(|phys| *phys != 0 && phys.is_multiple_of(HELIOC_PAGE_BYTES as u64))
}

fn helioc_pages_are_disjoint(resources: [&[u64]; 4]) -> bool {
    for (index, pages) in resources.iter().enumerate() {
        for page in pages.iter().copied() {
            if resources[..index].iter().any(|prior| prior.contains(&page)) {
                return false;
            }
        }
        for (offset, page) in pages.iter().copied().enumerate() {
            if pages[offset + 1..].contains(&page) {
                return false;
            }
        }
    }
    true
}

/// Perform the expensive exact-count/alignment/alias proof once at graph
/// creation. Frame planning consumes only the resulting opaque token.
pub(crate) fn admit_helioc_backing(
    volume_a_pages: &[u64],
    volume_b_pages: &[u64],
    sim_params_pages: &[u64],
    render_params_pages: &[u64],
) -> Option<HelioCloudBackingAdmission> {
    let resources = [volume_a_pages, volume_b_pages, sim_params_pages, render_params_pages];
    if !helioc_exact_pages_valid(volume_a_pages, HELIOC_VOLUME_PAGE_COUNT)
        || !helioc_exact_pages_valid(volume_b_pages, HELIOC_VOLUME_PAGE_COUNT)
        || !helioc_exact_pages_valid(sim_params_pages, HELIOC_PARAM_PAGE_COUNT)
        || !helioc_exact_pages_valid(render_params_pages, HELIOC_PARAM_PAGE_COUNT)
        || !helioc_pages_are_disjoint(resources)
    {
        return None;
    }
    Some(HelioCloudBackingAdmission {
        volume_page_count: HELIOC_VOLUME_PAGE_COUNT as u32,
        parameter_page_count: HELIOC_PARAM_PAGE_COUNT as u8,
        disjoint: true,
    })
}

fn helioc_surface_valid(surface: HelioCloudSurfaceDesc) -> bool {
    if surface.phys == 0
        || !surface.phys.is_multiple_of(HELIOC_PAGE_BYTES as u64)
        || !surface.producer_gpu.is_multiple_of(HELIOC_PAGE_BYTES as u64)
        || surface.bytes == 0
        || !surface.bytes.is_multiple_of(HELIOC_PAGE_BYTES)
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < surface.width.saturating_mul(4)
    {
        return false;
    }
    let Some(last_row) = (surface.height as usize - 1)
        .checked_mul(surface.pitch_bytes as usize)
    else {
        return false;
    };
    let Some(row_bytes) = (surface.width as usize).checked_mul(4) else {
        return false;
    };
    let Some(extent) = last_row.checked_add(row_bytes) else {
        return false;
    };
    extent <= surface.bytes
        && surface.producer_gpu >= crate::r::ui_surface::UI_SURFACE_GPU_BASE
        && surface
            .producer_gpu
            .checked_add(surface.bytes as u64)
            .is_some_and(|end| end <= crate::r::ui_surface::UI_SURFACE_GPU_LIMIT)
}

/// Reject a surface whose physical backing overlaps any admitted Cloud page.
/// This is intentionally a linear scan over the immutable page snapshots;
/// the expensive four-resource pairwise proof happens only at admission.
pub(crate) fn helioc_surface_overlaps_backing(
    surface: HelioCloudSurfaceDesc,
    resources: [&[u64]; 4],
) -> bool {
    let Some(surface_end) = surface.phys.checked_add(surface.bytes as u64) else {
        return true;
    };
    resources.into_iter().flatten().any(|page| {
        page.checked_add(HELIOC_PAGE_BYTES as u64)
            .is_none_or(|page_end| surface.phys < page_end && *page < surface_end)
    })
}

const fn helioc_volume_gpu(selector: u8) -> u64 {
    if selector == 0 {
        HELIOC_RCS_GPU_VA_VOLUME_A_BASE
    } else {
        HELIOC_RCS_GPU_VA_VOLUME_B_BASE
    }
}

/// Validate exact Cloud backing and derive the fixed Helio VA ping-pong
/// sequence. `current_volume` is broker-owned state; no tenant GPU VA is
/// accepted or copied into the result.
pub(crate) fn plan_helioc_frame(
    admission: HelioCloudBackingAdmission,
    surface: HelioCloudSurfaceDesc,
    simulation_steps: u32,
    current_volume: u8,
) -> Option<HelioCloudFramePlan> {
    if simulation_steps > 2
        || current_volume > 1
        || admission.volume_page_count != HELIOC_VOLUME_PAGE_COUNT as u32
        || admission.parameter_page_count != HELIOC_PARAM_PAGE_COUNT as u8
        || !admission.disjoint
    {
        return None;
    }
    if !helioc_surface_valid(surface) {
        return None;
    }

    let first_source = helioc_volume_gpu(current_volume);
    let first_destination = helioc_volume_gpu(current_volume ^ 1);
    let second = HelioCloudDispatchPlan {
        source_volume_gpu: first_destination,
        destination_volume_gpu: first_source,
        sim_params_gpu: HELIOC_SIM_PARAMS_GPU,
        render_params_gpu: HELIOC_RENDER_PARAMS_GPU,
        dispatch_groups: HELIOC_DISPATCH_GROUPS,
    };
    let first = HelioCloudDispatchPlan {
        source_volume_gpu: first_source,
        destination_volume_gpu: first_destination,
        sim_params_gpu: HELIOC_SIM_PARAMS_GPU,
        render_params_gpu: HELIOC_RENDER_PARAMS_GPU,
        dispatch_groups: HELIOC_DISPATCH_GROUPS,
    };
    let dispatches = if simulation_steps == 2 {
        [first, second]
    } else {
        [first, first]
    };
    let final_volume_gpu = if simulation_steps & 1 == 0 {
        first_source
    } else {
        first_destination
    };
    Some(HelioCloudFramePlan {
        dispatches,
        dispatch_count: simulation_steps as u8,
        final_volume_gpu,
        surface,
    })
}

fn helioc_plan_vas_valid(plan: &HelioCloudFramePlan) -> bool {
    let dispatch_valid = |dispatch: &HelioCloudDispatchPlan| {
        dispatch.sim_params_gpu == HELIOC_SIM_PARAMS_GPU
            && dispatch.render_params_gpu == HELIOC_RENDER_PARAMS_GPU
            && dispatch.dispatch_groups == HELIOC_DISPATCH_GROUPS
            && matches!(
                (dispatch.source_volume_gpu, dispatch.destination_volume_gpu),
                (HELIOC_RCS_GPU_VA_VOLUME_A_BASE, HELIOC_RCS_GPU_VA_VOLUME_B_BASE)
                    | (HELIOC_RCS_GPU_VA_VOLUME_B_BASE, HELIOC_RCS_GPU_VA_VOLUME_A_BASE)
            )
    };
    let [first, second] = &plan.dispatches;
    if !dispatch_valid(first)
        || !dispatch_valid(second)
        || !helioc_surface_valid(plan.surface)
    {
        return false;
    }
    match plan.dispatch_count {
        0 => plan.final_volume_gpu == first.source_volume_gpu,
        1 => plan.final_volume_gpu == first.destination_volume_gpu,
        2 => {
            second.source_volume_gpu == first.destination_volume_gpu
                && second.destination_volume_gpu == first.source_volume_gpu
                && plan.final_volume_gpu == second.destination_volume_gpu
        }
        _ => false,
    }
}

/// Map one already-admitted frame into the private HelioC PPGTT. The HelioC
/// submit guard covers both the all-ranges policy preflight and every leaf
/// update. The eventual encoder must keep that same guard through completion
/// and retirement, so another frame cannot replace live mappings. This helper
/// only prepares mappings; it does not encode, submit, poll, or emit telemetry.
/// Its caller must also retain the broker lease whose page snapshots produced
/// `admission` until the submission retires.
fn map_helioc_frame_resources(
    _submit_guard: &HelioCloudSubmitGuard,
    state: DirectRcsState,
    admission: HelioCloudBackingAdmission,
    plan: &HelioCloudFramePlan,
    volume_a_pages: &[u64],
    volume_b_pages: &[u64],
    sim_params_pages: &[u64],
    render_params_pages: &[u64],
) -> bool {
    let resources = [
        volume_a_pages,
        volume_b_pages,
        sim_params_pages,
        render_params_pages,
    ];
    if state.gpu_va != HELIOC_RCS_GPU_VA
        || !admission.disjoint
        || admission.volume_page_count != HELIOC_VOLUME_PAGE_COUNT as u32
        || admission.parameter_page_count != HELIOC_PARAM_PAGE_COUNT as u8
        || !helioc_plan_vas_valid(plan)
        || !helioc_exact_pages_valid(volume_a_pages, HELIOC_VOLUME_PAGE_COUNT)
        || !helioc_exact_pages_valid(volume_b_pages, HELIOC_VOLUME_PAGE_COUNT)
        || !helioc_exact_pages_valid(sim_params_pages, HELIOC_PARAM_PAGE_COUNT)
        || !helioc_exact_pages_valid(render_params_pages, HELIOC_PARAM_PAGE_COUNT)
        || !helioc_pages_are_disjoint(resources)
        || helioc_surface_overlaps_backing(plan.surface, resources)
        || validate_exact_ppgtt_page_map(
            HELIOC_RCS_GPU_VA_VOLUME_A_BASE,
            volume_a_pages.len(),
        )
        .is_none()
        || validate_exact_ppgtt_page_map(
            HELIOC_RCS_GPU_VA_VOLUME_B_BASE,
            volume_b_pages.len(),
        )
        .is_none()
        || validate_exact_ppgtt_page_map(HELIOC_SIM_PARAMS_GPU, sim_params_pages.len()).is_none()
        || validate_exact_ppgtt_page_map(HELIOC_RENDER_PARAMS_GPU, render_params_pages.len())
            .is_none()
    {
        return false;
    }
    if !direct_rcs_init_ppgtt(state)
        || !direct_rcs_preflight_ppgtt_kernel_pages(
            state,
            HELIOC_RCS_GPU_VA_VOLUME_A_BASE,
            volume_a_pages,
        )
        || !direct_rcs_preflight_ppgtt_kernel_pages(
            state,
            HELIOC_RCS_GPU_VA_VOLUME_B_BASE,
            volume_b_pages,
        )
        || !direct_rcs_preflight_ppgtt_kernel_pages(
            state,
            HELIOC_SIM_PARAMS_GPU,
            sim_params_pages,
        )
        || !direct_rcs_preflight_ppgtt_kernel_pages(
            state,
            HELIOC_RENDER_PARAMS_GPU,
            render_params_pages,
        )
        || !direct_rcs_preflight_ppgtt_kernel_region(
            state,
            plan.surface.producer_gpu,
            plan.surface.phys,
            plan.surface.bytes,
        )
    {
        return false;
    }
    if !direct_rcs_map_ppgtt_kernel_pages_and_publish(
            state,
            HELIOC_RCS_GPU_VA_VOLUME_A_BASE,
            volume_a_pages,
        )
        || !direct_rcs_map_ppgtt_kernel_pages_and_publish(
            state,
            HELIOC_RCS_GPU_VA_VOLUME_B_BASE,
            volume_b_pages,
        )
        || !direct_rcs_map_ppgtt_kernel_pages_and_publish(
            state,
            HELIOC_SIM_PARAMS_GPU,
            sim_params_pages,
        )
        || !direct_rcs_map_ppgtt_kernel_pages_and_publish(
            state,
            HELIOC_RENDER_PARAMS_GPU,
            render_params_pages,
        )
    {
        return false;
    }
    direct_rcs_map_ppgtt_destination(
        state,
        plan.surface.producer_gpu,
        plan.surface.phys,
        plan.surface.bytes,
        false,
    )
}

/// A submitted HelioC frame owns the lane guard until its marker and saved LRC
/// head prove retirement. Dropping an unfinished ticket quarantines the lane:
/// it is never safe to let a later frame rewrite this frame's PPGTT leaves or
/// state arena merely because its service owner disappeared.
#[must_use = "observe the ticket until it retires, or it will quarantine the HelioC lane"]
pub(crate) struct HelioCloudNativeTicket {
    guard: Option<HelioCloudSubmitGuard>,
    state: DirectRcsState,
    surface: HelioCloudSurfaceDesc,
}

impl Drop for HelioCloudNativeTicket {
    fn drop(&mut self) {
        if self.guard.is_some() {
            quarantine_helioc_rcs_context("helioc-native-ticket-dropped-before-retirement");
        }
    }
}

/// Submission state is explicit so the service task can return to its
/// executor between attempts. `Ambiguous` has already quarantined the lane;
/// it never carries a ticket whose backing could be reused.
pub(crate) enum HelioCloudNativeSubmit {
    Busy,
    Rejected,
    Ambiguous,
    Submitted(HelioCloudNativeTicket),
}

/// One non-blocking retirement observation. `Complete` is the only outcome
/// that mints a display release for the exact UI4 allocation.
pub(crate) enum HelioCloudNativeRetirement {
    Pending,
    Complete(GpgpuRgba8ReleaseFence),
    Invalid,
}

static HELIOC_RGBA8_RELEASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn helioc_materialization_variant(plan: &HelioCloudFramePlan) -> Option<u8> {
    if !helioc_plan_vas_valid(plan) {
        return None;
    }
    let current_volume: u8 = match plan.dispatches[0].source_volume_gpu {
        HELIOC_RCS_GPU_VA_VOLUME_A_BASE => 0,
        HELIOC_RCS_GPU_VA_VOLUME_B_BASE => 1,
        _ => return None,
    };
    current_volume
        .checked_mul(3)?
        .checked_add(plan.dispatch_count)
        .filter(|variant| *variant < 6)
}

fn helioc_state_window(
    state: DirectRcsState,
    gpu: u64,
    bytes: usize,
) -> Option<&'static mut [u8]> {
    let offset = usize::try_from(gpu.checked_sub(HELIOC_RCS_GPU_VA_SHADER_BASE)?).ok()?;
    let end = offset.checked_add(bytes)?;
    if state.gpu_va != HELIOC_RCS_GPU_VA
        || state.helioc_state_virt.is_null()
        || state.helioc_state_bytes != HELIOC_RCS_STATE_ARENA_BYTES
        || end > state.helioc_state_bytes
    {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts_mut(state.helioc_state_virt.add(offset), bytes) })
}

fn materialize_helioc_native_frame(
    state: DirectRcsState,
    package: helioc_native_package::NativePackage<'_>,
    plan: &HelioCloudFramePlan,
) -> Option<helioc_native_package::HelioCloudMaterializedBatch> {
    let compute = package.compute_isa()?;
    let vertex = package.vertex_isa()?;
    let fragment = package.fragment_isa()?;
    let reloc = package.reloc_state()?;
    let layout = plan_helioc_shader_layout(compute.len(), vertex.len(), fragment.len())?;
    let variant = helioc_materialization_variant(plan)?;
    if state.gpu_va != HELIOC_RCS_GPU_VA
        || state.batch_virt.is_null()
        || state.helioc_state_virt.is_null()
        || state.helioc_state_bytes != HELIOC_RCS_STATE_ARENA_BYTES
    {
        return None;
    }
    unsafe { core::ptr::write_bytes(state.helioc_state_virt, 0, state.helioc_state_bytes) };
    let shader = helioc_state_window(
        state,
        HELIOC_RCS_GPU_VA_SHADER_BASE,
        HELIOC_RCS_SHADER_BYTES,
    )?;
    let compute_offset = usize::try_from(layout.compute_offset).ok()?;
    let vertex_offset = usize::try_from(layout.vertex_offset).ok()?;
    let fragment_offset = usize::try_from(layout.fragment_offset).ok()?;
    let used_bytes = usize::try_from(layout.used_bytes).ok()?;
    if used_bytes > shader.len()
        || compute_offset.checked_add(compute.len())? > vertex_offset
        || vertex_offset.checked_add(vertex.len())? > fragment_offset
        || fragment_offset.checked_add(fragment.len())? > used_bytes
    {
        return None;
    }
    shader[compute_offset..compute_offset + compute.len()].copy_from_slice(compute);
    shader[vertex_offset..vertex_offset + vertex.len()].copy_from_slice(vertex);
    shader[fragment_offset..fragment_offset + fragment.len()].copy_from_slice(fragment);

    let surface = helioc_state_window(
        state,
        HELIOC_RCS_GPU_VA_SURFACE_STATE_BASE,
        HELIOC_RCS_SURFACE_STATE_BYTES,
    )?;
    let dynamic = helioc_state_window(
        state,
        HELIOC_RCS_GPU_VA_DYNAMIC_STATE_BASE,
        HELIOC_RCS_DYNAMIC_STATE_BYTES,
    )?;
    let indirect = helioc_state_window(
        state,
        HELIOC_RCS_GPU_VA_INDIRECT_DESCRIPTOR_BASE,
        HELIOC_RCS_INDIRECT_DESCRIPTOR_BYTES,
    )?;
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt, DIRECT_RCS_BATCH_BYTES) };
    let mut windows = helioc_native_package::HelioCloudMaterializationWindows {
        batch,
        surface,
        dynamic,
        indirect,
        batch_gpu: HELIOC_RCS_GPU_VA_BATCH_BASE,
        surface_gpu: HELIOC_RCS_GPU_VA_SURFACE_STATE_BASE,
        dynamic_gpu: HELIOC_RCS_GPU_VA_DYNAMIC_STATE_BASE,
        indirect_gpu: HELIOC_RCS_GPU_VA_INDIRECT_DESCRIPTOR_BASE,
    };
    let symbols = helioc_native_package::HelioCloudMaterializationSymbols {
        cs: HELIOC_RCS_GPU_VA_SHADER_BASE.checked_add(u64::from(layout.compute_offset))?,
        vs: HELIOC_RCS_GPU_VA_SHADER_BASE.checked_add(u64::from(layout.vertex_offset))?,
        fs: HELIOC_RCS_GPU_VA_SHADER_BASE.checked_add(u64::from(layout.fragment_offset))?,
        surface: HELIOC_RCS_GPU_VA_SURFACE_STATE_BASE,
        dynamic: HELIOC_RCS_GPU_VA_DYNAMIC_STATE_BASE,
        indirect: HELIOC_RCS_GPU_VA_INDIRECT_DESCRIPTOR_BASE,
        batch: HELIOC_RCS_GPU_VA_BATCH_BASE,
        result: HELIOC_RCS_GPU_VA_RESULT_BASE,
        volume_a: HELIOC_RCS_GPU_VA_VOLUME_A_BASE,
        volume_b: HELIOC_RCS_GPU_VA_VOLUME_B_BASE,
        sim_params: HELIOC_SIM_PARAMS_GPU,
        render_params: HELIOC_RENDER_PARAMS_GPU,
        ui4_producer_gpu: plan.surface.producer_gpu,
        width: plan.surface.width,
        height: plan.surface.height,
        pitch: plan.surface.pitch_bytes,
    };
    let materialized = helioc_native_package::materialize_relocatable_state(
        reloc,
        &mut windows,
        symbols,
        variant,
    )
    .ok()?;
    super::dma_flush(state.helioc_state_virt, state.helioc_state_bytes);
    Some(materialized)
}

/// Submit one package-authenticated Cloud frame. The package must already
/// have been parsed from a validated HELIOA container; this boundary accepts
/// neither raw ISA nor package-provided addresses. It performs no retirement
/// polling, leaving that to `observe_helioc_native_retirement` on later
/// executor turns.
pub(crate) fn submit_helioc_native_frame(
    package: helioc_native_package::NativePackage<'_>,
    admission: HelioCloudBackingAdmission,
    plan: HelioCloudFramePlan,
    volume_a_pages: &[u64],
    volume_b_pages: &[u64],
    sim_params_pages: &[u64],
    render_params_pages: &[u64],
) -> HelioCloudNativeSubmit {
    let Some(guard) = try_lock_helioc_submit_lane() else {
        return HelioCloudNativeSubmit::Busy;
    };
    let Some(dev) = crate::intel::claimed_device() else {
        return HelioCloudNativeSubmit::Rejected;
    };
    let Some(state) = helioc_rcs_state_once(dev) else {
        return HelioCloudNativeSubmit::Rejected;
    };
    let Some(batch) = materialize_helioc_native_frame(state, package, &plan) else {
        return HelioCloudNativeSubmit::Rejected;
    };
    if !map_helioc_frame_resources(
        &guard,
        state,
        admission,
        &plan,
        volume_a_pages,
        volume_b_pages,
        sim_params_pages,
        render_params_pages,
    ) {
        return HelioCloudNativeSubmit::Rejected;
    }
    match helioc_rcs_submit_batch_state(&guard, dev, state, batch) {
        DirectRcsSubmissionState::Rejected => HelioCloudNativeSubmit::Rejected,
        DirectRcsSubmissionState::Ambiguous => HelioCloudNativeSubmit::Ambiguous,
        DirectRcsSubmissionState::Submitted => {
            HelioCloudNativeSubmit::Submitted(HelioCloudNativeTicket {
                guard: Some(guard),
                state,
                surface: plan.surface,
            })
        }
    }
}

/// Observe an outstanding Cloud frame exactly once. A release token is minted
/// only after the package's terminal post-sync marker and the saved LRC head
/// agree; this never launches a second generic release submission.
pub(crate) fn observe_helioc_native_retirement(
    ticket: &mut HelioCloudNativeTicket,
) -> HelioCloudNativeRetirement {
    let Some(guard) = ticket.guard.as_ref() else {
        return HelioCloudNativeRetirement::Invalid;
    };
    match helioc_rcs_observe_retirement(guard, ticket.state) {
        HelioCloudRcsRetirement::Pending => HelioCloudNativeRetirement::Pending,
        HelioCloudRcsRetirement::Invalid => {
            quarantine_helioc_rcs_context("helioc-native-retirement-proof-invalid");
            ticket.guard.take();
            HelioCloudNativeRetirement::Invalid
        }
        HelioCloudRcsRetirement::Complete => {
            let sequence = HELIOC_RGBA8_RELEASE_SEQUENCE
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_add(1)
                .max(1);
            let release = GpgpuRgba8ReleaseFence {
                phys: ticket.surface.phys,
                byte_len: ticket.surface.bytes,
                sequence,
            };
            ticket.guard.take();
            HelioCloudNativeRetirement::Complete(release)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: u64 = 0x1000;
    const VOLUME_PAGE: u64 = 0x20_0000;

    fn pages(count: usize, base: u64) -> [u64; HELIOC_VOLUME_PAGE_COUNT] {
        let mut result = [0; HELIOC_VOLUME_PAGE_COUNT];
        assert_eq!(count, result.len());
        for (index, page) in result.iter_mut().enumerate() {
            *page = base + index as u64 * PAGE;
        }
        result
    }

    fn surface() -> HelioCloudSurfaceDesc {
        HelioCloudSurfaceDesc {
            producer_gpu: crate::r::ui_surface::UI_SURFACE_GPU_BASE,
            phys: 0x40_0000,
            bytes: 1920 * 1080 * 4,
            width: 1920,
            height: 1080,
            pitch_bytes: 1920 * 4,
        }
    }

    fn plan(steps: u32, selector: u8) -> HelioCloudFramePlan {
        let a = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE);
        let b = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2);
        let admission = admit_helioc_backing(&a, &b, &[PAGE], &[PAGE * 2]).unwrap();
        plan_helioc_frame(admission, surface(), steps, selector).unwrap()
    }

    #[test]
    fn zero_steps_samples_selected_volume_without_dispatch() {
        let plan = plan(0, 1);
        assert_eq!(plan.dispatch_count, 0);
        assert_eq!(plan.final_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
    }

    #[test]
    fn one_step_flips_once() {
        let plan = plan(1, 0);
        assert_eq!(plan.dispatch_count, 1);
        assert_eq!(
            plan.dispatches[0],
            HelioCloudDispatchPlan {
                source_volume_gpu: HELIOC_RCS_GPU_VA_VOLUME_A_BASE,
                destination_volume_gpu: HELIOC_RCS_GPU_VA_VOLUME_B_BASE,
                sim_params_gpu: HELIOC_SIM_PARAMS_GPU,
                render_params_gpu: HELIOC_RENDER_PARAMS_GPU,
                dispatch_groups: HELIOC_DISPATCH_GROUPS,
            }
        );
        assert_eq!(plan.final_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
    }

    #[test]
    fn two_steps_returns_to_original_volume() {
        let plan = plan(2, 1);
        assert_eq!(plan.dispatch_count, 2);
        assert_eq!(plan.dispatches[0].source_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
        assert_eq!(plan.dispatches[1].destination_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
        assert_eq!(plan.final_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
    }

    #[test]
    fn exact_shapes_alignment_and_surface_are_fail_closed() {
        let a = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE);
        let b = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2);
        let admission = admit_helioc_backing(&a, &b, &[PAGE], &[PAGE * 2]).unwrap();
        assert!(plan_helioc_frame(admission, surface(), 2, 0).is_some());
        assert!(admit_helioc_backing(&a[..a.len() - 1], &b, &[PAGE], &[PAGE * 2]).is_none());
        assert!(admit_helioc_backing(&a, &b, &[PAGE + 1], &[PAGE * 2]).is_none());
        assert!(plan_helioc_frame(admission, surface(), 3, 0).is_none());
        assert!(plan_helioc_frame(admission, surface(), 0, 2).is_none());
        let mut bad_surface = surface();
        bad_surface.pitch_bytes -= 4;
        assert!(plan_helioc_frame(admission, bad_surface, 0, 0).is_none());
        bad_surface = surface();
        bad_surface.phys += 1;
        assert!(plan_helioc_frame(admission, bad_surface, 0, 0).is_none());
        bad_surface = surface();
        bad_surface.producer_gpu = crate::r::ui_surface::UI_SURFACE_GPU_BASE - PAGE;
        assert!(plan_helioc_frame(admission, bad_surface, 0, 0).is_none());
    }

    #[test]
    fn rejects_physical_page_aliases_across_cloud_resources() {
        let a = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE);
        let mut b = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2);
        b[0] = a[0];
        assert!(admit_helioc_backing(&a, &b, &[PAGE], &[PAGE * 2]).is_none());
        assert!(admit_helioc_backing(
            &a,
            &pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2),
            &[a[1]],
            &[PAGE * 2]
        )
        .is_none());
    }

    #[test]
    fn admission_token_seals_valid_backing_for_all_step_shapes() {
        let a = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE);
        let b = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2);
        let admission = admit_helioc_backing(&a, &b, &[PAGE], &[PAGE * 2]).unwrap();
        for steps in 0..=2 {
            assert!(plan_helioc_frame(admission, surface(), steps, 0).is_some());
        }
    }

    #[test]
    fn every_dispatch_uses_fixed_parameters_and_groups() {
        let frame = plan(2, 0);
        for dispatch in frame.dispatches {
            assert_eq!(dispatch.sim_params_gpu, HELIOC_SIM_PARAMS_GPU);
            assert_eq!(dispatch.render_params_gpu, HELIOC_RENDER_PARAMS_GPU);
            assert_eq!(dispatch.dispatch_groups, HELIOC_DISPATCH_GROUPS);
        }
    }

    #[test]
    fn native_mapping_preflight_rejects_forged_plan_addresses_and_parity() {
        let valid = plan(2, 0);
        assert!(helioc_plan_vas_valid(&valid));

        let mut forged = valid;
        forged.dispatches[0].sim_params_gpu += PAGE;
        assert!(!helioc_plan_vas_valid(&forged));

        forged = valid;
        forged.dispatches[1].source_volume_gpu = HELIOC_RCS_GPU_VA_VOLUME_A_BASE;
        assert!(!helioc_plan_vas_valid(&forged));

        forged = valid;
        forged.final_volume_gpu = HELIOC_RCS_GPU_VA_VOLUME_B_BASE;
        assert!(!helioc_plan_vas_valid(&forged));

        forged = valid;
        forged.dispatch_count = 3;
        assert!(!helioc_plan_vas_valid(&forged));
    }

    #[test]
    fn surface_physical_overlap_is_fail_closed() {
        let pages = [0x20_0000, 0x30_0000, 0x40_0000, 0x50_0000];
        let resources = [&pages[..1], &pages[1..2], &pages[2..3], &pages[3..]];
        let mut exact = surface();
        exact.phys = pages[0];
        assert!(helioc_surface_overlaps_backing(exact, resources));

        let mut partial = surface();
        partial.phys = pages[1] - 0x800;
        partial.bytes = 0x2000;
        assert!(helioc_surface_overlaps_backing(partial, resources));

        let mut separate = surface();
        separate.phys = 0x60_0000;
        assert!(!helioc_surface_overlaps_backing(separate, resources));
    }

    #[test]
    fn captured_native_stages_fit_the_isolated_shader_heap() {
        let layout = plan_helioc_shader_layout(64_544, 368, 13_504).unwrap();
        assert_eq!(
            layout,
            HelioCloudShaderLayout {
                compute_offset: 0,
                vertex_offset: 64_576,
                fragment_offset: 64_960,
                used_bytes: 78_464,
            }
        );
        assert!((layout.vertex_offset as usize).is_multiple_of(HELIOC_SHADER_ALIGNMENT_BYTES));
        assert!((layout.fragment_offset as usize).is_multiple_of(HELIOC_SHADER_ALIGNMENT_BYTES));
        assert!(layout.used_bytes as usize <= HELIOC_RCS_SHADER_BYTES);
    }

    #[test]
    fn terminal_epilogue_is_ordered_and_helioc_owned() {
        let mut batch = [0u32; 16];
        let mut cursor = 0usize;
        assert!(direct_rcs_push_pipe_control(
            &mut batch,
            &mut cursor,
            PIPE_CONTROL_FLUSH_BITS | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH,
        ));
        assert!(direct_rcs_push_pipe_control_post_sync_marker_at(
            &mut batch,
            &mut cursor,
            HELIOC_RCS_GPU_VA_RESULT_BASE,
            HELIOC_RCS_COMPLETION_SLOT,
            HELIOC_RCS_COMPLETION_MARKER,
        ));
        assert!(direct_rcs_push(&mut batch, &mut cursor, MI_BATCH_BUFFER_END));
        assert_eq!(cursor, HELIOC_RCS_TERMINAL_EPILOGUE_DWORDS);
        assert!(helioc_terminal_epilogue_valid(&batch[..cursor]));

        batch[cursor - 3] ^= 1;
        assert!(!helioc_terminal_epilogue_valid(&batch[..cursor]));
    }

    #[test]
    fn shader_heap_layout_rejects_unaligned_empty_and_oversized_code() {
        assert!(plan_helioc_shader_layout(0, 368, 13_504).is_none());
        assert!(plan_helioc_shader_layout(64_545, 368, 13_504).is_none());
        assert!(plan_helioc_shader_layout(HELIOC_RCS_SHADER_BYTES, 4, 4).is_none());
    }

    #[test]
    fn materialization_variant_binds_current_volume_and_step_count() {
        for current_volume in 0..=1 {
            for steps in 0..=2 {
                assert_eq!(
                    helioc_materialization_variant(&plan(steps, current_volume)),
                    Some(current_volume * 3 + steps)
                );
            }
        }
        let mut forged = plan(1, 0);
        forged.dispatches[0].source_volume_gpu = HELIOC_SIM_PARAMS_GPU;
        assert_eq!(helioc_materialization_variant(&forged), None);
    }
}
