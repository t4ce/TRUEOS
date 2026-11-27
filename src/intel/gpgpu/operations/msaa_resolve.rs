// Reuse the retired legacy resolve's reserved instruction address. The shader
// bound here implements Gen12 Y0 MSS only, never the old Arc Tile64 layout.
const RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_GPU: u64 = 0x0D3C_0000;
// The render context's color VA overlaps ParticleCraft in the compute PPGTT.
// Alias the color backing into the gap below the UI4 producer arena.
const DIRECT_RCS_MSAA_SOURCE_GPU: u64 = 0x1100_0000;
const DIRECT_RCS_MSAA_SOURCE_MAX_BYTES: usize = 16 * 1024 * 1024;
const RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        RESOLVE_GEN12_Y0_MSAA_RGBA8_KERNEL_NAME,
        RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_BIN,
        RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_SPV,
        &RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );
static RESOLVE_GEN12_Y0_MSAA_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);

const _: () = {
    assert!(DIRECT_RCS_GPU_VA_PARTICLE_CRAFT_LIMIT <= DIRECT_RCS_MSAA_SOURCE_GPU);
    assert!(DIRECT_RCS_MSAA_SOURCE_GPU.is_multiple_of(4096));
    assert!(
        DIRECT_RCS_MSAA_SOURCE_GPU + DIRECT_RCS_MSAA_SOURCE_MAX_BYTES as u64
            <= crate::r::ui_surface::UI_SURFACE_GPU_BASE
    );
    let contract = RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_CPP_ABI_CONTRACT;
    assert!(matches!(contract.validate(), Ok(())));
    assert!(contract.target.supports(0x4680, 0x0C));
    assert!(contract.simd_width == 16 && contract.scratch_bytes == 0 && contract.slm_bytes == 0);
    let copy = COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT;
    assert!(contract.cross_thread_data_bytes == copy.cross_thread_data_bytes);
    assert!(contract.per_thread_data_bytes == copy.per_thread_data_bytes);
    assert!(contract.bindings.len() == 2);
    assert!(contract.bindings[0].arg_index == 0 && contract.bindings[0].bti == 0);
    assert!(contract.bindings[1].arg_index == 1 && contract.bindings[1].bti == 1);
    assert!(contract.implicit_payload_args.len() == 3);
    assert!(matches!(
        contract.implicit_payload_args[0].kind,
        GpgpuArtifactImplicitArgKind::GlobalIdOffset
    ));
    assert!(matches!(
        contract.implicit_payload_args[1].kind,
        GpgpuArtifactImplicitArgKind::LocalSize
    ));
    assert!(matches!(
        contract.implicit_payload_args[2].kind,
        GpgpuArtifactImplicitArgKind::EnqueuedLocalSize
    ));
    assert!(contract.implicit_payload_args[0].offset_bytes == 0);
    assert!(contract.implicit_payload_args[1].offset_bytes == 12);
    assert!(contract.implicit_payload_args[2].offset_bytes == 32);
    assert!(contract.per_thread_payload_args.len() == 1);
    assert!(matches!(
        contract.per_thread_payload_args[0].kind,
        GpgpuArtifactPerThreadArgKind::LocalId
    ));
    assert!(contract.per_thread_payload_args[0].offset_bytes == 0);
    assert!(contract.per_thread_payload_args[0].size_bytes == 96);
    let args = contract.payload_args;
    assert!(args.len() == 8);
    assert!(args[0].offset_bytes == 48 && args[0].size_bytes == 8);
    assert!(args[1].offset_bytes == 56 && args[1].size_bytes == 8);
    let mut i = 2;
    while i < args.len() {
        assert!(args[i].arg_index as usize == i);
        assert!(args[i].offset_bytes as usize == 56 + 4 * i && args[i].size_bytes == 4);
        i += 1;
    }
    assert!(
        RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_GPU
            + RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_BIN
                .len()
                .next_multiple_of(4096) as u64
            <= UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_GPU
    );
};

