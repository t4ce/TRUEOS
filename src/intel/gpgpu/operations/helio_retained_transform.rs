/// The simulation/runtime ABI is the single canonical byte contract consumed
/// by the retained-transform artifact. Keep the GPGPU names as boundary-local
/// aliases so this layer cannot silently drift to another stride or layout.
pub(crate) type GpgpuHelioRetainedTransform = trueos_helio_runtime::churn::GpuRetainedTransformSeed;
pub(crate) type GpgpuHelioRetainedDrawTemplate =
    trueos_helio_runtime::churn::GpuRetainedDrawTemplate;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuHelioBufferSlice {
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
}

impl GpgpuHelioBufferSlice {
    pub(crate) const fn new(gpu: u64, bytes: usize) -> Self {
        Self { gpu, bytes }
    }

    pub(crate) const fn unused() -> Self {
        Self { gpu: 0, bytes: 0 }
    }

    const fn covers(self, required: usize) -> bool {
        self.gpu != 0
            && self.gpu.is_multiple_of(core::mem::size_of::<u32>() as u64)
            && self.bytes >= required
            && self.gpu.checked_add(required as u64).is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuHelioRetainedTransformOutput {
    /// Emit Helio's persistent 208-byte instance matrices for the native
    /// storage-backed graphics path. Camera and expanded-vertex buffers are
    /// not touched by the compute pass in this mode.
    InstanceMatrices,
    /// Additionally project each retained mesh into the Float3 vertex stream
    /// consumed by the storage-free validation graphics path.
    ExpandedPositions,
}

/// Optional retained affine graph. Lists are compact GPU worklists produced by
/// generation propagation: camera-only changes never appear in them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuHelioRetainedHierarchyDispatch {
    pub(crate) nodes: GpgpuHelioBufferSlice,
    pub(crate) dynamic_bindings: GpgpuHelioBufferSlice,
    pub(crate) local_affines: GpgpuHelioBufferSlice,
    pub(crate) world_affines: GpgpuHelioBufferSlice,
    pub(crate) dirty_local_nodes: GpgpuHelioBufferSlice,
    pub(crate) dirty_world_nodes: GpgpuHelioBufferSlice,
    pub(crate) dirty_rows: GpgpuHelioBufferSlice,
    pub(crate) row_leaf_nodes: GpgpuHelioBufferSlice,
    pub(crate) node_count: u32,
    pub(crate) dirty_local_count: u32,
    pub(crate) dirty_world_count: u32,
    pub(crate) dirty_row_count: u32,
    pub(crate) max_depth: u32,
}

/// GPU-address-only operation contract. Every range must already share the
/// Render PPGTT with the command batch. No host pointer or readback is part of
/// this API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuHelioRetainedTransformDispatch {
    pub(crate) transforms: GpgpuHelioBufferSlice,
    pub(crate) draw_templates: GpgpuHelioBufferSlice,
    pub(crate) instances: GpgpuHelioBufferSlice,
    pub(crate) compacted_indices: GpgpuHelioBufferSlice,
    pub(crate) indirect_args: GpgpuHelioBufferSlice,
    pub(crate) camera: GpgpuHelioBufferSlice,
    pub(crate) source_vertices: GpgpuHelioBufferSlice,
    pub(crate) expanded_positions: GpgpuHelioBufferSlice,
    pub(crate) row_count: u32,
    pub(crate) draw_count: u32,
    pub(crate) output: GpgpuHelioRetainedTransformOutput,
    pub(crate) hierarchy: Option<GpgpuHelioRetainedHierarchyDispatch>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuHelioTransformError {
    Empty,
    TooManyRows,
    TooManyDraws,
    AddressOverlap,
    TransformBuffer,
    DrawTemplateBuffer,
    InstanceBuffer,
    CompactedBuffer,
    IndirectBuffer,
    CameraBuffer,
    SourceVertexBuffer,
    ExpandedPositionBuffer,
    TooManyHierarchyNodes,
    HierarchyDepth,
    HierarchyCount,
    HierarchyNodeBuffer,
    HierarchyDynamicBindingBuffer,
    HierarchyLocalAffineBuffer,
    HierarchyWorldAffineBuffer,
    HierarchyDirtyLocalBuffer,
    HierarchyDirtyWorldBuffer,
    HierarchyDirtyRowBuffer,
    HierarchyRowLeafBuffer,
    DrawTemplate,
    BatchBuffer,
    Artifact,
}

pub(crate) const GPGPU_HELIO_INSTANCE_BYTES: usize = 208;
pub(crate) const GPGPU_HELIO_COMPACTED_INDEX_BYTES: usize = 4;
pub(crate) const GPGPU_HELIO_INDIRECT_BYTES: usize = 20;
pub(crate) const GPGPU_HELIO_CAMERA_BYTES: usize =
    trueos_helio_runtime::churn::GpuCameraUniforms::BYTE_LEN;
pub(crate) const GPGPU_HELIO_SOURCE_VERTICES_PER_MESH: usize = 24;
pub(crate) const GPGPU_HELIO_SOURCE_VERTEX_BYTES: usize = 24;
pub(crate) const GPGPU_HELIO_INDICES_PER_MESH: u32 = 36;
pub(crate) const GPGPU_HELIO_EXPANDED_VERTICES_PER_ROW: usize = 24;
pub(crate) const GPGPU_HELIO_EXPANDED_POSITION_BYTES: usize = 12;
pub(crate) const GPGPU_HELIO_HIERARCHY_NODE_BYTES: usize = 16;
pub(crate) const GPGPU_HELIO_HIERARCHY_DYNAMIC_BINDING_BYTES: usize = 4;
pub(crate) const GPGPU_HELIO_AFFINE_BYTES: usize = 48;
pub(crate) const GPGPU_HELIO_HIERARCHY_INDEX_BYTES: usize = 4;
pub(crate) const GPGPU_HELIO_MAX_ROWS: u32 = 4_096;
pub(crate) const GPGPU_HELIO_MAX_DRAWS: u32 = 64;
pub(crate) const GPGPU_HELIO_MAX_HIERARCHY_NODES: u32 = GPGPU_HELIO_MAX_ROWS * 16;
pub(crate) const GPGPU_HELIO_MAX_HIERARCHY_DEPTH: u32 = 64;
pub(crate) const GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES: usize = 0x2000;
pub(crate) const GPGPU_HELIO_DIAGNOSTIC_SLOT_PROLOGUE: usize = 26;
pub(crate) const GPGPU_HELIO_DIAGNOSTIC_SLOT_PREPARE: usize = 27;
pub(crate) const GPGPU_HELIO_DIAGNOSTIC_SLOT_TRANSFORM: usize = 28;
pub(crate) const GPGPU_HELIO_DIAGNOSTIC_SLOT_3D_HANDOFF: usize = 29;
pub(crate) const GPGPU_HELIO_DIAGNOSTIC_PROLOGUE: u32 = 0x4845_4C10;
pub(crate) const GPGPU_HELIO_DIAGNOSTIC_PREPARE: u32 = 0x4845_4C11;
pub(crate) const GPGPU_HELIO_DIAGNOSTIC_TRANSFORM: u32 = 0x4845_4C12;
pub(crate) const GPGPU_HELIO_DIAGNOSTIC_3D_HANDOFF: u32 = 0x4845_4C13;

const _: () = {
    assert!(
        GPGPU_HELIO_MAX_ROWS as usize == trueos_helio_runtime::churn::MAX_RETAINED_TRANSFORM_ROWS
    );
    assert!(GPGPU_HELIO_MAX_ROWS <= GpgpuHelioRetainedTransform::MAX_COMPACT_SLOT);
    assert!(GPGPU_HELIO_CAMERA_BYTES == 368);
};

/// Authenticated native artifact mapping for a caller-owned Render PPGTT.
/// Render maps `phys..phys+mapped_bytes` at `gpu`; the encoder references that
/// address as its instruction base without creating another GPU context.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GpgpuHelioRetainedTransformArtifactMapping {
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
    pub(crate) mapped_bytes: usize,
    pub(crate) entry_offset: u64,
    upload: UploadedKernelArtifact,
}

