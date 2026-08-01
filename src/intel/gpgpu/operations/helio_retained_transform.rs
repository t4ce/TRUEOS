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

    const fn covers(self, required: usize) -> bool {
        self.gpu != 0
            && self.gpu.is_multiple_of(core::mem::size_of::<u32>() as u64)
            && self.bytes >= required
            && self.gpu.checked_add(required as u64).is_some()
    }
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
    pub(crate) row_count: u32,
    pub(crate) draw_count: u32,
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
    DrawTemplate,
    BatchBuffer,
    Artifact,
}

pub(crate) const GPGPU_HELIO_INSTANCE_BYTES: usize = 208;
pub(crate) const GPGPU_HELIO_COMPACTED_INDEX_BYTES: usize = 4;
pub(crate) const GPGPU_HELIO_INDIRECT_BYTES: usize = 20;
pub(crate) const GPGPU_HELIO_MAX_ROWS: u32 = 4_096;
pub(crate) const GPGPU_HELIO_MAX_DRAWS: u32 = 64;
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

        let ranges = [
            (self.transforms.gpu, transform_bytes),
            (self.draw_templates.gpu, template_bytes),
            (self.instances.gpu, instance_bytes),
            (self.compacted_indices.gpu, compacted_bytes),
            (self.indirect_args.gpu, indirect_bytes),
        ];
        for left in 0..ranges.len() {
            for right in left + 1..ranges.len() {
                if ranges_overlap(ranges[left], ranges[right]) {
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
            if template.index_count == 0 {
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
            row_count: 2,
            draw_count: 2,
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
                index_count: 6,
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
                index_count: 6,
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
}
