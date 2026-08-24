fn direct_rcs_push(batch: &mut [u32], cursor: &mut usize, value: u32) -> bool {
    if *cursor >= batch.len() {
        return false;
    }
    batch[*cursor] = value;
    *cursor += 1;
    true
}

fn direct_rcs_push_pipe_control_full(
    batch: &mut [u32],
    cursor: &mut usize,
    header_flags: u32,
    dw1_flags: u32,
) -> bool {
    direct_rcs_push(batch, cursor, PIPE_CONTROL_CMD | header_flags)
        && direct_rcs_push(batch, cursor, dw1_flags)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_pipe_control(batch: &mut [u32], cursor: &mut usize, flags: u32) -> bool {
    // Every caller is a GPGPU cache flush/invalidate boundary.  Drain HDC/LSC
    // in DW0 as required before the DW1 cache operation can be considered a
    // producer/consumer fence across GuC contexts.
    direct_rcs_push_pipe_control_full(batch, cursor, PIPE_CONTROL_HDC_PIPELINE_FLUSH, flags)
}

fn direct_rcs_push_pipe_control_post_sync_marker_at(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    slot: usize,
    value: u32,
) -> bool {
    // PIPE_CONTROL post-sync writes a QWord. Keep the destination naturally
    // aligned and reserve the following result slot for its high DWORD.
    if slot & 1 != 0 {
        return false;
    }
    let dst = result_gpu + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, PIPE_CONTROL_CMD)
        && direct_rcs_push(
            batch,
            cursor,
            PIPE_CONTROL_FLUSH_ENABLE
                | PIPE_CONTROL_CS_STALL
                | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,
        )
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, value)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_pipe_control_timestamp_at(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    slot: usize,
) -> bool {
    // PIPE_CONTROL writes a 64-bit command-stream timestamp. CS_STALL makes
    // the two samples ordered around the walker instead of merely describing
    // when their memory transactions became visible.
    if slot & 1 != 0 {
        return false;
    }
    let dst = result_gpu + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, PIPE_CONTROL_CMD)
        && direct_rcs_push(
            batch,
            cursor,
            PIPE_CONTROL_FLUSH_ENABLE
                | PIPE_CONTROL_CS_STALL
                | PIPE_CONTROL_POST_SYNC_WRITE_TIMESTAMP,
        )
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_store_marker(
    batch: &mut [u32],
    cursor: &mut usize,
    slot: usize,
    value: u32,
) -> bool {
    direct_rcs_push_store_marker_at(batch, cursor, DIRECT_RCS_GPU_VA_RESULT_BASE, slot, value)
}

fn direct_rcs_push_store_marker_for_state(
    state: DirectRcsState,
    batch: &mut [u32],
    cursor: &mut usize,
    slot: usize,
    value: u32,
) -> bool {
    direct_rcs_push_store_marker_at(batch, cursor, state.gpu_va.result, slot, value)
}

fn direct_rcs_push_store_marker_at(
    batch: &mut [u32],
    cursor: &mut usize,
    result_gpu: u64,
    slot: usize,
    value: u32,
) -> bool {
    let dst = result_gpu + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, MI_STORE_DATA_IMM_GGTT_DW1)
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, value)
}

fn direct_rcs_push_gpgpu_walker_2d(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    indirect_bytes: usize,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0 || group_y == 0 || right_mask == 0 {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, indirect_bytes as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_y)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

/// A deliberately bounded Xe-LP 3D walker shape.
///
/// The compiler contract selects the SIMD width and therefore both the number
/// of hardware threads and the local-ID payload footprint. This is not a
/// general OpenCL launch description: widening either dimension needs a new
/// hardware/ABI proof before it can enter the direct-RCS path.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct DirectRcsXeLp3dWalkerShape {
    group_dimensions: [u32; 3],
    local_dimensions: [u32; 3],
    simd_select: u32,
    hardware_threads: u32,
    right_execution_mask: u32,
    bottom_execution_mask: u32,
    indirect_bytes: usize,
}

impl DirectRcsXeLp3dWalkerShape {
    fn from_authenticated_contract(
        contract: &GpgpuKernelAbiContract,
        group_dimensions: [u32; 3],
        local_dimensions: [u32; 3],
        indirect_bytes: usize,
        right_execution_mask: u32,
        bottom_execution_mask: u32,
    ) -> Option<Self> {
        // The artifact uploader authenticates the contract against the
        // compiler output. Revalidate here so this low-level encoder remains
        // fail-closed when it is used independently of an upload path.
        if contract.validate().is_err()
            || group_dimensions != XELP_3D_WALKER_GROUP_DIMENSIONS
            || local_dimensions != XELP_3D_WALKER_LOCAL_DIMENSIONS
        {
            return None;
        }

        let local_invocations = local_dimensions[0]
            .checked_mul(local_dimensions[1])?
            .checked_mul(local_dimensions[2])?;
        if local_invocations != XELP_3D_WALKER_LOCAL_INVOCATIONS {
            return None;
        }

        let (simd_select, simd_width, expected_per_thread_bytes, expected_right_mask) =
            match contract.simd_width {
                16 => (
                    GPGPU_WALKER_SIMD16_SELECT,
                    16,
                    96usize,
                    GPGPU_WALKER_SIMD16_MASK,
                ),
                32 => (
                    GPGPU_WALKER_SIMD32_SELECT,
                    32,
                    192usize,
                    GPGPU_WALKER_SIMD32_MASK,
                ),
                _ => return None,
            };
        if contract.per_thread_data_bytes as usize != expected_per_thread_bytes
            || local_invocations % simd_width != 0
            || right_execution_mask != expected_right_mask
            || bottom_execution_mask != GPGPU_WALKER_BOTTOM_MASK
        {
            return None;
        }
        let hardware_threads = local_invocations / simd_width;
        // This literal 64-work-item shape has a proven thread count for each
        // admitted SIMD width. Keep the expectation explicit rather than
        // treating the packet's thread-count field as a generic scheduler.
        if hardware_threads
            != match simd_width {
                16 => 4,
                32 => 2,
                _ => return None,
            }
        {
            return None;
        }

        let expected_indirect_bytes = (contract.cross_thread_data_bytes as usize)
            .checked_add(expected_per_thread_bytes.checked_mul(hardware_threads as usize)?)?;
        if indirect_bytes != expected_indirect_bytes {
            return None;
        }

        Some(Self {
            group_dimensions,
            local_dimensions,
            simd_select,
            hardware_threads,
            right_execution_mask,
            bottom_execution_mask,
            indirect_bytes,
        })
    }
}

/// Emit the one admitted Xe-LP 3D GPGPU_WALKER packet.
///
/// `shape` can only be constructed from a validated compiler ABI contract.
/// Preserve the legacy 2D encoder for all existing users, including Font,
/// whose full-SIMD16 lane behavior is intentionally unchanged.
fn direct_rcs_push_xelp_3d_gpgpu_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    shape: DirectRcsXeLp3dWalkerShape,
) -> bool {
    if !payload_offset.is_multiple_of(GPGPU_WALKER_INDIRECT_ALIGNMENT_BYTES)
        || payload_offset
            .checked_add(shape.indirect_bytes)
            .is_none_or(|end| end > DIRECT_RCS_BATCH_BYTES)
        || *cursor > batch.len()
        || batch.len() - *cursor < GPGPU_WALKER_DWORDS
    {
        return false;
    }
    debug_assert_eq!(shape.local_dimensions, XELP_3D_WALKER_LOCAL_DIMENSIONS);
    let simd_width = if shape.simd_select == GPGPU_WALKER_SIMD16_SELECT {
        16
    } else {
        32
    };
    debug_assert_eq!(
        shape.hardware_threads * simd_width,
        XELP_3D_WALKER_LOCAL_INVOCATIONS
    );

    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, shape.indirect_bytes as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (shape.simd_select << 30) | (shape.hardware_threads - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, shape.group_dimensions[0])
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, shape.group_dimensions[1])
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, shape.group_dimensions[2])
        && direct_rcs_push(batch, cursor, shape.right_execution_mask)
        && direct_rcs_push(batch, cursor, shape.bottom_execution_mask)
}