pub(crate) fn prepare_helio_retained_transform_artifact()
-> Option<GpgpuHelioRetainedTransformArtifactMapping> {
    let upload = upload_helio_retained_transform_kernel()?;
    if upload.address_space != GpgpuArtifactAddressSpace::CallerPpgtt {
        return None;
    }
    Some(GpgpuHelioRetainedTransformArtifactMapping {
        gpu: upload.gpu,
        phys: upload.phys,
        bytes: upload.bytes,
        mapped_bytes: upload.mapped_bytes,
        entry_offset: HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.entry_offset,
        upload,
    })
}

/// Caller-owned dynamic-state/secondary-command storage. Its backing must be
/// mapped at `gpu` in the same Render PPGTT as all dispatch buffers.
pub(crate) struct GpgpuHelioRetainedTransformStateBlob<'a> {
    bytes: &'a mut [u8],
    pub(crate) gpu: u64,
}

impl<'a> GpgpuHelioRetainedTransformStateBlob<'a> {
    pub(crate) fn new(bytes: &'a mut [u8], gpu: u64) -> Result<Self, GpgpuHelioTransformError> {
        if bytes.len() < GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES
            || bytes.as_ptr().align_offset(core::mem::align_of::<u32>()) != 0
            || !gpu.is_multiple_of(4096)
        {
            return Err(GpgpuHelioTransformError::BatchBuffer);
        }
        Ok(Self { bytes, gpu })
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes[..GPGPU_HELIO_TRANSFORM_STATE_BLOB_BYTES]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuHelioRetainedTransformEncoding {
    pub(crate) command_dwords: usize,
    pub(crate) prepare_groups: u32,
    pub(crate) local_groups: u32,
    pub(crate) hierarchy_groups: u32,
    pub(crate) transform_groups: u32,
}

const _: () = {
    assert!(core::mem::size_of::<GpgpuHelioRetainedTransform>() == 64);
    assert!(core::mem::align_of::<GpgpuHelioRetainedTransform>() == 4);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedTransform, translation) == 0);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedTransform, scale) == 12);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedTransform, rotation) == 24);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedTransform, local_radius) == 40);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedTransform, previous_translation) == 44);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedTransform, draw_group) == 56);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedTransform, flags) == 60);
    assert!(core::mem::size_of::<GpgpuHelioRetainedDrawTemplate>() == 24);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedDrawTemplate, index_count) == 0);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedDrawTemplate, first_index) == 4);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedDrawTemplate, base_vertex) == 8);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedDrawTemplate, first_instance) == 12);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedDrawTemplate, capacity) == 16);
    assert!(core::mem::offset_of!(GpgpuHelioRetainedDrawTemplate, packed_mesh_material) == 20);
    assert!(GPGPU_HELIO_INSTANCE_BYTES == trueos_helio_runtime::churn::GpuInstanceData::BYTE_LEN);
    assert!(GPGPU_HELIO_INDIRECT_BYTES == trueos_helio_runtime::DrawIndexedIndirectArgs::BYTE_LEN);
    assert!(
        GPGPU_HELIO_HIERARCHY_NODE_BYTES
            == core::mem::size_of::<trueos_helio_runtime::retained_transform::RetainedTransformNode>(
            )
    );
    assert!(
        core::mem::offset_of!(
            trueos_helio_runtime::retained_transform::RetainedTransformNode,
            parent
        ) == 0
    );
    assert!(
        core::mem::offset_of!(
            trueos_helio_runtime::retained_transform::RetainedTransformNode,
            level
        ) == 4
    );
    assert!(
        core::mem::offset_of!(
            trueos_helio_runtime::retained_transform::RetainedTransformNode,
            local_generation
        ) == 8
    );
    assert!(
        core::mem::offset_of!(
            trueos_helio_runtime::retained_transform::RetainedTransformNode,
            world_generation
        ) == 12
    );
    assert!(
        GPGPU_HELIO_AFFINE_BYTES
            == core::mem::size_of::<trueos_helio_runtime::retained_transform::Affine3x4>()
    );
    assert!(core::mem::align_of::<trueos_helio_runtime::retained_transform::Affine3x4>() == 16);
    assert!(
        GPGPU_HELIO_MAX_HIERARCHY_DEPTH
            == trueos_helio_runtime::retained_transform::MAX_RETAINED_TRANSFORM_DEPTH
    );
};