fn gen12_y0_msaa_source_layout(
    src: GpgpuRgba8Surface,
    qpitch_rows: u32,
    dst: GpgpuRgba8Surface,
    samples: u32,
) -> Option<(u32, usize)> {
    if !matches!(samples, 2 | 4)
        || !src.is_valid()
        || !dst.is_valid()
        || (src.width, src.height) != (dst.width, dst.height)
        || !src.pitch_bytes.is_multiple_of(128)
        || !qpitch_rows.is_multiple_of(32)
        || qpitch_rows < src.height
        || !dst.pitch_bytes.is_multiple_of(4)
        || !src.gpu.is_multiple_of(4096)
        || !dst.gpu.is_multiple_of(4096)
    {
        return None;
    }
    let slice_bytes = src.pitch_bytes.checked_mul(qpitch_rows)?;
    let src_required = slice_bytes.checked_mul(samples)? as usize;
    let dst_required = dst.pitch_bytes.checked_mul(dst.height)? as usize;
    let overlaps = |a: u64, a_bytes: usize, b: u64, b_bytes: usize| {
        let a_end = a.checked_add(a_bytes as u64)?;
        let b_end = b.checked_add(b_bytes as u64)?;
        Some(a < b_end && b < a_end)
    };
    if src_required > src.bytes
        || src.bytes > DIRECT_RCS_MSAA_SOURCE_MAX_BYTES
        || dst_required > dst.bytes
        || overlaps(src.phys, src.bytes, dst.phys, dst.bytes)?
        || overlaps(src.gpu, src.bytes, dst.gpu, dst.bytes)?
    {
        return None;
    }
    Some((slice_bytes, src_required))
}

/// Resolve two or four uncompressed Y0 color slices directly into the linear UI4
/// back buffer. The renderer keeps the source pinned and retired throughout.
/// A submitted failure leaves both allocations live until device recovery.
pub(crate) fn resolve_gen12_y0_msaa_rgba8(
    src: GpgpuRgba8Surface,
    qpitch_rows: u32,
    dst: GpgpuRgba8Surface,
    samples: u32,
) -> GpgpuRgba8KernelResult {
    let src = GpgpuRgba8Surface {
        gpu: DIRECT_RCS_MSAA_SOURCE_GPU,
        ..src
    };
    let Some((slice_bytes, src_required)) =
        gen12_y0_msaa_source_layout(src, qpitch_rows, dst, samples)
    else {
        return GpgpuRgba8KernelResult::default();
    };
    // Growing scratch reuse may reserve four slices for a subsequent 2x frame.
    let src = GpgpuRgba8Surface {
        bytes: src_required,
        ..src
    };
    let Some(dispatch) = fill_rect_2d_dispatch(dst.width, dst.height) else {
        return GpgpuRgba8KernelResult::default();
    };
    let started = direct_rcs_now_tick();
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuRgba8KernelResult::default();
    };
    let upload = {
        let mut cached = RESOLVE_GEN12_Y0_MSAA_RGBA8_UPLOAD.lock();
        if cached.is_none() {
            *cached = upload_artifact(
                dev,
                RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_ARTIFACT,
                RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_GPU,
            );
        }
        *cached
    };
    let (Some(upload), Some(state)) = (upload, direct_rcs_state_once(dev)) else {
        return GpgpuRgba8KernelResult::default();
    };
    let prepared = direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && direct_rcs_init_ppgtt(state)
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        && direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes)
        && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes)
        && direct_rcs_encode_gen12_y0_msaa_resolve(
            state,
            upload,
            src,
            dst,
            slice_bytes,
            samples,
            dispatch,
        );
    let submission = if prepared {
        direct_rcs_submit_batch_state(dev, state)
    } else {
        DirectRcsSubmissionState::Rejected
    };
    let submitted = submission.may_have_submitted();
    let marker = if submission.can_poll() {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = marker == COPY_RECT_POST_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted,
        marker,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

fn direct_rcs_encode_gen12_y0_msaa_resolve(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    slice_bytes: u32,
    samples: u32,
    dispatch: FillRect2dDispatch,
) -> bool {
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    let shader = RESOLVE_GEN12_Y0_MSAA_RGBA8_ADLS_CPP_ABI_CONTRACT;
    if !direct_rcs_write_copy_rect_interface_descriptor_at(
        state,
        COPY_RECT_BATCH_IDD_OFFSET_BYTES,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        shader.entry_offset,
    ) || !direct_rcs_write_copy_rect_surface_states_at(
        state,
        COPY_RECT_BATCH_BINDING_TABLE_OFFSET_BYTES,
        COPY_RECT_BATCH_SRC_SURFACE_STATE_OFFSET_BYTES,
        COPY_RECT_BATCH_DST_SURFACE_STATE_OFFSET_BYTES,
        src.gpu,
        src.bytes,
        dst.gpu,
        dst.bytes,
    ) {
        return false;
    }

    let mut payload = [0u8; COPY_RECT_INDIRECT_BYTES];
    for (start, size) in [(12, 16u32), (16, 1), (20, 1), (32, 16), (36, 1), (40, 1)] {
        payload[start..start + 4].copy_from_slice(&size.to_le_bytes());
    }
    payload[48..56].copy_from_slice(&src.gpu.to_le_bytes());
    payload[56..64].copy_from_slice(&dst.gpu.to_le_bytes());
    let values = [
        src.pitch_bytes,
        dst.pitch_bytes,
        slice_bytes,
        samples | u32::from(src.storage_order != dst.storage_order),
        dst.width,
        dst.height,
    ];
    for (arg, value) in shader.payload_args[2..].iter().zip(values) {
        let start = arg.offset_bytes as usize;
        payload[start..start + 4].copy_from_slice(&value.to_le_bytes());
    }
    for lane in 0..16u16 {
        let start = COPY_RECT_CROSS_THREAD_BYTES + 2 * usize::from(lane);
        payload[start..start + 2].copy_from_slice(&lane.to_le_bytes());
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            payload.as_ptr(),
            state
                .batch_virt
                .add(COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES),
            payload.len(),
        );
    }
    direct_rcs_finish_copy_rect_dispatch_batch(
        state,
        upload,
        COPY_RECT_BATCH_PAYLOAD_BASE_OFFSET_BYTES,
        COPY_RECT_INDIRECT_BYTES,
        dispatch,
    )
}