fn direct_rcs_push_rect_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    group_x: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0 || group_x as usize > RECT_WORKLIST_DESCS_PER_WALKER {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, RECT_WORKLIST_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 1)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_sprite_quad_worklist_walker(
    batch: &mut [u32],
    cursor: &mut usize,
    payload_offset: usize,
    group_x: u32,
    group_y: u32,
    right_mask: u32,
) -> bool {
    if group_x == 0
        || group_y == 0
        || group_x as usize > SPRITE_QUAD_WORKLIST_MAX_GROUPS_PER_WALKER
        || !payload_offset.is_multiple_of(GPGPU_WALKER_INDIRECT_ALIGNMENT_BYTES)
        || payload_offset
            .checked_add(SPRITE_QUAD_WORKLIST_INDIRECT_BYTES)
            .is_none_or(|end| end > DIRECT_RCS_BATCH_BYTES)
    {
        return false;
    }
    direct_rcs_push(batch, cursor, GPGPU_WALKER_CMD)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, SPRITE_QUAD_WORKLIST_INDIRECT_BYTES as u32)
        && direct_rcs_push(batch, cursor, payload_offset as u32)
        && direct_rcs_push(
            batch,
            cursor,
            (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
        )
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_x)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, group_y)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_GROUP_Z_DIM)
        && direct_rcs_push(batch, cursor, right_mask)
        && direct_rcs_push(batch, cursor, GPGPU_WALKER_BOTTOM_MASK)
}

fn direct_rcs_push_state_base_address(
    batch: &mut [u32],
    cursor: &mut usize,
    indirect_object_base: u64,
    dynamic_state_base: u64,
    instruction_base: u64,
) -> bool {
    direct_rcs_push_state_base_address_with_mocs_index(
        batch,
        cursor,
        indirect_object_base,
        dynamic_state_base,
        instruction_base,
        RENDER_MOCS_INDEX,
    )
}

fn direct_rcs_push_state_base_address_with_mocs_index(
    batch: &mut [u32],
    cursor: &mut usize,
    indirect_object_base: u64,
    dynamic_state_base: u64,
    instruction_base: u64,
    mocs_index: u32,
) -> bool {
    if mocs_index > RENDER_MOCS_TABLE_INDEX_MASK {
        return false;
    }
    let mocs = direct_rcs_encode_mocs_index(mocs_index);
    direct_rcs_push(batch, cursor, STATE_BASE_ADDRESS_CMD)
        && direct_rcs_push_sba_address(batch, cursor, true, mocs, indirect_object_base)
        && direct_rcs_push(batch, cursor, mocs << 16)
        && direct_rcs_push_sba_address(batch, cursor, true, mocs, dynamic_state_base)
        && direct_rcs_push_sba_address(batch, cursor, true, mocs, dynamic_state_base)
        && direct_rcs_push_sba_address(batch, cursor, true, mocs, indirect_object_base)
        && direct_rcs_push_sba_address(batch, cursor, true, mocs, instruction_base)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_address(batch, cursor, true, mocs, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push_sba_address(batch, cursor, true, mocs, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_sba_address(
    batch: &mut [u32],
    cursor: &mut usize,
    enable: bool,
    mocs_command_value: u32,
    address: u64,
) -> bool {
    debug_assert!(mocs_command_value <= RENDER_MOCS_COMMAND_VALUE_MASK);
    debug_assert_eq!(mocs_command_value & 1, 0);
    let low = ((address as u32) & 0xFFFF_F000) | (mocs_command_value << 4) | u32::from(enable);
    direct_rcs_push(batch, cursor, low) && direct_rcs_push(batch, cursor, (address >> 32) as u32)
}

fn direct_rcs_push_sba_size(
    batch: &mut [u32],
    cursor: &mut usize,
    enable: bool,
    size_bytes: usize,
) -> bool {
    let Some(size_bytes) = align_up(size_bytes, 4096) else {
        return false;
    };
    let Ok(size_bytes) = u32::try_from(size_bytes) else {
        return false;
    };
    direct_rcs_push(batch, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DirectRcsLane {
    SystemService,
    Font,
    Execution,
    Lfm25,
    HelioCloud,
}

impl DirectRcsLane {
    const fn name(self) -> &'static str {
        match self {
            Self::SystemService => "system-service",
            Self::Font => "font",
            Self::Execution => "execution",
            Self::Lfm25 => "lfm25",
            Self::HelioCloud => "helioc",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DirectRcsSubmitFailureAction {
    RollBack,
    PreserveAndQuarantine,
}

const fn direct_rcs_submit_failure_action(
    error: crate::gpu::vgpu::VgpuError,
) -> DirectRcsSubmitFailureAction {
    match error {
        crate::gpu::vgpu::VgpuError::Busy => DirectRcsSubmitFailureAction::RollBack,
        _ => DirectRcsSubmitFailureAction::PreserveAndQuarantine,
    }
}

enum DirectRcsSubmitAttempt {
    Submitted(crate::gpu::executor::KernelSubmission),
    Rejected,
    Ambiguous {
        error: crate::gpu::vgpu::VgpuError,
        old_tail_bytes: usize,
        published_tail_bytes: usize,
        submission_sequence: u64,
    },
}

/// Observable ownership boundary for callers that must decide whether GPU
/// referenced storage can be recycled. `Ambiguous` means the LRC tail was
/// published but GuC did not provide a conclusive acceptance result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectRcsSubmissionState {
    Rejected,
    Submitted,
    Ambiguous,
}

impl DirectRcsSubmissionState {
    const fn may_have_submitted(self) -> bool {
        !matches!(self, Self::Rejected)
    }

    const fn can_poll(self) -> bool {
        matches!(self, Self::Submitted)
    }
}

fn direct_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    matches!(direct_rcs_submit_batch_state(dev, state), DirectRcsSubmissionState::Submitted)
}

fn direct_rcs_submit_batch_state(
    dev: super::Dev,
    state: DirectRcsState,
) -> DirectRcsSubmissionState {
    direct_rcs_submit_batch_on_lane_state(dev, state, DirectRcsLane::SystemService)
}

fn font_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    matches!(font_rcs_submit_batch_state(dev, state), DirectRcsSubmissionState::Submitted)
}

fn font_rcs_submit_batch_state(dev: super::Dev, state: DirectRcsState) -> DirectRcsSubmissionState {
    direct_rcs_submit_batch_on_lane_state(dev, state, DirectRcsLane::Font)
}

fn execution_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    matches!(
        direct_rcs_submit_batch_on_lane_state(dev, state, DirectRcsLane::Execution),
        DirectRcsSubmissionState::Submitted
    )
}

fn lfm25_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    if !super::gen12_lumen_mocs_ready() {
        quarantine_lfm25_rcs_context("lumen-mocs-not-ready");
        return false;
    }
    matches!(
        direct_rcs_submit_batch_on_lane_state(dev, state, DirectRcsLane::Lfm25),
        DirectRcsSubmissionState::Submitted
    )
}

#[expect(dead_code, reason = "reserved for the sealed HelioC frame encoder")]
fn helioc_rcs_submit_batch_state(
    dev: super::Dev,
    state: DirectRcsState,
) -> DirectRcsSubmissionState {
    direct_rcs_submit_batch_on_lane_state(dev, state, DirectRcsLane::HelioCloud)
}

fn direct_rcs_submit_batch_on_lane_state(
    dev: super::Dev,
    state: DirectRcsState,
    lane: DirectRcsLane,
) -> DirectRcsSubmissionState {
    let (quarantined, runtime, client) = match lane {
        DirectRcsLane::SystemService => (
            &DIRECT_RCS_CONTEXT_QUARANTINED,
            &DIRECT_RCS_SUBMIT_RUNTIME,
            crate::gpu::vgpu::KernelClient::GpgpuSystem,
        ),
        DirectRcsLane::Font => (
            &FONT_RCS_CONTEXT_QUARANTINED,
            &FONT_RCS_SUBMIT_RUNTIME,
            crate::gpu::vgpu::KernelClient::GpgpuFont,
        ),
        DirectRcsLane::Execution => (
            &EXECUTION_RCS_CONTEXT_QUARANTINED,
            &EXECUTION_RCS_SUBMIT_RUNTIME,
            crate::gpu::vgpu::KernelClient::GpgpuExecution,
        ),
        DirectRcsLane::Lfm25 => (
            &LFM25_RCS_CONTEXT_QUARANTINED,
            &LFM25_RCS_SUBMIT_RUNTIME,
            crate::gpu::vgpu::KernelClient::Lfm25,
        ),
        DirectRcsLane::HelioCloud => (
            &HELIOC_RCS_CONTEXT_QUARANTINED,
            &HELIOC_RCS_SUBMIT_RUNTIME,
            crate::gpu::vgpu::KernelClient::HelioCloud,
        ),
    };
    if quarantined.load(Ordering::Acquire) {
        return DirectRcsSubmissionState::Rejected;
    }
    let attempt = {
        let mut runtime = runtime.lock();
        direct_rcs_submit_batch_with_runtime_inner(dev, state, &mut runtime, client, false)
    };
    match attempt {
        DirectRcsSubmitAttempt::Submitted(_) => DirectRcsSubmissionState::Submitted,
        DirectRcsSubmitAttempt::Rejected => DirectRcsSubmissionState::Rejected,
        DirectRcsSubmitAttempt::Ambiguous {
            error,
            old_tail_bytes,
            published_tail_bytes,
            submission_sequence,
        } => {
            crate::log_error!(target: "gpgpu";
                "intel/gpgpu: direct-rcs submit result ambiguous lane={} client={} error={} old_tail={} published_tail={} submission={} runtime_state=advanced backing=retained action=quarantine-exact-lane direct_elsp=0\n",
                lane.name(),
                client.name(),
                error.name(),
                old_tail_bytes,
                published_tail_bytes,
                submission_sequence,
            );
            quarantine_direct_rcs_lane(lane, "submit-result-ambiguous-after-tail-publication");
            DirectRcsSubmissionState::Ambiguous
        }
    }
}

fn quarantine_direct_rcs_context(reason: &'static str) {
    quarantine_direct_rcs_lane(DirectRcsLane::SystemService, reason);
}

fn direct_rcs_state_reuse_permitted(quarantined: &AtomicBool) -> bool {
    !quarantined.load(Ordering::Acquire)
}

pub(crate) fn direct_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&DIRECT_RCS_CONTEXT_QUARANTINED)
}

