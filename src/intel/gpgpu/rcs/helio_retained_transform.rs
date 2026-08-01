const HELIO_TRANSFORM_IDD_OFFSET_BYTES: usize = 0x1000;
const HELIO_TRANSFORM_BINDING_TABLE_OFFSET_BYTES: usize = 0x1040;
const HELIO_TRANSFORM_SURFACE_STATE_OFFSET_BYTES: usize = 0x1080;
const HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES: usize = 0x1500;
const HELIO_TRANSFORM_LOCAL_PAYLOAD_OFFSET_BYTES: usize = 0x1680;
const HELIO_TRANSFORM_RESOLVE_PAYLOAD_OFFSET_BYTES: usize = 0x1800;
const HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES: usize = 0x1980;
const HELIO_TRANSFORM_BINDINGS: usize = 16;
const HELIO_TRANSFORM_MODE_PREPARE_NATIVE: u32 = 0;
const HELIO_TRANSFORM_MODE_ROWS_NATIVE: u32 = 1;
const HELIO_TRANSFORM_MODE_PREPARE_EXPANDED: u32 = 2;
const HELIO_TRANSFORM_MODE_ROWS_EXPANDED: u32 = 3;
const HELIO_TRANSFORM_MODE_HIERARCHY_LOCAL: u32 = 4;
const HELIO_TRANSFORM_MODE_HIERARCHY_RESOLVE: u32 = 5;
const HELIO_TRANSFORM_MODE_HIERARCHY_ROWS_NATIVE: u32 = 6;
const HELIO_TRANSFORM_MODE_HIERARCHY_ROWS_EXPANDED: u32 = 7;
const HELIO_TRANSFORM_CROSS_THREAD_BYTES: usize = 224;
const HELIO_TRANSFORM_PER_THREAD_BYTES: usize = 96;
const HELIO_TRANSFORM_INDIRECT_BYTES: usize =
    HELIO_TRANSFORM_CROSS_THREAD_BYTES + HELIO_TRANSFORM_PER_THREAD_BYTES;
// MEDIA_VFE_STATE encodes the per-thread URB entry size in 32-byte units.
// Derive it from the exact walker payload: growing the kernel ABI must grow
// the VFE allocation with it, or the first walker triggers a MemoryCat fault.
const HELIO_GPGPU_VFE_DW5_UOS: u32 = ((HELIO_TRANSFORM_INDIRECT_BYTES / 32) as u32) << 16;
// Gen12.0 exposes HDC Pipeline Flush at DW0 bit9. Bit11 is MBZ until gfx12.5.
const HELIO_TRANSFORM_RELEASE_HEADER_BITS: u32 = PIPE_CONTROL_HDC_PIPELINE_FLUSH;
// VF Cache Invalidation is a 3D-only DW1 control on Gen12. Keep it out of the
// pre-PIPELINE_SELECT GPGPU invalidation, then acquire the compute-authored
// position stream explicitly after switching back to the 3D pipeline.
const HELIO_TRANSFORM_VF_CACHE_INVALIDATE: u32 = 1 << 4;
const HELIO_TRANSFORM_3D_CONSUMER_BITS: u32 =
    PIPE_CONTROL_CS_STALL | HELIO_TRANSFORM_VF_CACHE_INVALIDATE;
const _: () = assert!(PIPE_CONTROL_CMD | HELIO_TRANSFORM_RELEASE_HEADER_BITS == 0x7A00_0204);

