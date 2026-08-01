const HELIO_TRANSFORM_IDD_OFFSET_BYTES: usize = 0x1000;
const HELIO_TRANSFORM_BINDING_TABLE_OFFSET_BYTES: usize = 0x1040;
const HELIO_TRANSFORM_SURFACE_STATE_OFFSET_BYTES: usize = 0x1080;
const HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES: usize = 0x1200;
const HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES: usize = 0x1300;
const HELIO_TRANSFORM_BINDINGS: usize = 5;
const HELIO_TRANSFORM_MODE_PREPARE: u32 = 0;
const HELIO_TRANSFORM_MODE_ROWS: u32 = 1;
const HELIO_TRANSFORM_CROSS_THREAD_BYTES: usize = 128;
const HELIO_TRANSFORM_PER_THREAD_BYTES: usize = 96;
const HELIO_TRANSFORM_INDIRECT_BYTES: usize =
    HELIO_TRANSFORM_CROSS_THREAD_BYTES + HELIO_TRANSFORM_PER_THREAD_BYTES;

const _: () = {
    assert!(HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES.is_multiple_of(64));
    assert!(HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES.is_multiple_of(64));
    assert!(
        HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES + HELIO_TRANSFORM_INDIRECT_BYTES
            <= HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES
    );
    assert!(
        HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES + HELIO_TRANSFORM_INDIRECT_BYTES
            <= GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES
    );
};

/// Encode one Render-secondary compute batch. It contains two ordered walkers
/// of the same native entrypoint:
///
/// 1. prepare all exact WGPU indirect records and zero instance counts;
/// 2. expand/compact rows in parallel and atomically publish those counts.
///
/// The batch restores the 3D pipeline before its second-level BBE. The next
/// resident draw secondary therefore consumes GPU-authored instance,
/// compacted-index, and indirect storage without a context switch or readback.
pub(crate) fn encode_helio_retained_transform_secondary(
    state: &mut GpgpuHelioRetainedTransformStateBlob<'_>,
    artifact: GpgpuHelioRetainedTransformArtifactMapping,
    dispatch: GpgpuHelioRetainedTransformDispatch,
) -> Result<GpgpuHelioRetainedTransformEncoding, GpgpuHelioTransformError> {
    use GpgpuHelioTransformError as Error;

    dispatch.validate()?;
    validate_helio_transform_artifact(artifact)?;
    validate_helio_transform_encoder_ranges(state.gpu, artifact, dispatch)?;

    let bytes = &mut state.bytes[..GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES];
    bytes.fill(0);
    write_helio_transform_interface_descriptor(bytes, artifact.entry_offset)?;
    write_helio_transform_surfaces(bytes, dispatch)?;
    write_helio_transform_payload(
        bytes,
        HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES,
        dispatch,
        HELIO_TRANSFORM_MODE_PREPARE,
    )?;
    write_helio_transform_payload(
        bytes,
        HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES,
        dispatch,
        HELIO_TRANSFORM_MODE_ROWS,
    )?;

    let prepare_groups = dispatch.draw_count.div_ceil(16);
    let transform_groups = dispatch.row_count.div_ceil(16);
    // Gen12 applies RightExecutionMask to every SIMD hardware thread, not
    // merely to the final X group.  Keep every lane live in every group and
    // let the entrypoint's item >= draw_count / row_count guards reject only
    // the padded lanes in each pass's final group.
    let prepare_mask = GPGPU_WALKER_SIMD16_MASK;
    let transform_mask = GPGPU_WALKER_SIMD16_MASK;
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            bytes.as_mut_ptr().cast::<u32>(),
            bytes.len() / core::mem::size_of::<u32>(),
        )
    };
    let mut cursor = 0usize;
    let mut ok =
        direct_rcs_push_gpgpu_dispatch_prologue(batch, &mut cursor, artifact.upload, state.gpu);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, COPY_RECT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, HELIO_TRANSFORM_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES,
        HELIO_TRANSFORM_INDIRECT_BYTES,
        prepare_groups,
        1,
        prepare_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    // Producer/consumer ordering inside the compute secondary: the row walker
    // must observe every zero count and immutable draw field from PREPARE.
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_gpgpu_walker_2d(
        batch,
        &mut cursor,
        HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES,
        HELIO_TRANSFORM_INDIRECT_BYTES,
        transform_groups,
        1,
        transform_mask,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    // GPU producer release for all following resident draw secondaries.
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH,
        PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);
    if !ok || cursor * core::mem::size_of::<u32>() >= HELIO_TRANSFORM_IDD_OFFSET_BYTES {
        return Err(Error::BatchBuffer);
    }

    super::dma_flush(bytes.as_mut_ptr(), GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES);
    Ok(GpgpuHelioRetainedTransformEncoding {
        command_dwords: cursor,
        prepare_groups,
        transform_groups,
    })
}