pub(crate) fn font_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&FONT_RCS_CONTEXT_QUARANTINED)
}

fn execution_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&EXECUTION_RCS_CONTEXT_QUARANTINED)
}

fn lfm25_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&LFM25_RCS_CONTEXT_QUARANTINED)
}

fn helioc_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&HELIOC_RCS_CONTEXT_QUARANTINED)
}

fn ui4_compositor_rcs_context_is_quarantined() -> bool {
    !direct_rcs_state_reuse_permitted(&UI4_COMPOSITOR_RCS_CONTEXT_QUARANTINED)
}

fn quarantine_execution_rcs_context(reason: &'static str) {
    quarantine_direct_rcs_lane(DirectRcsLane::Execution, reason);
}

fn quarantine_font_rcs_context(reason: &'static str) {
    quarantine_direct_rcs_lane(DirectRcsLane::Font, reason);
}

fn quarantine_lfm25_rcs_context(reason: &'static str) {
    quarantine_direct_rcs_lane(DirectRcsLane::Lfm25, reason);
}

fn quarantine_helioc_rcs_context(reason: &'static str) {
    quarantine_direct_rcs_lane(DirectRcsLane::HelioCloud, reason);
}

fn quarantine_ui4_compositor_rcs_context(reason: &'static str) {
    if UI4_COMPOSITOR_RCS_CONTEXT_QUARANTINED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let client = crate::gpu::vgpu::KernelClient::Ui4Compositor;
        let isolation = crate::gpu::vgpu::isolate_kernel_client(client);
        crate::log_error!(target: "ui4";
            "ui4/guc-compositor: context quarantined client={} reason={} device_found={} contexts_disabled={} contexts_retained={} action=isolate-compositor-and-reject-future-state-access-until-reboot late-batch-reuse=forbidden\n",
            client.name(),
            reason,
            isolation.device_found as u8,
            isolation.contexts_disabled,
            isolation.contexts_retained,
        );
    }
}

fn quarantine_direct_rcs_lane(lane: DirectRcsLane, reason: &'static str) {
    let (quarantined, client) = match lane {
        DirectRcsLane::SystemService => {
            (&DIRECT_RCS_CONTEXT_QUARANTINED, crate::gpu::vgpu::KernelClient::GpgpuSystem)
        }
        DirectRcsLane::Font => {
            (&FONT_RCS_CONTEXT_QUARANTINED, crate::gpu::vgpu::KernelClient::GpgpuFont)
        }
        DirectRcsLane::Execution => {
            (&EXECUTION_RCS_CONTEXT_QUARANTINED, crate::gpu::vgpu::KernelClient::GpgpuExecution)
        }
        DirectRcsLane::Lfm25 => {
            (&LFM25_RCS_CONTEXT_QUARANTINED, crate::gpu::vgpu::KernelClient::Lfm25)
        }
        DirectRcsLane::HelioCloud => {
            (&HELIOC_RCS_CONTEXT_QUARANTINED, crate::gpu::vgpu::KernelClient::HelioCloud)
        }
    };
    if quarantined
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let isolation = crate::gpu::vgpu::isolate_kernel_client(client);
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs context quarantined lane={} client={} reason={} device_found={} contexts_disabled={} contexts_retained={} action=isolate-consumer-and-reject-future-direct-submits-until-reboot late-batch-reuse=forbidden\n",
            lane.name(),
            client.name(),
            reason,
            isolation.device_found as u8,
            isolation.contexts_disabled,
            isolation.contexts_retained,
        );
    }
}

fn direct_rcs_submit_batch_with_runtime(
    dev: super::Dev,
    state: DirectRcsState,
    runtime: &mut DirectRcsSubmitRuntime,
    client: crate::gpu::vgpu::KernelClient,
    allow_queued: bool,
) -> Option<crate::gpu::executor::KernelSubmission> {
    debug_assert_eq!(client, crate::gpu::vgpu::KernelClient::Ui4Compositor);
    if ui4_compositor_rcs_context_is_quarantined() {
        return None;
    }
    match direct_rcs_submit_batch_with_runtime_inner(dev, state, runtime, client, allow_queued) {
        DirectRcsSubmitAttempt::Submitted(submission) => Some(submission),
        DirectRcsSubmitAttempt::Rejected => None,
        DirectRcsSubmitAttempt::Ambiguous {
            error,
            old_tail_bytes,
            published_tail_bytes,
            submission_sequence,
        } => {
            crate::log_error!(target: "ui4";
                "ui4/guc-compositor: submit result ambiguous client={} error={} old_tail={} published_tail={} submission={} runtime_state=advanced backing=retained action=quarantine-compositor direct_elsp=0\n",
                client.name(),
                error.name(),
                old_tail_bytes,
                published_tail_bytes,
                submission_sequence,
            );
            quarantine_ui4_compositor_rcs_context("submit-result-ambiguous-after-tail-publication");
            None
        }
    }
}