impl GpgpuHelioRetainedTransformDispatch {
    pub(crate) fn validate(self) -> Result<(), GpgpuHelioTransformError> {
        use GpgpuHelioTransformError as Error;

        if self.row_count == 0 || self.draw_count == 0 {
            return Err(Error::Empty);
        }
        if self.row_count > GPGPU_HELIO_MAX_ROWS {
            return Err(Error::TooManyRows);
        }
        if self.draw_count > GPGPU_HELIO_MAX_DRAWS {
            return Err(Error::TooManyDraws);
        }
        let rows = self.row_count as usize;
        let draws = self.draw_count as usize;
        let transform_bytes = rows
            .checked_mul(GpgpuHelioRetainedTransform::BYTE_LEN)
            .ok_or(Error::TransformBuffer)?;
        let template_bytes = draws
            .checked_mul(GpgpuHelioRetainedDrawTemplate::BYTE_LEN)
            .ok_or(Error::DrawTemplateBuffer)?;
        let instance_bytes = rows
            .checked_mul(GPGPU_HELIO_INSTANCE_BYTES)
            .ok_or(Error::InstanceBuffer)?;
        let compacted_bytes = rows
            .checked_mul(GPGPU_HELIO_COMPACTED_INDEX_BYTES)
            .ok_or(Error::CompactedBuffer)?;
        let indirect_bytes = draws
            .checked_mul(GPGPU_HELIO_INDIRECT_BYTES)
            .ok_or(Error::IndirectBuffer)?;
        let source_vertex_bytes = trueos_helio_runtime::churn::SHAPE_COUNT
            .checked_mul(GPGPU_HELIO_SOURCE_VERTICES_PER_MESH)
            .and_then(|vertices| vertices.checked_mul(GPGPU_HELIO_SOURCE_VERTEX_BYTES))
            .ok_or(Error::SourceVertexBuffer)?;
        let expanded_position_bytes = rows
            .checked_mul(GPGPU_HELIO_EXPANDED_VERTICES_PER_ROW)
            .and_then(|vertices| vertices.checked_mul(GPGPU_HELIO_EXPANDED_POSITION_BYTES))
            .ok_or(Error::ExpandedPositionBuffer)?;
        if !self.transforms.covers(transform_bytes) {
            return Err(Error::TransformBuffer);
        }
        if !self.draw_templates.covers(template_bytes) {
            return Err(Error::DrawTemplateBuffer);
        }
        if !self.instances.covers(instance_bytes) {
            return Err(Error::InstanceBuffer);
        }
        if !self.compacted_indices.covers(compacted_bytes) {
            return Err(Error::CompactedBuffer);
        }
        if !self.indirect_args.covers(indirect_bytes) {
            return Err(Error::IndirectBuffer);
        }

        let mut ranges: [Option<(u64, usize)>; 16] = [None; 16];
        let mut range_count = 0usize;
        for range in [
            (self.transforms.gpu, transform_bytes),
            (self.draw_templates.gpu, template_bytes),
            (self.instances.gpu, instance_bytes),
            (self.compacted_indices.gpu, compacted_bytes),
            (self.indirect_args.gpu, indirect_bytes),
        ] {
            ranges[range_count] = Some(range);
            range_count += 1;
        }

        if self.output == GpgpuHelioRetainedTransformOutput::ExpandedPositions {
            if !self.camera.covers(GPGPU_HELIO_CAMERA_BYTES) {
                return Err(Error::CameraBuffer);
            }
            if !self.source_vertices.covers(source_vertex_bytes) {
                return Err(Error::SourceVertexBuffer);
            }
            if !self.expanded_positions.covers(expanded_position_bytes) {
                return Err(Error::ExpandedPositionBuffer);
            }
            for range in [
                (self.camera.gpu, GPGPU_HELIO_CAMERA_BYTES),
                (self.source_vertices.gpu, source_vertex_bytes),
                (self.expanded_positions.gpu, expanded_position_bytes),
            ] {
                ranges[range_count] = Some(range);
                range_count += 1;
            }
        }

        if let Some(graph) = self.hierarchy {
            if graph.node_count == 0 || graph.node_count > GPGPU_HELIO_MAX_HIERARCHY_NODES {
                return Err(Error::TooManyHierarchyNodes);
            }
            if graph.max_depth == 0 || graph.max_depth > GPGPU_HELIO_MAX_HIERARCHY_DEPTH {
                return Err(Error::HierarchyDepth);
            }
            if graph.dirty_local_count > graph.node_count
                || graph.dirty_world_count > graph.node_count
                || graph.dirty_row_count > self.row_count
            {
                return Err(Error::HierarchyCount);
            }
            let nodes = graph.node_count as usize;
            let node_bytes = nodes
                .checked_mul(GPGPU_HELIO_HIERARCHY_NODE_BYTES)
                .ok_or(Error::HierarchyNodeBuffer)?;
            let binding_bytes = nodes
                .checked_mul(GPGPU_HELIO_HIERARCHY_DYNAMIC_BINDING_BYTES)
                .ok_or(Error::HierarchyDynamicBindingBuffer)?;
            let affine_bytes = nodes
                .checked_mul(GPGPU_HELIO_AFFINE_BYTES)
                .ok_or(Error::HierarchyLocalAffineBuffer)?;
            let dirty_local_bytes =
                graph.dirty_local_count as usize * GPGPU_HELIO_HIERARCHY_INDEX_BYTES;
            let dirty_world_bytes =
                graph.dirty_world_count as usize * GPGPU_HELIO_HIERARCHY_INDEX_BYTES;
            let dirty_row_bytes =
                graph.dirty_row_count as usize * GPGPU_HELIO_HIERARCHY_INDEX_BYTES;
            let row_leaf_bytes = rows * GPGPU_HELIO_HIERARCHY_INDEX_BYTES;
            for (slice, required, error) in [
                (graph.nodes, node_bytes, Error::HierarchyNodeBuffer),
                (graph.dynamic_bindings, binding_bytes, Error::HierarchyDynamicBindingBuffer),
                (graph.local_affines, affine_bytes, Error::HierarchyLocalAffineBuffer),
                (graph.world_affines, affine_bytes, Error::HierarchyWorldAffineBuffer),
                (graph.dirty_local_nodes, dirty_local_bytes, Error::HierarchyDirtyLocalBuffer),
                (graph.dirty_world_nodes, dirty_world_bytes, Error::HierarchyDirtyWorldBuffer),
                (graph.dirty_rows, dirty_row_bytes, Error::HierarchyDirtyRowBuffer),
                (graph.row_leaf_nodes, row_leaf_bytes, Error::HierarchyRowLeafBuffer),
            ] {
                if required != 0 && !slice.covers(required) {
                    return Err(error);
                }
                if required != 0 {
                    ranges[range_count] = Some((slice.gpu, required));
                    range_count += 1;
                }
            }
        }

        for left in 0..range_count {
            for right in left + 1..range_count {
                if ranges_overlap(ranges[left].unwrap(), ranges[right].unwrap()) {
                    return Err(Error::AddressOverlap);
                }
            }
        }
        Ok(())
    }