fn validate_helio_transform_artifact(
    artifact: GpgpuHelioRetainedTransformArtifactMapping,
) -> Result<(), GpgpuHelioTransformError> {
    let upload = artifact.upload;
    if artifact.gpu != upload.gpu
        || artifact.phys != upload.phys
        || artifact.bytes != upload.bytes
        || artifact.mapped_bytes != upload.mapped_bytes
        || artifact.entry_offset != HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.entry_offset
        || upload.name != HELIO_RETAINED_TRANSFORM_KERNEL_NAME
        || upload.gpu != HELIO_RETAINED_TRANSFORM_ADLS_GPU
        || upload.bytes != HELIO_RETAINED_TRANSFORM_ADLS_BIN.len()
        || upload.bin_sha256 != HELIO_RETAINED_TRANSFORM_ADLS_BIN_SHA256
        || !upload.verified
        || upload.mapped_bytes < upload.bytes
    {
        return Err(GpgpuHelioTransformError::Artifact);
    }
    Ok(())
}

fn validate_helio_transform_encoder_ranges(
    state_gpu: u64,
    artifact: GpgpuHelioRetainedTransformArtifactMapping,
    dispatch: GpgpuHelioRetainedTransformDispatch,
) -> Result<(), GpgpuHelioTransformError> {
    let state = (state_gpu, GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES);
    let artifact_range = (artifact.gpu, artifact.mapped_bytes);
    let buffers = [
        (dispatch.transforms.gpu, dispatch.transforms.bytes),
        (dispatch.draw_templates.gpu, dispatch.draw_templates.bytes),
        (dispatch.instances.gpu, dispatch.instances.bytes),
        (dispatch.compacted_indices.gpu, dispatch.compacted_indices.bytes),
        (dispatch.indirect_args.gpu, dispatch.indirect_args.bytes),
    ];
    if ranges_overlap(state, artifact_range)
        || buffers
            .into_iter()
            .any(|buffer| ranges_overlap(state, buffer) || ranges_overlap(artifact_range, buffer))
    {
        return Err(GpgpuHelioTransformError::AddressOverlap);
    }
    Ok(())
}

fn write_helio_transform_interface_descriptor(
    bytes: &mut [u8],
    entry_offset: u64,
) -> Result<(), GpgpuHelioTransformError> {
    write_helio_transform_u32(bytes, HELIO_TRANSFORM_IDD_OFFSET_BYTES, entry_offset as u32)?;
    write_helio_transform_u32(
        bytes,
        HELIO_TRANSFORM_IDD_OFFSET_BYTES + 2 * 4,
        IDD_THREAD_PREEMPTION_DISABLE,
    )?;
    write_helio_transform_u32(
        bytes,
        HELIO_TRANSFORM_IDD_OFFSET_BYTES + 4 * 4,
        HELIO_TRANSFORM_BINDING_TABLE_OFFSET_BYTES as u32 | HELIO_TRANSFORM_BINDINGS as u32,
    )?;
    write_helio_transform_u32(bytes, HELIO_TRANSFORM_IDD_OFFSET_BYTES + 5 * 4, 3 << 16)?;
    write_helio_transform_u32(
        bytes,
        HELIO_TRANSFORM_IDD_OFFSET_BYTES + 6 * 4,
        GPGPU_WALKER_GROUP_THREADS,
    )?;
    write_helio_transform_u32(
        bytes,
        HELIO_TRANSFORM_IDD_OFFSET_BYTES + 7 * 4,
        (HELIO_TRANSFORM_CROSS_THREAD_BYTES / 32) as u32,
    )
}