fn direct_rcs_submit_batch_with_runtime_inner(
    dev: super::Dev,
    state: DirectRcsState,
    runtime: &mut DirectRcsSubmitRuntime,
    client: crate::gpu::vgpu::KernelClient,
    allow_queued: bool,
) -> DirectRcsSubmitAttempt {
    if !direct_rcs_control_ggtt_ready(state) {
        return DirectRcsSubmitAttempt::Rejected;
    }
    if !allow_queued && runtime.pending.is_some() {
        return DirectRcsSubmitAttempt::Rejected;
    }
    // The GuC owns one persistent logical context for the direct-RCS client.
    // Its ring must therefore be persistent as well: publishing the same tail
    // for every request does not describe new work once the first request has
    // advanced the saved ring head. Append one BBS entry and advance the tail
    // instead of rebuilding the registered context at offset zero.
    let old_tail_bytes = runtime.ring_tail_bytes;
    if runtime.context_initialized {
        // HW updates HEAD and software updates TAIL in the same first HWLRCA
        // cache line. The producer marker is before MI_BATCH_BUFFER_END, so it
        // does not transfer ownership of that line back to the CPU. Only a
        // saved HEAD equal to the last published TAIL proves GuC has consumed
        // the ring entry and saved the context. Defer instead of racing a
        // GPU-written head with a CPU cache-line writeback.
        let saved_head = direct_rcs_read_lrc_ring_head(state) & (DIRECT_RCS_RING_BYTES as u32 - 1);
        if saved_head != old_tail_bytes as u32 {
            runtime.retire_deferrals = runtime.retire_deferrals.saturating_add(1);
            if runtime.retire_deferrals == 1 || runtime.retire_deferrals.is_power_of_two() {
                crate::log_info!(target: "gpgpu";
                    "intel/gpgpu: persistent-ring tail publish deferred client={} saved_head={} published_tail={} deferrals={} ownership=wait-guc-context-save action=retry\n",
                    client.name(),
                    saved_head,
                    old_tail_bytes,
                    runtime.retire_deferrals,
                );
            }
            return DirectRcsSubmitAttempt::Rejected;
        }
        if runtime.retire_deferrals != 0 {
            crate::log_info!(target: "gpgpu";
                "intel/gpgpu: persistent-ring context save observed client={} saved_head={} published_tail={} deferrals={} action=resume-tail-publish\n",
                client.name(),
                saved_head,
                old_tail_bytes,
                runtime.retire_deferrals,
            );
            runtime.retire_deferrals = 0;
        }
    }
    let ring_tail_bytes =
        direct_rcs_append_ring_batch_start(state, old_tail_bytes, state.gpu_va.batch);
    let ring_entries = DIRECT_RCS_RING_BYTES / (DIRECT_RCS_BATCH_START_DWORDS * 4);
    let submission_sequence = runtime.submissions.saturating_add(1);
    let ring_position = (submission_sequence as usize) % ring_entries;
    let trace_ring_boundary = runtime.submissions != 0
        && (ring_position <= 4 || ring_position >= ring_entries.saturating_sub(4));
    if trace_ring_boundary {
        crate::log_info!(target: "gpgpu";
            "intel/gpgpu: persistent-ring boundary client={} submission={} entries={} old_tail={} new_tail={} saved_head={} engine_head_snapshot={} encoder_ring_clear=0 ownership=guc-context\n",
            client.name(),
            submission_sequence,
            ring_entries,
            old_tail_bytes,
            ring_tail_bytes,
            direct_rcs_read_lrc_ring_head(state),
            super::mmio_read(dev, RCS_RING_HEAD) & (DIRECT_RCS_RING_BYTES as u32 - 1),
        );
    }
    let Some(ring_ctl) = direct_rcs_ring_ctl_value(DIRECT_RCS_RING_BYTES) else {
        return DirectRcsSubmitAttempt::Rejected;
    };
    if !runtime.context_initialized {
        if !direct_rcs_init_lrc_context_image(
            state,
            state.gpu_va.ring as u32,
            ring_tail_bytes as u32,
            ring_ctl,
        ) {
            return DirectRcsSubmitAttempt::Rejected;
        }
        runtime.context_initialized = true;
    } else {
        direct_rcs_write_lrc_ring_tail(state, ring_tail_bytes as u32);
    }
    let (context_desc_lo, context_desc_hi) = guc_rcs_context_descriptor(state.gpu_va.context);
    core::sync::atomic::fence(Ordering::SeqCst);
    let descriptor = crate::gpu::physical::PhysicalContextDescriptor {
        engine: crate::gpu::physical::PhysicalEngineId::RCS0,
        hwlrca_lo: context_desc_lo,
        hwlrca_hi: context_desc_hi,
        gpuvm_root_phys: state.ppgtt_phys,
    };
    match crate::gpu::executor::submit_kernel_context(client, descriptor) {
        Ok(submission) => {
            runtime.ring_tail_bytes = ring_tail_bytes;
            runtime.submissions = submission_sequence;
            if !allow_queued {
                runtime.pending = Some(submission);
            }
            DirectRcsSubmitAttempt::Submitted(submission)
        }
        Err(error) => {
            match direct_rcs_submit_failure_action(error) {
                DirectRcsSubmitFailureAction::RollBack => {
                    // Busy is the one proven pre-publication rejection for an
                    // isolated direct-RCS client.
                    direct_rcs_write_lrc_ring_tail(state, old_tail_bytes as u32);
                    crate::log!(
                        "gpgpu/vgpu: submit failed error={:?} tail_action=rollback submission_owner=gpu-executor/vgpu/guc direct_elsp=0\n",
                        error
                    );
                    DirectRcsSubmitAttempt::Rejected
                }
                DirectRcsSubmitFailureAction::PreserveAndQuarantine => {
                    // Once submit crosses the executor/vGPU/GuC boundary, an
                    // error other than Busy cannot prove that GuC did not see
                    // the request. Keep both software and LRC publication at
                    // the advanced tail and pin the backing via lane
                    // quarantine; replaying the same ring entry is forbidden.
                    runtime.ring_tail_bytes = ring_tail_bytes;
                    runtime.submissions = submission_sequence;
                    DirectRcsSubmitAttempt::Ambiguous {
                        error,
                        old_tail_bytes,
                        published_tail_bytes: ring_tail_bytes,
                        submission_sequence,
                    }
                }
            }
        }
    }
}