#[cfg(test)]
mod gen12_y0_msaa_resolve_tests {
    use super::*;

    #[test]
    fn admits_two_or_four_separate_y0_color_slices_and_a_linear_output() {
        let src = GpgpuRgba8Surface::new(
            0x1000_0000,
            DIRECT_RCS_MSAA_SOURCE_GPU,
            5_898_240,
            640,
            560,
            2560,
        )
        .unwrap();
        let dst = GpgpuRgba8Surface::new_bgra(0x2000_0000, 0x3000_0000, 1_433_600, 640, 560, 2560)
            .unwrap();
        assert_eq!(
            gen12_y0_msaa_source_layout(src, 576, dst, 4),
            Some((1_474_560, 5_898_240))
        );
        assert_eq!(
            gen12_y0_msaa_source_layout(src, 576, dst, 2),
            Some((1_474_560, 2_949_120))
        );
        let two_slices = GpgpuRgba8Surface {
            bytes: src.bytes / 2,
            ..src
        };
        assert_eq!(
            gen12_y0_msaa_source_layout(two_slices, 576, dst, 2),
            Some((1_474_560, 2_949_120))
        );
        assert_eq!(gen12_y0_msaa_source_layout(two_slices, 576, dst, 4), None);
        assert_eq!(gen12_y0_msaa_source_layout(src, 576, dst, 3), None);
        assert_eq!(gen12_y0_msaa_source_layout(src, 560, dst, 2), None);
        let short = GpgpuRgba8Surface {
            bytes: src.bytes - 4096,
            ..src
        };
        assert_eq!(gen12_y0_msaa_source_layout(short, 576, dst, 4), None);
        let oversize = GpgpuRgba8Surface {
            bytes: DIRECT_RCS_MSAA_SOURCE_MAX_BYTES + 4096,
            ..src
        };
        assert_eq!(gen12_y0_msaa_source_layout(oversize, 576, dst, 4), None);
        let aliased = GpgpuRgba8Surface {
            phys: src.phys + 4096,
            ..dst
        };
        assert_eq!(gen12_y0_msaa_source_layout(src, 576, aliased, 2), None);
    }
}