fn write_helio_transform_surfaces(
    bytes: &mut [u8],
    dispatch: GpgpuHelioRetainedTransformDispatch,
) -> Result<(), GpgpuHelioTransformError> {
    let surfaces = [
        dispatch.transforms,
        dispatch.draw_templates,
        dispatch.instances,
        dispatch.compacted_indices,
        dispatch.indirect_args,
    ];
    for (index, slice) in surfaces.into_iter().enumerate() {
        let surface_offset = HELIO_TRANSFORM_SURFACE_STATE_OFFSET_BYTES
            + index * COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
        write_helio_transform_u32(
            bytes,
            HELIO_TRANSFORM_BINDING_TABLE_OFFSET_BYTES + index * 4,
            surface_offset as u32,
        )?;
        write_helio_transform_buffer_surface(bytes, surface_offset, slice)?;
    }
    Ok(())
}

fn write_helio_transform_buffer_surface(
    bytes: &mut [u8],
    offset: usize,
    slice: GpgpuHelioBufferSlice,
) -> Result<(), GpgpuHelioTransformError> {
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let surface = bytes
        .get_mut(offset..offset + surface_bytes)
        .ok_or(GpgpuHelioTransformError::BatchBuffer)?;
    surface.fill(0);
    let extent = slice.bytes.saturating_sub(1);
    let width_minus1 = (extent & 0x7F) as u32;
    let height_minus1 = ((extent >> 7) & 0x3FFF) as u32;
    let depth_minus1 = ((extent >> 21) & 0x7FF) as u32;
    let mocs = direct_rcs_encode_mocs_index(RENDER_MOCS_INDEX);
    write_helio_transform_u32(surface, 0, (SURFTYPE_BUFFER << 29) | (SURFACE_FORMAT_RAW << 18))?;
    write_helio_transform_u32(surface, 4, mocs << 24)?;
    write_helio_transform_u32(surface, 8, (height_minus1 << 16) | width_minus1)?;
    write_helio_transform_u32(surface, 12, depth_minus1 << 21)?;
    write_helio_transform_u64(surface, 8 * 4, slice.gpu)
}

fn write_helio_transform_payload(
    bytes: &mut [u8],
    offset: usize,
    dispatch: GpgpuHelioRetainedTransformDispatch,
    mode: u32,
) -> Result<(), GpgpuHelioTransformError> {
    let payload = bytes
        .get_mut(offset..offset + HELIO_TRANSFORM_INDIRECT_BYTES)
        .ok_or(GpgpuHelioTransformError::BatchBuffer)?;
    payload.fill(0);
    // .ze_info implicit local/enqueued-local sizes.
    for implicit_offset in [12usize, 32] {
        write_helio_transform_u32(payload, implicit_offset, 16)?;
        write_helio_transform_u32(payload, implicit_offset + 4, 1)?;
        write_helio_transform_u32(payload, implicit_offset + 8, 1)?;
    }
    let pointers = [
        dispatch.transforms.gpu,
        dispatch.draw_templates.gpu,
        dispatch.instances.gpu,
        dispatch.compacted_indices.gpu,
        dispatch.indirect_args.gpu,
    ];
    for (index, pointer) in pointers.into_iter().enumerate() {
        write_helio_transform_u64(payload, 48 + index * 8, pointer)?;
    }
    write_helio_transform_u32(payload, 88, dispatch.row_count)?;
    write_helio_transform_u32(payload, 92, dispatch.draw_count)?;
    write_helio_transform_u32(payload, 96, mode)?;
    for lane in 0..16usize {
        write_helio_transform_u16(
            payload,
            HELIO_TRANSFORM_CROSS_THREAD_BYTES + lane * 2,
            lane as u16,
        )?;
        write_helio_transform_u16(
            payload,
            HELIO_TRANSFORM_CROSS_THREAD_BYTES + (16 + lane) * 2,
            0,
        )?;
        write_helio_transform_u16(
            payload,
            HELIO_TRANSFORM_CROSS_THREAD_BYTES + (32 + lane) * 2,
            0,
        )?;
    }
    Ok(())
}