fn direct_rcs_submit_runtime(lane: DirectRcsLane) -> &'static Mutex<DirectRcsSubmitRuntime> {
    match lane {
        DirectRcsLane::SystemService => &DIRECT_RCS_SUBMIT_RUNTIME,
        DirectRcsLane::Font => &FONT_RCS_SUBMIT_RUNTIME,
        DirectRcsLane::Execution => &EXECUTION_RCS_SUBMIT_RUNTIME,
        DirectRcsLane::Lfm25 => &LFM25_RCS_SUBMIT_RUNTIME,
        DirectRcsLane::HelioCloud => &HELIOC_RCS_SUBMIT_RUNTIME,
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct DirectRcsRetirementProof {
    marker_observed: bool,
    saved_head_bytes: u32,
    published_tail_bytes: usize,
}

impl DirectRcsRetirementProof {
    const fn complete(self) -> bool {
        direct_rcs_retirement_is_proven(
            self.marker_observed,
            self.saved_head_bytes,
            self.published_tail_bytes,
        )
    }
}

const fn direct_rcs_retirement_is_proven(
    marker_observed: bool,
    saved_head_bytes: u32,
    published_tail_bytes: usize,
) -> bool {
    marker_observed && saved_head_bytes as usize == published_tail_bytes
}

fn direct_rcs_retirement_proof_on_lane(
    state: DirectRcsState,
    lane: DirectRcsLane,
    marker_observed: bool,
) -> DirectRcsRetirementProof {
    let published_tail_bytes = direct_rcs_submit_runtime(lane).lock().ring_tail_bytes;
    let saved_head_bytes =
        direct_rcs_read_lrc_ring_head(state) & (DIRECT_RCS_RING_BYTES as u32 - 1);
    DirectRcsRetirementProof {
        marker_observed,
        saved_head_bytes,
        published_tail_bytes,
    }
}

fn execution_rcs_retirement_proof(
    state: DirectRcsState,
    marker_observed: bool,
) -> DirectRcsRetirementProof {
    direct_rcs_retirement_proof_on_lane(state, DirectRcsLane::Execution, marker_observed)
}

fn complete_direct_rcs_submission() {
    complete_direct_rcs_submission_on_lane(DirectRcsLane::SystemService);
}

fn complete_execution_rcs_submission() {
    complete_direct_rcs_submission_on_lane(DirectRcsLane::Execution);
}

fn complete_direct_rcs_submission_on_lane(lane: DirectRcsLane) {
    let submission = direct_rcs_submit_runtime(lane).lock().pending.take();
    if let Some(submission) = submission {
        let _ = crate::gpu::executor::complete_kernel_submission(submission, true);
    }
}

fn direct_rcs_append_ring_batch_start(
    state: DirectRcsState,
    ring_tail_bytes: usize,
    batch_gpu_addr: u64,
) -> usize {
    debug_assert_eq!(ring_tail_bytes % (DIRECT_RCS_BATCH_START_DWORDS * 4), 0);
    debug_assert!(ring_tail_bytes < DIRECT_RCS_RING_BYTES);
    let start = ring_tail_bytes / core::mem::size_of::<u32>();
    unsafe {
        let dwords = state.ring_virt as *mut u32;
        core::ptr::write_volatile(dwords.add(start), MI_BATCH_BUFFER_START_GEN8);
        core::ptr::write_volatile(dwords.add(start + 1), batch_gpu_addr as u32);
        core::ptr::write_volatile(dwords.add(start + 2), (batch_gpu_addr >> 32) as u32);
        core::ptr::write_volatile(dwords.add(start + 3), MI_NOOP);
    }
    let tail_bytes = (ring_tail_bytes
        + DIRECT_RCS_BATCH_START_DWORDS * core::mem::size_of::<u32>())
        % DIRECT_RCS_RING_BYTES;
    unsafe {
        super::dma_flush(
            state.ring_virt.add(ring_tail_bytes),
            DIRECT_RCS_BATCH_START_DWORDS * core::mem::size_of::<u32>(),
        );
    }
    tail_bytes
}

fn direct_rcs_poll_result_slot(state: DirectRcsState, slot: usize, expected: u32) -> u32 {
    let mut observed = 0;
    let mut completed = false;
    for _ in 0..DIRECT_RCS_SMOKE_POLL_ITERS {
        observed = direct_rcs_read_result_slot(state, slot);
        let proof = direct_rcs_retirement_proof_on_lane(
            state,
            DirectRcsLane::SystemService,
            observed == expected,
        );
        if proof.complete() {
            completed = true;
            break;
        }
        core::hint::spin_loop();
    }
    let proof = direct_rcs_retirement_proof_on_lane(
        state,
        DirectRcsLane::SystemService,
        observed == expected,
    );
    completed &= proof.complete();
    if !completed {
        // The physical request is not cancelled by failing its software
        // timeline. Poison the shared context before the submit lock can be
        // released so no caller rewrites memory a late batch may still fetch.
        let reason = if observed == expected {
            "completion-marker-observed-context-save-unproven-reboot-required"
        } else {
            "completion-marker-unobserved-reboot-required"
        };
        crate::log_error!(target: "gpgpu";
            "intel/gpgpu: direct-rcs retirement unproven lane={} marker_observed={} saved_head={} published_tail={} action=quarantine-retain-pending-and-backing\n",
            DirectRcsLane::SystemService.name(),
            (observed == expected) as u8,
            proof.saved_head_bytes,
            proof.published_tail_bytes,
        );
        quarantine_direct_rcs_context(reason);
    } else {
        complete_direct_rcs_submission();
    }
    if completed { observed } else { 0 }
}

fn direct_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::SystemService,
    )
}

fn font_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::Font,
    )
}

fn execution_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::Execution,
    )
}

fn lfm25_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::Lfm25,
    )
}

#[expect(dead_code, reason = "reserved for the sealed HelioC frame encoder")]
fn helioc_rcs_poll_result_slot_timeout_ms(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::HelioCloud,
    )
}

fn lfm25_rcs_poll_result_slot_timeout_ms_with_timestamp(
    dev: super::Dev,
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
) -> (u32, u64) {
    direct_rcs_poll_result_slot_timeout_ms_on_lane_with_timestamp(
        state,
        slot,
        expected,
        timeout_ms,
        DirectRcsLane::Lfm25,
        Some(dev),
    )
}

fn direct_rcs_poll_result_slot_timeout_ms_on_lane(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
    lane: DirectRcsLane,
) -> u32 {
    direct_rcs_poll_result_slot_timeout_ms_on_lane_with_timestamp(
        state, slot, expected, timeout_ms, lane, None,
    )
    .0
}

fn direct_rcs_poll_result_slot_timeout_ms_on_lane_with_timestamp(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    timeout_ms: u64,
    lane: DirectRcsLane,
    observe_timestamp_dev: Option<super::Dev>,
) -> (u32, u64) {
    let started = direct_rcs_now_tick();
    let deadline = started.saturating_add(direct_rcs_ticks_from_ms(timeout_ms));
    let probe_logged = match lane {
        DirectRcsLane::SystemService => &DIRECT_RCS_TIMEOUT_POLL_PROBE_LOGGED,
        DirectRcsLane::Font => &FONT_RCS_TIMEOUT_POLL_PROBE_LOGGED,
        DirectRcsLane::Execution => &EXECUTION_RCS_TIMEOUT_POLL_PROBE_LOGGED,
        DirectRcsLane::Lfm25 => &LFM25_RCS_TIMEOUT_POLL_PROBE_LOGGED,
        DirectRcsLane::HelioCloud => &HELIOC_RCS_TIMEOUT_POLL_PROBE_LOGGED,
    };
    let log_probe = probe_logged
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if log_probe {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: marker-poll begin lane={} slot={} expected=0x{:08X} timeout_ms={} completion_limit=deadline cache_flush_bytes=4 pause_iters={} worker_slot={}\n",
            lane.name(),
            slot,
            expected,
            timeout_ms,
            DIRECT_RCS_TIMEOUT_POLL_PAUSE_ITERS,
            crate::percpu::current_slot(),
        );
    }
    let mut iterations = 0usize;
    let mut marker_observe_timestamp = 0;
    let (observed, proof) = loop {
        iterations = iterations.saturating_add(1);
        let observed = direct_rcs_read_result_slot(state, slot);
        let marker_observed = observed == expected;
        if marker_observed && marker_observe_timestamp == 0 {
            marker_observe_timestamp = observe_timestamp_dev
                .map(direct_rcs_read_render_timestamp)
                .unwrap_or(0);
        }
        let proof = direct_rcs_retirement_proof_on_lane(state, lane, marker_observed);
        if proof.complete() {
            break (observed, proof);
        }
        if direct_rcs_now_tick() >= deadline {
            break (observed, proof);
        }
        for _ in 0..DIRECT_RCS_TIMEOUT_POLL_PAUSE_ITERS {
            core::hint::spin_loop();
        }
    };
    if log_probe {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: marker-poll end lane={} slot={} observed=0x{:08X} expected=0x{:08X} marker_matched={} saved_head={} published_tail={} retirement_proven={} iterations={} elapsed_ms={}\n",
            lane.name(),
            slot,
            observed,
            expected,
            (observed == expected) as u8,
            proof.saved_head_bytes,
            proof.published_tail_bytes,
            proof.complete() as u8,
            iterations,
            direct_rcs_elapsed_ms_since(started),
        );
    }
    let completed = proof.complete();
    if !completed {
        let reason = if observed == expected {
            "completion-marker-observed-context-save-timeout-reboot-required"
        } else {
            "completion-marker-timeout-reboot-required"
        };
        crate::log_error!(target: "gpgpu";
            "intel/gpgpu: direct-rcs retirement timeout lane={} marker_observed={} saved_head={} published_tail={} pending=retained timeline=retained backing=retained action=quarantine-exact-lane\n",
            lane.name(),
            (observed == expected) as u8,
            proof.saved_head_bytes,
            proof.published_tail_bytes,
        );
        quarantine_direct_rcs_lane(lane, reason);
    } else {
        complete_direct_rcs_submission_on_lane(lane);
    }
    (if completed { observed } else { 0 }, marker_observe_timestamp)
}