const _: () = {
    assert!(HELIO_TRANSFORM_INDIRECT_BYTES.is_multiple_of(32));
    assert!(HELIO_GPGPU_VFE_DW5_UOS == 0x000A_0000);
    assert!(HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES.is_multiple_of(64));
    assert!(HELIO_TRANSFORM_LOCAL_PAYLOAD_OFFSET_BYTES.is_multiple_of(64));
    assert!(HELIO_TRANSFORM_RESOLVE_PAYLOAD_OFFSET_BYTES.is_multiple_of(64));
    assert!(HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES.is_multiple_of(64));
    assert!(
        HELIO_TRANSFORM_SURFACE_STATE_OFFSET_BYTES
            + HELIO_TRANSFORM_BINDINGS
                * COPY_RECT_SURFACE_STATE_DWORDS
                * core::mem::size_of::<u32>()
            <= HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES
    );
    assert!(
        HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES + HELIO_TRANSFORM_INDIRECT_BYTES
            <= HELIO_TRANSFORM_LOCAL_PAYLOAD_OFFSET_BYTES
    );
    assert!(
        HELIO_TRANSFORM_LOCAL_PAYLOAD_OFFSET_BYTES + HELIO_TRANSFORM_INDIRECT_BYTES
            <= HELIO_TRANSFORM_RESOLVE_PAYLOAD_OFFSET_BYTES
    );
    assert!(
        HELIO_TRANSFORM_RESOLVE_PAYLOAD_OFFSET_BYTES + HELIO_TRANSFORM_INDIRECT_BYTES
            <= HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES
    );
    assert!(
        HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES + HELIO_TRANSFORM_INDIRECT_BYTES
            <= GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES
    );
    assert!(
        HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes
            == HELIO_TRANSFORM_CROSS_THREAD_BYTES as u32
    );
    assert!(
        HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.per_thread_data_bytes
            == HELIO_TRANSFORM_PER_THREAD_BYTES as u32
    );
};