fn write_helio_transform_u16(
    bytes: &mut [u8],
    offset: usize,
    value: u16,
) -> Result<(), GpgpuHelioTransformError> {
    let dst = bytes
        .get_mut(offset..offset + 2)
        .ok_or(GpgpuHelioTransformError::BatchBuffer)?;
    dst.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_helio_transform_u32(
    bytes: &mut [u8],
    offset: usize,
    value: u32,
) -> Result<(), GpgpuHelioTransformError> {
    let dst = bytes
        .get_mut(offset..offset + 4)
        .ok_or(GpgpuHelioTransformError::BatchBuffer)?;
    dst.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_helio_transform_u64(
    bytes: &mut [u8],
    offset: usize,
    value: u64,
) -> Result<(), GpgpuHelioTransformError> {
    let dst = bytes
        .get_mut(offset..offset + 8)
        .ok_or(GpgpuHelioTransformError::BatchBuffer)?;
    dst.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod helio_transform_encoder_tests {
    use super::*;

    fn artifact() -> GpgpuHelioRetainedTransformArtifactMapping {
        let upload = UploadedKernelArtifact {
            name: HELIO_RETAINED_TRANSFORM_KERNEL_NAME,
            target: "adls",
            source: HELIO_RETAINED_TRANSFORM_SOURCE_PATH,
            gpu: HELIO_RETAINED_TRANSFORM_ADLS_GPU,
            phys: 0x0200_0000,
            bytes: HELIO_RETAINED_TRANSFORM_ADLS_BIN.len(),
            mapped_bytes: HELIO_RETAINED_TRANSFORM_ADLS_BIN
                .len()
                .next_multiple_of(4096),
            verified: true,
            bin_sha256: HELIO_RETAINED_TRANSFORM_ADLS_BIN_SHA256,
            device_id: 0x4680,
            revision_id: 0x0C,
            abi_schema_version: Some(GPGPU_KERNEL_ABI_SCHEMA_VERSION),
            address_space: GpgpuArtifactAddressSpace::CallerPpgtt,
        };
        GpgpuHelioRetainedTransformArtifactMapping {
            gpu: upload.gpu,
            phys: upload.phys,
            bytes: upload.bytes,
            mapped_bytes: upload.mapped_bytes,
            entry_offset: HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.entry_offset,
            upload,
        }
    }

    fn dispatch() -> GpgpuHelioRetainedTransformDispatch {
        GpgpuHelioRetainedTransformDispatch {
            transforms: GpgpuHelioBufferSlice::new(0x0300_0000, 337 * 64),
            draw_templates: GpgpuHelioBufferSlice::new(0x0310_0000, 12 * 24),
            instances: GpgpuHelioBufferSlice::new(0x0320_0000, 337 * 208),
            compacted_indices: GpgpuHelioBufferSlice::new(0x0340_0000, 337 * 4),
            indirect_args: GpgpuHelioBufferSlice::new(0x0350_0000, 12 * 20),
            row_count: 337,
            draw_count: 12,
        }
    }

    #[test]
    fn encoder_emits_ordered_prepare_and_parallel_transform_walkers() {
        let mut storage = alloc::vec![0u8; GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES];
        let mut state =
            GpgpuHelioRetainedTransformStateBlob::new(&mut storage, 0x0100_0000).unwrap();
        let encoding =
            encode_helio_retained_transform_secondary(&mut state, artifact(), dispatch()).unwrap();
        assert_eq!(encoding.prepare_groups, 1);
        assert_eq!(encoding.transform_groups, 22);
        assert!(encoding.prepare_groups * 16 >= dispatch().draw_count);
        assert!((encoding.prepare_groups - 1) * 16 < dispatch().draw_count);
        assert!(encoding.transform_groups * 16 >= dispatch().row_count);
        assert!((encoding.transform_groups - 1) * 16 < dispatch().row_count);
        let commands = unsafe {
            core::slice::from_raw_parts(
                state.bytes().as_ptr().cast::<u32>(),
                encoding.command_dwords,
            )
        };
        let walkers = commands
            .iter()
            .enumerate()
            .filter_map(|(index, command)| (*command == GPGPU_WALKER_CMD).then_some(index))
            .collect::<alloc::vec::Vec<_>>();
        assert_eq!(walkers.len(), 2);
        assert_eq!(commands[walkers[0] + 13], GPGPU_WALKER_SIMD16_MASK);
        assert_eq!(commands[walkers[1] + 13], GPGPU_WALKER_SIMD16_MASK);
        assert!(commands.contains(&PIPELINE_SELECT_3D));
        assert_eq!(
            u32::from_le_bytes(
                state.bytes()[HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES + 96
                    ..HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES + 100]
                    .try_into()
                    .unwrap()
            ),
            HELIO_TRANSFORM_MODE_PREPARE
        );
        assert_eq!(
            u32::from_le_bytes(
                state.bytes()[HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES + 96
                    ..HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES + 100]
                    .try_into()
                    .unwrap()
            ),
            HELIO_TRANSFORM_MODE_ROWS
        );
    }
}