    /// Validate fixed compacted slices before the immutable template buffer is
    /// published. The GPGPU dispatch itself receives only GPU addresses.
    pub(crate) fn validate_templates(
        self,
        templates: &[GpgpuHelioRetainedDrawTemplate],
    ) -> Result<(), GpgpuHelioTransformError> {
        use GpgpuHelioTransformError as Error;

        self.validate()?;
        if templates.len() != self.draw_count as usize {
            return Err(Error::DrawTemplate);
        }
        for template in templates {
            if template.index_count != GPGPU_HELIO_INDICES_PER_MESH
                || template.mesh_id() as usize >= trueos_helio_runtime::churn::SHAPE_COUNT
            {
                return Err(Error::DrawTemplate);
            }
            let end = template
                .first_instance
                .checked_add(template.capacity)
                .ok_or(Error::DrawTemplate)?;
            if end > self.row_count {
                return Err(Error::DrawTemplate);
            }
        }
        for left in 0..templates.len() {
            if templates[left].capacity == 0 {
                continue;
            }
            let left_range = templates[left].first_instance
                ..templates[left].first_instance + templates[left].capacity;
            for right in left + 1..templates.len() {
                if templates[right].capacity == 0 {
                    continue;
                }
                let right_range = templates[right].first_instance
                    ..templates[right].first_instance + templates[right].capacity;
                if left_range.start < right_range.end && right_range.start < left_range.end {
                    return Err(Error::DrawTemplate);
                }
            }
        }
        Ok(())
    }
}