fn direct_rcs_poll_result_slot_elapsed(
    state: DirectRcsState,
    slot: usize,
    expected: u32,
    start_tick: u64,
) -> (u32, u64) {
    let observed = direct_rcs_poll_result_slot(state, slot, expected);
    (observed, direct_rcs_elapsed_ms_since(start_tick))
}

fn direct_rcs_read_result_slot(state: DirectRcsState, slot: usize) -> u32 {
    let offset = slot.saturating_mul(core::mem::size_of::<u32>());
    if offset + core::mem::size_of::<u32>() > DIRECT_RCS_RESULT_BYTES {
        return 0;
    }
    let marker = unsafe { state.result_virt.add(offset) };
    // CLFLUSH rounds to a cache-line boundary. Invalidating this one marker is
    // sufficient; flushing the full 4 KiB result page on every poll multiplied
    // each check into 64 CLFLUSH operations plus an MFENCE and starved sibling
    // tasks on the same executor core.
    super::dma_flush(marker, core::mem::size_of::<u32>());
    unsafe { core::ptr::read_volatile(marker as *const u32) }
}

fn direct_rcs_read_result_qword(state: DirectRcsState, slot: usize) -> u64 {
    let offset = slot.saturating_mul(core::mem::size_of::<u32>());
    if slot & 1 != 0 || offset + core::mem::size_of::<u64>() > DIRECT_RCS_RESULT_BYTES {
        return 0;
    }
    let value = unsafe { state.result_virt.add(offset) };
    super::dma_flush(value, core::mem::size_of::<u64>());
    let low = unsafe { core::ptr::read_volatile(value as *const u32) };
    let high =
        unsafe { core::ptr::read_volatile(value.add(core::mem::size_of::<u32>()) as *const u32) };
    (u64::from(low) | (u64::from(high) << 32)) & DIRECT_RCS_TIMESTAMP_MASK
}

fn direct_rcs_read_render_timestamp(dev: super::Dev) -> u64 {
    const RCS_TIMESTAMP_LOW_MMIO: usize = 0x2358;
    const RCS_TIMESTAMP_HIGH_MMIO: usize = 0x235C;

    // The Gen12 engine timestamp is 36 bits. Read upper-lower-upper so a
    // rollover of the low DWORD cannot splice two different counter epochs.
    let mut upper = super::mmio_read(dev, RCS_TIMESTAMP_HIGH_MMIO);
    for _ in 0..3 {
        let lower = super::mmio_read(dev, RCS_TIMESTAMP_LOW_MMIO);
        let next_upper = super::mmio_read(dev, RCS_TIMESTAMP_HIGH_MMIO);
        if next_upper == upper {
            return (u64::from(upper) << 32 | u64::from(lower)) & DIRECT_RCS_TIMESTAMP_MASK;
        }
        upper = next_upper;
    }
    let lower = super::mmio_read(dev, RCS_TIMESTAMP_LOW_MMIO);
    (u64::from(upper) << 32 | u64::from(lower)) & DIRECT_RCS_TIMESTAMP_MASK
}

fn direct_rcs_timestamp_delta_ticks(start: u64, end: u64) -> Option<u64> {
    if start == 0 || end == 0 {
        return None;
    }
    let delta = end.wrapping_sub(start) & DIRECT_RCS_TIMESTAMP_MASK;
    // Any phase in this probe is bounded to one second in normal operation
    // and to the one-second compositor timeout on failure. Half a 36-bit
    // epoch is roughly thirty minutes at 19.2 MHz, so a larger modular delta
    // proves that these were not an ordered pair from the same frame.
    (delta != 0 && delta < (1u64 << (DIRECT_RCS_TIMESTAMP_BITS - 1))).then_some(delta)
}

fn direct_rcs_timestamp_interval_us(start: u64, end: u64, frequency_hz: u64) -> Option<(u64, u64)> {
    if frequency_hz == 0 {
        return None;
    }
    let ticks = direct_rcs_timestamp_delta_ticks(start, end)?;
    Some((ticks, direct_rcs_timestamp_ticks_to_us(ticks, frequency_hz)))
}

fn direct_rcs_timestamp_frequency_hz(dev: super::Dev) -> u32 {
    const CTC_MODE_MMIO: usize = 0x0A26C;
    const CTC_SOURCE_DIVIDE_LOGIC: u32 = 1;
    const RPM_CONFIG0_MMIO: usize = 0x00D00;
    const TIMESTAMP_OVERRIDE_MMIO: usize = 0x44074;

    static CACHED_HZ: AtomicU32 = AtomicU32::new(0);
    let cached = CACHED_HZ.load(Ordering::Acquire);
    if cached != 0 {
        return cached;
    }

    // Gen11+ command-stream clock selection, matching the hardware-owned
    // CTC_MODE/RPM_CONFIG0 contract. TRUEOS owns this GT and never reprograms
    // the clock source after initialization, so the resolved frequency can be
    // cached for the lifetime of the boot.
    let ctc_mode = super::mmio_read(dev, CTC_MODE_MMIO);
    let frequency = if ctc_mode & CTC_SOURCE_DIVIDE_LOGIC != 0 {
        let timestamp_override = super::mmio_read(dev, TIMESTAMP_OVERRIDE_MMIO);
        let divider = (timestamp_override & 0x3FF).saturating_add(1);
        let denominator = ((timestamp_override >> 12) & 0xF).saturating_add(1);
        divider
            .saturating_mul(1_000_000)
            .saturating_add(1_000_000 / denominator)
    } else {
        let rpm_config = super::mmio_read(dev, RPM_CONFIG0_MMIO);
        let crystal_hz = match (rpm_config >> 3) & 0x7 {
            0 => 24_000_000,
            1 => 19_200_000,
            2 => 38_400_000,
            3 => 25_000_000,
            _ => 0,
        };
        let shift = (rpm_config >> 1) & 0x3;
        crystal_hz >> (3 - shift)
    };
    if frequency != 0 {
        CACHED_HZ.store(frequency, Ordering::Release);
    }
    frequency
}

fn direct_rcs_timestamp_ticks_to_us(ticks: u64, frequency_hz: u64) -> u64 {
    if frequency_hz == 0 {
        return 0;
    }
    ((ticks as u128)
        .saturating_mul(1_000_000)
        .saturating_add(u128::from(frequency_hz / 2))
        / u128::from(frequency_hz))
    .min(u128::from(u64::MAX)) as u64
}

fn direct_rcs_now_tick() -> u64 {
    embassy_time_driver::now()
}

fn direct_rcs_ticks_from_ms(ms: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        return ms.max(1);
    }
    let ticks = ((ms as u128).saturating_mul(hz as u128).saturating_add(999) / 1000) as u64;
    if ms == 0 { 0 } else { ticks.max(1) }
}

fn direct_rcs_elapsed_ms_since(start_tick: u64) -> u64 {
    let elapsed = direct_rcs_now_tick().saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        elapsed.saturating_mul(1000) / hz
    }
}