/// Encode one Render-secondary compute batch. It contains two flat walkers or
/// four ordered hierarchy walkers of the same native entrypoint:
///
/// 1. prepare all exact WGPU indirect records and publish group capacities;
/// 2. materialize dirty dynamic TRS nodes as local affine matrices;
/// 3. independently resolve dirty world nodes across SIMD16 lanes;
/// 4. emit dirty rows into producer-assigned compact slots.
///
/// The batch restores the 3D pipeline before its second-level BBE. The next
/// resident draw secondary therefore consumes either GPU-authored instance
/// matrices or the compatibility Float3 stream, plus indexed-indirect storage,
/// without a context switch or readback.
pub(crate) fn encode_helio_retained_transform_secondary(
    state: &mut GpgpuHelioRetainedTransformStateBlob<'_>,
    artifact: GpgpuHelioRetainedTransformArtifactMapping,
    dispatch: GpgpuHelioRetainedTransformDispatch,
    diagnostic_gpu: u64,
) -> Result<GpgpuHelioRetainedTransformEncoding, GpgpuHelioTransformError> {
    use GpgpuHelioTransformError as Error;

    dispatch.validate()?;
    validate_helio_transform_artifact(artifact)?;
    validate_helio_transform_encoder_ranges(state.gpu, artifact, dispatch)?;

    let bytes = &mut state.bytes[..GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES];
    bytes.fill(0);
    write_helio_transform_interface_descriptor(bytes, artifact.entry_offset)?;
    write_helio_transform_surfaces(bytes, dispatch)?;
    let expanded = dispatch.output == GpgpuHelioRetainedTransformOutput::ExpandedPositions;
    let prepare_mode = if expanded {
        HELIO_TRANSFORM_MODE_PREPARE_EXPANDED
    } else {
        HELIO_TRANSFORM_MODE_PREPARE_NATIVE
    };
    write_helio_transform_payload(
        bytes,
        HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES,
        dispatch,
        prepare_mode,
    )?;
    let (local_groups, hierarchy_groups, transform_groups, row_mode) =
        if let Some(graph) = dispatch.hierarchy {
            write_helio_transform_payload(
                bytes,
                HELIO_TRANSFORM_LOCAL_PAYLOAD_OFFSET_BYTES,
                dispatch,
                HELIO_TRANSFORM_MODE_HIERARCHY_LOCAL,
            )?;
            write_helio_transform_payload(
                bytes,
                HELIO_TRANSFORM_RESOLVE_PAYLOAD_OFFSET_BYTES,
                dispatch,
                HELIO_TRANSFORM_MODE_HIERARCHY_RESOLVE,
            )?;
            (
                graph.dirty_local_count.div_ceil(16),
                graph.dirty_world_count.div_ceil(16),
                graph.dirty_row_count.div_ceil(16),
                if expanded {
                    HELIO_TRANSFORM_MODE_HIERARCHY_ROWS_EXPANDED
                } else {
                    HELIO_TRANSFORM_MODE_HIERARCHY_ROWS_NATIVE
                },
            )
        } else {
            (
                0,
                0,
                dispatch.row_count.div_ceil(16),
                if expanded {
                    HELIO_TRANSFORM_MODE_ROWS_EXPANDED
                } else {
                    HELIO_TRANSFORM_MODE_ROWS_NATIVE
                },
            )
        };
    write_helio_transform_payload(
        bytes,
        HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES,
        dispatch,
        row_mode,
    )?;

    let prepare_groups = dispatch.draw_count.div_ceil(16);
    // Gen12 applies RightExecutionMask to every SIMD hardware thread, not
    // merely to the final X group.  Keep every lane live in every group and
    // let the entrypoint's item >= draw_count / row_count guards reject only
    // the padded lanes in each pass's final group.
    let prepare_mask = GPGPU_WALKER_SIMD16_MASK;
    let local_mask = GPGPU_WALKER_SIMD16_MASK;
    let hierarchy_mask = GPGPU_WALKER_SIMD16_MASK;
    let transform_mask = GPGPU_WALKER_SIMD16_MASK;
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            bytes.as_mut_ptr().cast::<u32>(),
            bytes.len() / core::mem::size_of::<u32>(),
        )
    };
    let mut cursor = 0usize;
    let mut ok = direct_rcs_push_gpgpu_dispatch_prologue_with_vfe_dw5(
        batch,
        &mut cursor,
        artifact.upload,
        state.gpu,
        HELIO_GPGPU_VFE_DW5_UOS,
    );
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        diagnostic_gpu,
        GPGPU_HELIO_DIAGNOSTIC_SLOT_PROLOGUE,
        GPGPU_HELIO_DIAGNOSTIC_PROLOGUE,
    );
    // Gen12 requires MEDIA_STATE_FLUSH before loading a new interface
    // descriptor so temporary VFE descriptor storage is clear. This secondary
    // enters GPGPU from a reused Render0 3D context, not a fresh media context.
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
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
    // must observe every published capacity and immutable draw field.
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        HELIO_TRANSFORM_RELEASE_HEADER_BITS,
        PIPE_CONTROL_FLUSH_BITS,
    );
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        diagnostic_gpu,
        GPGPU_HELIO_DIAGNOSTIC_SLOT_PREPARE,
        GPGPU_HELIO_DIAGNOSTIC_PREPARE,
    );
    if local_groups != 0 {
        ok &= direct_rcs_push_gpgpu_walker_2d(
            batch,
            &mut cursor,
            HELIO_TRANSFORM_LOCAL_PAYLOAD_OFFSET_BYTES,
            HELIO_TRANSFORM_INDIRECT_BYTES,
            local_groups,
            1,
            local_mask,
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        // Dynamic TRS seeds have now authored their node-local affine rows.
        // Publish those rows before any independent ancestor walker reads
        // them, including when parent and leaf occupy different workgroups.
        ok &= direct_rcs_push_pipe_control_full(
            batch,
            &mut cursor,
            HELIO_TRANSFORM_RELEASE_HEADER_BITS,
            PIPE_CONTROL_FLUSH_BITS,
        );
    }
    if hierarchy_groups != 0 {
        ok &= direct_rcs_push_gpgpu_walker_2d(
            batch,
            &mut cursor,
            HELIO_TRANSFORM_RESOLVE_PAYLOAD_OFFSET_BYTES,
            HELIO_TRANSFORM_INDIRECT_BYTES,
            hierarchy_groups,
            1,
            hierarchy_mask,
        );
        ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
        ok &= direct_rcs_push(batch, &mut cursor, 0);
        // Every dirty world is now complete. The dirty-row emitter can read a
        // leaf written by any preceding SIMD16 group without a CPU fence.
        ok &= direct_rcs_push_pipe_control_full(
            batch,
            &mut cursor,
            HELIO_TRANSFORM_RELEASE_HEADER_BITS,
            PIPE_CONTROL_FLUSH_BITS,
        );
    }
    if transform_groups != 0 {
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
    }
    // GPU producer release for all following resident draw secondaries.
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        HELIO_TRANSFORM_RELEASE_HEADER_BITS,
        PIPE_CONTROL_FLUSH_BITS,
    );
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        diagnostic_gpu,
        GPGPU_HELIO_DIAGNOSTIC_SLOT_TRANSFORM,
        GPGPU_HELIO_DIAGNOSTIC_TRANSFORM,
    );
    // Split producer release from consumer invalidation before changing the
    // pipeline. The release above makes transformed rows globally visible;
    // this command discards stale 3D-side views of those buffers.
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        HELIO_TRANSFORM_RELEASE_HEADER_BITS,
        PIPE_CONTROL_INVALIDATE_BITS,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        HELIO_TRANSFORM_RELEASE_HEADER_BITS,
        HELIO_TRANSFORM_3D_CONSUMER_BITS,
    );
    ok &= direct_rcs_push_store_marker_at(
        batch,
        &mut cursor,
        diagnostic_gpu,
        GPGPU_HELIO_DIAGNOSTIC_SLOT_3D_HANDOFF,
        GPGPU_HELIO_DIAGNOSTIC_3D_HANDOFF,
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
        local_groups,
        hierarchy_groups,
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
    let buffers = helio_transform_surfaces(dispatch).map(|surface| (surface.gpu, surface.bytes));
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
    let surfaces = helio_transform_surfaces(dispatch);
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

/// The compiled artifact has a fixed sixteen-entry binding table. Unused mode
/// inputs alias an already-valid dispatch range; the kernel mode guarantees
/// that those BTIs are never accessed. This keeps the legacy flat path and the
/// native matrix output free from dummy allocations.
fn helio_transform_surfaces(
    dispatch: GpgpuHelioRetainedTransformDispatch,
) -> [GpgpuHelioBufferSlice; HELIO_TRANSFORM_BINDINGS] {
    let (camera, source_vertices, expanded_positions) =
        if dispatch.output == GpgpuHelioRetainedTransformOutput::ExpandedPositions {
            (dispatch.camera, dispatch.source_vertices, dispatch.expanded_positions)
        } else {
            (dispatch.transforms, dispatch.transforms, dispatch.instances)
        };
    let graph = dispatch.hierarchy;
    [
        dispatch.transforms,
        dispatch.draw_templates,
        dispatch.instances,
        dispatch.compacted_indices,
        dispatch.indirect_args,
        camera,
        source_vertices,
        expanded_positions,
        graph.map_or(dispatch.transforms, |value| value.nodes),
        graph.map_or(dispatch.transforms, |value| value.dynamic_bindings),
        graph.map_or(dispatch.instances, |value| value.local_affines),
        graph.map_or(dispatch.instances, |value| value.world_affines),
        graph.map_or(dispatch.compacted_indices, |value| value.dirty_local_nodes),
        graph.map_or(dispatch.compacted_indices, |value| value.dirty_world_nodes),
        graph.map_or(dispatch.compacted_indices, |value| value.dirty_rows),
        graph.map_or(dispatch.compacted_indices, |value| value.row_leaf_nodes),
    ]
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
    let pointers = helio_transform_surfaces(dispatch).map(|surface| surface.gpu);
    for (index, pointer) in pointers.into_iter().enumerate() {
        write_helio_transform_u64(payload, 48 + index * 8, pointer)?;
    }
    write_helio_transform_u32(payload, 176, dispatch.row_count)?;
    write_helio_transform_u32(payload, 180, dispatch.draw_count)?;
    write_helio_transform_u32(payload, 184, mode)?;
    if let Some(graph) = dispatch.hierarchy {
        write_helio_transform_u32(payload, 188, graph.node_count)?;
        write_helio_transform_u32(payload, 192, graph.dirty_local_count)?;
        write_helio_transform_u32(payload, 196, graph.dirty_world_count)?;
        write_helio_transform_u32(payload, 200, graph.dirty_row_count)?;
        write_helio_transform_u32(payload, 204, graph.max_depth)?;
    }
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
            camera: GpgpuHelioBufferSlice::new(0x0360_0000, GPGPU_HELIO_CAMERA_BYTES),
            source_vertices: GpgpuHelioBufferSlice::new(0x0370_0000, 3 * 24 * 24),
            expanded_positions: GpgpuHelioBufferSlice::new(0x0380_0000, 337 * 24 * 12),
            row_count: 337,
            draw_count: 12,
            output: GpgpuHelioRetainedTransformOutput::ExpandedPositions,
            hierarchy: None,
        }
    }

    #[test]
    fn generated_bindings_map_each_bti_to_the_expected_buffer() {
        let dispatch = dispatch();
        let mut storage = alloc::vec![0u8; GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES];
        write_helio_transform_interface_descriptor(
            &mut storage,
            HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.entry_offset,
        )
        .unwrap();
        write_helio_transform_surfaces(&mut storage, dispatch).unwrap();
        write_helio_transform_payload(
            &mut storage,
            HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES,
            dispatch,
            HELIO_TRANSFORM_MODE_PREPARE_EXPANDED,
        )
        .unwrap();

        let read_u32 =
            |offset: usize| u32::from_le_bytes(storage[offset..offset + 4].try_into().unwrap());
        let read_u64 =
            |offset: usize| u64::from_le_bytes(storage[offset..offset + 8].try_into().unwrap());
        let arg_indices = [0u16, 1, 2, 3, 4, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
        let surfaces = helio_transform_surfaces(dispatch);
        let bindings = HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.bindings;
        assert_eq!(bindings.len(), HELIO_TRANSFORM_BINDINGS);
        for (index, (arg_index, surface)) in arg_indices.into_iter().zip(surfaces).enumerate() {
            let bti = index as u16;
            assert_eq!(bindings[index].arg_index, arg_index);
            assert_eq!(bindings[index].bti, bti);
            let surface_offset =
                read_u32(HELIO_TRANSFORM_BINDING_TABLE_OFFSET_BYTES + usize::from(bti) * 4)
                    as usize;
            assert_eq!(read_u64(surface_offset + 8 * 4), surface.gpu);
        }
        assert_eq!(
            read_u32(HELIO_TRANSFORM_IDD_OFFSET_BYTES + 4 * 4),
            HELIO_TRANSFORM_BINDING_TABLE_OFFSET_BYTES as u32 | HELIO_TRANSFORM_BINDINGS as u32
        );
        for (index, surface) in surfaces.into_iter().enumerate() {
            assert_eq!(
                read_u64(HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES + 48 + index * 8),
                surface.gpu
            );
        }
        // Generated BufferOffset{arg1}; zero means the draw-template surface
        // and pointer share the same byte-zero origin.
        assert_eq!(read_u32(HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES + 208), 0);
    }

    #[test]
    fn encoder_emits_ordered_prepare_and_parallel_transform_walkers() {
        let mut storage = alloc::vec![0u8; GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES];
        let mut state =
            GpgpuHelioRetainedTransformStateBlob::new(&mut storage, 0x0100_0000).unwrap();
        let encoding = encode_helio_retained_transform_secondary(
            &mut state,
            artifact(),
            dispatch(),
            0x0084_0000,
        )
        .unwrap();
        assert_eq!(encoding.prepare_groups, 1);
        assert_eq!(encoding.local_groups, 0);
        assert_eq!(encoding.hierarchy_groups, 0);
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
        let id_load = commands
            .iter()
            .position(|command| *command == MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD)
            .unwrap();
        assert!(id_load >= 2);
        assert_eq!(commands[id_load - 2], MEDIA_STATE_FLUSH_CMD);
        assert_eq!(commands[id_load - 1], 0);
        let release_cmd = PIPE_CONTROL_CMD | HELIO_TRANSFORM_RELEASE_HEADER_BITS;
        let prepare_release = (walkers[0]..walkers[1])
            .find(|index| {
                commands[*index] == release_cmd && commands[*index + 1] == PIPE_CONTROL_FLUSH_BITS
            })
            .unwrap();
        let transform_release = (walkers[1]..commands.len() - 1)
            .find(|index| {
                commands[*index] == release_cmd && commands[*index + 1] == PIPE_CONTROL_FLUSH_BITS
            })
            .unwrap();
        assert!(prepare_release < walkers[1]);
        let handoff_select = (transform_release + 1..commands.len())
            .find(|index| commands[*index] == PIPELINE_SELECT_3D)
            .unwrap();
        let consumer_invalidate = (transform_release + 1..handoff_select)
            .find(|index| {
                commands[*index] == release_cmd
                    && commands[*index + 1] == PIPE_CONTROL_INVALIDATE_BITS
            })
            .unwrap();
        assert!(transform_release < consumer_invalidate);
        assert!(consumer_invalidate < handoff_select);
        assert_eq!(commands[handoff_select + 1], release_cmd);
        assert_eq!(commands[handoff_select + 2], HELIO_TRANSFORM_3D_CONSUMER_BITS);
        assert_eq!(
            commands[handoff_select + 2] & HELIO_TRANSFORM_VF_CACHE_INVALIDATE,
            HELIO_TRANSFORM_VF_CACHE_INVALIDATE
        );
        assert_eq!(commands[consumer_invalidate + 1] & HELIO_TRANSFORM_VF_CACHE_INVALIDATE, 0);
        assert_eq!(
            u32::from_le_bytes(
                state.bytes()[HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES + 184
                    ..HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES + 188]
                    .try_into()
                    .unwrap()
            ),
            HELIO_TRANSFORM_MODE_PREPARE_EXPANDED
        );
        assert_eq!(
            u32::from_le_bytes(
                state.bytes()[HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES + 184
                    ..HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES + 188]
                    .try_into()
                    .unwrap()
            ),
            HELIO_TRANSFORM_MODE_ROWS_EXPANDED
        );
    }

    #[test]
    fn hierarchy_encoder_exposes_exact_dirty_groups_and_barriers() {
        let mut dispatch = dispatch();
        dispatch.output = GpgpuHelioRetainedTransformOutput::InstanceMatrices;
        dispatch.camera = GpgpuHelioBufferSlice::unused();
        dispatch.source_vertices = GpgpuHelioBufferSlice::unused();
        dispatch.expanded_positions = GpgpuHelioBufferSlice::unused();
        dispatch.hierarchy = Some(GpgpuHelioRetainedHierarchyDispatch {
            nodes: GpgpuHelioBufferSlice::new(0x0390_0000, 400 * 16),
            dynamic_bindings: GpgpuHelioBufferSlice::new(0x03A0_0000, 400 * 4),
            local_affines: GpgpuHelioBufferSlice::new(0x03B0_0000, 400 * 48),
            world_affines: GpgpuHelioBufferSlice::new(0x03C0_0000, 400 * 48),
            dirty_local_nodes: GpgpuHelioBufferSlice::new(0x03D0_0000, 33 * 4),
            dirty_world_nodes: GpgpuHelioBufferSlice::new(0x03E0_0000, 257 * 4),
            dirty_rows: GpgpuHelioBufferSlice::new(0x03F0_0000, 65 * 4),
            row_leaf_nodes: GpgpuHelioBufferSlice::new(0x0400_0000, 337 * 4),
            node_count: 400,
            dirty_local_count: 33,
            dirty_world_count: 257,
            dirty_row_count: 65,
            max_depth: 8,
        });

        let mut storage = alloc::vec![0u8; GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES];
        let mut state =
            GpgpuHelioRetainedTransformStateBlob::new(&mut storage, 0x0100_0000).unwrap();
        let encoding = encode_helio_retained_transform_secondary(
            &mut state,
            artifact(),
            dispatch,
            0x0084_0000,
        )
        .unwrap();
        assert_eq!(encoding.prepare_groups, 1);
        assert_eq!(encoding.local_groups, 3);
        assert_eq!(encoding.hierarchy_groups, 17);
        assert_eq!(encoding.transform_groups, 5);

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
        assert_eq!(walkers.len(), 4);
        let release_cmd = PIPE_CONTROL_CMD | HELIO_TRANSFORM_RELEASE_HEADER_BITS;
        for pair in walkers.windows(2) {
            assert!((pair[0]..pair[1]).any(|index| {
                commands[index] == release_cmd && commands[index + 1] == PIPE_CONTROL_FLUSH_BITS
            }));
        }

        let read_mode = |payload: usize| {
            u32::from_le_bytes(
                state.bytes()[payload + 184..payload + 188]
                    .try_into()
                    .unwrap(),
            )
        };
        assert_eq!(
            read_mode(HELIO_TRANSFORM_PREPARE_PAYLOAD_OFFSET_BYTES),
            HELIO_TRANSFORM_MODE_PREPARE_NATIVE
        );
        assert_eq!(
            read_mode(HELIO_TRANSFORM_LOCAL_PAYLOAD_OFFSET_BYTES),
            HELIO_TRANSFORM_MODE_HIERARCHY_LOCAL
        );
        assert_eq!(
            read_mode(HELIO_TRANSFORM_RESOLVE_PAYLOAD_OFFSET_BYTES),
            HELIO_TRANSFORM_MODE_HIERARCHY_RESOLVE
        );
        assert_eq!(
            read_mode(HELIO_TRANSFORM_ROWS_PAYLOAD_OFFSET_BYTES),
            HELIO_TRANSFORM_MODE_HIERARCHY_ROWS_NATIVE
        );
    }
}