fn ranges_overlap(left: (u64, usize), right: (u64, usize)) -> bool {
    let left_end = left.0.saturating_add(left.1 as u64);
    let right_end = right.0.saturating_add(right.1 as u64);
    left.0 < right_end && right.0 < left_end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatch() -> GpgpuHelioRetainedTransformDispatch {
        GpgpuHelioRetainedTransformDispatch {
            transforms: GpgpuHelioBufferSlice::new(0x1000, 2 * 64),
            draw_templates: GpgpuHelioBufferSlice::new(0x2000, 2 * 24),
            instances: GpgpuHelioBufferSlice::new(0x3000, 2 * 208),
            compacted_indices: GpgpuHelioBufferSlice::new(0x4000, 2 * 4),
            indirect_args: GpgpuHelioBufferSlice::new(0x5000, 2 * 20),
            camera: GpgpuHelioBufferSlice::new(0x6000, GPGPU_HELIO_CAMERA_BYTES),
            source_vertices: GpgpuHelioBufferSlice::new(0x7000, 3 * 24 * 24),
            expanded_positions: GpgpuHelioBufferSlice::new(0x8000, 2 * 24 * 12),
            row_count: 2,
            draw_count: 2,
            output: GpgpuHelioRetainedTransformOutput::ExpandedPositions,
            hierarchy: None,
        }
    }

    #[test]
    fn retained_transform_abi_matches_helio_draw_storage() {
        assert_eq!(GpgpuHelioRetainedTransform::BYTE_LEN, 64);
        assert_eq!(GpgpuHelioRetainedDrawTemplate::BYTE_LEN, 24);
        assert_eq!(GPGPU_HELIO_INSTANCE_BYTES, 208);
        assert_eq!(GPGPU_HELIO_INDIRECT_BYTES, 20);
        assert_eq!(dispatch().validate(), Ok(()));
    }

    #[test]
    fn retained_transform_artifact_is_strictly_admitted_for_baked_adls_revision() {
        assert_eq!(
            admit_kernel_artifact_bytes(
                HELIO_RETAINED_TRANSFORM_ADLS_ARTIFACT,
                0x4680,
                0x0C,
                HELIO_RETAINED_TRANSFORM_ADLS_BIN,
            ),
            Ok(HELIO_RETAINED_TRANSFORM_ADLS_BIN_SHA256)
        );
        assert_eq!(
            kernel_source_path(HELIO_RETAINED_TRANSFORM_KERNEL_NAME),
            Some(HELIO_RETAINED_TRANSFORM_SOURCE_PATH)
        );
    }

    #[test]
    fn retained_draw_slices_are_fixed_disjoint_and_bounded() {
        let templates = [
            GpgpuHelioRetainedDrawTemplate {
                index_count: 36,
                first_instance: 0,
                capacity: 1,
                ..GpgpuHelioRetainedDrawTemplate::default()
            },
            GpgpuHelioRetainedDrawTemplate {
                index_count: 36,
                first_instance: 1,
                capacity: 1,
                ..GpgpuHelioRetainedDrawTemplate::default()
            },
        ];
        assert_eq!(dispatch().validate_templates(&templates), Ok(()));
    }

    #[test]
    fn retained_draw_slices_reject_overlap_before_gpu_submission() {
        let templates = [
            GpgpuHelioRetainedDrawTemplate {
                index_count: 36,
                first_instance: 0,
                capacity: 2,
                ..GpgpuHelioRetainedDrawTemplate::default()
            },
            GpgpuHelioRetainedDrawTemplate {
                index_count: 36,
                first_instance: 1,
                capacity: 1,
                ..GpgpuHelioRetainedDrawTemplate::default()
            },
        ];
        assert_eq!(
            dispatch().validate_templates(&templates),
            Err(GpgpuHelioTransformError::DrawTemplate)
        );
    }

    #[test]
    fn native_matrix_output_needs_no_expanded_vertex_allocations() {
        let mut dispatch = dispatch();
        dispatch.output = GpgpuHelioRetainedTransformOutput::InstanceMatrices;
        dispatch.camera = GpgpuHelioBufferSlice::unused();
        dispatch.source_vertices = GpgpuHelioBufferSlice::unused();
        dispatch.expanded_positions = GpgpuHelioBufferSlice::unused();
        assert_eq!(dispatch.validate(), Ok(()));
    }

    #[test]
    fn hierarchy_contract_counts_exact_dirty_worklists() {
        let mut dispatch = dispatch();
        dispatch.output = GpgpuHelioRetainedTransformOutput::InstanceMatrices;
        dispatch.camera = GpgpuHelioBufferSlice::unused();
        dispatch.source_vertices = GpgpuHelioBufferSlice::unused();
        dispatch.expanded_positions = GpgpuHelioBufferSlice::unused();
        dispatch.hierarchy = Some(GpgpuHelioRetainedHierarchyDispatch {
            nodes: GpgpuHelioBufferSlice::new(0x9000, 3 * 16),
            dynamic_bindings: GpgpuHelioBufferSlice::new(0xA000, 3 * 4),
            local_affines: GpgpuHelioBufferSlice::new(0xB000, 3 * 48),
            world_affines: GpgpuHelioBufferSlice::new(0xC000, 3 * 48),
            dirty_local_nodes: GpgpuHelioBufferSlice::new(0xD000, 2 * 4),
            dirty_world_nodes: GpgpuHelioBufferSlice::new(0xE000, 3 * 4),
            dirty_rows: GpgpuHelioBufferSlice::new(0xF000, 2 * 4),
            row_leaf_nodes: GpgpuHelioBufferSlice::new(0x1_0000, 2 * 4),
            node_count: 3,
            dirty_local_count: 2,
            dirty_world_count: 3,
            dirty_row_count: 2,
            max_depth: 2,
        });
        assert_eq!(dispatch.validate(), Ok(()));
        dispatch.hierarchy.as_mut().unwrap().dirty_world_count = 4;
        assert_eq!(dispatch.validate(), Err(GpgpuHelioTransformError::HierarchyCount));
    }
}