fn direct_rcs_elapsed_us_since(start_tick: u64) -> u64 {
    let elapsed = direct_rcs_now_tick().saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ((elapsed as u128).saturating_mul(1_000_000) / hz as u128) as u64
    }
}

#[cfg(test)]
mod direct_rcs_fail_closed_tests {
    use super::*;

    const SIMD32_LOCAL_ID_PAYLOAD: &[GpgpuArtifactPerThreadPayloadArg] =
        &[GpgpuArtifactPerThreadPayloadArg {
            kind: GpgpuArtifactPerThreadArgKind::LocalId,
            offset_bytes: 0,
            size_bytes: 192,
        }];
    const SIMD32_TEST_CONTRACT: GpgpuKernelAbiContract = GpgpuKernelAbiContract {
        simd_width: 32,
        per_thread_data_bytes: 192,
        per_thread_payload_args: SIMD32_LOCAL_ID_PAYLOAD,
        ..COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT
    };

    fn xelp_3d_shape(
        contract: &GpgpuKernelAbiContract,
        right_execution_mask: u32,
        bottom_execution_mask: u32,
        indirect_bytes: usize,
    ) -> Option<DirectRcsXeLp3dWalkerShape> {
        DirectRcsXeLp3dWalkerShape::from_authenticated_contract(
            contract,
            XELP_3D_WALKER_GROUP_DIMENSIONS,
            XELP_3D_WALKER_LOCAL_DIMENSIONS,
            indirect_bytes,
            right_execution_mask,
            bottom_execution_mask,
        )
    }

    #[test]
    fn xelp_3d_walker_encodes_compiler_selected_simd16_shape() {
        let indirect_bytes = COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes as usize
            + 4 * COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT.per_thread_data_bytes as usize;
        let shape = xelp_3d_shape(
            &COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT,
            GPGPU_WALKER_SIMD16_MASK,
            GPGPU_WALKER_BOTTOM_MASK,
            indirect_bytes,
        )
        .expect("SIMD16 contract must select the four-thread Xe-LP shape");
        assert_eq!(shape.local_dimensions, [4, 4, 4]);
        assert_eq!(shape.hardware_threads, 4);
        assert_eq!(shape.indirect_bytes, 480);

        let mut batch = [0u32; GPGPU_WALKER_DWORDS];
        let mut cursor = 0usize;
        assert!(direct_rcs_push_xelp_3d_gpgpu_walker(
            &mut batch,
            &mut cursor,
            0x4000,
            shape,
        ));
        assert_eq!(cursor, GPGPU_WALKER_DWORDS);
        assert_eq!(batch[0], GPGPU_WALKER_CMD);
        assert_eq!(batch[2], 480);
        assert_eq!(batch[3], 0x4000);
        assert_eq!(batch[4], (GPGPU_WALKER_SIMD16_SELECT << 30) | 3);
        assert_eq!([batch[7], batch[10], batch[12]], [24, 12, 24]);
        assert_eq!(batch[13], GPGPU_WALKER_SIMD16_MASK);
        assert_eq!(batch[14], GPGPU_WALKER_BOTTOM_MASK);
    }

    #[test]
    fn xelp_3d_walker_encodes_compiler_selected_simd32_shape() {
        assert_eq!(SIMD32_TEST_CONTRACT.validate(), Ok(()));
        let indirect_bytes = SIMD32_TEST_CONTRACT.cross_thread_data_bytes as usize
            + 2 * SIMD32_TEST_CONTRACT.per_thread_data_bytes as usize;
        let shape = xelp_3d_shape(
            &SIMD32_TEST_CONTRACT,
            GPGPU_WALKER_SIMD32_MASK,
            GPGPU_WALKER_BOTTOM_MASK,
            indirect_bytes,
        )
        .expect("SIMD32 contract must select the two-thread Xe-LP shape");
        assert_eq!(shape.hardware_threads, 2);
        assert_eq!(shape.indirect_bytes, 480);

        let mut batch = [0u32; GPGPU_WALKER_DWORDS];
        let mut cursor = 0usize;
        assert!(direct_rcs_push_xelp_3d_gpgpu_walker(
            &mut batch,
            &mut cursor,
            0x4000,
            shape,
        ));
        assert_eq!(cursor, GPGPU_WALKER_DWORDS);
        assert_eq!(batch[2], 480);
        assert_eq!(batch[4], (GPGPU_WALKER_SIMD32_SELECT << 30) | 1);
        assert_eq!([batch[7], batch[10], batch[12]], [24, 12, 24]);
        assert_eq!(batch[13], GPGPU_WALKER_SIMD32_MASK);
        assert_eq!(batch[14], GPGPU_WALKER_BOTTOM_MASK);
    }

    #[test]
    fn xelp_3d_walker_rejects_unproven_shape_masks_and_payloads() {
        let simd16 = COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT;
        let indirect_bytes = simd16.cross_thread_data_bytes as usize
            + 4 * simd16.per_thread_data_bytes as usize;
        assert!(DirectRcsXeLp3dWalkerShape::from_authenticated_contract(
            &simd16,
            [23, 12, 24],
            XELP_3D_WALKER_LOCAL_DIMENSIONS,
            indirect_bytes,
            GPGPU_WALKER_SIMD16_MASK,
            GPGPU_WALKER_BOTTOM_MASK,
        )
        .is_none());
        assert!(DirectRcsXeLp3dWalkerShape::from_authenticated_contract(
            &simd16,
            XELP_3D_WALKER_GROUP_DIMENSIONS,
            [8, 4, 2],
            indirect_bytes,
            GPGPU_WALKER_SIMD16_MASK,
            GPGPU_WALKER_BOTTOM_MASK,
        )
        .is_none());
        assert!(xelp_3d_shape(
            &simd16,
            GPGPU_WALKER_SIMD32_MASK,
            GPGPU_WALKER_BOTTOM_MASK,
            indirect_bytes,
        )
        .is_none());
        assert!(xelp_3d_shape(
            &simd16,
            GPGPU_WALKER_SIMD16_MASK,
            0,
            indirect_bytes,
        )
        .is_none());
        assert!(xelp_3d_shape(
            &simd16,
            GPGPU_WALKER_SIMD16_MASK,
            GPGPU_WALKER_BOTTOM_MASK,
            indirect_bytes - 1,
        )
        .is_none());

        let shape = xelp_3d_shape(
            &simd16,
            GPGPU_WALKER_SIMD16_MASK,
            GPGPU_WALKER_BOTTOM_MASK,
            indirect_bytes,
        )
        .unwrap();
        let mut batch = [0xA5A5_A5A5; GPGPU_WALKER_DWORDS];
        let before = batch;
        let mut cursor = 0usize;
        assert!(!direct_rcs_push_xelp_3d_gpgpu_walker(
            &mut batch,
            &mut cursor,
            0x4020,
            shape,
        ));
        assert_eq!(cursor, 0);
        assert_eq!(batch, before);

        let mut short_batch = [0xA5A5_A5A5; GPGPU_WALKER_DWORDS - 1];
        let short_before = short_batch;
        assert!(!direct_rcs_push_xelp_3d_gpgpu_walker(
            &mut short_batch,
            &mut cursor,
            0x4000,
            shape,
        ));
        assert_eq!(cursor, 0);
        assert_eq!(short_batch, short_before);
    }

    #[test]
    fn only_busy_rolls_back_an_isolated_lane_submission() {
        assert_eq!(
            direct_rcs_submit_failure_action(crate::gpu::vgpu::VgpuError::Busy),
            DirectRcsSubmitFailureAction::RollBack,
        );
        assert_eq!(
            direct_rcs_submit_failure_action(crate::gpu::vgpu::VgpuError::DeviceLost),
            DirectRcsSubmitFailureAction::PreserveAndQuarantine,
        );
        assert_eq!(
            direct_rcs_submit_failure_action(crate::gpu::vgpu::VgpuError::OutOfMemory),
            DirectRcsSubmitFailureAction::PreserveAndQuarantine,
        );
    }

    #[test]
    fn marker_alone_cannot_retire_a_direct_rcs_submission() {
        let tail = DIRECT_RCS_BATCH_START_DWORDS * core::mem::size_of::<u32>();
        assert!(!direct_rcs_retirement_is_proven(false, tail as u32, tail));
        assert!(!direct_rcs_retirement_is_proven(true, 0, tail));
        assert!(direct_rcs_retirement_is_proven(true, tail as u32, tail));
    }

    #[test]
    fn quarantine_irreversibly_denies_shared_state_reuse() {
        let quarantined = AtomicBool::new(false);
        assert!(direct_rcs_state_reuse_permitted(&quarantined));

        quarantined.store(true, Ordering::Release);
        assert!(!direct_rcs_state_reuse_permitted(&quarantined));
    }

    #[test]
    fn copy_rect_completion_is_an_ordered_non_overlapping_post_sync_qword() {
        assert!(COPY_RECT_POST_MARKER_SLOT.is_multiple_of(2));
        assert_ne!(COPY_RECT_PRE_MARKER_SLOT, COPY_RECT_POST_MARKER_SLOT);
        assert_ne!(COPY_RECT_PRE_MARKER_SLOT, COPY_RECT_POST_MARKER_SLOT + 1);

        let mut batch = [0u32; 16];
        let mut cursor = 0usize;
        assert!(direct_rcs_push_gpgpu_dispatch_epilogue(
            &mut batch,
            &mut cursor,
            DIRECT_RCS_GPU_VA_RESULT_BASE,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
        ));
        assert_eq!(cursor, 14);

        // Producer release: HDC pipeline drain plus the full cache-flush set.
        assert_eq!(batch[0], PIPE_CONTROL_CMD | PIPE_CONTROL_HDC_PIPELINE_FLUSH);
        assert_eq!(batch[1], PIPE_CONTROL_FLUSH_BITS);

        // Ordered PIPE_CONTROL post-sync QWord, followed by batch retirement.
        assert_eq!(batch[6], PIPE_CONTROL_CMD);
        assert_eq!(
            batch[7],
            PIPE_CONTROL_FLUSH_ENABLE
                | PIPE_CONTROL_CS_STALL
                | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE
        );
        let marker_gpu = DIRECT_RCS_GPU_VA_RESULT_BASE
            + (COPY_RECT_POST_MARKER_SLOT * core::mem::size_of::<u32>()) as u64;
        assert_eq!(batch[8], marker_gpu as u32);
        assert_eq!(batch[9], (marker_gpu >> 32) as u32);
        assert_eq!(batch[10], COPY_RECT_POST_MARKER);
        assert_eq!(batch[11], 0);
        assert_eq!(batch[12], MI_BATCH_BUFFER_END);
        assert_eq!(batch[13], MI_NOOP);
    }

    #[test]
    fn sprite_pre_marker_does_not_alias_completion_qword() {
        assert!(SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT.is_multiple_of(2));
        assert_ne!(SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT, SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT);
        assert_ne!(SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT, SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT + 1);

        let mut batch = [0u32; 4];
        let mut cursor = 0usize;
        assert!(direct_rcs_push_store_marker_at(
            &mut batch,
            &mut cursor,
            DIRECT_RCS_GPU_VA_RESULT_BASE,
            SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT,
            SPRITE_QUAD_WORKLIST_PRE_MARKER,
        ));
        let marker_gpu = DIRECT_RCS_GPU_VA_RESULT_BASE
            + (SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT * core::mem::size_of::<u32>()) as u64;
        assert_eq!(batch[0], MI_STORE_DATA_IMM_GGTT_DW1);
        assert_eq!(batch[1], marker_gpu as u32);
        assert_eq!(batch[2], (marker_gpu >> 32) as u32);
        assert_eq!(batch[3], SPRITE_QUAD_WORKLIST_PRE_MARKER);
    }

    #[test]
    fn sprite_walker_payload_slots_are_aligned_and_non_overlapping() {
        assert_eq!(SPRITE_QUAD_WORKLIST_INDIRECT_BYTES, 224);
        assert_eq!(SPRITE_QUAD_WORKLIST_PAYLOAD_STRIDE_BYTES, 256);

        for descriptor in 0..SPRITE_QUAD_WORKLIST_MAX_DESCS {
            let offset = sprite_quad_worklist_payload_offset(
                SPRITE_QUAD_WORKLIST_SINGLE_PAYLOAD_BASE_OFFSET_BYTES,
                descriptor,
            )
            .unwrap();
            assert!(offset.is_multiple_of(GPGPU_WALKER_INDIRECT_ALIGNMENT_BYTES));
            assert!(offset + SPRITE_QUAD_WORKLIST_INDIRECT_BYTES <= DIRECT_RCS_BATCH_BYTES);
            if descriptor + 1 < SPRITE_QUAD_WORKLIST_MAX_DESCS {
                let next = sprite_quad_worklist_payload_offset(
                    SPRITE_QUAD_WORKLIST_SINGLE_PAYLOAD_BASE_OFFSET_BYTES,
                    descriptor + 1,
                )
                .unwrap();
                assert_eq!(next - offset, SPRITE_QUAD_WORKLIST_PAYLOAD_STRIDE_BYTES);
                assert!(offset + SPRITE_QUAD_WORKLIST_INDIRECT_BYTES <= next);
            }
        }
    }

    #[test]
    fn sprite_multi_run_payload_slots_are_aligned_and_non_overlapping() {
        for run_count in 1..=SPRITE_QUAD_WORKLIST_MAX_DESCS {
            let state_bytes = run_count * SPRITE_QUAD_WORKLIST_RUN_STATE_BLOCK_BYTES;
            let payload_base = align_up(
                SPRITE_QUAD_WORKLIST_STATE_BASE_OFFSET_BYTES + state_bytes,
                GPGPU_WALKER_INDIRECT_ALIGNMENT_BYTES,
            )
            .unwrap();
            assert!(payload_base.is_multiple_of(GPGPU_WALKER_INDIRECT_ALIGNMENT_BYTES));

            let first = sprite_quad_worklist_payload_offset(payload_base, 0).unwrap();
            let last = sprite_quad_worklist_payload_offset(
                payload_base,
                SPRITE_QUAD_WORKLIST_MAX_DESCS - 1,
            )
            .unwrap();
            assert_eq!(first, payload_base);
            assert!(last.is_multiple_of(GPGPU_WALKER_INDIRECT_ALIGNMENT_BYTES));
            assert!(last + SPRITE_QUAD_WORKLIST_INDIRECT_BYTES <= DIRECT_RCS_BATCH_BYTES);
        }
    }

    #[test]
    fn sprite_walker_rejects_a_misaligned_indirect_start() {
        let mut batch = [0u32; 15];
        let mut cursor = 0usize;
        assert!(!direct_rcs_push_sprite_quad_worklist_walker(
            &mut batch,
            &mut cursor,
            SPRITE_QUAD_WORKLIST_SINGLE_PAYLOAD_BASE_OFFSET_BYTES + 32,
            1,
            1,
            GPGPU_WALKER_SIMD16_MASK,
        ));
        assert_eq!(cursor, 0);

        assert!(direct_rcs_push_sprite_quad_worklist_walker(
            &mut batch,
            &mut cursor,
            SPRITE_QUAD_WORKLIST_SINGLE_PAYLOAD_BASE_OFFSET_BYTES,
            1,
            1,
            GPGPU_WALKER_SIMD16_MASK,
        ));
        assert_eq!(cursor, batch.len());
        assert_eq!(batch[2], SPRITE_QUAD_WORKLIST_INDIRECT_BYTES as u32);
        assert_eq!(batch[3], SPRITE_QUAD_WORKLIST_SINGLE_PAYLOAD_BASE_OFFSET_BYTES as u32,);
    }
}
